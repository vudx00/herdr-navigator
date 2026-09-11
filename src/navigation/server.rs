//! Ordinary server acknowledgements do not describe an invoking client's selection.
use std::{sync::Mutex, time::Instant};

use serde::{de::DeserializeOwned, Deserialize};

use super::inventory::{AgentRecord, PaneRecord, TabRecord, WorkspaceRecord};
use serde_json::{json, Value};

use crate::{
    herdr::{
        CommandRunner, Failure, Invocation, Limits, MutationOutcome, ProcessRunner, TransportError,
    },
    model::{LiveTarget, ObjectKind, ServerScope},
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Capability {
    Unknown,
    Supported,
    Rejected,
}

#[derive(Debug)]
pub(crate) struct OrdinaryServer {
    route: Invocation,
    runner: Box<dyn CommandRunner>,
    limits: Limits,
    focus_capabilities: Mutex<[Capability; 3]>,
    observed_version: Mutex<Option<(String, u64)>>,
    mutation_failure: Mutex<Option<TransportError>>,
}
impl OrdinaryServer {
    pub(crate) fn new(route: Invocation, runner: Box<dyn CommandRunner>, limits: Limits) -> Self {
        Self {
            route,
            runner,
            limits,
            focus_capabilities: Mutex::new([Capability::Unknown; 3]),
            observed_version: Mutex::new(None),
            mutation_failure: Mutex::new(None),
        }
    }
    pub(crate) fn inherited() -> Self {
        Self::new(
            crate::herdr::invocation(),
            Box::new(ProcessRunner),
            Limits::default(),
        )
    }
    pub(crate) fn process_route(&self) -> &Invocation {
        &self.route
    }
    pub(crate) fn run<const N: usize>(&self, args: [&str; N]) -> Result<(), String> {
        self.mutation(args).map(|_| ())
    }
    pub(crate) fn json<const N: usize>(&self, args: [&str; N]) -> Result<Value, String> {
        let bytes = self.mutation(args)?;
        let value: Value = serde_json::from_slice(&bytes)
            .map_err(|_| self.record_mutation_failure(malformed()))?;
        if value.get("error").is_some() || value.get("result").is_none() {
            return Err(self.record_mutation_failure(malformed()));
        }
        Ok(value)
    }
    fn record_mutation_failure(&self, error: TransportError) -> String {
        let text = error.to_string();
        *self
            .mutation_failure
            .lock()
            .unwrap_or_else(|e| e.into_inner()) = Some(error);
        text
    }
    fn mutation<const N: usize>(&self, args: [&str; N]) -> Result<Vec<u8>, String> {
        self.runner
            .run(&self.route.command(args, self.limits))
            .map_err(|error| self.record_mutation_failure(error))
    }
    pub(crate) fn take_mutation_failure(&self) -> Option<TransportError> {
        self.mutation_failure
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .take()
    }
    pub(crate) fn notify(
        &self,
        body: &str,
        success: bool,
        config: &crate::config::NotificationsConfig,
    ) {
        if !config.enabled {
            return;
        }
        let body = crate::herdr::truncate(body, 180);
        let (sound, custom) =
            crate::herdr::notification_audio(config, if success { "done" } else { "request" });
        let _ = self.runner.run(&self.route.command(
            [
                "notification",
                "show",
                "Herdr Navigator",
                "--body",
                &body,
                "--position",
                "top-right",
                "--sound",
                sound,
            ],
            self.limits,
        ));
        if let Some(path) = custom {
            crate::herdr::play_custom_sound(path);
        }
    }
    pub(crate) fn managed_plugin(&self) -> bool {
        self.route.managed_plugin
    }
    pub(crate) fn invalidate(&self) {
        *self
            .observed_version
            .lock()
            .unwrap_or_else(|e| e.into_inner()) = None;
        *self
            .focus_capabilities
            .lock()
            .unwrap_or_else(|e| e.into_inner()) = [Capability::Unknown; 3];
    }
    pub(crate) fn focus_unavailable(&self, kind: ObjectKind) -> Option<&'static str> {
        if self.route.socket.is_none() {
            return Some("exact focus requires an explicit ordinary socket");
        }
        (self.capability(kind) == Capability::Rejected)
            .then_some("exact focus is unsupported for this target kind")
    }

    pub(crate) fn capability(&self, kind: ObjectKind) -> Capability {
        self.focus_capabilities
            .lock()
            .unwrap_or_else(|e| e.into_inner())[kind_index(kind)]
    }
    pub(crate) fn workspaces(&self) -> Result<Vec<WorkspaceRecord>, TransportError> {
        self.list("workspace", "workspaces")
    }
    pub(crate) fn panes(&self) -> Result<Vec<PaneRecord>, TransportError> {
        self.list("pane", "panes")
    }
    pub(crate) fn tabs(&self) -> Result<Vec<TabRecord>, TransportError> {
        self.list("tab", "tabs")
    }
    pub(crate) fn agents(&self) -> Result<Vec<AgentRecord>, TransportError> {
        self.list("agent", "agents")
    }
    fn list<T: DeserializeOwned>(&self, kind: &str, field: &str) -> Result<Vec<T>, TransportError> {
        let deadline = Instant::now() + self.limits.deadline;
        let bytes = self
            .runner
            .run(&self.route.command([kind, "list"], self.limits))?;
        if bytes.len() > self.limits.stdout {
            return Err(TransportError::new(
                Failure::StdoutLimit,
                MutationOutcome::Unknown,
            ));
        }
        // The CLI owns its request ID. Check its type, but never compare it with
        // raw socket IDs generated by Navigator or assume a fixed cli:* spelling.
        let value = response(&bytes, None, &format!("{kind}_list"))?;
        let rows = value["result"][field].as_array().ok_or_else(malformed)?;
        for row in rows {
            validate_row(row, kind)?;
            check_deadline(deadline, MutationOutcome::Unknown)?;
        }
        let records =
            serde_json::from_value(value["result"][field].clone()).map_err(|_| malformed())?;
        check_deadline(deadline, MutationOutcome::Unknown)?;
        Ok(records)
    }
    pub(crate) fn focus(
        &self,
        scope: &ServerScope,
        target: &LiveTarget,
    ) -> Result<(), TransportError> {
        if target.identity().scope() != scope {
            return Err(TransportError::new(
                Failure::StaleScope,
                MutationOutcome::NotSent,
            ));
        }
        let kind = target.identity().kind();
        let deadline = Instant::now() + self.limits.deadline;
        if self.capability(kind) != Capability::Rejected {
            // A fresh connection may reach a replaced server. Recheck metadata
            // before each mutation even while prior observations remain useful.
            {
                let mut capabilities = self
                    .focus_capabilities
                    .lock()
                    .unwrap_or_else(|e| e.into_inner());
                for capability in capabilities.iter_mut() {
                    if *capability == Capability::Supported {
                        *capability = Capability::Unknown;
                    }
                }
            }
            // Read-only negotiation, never a mutation probe. Version/protocol
            // enables documented operations, not identity or client authority.
            let snapshot = self
                .request(
                    "session.snapshot",
                    json!({}),
                    "session_snapshot",
                    self.limits.stdout,
                    deadline,
                )
                .map_err(|mut error| {
                    error.outcome = MutationOutcome::NotSent;
                    error
                })?;
            let snapshot = &snapshot["result"]["snapshot"];
            validate_snapshot(snapshot).map_err(|mut error| {
                error.outcome = MutationOutcome::NotSent;
                error
            })?;
            let version = (
                snapshot["version"]
                    .as_str()
                    .ok_or_else(malformed)?
                    .to_owned(),
                snapshot["protocol"].as_u64().ok_or_else(malformed)?,
            );
            {
                let mut observed = self
                    .observed_version
                    .lock()
                    .unwrap_or_else(|e| e.into_inner());
                if observed.as_ref().is_some_and(|old| old != &version) {
                    return Err(TransportError::new(
                        Failure::ServerChanged,
                        MutationOutcome::NotSent,
                    ));
                }
                *observed = Some(version);
            }
            if snapshot["version"] == "0.9.0" && snapshot["protocol"] == 22 {
                let mut capabilities = self
                    .focus_capabilities
                    .lock()
                    .unwrap_or_else(|e| e.into_inner());
                for capability in capabilities.iter_mut() {
                    if *capability == Capability::Unknown {
                        *capability = Capability::Supported;
                    }
                }
            }
        }
        if self.capability(kind) != Capability::Supported {
            return Err(TransportError::new(
                Failure::Unsupported,
                MutationOutcome::NotSent,
            ));
        }
        let (method, field) = match kind {
            ObjectKind::Workspace => ("workspace.focus", "workspace_id"),
            ObjectKind::Tab => ("tab.focus", "tab_id"),
            ObjectKind::Pane => ("pane.focus", "pane_id"),
        };
        let result = self.request(
            method,
            json!({field: target.identity().id()}),
            "ok",
            self.limits.stdout.min(64 * 1024),
            deadline,
        );
        if let Err(TransportError {
            cause: Failure::ServerRejected { code },
            ..
        }) = &result
        {
            // Error codes are open strings in the schema. Only an explicit
            // unsupported-method rejection changes a capability observation.
            if matches!(
                code.as_str(),
                "unknown_method" | "unsupported_method" | "method_not_found"
            ) {
                self.focus_capabilities
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())[kind_index(kind)] = Capability::Rejected;
            }
        }
        result.map(|_| ())
    }
    fn request(
        &self,
        method: &str,
        params: Value,
        expected: &str,
        cap: usize,
        deadline: Instant,
    ) -> Result<Value, TransportError> {
        use std::sync::atomic::{AtomicU64, Ordering};
        static NEXT: AtomicU64 = AtomicU64::new(1);
        let id = format!("navigator:{}", NEXT.fetch_add(1, Ordering::Relaxed));
        let mut bytes = serde_json::to_vec(&json!({"id": id, "method": method, "params": params}))
            .map_err(|_| malformed())?;
        bytes.push(b'\n');
        if bytes.len() > self.limits.request {
            return Err(TransportError::new(
                Failure::RequestLimit,
                MutationOutcome::NotSent,
            ));
        }
        let path =
            self.route.socket.as_deref().ok_or_else(|| {
                TransportError::new(Failure::Unsupported, MutationOutcome::NotSent)
            })?;
        let bytes = exchange(path, &bytes, cap, deadline)?;
        let response = response(&bytes, Some(&id), expected)?;
        check_deadline(deadline, MutationOutcome::Unknown)?;
        Ok(response)
    }
}
fn check_deadline(deadline: Instant, outcome: MutationOutcome) -> Result<(), TransportError> {
    if Instant::now() >= deadline {
        return Err(TransportError::new(Failure::Timeout, outcome));
    }
    Ok(())
}
fn kind_index(kind: ObjectKind) -> usize {
    match kind {
        ObjectKind::Workspace => 0,
        ObjectKind::Tab => 1,
        ObjectKind::Pane => 2,
    }
}
fn malformed() -> TransportError {
    TransportError::new(Failure::Malformed, MutationOutcome::Unknown)
}

