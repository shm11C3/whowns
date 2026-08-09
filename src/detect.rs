use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::model::{ActionGuide, Confidence, Evidence, OwnerId, OwnershipNode};

pub fn detect(command: &str, path: &Path, real_path: &Path) -> OwnershipNode {
    let link_target = immediate_link_target(path);
    let mut candidate_paths = vec![path.to_path_buf(), real_path.to_path_buf()];
    if let Some(target) = &link_target {
        candidate_paths.push(target.clone());
    }
    let paths: Vec<_> = candidate_paths
        .iter()
        .map(|candidate| candidate.to_string_lossy().into_owned())
        .collect();
    let path_evidence = || path_evidence(path, real_path, link_target.as_deref());

    if paths.iter().any(|value| value.contains("/nix/store/")) {
        let package = nix_store_name(&paths);
        return owner(
            OwnerId::Nix,
            Confidence::Confirmed,
            package.clone(),
            None,
            path_evidence(),
            actions(OwnerId::Nix, command, package.as_deref(), None, real_path),
        );
    }

    if let Some((package, version)) = cellar_package(&paths) {
        let guide = actions(
            OwnerId::Homebrew,
            command,
            Some(&package),
            Some(&version),
            real_path,
        );
        return owner(
            OwnerId::Homebrew,
            Confidence::Confirmed,
            Some(package),
            Some(version),
            path_evidence(),
            guide,
        );
    }

    let path_managers = [
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
    for (marker, manager) in path_managers {
        if paths.iter().any(|value| value.contains(marker)) {
            let package = tool_for_manager(manager, &paths);
            let version = version_for_manager(manager, &paths);
            return owner(
                manager,
                Confidence::Confirmed,
                package.clone(),
                version.clone(),
                path_evidence(),
                actions(
                    manager,
                    command,
                    package.as_deref(),
                    version.as_deref(),
                    real_path,
                ),
            );
        }
    }

    if paths.iter().any(|value| value.contains("/.cargo/bin/")) {
        let manager = if matches!(
            command,
            "cargo" | "rustc" | "rustdoc" | "rustfmt" | "clippy-driver"
        ) {
            OwnerId::Rustup
        } else if command == "rustup" {
            OwnerId::RustupInstaller
        } else {
            OwnerId::CargoInstall
        };
        let confidence = if manager == OwnerId::Rustup && real_path.ends_with("rustup") {
            Confidence::Confirmed
        } else {
            Confidence::Probable
        };
        return owner(
            manager,
            confidence,
            None,
            None,
            path_evidence(),
            actions(manager, command, None, None, real_path),
        );
    }

    if paths
        .iter()
        .any(|value| value.contains("/Library/pnpm/") || value.contains("/.local/share/pnpm/"))
    {
        return owner(
            OwnerId::PnpmHome,
            Confidence::Probable,
            None,
            None,
            path_evidence(),
            actions(OwnerId::PnpmHome, command, None, None, real_path),
        );
    }

    if paths.iter().any(|value| value.contains("/.deno/")) {
        return owner(
            OwnerId::DenoInstaller,
            Confidence::Confirmed,
            None,
            None,
            path_evidence(),
            actions(OwnerId::DenoInstaller, command, None, None, real_path),
        );
    }

    if paths.iter().any(|value| value.contains("/.bun/")) {
        return owner(
            OwnerId::BunInstaller,
            Confidence::Confirmed,
            None,
            None,
            path_evidence(),
            actions(OwnerId::BunInstaller, command, None, None, real_path),
        );
    }

    #[cfg(target_os = "macos")]
    if let Some(receipt) =
        macos_receipt(command, real_path).or_else(|| macos_receipt(command, path))
    {
        return receipt;
    }

    #[cfg(target_os = "macos")]
    if let Some(receipt) = python_org_receipt(command, &paths, path_evidence(), real_path) {
        return receipt;
    }

    #[cfg(target_os = "linux")]
    if let Some(package) = linux_package(command, real_path) {
        return package;
    }

    if paths.iter().any(|value| {
        value.starts_with("/usr/bin/")
            || value.starts_with("/bin/")
            || value.starts_with("/System/")
    }) {
        return owner(
            OwnerId::OperatingSystem,
            Confidence::Probable,
            None,
            None,
            path_evidence(),
            actions(OwnerId::OperatingSystem, command, None, None, real_path),
        );
    }

    if paths.iter().any(|value| value.starts_with("/opt/local/")) {
        return owner(
            OwnerId::MacPorts,
            Confidence::Probable,
            None,
            None,
            path_evidence(),
            actions(OwnerId::MacPorts, command, None, None, real_path),
        );
    }

    let mut evidence = path_evidence();
    let reason = if paths.iter().any(|value| value.starts_with("/usr/local/")) {
        "/usr/local can contain files from vendor installers, package managers, or manual copies; no recognized owner claimed this path"
    } else {
        "no recognized manager path, package receipt, or operating-system package query claimed this executable"
    };
    evidence.push(Evidence::new("unconfirmed", reason));
    owner(
        OwnerId::UnconfirmedOwner,
        Confidence::Unknown,
        None,
        None,
        evidence,
        actions(OwnerId::UnconfirmedOwner, command, None, None, real_path),
    )
}

pub fn enrich_with_manager_query(owner: &mut OwnershipNode, command: &str, expected_path: &Path) {
    let Some(query) = manager_query(owner.id, command, expected_path) else {
        return;
    };
    let query_paths = vec![query.result.clone()];
    if owner.package.is_none() {
        owner.package = tool_for_manager(owner.id, &query_paths);
    }
    if owner.version.is_none() {
        owner.version = match owner.id {
            OwnerId::Fnm => Some(query.result.trim_start_matches('v').to_owned()),
            OwnerId::Rustup => toolchain_from_rustup_path(&query.result),
            id => version_for_manager(id, &query_paths),
        };
    }
    owner.actions = actions(
        owner.id,
        command,
        owner.package.as_deref(),
        owner.version.as_deref(),
        expected_path,
    );
    owner.evidence.push(query.evidence);
}

struct ManagerQuery {
    result: String,
    evidence: Evidence,
}

fn manager_query(manager: OwnerId, command: &str, expected_path: &Path) -> Option<ManagerQuery> {
    let (program, arguments): (&str, Vec<&str>) = match manager {
        OwnerId::Mise => ("mise", vec!["which", command]),
        OwnerId::Asdf => ("asdf", vec!["which", command]),
        OwnerId::Pyenv => ("pyenv", vec!["which", command]),
        OwnerId::Rbenv => ("rbenv", vec!["which", command]),
        OwnerId::Volta => ("volta", vec!["which", command]),
        OwnerId::Rustup => ("rustup", vec!["which", command]),
        OwnerId::Fnm => ("fnm", vec!["current"]),
        _ => return None,
    };
    let output = Command::new(program).args(&arguments).output().ok()?;
    if !output.status.success() {
        return None;
    }
    let result = String::from_utf8_lossy(&output.stdout).trim().to_owned();
    if result.is_empty() {
        return None;
    }
    let invocation = std::iter::once(program)
        .chain(arguments)
        .collect::<Vec<_>>()
        .join(" ");
    let relation = if Path::new(&result) == expected_path {
        " and it matches the resolved executable"
    } else {
        ""
    };
    Some(ManagerQuery {
        result: result.clone(),
        evidence: Evidence::new(
            "manager query",
            format!("`{invocation}` returned `{result}`{relation}"),
        ),
    })
}

pub fn manager_executable(manager: OwnerId) -> Option<&'static str> {
    match manager {
        OwnerId::Fnm => Some("fnm"),
        OwnerId::Volta => Some("volta"),
        OwnerId::Mise => Some("mise"),
        OwnerId::Asdf => Some("asdf"),
        OwnerId::Pyenv => Some("pyenv"),
        OwnerId::Rbenv => Some("rbenv"),
        OwnerId::Uv => Some("uv"),
        OwnerId::Rustup => Some("rustup"),
        OwnerId::CargoInstall => Some("cargo"),
        _ => None,
    }
}

