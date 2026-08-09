use std::path::Path;

use crate::model::{ActionGuide, OwnerId};

// Identity, display text, and OwnerKind live in `model`; this module holds only
// the detection rules and action guides keyed by that identity.

pub(super) struct QuerySpec<'a> {
    pub program: &'static str,
    pub arguments: Vec<&'a str>,
}

pub(super) const PATH_MANAGER_RULES: &[(&str, OwnerId)] = &[
    ("/.nvm/versions/", OwnerId::Nvm),
    ("/.local/share/fnm/", OwnerId::Fnm),
    ("/fnm_multishells/", OwnerId::Fnm),
    ("/.volta/", OwnerId::Volta),
    ("/.local/share/mise/", OwnerId::Mise),
    ("/.mise/", OwnerId::Mise),
    ("/.asdf/", OwnerId::Asdf),
    ("/.pyenv/", OwnerId::Pyenv),
    ("/.rbenv/", OwnerId::Rbenv),
    ("/.sdkman/", OwnerId::Sdkman),
    ("/.local/share/uv/python/", OwnerId::Uv),
];

impl OwnerId {
    pub(crate) const fn manager_executable(self) -> Option<&'static str> {
        match self {
            Self::Fnm => Some("fnm"),
            Self::Volta => Some("volta"),
            Self::Mise => Some("mise"),
            Self::Asdf => Some("asdf"),
            Self::Pyenv => Some("pyenv"),
            Self::Rbenv => Some("rbenv"),
            Self::Uv => Some("uv"),
            Self::Rustup => Some("rustup"),
            Self::CargoInstall => Some("cargo"),
            _ => None,
        }
    }

    pub(super) fn query(self, command: &str) -> Option<QuerySpec<'_>> {
        let (program, arguments): (&str, Vec<&str>) = match self {
            Self::Mise => ("mise", vec!["which", command]),
            Self::Asdf => ("asdf", vec!["which", command]),
            Self::Pyenv => ("pyenv", vec!["which", command]),
            Self::Rbenv => ("rbenv", vec!["which", command]),
            Self::Volta => ("volta", vec!["which", command]),
            Self::Rustup => ("rustup", vec!["which", command]),
            Self::Fnm => ("fnm", vec!["current"]),
            _ => return None,
        };
        Some(QuerySpec { program, arguments })
    }

    pub(super) fn tool_for_paths(self, paths: &[String]) -> Option<String> {
        let fixed = match self {
            Self::Nvm | Self::Fnm => Some("node"),
            Self::Pyenv | Self::Uv => Some("python"),
            Self::Rbenv => Some("ruby"),
            _ => None,
        };
        if let Some(tool) = fixed {
            return Some(tool.into());
        }

        let markers: &[&str] = match self {
            Self::Volta => &["/.volta/tools/image/"],
            Self::Mise => &["/.local/share/mise/installs/", "/.mise/installs/"],
            Self::Asdf => &["/.asdf/installs/"],
            Self::Sdkman => &["/.sdkman/candidates/"],
            _ => &[],
        };
        paths.iter().find_map(|path| {
            markers.iter().find_map(|marker| {
                path.split_once(marker)
                    .and_then(|(_, remainder)| remainder.split('/').next())
                    .filter(|value| !value.is_empty())
                    .map(str::to_owned)
            })
        })
    }

    pub(super) fn version_for_paths(self, paths: &[String]) -> Option<String> {
        let markers: &[&str] = match self {
            Self::Nvm => &["/.nvm/versions/node/"],
            Self::Fnm => &["/node-versions/"],
            Self::Volta => &["/.volta/tools/image/node/"],
            Self::Mise => &["/.local/share/mise/installs/", "/.mise/installs/"],
            Self::Asdf => &["/.asdf/installs/"],
            Self::Pyenv => &["/.pyenv/versions/"],
            Self::Rbenv => &["/.rbenv/versions/"],
            Self::Sdkman => &["/.sdkman/candidates/"],
            Self::Uv => &["/.local/share/uv/python/"],
            _ => &[],
        };
        for path in paths {
            for marker in markers {
                let Some((_, remainder)) = path.split_once(marker) else {
                    continue;
                };
                let parts: Vec<_> = remainder.split('/').collect();
                let candidate = if matches!(self, Self::Mise | Self::Asdf | Self::Sdkman) {
                    parts.get(1)
                } else {
                    parts.first()
                };
                if let Some(version) = candidate.filter(|value| !value.is_empty()) {
                    return Some(version.trim_start_matches('v').to_owned());
                }
            }
        }
        None
    }

    pub(super) fn actions(
        self,
        command: &str,
        package: Option<&str>,
        version: Option<&str>,
        path: &Path,
    ) -> ActionGuide {
        let tool = shell_quote(package.unwrap_or(command));
        let command = shell_quote(command);
        let version = shell_quote(version.unwrap_or("<version>"));
        let new_version = "<new-version>";
        let package = shell_quote(package.unwrap_or("<package>"));
        let path = shell_quote(&path.to_string_lossy());
        match self {
            Self::Homebrew => ActionGuide {
                inspect: Some(format!("brew info {package}")),
                update: Some(format!("brew upgrade {package}")),
                remove: Some(format!("brew uninstall {package}")),
                note: None,
            },
            Self::Nix => ActionGuide {
                inspect: Some(format!("nix-store --query --roots {path}")),
                note: Some("Use the flake, profile, or configuration reported as a root; the store path alone does not identify a safe update/remove command.".into()),
                ..ActionGuide::default()
            },
            Self::Nvm => ActionGuide {
                inspect: Some("nvm current".into()),
                update: Some(format!("nvm install {new_version}")),
                remove: Some(format!("nvm uninstall {version}")),
                note: Some("Switch away from the selected version before uninstalling it.".into()),
            },
            Self::Fnm => ActionGuide {
                inspect: Some("fnm current".into()),
                update: Some(format!("fnm install {new_version}")),
                remove: Some(format!("fnm uninstall {version}")),
                note: None,
            },
            Self::Volta => ActionGuide {
                inspect: Some(format!("volta which {command}")),
                update: Some(format!("volta install {tool}@{new_version}")),
                remove: Some(format!("volta uninstall {tool}")),
                note: None,
            },
            Self::Mise => ActionGuide {
                inspect: Some(format!("mise which {command}")),
                update: Some(format!("mise upgrade {tool}")),
                remove: Some(format!("mise uninstall {tool}@{version}")),
                note: None,
            },
            Self::Asdf => ActionGuide {
                inspect: Some(format!("asdf which {command}")),
                update: Some(format!("asdf install {tool} {new_version}")),
                remove: Some(format!("asdf uninstall {tool} {version}")),
                note: None,
            },
            Self::Pyenv => ActionGuide {
                inspect: Some(format!("pyenv which {command}")),
                update: Some(format!("pyenv install {new_version}")),
                remove: Some(format!("pyenv uninstall {version}")),
                note: None,
            },
            Self::Rbenv => ActionGuide {
                inspect: Some(format!("rbenv which {command}")),
                update: Some(format!("rbenv install {new_version}")),
                remove: Some(format!("rbenv uninstall {version}")),
                note: Some("The install/uninstall subcommands require the ruby-build plugin.".into()),
            },
            Self::Sdkman => ActionGuide {
                inspect: Some("sdk current".into()),
                update: Some(format!("sdk upgrade {tool}")),
                remove: Some(format!("sdk uninstall {tool} {version}")),
                note: None,
            },
            Self::Uv => ActionGuide {
                inspect: Some("uv python list".into()),
                update: Some(format!("uv python install {new_version}")),
                remove: Some(format!("uv python uninstall {version}")),
                note: None,
            },
            Self::Rustup => ActionGuide {
                inspect: Some(format!("rustup which {command}")),
                update: Some("rustup update".into()),
                remove: Some(format!("rustup toolchain uninstall {version}")),
                note: Some("Confirm the active toolchain with `rustup show active-toolchain` before removing it.".into()),
            },
            Self::RustupInstaller => ActionGuide {
                inspect: Some("rustup show".into()),
                update: Some("rustup self update".into()),
                remove: Some("rustup self uninstall".into()),
                note: Some("Self-uninstall removes rustup-managed toolchains as well; review `rustup show` first.".into()),
            },
            Self::CargoInstall => ActionGuide {
                inspect: Some("cargo install --list".into()),
                update: None,
                remove: None,
                note: Some(format!("Map executable {command} to its crate in `cargo install --list` before using `cargo install <crate>` or `cargo uninstall <crate>`.")),
            },
            Self::PnpmHome => ActionGuide {
                inspect: Some("pnpm bin -g".into()),
                update: None,
                remove: None,
                note: Some("Confirm whether Corepack or pnpm's standalone installer created this home before removing it.".into()),
            },
            Self::DenoInstaller => ActionGuide {
                inspect: Some("deno --version".into()),
                update: Some("deno upgrade".into()),
                remove: None,
                note: Some("Use the uninstall instructions for the installer that created DENO_INSTALL; no removal command is inferred from the path alone.".into()),
            },
            Self::BunInstaller => ActionGuide {
                inspect: Some("bun --version".into()),
                update: Some("bun upgrade".into()),
                remove: None,
                note: Some("Confirm BUN_INSTALL and follow Bun's uninstall instructions before deleting files.".into()),
            },
            Self::MacosInstaller => ActionGuide {
                inspect: package_id_from_command_context(&package)
                    .map(|id| format!("pkgutil --pkg-info {id}")),
                note: Some("Update by installing a newer package from the same vendor. macOS receipts do not define a generic safe uninstall command; follow the vendor's uninstall instructions.".into()),
                ..ActionGuide::default()
            },
            Self::PythonOrgInstaller => ActionGuide {
                inspect: Some(format!("pkgutil --pkg-info {package}")),
                note: Some("Update with a newer python.org installer. Follow python.org's macOS uninstall guidance rather than deleting framework files blindly.".into()),
                ..ActionGuide::default()
            },
            Self::MacPorts => ActionGuide {
                inspect: Some(format!("port provides {path}")),
                note: Some("Resolve the owning port first, then use `port upgrade <port>` or `port uninstall <port>`.".into()),
                ..ActionGuide::default()
            },
            Self::Dpkg => ActionGuide {
                inspect: Some(format!("dpkg-query -S {path}")),
                update: Some(format!("apt install --only-upgrade {package}")),
                remove: Some(format!("apt remove {package}")),
                note: None,
            },
            Self::Rpm => ActionGuide {
                inspect: Some(format!("rpm -qf {path}")),
                note: Some("Use the system's RPM frontend (for example dnf or zypper) with the reported package; rpm ownership alone does not identify the frontend.".into()),
                ..ActionGuide::default()
            },
            Self::Pacman => ActionGuide {
                inspect: Some(format!("pacman -Qo {path}")),
                update: Some(format!("pacman -S {package}")),
                remove: Some(format!("pacman -Rns {package}")),
                note: None,
            },
            Self::Apk => ActionGuide {
                inspect: Some(format!("apk info -W {path}")),
                update: Some(format!("apk upgrade {package}")),
                remove: Some(format!("apk del {package}")),
                note: None,
            },
            Self::OperatingSystem => ActionGuide {
                note: Some("Update this runtime through operating-system updates. Removing an OS-provided runtime is not recommended by this tool.".into()),
                ..ActionGuide::default()
            },
            Self::UnconfirmedOwner | Self::UnconfirmedSource => ActionGuide {
                inspect: unknown_inspect_command(&path),
                note: Some("Ownership is unconfirmed. Do not update or remove this file until a receipt, package record, or installer is identified.".into()),
                ..ActionGuide::default()
            },
        }
    }
}

fn package_id_from_command_context(package: &str) -> Option<&str> {
    (package != "<package>").then_some(package)
}

#[cfg(target_os = "macos")]
fn unknown_inspect_command(path: &str) -> Option<String> {
    Some(format!("pkgutil --file-info {path}"))
}

#[cfg(target_os = "linux")]
fn unknown_inspect_command(path: &str) -> Option<String> {
    Some(format!(
        "dpkg-query -S {path}  # also try rpm -qf, pacman -Qo, or apk info -W"
    ))
}

#[cfg(not(any(target_os = "macos", target_os = "linux")))]
fn unknown_inspect_command(_path: &str) -> Option<String> {
    None
}

pub(super) fn shell_quote(value: &str) -> String {
    if !value.is_empty()
        && value
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || "-._@/+<>".contains(character))
    {
        value.into()
    } else {
        format!("'{}'", value.replace('\'', "'\\''"))
    }
}