// Reject duplicate keys too. Parsing directly to Value would silently keep the
// last id/result/error or discriminator and hide a conflicting response.
struct UniqueValue(Value);
impl<'de> Deserialize<'de> for UniqueValue {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct Visitor;
        impl<'de> serde::de::Visitor<'de> for Visitor {
            type Value = UniqueValue;
            fn expecting(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                f.write_str("JSON without duplicate keys")
            }
            fn visit_map<A: serde::de::MapAccess<'de>>(
                self,
                mut map: A,
            ) -> Result<Self::Value, A::Error> {
                let mut out = serde_json::Map::new();
                while let Some((key, UniqueValue(value))) =
                    map.next_entry::<String, UniqueValue>()?
                {
                    if out.insert(key, value).is_some() {
                        return Err(serde::de::Error::custom("duplicate JSON key"));
                    }
                }
                Ok(UniqueValue(Value::Object(out)))
            }
            fn visit_seq<A: serde::de::SeqAccess<'de>>(
                self,
                mut seq: A,
            ) -> Result<Self::Value, A::Error> {
                let mut out = Vec::new();
                while let Some(UniqueValue(value)) = seq.next_element()? {
                    out.push(value);
                }
                Ok(UniqueValue(Value::Array(out)))
            }
            fn visit_bool<E: serde::de::Error>(self, v: bool) -> Result<Self::Value, E> {
                Ok(UniqueValue(v.into()))
            }
            fn visit_str<E: serde::de::Error>(self, v: &str) -> Result<Self::Value, E> {
                Ok(UniqueValue(v.into()))
            }
            fn visit_i64<E: serde::de::Error>(self, v: i64) -> Result<Self::Value, E> {
                Ok(UniqueValue(v.into()))
            }
            fn visit_u64<E: serde::de::Error>(self, v: u64) -> Result<Self::Value, E> {
                Ok(UniqueValue(v.into()))
            }
            fn visit_f64<E: serde::de::Error>(self, v: f64) -> Result<Self::Value, E> {
                Ok(UniqueValue(v.into()))
            }
            fn visit_unit<E: serde::de::Error>(self) -> Result<Self::Value, E> {
                Ok(UniqueValue(Value::Null))
            }
        }
        deserializer.deserialize_any(Visitor)
    }
}
fn response(bytes: &[u8], id: Option<&str>, expected: &str) -> Result<Value, TransportError> {
    let UniqueValue(value) = serde_json::from_slice(bytes).map_err(|_| malformed())?;
    let response_id = value
        .get("id")
        .and_then(Value::as_str)
        .filter(|id| !id.is_empty())
        .ok_or_else(malformed)?;
    if id.is_some_and(|id| id != response_id) {
        return Err(malformed());
    }
    match (value.get("result"), value.get("error")) {
        (Some(result), None) if result.get("type").and_then(Value::as_str) == Some(expected) => {
            Ok(value)
        }
        (None, Some(error)) => {
            let code = error
                .get("code")
                .and_then(Value::as_str)
                .filter(|code| !code.is_empty())
                .ok_or_else(malformed)?;
            error
                .get("message")
                .and_then(Value::as_str)
                .ok_or_else(malformed)?;
            Err(TransportError::new(
                Failure::ServerRejected { code: code.into() },
                MutationOutcome::Rejected,
            ))
        }
        _ => Err(malformed()),
    }
}
fn validate_row(row: &Value, kind: &str) -> Result<(), TransportError> {
    let strings: &[&str] = match kind {
        "workspace" => &["workspace_id", "label", "active_tab_id", "agent_status"],
        "tab" => &["tab_id", "workspace_id", "label", "agent_status"],
        _ => &[
            "pane_id",
            "terminal_id",
            "workspace_id",
            "tab_id",
            "agent_status",
        ],
    };
    for field in strings {
        let value = row
            .get(field)
            .and_then(Value::as_str)
            .ok_or_else(malformed)?;
        if field.ends_with("_id") && value.trim().is_empty() {
            return Err(malformed());
        }
    }
    if !matches!(
        row["agent_status"].as_str(),
        Some("idle" | "working" | "blocked" | "done" | "unknown")
    ) {
        return Err(malformed());
    }
    row.get("focused")
        .and_then(Value::as_bool)
        .ok_or_else(malformed)?;
    let integers: &[&str] = match kind {
        "workspace" => &["number", "pane_count", "tab_count"],
        "tab" => &["number", "pane_count"],
        _ => &["revision"],
    };
    for field in integers {
        row.get(field)
            .and_then(Value::as_u64)
            .ok_or_else(malformed)?;
    }
    // Check optional fields consumed by inventory and focus negotiation.
    for field in [
        "cwd",
        "foreground_cwd",
        "agent",
        "label",
        "name",
        "display_agent",
        "title",
        "terminal_title",
        "terminal_title_stripped",
    ] {
        if row
            .get(field)
            .is_some_and(|v| !v.is_null() && !v.is_string())
        {
            return Err(malformed());
        }
    }
    if let Some(session) = row.get("agent_session").filter(|v| !v.is_null()) {
        serde_json::from_value::<super::inventory::AgentSession>(session.clone())
            .map_err(|_| malformed())?;
    }
    Ok(())
}
fn validate_snapshot(snapshot: &Value) -> Result<(), TransportError> {
    snapshot
        .get("version")
        .and_then(Value::as_str)
        .ok_or_else(malformed)?;
    snapshot
        .get("protocol")
        .and_then(Value::as_u64)
        .filter(|v| u32::try_from(*v).is_ok())
        .ok_or_else(malformed)?;
    for (field, kind) in [
        ("workspaces", "workspace"),
        ("tabs", "tab"),
        ("panes", "pane"),
        ("agents", "agent"),
    ] {
        for row in snapshot
            .get(field)
            .and_then(Value::as_array)
            .ok_or_else(malformed)?
        {
            validate_row(row, kind)?;
        }
    }
    snapshot
        .get("layouts")
        .and_then(Value::as_array)
        .ok_or_else(malformed)?;
    Ok(())
}

