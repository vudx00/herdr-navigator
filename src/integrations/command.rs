use std::{
    cell::OnceCell,
    io::Read,
    process::{Command, ExitStatus, Stdio},
    thread,
    time::{Duration, Instant},
};

use serde::Deserialize;

use crate::{
    config::IntegrationConfig,
    model::{Entry, EntryAction, Source},
    paths::{expand_path, home},
};

#[derive(Debug, Deserialize)]
struct IntegrationItem {
    id: String,
    title: String,
    #[serde(default)]
    subtitle: String,
    path: Option<String>,
    #[serde(default)]
    kind: String,
}

pub(crate) fn collect(integrations: &[IntegrationConfig]) -> Vec<Entry> {
    let enabled = integrations
        .iter()
        .filter(|integration| integration.enabled)
        .collect::<Vec<_>>();
    let mut entries = Vec::new();
    thread::scope(|scope| {
        for chunk in enabled.chunks(4) {
            let handles = chunk
                .iter()
                .map(|integration| scope.spawn(|| collect_one(integration)))
                .collect::<Vec<_>>();
            for handle in handles {
                entries.extend(handle.join().unwrap_or_default());
            }
        }
    });
    entries
}

fn collect_one(integration: &IntegrationConfig) -> Vec<Entry> {
    if integration.id.trim().is_empty() {
        return vec![];
    }
    let Ok(stdout) = run_shell_capture(
        &integration.collect,
        integration.collect_timeout_ms,
        integration.max_output_bytes,
    ) else {
        return vec![];
    };
    parse_items(&stdout)
        .unwrap_or_default()
        .into_iter()
        .map(|item| entry_from_item(integration, item))
        .collect()
}

fn parse_items(bytes: &[u8]) -> Result<Vec<IntegrationItem>, serde_json::Error> {
    serde_json::from_slice(bytes)
}

fn entry_from_item(integration: &IntegrationConfig, item: IntegrationItem) -> Entry {
    let path = item.path.as_deref().map(expand_path).unwrap_or_else(home);
    let subtitle = subtitle(integration, &item);
    let command = render_template(&integration.open, &item);
    let id = item.id.clone();
    let kind = item.kind.clone();
    let source = match kind.as_str() {
        "server" | "remote-terminal" => Source::Server,
        "session" => Source::Session,
        _ => Source::Integration,
    };
    Entry {
        source,
        title: item.title,
        subtitle,
        path,
        workspace_id: None,
        workspace_label: None,
        agent_target: None,
        project: None,
        action: EntryAction::RunCommand {
            command,
            timeout_ms: integration.open_timeout_ms,
            notify_success: integration.notify_success,
            notify_error: integration.notify_error,
        },
        source_label: (!matches!(kind.as_str(), "server" | "remote-terminal" | "session"))
            .then(|| integration.label.clone()),
        search_terms: vec![id, kind],
        canonical_key: OnceCell::new(),
    }
}

fn subtitle(integration: &IntegrationConfig, item: &IntegrationItem) -> String {
    match (item.kind.is_empty(), item.subtitle.is_empty()) {
        (true, true) => integration.label.clone(),
        (true, false) => format!("{} · {}", integration.label, item.subtitle),
        (false, true) => format!("{} · {}", integration.label, item.kind),
        (false, false) => format!("{} · {} · {}", integration.label, item.kind, item.subtitle),
    }
}

fn render_template(template: &str, item: &IntegrationItem) -> String {
    let path = item.path.as_deref().unwrap_or("");
    template
        .replace("{{id}}", &shell_quote(&item.id))
        .replace("{{title}}", &shell_quote(&item.title))
        .replace("{{subtitle}}", &shell_quote(&item.subtitle))
        .replace("{{path}}", &shell_quote(path))
        .replace("{{kind}}", &shell_quote(&item.kind))
}

fn shell_quote(value: &str) -> String {
    if value.is_empty() {
        "''".into()
    } else {
        format!("'{}'", value.replace('\'', "'\\''"))
    }
}

fn shell(command: &str) -> Command {
    let mut process = Command::new("sh");
    process.arg("-c").arg(command);
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        process.process_group(0);
    }
    process
}

fn wait_with_timeout(
    child: &mut std::process::Child,
    timeout_ms: u64,
) -> Result<ExitStatus, String> {
    let deadline = (timeout_ms > 0).then(|| Instant::now() + Duration::from_millis(timeout_ms));
    loop {
        if let Some(status) = child.try_wait().map_err(|error| error.to_string())? {
            return Ok(status);
        }
        if deadline.is_some_and(|deadline| Instant::now() >= deadline) {
            terminate_process_group(child);
            let _ = child.wait();
            return Err(format!("command timed out after {timeout_ms}ms"));
        }
        thread::sleep(Duration::from_millis(10));
    }
}

fn terminate_process_group(child: &mut std::process::Child) {
    #[cfg(unix)]
    // Kill the group so timed-out grandchildren cannot survive or hold pipes open.
    unsafe {
        libc::kill(-(child.id() as i32), libc::SIGKILL);
    }
    let _ = child.kill();
}

