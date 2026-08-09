use std::collections::HashSet;
use std::env;
use std::fs;
use std::path::{Path, PathBuf};

#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;

pub const KNOWN_RUNTIMES: &[&str] = &[
    "node", "npm", "pnpm", "deno", "bun", "python3", "pip3", "ruby", "gem", "java", "javac", "go",
    "rustc", "cargo", "php", "dotnet", "flutter", "dart",
];

#[derive(Debug, Eq, PartialEq)]
pub struct ResolvedExecutable {
    pub path: PathBuf,
    pub real_path: PathBuf,
    pub active: bool,
}

pub fn find_executables(command: &str) -> Vec<ResolvedExecutable> {
    find_executables_in(command, env::var_os("PATH").as_deref())
}

fn find_executables_in(
    command: &str,
    path_env: Option<&std::ffi::OsStr>,
) -> Vec<ResolvedExecutable> {
    let requested = Path::new(command);
    if requested.components().count() > 1 {
        return executable(requested)
            .then(|| resolved(requested.to_path_buf(), true))
            .into_iter()
            .collect();
    }

    let mut seen = HashSet::new();
    let mut matches = Vec::new();
    for directory in path_env.into_iter().flat_map(env::split_paths) {
        let candidate = directory.join(command);
        if !seen.insert(candidate.clone()) || !executable(&candidate) {
            continue;
        }

        matches.push(resolved(candidate, matches.is_empty()));
    }
    matches
}

fn resolved(path: PathBuf, active: bool) -> ResolvedExecutable {
    let real_path = fs::canonicalize(&path).unwrap_or_else(|_| path.clone());
    ResolvedExecutable {
        path,
        real_path,
        active,
    }
}

fn executable(path: &Path) -> bool {
    let Ok(metadata) = fs::metadata(path) else {
        return false;
    };
    if !metadata.is_file() {
        return false;
    }

    #[cfg(unix)]
    {
        metadata.permissions().mode() & 0o111 != 0
    }
    #[cfg(not(unix))]
    {
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs::{self, File};

    #[test]
    fn finds_active_and_shadowed_executables_in_path_order() {
        let root = std::env::temp_dir().join(format!("whowns-scan-{}", std::process::id()));
        let first = root.join("first");
        let second = root.join("second");
        fs::create_dir_all(&first).unwrap();
        fs::create_dir_all(&second).unwrap();
        let first_node = first.join("node");
        let second_node = second.join("node");
        File::create(&first_node).unwrap();
        File::create(&second_node).unwrap();
        #[cfg(unix)]
        for path in [&first_node, &second_node] {
            let mut permissions = fs::metadata(path).unwrap().permissions();
            permissions.set_mode(0o755);
            fs::set_permissions(path, permissions).unwrap();
        }

        let joined = env::join_paths([&first, &second, &first]).unwrap();
        let found = find_executables_in("node", Some(&joined));

        assert_eq!(found.len(), 2);
        assert!(found[0].active);
        assert!(!found[1].active);
        assert_eq!(found[0].path, first_node);
        assert_eq!(found[1].path, second_node);

        fs::remove_dir_all(root).unwrap();
    }
}
