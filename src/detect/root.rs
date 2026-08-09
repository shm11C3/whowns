use std::env;
use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};

use crate::model::OwnerId;

#[derive(Clone, Copy)]
enum RootSource {
    Environment(&'static str),
    Default,
    Inferred,
}

#[derive(Clone)]
struct ResolvedRoot {
    path: PathBuf,
    canonical_path: PathBuf,
    source: RootSource,
}

impl ResolvedRoot {
    fn new(path: PathBuf, source: RootSource) -> Self {
        let canonical_path = fs::canonicalize(&path).unwrap_or_else(|_| path.clone());
        Self {
            path,
            canonical_path,
            source,
        }
    }

    fn relative_path(&self, path: &Path) -> Option<PathBuf> {
        path.strip_prefix(&self.path)
            .ok()
            .filter(|relative| !relative.as_os_str().is_empty())
            .map(Path::to_path_buf)
            .or_else(|| {
                let canonical_path = fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
                canonical_path
                    .strip_prefix(&self.canonical_path)
                    .ok()
                    .filter(|relative| !relative.as_os_str().is_empty())
                    .map(Path::to_path_buf)
            })
    }

    fn evidence(&self, owner: OwnerId) -> String {
        match self.source {
            RootSource::Environment(variable) => format!(
                "path is inside the {} root from `${variable}` ({})",
                owner.display_name(),
                self.path.display()
            ),
            RootSource::Default => format!(
                "path is inside the default {} root ({})",
                owner.display_name(),
                self.path.display()
            ),
            RootSource::Inferred if owner == OwnerId::Fnm => format!(
                "path is inside the {} multishell root inferred from its layout ({})",
                owner.display_name(),
                self.path.display()
            ),
            RootSource::Inferred => format!(
                "path is inside the {} root inferred from its layout ({})",
                owner.display_name(),
                self.path.display()
            ),
        }
    }

    fn cargo_evidence(&self) -> String {
        match self.source {
            RootSource::Environment(variable) => format!(
                "path is inside Cargo home from `${variable}` ({})",
                self.path.display()
            ),
            RootSource::Default => {
                format!(
                    "path is inside the default Cargo home ({})",
                    self.path.display()
                )
            }
            RootSource::Inferred => unreachable!("Cargo home is never inferred from a path"),
        }
    }
}

struct OwnerRoot {
    owner: OwnerId,
    root: ResolvedRoot,
}

pub(crate) struct ManagerPath {
    pub(crate) owner: OwnerId,
    pub(crate) relative_path: PathBuf,
    root: ResolvedRoot,
}

impl ManagerPath {
    pub(crate) fn evidence(&self) -> String {
        self.root.evidence(self.owner)
    }
}

pub(crate) struct CargoPath {
    pub(crate) relative_path: PathBuf,
    root: ResolvedRoot,
}

impl CargoPath {
    pub(crate) fn evidence(&self) -> String {
        self.root.cargo_evidence()
    }
}

pub(crate) struct ManagerRoots {
    managers: Vec<OwnerRoot>,
    cargo: Option<ResolvedRoot>,
}

impl ManagerRoots {
    pub(crate) fn from_environment() -> Self {
        Self::resolve(|variable| env::var_os(variable))
    }

    #[cfg(test)]
    pub(crate) fn for_home(home: &Path) -> Self {
        Self::resolve(|variable| (variable == "HOME").then(|| home.as_os_str().to_os_string()))
    }

