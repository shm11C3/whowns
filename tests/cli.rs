#![cfg(unix)]

use std::env;
use std::fs;
use std::os::unix::fs::{PermissionsExt, symlink};
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};

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
    assert!(stdout.contains("├── ● active"));
    assert!(stdout.contains(&format!("executable: {}/fixture-tool", first.display())));
    assert!(stdout.contains("└── ○ shadowed"));
    assert!(stdout.contains(&format!("executable: {}/fixture-tool", second.display())));
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
    assert!(stdout.contains("ownership: node → nvm [probable] → mise [probable]"));
    assert!(stdout.contains("├── nvm [probable]"));
    assert!(stdout.contains("├── mise [probable]"));
    assert!(stdout.contains("└── unconfirmed source [unknown]"));
    assert!(stdout.contains("kind: version_manager"));
}

#[test]
fn resolves_more_than_two_ownership_nodes_in_nearest_first_order() {
    let sandbox = Sandbox::new("deep-ownership-chain");
    let managed_nvm = sandbox.path(".local/share/mise/installs/nvm/0.40.3");
    sandbox.executable(
        ".local/share/mise/installs/nvm/0.40.3/versions/node/v22.3.0/bin/node",
        "#!/bin/sh\nexit 0\n",
    );
    symlink(&managed_nvm, sandbox.path(".nvm")).unwrap();
    let selected_bin = sandbox.path(".nvm/versions/node/v22.3.0/bin");

    let real_managed_nvm = fs::canonicalize(&managed_nvm).unwrap();
    let homebrew_mise = sandbox.executable(
        "homebrew/Cellar/mise/2026.8.0/bin/mise",
        &format!(
            "#!/bin/sh\nif [ \"$1\" = \"which\" ] && [ \"$2\" = \"nvm\" ]; then\n  printf '%s\\n' '{}'\nfi\n",
            real_managed_nvm.display()
        ),
    );
    let tools = sandbox.path("tools");
    fs::create_dir_all(&tools).unwrap();
    symlink(&homebrew_mise, tools.join("mise")).unwrap();

    let output = sandbox.run(WHOWNS, &["node", "--explain"], &[&selected_bin, &tools]);
    let stdout = stdout(&output);

    assert!(output.status.success());
    assert!(
        stdout
            .contains("ownership: node → nvm [probable] → mise [confirmed] → Homebrew [probable]"),
        "output: {stdout}"
    );
    assert!(
        stdout.contains("`mise which nvm` returned")
            && stdout.contains("and it matches the resolved executable"),
        "output: {stdout}"
    );
    assert!(stderr(&output).is_empty());
}

#[test]
fn stops_an_ownership_cycle_with_visible_evidence() {
    let sandbox = Sandbox::new("ownership-cycle");
    let managed_nvm = sandbox.path(".local/share/mise/installs/nvm/0.40.3");
    sandbox.executable(
        ".local/share/mise/installs/nvm/0.40.3/versions/node/v22.3.0/bin/node",
        "#!/bin/sh\nexit 0\n",
    );
    symlink(&managed_nvm, sandbox.path(".nvm")).unwrap();
    let selected_bin = sandbox.path(".nvm/versions/node/v22.3.0/bin");

    let tools = sandbox.path("tools");
    let managed_mise = sandbox.executable(
        ".local/share/mise/installs/mise/2026.8.0/bin/mise",
        "#!/bin/sh\nexit 0\n",
    );
    fs::create_dir_all(&tools).unwrap();
    symlink(&managed_mise, tools.join("mise")).unwrap();

    let output = sandbox.run(WHOWNS, &["node", "--explain"], &[&selected_bin, &tools]);
    let stdout = stdout(&output);

    assert!(output.status.success());
    assert!(
        stdout.contains("unconfirmed source [unknown]"),
        "output: {stdout}"
    );
    assert!(
        stdout.contains("ownership cycle detected"),
        "output: {stdout}"
    );
    assert!(stderr(&output).is_empty());
}