fn run_shell_capture(command: &str, timeout_ms: u64, max_bytes: usize) -> Result<Vec<u8>, String> {
    let mut child = shell(command)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|error| error.to_string())?;
    let stdout = child.stdout.take().ok_or("collect command has no stdout")?;
    let limit = max_bytes.saturating_add(1) as u64;
    let reader = thread::spawn(move || {
        let mut bytes = Vec::new();
        stdout
            .take(limit)
            .read_to_end(&mut bytes)
            .map(|_| bytes)
            .map_err(|error| error.to_string())
    });
    let status = wait_with_timeout(&mut child, timeout_ms)?;
    let bytes = reader
        .join()
        .map_err(|_| "collect output reader panicked".to_string())??;
    if bytes.len() > max_bytes {
        return Err(format!("command output exceeded {max_bytes} bytes"));
    }
    if !status.success() {
        return Err(format!("command exited with {status}"));
    }
    Ok(bytes)
}

pub(crate) fn run_command(command: &str, timeout_ms: u64) -> Result<(), String> {
    let mut child = shell(command).spawn().map_err(|error| error.to_string())?;
    let status = wait_with_timeout(&mut child, timeout_ms)?;
    if status.success() {
        Ok(())
    } else {
        Err(format!("command exited with {status}"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn config() -> IntegrationConfig {
        IntegrationConfig {
            id: "demo".into(),
            label: "Demo".into(),
            enabled: true,
            collect: "demo list --json".into(),
            open: "demo open {{id}} --path {{path}}".into(),
            collect_timeout_ms: 5_000,
            open_timeout_ms: 0,
            max_output_bytes: 1_048_576,
            notify_success: true,
            notify_error: true,
        }
    }

    #[test]
    fn parses_collect_json() {
        let items = parse_items(
            br#"[{"id":"abc","title":"Item","subtitle":"Info","path":"/tmp","kind":"action"}]"#,
        )
        .unwrap();

        assert_eq!(items.len(), 1);
        assert_eq!(items[0].id, "abc");
        assert_eq!(items[0].title, "Item");
    }

    #[test]
    fn renders_shell_safe_open_command() {
        let item = IntegrationItem {
            id: "a b".into(),
            title: "It's fine".into(),
            subtitle: String::new(),
            path: Some("/tmp/a b".into()),
            kind: "action".into(),
        };

        assert_eq!(
            render_template("demo open {{id}} --title {{title}} --path {{path}}", &item),
            "demo open 'a b' --title 'It'\\''s fine' --path '/tmp/a b'"
        );
    }

    #[test]
    fn session_kind_builds_session_source() {
        let cfg = config();
        let item = IntegrationItem {
            id: "nn".into(),
            title: "nn".into(),
            subtitle: "session nn".into(),
            path: Some("/tmp/session/nn".into()),
            kind: "session".into(),
        };
        let entry = entry_from_item(&cfg, item);

        assert_eq!(entry.source, Source::Session);
        assert_eq!(entry.source_name(), "session");
    }

    #[test]
    fn server_kind_builds_server_source() {
        let cfg = config();
        let item = IntegrationItem {
            id: "s87".into(),
            title: "s87".into(),
            subtitle: "autossh/ssh s87".into(),
            path: Some("/tmp/server/s87".into()),
            kind: "server".into(),
        };
        let entry = entry_from_item(&cfg, item);

        assert_eq!(entry.source, Source::Server);
        assert_eq!(entry.source_name(), "server");
    }

    #[test]
    fn remote_terminal_kind_builds_server_source() {
        let cfg = config();
        let item = IntegrationItem {
            id: "s211::term_abc".into(),
            title: "term_abc".into(),
            subtitle: "remote terminal".into(),
            path: Some("remote:s211:term_abc".into()),
            kind: "remote-terminal".into(),
        };
        let entry = entry_from_item(&cfg, item);

        assert_eq!(entry.source, Source::Server);
        assert_eq!(entry.source_name(), "server");
    }

    #[test]
    fn failed_collect_is_optional() {
        let mut cfg = config();
        cfg.collect = "exit 7".into();

        assert!(collect(&[cfg]).is_empty());
    }

    #[test]
    fn collect_timeout_and_output_limit_are_enforced() {
        assert!(run_shell_capture("sleep 1", 20, 1024)
            .unwrap_err()
            .contains("timed out"));
        assert!(run_shell_capture("printf 12345", 1000, 4)
            .unwrap_err()
            .contains("exceeded"));
    }

    #[test]
    fn builds_entry_with_run_command_action() {
        let cfg = config();
        let item = IntegrationItem {
            id: "abc".into(),
            title: "Item".into(),
            subtitle: "Info".into(),
            path: Some("/tmp".into()),
            kind: "action".into(),
        };
        let entry = entry_from_item(&cfg, item);

        assert_eq!(entry.source, Source::Integration);
        assert_eq!(entry.title, "Item");
        assert!(matches!(entry.action, EntryAction::RunCommand { .. }));
        assert_eq!(entry.path, std::path::PathBuf::from("/tmp"));
    }
}