pub fn unconfirmed_manager_source(manager: OwnerId, path: Option<&Path>) -> OwnershipNode {
    let manager = manager.display_name();
    let mut evidence = Vec::new();
    if let Some(path) = path {
        evidence.push(Evidence::new(
            "manager location",
            format!(
                "{manager} is located at {}, but that location has no recognized upstream owner",
                path.display()
            ),
        ));
    } else {
        evidence.push(Evidence::new(
            "manager lookup",
            format!("the {manager} runtime layout was recognized, but its own installation source was not discoverable on PATH"),
        ));
    }
    owner(
        OwnerId::UnconfirmedSource,
        Confidence::Unknown,
        None,
        None,
        evidence,
        ActionGuide {
            note: Some(format!(
                "Do not remove {manager} until its installer or package-manager record is identified."
            )),
            ..ActionGuide::default()
        },
    )
}

fn owner(
    id: OwnerId,
    confidence: Confidence,
    package: Option<String>,
    version: Option<String>,
    evidence: Vec<Evidence>,
    actions: ActionGuide,
) -> OwnershipNode {
    OwnershipNode {
        id,
        package,
        version,
        confidence,
        evidence,
        actions,
    }
}

fn path_evidence(path: &Path, real_path: &Path, link_target: Option<&Path>) -> Vec<Evidence> {
    let mut evidence = vec![Evidence::new(
        "PATH",
        format!("PATH entry is {}", path.display()),
    )];
    if let Some(target) = link_target {
        evidence.push(Evidence::new(
            "symlink",
            format!("direct target is {}", target.display()),
        ));
    }
    if path != real_path {
        evidence.push(Evidence::new(
            "filesystem",
            format!("ultimate target is {}", real_path.display()),
        ));
    }
    evidence
}

