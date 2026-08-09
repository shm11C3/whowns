use std::cell::RefCell;
use std::collections::HashMap;
use std::ffi::{OsStr, OsString};
use std::io::{ErrorKind, Read};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::mpsc;
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
    /// True when the captured output may be incomplete: it hit the
    /// output-size cap, or a descendant process was still holding the
    /// stdout pipe open when this query's overall timeout elapsed.
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
    arguments: Vec<OsString>,
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
    ///
    /// Arguments are taken as `OsStr` rather than `str` so a path argument
    /// (which on Unix need not be valid UTF-8) reaches the child exactly as
    /// resolved; lossily re-encoding it first could point the query at a
    /// different path than the one `whowns` actually inspected.
    pub fn query(&self, program: &Path, arguments: &[&OsStr]) -> QueryResult {
        let key = CacheKey {
            program: program.to_path_buf(),
            arguments: arguments
                .iter()
                .map(|argument| (*argument).to_os_string())
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

    fn record_diagnostics(&self, program: &Path, arguments: &[&OsStr], result: &QueryResult) {
        // Lossy here is fine: this is a human-readable note, not the actual
        // argument handed to the child (that stays byte-exact in `query`).
        let rendered_arguments = arguments
            .iter()
            .map(|argument| argument.to_string_lossy())
            .collect::<Vec<_>>()
            .join(" ");
        let invocation = format!("`{} {rendered_arguments}`", program.display());
        match result {
            Err(failure) => self
                .diagnostics
                .borrow_mut()
                .push(format!("{invocation} {}", failure.describe())),
            Ok(output) if output.truncated => self
                .diagnostics
                .borrow_mut()
                .push(format!("{invocation} output capture was truncated")),
            Ok(_) => {}
        }
    }
}

impl Default for CommandRunner {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(unix)]
fn isolate_process_group(command: &mut Command) {
    use std::os::unix::process::CommandExt;
    // Give the child (and anything it forks) its own process group so a
    // timeout can terminate the whole tree via kill_process_group, not just
    // the direct child. Without this, killing the direct child leaves
    // anything it background-forked running as an orphan reparented to
    // init, which can leak indefinitely for a long-running or infinite job.
    command.process_group(0);
}

#[cfg(not(unix))]
fn isolate_process_group(_command: &mut Command) {}

#[cfg(unix)]
fn kill_process_group(pid: u32) {
    // Declaring `kill(2)` here does not add a crate dependency: it is part
    // of the platform C library every Rust binary already links against, so
    // this avoids pulling in the `libc` crate for one syscall. The negative
    // pid targets the whole process group `isolate_process_group` placed
    // this child in, not just the single process `Child::kill` would reach.
    unsafe extern "C" {
        fn kill(pid: i32, sig: i32) -> i32;
    }
    const SIGKILL: i32 = 9;
    unsafe {
        kill(-(pid as i32), SIGKILL);
    }
}

#[cfg(not(unix))]
fn kill_process_group(_pid: u32) {}

fn run_with_timeout(program: &Path, arguments: &[&OsStr], timeout: Duration) -> QueryResult {
    let mut command = Command::new(program);
    command
        .args(arguments)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    isolate_process_group(&mut command);

    let mut child = match command.spawn() {
        Ok(child) => child,
        Err(error) => return Err(QueryFailure::SpawnFailed(error.to_string())),
    };
    let pid = child.id();

    // Drain stdout concurrently with waiting: a child that writes more than
    // one pipe buffer of output would otherwise block on write() forever if
    // nobody reads it, turning "slow" into "hung" for reasons unrelated to
    // the timeout below. A channel (rather than a JoinHandle we `.join()`)
    // lets the wait for its result be bounded too, not just the wait for
    // exit: see the comment below on why an unconditional join is not safe.
    let mut stdout = child.stdout.take().expect("stdout was requested as piped");
    let (output_tx, output_rx) = mpsc::channel();
    thread::spawn(move || {
        let _ = output_tx.send(read_bounded(&mut stdout));
    });

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
        kill_process_group(pid);
        let _ = child.wait();
        // Deliberately not waiting on the reader here: a killed shell-script
        // manager can leave a grandchild process (its own forked worker)
        // holding the stdout pipe open long after the direct child is gone,
        // which would block waiting for it to finish. Its output is moot on
        // a timeout anyway; letting the reader thread finish in the
        // background avoids turning "the manager hung" into "whowns hangs
        // too".
        return Err(QueryFailure::Timeout);
    };

    // The direct child exited in time, but it may have backgrounded a
    // descendant that inherited the same stdout pipe and is still holding it
    // open (for example a manager script that runs `some-job & ; exit 0`).
    // Bound the wait for output by what remains of the same deadline instead
    // of joining unconditionally, so that case cannot outlast the timeout
    // either; whatever wasn't read in time is reported as truncated rather
    // than blocking on it. Any such descendant is also killed rather than
    // left to run past the deadline on its own.
    let (stdout, truncated) =
        match output_rx.recv_timeout(deadline.saturating_duration_since(Instant::now())) {
            Ok(result) => result,
            Err(_) => {
                kill_process_group(pid);
                (Vec::new(), true)
            }
        };
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
    use std::process::Command as StdCommand;

    #[test]
    fn runs_a_command_and_captures_stdout() {
        let runner = CommandRunner::new();
        let output = runner
            .query(Path::new("printf"), &[OsStr::new("hello")])
            .unwrap();
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

        let result = runner.query(Path::new("sleep"), &[OsStr::new("30")]);

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
    fn returns_promptly_when_a_successful_command_backgrounds_a_descendant() {
        // The manager exits successfully well within the timeout, but first
        // backgrounds a long-running job that inherits the piped stdout fd
        // and outlives it. Waiting for that descendant to close the pipe
        // would silently defeat the timeout on the success path even though
        // the direct child already reported its exit status.
        let dir =
            std::env::temp_dir().join(format!("whowns-exec-background-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let script = dir.join("backgrounder.sh");
        fs::write(&script, "#!/bin/sh\n/bin/sleep 20 &\nexit 0\n").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut permissions = fs::metadata(&script).unwrap().permissions();
            permissions.set_mode(0o755);
            fs::set_permissions(&script, permissions).unwrap();
        }

        // Generous relative to how fast the shell actually exits, but still
        // tiny relative to the 20-second background job, so the assertion
        // below stays meaningful without being flaky under CI load.
        let runner = CommandRunner::with_timeout(Duration::from_secs(3));
        let started = Instant::now();

        let output = runner.query(&script, &[]).unwrap();

        assert!(output.success);
        assert!(
            started.elapsed() < Duration::from_secs(10),
            "a successful exit must not wait on a backgrounded descendant, elapsed: {:?}",
            started.elapsed()
        );

        fs::remove_dir_all(&dir).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn kills_the_whole_process_group_not_just_the_direct_child() {
        // The shell backgrounds sleep as a real child process (not a tail
        // exec), records its pid, and then waits on it, so the shell stays
        // alive as the group leader until killed. Killing only that direct
        // child (as `Child::kill` alone would) leaves the backgrounded sleep
        // running as an orphan; a timeout must take the whole group with it.
        let dir =
            std::env::temp_dir().join(format!("whowns-exec-group-kill-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let script = dir.join("wrapper.sh");
        let pidfile = dir.join("child.pid");
        fs::write(
            &script,
            format!(
                "#!/bin/sh\n/bin/sleep 30 &\necho $! > '{}'\nwait\n",
                pidfile.display()
            ),
        )
        .unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut permissions = fs::metadata(&script).unwrap().permissions();
            permissions.set_mode(0o755);
            fs::set_permissions(&script, permissions).unwrap();
        }

        // Generous: isolating the child into its own process group can add
        // noticeable startup latency under a supervised/sandboxed test
        // runner before it reaches `wait`, and the timeout must not fire
        // before that point or the pid is never recorded at all. Still tiny
        // next to the 30-second sleep this is bounding.
        let runner = CommandRunner::with_timeout(Duration::from_secs(3));
        let result = runner.query(&script, &[]);
        assert!(matches!(result, Err(QueryFailure::Timeout)));

        let read_deadline = Instant::now() + Duration::from_secs(2);
        let grandchild_pid = loop {
            if let Ok(contents) = fs::read_to_string(&pidfile)
                && let Ok(pid) = contents.trim().parse::<u32>()
            {
                break pid;
            }
            assert!(
                Instant::now() < read_deadline,
                "the script never recorded its backgrounded child's pid"
            );
            thread::sleep(Duration::from_millis(10));
        };

        // Give the OS a brief moment to actually deliver the signal.
        thread::sleep(Duration::from_millis(200));
        let still_alive = StdCommand::new("kill")
            .arg("-0")
            .arg(grandchild_pid.to_string())
            .status()
            .unwrap()
            .success();
        assert!(
            !still_alive,
            "the backgrounded descendant (pid {grandchild_pid}) should have been killed along with its process group"
        );

        fs::remove_dir_all(&dir).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn preserves_non_utf8_argument_bytes() {
        use std::os::unix::ffi::OsStrExt;

        let dir = std::env::temp_dir().join(format!("whowns-exec-non-utf8-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let script = dir.join("echo_arg.sh");
        let outfile = dir.join("arg.bin");
        fs::write(
            &script,
            format!("#!/bin/sh\nprintf '%s' \"$1\" > '{}'\n", outfile.display()),
        )
        .unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut permissions = fs::metadata(&script).unwrap().permissions();
            permissions.set_mode(0o755);
            fs::set_permissions(&script, permissions).unwrap();
        }

        // 0xFF is not valid UTF-8 in this position; a `&str`-based argument
        // could never represent it, which is exactly the case a path with
        // non-UTF-8 bytes would hit.
        let raw_argument: &[u8] = b"whowns-\xFF-fixture";
        let argument = OsStr::from_bytes(raw_argument);

        let runner = CommandRunner::new();
        let output = runner.query(&script, &[argument]).unwrap();
        assert!(output.success);

        let written = fs::read(&outfile).unwrap();
        assert_eq!(
            written, raw_argument,
            "the exact argument bytes must reach the child process"
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
            format!("#!/bin/sh\necho called >> '{}'\necho ok\n", log.display()),
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
            .query(
                Path::new("sh"),
                &[OsStr::new("-c"), OsStr::new("yes | head -c 200000")],
            )
            .unwrap();

        assert!(output.stdout.len() <= MAX_OUTPUT_BYTES);
        assert!(output.truncated);
        assert_eq!(runner.diagnostics().len(), 1);
    }
}