#[test]
fn stops_at_the_maximum_depth_with_visible_evidence() {
    let sandbox = Sandbox::new("ownership-depth-limit");
    let managed_nvm = sandbox.path(".local/share/mise/installs/nvm/0.40.3");
    sandbox.executable(
        ".local/share/mise/installs/nvm/0.40.3/versions/node/v22.3.0/bin/node",
        "#!/bin/sh\nexit 0\n",
    );
    symlink(&managed_nvm, sandbox.path(".nvm")).unwrap();
    let selected_bin = sandbox.path(".nvm/versions/node/v22.3.0/bin");

    let tools = sandbox.path("tools");
    fs::create_dir_all(&tools).unwrap();
    for (command, target) in [
        ("mise", ".asdf/installs/mise/2026.8.0/bin/mise"),
        ("asdf", ".pyenv/versions/3.14/bin/asdf"),
        ("pyenv", ".rbenv/versions/4.0/bin/pyenv"),
        ("rbenv", ".volta/tools/image/rbenv/4.0/bin/rbenv"),
        ("volta", ".local/share/fnm/node-versions/v24/bin/volta"),
        ("fnm", ".local/share/uv/python/cpython-3.14/bin/fnm"),
    ] {
        let target = sandbox.executable(target, "#!/bin/sh\nexit 0\n");
        symlink(target, tools.join(command)).unwrap();
    }

    let output = sandbox.run(WHOWNS, &["node", "--explain"], &[&selected_bin, &tools]);
    let stdout = stdout(&output);

    assert!(output.status.success());
    assert!(
        stdout.contains("ownership resolution reached the safety limit of 8 owners"),
        "output: {stdout}"
    );
    assert!(stderr(&output).is_empty(), "stderr: {}", stderr(&output));
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
    assert!(stdout.contains("\"confidence\": \"probable\""));
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

#[test]
fn confirms_a_mise_managed_runtime_via_its_which_query() {
    let sandbox = Sandbox::new("mise-which-query");
    let node = sandbox.executable(
        ".local/share/mise/installs/node/22.3.0/bin/node",
        "#!/bin/sh\nexit 0\n",
    );
    let node_bin = node.parent().unwrap().to_path_buf();
    // whowns compares the query result against the runtime's canonicalized
    // real path, so the fixture must answer with that same canonical form.
    let real_node = fs::canonicalize(&node).unwrap();
    let tools = sandbox.path("tools");
    sandbox.executable(
        "tools/mise",
        &format!(
            "#!/bin/sh\nif [ \"$1\" = \"which\" ]; then\n  printf '%s\\n' '{}'\nfi\n",
            real_node.display()
        ),
    );

    let output = sandbox.run(WHOWNS, &["node", "--explain"], &[&node_bin, &tools]);
    let stdout = stdout(&output);

    assert!(output.status.success());
    assert!(
        stdout.contains("mise which node` returned"),
        "output: {stdout}"
    );
    assert!(
        stdout.contains("and it matches the resolved executable"),
        "output: {stdout}"
    );
    assert!(stdout.contains("mise [confirmed]"), "output: {stdout}");
    assert!(stderr(&output).is_empty());
}

#[test]
fn rejects_truncated_manager_query_output_instead_of_using_a_partial_value() {
    // A manager that misbehaves and floods stdout must not have that
    // truncated 64-KiB prefix treated as a real path/version and enriched
    // into ownership evidence or generated action commands.
    let sandbox = Sandbox::new("truncated-manager-query");
    let node = sandbox.executable(
        ".local/share/mise/installs/node/22.3.0/bin/node",
        "#!/bin/sh\nexit 0\n",
    );
    let node_bin = node.parent().unwrap().to_path_buf();
    let tools = sandbox.path("tools");
    sandbox.executable(
        "tools/mise",
        "#!/bin/sh\nif [ \"$1\" = \"which\" ]; then\n  /usr/bin/yes | /usr/bin/head -c 200000\nfi\n",
    );

    let output = sandbox.run(WHOWNS, &["node", "--explain"], &[&node_bin, &tools]);
    let stdout = stdout(&output);

    assert!(output.status.success());
    assert!(
        stdout.contains("mise which node` output was truncated; ignoring it"),
        "output: {stdout}"
    );
    assert!(
        !stdout.contains("returned `"),
        "a truncated result must not be reported as a returned value, output: {stdout}"
    );
}

#[test]
fn skips_the_manager_query_silently_when_mise_itself_is_not_on_path() {
    // The sandbox PATH deliberately has no `mise` executable anywhere on it,
    // regardless of what happens to be installed on the host running this
    // test. resolve_program("mise") must fail before any subprocess is
    // attempted, and enrich_with_manager_query must not add evidence or fail
    // for that.
    let sandbox = Sandbox::new("mise-not-on-path");
    let node = sandbox.executable(
        ".local/share/mise/installs/node/22.3.0/bin/node",
        "#!/bin/sh\nexit 0\n",
    );
    let node_bin = node.parent().unwrap().to_path_buf();

    let output = sandbox.run(WHOWNS, &["node", "--explain"], &[&node_bin]);
    let stdout = stdout(&output);

    assert!(output.status.success());
    assert!(stdout.contains("mise [probable]"), "output: {stdout}");
    assert!(
        !stdout.contains("manager query"),
        "no manager query should have been attempted, output: {stdout}"
    );
    assert!(stderr(&output).is_empty());
}

#[test]
fn a_hung_manager_query_is_killed_and_reported_instead_of_blocking() {
    let sandbox = Sandbox::new("hung-manager-query");
    let node = sandbox.executable(
        ".local/share/mise/installs/node/22.3.0/bin/node",
        "#!/bin/sh\nexit 0\n",
    );
    let node_bin = node.parent().unwrap().to_path_buf();
    let tools = sandbox.path("tools");
    // Simulates a manager that hangs instead of answering; the runner must
    // kill it after its timeout rather than block whowns indefinitely. The
    // sandbox PATH only contains fixture directories, so `sleep` is called
    // by its absolute path rather than relying on PATH to find it.
    sandbox.executable("tools/mise", "#!/bin/sh\n/bin/sleep 30\n");

    let started = Instant::now();
    let output = sandbox.run(WHOWNS, &["node", "--explain"], &[&node_bin, &tools]);
    let stdout = stdout(&output);
    let stderr = stderr(&output);

    // Bounds this against actually blocking on the 30-second sleep, not just
    // against the process eventually returning: a regression that dropped
    // the timeout back to an unconditional wait would still exit 0
    // eventually, just 30 seconds later.
    assert!(
        started.elapsed() < Duration::from_secs(10),
        "a hung manager query should not block whowns for anywhere near its 30-second sleep, elapsed: {:?}",
        started.elapsed()
    );
    assert!(output.status.success());
    assert!(
        stdout.contains("`mise which node` did not finish within the timeout and was killed"),
        "output: {stdout}"
    );
    assert!(
        stderr.contains("mise which node")
            && stderr.contains("did not finish within the timeout and was killed"),
        "stderr: {stderr}"
    );
}

#[cfg(target_os = "macos")]
#[test]
fn caches_identical_manager_queries_across_runtimes_in_all_mode() {
    let sandbox = Sandbox::new("query-cache");
    let node = sandbox.executable(
        ".local/share/mise/installs/node/22.3.0/bin/node",
        "#!/bin/sh\nexit 0\n",
    );
    let node_bin = node.parent().unwrap().to_path_buf();
    let python = sandbox.executable(
        ".local/share/mise/installs/python/3.12.4/bin/python3",
        "#!/bin/sh\nexit 0\n",
    );
    let python_bin = python.parent().unwrap().to_path_buf();
    // Neither runtime nor mise itself resolves via `which`, so the only
    // subprocess in play is the upstream `pkgutil --file-info <mise>` lookup
    // that both node's and python3's upstream-owner resolution need.
    let manager = sandbox.executable("manager/mise", "#!/bin/sh\nexit 1\n");
    let manager_bin = manager.parent().unwrap().to_path_buf();
    let tools = sandbox.path("tools");
    let log = sandbox.path("pkgutil-calls.log");
    sandbox.executable(
        "tools/pkgutil",
        &format!(
            "#!/bin/sh\necho called >> '{}'\nif [ \"$1\" = \"--file-info\" ]; then\n  printf '%s\\n' 'pkgid: com.example.mise' 'pkg-version: 1.0.0'\n  exit 0\nfi\nexit 1\n",
            log.display()
        ),
    );

    let output = sandbox.run(
        WHOWNS,
        &["--all", "--explain"],
        &[&node_bin, &python_bin, &manager_bin, &tools],
    );
    let stdout = stdout(&output);

    assert!(output.status.success());
    assert_eq!(
        stdout
            .matches("└── macOS Installer (.pkg) [confirmed]")
            .count(),
        2,
        "expected one macOS Installer owner per runtime, output: {stdout}"
    );
    let calls = fs::read_to_string(&log).unwrap_or_default();
    assert_eq!(
        calls.lines().count(),
        1,
        "pkgutil should be queried once and cached across runtimes; log: {calls}"
    );
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
    assert!(stdout.contains("kind: package_manager"), "output: {stdout}");
    assert!(stdout.contains(expected_guide), "output: {stdout}");
    assert!(stdout.contains("package query:"));
}

#[cfg(target_os = "linux")]
#[test]
fn reads_dpkg_package_ownership() {
    assert_linux_package_query(
        "dpkg-query",
        "fixture-package: /fixture-tool",
        "dpkg [confirmed]",
        "apt install --only-upgrade fixture-package",
    );
}

#[cfg(target_os = "linux")]
#[test]
fn reads_rpm_package_ownership() {
    assert_linux_package_query(
        "rpm",
        "fixture-package-1.2.3-1.x86_64",
        "RPM [confirmed]",
        "Use the system's RPM frontend",
    );
}

#[cfg(target_os = "linux")]
#[test]
fn reads_pacman_package_ownership() {
    assert_linux_package_query(
        "pacman",
        "/fixture-tool is owned by fixture-package 1.2.3-1",
        "pacman [confirmed]",
        "inspect: pacman -Qo",
    );
}

#[cfg(target_os = "linux")]
#[test]
fn reads_apk_package_ownership() {
    assert_linux_package_query(
        "apk",
        "/fixture-tool is owned by fixture-package-1.2.3-r0",
        "apk [confirmed]",
        "inspect: apk info -W",
    );
}

#[cfg(target_os = "linux")]
#[test]
fn continues_to_the_next_package_tool_when_one_reports_success_with_empty_output() {
    // dpkg-query exits 0 (as it would with no output at all) without
    // printing anything; that must not abort the whole detector before rpm
    // gets a chance to answer.
    let sandbox = Sandbox::new("empty-then-rpm");
    let bin = sandbox.path("bin");
    let tools = sandbox.path("tools");
    sandbox.executable("bin/fixture-tool", "#!/bin/sh\nexit 0\n");
    sandbox.executable("tools/dpkg-query", "#!/bin/sh\nexit 0\n");
    sandbox.executable(
        "tools/rpm",
        "#!/bin/sh\nprintf '%s\\n' 'fixture-package-1.2.3-1.x86_64'\n",
    );

    let output = sandbox.run(WHOWNS, &["fixture-tool", "--explain"], &[&bin, &tools]);
    let stdout = stdout(&output);

    assert!(output.status.success());
    assert!(stdout.contains("RPM [confirmed]"), "output: {stdout}");
}
