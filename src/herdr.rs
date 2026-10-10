use std::{
    env,
    process::{Command, Stdio},
    thread,
};

use serde_json::Value;

use crate::{config::NotificationsConfig, paths::expand_path};

pub(crate) fn herdr_bin() -> String {
    invocation().executable.to_string_lossy().into_owned()
}

/// Serializes tests that swap `HERDR_BIN_PATH` for a stub, since the
/// environment is process-wide and tests run in parallel.
#[cfg(test)]
pub(crate) fn bin_path_lock() -> std::sync::MutexGuard<'static, ()> {
    static LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
    LOCK.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}
pub(crate) fn herdr_json<const N: usize>(args: [&str; N]) -> Result<Value, String> {
    let spec = invocation().command(args, Limits::default());
    let out = ProcessRunner
        .run(&spec)
        .map_err(|error| error.to_string())?;
    serde_json::from_slice(&out).map_err(|_| "invalid CLI JSON".into())
}
pub(crate) fn run_herdr<const N: usize>(args: [&str; N]) -> Result<(), String> {
    let spec = invocation().command(args, Limits::focus());
    ProcessRunner
        .run(&spec)
        .map(|_| ())
        .map_err(|error| error.to_string())
}

pub(crate) fn notify_error(body: &str, config: &NotificationsConfig) {
    notify(body, "request", config);
}

fn notify(body: &str, default_sound: &'static str, config: &NotificationsConfig) {
    if !config.enabled {
        return;
    }
    let body = truncate(body, 180);
    let (sound, custom_sound) = notification_audio(config, default_sound);
    let _ = run_herdr([
        "notification",
        "show",
        "Herdr Navigator",
        "--body",
        &body,
        "--position",
        "top-right",
        "--sound",
        sound,
    ]);
    if let Some(path) = custom_sound {
        play_custom_sound(path);
    }
}

pub(crate) fn notification_audio<'a>(
    config: &'a NotificationsConfig,
    default_sound: &'static str,
) -> (&'static str, Option<&'a str>) {
    if !config.audio {
        return ("none", None);
    }
    match config.sound.trim().to_ascii_lowercase().as_str() {
        "default" => (default_sound, None),
        "custom" => (
            "none",
            config
                .custom_sound
                .as_deref()
                .filter(|path| !path.is_empty()),
        ),
        _ => ("none", None),
    }
}

pub(crate) fn play_custom_sound(path: &str) {
    let path = expand_path(path);
    if !path.is_file() {
        return;
    }
    thread::spawn(move || {
        #[cfg(target_os = "macos")]
        let players: &[(&str, &[&str])] = &[("afplay", &[])];
        #[cfg(not(target_os = "macos"))]
        let players: &[(&str, &[&str])] = &[("pw-play", &[]), ("paplay", &[]), ("aplay", &[])];

        for (player, args) in players {
            let status = Command::new(player)
                .args(*args)
                .arg(&path)
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status();
            if status.is_ok_and(|status| status.success()) {
                break;
            }
        }
    });
}

