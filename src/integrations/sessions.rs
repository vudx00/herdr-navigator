use std::{path::PathBuf, process::Command};

use serde::Deserialize;
use serde_json::Value;

use crate::{
    config::{Config, SessionEntryConfig},
    herdr::herdr_json,
    model::{Entry, EntryAction, EntryIdentity, ProviderId, Source},
    paths::OwnedPath,
};

#[derive(Debug, Deserialize)]
struct SessionList {
    #[serde(default)]
    sessions: Vec<ListedSession>,
}

#[derive(Debug, Deserialize)]
struct ListedSession {
    name: String,
    #[serde(default)]
    running: bool,
    #[serde(default)]
    default: bool,
    session_dir: Option<String>,
}

pub(crate) fn collect_sessions(config: &Config) -> Vec<Entry> {
    let mut entries = Vec::new();
    if config.sessions.local {
        entries.extend(collect_local_sessions());
    }
    entries.extend(
        config
            .sessions
            .entries
            .iter()
            .filter(|entry| {
                entry.remote.is_none()
                    && !entry
                        .session
                        .as_deref()
                        .unwrap_or(&entry.name)
                        .trim()
                        .is_empty()
            })
            .map(manual_session_entry),
    );
    entries
}

pub(crate) fn collect_remotes(config: &Config) -> Vec<Entry> {
    config
        .sessions
        .entries
        .iter()
        .filter_map(remote_entry)
        .collect()
}

fn collect_local_sessions() -> Vec<Entry> {
    let json = herdr_json(["session", "list", "--json"]).unwrap_or(Value::Null);
    let list: SessionList = serde_json::from_value(json.clone())
        .or_else(|_| {
            serde_json::from_value(json.pointer("/result").cloned().unwrap_or(Value::Null))
        })
        .unwrap_or(SessionList { sessions: vec![] });
    list.sessions
        .into_iter()
        .filter(|session| !session.name.trim().is_empty())
        .map(local_session_entry)
        .collect()
}

fn local_session_entry(session: ListedSession) -> Entry {
    let mut flags = Vec::new();
    if session.default {
        flags.push("default");
    }
    if session.running {
        flags.push("running");
    }
    let subtitle = if flags.is_empty() {
        "local session".into()
    } else {
        format!("local session · {}", flags.join(" · "))
    };
    let path = session
        .session_dir
        .as_ref()
        .map(|path| OwnedPath::process(PathBuf::from(path)));
    Entry::new(
        EntryIdentity::provider("sessions", &session.name, "attach").unwrap(),
        crate::model::EntryData {
            source: Source::Session,
            title: session.name.clone(),
            subtitle,
            path,
            workspace_id: None,
            workspace_label: None,
            agent: None,
            project: None,
            action: EntryAction::AttachSession {
                name: session.name,
                remote: None,
            },
            source_label: None,
            search_terms: vec!["local".into(), "session".into()],
            provenance: [ProviderId::new("sessions").unwrap()].into(),
        },
    )
}

fn manual_session_entry(config: &SessionEntryConfig) -> Entry {
    let session = config.session.as_deref().unwrap_or(&config.name);
    let path = None;
    let mut search_terms = vec!["session".into(), session.into()];
    search_terms.extend(config.tags.iter().cloned());
    Entry::new(
        EntryIdentity::provider("sessions", session, "attach").unwrap(),
        crate::model::EntryData {
            source: Source::Session,
            title: config.name.clone(),
            subtitle: format!("local session · {session}"),
            path,
            workspace_id: None,
            workspace_label: None,
            agent: None,
            project: None,
            action: EntryAction::AttachSession {
                name: session.into(),
                remote: None,
            },
            source_label: None,
            search_terms,
            provenance: [ProviderId::new("sessions").unwrap()].into(),
        },
    )
}

fn remote_entry(config: &SessionEntryConfig) -> Option<Entry> {
    let target = config.remote.clone()?;
    let mut search_terms = vec!["server".into(), "remote".into(), target.clone()];
    search_terms.extend(config.tags.iter().cloned());
    Some(Entry::new(
        EntryIdentity::provider("configured-remotes", &target, "attach-remote")?,
        crate::model::EntryData {
            source: Source::Server,
            title: config.name.clone(),
            subtitle: format!("remote Herdr · {target}"),
            path: None,
            workspace_id: None,
            workspace_label: None,
            agent: None,
            project: None,
            action: EntryAction::OpenRemote { target },
            source_label: None,
            search_terms,
            provenance: [ProviderId::new("configured-remotes").unwrap()].into(),
        },
    ))
}

pub(crate) fn attach_session(route: &crate::herdr::Invocation, name: &str) -> Result<(), String> {
    attach(route.interactive(true).args(["session", "attach", name]))
}

pub(crate) fn open_remote(route: &crate::herdr::Invocation, target: &str) -> Result<(), String> {
    attach(
        route
            .interactive(false)
            .args(["--remote", target, "--handoff"]),
    )
}

fn attach(command: &mut Command) -> Result<(), String> {
    let status = command.status().map_err(|error| error.to_string())?;
    if status.success() {
        Ok(())
    } else {
        Err(format!("herdr exited with {status}"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn local_session_entry_attaches_by_name() {
        let entry = local_session_entry(ListedSession {
            name: "work".into(),
            running: true,
            default: false,
            session_dir: Some("/tmp/herdr-work".into()),
        });

        assert_eq!(entry.source, Source::Session);
        assert!(entry.subtitle.contains("running"));
        assert!(matches!(
            entry.action,
            EntryAction::AttachSession { ref name, remote: None } if name == "work"
        ));
    }

    #[test]
    fn manual_remote_entry_opens_remote_target() {
        let entry = remote_entry(&SessionEntryConfig {
            name: "prod".into(),
            remote: Some("prod-box".into()),
            session: Some("default".into()),
            tags: vec!["api".into()],
        })
        .unwrap();

        assert_eq!(entry.source, Source::Server);
        assert!(entry.has_provenance(Source::Server));
        assert_eq!(
            entry.destination_kind(),
            crate::model::DestinationKind::Session
        );
        assert!(entry
            .search_fields()
            .iter()
            .any(|field| field.contains("prod-box")));
        assert!(matches!(
            entry.action,
            EntryAction::OpenRemote { ref target } if target == "prod-box"
        ));
    }
    #[test]
    fn session_keys_follow_executed_attach_targets_without_fake_paths() {
        let mut config = SessionEntryConfig {
            name: "label".into(),
            session: Some("work".into()),
            remote: None,
            tags: vec![],
        };
        let local = manual_session_entry(&config);
        assert!(local.path.is_none());
        config.name = "renamed".into();
        assert_eq!(local.key(), manual_session_entry(&config).key());
        config.session = Some("other".into());
        assert_ne!(local.key(), manual_session_entry(&config).key());
        config.remote = Some("target".into());
        let remote = remote_entry(&config).unwrap();
        assert!(remote.path.is_none());
        config.session = Some("not-used-by-remote-handoff".into());
        assert_eq!(remote.key(), remote_entry(&config).unwrap().key());
        config.remote = Some("different-target".into());
        assert_ne!(remote.key(), remote_entry(&config).unwrap().key());
    }
}