fn immediate_link_target(path: &Path) -> Option<PathBuf> {
    let target = fs::read_link(path).ok()?;
    if target.is_absolute() {
        Some(target)
    } else {
        Some(path.parent()?.join(target))
    }
}

fn cellar_package(paths: &[String]) -> Option<(String, String)> {
    for path in paths {
        let parts: Vec<_> = path.split('/').collect();
        let Some(index) = parts.iter().position(|part| *part == "Cellar") else {
            continue;
        };
        if let (Some(package), Some(version)) = (parts.get(index + 1), parts.get(index + 2)) {
            return Some(((*package).into(), (*version).into()));
        }
    }
    None
}

fn nix_store_name(paths: &[String]) -> Option<String> {
    paths.iter().find_map(|path| {
        let name = path.split("/nix/store/").nth(1)?.split('/').next()?;
        name.split_once('-').map(|(_, package)| package.to_owned())
    })
}

fn tool_for_manager(manager: OwnerId, paths: &[String]) -> Option<String> {
    let fixed = match manager {
        OwnerId::Nvm | OwnerId::Fnm => Some("node"),
        OwnerId::Pyenv | OwnerId::Uv => Some("python"),
        OwnerId::Rbenv => Some("ruby"),
        _ => None,
    };
    if let Some(tool) = fixed {
        return Some(tool.into());
    }

    let markers: &[&str] = match manager {
        OwnerId::Volta => &["/.volta/tools/image/"],
        OwnerId::Mise => &["/.local/share/mise/installs/", "/.mise/installs/"],
        OwnerId::Asdf => &["/.asdf/installs/"],
        OwnerId::Sdkman => &["/.sdkman/candidates/"],
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

fn toolchain_from_rustup_path(path: &str) -> Option<String> {
    path.split_once("/.rustup/toolchains/")
        .and_then(|(_, remainder)| remainder.split('/').next())
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
}

fn version_for_manager(manager: OwnerId, paths: &[String]) -> Option<String> {
    let markers: &[&str] = match manager {
        OwnerId::Nvm => &["/.nvm/versions/node/"],
        OwnerId::Fnm => &["/node-versions/"],
        OwnerId::Volta => &["/.volta/tools/image/node/"],
        OwnerId::Mise => &["/.local/share/mise/installs/", "/.mise/installs/"],
        OwnerId::Asdf => &["/.asdf/installs/"],
        OwnerId::Pyenv => &["/.pyenv/versions/"],
        OwnerId::Rbenv => &["/.rbenv/versions/"],
        OwnerId::Sdkman => &["/.sdkman/candidates/"],
        OwnerId::Uv => &["/.local/share/uv/python/"],
        _ => &[],
    };
    for path in paths {
        for marker in markers {
            let Some((_, remainder)) = path.split_once(marker) else {
                continue;
            };
            let parts: Vec<_> = remainder.split('/').collect();
            // These managers nest the version one level under the tool name.
            let candidate = if matches!(manager, OwnerId::Mise | OwnerId::Asdf | OwnerId::Sdkman) {
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

fn actions(
    manager: OwnerId,
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
    match manager {
        OwnerId::Homebrew => ActionGuide {
            inspect: Some(format!("brew info {package}")),
            update: Some(format!("brew upgrade {package}")),
            remove: Some(format!("brew uninstall {package}")),
            note: None,
        },
        OwnerId::Nix => ActionGuide {
            inspect: Some(format!("nix-store --query --roots {path}")),
            note: Some("Use the flake, profile, or configuration reported as a root; the store path alone does not identify a safe update/remove command.".into()),
            ..ActionGuide::default()
        },
        OwnerId::Nvm => ActionGuide {
            inspect: Some("nvm current".into()),
            update: Some(format!("nvm install {new_version}")),
            remove: Some(format!("nvm uninstall {version}")),
            note: Some("Switch away from the selected version before uninstalling it.".into()),
        },
        OwnerId::Fnm => ActionGuide {
            inspect: Some("fnm current".into()),
            update: Some(format!("fnm install {new_version}")),
            remove: Some(format!("fnm uninstall {version}")),
            note: None,
        },
        OwnerId::Volta => ActionGuide {
            inspect: Some(format!("volta which {command}")),
            update: Some(format!("volta install {tool}@{new_version}")),
            remove: Some(format!("volta uninstall {tool}")),
            note: None,
        },
        OwnerId::Mise => ActionGuide {
            inspect: Some(format!("mise which {command}")),
            update: Some(format!("mise upgrade {tool}")),
            remove: Some(format!("mise uninstall {tool}@{version}")),
            note: None,
        },
        OwnerId::Asdf => ActionGuide {
            inspect: Some(format!("asdf which {command}")),
            update: Some(format!("asdf install {tool} {new_version}")),
            remove: Some(format!("asdf uninstall {tool} {version}")),
            note: None,
        },
        OwnerId::Pyenv => ActionGuide {
            inspect: Some(format!("pyenv which {command}")),
            update: Some(format!("pyenv install {new_version}")),
            remove: Some(format!("pyenv uninstall {version}")),
            note: None,
        },
        OwnerId::Rbenv => ActionGuide {
            inspect: Some(format!("rbenv which {command}")),
            update: Some(format!("rbenv install {new_version}")),
            remove: Some(format!("rbenv uninstall {version}")),
            note: Some("The install/uninstall subcommands require the ruby-build plugin.".into()),
        },
        OwnerId::Sdkman => ActionGuide {
            inspect: Some("sdk current".into()),
            update: Some(format!("sdk upgrade {tool}")),
            remove: Some(format!("sdk uninstall {tool} {version}")),
            note: None,
        },
        OwnerId::Uv => ActionGuide {
            inspect: Some("uv python list".into()),
            update: Some(format!("uv python install {new_version}")),
            remove: Some(format!("uv python uninstall {version}")),
            note: None,
        },
        OwnerId::Rustup => ActionGuide {
            inspect: Some(format!("rustup which {command}")),
            update: Some("rustup update".into()),
            remove: Some(format!("rustup toolchain uninstall {version}")),
            note: Some("Confirm the active toolchain with `rustup show active-toolchain` before removing it.".into()),
        },
        OwnerId::RustupInstaller => ActionGuide {
            inspect: Some("rustup show".into()),
            update: Some("rustup self update".into()),
            remove: Some("rustup self uninstall".into()),
            note: Some("Self-uninstall removes rustup-managed toolchains as well; review `rustup show` first.".into()),
        },
        OwnerId::CargoInstall => ActionGuide {
            inspect: Some("cargo install --list".into()),
            update: None,
            remove: None,
            note: Some(format!("Map executable {command} to its crate in `cargo install --list` before using `cargo install <crate>` or `cargo uninstall <crate>`.")),
        },
        OwnerId::PnpmHome => ActionGuide {
            inspect: Some("pnpm bin -g".into()),
            update: None,
            remove: None,
            note: Some("Confirm whether Corepack or pnpm's standalone installer created this home before removing it.".into()),
        },
        OwnerId::DenoInstaller => ActionGuide {
            inspect: Some("deno --version".into()),
            update: Some("deno upgrade".into()),
            remove: None,
            note: Some("Use the uninstall instructions for the installer that created DENO_INSTALL; no removal command is inferred from the path alone.".into()),
        },
        OwnerId::BunInstaller => ActionGuide {
            inspect: Some("bun --version".into()),
            update: Some("bun upgrade".into()),
            remove: None,
            note: Some("Confirm BUN_INSTALL and follow Bun's uninstall instructions before deleting files.".into()),
        },
        OwnerId::MacosInstaller => ActionGuide {
            inspect: package_id_from_command_context(package.as_ref()).map(|id| format!("pkgutil --pkg-info {id}")),
            note: Some("Update by installing a newer package from the same vendor. macOS receipts do not define a generic safe uninstall command; follow the vendor's uninstall instructions.".into()),
            ..ActionGuide::default()
        },
        OwnerId::PythonOrgInstaller => ActionGuide {
            inspect: Some(format!("pkgutil --pkg-info {package}")),
            note: Some("Update with a newer python.org installer. Follow python.org's macOS uninstall guidance rather than deleting framework files blindly.".into()),
            ..ActionGuide::default()
        },
        OwnerId::MacPorts => ActionGuide {
            inspect: Some(format!("port provides {path}")),
            note: Some("Resolve the owning port first, then use `port upgrade <port>` or `port uninstall <port>`.".into()),
            ..ActionGuide::default()
        },
        OwnerId::Dpkg => ActionGuide {
            inspect: Some(format!("dpkg-query -S {path}")),
            update: Some(format!("apt install --only-upgrade {package}")),
            remove: Some(format!("apt remove {package}")),
            note: None,
        },
        OwnerId::Rpm => ActionGuide {
            inspect: Some(format!("rpm -qf {path}")),
            note: Some("Use the system's RPM frontend (for example dnf or zypper) with the reported package; rpm ownership alone does not identify the frontend.".into()),
            ..ActionGuide::default()
        },
        OwnerId::Pacman => ActionGuide {
            inspect: Some(format!("pacman -Qo {path}")),
            update: Some(format!("pacman -S {package}")),
            remove: Some(format!("pacman -Rns {package}")),
            note: None,
        },
        OwnerId::Apk => ActionGuide {
            inspect: Some(format!("apk info -W {path}")),
            update: Some(format!("apk upgrade {package}")),
            remove: Some(format!("apk del {package}")),
            note: None,
        },
        OwnerId::OperatingSystem => ActionGuide {
            note: Some("Update this runtime through operating-system updates. Removing an OS-provided runtime is not recommended by this tool.".into()),
            ..ActionGuide::default()
        },
        OwnerId::UnconfirmedOwner | OwnerId::UnconfirmedSource => ActionGuide {
            inspect: unknown_inspect_command(&path),
            note: Some("Ownership is unconfirmed. Do not update or remove this file until a receipt, package record, or installer is identified.".into()),
            ..ActionGuide::default()
        },
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

fn shell_quote(value: &str) -> String {
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

#[cfg(target_os = "macos")]
fn python_org_receipt(
    command: &str,
    paths: &[String],
    mut evidence: Vec<Evidence>,
    real_path: &Path,
) -> Option<OwnershipNode> {
    const MARKER: &str = "/Library/Frameworks/Python.framework/Versions/";
    let version = paths.iter().find_map(|path| {
        path.split_once(MARKER)
            .and_then(|(_, remainder)| remainder.split('/').next())
            .filter(|value| !value.is_empty())
    })?;
    let package = format!("org.python.Python.PythonFramework-{version}");
    let status = Command::new("pkgutil")
        .arg("--pkg-info")
        .arg(&package)
        .output()
        .ok()?
        .status;
    if !status.success() {
        return None;
    }
    evidence.push(Evidence::new(
        "pkgutil",
        format!("matching Python framework receipt {package} is installed"),
    ));
    let guide = actions(
        OwnerId::PythonOrgInstaller,
        command,
        Some(&package),
        Some(version),
        real_path,
    );
    Some(owner(
        OwnerId::PythonOrgInstaller,
        Confidence::Probable,
        Some(package),
        Some(version.into()),
        evidence,
        guide,
    ))
}

#[cfg(target_os = "macos")]
fn macos_receipt(command: &str, path: &Path) -> Option<OwnershipNode> {
    let output = Command::new("pkgutil")
        .arg("--file-info")
        .arg(path)
        .output()
        .ok()?;
    let stdout = String::from_utf8_lossy(&output.stdout);
    let package = field(&stdout, "pkgid:")?;
    let version = field(&stdout, "pkg-version:");
    let mut evidence = vec![Evidence::new(
        "pkgutil",
        format!("receipt {package} owns {}", path.display()),
    )];
    if let Some(version) = &version {
        evidence.push(Evidence::new(
            "pkgutil",
            format!("receipt records version {version}"),
        ));
    }
    let guide = actions(
        OwnerId::MacosInstaller,
        command,
        Some(&package),
        version.as_deref(),
        path,
    );
    Some(owner(
        OwnerId::MacosInstaller,
        Confidence::Confirmed,
        Some(package),
        version,
        evidence,
        guide,
    ))
}

#[cfg(target_os = "macos")]
fn field(text: &str, prefix: &str) -> Option<String> {
    text.lines()
        .find_map(|line| line.trim().strip_prefix(prefix))
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
}

#[cfg(target_os = "linux")]
fn linux_package(command: &str, path: &Path) -> Option<OwnershipNode> {
    let path_text = path.to_str()?;
    let queries: [(&str, &[&str], OwnerId); 4] = [
        ("dpkg-query", &["-S"], OwnerId::Dpkg),
        ("rpm", &["-qf"], OwnerId::Rpm),
        ("pacman", &["-Qo"], OwnerId::Pacman),
        ("apk", &["info", "-W"], OwnerId::Apk),
    ];
    for (program, arguments, manager) in queries {
        let Ok(output) = Command::new(program)
            .args(arguments)
            .arg(path_text)
            .output()
        else {
            continue;
        };
        if !output.status.success() {
            continue;
        }
        let raw = String::from_utf8_lossy(&output.stdout)
            .lines()
            .next()
            .map(str::trim)
            .filter(|value| !value.is_empty())?
            .to_owned();
        let package = if manager == OwnerId::Dpkg {
            raw.rsplit_once(": ")
                .map(|(package, _)| package)
                .unwrap_or(&raw)
                .to_owned()
        } else {
            raw.clone()
        };
        return Some(owner(
            manager,
            Confidence::Confirmed,
            Some(package.clone()),
            None,
            vec![Evidence::new(
                "package query",
                format!("`{program}` reports: {raw}"),
            )],
            actions(manager, command, Some(&package), None, path),
        ));
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::OwnerKind;

    #[cfg(unix)]
    use std::os::unix::fs::symlink;

    struct PathManagerFixture {
        command: &'static str,
        path: &'static str,
        owner: OwnerId,
        kind: OwnerKind,
        confidence: Confidence,
        package: Option<&'static str>,
        version: Option<&'static str>,
        action_fragment: &'static str,
    }

    fn action_text(actions: &ActionGuide) -> String {
        [
            actions.inspect.as_deref(),
            actions.update.as_deref(),
            actions.remove.as_deref(),
            actions.note.as_deref(),
        ]
        .into_iter()
        .flatten()
        .collect::<Vec<_>>()
        .join("\n")
    }

    #[test]
    fn detects_nvm_with_version_and_actions() {
        let path = Path::new("/home/me/.nvm/versions/node/v22.3.0/bin/node");
        let result = detect("node", path, path);
        assert_eq!(result.id, OwnerId::Nvm);
        assert_eq!(result.package.as_deref(), Some("node"));
        assert_eq!(result.version.as_deref(), Some("22.3.0"));
        assert_eq!(result.confidence, Confidence::Confirmed);
        assert_eq!(
            result.actions.update.as_deref(),
            Some("nvm install <new-version>")
        );
    }

    #[test]
    fn detects_advertised_path_based_managers() {
        let fixtures = [
            PathManagerFixture {
                command: "node",
                path: "/nix/store/abc123-nodejs-22.3.0/bin/node",
                owner: OwnerId::Nix,
                kind: OwnerKind::PackageManager,
                confidence: Confidence::Confirmed,
                package: Some("nodejs-22.3.0"),
                version: None,
                action_fragment: "nix-store --query --roots",
            },
            PathManagerFixture {
                command: "node",
                path: "/home/me/.local/share/fnm/node-versions/v22.3.0/installation/bin/node",
                owner: OwnerId::Fnm,
                kind: OwnerKind::VersionManager,
                confidence: Confidence::Confirmed,
                package: Some("node"),
                version: Some("22.3.0"),
                action_fragment: "fnm install <new-version>",
            },
            PathManagerFixture {
                command: "node",
                path: "/home/me/.volta/tools/image/node/22.3.0/bin/node",
                owner: OwnerId::Volta,
                kind: OwnerKind::VersionManager,
                confidence: Confidence::Confirmed,
                package: Some("node"),
                version: Some("22.3.0"),
                action_fragment: "volta install node@<new-version>",
            },
            PathManagerFixture {
                command: "node",
                path: "/home/me/.local/share/mise/installs/node/22.3.0/bin/node",
                owner: OwnerId::Mise,
                kind: OwnerKind::VersionManager,
                confidence: Confidence::Confirmed,
                package: Some("node"),
                version: Some("22.3.0"),
                action_fragment: "mise upgrade node",
            },
            PathManagerFixture {
                command: "python3",
                path: "/home/me/.pyenv/versions/3.12.4/bin/python3",
                owner: OwnerId::Pyenv,
                kind: OwnerKind::VersionManager,
                confidence: Confidence::Confirmed,
                package: Some("python"),
                version: Some("3.12.4"),
                action_fragment: "pyenv install <new-version>",
            },
            PathManagerFixture {
                command: "ruby",
                path: "/home/me/.rbenv/versions/3.3.3/bin/ruby",
                owner: OwnerId::Rbenv,
                kind: OwnerKind::VersionManager,
                confidence: Confidence::Confirmed,
                package: Some("ruby"),
                version: Some("3.3.3"),
                action_fragment: "rbenv uninstall 3.3.3",
            },
            PathManagerFixture {
                command: "java",
                path: "/home/me/.sdkman/candidates/java/21.0.2-tem/bin/java",
                owner: OwnerId::Sdkman,
                kind: OwnerKind::VersionManager,
                confidence: Confidence::Confirmed,
                package: Some("java"),
                version: Some("21.0.2-tem"),
                action_fragment: "sdk upgrade java",
            },
            PathManagerFixture {
                command: "python3",
                path: "/home/me/.local/share/uv/python/cpython-3.12.4-linux-x86_64-gnu/bin/python3",
                owner: OwnerId::Uv,
                kind: OwnerKind::VersionManager,
                confidence: Confidence::Confirmed,
                package: Some("python"),
                version: Some("cpython-3.12.4-linux-x86_64-gnu"),
                action_fragment: "uv python install <new-version>",
            },
            PathManagerFixture {
                command: "pnpm",
                path: "/home/me/.local/share/pnpm/pnpm",
                owner: OwnerId::PnpmHome,
                kind: OwnerKind::ToolInstaller,
                confidence: Confidence::Probable,
                package: None,
                version: None,
                action_fragment: "pnpm bin -g",
            },
            PathManagerFixture {
                command: "deno",
                path: "/home/me/.deno/bin/deno",
                owner: OwnerId::DenoInstaller,
                kind: OwnerKind::ToolInstaller,
                confidence: Confidence::Confirmed,
                package: None,
                version: None,
                action_fragment: "deno upgrade",
            },
            PathManagerFixture {
                command: "bun",
                path: "/home/me/.bun/bin/bun",
                owner: OwnerId::BunInstaller,
                kind: OwnerKind::ToolInstaller,
                confidence: Confidence::Confirmed,
                package: None,
                version: None,
                action_fragment: "bun upgrade",
            },
        ];

        for fixture in fixtures {
            let path = Path::new(fixture.path);
            let result = detect(fixture.command, path, path);
            let owner = fixture.owner.as_str();
            assert_eq!(result.id, fixture.owner, "path: {}", fixture.path);
            assert_eq!(result.kind(), fixture.kind, "owner: {owner}");
            assert_eq!(result.confidence, fixture.confidence, "owner: {owner}");
            assert_eq!(result.package.as_deref(), fixture.package, "owner: {owner}");
            assert_eq!(result.version.as_deref(), fixture.version, "owner: {owner}");
            assert!(
                action_text(&result.actions).contains(fixture.action_fragment),
                "owner: {owner}"
            );
        }
    }

    #[test]
    fn distinguishes_rustup_and_cargo_home_installers() {
        let rustup = detect(
            "rustc",
            Path::new("/home/me/.cargo/bin/rustc"),
            Path::new("/home/me/.cargo/bin/rustup"),
        );
        assert_eq!(rustup.id, OwnerId::Rustup);
        assert_eq!(rustup.kind(), OwnerKind::VersionManager);
        assert_eq!(rustup.confidence, Confidence::Confirmed);
        assert_eq!(rustup.actions.update.as_deref(), Some("rustup update"));

        let installer = detect(
            "rustup",
            Path::new("/home/me/.cargo/bin/rustup"),
            Path::new("/home/me/.cargo/bin/rustup"),
        );
        assert_eq!(installer.id, OwnerId::RustupInstaller);
        assert_eq!(installer.kind(), OwnerKind::ToolInstaller);
        assert_eq!(
            installer.actions.remove.as_deref(),
            Some("rustup self uninstall")
        );

        let cargo = detect(
            "rg",
            Path::new("/home/me/.cargo/bin/rg"),
            Path::new("/home/me/.cargo/bin/rg"),
        );
        assert_eq!(cargo.id, OwnerId::CargoInstall);
        assert_eq!(cargo.kind(), OwnerKind::ToolInstaller);
        assert_eq!(cargo.confidence, Confidence::Probable);
        assert!(cargo.actions.update.is_none());
        assert!(cargo.actions.remove.is_none());
    }

    #[test]
    fn recognizes_macports_layout() {
        let path = Path::new("/opt/local/bin/fixture-tool-that-does-not-exist");
        let result = detect("fixture-tool", path, path);

        assert_eq!(result.id, OwnerId::MacPorts);
        assert_eq!(result.kind(), OwnerKind::PackageManager);
        assert_eq!(result.confidence, Confidence::Probable);
        assert!(
            action_text(&result.actions)
                .contains("port provides /opt/local/bin/fixture-tool-that-does-not-exist")
        );
    }

    #[test]
    fn recognizes_operating_system_layout_without_removal_guidance() {
        let path = Path::new("/System/whowns-fixture-that-does-not-exist");
        let result = detect("fixture-tool", path, path);

        assert_eq!(result.id, OwnerId::OperatingSystem);
        assert_eq!(result.kind(), OwnerKind::OperatingSystem);
        assert_eq!(result.confidence, Confidence::Probable);
        assert!(result.actions.update.is_none());
        assert!(result.actions.remove.is_none());
    }

    #[test]
    fn uses_manager_tool_name_in_asdf_actions() {
        let path = Path::new("/home/me/.asdf/installs/nodejs/22.3.0/bin/node");
        let result = detect("node", path, path);
        assert_eq!(result.package.as_deref(), Some("nodejs"));
        assert_eq!(
            result.actions.remove.as_deref(),
            Some("asdf uninstall nodejs 22.3.0")
        );
    }

    #[test]
    fn detects_homebrew_package_and_version_from_real_path() {
        let link = Path::new("/opt/homebrew/bin/node");
        let real = Path::new("/opt/homebrew/Cellar/node/24.1.0/bin/node");
        let result = detect("node", link, real);
        assert_eq!(result.id, OwnerId::Homebrew);
        assert_eq!(result.package.as_deref(), Some("node"));
        assert_eq!(result.version.as_deref(), Some("24.1.0"));
        assert_eq!(result.confidence, Confidence::Confirmed);
        assert_eq!(
            result.actions.remove.as_deref(),
            Some("brew uninstall node")
        );
    }

    #[cfg(unix)]
    #[test]
    fn detects_homebrew_from_direct_link_before_final_resolution() {
        let link =
            std::env::temp_dir().join(format!("whowns-homebrew-link-{}", std::process::id()));
        let cellar_target = Path::new("/opt/homebrew/Cellar/node/25.6.1/bin/npm");
        let _ = fs::remove_file(&link);
        symlink(cellar_target, &link).unwrap();

        let result = detect(
            "npm",
            &link,
            Path::new("/opt/homebrew/lib/node_modules/npm/bin/npm-cli.js"),
        );

        assert_eq!(result.id, OwnerId::Homebrew);
        assert_eq!(result.package.as_deref(), Some("node"));
        assert_eq!(result.version.as_deref(), Some("25.6.1"));
        fs::remove_file(link).unwrap();
    }

    #[test]
    fn leaves_unrecognized_usr_local_owner_unconfirmed() {
        let path = Path::new("/usr/local/bin/custom");
        let result = detect("custom", path, path);
        assert_eq!(result.id, OwnerId::UnconfirmedOwner);
        assert_eq!(result.confidence, Confidence::Unknown);
        assert!(result.actions.update.is_none());
        assert!(result.actions.remove.is_none());
        assert!(
            result
                .evidence
                .iter()
                .any(|evidence| evidence.source == "unconfirmed")
        );
    }

    #[test]
    fn detected_owners_carry_identity_independent_of_display_text() {
        let path = Path::new("/home/me/.sdkman/candidates/java/21.0.2-tem/bin/java");
        let result = detect("java", path, path);

        assert_eq!(result.id, OwnerId::Sdkman);
        assert_eq!(result.id.as_str(), "sdkman");
        assert_eq!(result.display_name(), "SDKMAN!");
        // The action guide is selected by identity, so it survives a rename of
        // the display text.
        assert_eq!(result.actions.update.as_deref(), Some("sdk upgrade java"));
    }

    #[test]
    fn manager_lookups_are_keyed_by_identity() {
        assert_eq!(manager_executable(OwnerId::CargoInstall), Some("cargo"));
        assert_eq!(manager_executable(OwnerId::Volta), Some("volta"));
        // Owners that are not discoverable as an executable on PATH.
        assert_eq!(manager_executable(OwnerId::Nvm), None);
        assert_eq!(manager_executable(OwnerId::Homebrew), None);
    }

    #[test]
    fn unconfirmed_source_keeps_its_own_identity_and_names_the_manager() {
        let source = unconfirmed_manager_source(OwnerId::Sdkman, None);

        assert_eq!(source.id, OwnerId::UnconfirmedSource);
        assert_eq!(source.confidence, Confidence::Unknown);
        assert!(
            source
                .evidence
                .iter()
                .any(|evidence| evidence.detail.contains("SDKMAN!"))
        );
    }

    #[test]
    fn quotes_unsafe_action_arguments() {
        assert_eq!(shell_quote("safe@1.2"), "safe@1.2");
        assert_eq!(shell_quote("a b'c"), "'a b'\\''c'");
    }

    #[test]
    fn extracts_rustup_toolchain_from_query_result() {
        assert_eq!(
            toolchain_from_rustup_path(
                "/home/me/.rustup/toolchains/stable-x86_64-unknown-linux-gnu/bin/rustc"
            )
            .as_deref(),
            Some("stable-x86_64-unknown-linux-gnu")
        );
    }
}
