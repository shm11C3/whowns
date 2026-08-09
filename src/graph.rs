use std::fs;
use std::path::{Path, PathBuf};

use crate::detect;
use crate::exec::CommandRunner;
use crate::model::{
    OwnerId, OwnerKind, OwnershipGraph, OwnershipNode, Resolution, ResolutionStatus,
};
use crate::scan::{self, ResolvedExecutable};

const MAX_OWNERSHIP_DEPTH: usize = 8;

struct UpstreamOwner {
    node: OwnershipNode,
    path: Option<PathBuf>,
}

pub fn inspect(command: &str, runner: &CommandRunner) -> OwnershipGraph {
    let roots = detect::ManagerRoots::from_environment();
    from_resolutions(command, scan::find_executables(command), runner, &roots)
}

pub fn all(show_missing: bool, runner: &CommandRunner) -> Vec<OwnershipGraph> {
    let roots = detect::ManagerRoots::from_environment();
    let mut graphs: Vec<_> = scan::KNOWN_RUNTIMES
        .iter()
        .map(|command| from_resolutions(command, scan::find_executables(command), runner, &roots))
        .collect();
    if !show_missing {
        graphs.retain(|graph| !graph.resolutions.is_empty());
    }
    graphs
}

fn from_resolutions(
    command: &str,
    resolved: Vec<ResolvedExecutable>,
    runner: &CommandRunner,
    roots: &detect::ManagerRoots,
) -> OwnershipGraph {
    let resolutions = resolved
        .into_iter()
        .map(|resolved| {
            let primary = detect_owner(command, &resolved.path, &resolved.real_path, runner, roots);
            let owners = ownership_chain(primary, &resolved.path, runner, roots);
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

fn detect_owner(
    command: &str,
    path: &Path,
    real_path: &Path,
    runner: &CommandRunner,
    roots: &detect::ManagerRoots,
) -> OwnershipNode {
    let mut owner = detect::detect(command, path, real_path, runner, roots);
    detect::enrich_with_manager_query(&mut owner, command, real_path, runner, roots);
    owner
}

fn ownership_chain(
    primary: OwnershipNode,
    runtime_path: &Path,
    runner: &CommandRunner,
    roots: &detect::ManagerRoots,
) -> Vec<OwnershipNode> {
    let mut visited = vec![(primary.id, path_identity(runtime_path))];
    let mut current_path = runtime_path.to_path_buf();
    let mut owners = vec![primary];

    loop {
        let current = owners.last().expect("an ownership chain is never empty");
        if !matches!(
            current.kind(),
            OwnerKind::VersionManager | OwnerKind::ToolInstaller
        ) {
            break;
        }
        if owners.len() >= MAX_OWNERSHIP_DEPTH {
            owners.push(detect::unconfirmed_chain_termination(format!(
                "ownership resolution reached the safety limit of {MAX_OWNERSHIP_DEPTH} owners"
            )));
            break;
        }

        let Some(upstream) = upstream_owner(current, &current_path, runner, roots) else {
            break;
        };
        if upstream.node.id == OwnerId::UnconfirmedSource {
            owners.push(upstream.node);
            break;
        }

        let upstream_path = upstream.path.map(|path| path_identity(&path));
        let repeated_identity = upstream_path.as_ref().is_some_and(|path| {
            visited
                .iter()
                .any(|(owner, visited_path)| *owner == upstream.node.id && visited_path == path)
        });
        if repeated_identity {
            let path = upstream_path.as_deref().map_or_else(
                || "an unknown path".into(),
                |path| path.display().to_string(),
            );
            owners.push(detect::unconfirmed_chain_termination(format!(
                "ownership cycle detected while resolving {} at {path}",
                upstream.node.display_name()
            )));
            break;
        }

        if let Some(path) = upstream_path {
            visited.push((upstream.node.id, path.clone()));
            current_path = path;
        }
        owners.push(upstream.node);
    }

    owners
}

fn path_identity(path: &Path) -> PathBuf {
    fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf())
}

fn upstream_owner(
    primary: &OwnershipNode,
    runtime_path: &Path,
    runner: &CommandRunner,
    roots: &detect::ManagerRoots,
) -> Option<UpstreamOwner> {
    if !matches!(
        primary.kind(),
        OwnerKind::VersionManager | OwnerKind::ToolInstaller
    ) {
        return None;
    }

    let manager = primary.id;
    if let Some(command) = manager.root_source_command() {
        return Some(source_from_root(
            manager,
            command,
            roots.root_for(manager, runtime_path),
            runner,
            roots,
        ));
    }

    let manager_command = manager.manager_executable()?;
    let manager_resolution = scan::find_executables(manager_command)
        .into_iter()
        .find(|resolution| resolution.active);
    let Some(manager_resolution) = manager_resolution else {
        return Some(UpstreamOwner {
            node: detect::unconfirmed_manager_source(manager, None),
            path: None,
        });
    };
    let source = detect_owner(
        manager_command,
        &manager_resolution.path,
        &manager_resolution.real_path,
        runner,
        roots,
    );
    Some(UpstreamOwner {
        node: source,
        path: Some(manager_resolution.path),
    })
}

fn source_from_root(
    manager: OwnerId,
    command: &str,
    root: Option<PathBuf>,
    runner: &CommandRunner,
    roots: &detect::ManagerRoots,
) -> UpstreamOwner {
    let Some(root) = root else {
        return UpstreamOwner {
            node: detect::unconfirmed_manager_source(manager, None),
            path: None,
        };
    };
    let real_root = fs::canonicalize(&root).unwrap_or_else(|_| root.clone());
    let mut source = detect::detect(command, &root, &real_root, runner, roots);
    detect::enrich_with_manager_query_in_root(&mut source, command, &real_root, runner, roots);
    UpstreamOwner {
        node: source,
        path: Some(root),
    }
}

#[cfg(test)]
mod tests {
    use std::env;

    use super::*;

    #[cfg(unix)]
    use std::os::unix::fs::symlink;

    fn from_resolutions_for_test(
        command: &str,
        resolved: Vec<ResolvedExecutable>,
    ) -> OwnershipGraph {
        from_resolutions_for_test_with_home(command, resolved, Path::new("/home/me"))
    }

    fn from_resolutions_for_test_with_home(
        command: &str,
        resolved: Vec<ResolvedExecutable>,
        home: &Path,
    ) -> OwnershipGraph {
        from_resolutions(
            command,
            resolved,
            &CommandRunner::new(),
            &detect::ManagerRoots::for_home(home),
        )
    }

    #[test]
    fn one_graph_holds_active_and_shadowed_resolutions() {
        let graph = from_resolutions_for_test(
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
        assert_eq!(graph.resolutions[1].owners[0].id, OwnerId::Homebrew);
    }

    #[test]
    fn extracts_manager_root_from_runtime_path() {
        let roots = detect::ManagerRoots::for_home(Path::new("/home/me"));
        assert_eq!(
            roots.root_for(
                OwnerId::Nvm,
                Path::new("/home/me/.nvm/versions/node/v22/bin/node")
            ),
            Some(PathBuf::from("/home/me/.nvm"))
        );
        assert_eq!(
            roots.root_for(
                OwnerId::Sdkman,
                Path::new("/home/me/.sdkman/candidates/java/21/bin/java")
            ),
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

        let graph = from_resolutions_for_test_with_home(
            "node",
            vec![ResolvedExecutable {
                path: runtime.clone(),
                real_path: runtime,
                active: true,
            }],
            &parent,
        );

        let owners = &graph.resolutions[0].owners;
        assert_eq!(owners.len(), 2);
        assert_eq!(owners[0].id, OwnerId::Nvm);
        assert_eq!(owners[1].id, OwnerId::Homebrew);
        assert_eq!(owners[1].package.as_deref(), Some("nvm"));
        fs::remove_file(nvm_root).unwrap();
        fs::remove_dir(parent).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn manager_root_detection_uses_the_real_command_not_the_display_name() {
        let parent = env::temp_dir().join(format!("whowns-sdkman-chain-{}", std::process::id()));
        fs::create_dir_all(&parent).unwrap();
        let sdkman_root = parent.join(".sdkman");
        let _ = fs::remove_file(&sdkman_root);
        symlink(
            "/home/me/.local/share/mise/installs/sdkman/5.18.2",
            &sdkman_root,
        )
        .unwrap();

        let upstream = source_from_root(
            OwnerId::Sdkman,
            "sdk",
            Some(sdkman_root.clone()),
            &CommandRunner::new(),
            &detect::ManagerRoots::for_home(Path::new("/home/me")),
        );

        assert_eq!(upstream.node.id, OwnerId::Mise);
        assert_eq!(
            upstream.node.actions.inspect.as_deref(),
            Some("mise which sdk")
        );
        fs::remove_file(sdkman_root).unwrap();
        fs::remove_dir(parent).unwrap();
    }
}