    fn resolve(mut get: impl FnMut(&str) -> Option<OsString>) -> Self {
        let home = non_empty(get("HOME")).map(PathBuf::from);
        let xdg_config = non_empty(get("XDG_CONFIG_HOME")).map(PathBuf::from);
        let xdg_data = non_empty(get("XDG_DATA_HOME")).map(PathBuf::from);
        let mut managers = Vec::new();

        push_environment_or_default(
            &mut managers,
            OwnerId::Nvm,
            "NVM_DIR",
            non_empty(get("NVM_DIR")),
            xdg_config
                .as_ref()
                .map(|root| (root.join("nvm"), RootSource::Environment("XDG_CONFIG_HOME")))
                .or_else(|| default_root(&home, ".nvm")),
        );
        let fnm_default = xdg_data
            .as_ref()
            .map(|root| (root.join("fnm"), RootSource::Environment("XDG_DATA_HOME")))
            .or_else(|| default_root(&home, ".local/share/fnm"));
        push_environment_or_default(
            &mut managers,
            OwnerId::Fnm,
            "FNM_DIR",
            non_empty(get("FNM_DIR")),
            fnm_default,
        );
        push_environment_or_default(
            &mut managers,
            OwnerId::Volta,
            "VOLTA_HOME",
            non_empty(get("VOLTA_HOME")),
            default_root(&home, ".volta"),
        );
        push_environment_or_default(
            &mut managers,
            OwnerId::Mise,
            "MISE_DATA_DIR",
            non_empty(get("MISE_DATA_DIR")),
            xdg_data
                .as_ref()
                .map(|root| (root.join("mise"), RootSource::Environment("XDG_DATA_HOME")))
                .or_else(|| default_root(&home, ".local/share/mise")),
        );
        if let Some((path, source)) = default_root(&home, ".mise") {
            managers.push(OwnerRoot {
                owner: OwnerId::Mise,
                root: ResolvedRoot::new(path, source),
            });
        }
        push_environment_or_default(
            &mut managers,
            OwnerId::Asdf,
            "ASDF_DATA_DIR",
            non_empty(get("ASDF_DATA_DIR")),
            default_root(&home, ".asdf"),
        );
        push_environment_or_default(
            &mut managers,
            OwnerId::Pyenv,
            "PYENV_ROOT",
            non_empty(get("PYENV_ROOT")),
            default_root(&home, ".pyenv"),
        );
        push_environment_or_default(
            &mut managers,
            OwnerId::Rbenv,
            "RBENV_ROOT",
            non_empty(get("RBENV_ROOT")),
            default_root(&home, ".rbenv"),
        );
        push_environment_or_default(
            &mut managers,
            OwnerId::Sdkman,
            "SDKMAN_DIR",
            non_empty(get("SDKMAN_DIR")),
            default_root(&home, ".sdkman"),
        );
        let uv_default = xdg_data
            .as_ref()
            .map(|root| {
                (
                    root.join("uv/python"),
                    RootSource::Environment("XDG_DATA_HOME"),
                )
            })
            .or_else(|| default_root(&home, ".local/share/uv/python"));
        push_environment_or_default(
            &mut managers,
            OwnerId::Uv,
            "UV_PYTHON_INSTALL_DIR",
            non_empty(get("UV_PYTHON_INSTALL_DIR")),
            uv_default,
        );

        let cargo = non_empty(get("CARGO_HOME"))
            .map(|path| {
                ResolvedRoot::new(PathBuf::from(path), RootSource::Environment("CARGO_HOME"))
            })
            .or_else(|| default_root(&home, ".cargo").map(resolved));

        Self { managers, cargo }
    }

    pub(crate) fn manager_path(&self, paths: &[PathBuf]) -> Option<ManagerPath> {
        self.managers
            .iter()
            .find_map(|owner_root| {
                paths.iter().find_map(|path| {
                    owner_root
                        .root
                        .relative_path(path)
                        .map(|relative_path| ManagerPath {
                            owner: owner_root.owner,
                            relative_path,
                            root: owner_root.root.clone(),
                        })
                })
            })
            .or_else(|| inferred_manager_root(paths, OwnerId::Fnm, "fnm_multishells"))
            .or_else(|| inferred_manager_root(paths, OwnerId::Mise, ".mise"))
    }

    pub(crate) fn manager_path_for(
        &self,
        owner: OwnerId,
        paths: &[PathBuf],
    ) -> Option<ManagerPath> {
        self.managers
            .iter()
            .filter(|owner_root| owner_root.owner == owner)
            .find_map(|owner_root| {
                paths.iter().find_map(|path| {
                    owner_root
                        .root
                        .relative_path(path)
                        .map(|relative_path| ManagerPath {
                            owner,
                            relative_path,
                            root: owner_root.root.clone(),
                        })
                })
            })
            .or_else(|| match owner {
                OwnerId::Fnm => inferred_manager_root(paths, owner, "fnm_multishells"),
                OwnerId::Mise => inferred_manager_root(paths, owner, ".mise"),
                _ => None,
            })
    }