pub(crate) fn truncate(value: &str, max_chars: usize) -> String {
    let mut out: String = value.chars().take(max_chars).collect();
    if value.chars().count() > max_chars {
        out.push('…');
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::NotificationsConfig;

    #[test]
    fn notification_audio_resolves_silent_default_herdr_and_custom() {
        let silent = NotificationsConfig::default();
        assert_eq!(notification_audio(&silent, "request"), ("none", None));

        let herdr = NotificationsConfig {
            enabled: true,
            audio: true,
            sound: "default".into(),
            custom_sound: None,
        };
        assert_eq!(notification_audio(&herdr, "done"), ("done", None));

        let custom = NotificationsConfig {
            enabled: true,
            audio: true,
            sound: "custom".into(),
            custom_sound: Some("~/sounds/navigator.wav".into()),
        };
        assert_eq!(
            notification_audio(&custom, "done"),
            ("none", Some("~/sounds/navigator.wav"))
        );
    }
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct Limits {
    pub(crate) deadline: std::time::Duration,
    pub(crate) request: usize,
    pub(crate) stdout: usize,
    pub(crate) stderr: usize,
}
impl Default for Limits {
    fn default() -> Self {
        Self {
            deadline: std::time::Duration::from_secs(2),
            request: 64 * 1024,
            stdout: 8 * 1024 * 1024,
            stderr: 64 * 1024,
        }
    }
}
impl Limits {
    pub(crate) fn focus() -> Self {
        Self {
            stdout: 64 * 1024,
            ..Self::default()
        }
    }
}

/// Captures absence too. Child commands never inherit later environment changes.
#[derive(Clone)]
pub(crate) struct Invocation {
    executable: std::path::PathBuf,
    environment: Vec<(std::ffi::OsString, std::ffi::OsString)>,
    cwd: std::path::PathBuf,
    pub(crate) socket: Option<std::path::PathBuf>,
    pub(crate) managed_plugin: bool,
}
impl std::fmt::Debug for Invocation {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Invocation { route: redacted }")
    }
}
impl Invocation {
    #[cfg(test)]
    pub(crate) fn for_test(
        executable: std::path::PathBuf,
        cwd: std::path::PathBuf,
        environment: Vec<(std::ffi::OsString, std::ffi::OsString)>,
    ) -> Self {
        assert!(executable.is_absolute());
        assert!(cwd.is_absolute());
        Self {
            executable,
            environment,
            cwd,
            socket: None,
            managed_plugin: false,
        }
    }

    pub(crate) fn capture() -> Self {
        let environment: Vec<_> = env::vars_os().collect();
        let get = |key: &str| {
            environment
                .iter()
                .find(|(name, _)| name == key)
                .map(|(_, v)| v.clone())
        };
        let cwd = env::current_dir().unwrap_or_default();
        let executable = get("HERDR_BIN_PATH").unwrap_or_else(|| "herdr".into());
        // Resolve PATH now, not when a later request spawns.
        let executable = if std::path::Path::new(&executable).components().count() > 1 {
            cwd.join(executable)
        } else {
            get("PATH")
                .and_then(|path| {
                    env::split_paths(&path)
                        .map(|dir| cwd.join(dir).join(&executable))
                        .find(|path| executable_file(path))
                })
                .unwrap_or_else(|| cwd.join(executable))
        };
        let socket = get("HERDR_SOCKET_PATH")
            .filter(|p| !p.is_empty())
            .map(std::path::PathBuf::from)
            .filter(|p| p.is_absolute() || !cwd.as_os_str().is_empty())
            .map(|p| cwd.join(p));
        let managed_plugin = !cwd.as_os_str().is_empty()
            && socket.is_some()
            && get("HERDR_ENV").is_some_and(|v| v == "1")
            && get("HERDR_PLUGIN_ID").is_some_and(|v| !v.is_empty())
            && get("HERDR_PLUGIN_ROOT").is_some_and(|v| !v.is_empty());
        Self {
            executable,
            environment,
            cwd,
            socket,
            managed_plugin,
        }
    }
    pub(crate) fn absolute_path(&self, path: &std::path::Path) -> std::path::PathBuf {
        self.cwd.join(path)
    }

    pub(crate) fn interactive(&self, local: bool) -> Command {
        let mut command = Command::new(&self.executable);
        command
            .env_clear()
            .envs(
                self.environment
                    .iter()
                    .filter(|(key, _)| !local || !key.to_string_lossy().starts_with("HERDR_"))
                    .cloned(),
            )
            .current_dir(&self.cwd);
        command
    }

    pub(crate) fn configure_process(&self, command: &mut Command) {
        command
            .env_clear()
            .envs(self.environment.iter().cloned())
            .current_dir(&self.cwd);
    }

    pub(crate) fn command<I, S>(&self, args: I, limits: Limits) -> CommandSpec
    where
        I: IntoIterator<Item = S>,
        S: Into<std::ffi::OsString>,
    {
        CommandSpec {
            invocation: self.clone(),
            args: args.into_iter().map(Into::into).collect(),
            stdin: None,
            limits,
        }
    }
}
fn executable_file(path: &std::path::Path) -> bool {
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    {
        use std::os::unix::fs::PermissionsExt;
        path.metadata()
            .is_ok_and(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    {
        path.is_file()
    }
}

pub(crate) fn invocation() -> Invocation {
    // Existing environment-based CLI tests capture their own invocation. Production
    // freezes at startup, before dispatch, including CLI-only launch commands.
    #[cfg(not(test))]
    {
        static INVOCATION: std::sync::OnceLock<Invocation> = std::sync::OnceLock::new();
        INVOCATION.get_or_init(Invocation::capture).clone()
    }
    #[cfg(test)]
    {
        Invocation::capture()
    }
}
#[derive(Clone, Debug)]
pub(crate) struct CommandSpec {
    pub(crate) invocation: Invocation,
    pub(crate) args: Vec<std::ffi::OsString>,
    pub(crate) stdin: Option<Vec<u8>>,
    pub(crate) limits: Limits,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum MutationOutcome {
    NotSent,
    Rejected,
    Unknown,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Failure {
    Unsupported,
    Spawn,
    Io,
    Timeout,
    RequestLimit,
    StdoutLimit,
    StderrLimit,
    Exit,
    Malformed,
    StaleScope,
    ServerChanged,
    ServerRejected { code: String },
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct TransportError {
    pub(crate) cause: Failure,
    pub(crate) outcome: MutationOutcome,
}
impl TransportError {
    pub(crate) fn new(cause: Failure, outcome: MutationOutcome) -> Self {
        Self { cause, outcome }
    }
    pub(crate) fn uncertain_continuity(&self) -> bool {
        !matches!(
            self.cause,
            Failure::Unsupported
                | Failure::RequestLimit
                | Failure::StaleScope
                | Failure::ServerRejected { .. }
        )
    }
}
impl std::fmt::Display for TransportError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // Do not expose paths, environment, stderr, or arbitrary server messages.
        let cause = match self.cause {
            Failure::ServerRejected { .. } => "server rejected request",
            Failure::Unsupported => "ordinary-server operation unsupported",
            Failure::Spawn => "could not start Herdr CLI",
            Failure::Io => "ordinary-server transport failed",
            Failure::Timeout => "ordinary-server deadline exceeded",
            Failure::RequestLimit => "request byte limit exceeded",
            Failure::StdoutLimit => "response byte limit exceeded",
            Failure::StderrLimit => "stderr byte limit exceeded",
            Failure::Exit => "Herdr CLI exited unsuccessfully",
            Failure::Malformed => "invalid ordinary-server response",
            Failure::StaleScope => "target belongs to a different server scope",
            Failure::ServerChanged => "ordinary-server version or protocol changed",
        };
        write!(f, "{cause} ({:?})", self.outcome)
    }
}
impl std::error::Error for TransportError {}

pub(crate) trait CommandRunner: Send + Sync + std::fmt::Debug {
    fn run(&self, spec: &CommandSpec) -> Result<Vec<u8>, TransportError>;
}
#[derive(Debug)]
pub(crate) struct ProcessRunner;
impl CommandRunner for ProcessRunner {
    fn run(&self, spec: &CommandSpec) -> Result<Vec<u8>, TransportError> {
        #[cfg(any(target_os = "linux", target_os = "macos"))]
        {
            process_output(spec)
        }
        #[cfg(not(any(target_os = "linux", target_os = "macos")))]
        {
            let _ = spec;
            Err(TransportError::new(
                Failure::Unsupported,
                MutationOutcome::NotSent,
            ))
        }
    }
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
pub(crate) fn nonblocking(fd: std::os::fd::RawFd) -> std::io::Result<()> {
    // SAFETY: fcntl does not retain the borrowed descriptor.
    unsafe {
        let flags = libc::fcntl(fd, libc::F_GETFL);
        if flags < 0 || libc::fcntl(fd, libc::F_SETFL, flags | libc::O_NONBLOCK) < 0 {
            return Err(std::io::Error::last_os_error());
        }
    }
    Ok(())
}
#[cfg(any(target_os = "linux", target_os = "macos"))]
pub(crate) fn poll_until(
    fds: &mut [libc::pollfd],
    deadline: std::time::Instant,
) -> Result<(), Failure> {
    loop {
        let remaining = deadline
            .checked_duration_since(std::time::Instant::now())
            .ok_or(Failure::Timeout)?;
        let ms = remaining.as_millis().clamp(1, 10) as i32;
        // SAFETY: poll only accesses the supplied live slice for this call.
        let result = unsafe { libc::poll(fds.as_mut_ptr(), fds.len() as libc::nfds_t, ms) };
        if result >= 0 {
            return Ok(());
        }
        if std::io::Error::last_os_error().kind() != std::io::ErrorKind::Interrupted {
            return Err(Failure::Io);
        }
    }
}
#[cfg(any(target_os = "linux", target_os = "macos"))]
fn process_output(spec: &CommandSpec) -> Result<Vec<u8>, TransportError> {
    use std::{
        io::{Read, Write},
        os::{fd::AsRawFd, unix::process::CommandExt},
        time::Instant,
    };
    use MutationOutcome::{NotSent, Unknown};
    let deadline = Instant::now() + spec.limits.deadline;
    let request_bytes = spec
        .args
        .iter()
        .try_fold(spec.stdin.as_ref().map_or(0, Vec::len), |total, arg| {
            total.checked_add(arg.len() + 1)
        });
    if request_bytes.is_none_or(|bytes| bytes > spec.limits.request) {
        return Err(TransportError::new(Failure::RequestLimit, NotSent));
    }
    let child = Command::new(&spec.invocation.executable)
        .args(&spec.args)
        .env_clear()
        .envs(spec.invocation.environment.iter().cloned())
        .current_dir(&spec.invocation.cwd)
        .process_group(0)
        .stdin(if spec.stdin.is_some() {
            Stdio::piped()
        } else {
            Stdio::null()
        })
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|_| TransportError::new(Failure::Spawn, NotSent))?;
    struct ChildGuard(std::process::Child);
    impl Drop for ChildGuard {
        fn drop(&mut self) {
            // SAFETY: this child's process group was created at spawn. Kill the
            // whole group even if its leader exited while descendants kept pipes.
            unsafe {
                libc::kill(-(self.0.id() as i32), libc::SIGKILL);
            }
            let _ = self.0.wait();
        }
    }
    let mut child = ChildGuard(child);
    let result = (|| {
        let mut stdout = child.0.stdout.take().ok_or(Failure::Io)?;
        let mut stderr = child.0.stderr.take().ok_or(Failure::Io)?;
        let mut stdin = child.0.stdin.take();
        for fd in [
            Some(stdout.as_raw_fd()),
            Some(stderr.as_raw_fd()),
            stdin.as_ref().map(AsRawFd::as_raw_fd),
        ]
        .into_iter()
        .flatten()
        {
            nonblocking(fd).map_err(|_| Failure::Io)?;
        }
        let mut out = Vec::new();
        let mut err_count = 0usize;
        let mut input_offset = 0;
        let mut out_eof = false;
        let mut err_eof = false;
        loop {
            if Instant::now() >= deadline {
                return Err(Failure::Timeout);
            }
            // One bounded read per pipe per iteration prevents a busy writer
            // starving stderr or the absolute deadline.
            let mut buf = [0u8; 8192];
            for (reader, eof, is_stdout) in [
                (&mut stdout as &mut dyn Read, &mut out_eof, true),
                (&mut stderr as &mut dyn Read, &mut err_eof, false),
            ] {
                if *eof {
                    continue;
                }
                match reader.read(&mut buf) {
                    Ok(0) => *eof = true,
                    Ok(n) if is_stdout => {
                        if n > spec.limits.stdout.saturating_sub(out.len()) {
                            return Err(Failure::StdoutLimit);
                        }
                        out.extend_from_slice(&buf[..n]);
                    }
                    Ok(n) => {
                        if n > spec.limits.stderr.saturating_sub(err_count) {
                            return Err(Failure::StderrLimit);
                        }
                        err_count += n;
                    }
                    Err(e)
                        if matches!(
                            e.kind(),
                            std::io::ErrorKind::WouldBlock | std::io::ErrorKind::Interrupted
                        ) => {}
                    Err(_) => return Err(Failure::Io),
                }
            }
            if let Some(writer) = stdin.as_mut() {
                let bytes = spec.stdin.as_deref().unwrap_or_default();
                match writer.write(&bytes[input_offset..]) {
                    Ok(n) => input_offset += n,
                    Err(e)
                        if matches!(
                            e.kind(),
                            std::io::ErrorKind::WouldBlock | std::io::ErrorKind::Interrupted
                        ) => {}
                    Err(_) => return Err(Failure::Io),
                }
                if input_offset == bytes.len() {
                    stdin = None;
                }
            }
            // Do not reap the leader before draining. Its PID reserves the group
            // number until cleanup, including the timeout path.
            if out_eof && err_eof {
                // SAFETY: waitid inspects our child without reaping it, so the
                // group ID cannot be reused before ChildGuard kills the group.
                let mut status: libc::siginfo_t = unsafe { std::mem::zeroed() };
                let result = unsafe {
                    libc::waitid(
                        libc::P_PID,
                        child.0.id() as libc::id_t,
                        &mut status,
                        libc::WEXITED | libc::WNOHANG | libc::WNOWAIT,
                    )
                };
                if result < 0 {
                    return Err(Failure::Io);
                }
                // SAFETY: successful waitid initialized siginfo_t for CLD_*.
                if unsafe { status.si_pid() } != 0 {
                    return if status.si_code == libc::CLD_EXITED
                        && unsafe { status.si_status() } == 0
                    {
                        Ok(out)
                    } else {
                        Err(Failure::Exit)
                    };
                }
            }
            let mut fds = [
                libc::pollfd {
                    fd: if out_eof { -1 } else { stdout.as_raw_fd() },
                    events: libc::POLLIN,
                    revents: 0,
                },
                libc::pollfd {
                    fd: if err_eof { -1 } else { stderr.as_raw_fd() },
                    events: libc::POLLIN,
                    revents: 0,
                },
                libc::pollfd {
                    fd: stdin.as_ref().map_or(-1, AsRawFd::as_raw_fd),
                    events: libc::POLLOUT,
                    revents: 0,
                },
            ];
            poll_until(&mut fds, deadline)?;
        }
    })();
    result.map_err(|cause| TransportError::new(cause, Unknown))
}

#[cfg(test)]
#[derive(Debug, Default)]
pub(crate) struct RecordingRunner {
    pub(crate) calls: std::sync::Mutex<Vec<CommandSpec>>,
    pub(crate) replies:
        std::sync::Mutex<std::collections::VecDeque<Result<Vec<u8>, TransportError>>>,
}
#[cfg(test)]
impl CommandRunner for RecordingRunner {
    fn run(&self, spec: &CommandSpec) -> Result<Vec<u8>, TransportError> {
        self.calls.lock().unwrap().push(spec.clone());
        self.replies
            .lock()
            .unwrap()
            .pop_front()
            .expect("unexpected CLI call")
    }
}
#[cfg(test)]
impl CommandRunner for std::sync::Arc<RecordingRunner> {
    fn run(&self, spec: &CommandSpec) -> Result<Vec<u8>, TransportError> {
        self.as_ref().run(spec)
    }
}

#[cfg(all(test, any(target_os = "linux", target_os = "macos")))]
mod runner_tests {
    use super::*;
    use std::{
        ffi::OsString,
        path::PathBuf,
        time::{Duration, Instant},
    };

    fn script(text: &str, limits: Limits) -> CommandSpec {
        Invocation {
            executable: PathBuf::from("/bin/sh"),
            environment: vec![],
            cwd: PathBuf::from("/"),
            socket: None,
            managed_plugin: false,
        }
        .command(["-c", text], limits)
    }
    #[test]
    fn bounded_runner_drains_both_pipes_and_handles_stdin_and_exit() {
        let mut spec = script("cat; printf noise >&2", Limits::default());
        spec.stdin = Some(b"payload".to_vec());
        assert_eq!(ProcessRunner.run(&spec).unwrap(), b"payload");
        assert_eq!(
            ProcessRunner
                .run(&script("exit 7", Limits::default()))
                .unwrap_err()
                .cause,
            Failure::Exit
        );
        let spec = script(
            "i=0; while [ $i -lt 3000 ]; do printf 'stdout'; printf 'stderr' >&2; i=$((i+1)); done",
            Limits::default(),
        );
        assert_eq!(ProcessRunner.run(&spec).unwrap().len(), 18000);
    }
    #[test]
    fn process_limits_and_inherited_pipes_have_absolute_deadlines() {
        let limits = Limits {
            deadline: Duration::from_millis(100),
            ..Limits::default()
        };
        for text in ["sleep 5", "sleep 5 & exit 0"] {
            let start = Instant::now();
            assert_eq!(
                ProcessRunner.run(&script(text, limits)).unwrap_err(),
                TransportError::new(Failure::Timeout, MutationOutcome::Unknown)
            );
            assert!(start.elapsed() >= limits.deadline);
            assert!(start.elapsed() < Duration::from_secs(1));
        }
    }

    #[test]
    fn request_limit_and_spawn_failure_are_not_sent() {
        let mut spec = script(
            "exit 0",
            Limits {
                request: 8,
                ..Limits::default()
            },
        );
        spec.stdin = Some(vec![0; 9]);
        let error = ProcessRunner.run(&spec).unwrap_err();
        assert_eq!(
            error,
            TransportError::new(Failure::RequestLimit, MutationOutcome::NotSent)
        );
        spec.invocation.executable = PathBuf::from("/nonexistent/n04-herdr");
        spec.limits.request = 100;
        assert_eq!(
            ProcessRunner.run(&spec).unwrap_err(),
            TransportError::new(Failure::Spawn, MutationOutcome::NotSent)
        );
    }
    #[test]
    fn finite_stdout_exceeding_cap_reports_stdout_limit() {
        assert_finite_output_limit("", Failure::StdoutLimit);
    }

    #[test]
    fn finite_stderr_exceeding_cap_reports_stderr_limit() {
        assert_finite_output_limit(">&2", Failure::StderrLimit);
    }

    fn assert_finite_output_limit(redirect: &str, expected: Failure) {
        // A finite 640-byte response exceeds the cap even after a delayed startup.
        let text = format!("sleep 0.15; i=0; while [ $i -lt 64 ]; do printf 1234567890 {redirect}; i=$((i+1)); done");
        let start = Instant::now();
        let limits = Limits {
            stdout: 512,
            stderr: 512,
            ..Limits::default()
        };
        assert_eq!(
            ProcessRunner.run(&script(&text, limits)).unwrap_err(),
            TransportError::new(expected, MutationOutcome::Unknown)
        );
        assert!(start.elapsed() < limits.deadline);
    }

    #[test]
    fn capture_freezes_executable_cwd_environment_and_absence() {
        let _lock = bin_path_lock();
        struct Restore(Vec<(&'static str, Option<OsString>)>);
        impl Drop for Restore {
            fn drop(&mut self) {
                for (key, value) in &self.0 {
                    match value {
                        Some(v) => env::set_var(key, v),
                        None => env::remove_var(key),
                    }
                }
            }
        }
        let keys = [
            "HERDR_BIN_PATH",
            "HERDR_SOCKET_PATH",
            "HERDR_SESSION",
            "N04_VALUE",
        ];
        let _restore = Restore(keys.map(|key| (key, env::var_os(key))).into());
        env::set_var("HERDR_BIN_PATH", "/bin/sh");
        env::remove_var("HERDR_SOCKET_PATH");
        env::remove_var("HERDR_SESSION");
        env::set_var("N04_VALUE", "before");
        let route = Invocation::capture();
        env::set_var("HERDR_BIN_PATH", "/not/a/program");
        env::set_var("HERDR_SOCKET_PATH", "/not/a/socket");
        env::set_var("HERDR_SESSION", "changed");
        env::set_var("N04_VALUE", "after");
        let output = ProcessRunner.run(&route.command(["-c", "printf '%s|%s|%s' \"$N04_VALUE\" \"${HERDR_SOCKET_PATH-unset}\" \"${HERDR_SESSION-unset}\""], Limits::default())).unwrap();
        assert_eq!(output, b"before|unset|unset");
        assert!(route.socket.is_none());
        assert_eq!(route.executable, PathBuf::from("/bin/sh"));
        assert_eq!(route.cwd, env::current_dir().unwrap());
    }
    #[test]
    fn recording_runner_records_immutable_specs() {
        let runner = RecordingRunner::default();
        runner
            .replies
            .lock()
            .unwrap()
            .push_back(Ok(b"reply".to_vec()));
        let spec = script("not executed", Limits::default());
        assert_eq!(runner.run(&spec).unwrap(), b"reply");
        assert_eq!(runner.calls.lock().unwrap()[0].args, spec.args);
    }
    #[test]
    fn managed_plugin_contract_requires_explicit_inherited_route_and_runtime_markers() {
        let _lock = bin_path_lock();
        let keys = [
            "HERDR_SOCKET_PATH",
            "HERDR_ENV",
            "HERDR_PLUGIN_ID",
            "HERDR_PLUGIN_ROOT",
            "HERDR_PLUGIN_CONTEXT_JSON",
        ];
        struct Restore(Vec<(&'static str, Option<OsString>)>);
        impl Drop for Restore {
            fn drop(&mut self) {
                for (key, value) in &self.0 {
                    match value {
                        Some(v) => env::set_var(key, v),
                        None => env::remove_var(key),
                    }
                }
            }
        }
        let _restore = Restore(keys.map(|key| (key, env::var_os(key))).into());
        for key in keys {
            env::remove_var(key);
        }
        env::set_var(
            "HERDR_PLUGIN_CONTEXT_JSON",
            r#"{"workspace_id":"w1","client_id":"not-authority"}"#,
        );
        assert!(!Invocation::capture().managed_plugin);
        env::set_var("HERDR_SOCKET_PATH", "/synthetic/not-connected.sock");
        assert!(!Invocation::capture().managed_plugin);
        env::set_var("HERDR_ENV", "1");
        env::set_var("HERDR_PLUGIN_ID", "test-plugin");
        env::set_var("HERDR_PLUGIN_ROOT", "/synthetic/plugin");
        let route = Invocation::capture();
        assert!(route.managed_plugin);
        let mut context = crate::navigation::NavigationContext::with_server(
            crate::navigation::server::OrdinaryServer::new(
                route,
                Box::new(RecordingRunner::default()),
                Limits::default(),
            ),
        );
        context.require_process_server_colocation().unwrap();
        assert!(context.current_client_selection().is_none());
        let reported =
            crate::paths::OwnedPath::reported(context.scope(), "/must/not/resolve").unwrap();
        assert!(context.process_path(&reported).is_err());
        context.invalidate_scope();
        assert!(context.require_process_server_colocation().is_err());
        env::remove_var("HERDR_SOCKET_PATH");
        assert!(!Invocation::capture().managed_plugin);
    }

    #[test]
    fn timeout_kills_descendants_that_inherit_output_pipes() {
        use std::time::{SystemTime, UNIX_EPOCH};
        let marker = env::temp_dir().join(format!(
            "n04-child-{}",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let text = format!(
            "(sleep 0.3; printf escaped > '{}') & exit 0",
            marker.display()
        );
        let spec = script(
            &text,
            Limits {
                deadline: Duration::from_millis(50),
                ..Limits::default()
            },
        );
        let start = Instant::now();
        assert_eq!(
            ProcessRunner.run(&spec).unwrap_err(),
            TransportError::new(Failure::Timeout, MutationOutcome::Unknown)
        );
        assert!(start.elapsed() >= spec.limits.deadline);
        assert!(start.elapsed() < Duration::from_secs(1));
        thread::sleep(Duration::from_millis(400));
        assert!(!marker.exists(), "descendant survived group cleanup");
    }
    #[test]
    fn frozen_route_covers_template_followons_notifications_attach_and_process_environment() {
        let _lock = bin_path_lock();
        let keys = [
            "HERDR_BIN_PATH",
            "HERDR_SOCKET_PATH",
            "HERDR_SESSION",
            "HERDR_NEW_AFTER_CAPTURE",
            "N06_TOKEN",
        ];
        struct Restore {
            values: Vec<(&'static str, Option<OsString>)>,
            cwd: std::path::PathBuf,
        }
        impl Drop for Restore {
            fn drop(&mut self) {
                for (key, value) in &self.values {
                    match value {
                        Some(value) => env::set_var(key, value),
                        None => env::remove_var(key),
                    }
                }
                env::set_current_dir(&self.cwd).unwrap();
            }
        }
        let restore = Restore {
            values: keys.iter().map(|key| (*key, env::var_os(key))).collect(),
            cwd: env::current_dir().unwrap(),
        };
        env::set_var("HERDR_BIN_PATH", "/synthetic/first/herdr");
        env::set_var("HERDR_SOCKET_PATH", "/synthetic/first/socket");
        env::set_var("HERDR_SESSION", "first");
        env::set_var("N06_TOKEN", "first");
        env::remove_var("HERDR_NEW_AFTER_CAPTURE");
        let route = Invocation::capture();
        let runner = std::sync::Arc::new(RecordingRunner::default());
        runner
            .replies
            .lock()
            .unwrap()
            .extend((0..4).map(|_| Ok(vec![])));
        let server = crate::navigation::server::OrdinaryServer::new(
            route.clone(),
            Box::new(runner.clone()),
            Limits::default(),
        );
        server
            .run(["plugin", "action", "invoke", "first-call"])
            .unwrap();
        env::set_var("HERDR_BIN_PATH", "/synthetic/second/herdr");
        env::set_var("HERDR_SOCKET_PATH", "/synthetic/second/socket");
        env::set_var("HERDR_SESSION", "second");
        env::set_var("HERDR_NEW_AFTER_CAPTURE", "never inherited");
        env::set_var("N06_TOKEN", "second");
        env::set_current_dir(env::temp_dir()).unwrap();
        let project = crate::model::Project {
            name: "demo".into(),
            description: String::new(),
            working_dir: "/tmp".into(),
            tabs: vec![crate::model::ProjectTab {
                name: "shell".into(),
                command: Some("echo startup".into()),
                panes: vec![],
            }],
        };
        crate::integrations::herdr_plus::bootstrap_project_tabs(&server, &project,
            &serde_json::json!({"result":{"workspace":{"workspace_id":"w1"},"root_pane":{"pane_id":"p1"}}}), std::path::Path::new("/tmp")).unwrap();
        let mut config = crate::config::Config::default().notifications;
        config.enabled = true;
        config.audio = false;
        server.notify("done", true, &config);
        let calls = runner.calls.lock().unwrap();
        assert_eq!(calls.len(), 4);
        for call in calls.iter() {
            assert_eq!(call.invocation.executable, route.executable);
            assert_eq!(call.invocation.environment, route.environment);
            assert_eq!(call.invocation.cwd, route.cwd);
            assert_eq!(call.invocation.socket, route.socket);
        }
        let remote = route.interactive(false);
        assert_eq!(remote.get_program(), route.executable);
        assert_eq!(remote.get_current_dir(), Some(restore.cwd.as_path()));
        assert!(remote
            .get_envs()
            .any(|(k, v)| k == "HERDR_SESSION" && v == Some(std::ffi::OsStr::new("first"))));
        assert!(!remote
            .get_envs()
            .any(|(k, _)| k == "HERDR_NEW_AFTER_CAPTURE"));
        let local = route.interactive(true);
        assert!(!local
            .get_envs()
            .any(|(k, _)| k.to_string_lossy().starts_with("HERDR_")));
        let mut process = Command::new("sh");
        route.configure_process(&mut process);
        assert_eq!(process.get_current_dir(), Some(restore.cwd.as_path()));
        assert!(process
            .get_envs()
            .any(|(k, v)| k == "N06_TOKEN" && v == Some(std::ffi::OsStr::new("first"))));
    }
}
