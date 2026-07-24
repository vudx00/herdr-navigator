use std::{cell::OnceCell, path::PathBuf};

use serde::{Deserialize, Serialize};

use crate::paths::canonical_str;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum Source {
    Workspace,
    Project,
    Zoxide,
    Root,
    Agent,
    Server,
    Session,
    QuickAction,
    Integration,
}

impl Source {
    pub(crate) fn label(&self) -> &'static str {
        match self {
            Source::Workspace => "open",
            Source::Project => "project",
            Source::Zoxide => "zoxide",
            Source::Root => "root",
            Source::Agent => "agent",
            Source::Server => "server",
            Source::Session => "session",
            Source::QuickAction => "quick",
            Source::Integration => "plugin",
        }
    }

    /// Allocation-free: runs for every `source_order` entry on every filter pass.
    pub(crate) fn from_config(value: &str) -> Option<Self> {
        const ALIASES: &[(Source, &[&str])] = &[
            (
                Source::Workspace,
                &["workspace", "workspaces", "open", "open_workspaces"],
            ),
            (
                Source::Project,
                &["project", "projects", "herdr_plus_projects"],
            ),
            (Source::Zoxide, &["zoxide", "z"]),
            (Source::Root, &["root", "roots", "scan"]),
            (Source::Agent, &["agent", "agents"]),
            (
                Source::Server,
                &["server", "servers", "remote", "remotes", "ssh"],
            ),
            (Source::Session, &["session", "sessions"]),
            (
                Source::QuickAction,
                &[
                    "quick",
                    "quick_action",
                    "quick_actions",
                    "herdr_plus_quick_actions",
                ],
            ),
            (
                Source::Integration,
                &["plugin", "integration", "integrations"],
            ),
        ];
        let value = value.trim();
        ALIASES
            .iter()
            .find(|(_, aliases)| {
                aliases
                    .iter()
                    .any(|alias| value.eq_ignore_ascii_case(alias))
            })
            .map(|(source, _)| *source)
    }

    pub(crate) const COUNT: usize = 9;

    /// Dense index for per-source lookup tables.
    pub(crate) const fn index(self) -> usize {
        self as usize
    }

    pub(crate) fn all() -> [Source; Self::COUNT] {
        [
            Source::Workspace,
            Source::Project,
            Source::Server,
            Source::Session,
            Source::Zoxide,
            Source::Root,
            Source::Agent,
            Source::QuickAction,
            Source::Integration,
        ]
    }
}

// A new variant would index past every `[_; Source::COUNT]` lookup table.
const _: () = assert!(Source::Integration.index() + 1 == Source::COUNT);

#[derive(Clone, Debug)]
pub(crate) enum EntryAction {
    FocusWorkspace {
        id: String,
    },
    FocusAgent {
        target: String,
    },
    OpenProject,
    OpenRemote {
        target: String,
    },
    AttachSession {
        name: String,
        remote: Option<String>,
    },
    InvokePluginAction {
        action: String,
    },
    FocusOrCreateDir,
    RunCommand {
        command: String,
        timeout_ms: u64,
        notify_success: bool,
        notify_error: bool,
    },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum WorkspaceKind {
    Project,
    Dir,
    Unknown,
}

#[derive(Clone, Debug)]
pub(crate) struct WorkspaceRef {
    pub(crate) id: String,
    pub(crate) label: String,
    pub(crate) kind: WorkspaceKind,
    pub(crate) path: PathBuf,
    pub(crate) tab_count: i64,
    pub(crate) pane_count: i64,
}

#[derive(Clone, Debug)]
pub(crate) struct Entry {
    pub(crate) source: Source,
    pub(crate) title: String,
    pub(crate) subtitle: String,
    pub(crate) path: PathBuf,
    pub(crate) workspace_id: Option<String>,
    pub(crate) workspace_label: Option<String>,
    pub(crate) agent_target: Option<String>,
    pub(crate) project: Option<Project>,
    pub(crate) action: EntryAction,
    pub(crate) source_label: Option<String>,
    pub(crate) search_terms: Vec<String>,
    /// Memoized [`Entry::key`]: canonicalizing hits the filesystem, and the key
    /// is read once per row per frame plus once per filter pass.
    pub(crate) canonical_key: OnceCell<String>,
}

impl Entry {
    pub(crate) fn key(&self) -> &str {
        self.canonical_key.get_or_init(|| {
            canonical_str(&self.path).unwrap_or_else(|| self.path.display().to_string())
        })
    }

    pub(crate) fn source_name(&self) -> &str {
        self.source_label
            .as_deref()
            .unwrap_or_else(|| self.source.label())
    }

    pub(crate) fn search_fields(&self) -> Vec<String> {
        fn push(fields: &mut Vec<String>, value: &str) {
            if value.is_empty() {
                return;
            }
            let value = value.to_lowercase();
            if !fields.contains(&value) {
                fields.push(value);
            }
        }

        let mut fields = Vec::with_capacity(6 + self.search_terms.len());
        push(&mut fields, &self.title);
        if let Some(name) = self.path.file_name().and_then(|name| name.to_str()) {
            push(&mut fields, name);
        }
        push(&mut fields, &self.path.to_string_lossy());
        for component in self.path.components() {
            push(&mut fields, &component.as_os_str().to_string_lossy());
        }
        push(&mut fields, &self.subtitle);
        if let Some(label) = self.workspace_label.as_deref() {
            push(&mut fields, label);
        }
        push(&mut fields, self.source_name());
        for term in &self.search_terms {
            push(&mut fields, term);
        }
        fields
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub(crate) struct Project {
    pub(crate) name: String,
    #[serde(default)]
    pub(crate) description: String,
    pub(crate) working_dir: String,
    #[serde(default)]
    pub(crate) tabs: Vec<ProjectTab>,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
pub(crate) struct ProjectTab {
    pub(crate) name: String,
    pub(crate) command: Option<String>,
    #[serde(default)]
    pub(crate) panes: Vec<ProjectPane>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub(crate) struct ProjectPane {
    pub(crate) command: Option<String>,
    pub(crate) split: Option<String>,
    pub(crate) label: Option<String>,
}

impl ProjectTab {
    pub(crate) fn effective_panes(&self) -> Vec<ProjectPane> {
        if self.panes.is_empty() {
            return vec![ProjectPane {
                command: self.command.clone(),
                split: None,
                label: None,
            }];
        }

        self.panes
            .iter()
            .cloned()
            .enumerate()
            .map(|(index, mut pane)| {
                pane.split = match (index, pane.split.as_deref()) {
                    (0, _) => None,
                    (_, Some(split)) if !split.is_empty() => Some(split.into()),
                    _ => Some("down".into()),
                };
                pane
            })
            .collect()
    }
}
