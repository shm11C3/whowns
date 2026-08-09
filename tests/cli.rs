#![cfg(unix)]

use std::env;
use std::fs;
use std::os::unix::fs::{PermissionsExt, symlink};
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicUsize, Ordering};

const WHOWNS: &str = env!("CARGO_BIN_EXE_whowns");

static NEXT_SANDBOX: AtomicUsize = AtomicUsize::new(0);

struct Sandbox {
    root: PathBuf,
}

impl Sandbox {
    fn new(name: &str) -> Self {
        let sequence = NEXT_SANDBOX.fetch_add(1, Ordering::Relaxed);
        let root = env::temp_dir().join(format!(
            "whowns-cli-{name}-{}-{sequence}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        Self { root }
    }

    fn path(&self, relative: impl AsRef<Path>) -> PathBuf {
        self.root.join(relative)
    }

    fn executable(&self, relative: impl AsRef<Path>, contents: &str) -> PathBuf {
        let path = self.path(relative);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, contents).unwrap();
        let mut permissions = fs::metadata(&path).unwrap().permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(&path, permissions).unwrap();
        path
    }

    fn run(&self, binary: &str, arguments: &[&str], path_entries: &[&Path]) -> Output {
        let path = env::join_paths(path_entries).unwrap();
        Command::new(binary)
            .args(arguments)
            .current_dir(&self.root)
            .env("HOME", &self.root)
            .env("PATH", path)
            .env_remove("NVM_DIR")
            .env_remove("SDKMAN_DIR")
            .output()
            .unwrap()
    }
}

impl Drop for Sandbox {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

fn stdout(output: &Output) -> String {
    String::from_utf8(output.stdout.clone()).unwrap()
}

fn stderr(output: &Output) -> String {
    String::from_utf8(output.stderr.clone()).unwrap()
}

#[test]
fn reports_active_and_shadowed_executables_in_path_order() {
    let sandbox = Sandbox::new("path-order");
    let first = sandbox.path("first");
    let second = sandbox.path("second");
    sandbox.executable("first/fixture-tool", "#!/bin/sh\nexit 0\n");
    sandbox.executable("second/fixture-tool", "#!/bin/sh\nexit 0\n");

    let output = sandbox.run(WHOWNS, &["fixture-tool"], &[&first, &second]);
    let stdout = stdout(&output);

    assert!(output.status.success());
    assert!(stdout.contains(&format!("active: {}/fixture-tool", first.display())));
    assert!(stdout.contains(&format!("shadowed: {}/fixture-tool", second.display())));
    assert!(stderr(&output).is_empty());
}

#[test]
fn explains_a_runtime_manager_and_the_managers_installation_source() {
    let sandbox = Sandbox::new("ownership-chain");
    let managed_nvm = sandbox.path(".local/share/mise/installs/nvm/0.40.3");
    sandbox.executable(
        ".local/share/mise/installs/nvm/0.40.3/versions/node/v22.3.0/bin/node",
        "#!/bin/sh\nexit 0\n",
    );
    symlink(&managed_nvm, sandbox.path(".nvm")).unwrap();
    let selected_bin = sandbox.path(".nvm/versions/node/v22.3.0/bin");

    let output = sandbox.run(WHOWNS, &["node", "--explain"], &[&selected_bin]);
    let stdout = stdout(&output);

    assert!(output.status.success());
    assert!(stdout.contains("ownership: node -> nvm [confirmed] -> mise [confirmed]"));
    assert!(stdout.contains("owner[1]: nvm (version_manager, confirmed)"));
    assert!(stdout.contains("owner[2]: mise (version_manager, confirmed)"));
}

#[test]
fn leaves_an_unclaimed_executable_unknown_without_mutating_actions() {
    let sandbox = Sandbox::new("unknown-owner");
    let bin = sandbox.path("local/bin");
    sandbox.executable("local/bin/custom-tool", "#!/bin/sh\nexit 0\n");

    let output = sandbox.run(WHOWNS, &["custom-tool", "--explain"], &[&bin]);
    let stdout = stdout(&output);

    assert!(output.status.success());
    assert!(stdout.contains("unconfirmed owner [unknown]"));
    assert!(stdout.contains("no recognized manager path"));
    assert!(!stdout.contains("update:"));
    assert!(!stdout.contains("remove:"));
}

#[test]
fn emits_the_common_ownership_graph_as_json() {
    let sandbox = Sandbox::new("json");
    let cellar_node = sandbox.executable("Cellar/node/25.0.0/bin/node", "#!/bin/sh\nexit 0\n");
    let bin = sandbox.path("bin");
    fs::create_dir_all(&bin).unwrap();
    symlink(&cellar_node, bin.join("node")).unwrap();

    let output = sandbox.run(WHOWNS, &["node", "--json"], &[&bin]);
    let stdout = stdout(&output);

    assert!(output.status.success());
    assert!(stdout.starts_with("[\n"));
    assert!(stdout.contains("\"command\": \"node\""));
    assert!(stdout.contains("\"status\": \"active\""));
    assert!(stdout.contains("\"id\": \"homebrew\""));
    assert!(stdout.contains("\"name\": \"Homebrew\""));
    assert!(stdout.contains("\"confidence\": \"confirmed\""));
    assert!(stdout.contains("\"action_guide\""));
    assert!(stderr(&output).is_empty());
}

#[test]
fn uses_distinct_exit_codes_and_output_streams_for_user_errors() {
    let sandbox = Sandbox::new("exit-codes");
    let empty_bin = sandbox.path("empty-bin");
    fs::create_dir_all(&empty_bin).unwrap();

    let missing = sandbox.run(WHOWNS, &["missing-tool"], &[&empty_bin]);
    assert_eq!(missing.status.code(), Some(1));
    assert!(stdout(&missing).contains("not found in PATH"));
    assert!(stderr(&missing).is_empty());

    let invalid = sandbox.run(WHOWNS, &["--not-an-option"], &[&empty_bin]);
    assert_eq!(invalid.status.code(), Some(2));
    assert!(stdout(&invalid).is_empty());
    assert!(stderr(&invalid).contains("error: unknown option: --not-an-option"));
    assert!(stderr(&invalid).contains("USAGE:"));
}

#[test]
fn all_mode_reuses_the_individual_diagnostic_model() {
    let sandbox = Sandbox::new("all-mode");
    let bin = sandbox.path("bin");
    let cellar_node = sandbox.executable("Cellar/node/25.0.0/bin/node", "#!/bin/sh\nexit 0\n");
    fs::create_dir_all(&bin).unwrap();
    symlink(&cellar_node, bin.join("node")).unwrap();

    let individual = sandbox.run(WHOWNS, &["node", "--json"], &[&bin]);
    let all = sandbox.run(WHOWNS, &["--all", "--json"], &[&bin]);

    assert_eq!(individual.status.code(), all.status.code());
    assert_eq!(individual.stdout, all.stdout);
    assert_eq!(individual.stderr, all.stderr);
}

#[cfg(target_os = "macos")]
#[test]
fn reads_a_macos_installer_receipt_through_pkgutil() {
    let sandbox = Sandbox::new("macos-receipt");
    let bin = sandbox.path("bin");
    let tools = sandbox.path("tools");
    sandbox.executable("bin/fixture-tool", "#!/bin/sh\nexit 0\n");
    sandbox.executable(
        "tools/pkgutil",
        "#!/bin/sh\n\
         if [ \"$1\" = \"--file-info\" ]; then\n\
           printf '%s\\n' 'pkgid: com.example.fixture' 'pkg-version: 1.2.3'\n\
           exit 0\n\
         fi\n\
         exit 1\n",
    );

    let output = sandbox.run(WHOWNS, &["fixture-tool", "--explain"], &[&bin, &tools]);
    let stdout = stdout(&output);

    assert!(output.status.success());
    assert!(stdout.contains("macOS Installer (.pkg) [confirmed]"));
    assert!(stdout.contains("package: com.example.fixture"));
    assert!(stdout.contains("version: 1.2.3"));
    assert!(stdout.contains("pkgutil --pkg-info com.example.fixture"));
}

#[cfg(target_os = "linux")]
fn assert_linux_package_query(
    query_program: &str,
    query_result: &str,
    expected_owner: &str,
    expected_guide: &str,
) {
    let sandbox = Sandbox::new(query_program);
    let bin = sandbox.path("bin");
    let tools = sandbox.path("tools");
    sandbox.executable("bin/fixture-tool", "#!/bin/sh\nexit 0\n");
    sandbox.executable(
        format!("tools/{query_program}"),
        &format!("#!/bin/sh\nprintf '%s\\n' '{query_result}'\n"),
    );

    let output = sandbox.run(WHOWNS, &["fixture-tool", "--explain"], &[&bin, &tools]);
    let stdout = stdout(&output);

    assert!(output.status.success());
    assert!(stdout.contains(expected_owner), "output: {stdout}");
    assert!(stdout.contains(expected_guide), "output: {stdout}");
    assert!(stdout.contains("evidence[package query]"));
}

#[cfg(target_os = "linux")]
#[test]
fn reads_dpkg_package_ownership() {
    assert_linux_package_query(
        "dpkg-query",
        "fixture-package: /fixture-tool",
        "dpkg (package_manager, confirmed)",
        "apt install --only-upgrade fixture-package",
    );
}

#[cfg(target_os = "linux")]
#[test]
fn reads_rpm_package_ownership() {
    assert_linux_package_query(
        "rpm",
        "fixture-package-1.2.3-1.x86_64",
        "RPM (package_manager, confirmed)",
        "Use the system's RPM frontend",
    );
}

#[cfg(target_os = "linux")]
#[test]
fn reads_pacman_package_ownership() {
    assert_linux_package_query(
        "pacman",
        "/fixture-tool is owned by fixture-package 1.2.3-1",
        "pacman (package_manager, confirmed)",
        "inspect: pacman -Qo",
    );
}

#[cfg(target_os = "linux")]
#[test]
fn reads_apk_package_ownership() {
    assert_linux_package_query(
        "apk",
        "/fixture-tool is owned by fixture-package-1.2.3-r0",
        "apk (package_manager, confirmed)",
        "inspect: apk info -W",
    );
}