    pub(crate) fn cargo_path(&self, paths: &[PathBuf]) -> Option<CargoPath> {
        let root = self.cargo.as_ref()?;
        paths.iter().find_map(|path| {
            root.relative_path(path).map(|relative_path| CargoPath {
                relative_path,
                root: root.clone(),
            })
        })
    }

    pub(crate) fn root_for(&self, owner: OwnerId, runtime_path: &Path) -> Option<PathBuf> {
        self.manager_path_for(owner, &[runtime_path.to_path_buf()])
            .map(|manager_path| manager_path.root.path)
    }
}

fn non_empty(value: Option<OsString>) -> Option<OsString> {
    value.filter(|value| !value.is_empty())
}

fn default_root(home: &Option<PathBuf>, relative: &str) -> Option<(PathBuf, RootSource)> {
    home.as_ref()
        .map(|home| (home.join(relative), RootSource::Default))
}

fn resolved((path, source): (PathBuf, RootSource)) -> ResolvedRoot {
    ResolvedRoot::new(path, source)
}

fn push_environment_or_default(
    roots: &mut Vec<OwnerRoot>,
    owner: OwnerId,
    variable: &'static str,
    environment: Option<OsString>,
    default: Option<(PathBuf, RootSource)>,
) {
    let root = environment
        .map(|path| ResolvedRoot::new(PathBuf::from(path), RootSource::Environment(variable)))
        .or_else(|| default.map(resolved));
    if let Some(root) = root {
        roots.push(OwnerRoot { owner, root });
    }
}

fn inferred_manager_root(
    paths: &[PathBuf],
    owner: OwnerId,
    directory_name: &str,
) -> Option<ManagerPath> {
    paths.iter().find_map(|path| {
        path.ancestors().find_map(|ancestor| {
            (ancestor
                .file_name()
                .is_some_and(|name| name == directory_name))
            .then(|| {
                let root = ResolvedRoot::new(ancestor.to_path_buf(), RootSource::Inferred);
                let relative_path = path.strip_prefix(ancestor).ok()?.to_path_buf();
                (!relative_path.as_os_str().is_empty()).then_some(ManagerPath {
                    owner,
                    relative_path,
                    root,
                })
            })
            .flatten()
        })
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolves_an_environment_root_without_requiring_it_to_exist() {
        let roots = ManagerRoots::resolve(|variable| match variable {
            "HOME" => Some(OsString::from("/home/me")),
            "PYENV_ROOT" => Some(OsString::from("/not-created/custom-pyenv")),
            _ => None,
        });

        let path = roots
            .manager_path(&[PathBuf::from(
                "/not-created/custom-pyenv/versions/3.12.4/bin/python3",
            )])
            .unwrap();

        assert_eq!(path.owner, OwnerId::Pyenv);
        assert_eq!(path.relative_path, Path::new("versions/3.12.4/bin/python3"));
        assert!(path.evidence().contains("$PYENV_ROOT"));
    }

    #[test]
    fn preserves_fnm_multishell_detection() {
        let roots = ManagerRoots::for_home(Path::new("/home/me"));
        let path = roots
            .manager_path(&[PathBuf::from(
                "/run/user/1000/fnm_multishells/session/bin/node",
            )])
            .unwrap();

        assert_eq!(path.owner, OwnerId::Fnm);
        assert!(path.evidence().contains("multishell root inferred"));
    }

    #[test]
    fn preserves_project_local_mise_detection() {
        let roots = ManagerRoots::for_home(Path::new("/home/me"));
        let path = roots
            .manager_path(&[PathBuf::from(
                "/work/project/.mise/installs/node/22.3.0/bin/node",
            )])
            .unwrap();

        assert_eq!(path.owner, OwnerId::Mise);
        assert_eq!(
            path.relative_path,
            Path::new("installs/node/22.3.0/bin/node")
        );
        assert!(path.evidence().contains("root inferred from its layout"));
    }
}