#[cfg(not(any(target_os = "linux", target_os = "macos")))]
fn exchange(
    _path: &std::path::Path,
    _request: &[u8],
    _cap: usize,
    _deadline: Instant,
) -> Result<Vec<u8>, TransportError> {
    Err(TransportError::new(
        Failure::Unsupported,
        MutationOutcome::NotSent,
    ))
}
#[cfg(any(target_os = "linux", target_os = "macos"))]
fn exchange(
    path: &std::path::Path,
    request: &[u8],
    cap: usize,
    deadline: Instant,
) -> Result<Vec<u8>, TransportError> {
    use std::{
        io::{Read, Write},
        os::{
            fd::{AsRawFd, FromRawFd, OwnedFd},
            unix::{ffi::OsStrExt, net::UnixStream},
        },
    };
    let mut outcome = MutationOutcome::NotSent;
    let result = (|| {
        // SAFETY: zero is a valid initial sockaddr_un; sun_path is filled below.
        let mut address: libc::sockaddr_un = unsafe { std::mem::zeroed() };
        let path = path.as_os_str().as_bytes();
        if path.is_empty() || path.len() >= address.sun_path.len() || path.contains(&0) {
            return Err(Failure::Unsupported);
        }
        address.sun_family = libc::AF_UNIX as libc::sa_family_t;
        for (dest, byte) in address.sun_path.iter_mut().zip(path) {
            *dest = *byte as libc::c_char;
        }
        let len =
            (std::mem::offset_of!(libc::sockaddr_un, sun_path) + path.len() + 1) as libc::socklen_t;
        #[cfg(target_os = "macos")]
        {
            address.sun_len = len as u8;
        }
        // SAFETY: socket returns a fresh descriptor, owned immediately below.
        let raw = unsafe { libc::socket(libc::AF_UNIX, libc::SOCK_STREAM, 0) };
        if raw < 0 {
            return Err(Failure::Io);
        }
        // SAFETY: raw is valid and has no other Rust owner.
        let fd = unsafe { OwnedFd::from_raw_fd(raw) };
        crate::herdr::nonblocking(fd.as_raw_fd()).map_err(|_| Failure::Io)?;
        // SAFETY: fd is live and address has the supplied initialized length.
        unsafe {
            if libc::fcntl(fd.as_raw_fd(), libc::F_SETFD, libc::FD_CLOEXEC) < 0 {
                return Err(Failure::Io);
            }
            if libc::connect(
                fd.as_raw_fd(),
                (&address as *const libc::sockaddr_un).cast(),
                len,
            ) < 0
            {
                let error = std::io::Error::last_os_error();
                if !matches!(error.raw_os_error(), Some(libc::EINPROGRESS | libc::EAGAIN)) {
                    return Err(Failure::Io);
                }
                // EAGAIN on Unix connect can mean a full listener queue. Do not
                // retry connecting or send a mutation to an unconnected socket.
                if error.raw_os_error() == Some(libc::EAGAIN) {
                    return Err(Failure::Io);
                }
                loop {
                    let mut fds = [libc::pollfd {
                        fd: fd.as_raw_fd(),
                        events: libc::POLLOUT,
                        revents: 0,
                    }];
                    crate::herdr::poll_until(&mut fds, deadline)?;
                    if fds[0].revents != 0 {
                        break;
                    }
                }
                let mut error: libc::c_int = 0;
                let mut size = std::mem::size_of_val(&error) as libc::socklen_t;
                if libc::getsockopt(
                    fd.as_raw_fd(),
                    libc::SOL_SOCKET,
                    libc::SO_ERROR,
                    (&mut error as *mut libc::c_int).cast(),
                    &mut size,
                ) < 0
                    || error != 0
                {
                    return Err(Failure::Io);
                }
            }
        }
        let mut stream = UnixStream::from(fd);
        let mut sent = 0;
        let mut response = Vec::new();
        loop {
            if Instant::now() >= deadline {
                return Err(Failure::Timeout);
            }
            if sent < request.len() {
                match stream.write(&request[sent..]) {
                    Ok(0) => return Err(Failure::Io),
                    Ok(n) => {
                        sent += n;
                        outcome = MutationOutcome::Unknown;
                    }
                    Err(e)
                        if matches!(
                            e.kind(),
                            std::io::ErrorKind::WouldBlock | std::io::ErrorKind::Interrupted
                        ) => {}
                    Err(_) => return Err(Failure::Io),
                }
            } else {
                let mut buffer = [0u8; 8192];
                match stream.read(&mut buffer) {
                    Ok(0) => return Err(Failure::Malformed), // NDJSON requires a final newline.
                    Ok(n) => {
                        if n > cap.saturating_sub(response.len()) {
                            return Err(Failure::StdoutLimit);
                        }
                        response.extend_from_slice(&buffer[..n]);
                        if response.contains(&b'\n') {
                            // One response per request. serde rejects any trailing
                            // JSON already received with this frame.
                            return Ok(response);
                        }
                    }
                    Err(e)
                        if matches!(
                            e.kind(),
                            std::io::ErrorKind::WouldBlock | std::io::ErrorKind::Interrupted
                        ) => {}
                    Err(_) => return Err(Failure::Io),
                }
            }
            let mut fds = [libc::pollfd {
                fd: stream.as_raw_fd(),
                events: if sent < request.len() {
                    libc::POLLOUT
                } else {
                    libc::POLLIN
                },
                revents: 0,
            }];
            crate::herdr::poll_until(&mut fds, deadline)?;
        }
    })();
    result.map_err(|cause| TransportError::new(cause, outcome))
}

