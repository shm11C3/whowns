use std::cell::RefCell;
use std::collections::HashMap;
use std::io::{ErrorKind, Read};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

/// Ceiling on how long a single external query may run. `whowns` only ever
/// invokes read-only inspection subcommands (`which`, `current`,
/// `--file-info`, ...); none of them should legitimately take longer than
/// this, and a hung one must not block the rest of the run.
pub const DEFAULT_TIMEOUT: Duration = Duration::from_secs(3);

/// Captured stdout beyond this size is discarded. Ownership detection only
/// ever needs a path or a short version string out of these queries.
pub const MAX_OUTPUT_BYTES: usize = 64 * 1024;

const POLL_INTERVAL: Duration = Duration::from_millis(5);
const READ_CHUNK_SIZE: usize = 8 * 1024;

/// Output of a completed external query, bounded to `MAX_OUTPUT_BYTES`.
#[derive(Clone, Debug)]
pub struct QueryOutput {
    pub success: bool,
    pub stdout: String,
    pub truncated: bool,
}

/// Why an external query produced no usable output.
#[derive(Clone, Debug)]
pub enum QueryFailure {
    /// The process did not exit before the timeout and was killed.
    Timeout,
    /// The process could not be started at all.
    SpawnFailed(String),
}

impl QueryFailure {
    pub fn describe(&self) -> String {
        match self {
            Self::Timeout => "did not finish within the timeout and was killed".to_owned(),
            Self::SpawnFailed(reason) => format!("could not be started: {reason}"),
        }
    }
}

pub type QueryResult = Result<QueryOutput, QueryFailure>;

#[derive(Eq, Hash, PartialEq)]
struct CacheKey {
    program: PathBuf,
    arguments: Vec<String>,
}

/// Runs read-only inspection commands for the duration of one `whowns`
/// invocation. Every external process a detector needs goes through here, so
/// timeout, output-size, and repeat-query policy are enforced in one place
/// instead of at each call site.
///
/// Queries inherit the parent process environment unmodified. Managers such
/// as mise, asdf, or rustup resolve their own data directories from
/// environment variables (`HOME`, `MISE_DATA_DIR`, ...); clearing or
/// fabricating environment state for them would make their answers wrong
/// rather than safer.
pub struct CommandRunner {
    timeout: Duration,
    cache: RefCell<HashMap<CacheKey, QueryResult>>,
    diagnostics: RefCell<Vec<String>>,
}

impl CommandRunner {
    pub fn new() -> Self {
        Self::with_timeout(DEFAULT_TIMEOUT)
    }

    pub fn with_timeout(timeout: Duration) -> Self {
        Self {
            timeout,
            cache: RefCell::new(HashMap::new()),
            diagnostics: RefCell::new(Vec::new()),
        }
    }

    /// Runs `program arguments...` with no stdin and bounded stdout capture.
    /// An identical `(program, arguments)` query is only ever executed once
    /// per `CommandRunner`; later calls return the cached result.
    pub fn query(&self, program: &Path, arguments: &[&str]) -> QueryResult {
        let key = CacheKey {
            program: program.to_path_buf(),
            arguments: arguments
                .iter()
                .map(|argument| (*argument).to_owned())
                .collect(),
        };
        if let Some(cached) = self.cache.borrow().get(&key) {
            return cached.clone();
        }

        let result = run_with_timeout(program, arguments, self.timeout);
        self.record_diagnostics(program, arguments, &result);
        self.cache.borrow_mut().insert(key, result.clone());
        result
    }

    /// Failures and truncations recorded so far, in execution order.
    /// `whowns` prints these as notes so a timeout or a bounded capture stays
    /// visible even when the affected detector had no owner node to attach
    /// evidence to.
    pub fn diagnostics(&self) -> Vec<String> {
        self.diagnostics.borrow().clone()
    }

    fn record_diagnostics(&self, program: &Path, arguments: &[&str], result: &QueryResult) {
        let invocation = format!("`{} {}`", program.display(), arguments.join(" "));
        match result {
            Err(failure) => self
                .diagnostics
                .borrow_mut()
                .push(format!("{invocation} {}", failure.describe())),
            Ok(output) if output.truncated => self.diagnostics.borrow_mut().push(format!(
                "{invocation} output was truncated to {MAX_OUTPUT_BYTES} bytes"
            )),
            Ok(_) => {}
        }
    }
}

impl Default for CommandRunner {
    fn default() -> Self {
        Self::new()
    }
}

fn run_with_timeout(program: &Path, arguments: &[&str], timeout: Duration) -> QueryResult {
    let mut command = Command::new(program);
    command
        .args(arguments)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());

    let mut child = match command.spawn() {
        Ok(child) => child,
        Err(error) => return Err(QueryFailure::SpawnFailed(error.to_string())),
    };

    // Drain stdout concurrently with waiting: a child that writes more than
    // one pipe buffer of output would otherwise block on write() forever if
    // nobody reads it, turning "slow" into "hung" for reasons unrelated to
    // the timeout below.
    let mut stdout = child.stdout.take().expect("stdout was requested as piped");
    let reader = thread::spawn(move || read_bounded(&mut stdout));

    // std has no `Child::wait_timeout`; poll instead of blocking so a hung
    // child can be killed rather than waited on indefinitely.
    let deadline = Instant::now() + timeout;
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break Some(status),
            Ok(None) if Instant::now() < deadline => thread::sleep(POLL_INTERVAL),
            Ok(None) | Err(_) => break None,
        }
    };

    let Some(status) = status else {
        let _ = child.kill();
        let _ = child.wait();
        // Deliberately not joining `reader` here: a killed shell-script
        // manager can leave a grandchild process (its own forked worker)
        // holding the stdout pipe open long after the direct child is gone,
        // which would make the join block for as long as that grandchild
        // keeps running. Its output is moot on a timeout anyway; dropping
        // the handle lets the thread finish in the background instead of
        // turning "the manager hung" into "whowns hangs too".
        return Err(QueryFailure::Timeout);
    };

    let (stdout, truncated) = reader.join().unwrap_or_default();
    Ok(QueryOutput {
        success: status.success(),
        stdout: String::from_utf8_lossy(&stdout).into_owned(),
        truncated,
    })
}

