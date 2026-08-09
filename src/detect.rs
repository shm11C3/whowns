use std::ffi::OsStr;
use std::fs;
use std::path::{Path, PathBuf};

use crate::exec::CommandRunner;
use crate::model::{ActionGuide, Confidence, Evidence, OwnerId, OwnershipNode};
use crate::scan;

mod owner;

use owner::PATH_MANAGER_RULES;

type Detector = for<'a> fn(&DetectionContext<'a>) -> Option<OwnershipNode>;

// Order is semantic: the first detector with sufficient evidence owns the result.
const DETECTORS: &[Detector] = &[
    detect_nix,
    detect_homebrew,
    detect_path_manager,
    detect_cargo_home,
    detect_pnpm_home,
    detect_deno_installer,
    detect_bun_installer,
    #[cfg(target_os = "macos")]
    detect_macos_receipt,
    #[cfg(target_os = "macos")]
    detect_python_org_installer,
    #[cfg(target_os = "linux")]
    detect_linux_package,
    detect_operating_system,
    detect_macports,
];

struct DetectionContext<'a> {
    command: &'a str,
    path: &'a Path,
    real_path: &'a Path,
    link_target: Option<PathBuf>,
    paths: Vec<String>,
    runner: &'a CommandRunner,
}

impl<'a> DetectionContext<'a> {
    fn new(
        command: &'a str,
        path: &'a Path,
        real_path: &'a Path,
        runner: &'a CommandRunner,
    ) -> Self {
        let link_target = immediate_link_target(path);
        let mut candidate_paths = vec![path.to_path_buf(), real_path.to_path_buf()];
        if let Some(target) = &link_target {
            candidate_paths.push(target.clone());
        }
        let paths = candidate_paths
            .iter()
            .map(|candidate| candidate.to_string_lossy().into_owned())
            .collect();
        Self {
            command,
            path,
            real_path,
            link_target,
            paths,
            runner,
        }
    }

    fn contains(&self, marker: &str) -> bool {
        self.paths.iter().any(|path| path.contains(marker))
    }

    fn starts_with_any(&self, prefixes: &[&str]) -> bool {
        self.paths
            .iter()
            .any(|path| prefixes.iter().any(|prefix| path.starts_with(prefix)))
    }

    fn evidence(&self) -> Vec<Evidence> {
        path_evidence(self.path, self.real_path, self.link_target.as_deref())
    }

    fn owner(
        &self,
        id: OwnerId,
        confidence: Confidence,
        package: Option<String>,
        version: Option<String>,
        evidence: Vec<Evidence>,
    ) -> OwnershipNode {
        let actions = id.actions(
            self.command,
            package.as_deref(),
            version.as_deref(),
            self.real_path,
        );
        owner(id, confidence, package, version, evidence, actions)
    }
}

pub fn detect(
    command: &str,
    path: &Path,
    real_path: &Path,
    runner: &CommandRunner,
) -> OwnershipNode {
    let context = DetectionContext::new(command, path, real_path, runner);
    DETECTORS
        .iter()
        .find_map(|detector| detector(&context))
        .unwrap_or_else(|| detect_unconfirmed(&context))
}

fn detect_nix(context: &DetectionContext<'_>) -> Option<OwnershipNode> {
    context.contains("/nix/store/").then_some(())?;
    let package = nix_store_name(&context.paths);
    Some(context.owner(
        OwnerId::Nix,
        Confidence::Confirmed,
        package,
        None,
        context.evidence(),
    ))
}

fn detect_homebrew(context: &DetectionContext<'_>) -> Option<OwnershipNode> {
    let (package, version) = cellar_package(&context.paths)?;
    Some(context.owner(
        OwnerId::Homebrew,
        Confidence::Confirmed,
        Some(package),
        Some(version),
        context.evidence(),
    ))
}