#[cfg(all(test, any(target_os = "linux", target_os = "macos")))]
pub(crate) mod tests {
    use super::*;
    use crate::{herdr::RecordingRunner, model::LiveIdentity, navigation::NavigationContext};
    use std::{
        io::{Read, Write},
        os::unix::net::UnixListener,
        path::PathBuf,
        sync::{
            atomic::{AtomicBool, AtomicU64, Ordering},
            mpsc, Arc,
        },
        thread,
        time::Duration,
    };

    pub(crate) enum Reply {
        Result(Value),
        Error(&'static str),
        Bytes(Vec<u8>),
        WrongId,
        Hang,
        Truncated,
    }
    pub(crate) struct Socket {
        pub(crate) path: PathBuf,
        pub(crate) requests: Arc<Mutex<Vec<Value>>>,
        worker: Option<thread::JoinHandle<Result<(), String>>>,
        cancel: Arc<AtomicBool>,
        expected_requests: usize,
    }
    impl Socket {
        pub(crate) fn new(replies: Vec<Reply>) -> Self {
            static NEXT: AtomicU64 = AtomicU64::new(0);
            let path = std::env::temp_dir().join(format!(
                "n04-{}-{}.sock",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            let listener = UnixListener::bind(&path).unwrap();
            listener.set_nonblocking(true).unwrap();
            let requests = Arc::new(Mutex::new(Vec::new()));
            let recorded = requests.clone();
            let cancel = Arc::new(AtomicBool::new(false));
            let cancelled = cancel.clone();
            let expected_requests = replies.len();
            let (ready, readiness) = mpsc::sync_channel(1);
            let worker = thread::spawn(move || {
                ready.send(()).map_err(|error| error.to_string())?;
                for reply in replies {
                    // The fixture watchdog is separate from the client's operation deadline.
                    let deadline = Instant::now() + Duration::from_secs(5);
                    let (mut stream, _) = loop {
                        if cancelled.load(Ordering::Acquire) {
                            return Ok(());
                        }
                        match listener.accept() {
                            Ok(pair) => break pair,
                            Err(error)
                                if error.kind() == std::io::ErrorKind::WouldBlock
                                    && Instant::now() < deadline =>
                            {
                                thread::sleep(Duration::from_millis(1))
                            }
                            Err(error) => return Err(format!("synthetic socket accept: {error}")),
                        }
                    };
                    stream
                        .set_nonblocking(true)
                        .map_err(|error| error.to_string())?;
                    let mut line = Vec::new();
                    loop {
                        if cancelled.load(Ordering::Acquire) {
                            return Ok(());
                        }
                        if Instant::now() >= deadline {
                            return Err("synthetic socket request watchdog expired".into());
                        }
                        let mut byte = [0];
                        match stream.read(&mut byte) {
                            Ok(0) => {
                                return Err("client disconnected before request newline".into())
                            }
                            Ok(_) => {
                                line.push(byte[0]);
                                if byte[0] == b'\n' {
                                    break;
                                }
                                if line.len() > 64 * 1024 {
                                    return Err("synthetic socket request too large".into());
                                }
                            }
                            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                                thread::sleep(Duration::from_millis(1));
                            }
                            Err(error) => return Err(error.to_string()),
                        }
                    }
                    let request: Value =
                        serde_json::from_slice(&line).map_err(|error| error.to_string())?;
                    recorded.lock().unwrap().push(request.clone());
                    let bytes = match reply {
                        Reply::Result(result) => format!("{}\n", json!({"id": request["id"], "result": result})).into_bytes(),
                        Reply::Error(code) => format!("{}\n", json!({"id": request["id"], "error": {"code": code, "message": "synthetic rejection"}})).into_bytes(),
                        Reply::Bytes(bytes) => bytes,
                        Reply::WrongId => b"{\"id\":\"wrong\",\"result\":{\"type\":\"ok\"}}\n".to_vec(),
                        Reply::Truncated => format!("{}", json!({"id": request["id"], "result": {"type": "ok"}})).into_bytes(),
                        Reply::Hang => {
                            // Record the request, then withhold the acknowledgement until disconnect.
                            loop {
                                if cancelled.load(Ordering::Acquire) {
                                    return Ok(());
                                }
                                if Instant::now() >= deadline {
                                    return Err("synthetic socket acknowledgement watchdog expired".into());
                                }
                                match stream.read(&mut [0]) {
                                    Ok(0) => break,
                                    Ok(_) => return Err("unexpected bytes after request".into()),
                                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                                        thread::sleep(Duration::from_millis(1));
                                    }
                                    Err(error) => return Err(error.to_string()),
                                }
                            }
                            continue;
                        }
                    };
                    stream
                        .set_nonblocking(false)
                        .map_err(|error| error.to_string())?;
                    stream
                        .set_write_timeout(Some(Duration::from_secs(1)))
                        .map_err(|error| error.to_string())?;
                    let _ = stream.write_all(&bytes);
                }
                Ok(())
            });
            let socket = Self {
                path,
                requests,
                worker: Some(worker),
                cancel,
                expected_requests,
            };
            readiness
                .recv_timeout(Duration::from_secs(5))
                .expect("synthetic socket worker readiness");
            socket
        }
        pub(crate) fn server(&self, limits: Limits) -> OrdinaryServer {
            let mut route = crate::herdr::invocation();
            route.socket = Some(self.path.clone());
            route.managed_plugin = false;
            OrdinaryServer::new(route, Box::new(RecordingRunner::default()), limits)
        }
    }
    impl Drop for Socket {
        fn drop(&mut self) {
            self.cancel.store(true, Ordering::Release);
            let result = self.worker.take().unwrap().join();
            let _ = std::fs::remove_file(&self.path);
            if !thread::panicking() {
                result.unwrap().unwrap();
                assert_eq!(
                    self.requests.lock().unwrap().len(),
                    self.expected_requests,
                    "synthetic socket did not receive every expected request"
                );
            }
        }
    }
    pub(crate) fn snapshot() -> Value {
        json!({"type":"session_snapshot", "snapshot":{"version":"0.9.0", "protocol":22,
            "workspaces":[], "tabs":[], "panes":[], "layouts":[], "agents":[]}})
    }
    pub(crate) fn target(scope: &ServerScope, kind: ObjectKind) -> LiveTarget {
        let workspace =
            LiveIdentity::new(scope, ObjectKind::Workspace, "workspace:opaque").unwrap();
        let tab = LiveIdentity::new(scope, ObjectKind::Tab, "tab:opaque").unwrap();
        match kind {
            ObjectKind::Workspace => LiveTarget::new(workspace, None, None).unwrap(),
            ObjectKind::Tab => LiveTarget::new(tab, Some(workspace), None).unwrap(),
            ObjectKind::Pane => LiveTarget::new(
                LiveIdentity::new(scope, ObjectKind::Pane, "pane:not-terminal").unwrap(),
                Some(workspace),
                Some(tab),
            )
            .unwrap(),
        }
    }
    #[test]
    fn exact_focus_uses_only_the_requested_kind_and_id_after_read_only_negotiation() {
        let socket = Socket::new(vec![
            Reply::Result(snapshot()),
            Reply::Result(json!({"type":"ok"})),
            Reply::Result(snapshot()),
            Reply::Result(json!({"type":"ok"})),
            Reply::Result(snapshot()),
            Reply::Result(json!({"type":"ok"})),
        ]);
        let mut context = NavigationContext::with_server(socket.server(Limits::default()));
        let scope = context.scope().clone();
        for kind in [ObjectKind::Pane, ObjectKind::Workspace, ObjectKind::Tab] {
            context.focus(&target(&scope, kind)).unwrap();
        }
        let requests = socket.requests.lock().unwrap();
        assert_eq!(requests.len(), 6);
        assert_eq!(requests[0]["method"], "session.snapshot");
        for (request, (method, params)) in requests[1..].iter().step_by(2).zip([
            ("pane.focus", json!({"pane_id":"pane:not-terminal"})),
            (
                "workspace.focus",
                json!({"workspace_id":"workspace:opaque"}),
            ),
            ("tab.focus", json!({"tab_id":"tab:opaque"})),
        ]) {
            assert_eq!(request["method"], method);
            assert_eq!(request["params"], params);
            assert_eq!(request.as_object().unwrap().len(), 3);
        }
        assert_eq!(
            requests
                .iter()
                .map(|r| r["id"].as_str().unwrap())
                .collect::<std::collections::HashSet<_>>()
                .len(),
            6
        );
        assert_eq!(context.scope(), &scope);
        assert!(context.current_client_selection().is_none());
        assert!(context.require_process_server_colocation().is_err());
    }
    #[test]
    fn foreign_scope_and_absent_route_never_send_or_fall_back_to_cli() {
        let runner = Arc::new(RecordingRunner::default());
        let mut route = crate::herdr::invocation();
        route.socket = None;
        let server = OrdinaryServer::new(route, Box::new(runner.clone()), Limits::default());
        let scope = NavigationContext::new().scope().clone();
        let foreign = NavigationContext::new().scope().clone();
        assert_eq!(
            server
                .focus(&scope, &target(&foreign, ObjectKind::Pane))
                .unwrap_err(),
            TransportError::new(Failure::StaleScope, MutationOutcome::NotSent)
        );
        assert_eq!(
            server
                .focus(&scope, &target(&scope, ObjectKind::Pane))
                .unwrap_err(),
            TransportError::new(Failure::Unsupported, MutationOutcome::NotSent)
        );
        assert!(runner.calls.lock().unwrap().is_empty());
        let scope = ServerScope::reported("owner-a", "session-a", "epoch-a").unwrap();
        for foreign in [
            ServerScope::reported("owner-b", "session-a", "epoch-a").unwrap(),
            ServerScope::reported("owner-a", "session-b", "epoch-a").unwrap(),
            ServerScope::reported("owner-a", "session-a", "epoch-b").unwrap(),
        ] {
            assert_eq!(
                server
                    .focus(&scope, &target(&foreign, ObjectKind::Pane))
                    .unwrap_err(),
                TransportError::new(Failure::StaleScope, MutationOutcome::NotSent)
            );
        }
        assert!(runner.calls.lock().unwrap().is_empty());
    }
    #[test]
    fn mismatched_version_or_protocol_never_enables_mutations() {
        for (version, protocol) in [("0.7.3", 22), ("0.9.0", 21), ("0.10.0", 22)] {
            let mut result = snapshot();
            result["snapshot"]["version"] = version.into();
            result["snapshot"]["protocol"] = protocol.into();
            let socket = Socket::new(vec![Reply::Result(result)]);
            let mut context = NavigationContext::with_server(socket.server(Limits::default()));
            let scope = context.scope().clone();
            assert_eq!(
                context
                    .focus(&target(&scope, ObjectKind::Pane))
                    .unwrap_err()
                    .outcome,
                MutationOutcome::NotSent
            );
            assert_eq!(
                context.server().capability(ObjectKind::Pane),
                Capability::Unknown
            );
            assert_eq!(context.scope(), &scope);
            assert_eq!(socket.requests.lock().unwrap().len(), 1);
        }
    }
    #[test]
    fn explicit_method_rejection_is_per_kind_and_target_rejection_keeps_scope() {
        let socket = Socket::new(vec![
            Reply::Result(snapshot()),
            Reply::Error("unknown_method"),
            Reply::Result(snapshot()),
            Reply::Error("target_not_found"),
        ]);
        let mut context = NavigationContext::with_server(socket.server(Limits::default()));
        let scope = context.scope().clone();
        assert_eq!(
            context
                .focus(&target(&scope, ObjectKind::Pane))
                .unwrap_err()
                .outcome,
            MutationOutcome::Rejected
        );
        assert_eq!(
            context.server().capability(ObjectKind::Pane),
            Capability::Rejected
        );
        assert_eq!(
            context.server().capability(ObjectKind::Workspace),
            Capability::Supported
        );
        assert_eq!(
            context
                .focus(&target(&scope, ObjectKind::Pane))
                .unwrap_err()
                .outcome,
            MutationOutcome::NotSent
        );
        assert_eq!(
            context
                .focus(&target(&scope, ObjectKind::Workspace))
                .unwrap_err()
                .outcome,
            MutationOutcome::Rejected
        );
        assert_eq!(
            context.server().capability(ObjectKind::Workspace),
            Capability::Supported
        );
        assert_eq!(context.scope(), &scope);
        assert_eq!(socket.requests.lock().unwrap().len(), 4);
    }
    #[test]
    fn uncertain_ack_invalidates_scope_and_capabilities_without_retry() {
        for reply in [
            Reply::WrongId,
            Reply::Truncated,
            Reply::Bytes(b"garbage\n".to_vec()),
        ] {
            assert_failed_focus(
                vec![Reply::Result(snapshot()), reply],
                TransportError::new(Failure::Malformed, MutationOutcome::Unknown),
            );
        }
    }