/// Drains `source` to EOF so a still-writing child never blocks on a full
/// pipe, but only keeps the first `MAX_OUTPUT_BYTES` of it.
fn read_bounded(source: &mut impl Read) -> (Vec<u8>, bool) {
    let mut buffer = Vec::new();
    let mut truncated = false;
    let mut chunk = [0u8; READ_CHUNK_SIZE];
    loop {
        let read = match source.read(&mut chunk) {
            Ok(0) => break,
            Ok(n) => n,
            Err(error) if error.kind() == ErrorKind::Interrupted => continue,
            Err(_) => break,
        };
        if buffer.len() < MAX_OUTPUT_BYTES {
            let take = read.min(MAX_OUTPUT_BYTES - buffer.len());
            buffer.extend_from_slice(&chunk[..take]);
            truncated |= take < read;
        } else {
            truncated = true;
        }
    }
    (buffer, truncated)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn runs_a_command_and_captures_stdout() {
        let runner = CommandRunner::new();
        let output = runner.query(Path::new("printf"), &["hello"]).unwrap();
        assert!(output.success);
        assert_eq!(output.stdout, "hello");
        assert!(!output.truncated);
        assert!(runner.diagnostics().is_empty());
    }

    #[test]
    fn reports_spawn_failure_for_a_program_that_does_not_exist() {
        let runner = CommandRunner::new();
        let result = runner.query(Path::new("whowns-fixture-does-not-exist"), &[]);
        assert!(matches!(result, Err(QueryFailure::SpawnFailed(_))));
        assert_eq!(runner.diagnostics().len(), 1);
    }

    #[test]
    fn kills_a_hung_process_instead_of_blocking_indefinitely() {
        let runner = CommandRunner::with_timeout(Duration::from_millis(50));
        let started = Instant::now();

        let result = runner.query(Path::new("sleep"), &["30"]);

        assert!(matches!(result, Err(QueryFailure::Timeout)));
        assert!(
            started.elapsed() < Duration::from_secs(5),
            "a killed process should not make the caller wait for the full sleep"
        );
        assert_eq!(runner.diagnostics().len(), 1);
    }

    #[test]
    fn kills_a_shell_wrapper_without_waiting_on_its_orphaned_grandchild() {
        // A shell-script "manager" forks sleep as a grandchild that inherits
        // the piped stdout fd. Killing only the direct child (the shell)
        // leaves that grandchild running and still holding the pipe open;
        // the query must still return promptly rather than waiting for the
        // grandchild to finish its own 30-second sleep.
        let dir = std::env::temp_dir().join(format!("whowns-exec-orphan-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let script = dir.join("wrapper.sh");
        fs::write(&script, "#!/bin/sh\n/bin/sleep 30\n").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut permissions = fs::metadata(&script).unwrap().permissions();
            permissions.set_mode(0o755);
            fs::set_permissions(&script, permissions).unwrap();
        }

        let runner = CommandRunner::with_timeout(Duration::from_millis(100));
        let started = Instant::now();

        let result = runner.query(&script, &[]);

        assert!(matches!(result, Err(QueryFailure::Timeout)));
        assert!(
            started.elapsed() < Duration::from_secs(5),
            "the orphaned grandchild must not make the caller wait for its own sleep, elapsed: {:?}",
            started.elapsed()
        );

        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn identical_queries_run_at_most_once() {
        let dir = std::env::temp_dir().join(format!("whowns-exec-cache-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let script = dir.join("counter.sh");
        let log = dir.join("calls.log");
        fs::write(
            &script,
            format!("#!/bin/sh\necho called >> {}\necho ok\n", log.display()),
        )
        .unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut permissions = fs::metadata(&script).unwrap().permissions();
            permissions.set_mode(0o755);
            fs::set_permissions(&script, permissions).unwrap();
        }

        let runner = CommandRunner::new();
        let first = runner.query(&script, &[]).unwrap();
        let second = runner.query(&script, &[]).unwrap();

        assert_eq!(first.stdout, second.stdout);
        let calls = fs::read_to_string(&log).unwrap_or_default();
        assert_eq!(
            calls.lines().count(),
            1,
            "script should have run exactly once, log: {calls}"
        );

        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn stdout_capture_is_bounded_and_reports_truncation() {
        let runner = CommandRunner::new();

        let output = runner
            .query(Path::new("sh"), &["-c", "yes | head -c 200000"])
            .unwrap();

        assert!(output.stdout.len() <= MAX_OUTPUT_BYTES);
        assert!(output.truncated);
        assert_eq!(runner.diagnostics().len(), 1);
    }
}
