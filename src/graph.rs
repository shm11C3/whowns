use std::env;
use std::fs;
use std::path::{Path, PathBuf};

use crate::detect;
use crate::model::{OwnerKind, OwnershipGraph, OwnershipNode, Resolution, ResolutionStatus};
use crate::scan::{self, ResolvedExecutable};

pub fn inspect(command: &str) -> OwnershipGraph {
    from_resolutions(command, scan::find_executables(command))
}

pub fn all(show_missing: bool) -> Vec<OwnershipGraph> {
    let mut graphs: Vec<_> = scan::KNOWN_RUNTIMES
        .iter()
        .map(|command| inspect(command))
        .collect();
    if !show_missing {
        graphs.retain(|graph| !graph.resolutions.is_empty());
    }
    graphs
}

fn from_resolutions(command: &str, resolved: Vec<ResolvedExecutable>) -> OwnershipGraph {
    let resolutions = resolved
        .into_iter()
        .map(|resolved| {
            let mut primary = detect::detect(command, &resolved.path, &resolved.real_path);
            detect::enrich_with_manager_query(&mut primary, command, &resolved.real_path);
            let upstream = upstream_owner(&primary, &resolved.path);
            let mut owners = vec![primary];
            if let Some(upstream) = upstream {
                owners.push(upstream);
            }
            Resolution {
                path: resolved.path,
                real_path: resolved.real_path,
                status: if resolved.active {
                    ResolutionStatus::Active
                } else {
                    ResolutionStatus::Shadowed
                },
                owners,
            }
        })
        .collect();
    OwnershipGraph {
        command: command.into(),
        resolutions,
    }
}

fn upstream_owner(primary: &OwnershipNode, runtime_path: &Path) -> Option<OwnershipNode> {
    if !matches!(
        primary.kind,
        OwnerKind::VersionManager | OwnerKind::ToolInstaller
    ) {
        return None;
    }

    if primary.name == "nvm" {
        return Some(source_from_root("nvm", nvm_root(runtime_path)));
    }
    if primary.name == "SDKMAN!" {
        return Some(source_from_root("SDKMAN!", sdkman_root(runtime_path)));
    }

    let manager_command = detect::manager_executable(&primary.name)?;
    let manager_resolution = scan::find_executables(manager_command)
        .into_iter()
        .find(|resolution| resolution.active);
    let Some(manager_resolution) = manager_resolution else {
        return Some(detect::unconfirmed_manager_source(&primary.name, None));
    };
    let source = detect::detect(
        manager_command,
        &manager_resolution.path,
        &manager_resolution.real_path,
    );
    if source.name == primary.name {
        Some(detect::unconfirmed_manager_source(
            &primary.name,
            Some(&manager_resolution.path),
        ))
    } else {
        Some(source)
    }
}

fn source_from_root(manager: &str, root: Option<PathBuf>) -> OwnershipNode {
    let Some(root) = root else {
        return detect::unconfirmed_manager_source(manager, None);
    };
    let real_root = fs::canonicalize(&root).unwrap_or_else(|_| root.clone());
    let source = detect::detect(manager, &root, &real_root);
    if source.name == manager {
        detect::unconfirmed_manager_source(manager, Some(&root))
    } else {
        source
    }
}

fn nvm_root(runtime_path: &Path) -> Option<PathBuf> {
    root_before_marker(runtime_path, "/versions/node/")
        .or_else(|| env::var_os("NVM_DIR").map(PathBuf::from))
}

fn sdkman_root(runtime_path: &Path) -> Option<PathBuf> {
    root_before_marker(runtime_path, "/candidates/")
        .or_else(|| env::var_os("SDKMAN_DIR").map(PathBuf::from))
}

fn root_before_marker(path: &Path, marker: &str) -> Option<PathBuf> {
    let path = path.to_string_lossy();
    let (root, _) = path.split_once(marker)?;
    Some(PathBuf::from(root))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(unix)]
    use std::os::unix::fs::symlink;

    #[test]
    fn one_graph_holds_active_and_shadowed_resolutions() {
        let graph = from_resolutions(
            "node",
            vec![
                ResolvedExecutable {
                    path: PathBuf::from("/usr/local/bin/node"),
                    real_path: PathBuf::from("/usr/local/bin/node"),
                    active: true,
                },
                ResolvedExecutable {
                    path: PathBuf::from("/opt/homebrew/bin/node"),
                    real_path: PathBuf::from("/opt/homebrew/Cellar/node/25.0/bin/node"),
                    active: false,
                },
            ],
        );

        assert_eq!(graph.resolutions.len(), 2);
        assert_eq!(graph.active().unwrap().status, ResolutionStatus::Active);
        assert_eq!(graph.shadowed_count(), 1);
        assert_eq!(graph.resolutions[1].owners[0].name, "Homebrew");
    }

    #[test]
    fn extracts_manager_root_from_runtime_path() {
        assert_eq!(
            nvm_root(Path::new("/home/me/.nvm/versions/node/v22/bin/node")),
            Some(PathBuf::from("/home/me/.nvm"))
        );
        assert_eq!(
            sdkman_root(Path::new("/home/me/.sdkman/candidates/java/21/bin/java")),
            Some(PathBuf::from("/home/me/.sdkman"))
        );
    }

    #[cfg(unix)]
    #[test]
    fn builds_runtime_to_manager_to_install_source_chain() {
        let parent = env::temp_dir().join(format!("whowns-nvm-chain-{}", std::process::id()));
        fs::create_dir_all(&parent).unwrap();
        let nvm_root = parent.join(".nvm");
        let _ = fs::remove_file(&nvm_root);
        symlink("/opt/homebrew/Cellar/nvm/0.40.3", &nvm_root).unwrap();
        let runtime = nvm_root.join("versions/node/v22.3.0/bin/node");

        let graph = from_resolutions(
            "node",
            vec![ResolvedExecutable {
                path: runtime.clone(),
                real_path: runtime,
                active: true,
            }],
        );

        let owners = &graph.resolutions[0].owners;
        assert_eq!(owners.len(), 2);
        assert_eq!(owners[0].name, "nvm");
        assert_eq!(owners[1].name, "Homebrew");
        assert_eq!(owners[1].package.as_deref(), Some("nvm"));
        fs::remove_file(nvm_root).unwrap();
        fs::remove_dir(parent).unwrap();
    }
}