    fn assert_failed_focus(replies: Vec<Reply>, expected: TransportError) {
        let socket = Socket::new(replies);
        let limits = Limits::default();
        let mut context = NavigationContext::with_server(socket.server(limits));
        let scope = context.scope().clone();
        let target = target(&scope, ObjectKind::Pane);
        context.set_picker_owner(target.clone()).unwrap();
        let start = Instant::now();
        assert_eq!(context.focus(&target).unwrap_err(), expected);
        if expected.cause == Failure::Timeout {
            assert!(
                start.elapsed() >= limits.deadline,
                "client returned before its deadline"
            );
        }
        assert_ne!(context.scope(), &scope);
        assert!(context.picker_workspace().is_none());
        for kind in [ObjectKind::Workspace, ObjectKind::Tab, ObjectKind::Pane] {
            assert_eq!(context.server().capability(kind), Capability::Unknown);
        }
        assert_eq!(
            context.focus(&target).unwrap_err(),
            TransportError::new(Failure::StaleScope, MutationOutcome::NotSent)
        );
        {
            let requests = socket.requests.lock().unwrap();
            let methods: Vec<_> = requests
                .iter()
                .map(|request| request["method"].as_str().unwrap())
                .collect();
            if expected.outcome == MutationOutcome::NotSent {
                assert_eq!(methods, ["session.snapshot"]);
            } else {
                assert_eq!(methods, ["session.snapshot", "pane.focus"]);
                assert_eq!(
                    requests[1]["params"],
                    json!({"pane_id": "pane:not-terminal"})
                );
            }
        }
        drop(socket);
        assert!(
            start.elapsed() < Duration::from_secs(5),
            "client deadline or fixture cleanup exceeded watchdog"
        );
    }