fn detect_path_manager(context: &DetectionContext<'_>) -> Option<OwnershipNode> {
    let id = PATH_MANAGER_RULES
        .iter()
        .find(|(marker, _)| context.contains(marker))
        .map(|(_, id)| *id)?;
    let package = id.tool_for_paths(&context.paths);
    let version = id.version_for_paths(&context.paths);
    Some(context.owner(
        id,
        Confidence::Confirmed,
        package,
        version,
        context.evidence(),
    ))
}

fn detect_cargo_home(context: &DetectionContext<'_>) -> Option<OwnershipNode> {
    context.contains("/.cargo/bin/").then_some(())?;
    let id = if matches!(
        context.command,
        "cargo" | "rustc" | "rustdoc" | "rustfmt" | "clippy-driver"
    ) {
        OwnerId::Rustup
    } else if context.command == "rustup" {
        OwnerId::RustupInstaller
    } else {
        OwnerId::CargoInstall
    };
    let confidence = if id == OwnerId::Rustup && context.real_path.ends_with("rustup") {
        Confidence::Confirmed
    } else {
        Confidence::Probable
    };
    Some(context.owner(id, confidence, None, None, context.evidence()))
}

fn detect_pnpm_home(context: &DetectionContext<'_>) -> Option<OwnershipNode> {
    (context.contains("/Library/pnpm/") || context.contains("/.local/share/pnpm/")).then(|| {
        context.owner(
            OwnerId::PnpmHome,
            Confidence::Probable,
            None,
            None,
            context.evidence(),
        )
    })
}

fn detect_deno_installer(context: &DetectionContext<'_>) -> Option<OwnershipNode> {
    context.contains("/.deno/").then(|| {
        context.owner(
            OwnerId::DenoInstaller,
            Confidence::Confirmed,
            None,
            None,
            context.evidence(),
        )
    })
}

fn detect_bun_installer(context: &DetectionContext<'_>) -> Option<OwnershipNode> {
    context.contains("/.bun/").then(|| {
        context.owner(
            OwnerId::BunInstaller,
            Confidence::Confirmed,
            None,
            None,
            context.evidence(),
        )
    })
}

#[cfg(target_os = "macos")]
fn detect_macos_receipt(context: &DetectionContext<'_>) -> Option<OwnershipNode> {
    macos_receipt(context.runner, context.command, context.real_path)
        .or_else(|| macos_receipt(context.runner, context.command, context.path))
}

#[cfg(target_os = "macos")]
fn detect_python_org_installer(context: &DetectionContext<'_>) -> Option<OwnershipNode> {
    python_org_receipt(
        context.runner,
        context.command,
        &context.paths,
        context.evidence(),
        context.real_path,
    )
}

#[cfg(target_os = "linux")]
fn detect_linux_package(context: &DetectionContext<'_>) -> Option<OwnershipNode> {
    linux_package(context.runner, context.command, context.real_path)
}

fn detect_operating_system(context: &DetectionContext<'_>) -> Option<OwnershipNode> {
    context
        .starts_with_any(&["/usr/bin/", "/bin/", "/System/"])
        .then(|| {
            context.owner(
                OwnerId::OperatingSystem,
                Confidence::Probable,
                None,
                None,
                context.evidence(),
            )
        })
}

fn detect_macports(context: &DetectionContext<'_>) -> Option<OwnershipNode> {
    context.starts_with_any(&["/opt/local/"]).then(|| {
        context.owner(
            OwnerId::MacPorts,
            Confidence::Probable,
            None,
            None,
            context.evidence(),
        )
    })
}

fn detect_unconfirmed(context: &DetectionContext<'_>) -> OwnershipNode {
    let mut evidence = context.evidence();
    let reason = if context.starts_with_any(&["/usr/local/"]) {
        "/usr/local can contain files from vendor installers, package managers, or manual copies; no recognized owner claimed this path"
    } else {
        "no recognized manager path, package receipt, or operating-system package query claimed this executable"
    };
    evidence.push(Evidence::new("unconfirmed", reason));
    context.owner(
        OwnerId::UnconfirmedOwner,
        Confidence::Unknown,
        None,
        None,
        evidence,
    )
}