    #[test]
    fn withheld_ack_times_out_after_mutation_and_invalidates_without_retry() {
        assert_failed_focus(
            vec![Reply::Result(snapshot()), Reply::Hang],
            TransportError::new(Failure::Timeout, MutationOutcome::Unknown),
        );
    }

    #[test]
    fn withheld_negotiation_times_out_without_sending_mutation() {
        assert_failed_focus(
            vec![Reply::Hang],
            TransportError::new(Failure::Timeout, MutationOutcome::NotSent),
        );
    }

    #[test]
    fn socket_cleanup_cancels_pending_accept_but_fails_missing_requests() {
        let socket = Socket::new(vec![Reply::Hang]);
        let path = socket.path.clone();
        let start = Instant::now();
        assert!(
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(move || drop(socket))).is_err()
        );
        assert!(!path.exists());
        assert!(start.elapsed() < Duration::from_secs(5));
    }

    #[test]
    fn raw_request_and_response_caps_are_enforced() {
        let socket = Socket::new(vec![Reply::Bytes(vec![b'x'; 100])]);
        let server = socket.server(Limits {
            stdout: 50,
            ..Limits::default()
        });
        let error = server
            .request(
                "session.snapshot",
                json!({}),
                "session_snapshot",
                50,
                Instant::now() + Duration::from_secs(1),
            )
            .unwrap_err();
        assert_eq!(error.cause, Failure::StdoutLimit);
        let socket = Socket::new(vec![]);
        let server = socket.server(Limits {
            request: 1,
            ..Limits::default()
        });
        let scope = NavigationContext::new().scope().clone();
        assert_eq!(
            server
                .focus(&scope, &target(&scope, ObjectKind::Pane))
                .unwrap_err(),
            TransportError::new(Failure::RequestLimit, MutationOutcome::NotSent)
        );
        assert!(socket.requests.lock().unwrap().is_empty());
    }
    #[test]
    fn response_parser_rejects_conflicting_trailing_missing_and_mistyped_values() {
        for text in [
            r#"{"id":"r","result":{"type":"ok"},"error":null}"#,
            r#"{"id":"r","id":"r","result":{"type":"ok"}}"#,
            r#"{"id":"r","result":{"type":"bad","type":"ok"}}"#,
            r#"{"id":"r","result":{"type":"ok"}} {}"#,
            r#"{"id":4,"result":{"type":"ok"}}"#,
            r#"{"result":{"type":"ok"}}"#,
            r#"{"id":"r","result":{}}"#,
            r#"{"id":"r","result":{"type":4}}"#,
            r#"{"id":"r","error":{"code":"bad","message":4}}"#,
            r#"{"id":"r","error":{"code":"bad"}}"#,
        ] {
            assert_eq!(
                response(text.as_bytes(), Some("r"), "ok")
                    .unwrap_err()
                    .cause,
                Failure::Malformed,
                "{text}"
            );
        }
        assert!(response(
            b"{\"id\":\"r\",\"result\":{\"type\":\"ok\"}}\n",
            Some("r"),
            "ok"
        )
        .is_ok());
    }
    #[test]
    fn cli_reads_validate_envelopes_and_required_fields_without_fixed_cli_ids() {
        let runner = Arc::new(RecordingRunner::default());
        runner.replies.lock().unwrap().extend([
            Ok(crate::navigation_test_support::WORKSPACE_LIST.as_bytes().to_vec()),
            Ok(crate::navigation_test_support::AGENT_LIST.as_bytes().to_vec()),
            Ok(br#"{"id":"cli-owned:any","result":{"type":"tab_list","tabs":[]}}"#.to_vec()),
            Ok(br#"{"id":"cli-owned:any","result":{"type":"pane_list","panes":[{"pane_id":"p"}]}}"#.to_vec()),
            Ok(br#"{"id":"cli-owned:any","result":{"type":"pane_list","panes":null}}"#.to_vec()),
        ]);
        let server = OrdinaryServer::new(
            crate::herdr::invocation(),
            Box::new(runner.clone()),
            Limits::default(),
        );
        assert_eq!(server.workspaces().unwrap().len(), 2);
        assert_eq!(server.agents().unwrap().len(), 1);
        assert!(server.tabs().is_ok());
        assert_eq!(server.panes().unwrap_err().cause, Failure::Malformed);
        assert_eq!(server.panes().unwrap_err().cause, Failure::Malformed);
        let calls = runner.calls.lock().unwrap();
        assert_eq!(calls.len(), 5);
        assert!(calls.iter().all(|call| call.stdin.is_none()));
    }
    #[test]
    fn typed_inventory_reads_all_four_kinds_and_validate_consumed_optional_metadata() {
        let lists = crate::navigation_test_support::ordinary_lists();
        let runner = Arc::new(RecordingRunner::default());
        runner.replies.lock().unwrap().extend(
            lists
                .clone()
                .map(|value| Ok(serde_json::to_vec(&value).unwrap())),
        );
        let server = OrdinaryServer::new(
            crate::herdr::invocation(),
            Box::new(runner.clone()),
            Limits::default(),
        );
        assert_eq!(server.workspaces().unwrap()[1].workspace_id, "w43");
        let panes = server.panes().unwrap();
        assert_eq!(panes[0].pane_id, "w43:p1");
        assert_eq!(panes[0].label.as_deref(), Some("Review key handling"));
        assert_eq!(server.tabs().unwrap()[0].label, "1");
        let agents = server.agents().unwrap();
        assert_eq!(
            agents[0].agent_session.as_ref().unwrap().value,
            "58f4-session"
        );
        let calls = runner.calls.lock().unwrap();
        for (call, kind) in calls.iter().zip(["workspace", "pane", "tab", "agent"]) {
            assert_eq!(call.args, [kind, "list"]);
        }
        drop(calls);
        for field in [
            "label",
            "title",
            "terminal_title",
            "terminal_title_stripped",
            "cwd",
            "foreground_cwd",
            "display_agent",
            "name",
        ] {
            let mut bad = lists[1].clone();
            bad["result"]["panes"][0][field] = 42.into();
            runner
                .replies
                .lock()
                .unwrap()
                .push_back(Ok(serde_json::to_vec(&bad).unwrap()));
            assert_eq!(
                server.panes().unwrap_err().cause,
                Failure::Malformed,
                "{field}"
            );
        }
        for session in [
            json!({"kind":"id","value":42}),
            json!({"kind":"future","value":"x","agent":"a","source":"s"}),
            json!("not-an-object"),
        ] {
            let mut bad = lists[3].clone();
            bad["result"]["agents"][0]["agent_session"] = session;
            runner
                .replies
                .lock()
                .unwrap()
                .push_back(Ok(serde_json::to_vec(&bad).unwrap()));
            assert_eq!(server.agents().unwrap_err().cause, Failure::Malformed);
        }
        runner.replies.lock().unwrap().extend([
            Ok(br#"{"id":"r","error":{"code":"unsupported_method","message":"optional rejection"}}"#.to_vec()),
            Err(TransportError::new(Failure::Timeout, MutationOutcome::Unknown)),
        ]);
        assert_eq!(
            server.agents().unwrap_err().outcome,
            MutationOutcome::Rejected
        );
        assert_eq!(server.tabs().unwrap_err().cause, Failure::Timeout);
    }

    #[test]
    fn changed_server_version_is_rechecked_before_another_mutation() {
        let mut changed = snapshot();
        changed["snapshot"]["version"] = "unverified".into();
        let socket = Socket::new(vec![
            Reply::Result(snapshot()),
            Reply::Result(json!({"type":"ok"})),
            Reply::Result(changed),
        ]);
        let mut context = NavigationContext::with_server(socket.server(Limits::default()));
        let target = target(context.scope(), ObjectKind::Pane);
        context.focus(&target).unwrap();
        let old_scope = context.scope().clone();
        assert_eq!(
            context.focus(&target).unwrap_err().outcome,
            MutationOutcome::NotSent
        );
        assert_eq!(
            context.server().capability(ObjectKind::Pane),
            Capability::Unknown
        );
        assert_eq!(socket.requests.lock().unwrap().len(), 3);
        assert_ne!(context.scope(), &old_scope);
    }
}