pub fn enrich_with_manager_query(
    owner: &mut OwnershipNode,
    command: &str,
    expected_path: &Path,
    runner: &CommandRunner,
) {
    let id = owner.id;
    let Some(query) = manager_query(runner, id, command, expected_path) else {
        return;
    };
    owner.evidence.push(query.evidence);
    let Some(result) = query.result else {
        // The query ran but failed (timeout or spawn failure); the evidence
        // above already records that, and there is nothing to enrich.
        return;
    };
    let query_paths = vec![result.clone()];
    if owner.package.is_none() {
        owner.package = id.tool_for_paths(&query_paths);
    }
    if owner.version.is_none() {
        owner.version = match id {
            OwnerId::Fnm => Some(result.trim_start_matches('v').to_owned()),
            OwnerId::Rustup => toolchain_from_rustup_path(&result),
            _ => id.version_for_paths(&query_paths),
        };
    }
    owner.actions = id.actions(
        command,
        owner.package.as_deref(),
        owner.version.as_deref(),
        expected_path,
    );
}

struct ManagerQuery {
    /// `None` when the query ran but failed (timeout or spawn failure): the
    /// evidence still explains what happened, but there is no data to enrich
    /// package/version/actions with.
    result: Option<String>,
    evidence: Evidence,
}

/// Resolves `name` to the executable `whowns` already found on PATH, rather
/// than handing a bare name to `Command` and letting it search PATH again.
/// The second search could pick a different shim than the one `whowns`
/// inspected if PATH changed between the two lookups, so reusing our own
/// resolution keeps the query honest about which binary it is questioning.
///
/// Uses the PATH entry itself, not its canonicalized target: a manager whose
/// behavior depends on the invoked path or name (a multi-call binary reached
/// through a symlink, for instance) would misbehave if invoked under its
/// resolved-away canonical name instead of the one actually found on PATH.
fn resolve_program(name: &str) -> Option<PathBuf> {
    scan::find_executables(name)
        .into_iter()
        .find(|resolution| resolution.active)
        .map(|resolution| resolution.path)
}

fn manager_query(
    runner: &CommandRunner,
    id: OwnerId,
    command: &str,
    expected_path: &Path,
) -> Option<ManagerQuery> {
    let query = id.query(command)?;
    let program = resolve_program(query.program)?;
    let invocation = std::iter::once(query.program)
        .chain(query.arguments.iter().copied())
        .collect::<Vec<_>>()
        .join(" ");
    let arguments: Vec<&OsStr> = query
        .arguments
        .iter()
        .map(|argument| OsStr::new(*argument))
        .collect();
    match runner.query(&program, &arguments) {
        Err(failure) => Some(ManagerQuery {
            result: None,
            evidence: Evidence::new(
                "manager query",
                format!("`{invocation}` {}", failure.describe()),
            ),
        }),
        Ok(output) if output.success && output.truncated => Some(ManagerQuery {
            result: None,
            evidence: Evidence::new(
                "manager query",
                format!("`{invocation}` output was truncated; ignoring it"),
            ),
        }),
        Ok(output) if output.success => {
            let result = output.stdout.trim().to_owned();
            if result.is_empty() {
                return None;
            }
            let relation = if Path::new(&result) == expected_path {
                " and it matches the resolved executable"
            } else {
                ""
            };
            Some(ManagerQuery {
                result: Some(result.clone()),
                evidence: Evidence::new(
                    "manager query",
                    format!("`{invocation}` returned `{result}`{relation}"),
                ),
            })
        }
        Ok(_) => Some(ManagerQuery {
            result: None,
            evidence: Evidence::new(
                "manager query",
                format!("`{invocation}` exited without confirming ownership"),
            ),
        }),
    }
}

pub(crate) fn unconfirmed_manager_source(manager: OwnerId, path: Option<&Path>) -> OwnershipNode {
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

fn toolchain_from_rustup_path(path: &str) -> Option<String> {
    path.split_once("/.rustup/toolchains/")
        .and_then(|(_, remainder)| remainder.split('/').next())
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
}

#[cfg(target_os = "macos")]
fn python_org_receipt(
    runner: &CommandRunner,
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
    let program = resolve_program("pkgutil")?;
    let output = runner
        .query(&program, &[OsStr::new("--pkg-info"), OsStr::new(&package)])
        .ok()?;
    if !output.success {
        return None;
    }
    evidence.push(Evidence::new(
        "pkgutil",
        format!("matching Python framework receipt {package} is installed"),
    ));
    let id = OwnerId::PythonOrgInstaller;
    let guide = id.actions(command, Some(&package), Some(version), real_path);
    Some(owner(
        id,
        Confidence::Probable,
        Some(package),
        Some(version.into()),
        evidence,
        guide,
    ))
}

#[cfg(target_os = "macos")]
fn macos_receipt(runner: &CommandRunner, command: &str, path: &Path) -> Option<OwnershipNode> {
    let program = resolve_program("pkgutil")?;
    let output = runner
        .query(&program, &[OsStr::new("--file-info"), path.as_os_str()])
        .ok()?;
    let package = field(&output.stdout, "pkgid:")?;
    let version = field(&output.stdout, "pkg-version:");
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
    let id = OwnerId::MacosInstaller;
    let guide = id.actions(command, Some(&package), version.as_deref(), path);
    Some(owner(
        id,
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
fn linux_package(runner: &CommandRunner, command: &str, path: &Path) -> Option<OwnershipNode> {
    let queries: [(&str, &[&str], OwnerId); 4] = [
        ("dpkg-query", &["-S"], OwnerId::Dpkg),
        ("rpm", &["-qf"], OwnerId::Rpm),
        ("pacman", &["-Qo"], OwnerId::Pacman),
        ("apk", &["info", "-W"], OwnerId::Apk),
    ];
    for (program_name, arguments, id) in queries {
        let Some(program) = resolve_program(program_name) else {
            continue;
        };
        let full_arguments: Vec<&OsStr> = arguments
            .iter()
            .map(|argument| OsStr::new(*argument))
            .chain([path.as_os_str()])
            .collect();
        let Ok(output) = runner.query(&program, &full_arguments) else {
            continue;
        };
        if !output.success {
            continue;
        }
        let Some(raw) = output
            .stdout
            .lines()
            .next()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_owned)
        else {
            continue;
        };
        let package = if id == OwnerId::Dpkg {
            raw.rsplit_once(": ")
                .map(|(package, _)| package)
                .unwrap_or(&raw)
                .to_owned()
        } else {
            raw.clone()
        };
        return Some(owner(
            id,
            Confidence::Confirmed,
            Some(package.clone()),
            None,
            vec![Evidence::new(
                "package query",
                format!("`{program_name}` reports: {raw}"),
            )],
            id.actions(command, Some(&package), None, path),
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

    fn detect_for_test(command: &str, path: &Path, real_path: &Path) -> OwnershipNode {
        detect(command, path, real_path, &CommandRunner::new())
    }

    #[test]
    fn detects_nvm_with_version_and_actions() {
        let path = Path::new("/home/me/.nvm/versions/node/v22.3.0/bin/node");
        let result = detect_for_test("node", path, path);
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
            let result = detect_for_test(fixture.command, path, path);
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
        let rustup = detect_for_test(
            "rustc",
            Path::new("/home/me/.cargo/bin/rustc"),
            Path::new("/home/me/.cargo/bin/rustup"),
        );
        assert_eq!(rustup.id, OwnerId::Rustup);
        assert_eq!(rustup.kind(), OwnerKind::VersionManager);
        assert_eq!(rustup.confidence, Confidence::Confirmed);
        assert_eq!(rustup.actions.update.as_deref(), Some("rustup update"));

        let installer = detect_for_test(
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

        let cargo = detect_for_test(
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
        let result = detect_for_test("fixture-tool", path, path);

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
        let result = detect_for_test("fixture-tool", path, path);

        assert_eq!(result.id, OwnerId::OperatingSystem);
        assert_eq!(result.kind(), OwnerKind::OperatingSystem);
        assert_eq!(result.confidence, Confidence::Probable);
        assert!(result.actions.update.is_none());
        assert!(result.actions.remove.is_none());
    }

    #[test]
    fn uses_manager_tool_name_in_asdf_actions() {
        let path = Path::new("/home/me/.asdf/installs/nodejs/22.3.0/bin/node");
        let result = detect_for_test("node", path, path);
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
        let result = detect_for_test("node", link, real);
        assert_eq!(result.id, OwnerId::Homebrew);
        assert_eq!(result.package.as_deref(), Some("node"));
        assert_eq!(result.version.as_deref(), Some("24.1.0"));
        assert_eq!(result.confidence, Confidence::Confirmed);
        assert_eq!(
            result.actions.remove.as_deref(),
            Some("brew uninstall node")
        );
    }

    #[test]
    fn preserves_detector_priority_across_candidate_paths() {
        let path = Path::new("/home/me/.nvm/versions/node/v22.3.0/bin/node");
        let real = Path::new("/opt/homebrew/Cellar/node/24.1.0/bin/node");

        let result = detect_for_test("node", path, real);

        assert_eq!(result.id, OwnerId::Homebrew);
        assert_eq!(result.package.as_deref(), Some("node"));
        assert_eq!(result.version.as_deref(), Some("24.1.0"));
    }

    #[cfg(unix)]
    #[test]
    fn detects_homebrew_from_direct_link_before_final_resolution() {
        let link =
            std::env::temp_dir().join(format!("whowns-homebrew-link-{}", std::process::id()));
        let cellar_target = Path::new("/opt/homebrew/Cellar/node/25.6.1/bin/npm");
        let _ = fs::remove_file(&link);
        symlink(cellar_target, &link).unwrap();

        let result = detect_for_test(
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
        let result = detect_for_test("custom", path, path);
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
        let result = detect_for_test("java", path, path);

        assert_eq!(result.id, OwnerId::Sdkman);
        assert_eq!(result.id.as_str(), "sdkman");
        assert_eq!(result.display_name(), "SDKMAN!");
        // The action guide is selected by identity, so it survives a rename of
        // the display text.
        assert_eq!(result.actions.update.as_deref(), Some("sdk upgrade java"));
    }

    #[test]
    fn manager_lookups_are_keyed_by_identity() {
        assert_eq!(OwnerId::CargoInstall.manager_executable(), Some("cargo"));
        assert_eq!(OwnerId::Volta.manager_executable(), Some("volta"));
        // Owners that are not discoverable as an executable on PATH.
        assert_eq!(OwnerId::Nvm.manager_executable(), None);
        assert_eq!(OwnerId::Homebrew.manager_executable(), None);
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
        assert_eq!(owner::shell_quote("safe@1.2"), "safe@1.2");
        assert_eq!(owner::shell_quote("a b'c"), "'a b'\\''c'");
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

    #[test]
    fn resolve_program_returns_none_when_the_executable_is_not_on_path() {
        // A name this specific will never coincidentally exist on a real
        // PATH, so this is deterministic regardless of what happens to be
        // installed on the machine running the test (unlike a real manager
        // name such as "mise", which may or may not actually be present).
        assert_eq!(
            resolve_program("whowns-fixture-program-that-does-not-exist-zzz"),
            None
        );
    }
}
