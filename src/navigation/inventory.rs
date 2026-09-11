//! Typed ordinary-server inventory. Each read keeps its own scope and outcome.
use std::time::Instant;

use serde::Deserialize;

use crate::{herdr::TransportError, model::ServerScope};

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
pub(crate) enum AgentStatus {
    Idle,
    Working,
    Blocked,
    Done,
    Unknown,
}
impl AgentStatus {
    pub(crate) fn text(self) -> &'static str {
        match self {
            Self::Idle => "idle",
            Self::Working => "working",
            Self::Blocked => "blocked",
            Self::Done => "done",
            Self::Unknown => "unknown",
        }
    }
}
#[derive(Clone, Debug, Deserialize)]
pub(crate) struct WorkspaceRecord {
    pub(crate) workspace_id: String,
    pub(crate) label: String,
    pub(crate) focused: bool,
    pub(crate) tab_count: u64,
    pub(crate) pane_count: u64,
    pub(crate) agent_status: AgentStatus,
}
#[derive(Clone, Debug, Deserialize)]
pub(crate) struct TabRecord {
    pub(crate) tab_id: String,
    pub(crate) workspace_id: String,
    pub(crate) label: String,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum TabNaming {
    #[allow(dead_code)] // Explicit client/fixture evidence only, absent in protocol 22.
    Named,
    #[allow(dead_code)] // Never derived from the label "1".
    Unnamed,
    // Protocol 22 reports a label, not whether a user named the tab.
    Unknown,
}
#[derive(Clone, Debug, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
pub(crate) enum AgentSessionKind {
    Id,
    Path,
}
#[derive(Clone, Debug, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct AgentSession {
    pub(crate) agent: String,
    pub(crate) source: String,
    pub(crate) kind: AgentSessionKind,
    pub(crate) value: String,
}
#[derive(Clone, Debug, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct PaneRecord {
    pub(crate) pane_id: String,
    pub(crate) workspace_id: String,
    pub(crate) tab_id: String,
    pub(crate) terminal_id: String,
    pub(crate) revision: u64,
    pub(crate) label: Option<String>,
    pub(crate) title: Option<String>,
    pub(crate) terminal_title: Option<String>,
    pub(crate) terminal_title_stripped: Option<String>,
    pub(crate) cwd: Option<String>,
    pub(crate) foreground_cwd: Option<String>,
}
#[derive(Clone, Debug, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct AgentRecord {
    pub(crate) pane_id: String,
    pub(crate) workspace_id: String,
    pub(crate) tab_id: String,
    pub(crate) terminal_id: String,
    pub(crate) revision: u64,
    pub(crate) agent: Option<String>,
    pub(crate) name: Option<String>,
    pub(crate) display_agent: Option<String>,
    pub(crate) agent_status: AgentStatus,
    pub(crate) agent_session: Option<AgentSession>,
}
#[derive(Clone, Debug)]
pub(crate) enum ReadState<T> {
    Disabled,
    Ready(Vec<T>),
    Unavailable(TransportError),
    Stale,
}
#[derive(Clone, Debug)]
pub(crate) struct InventoryRead<T> {
    pub(crate) scope: ServerScope,
    pub(crate) observed_at: Instant,
    pub(crate) state: ReadState<T>,
}
impl<T> InventoryRead<T> {
    pub(crate) fn new(scope: &ServerScope, result: Option<Result<Vec<T>, TransportError>>) -> Self {
        Self {
            scope: scope.clone(),
            observed_at: Instant::now(),
            state: match result {
                None => ReadState::Disabled,
                Some(Ok(rows)) => ReadState::Ready(rows),
                Some(Err(error)) => ReadState::Unavailable(error),
            },
        }
    }
    pub(crate) fn rows(&self, scope: &ServerScope) -> &[T] {
        match &self.state {
            ReadState::Ready(rows) if &self.scope == scope => rows,
            _ => &[],
        }
    }
    pub(crate) fn error(&self) -> Option<&TransportError> {
        match &self.state {
            ReadState::Unavailable(error) => Some(error),
            _ => None,
        }
    }
    pub(crate) fn count(&self) -> Option<usize> {
        match &self.state {
            ReadState::Ready(rows) => Some(rows.len()),
            _ => None,
        }
    }
    pub(crate) fn discard_old_scope(&mut self, scope: &ServerScope) {
        if &self.scope != scope && matches!(self.state, ReadState::Ready(_)) {
            self.state = ReadState::Stale;
        }
    }
    pub(crate) fn description(&self, kind: &str) -> String {
        match &self.state {
            ReadState::Disabled => format!("{kind}: disabled"),
            ReadState::Ready(_) => format!(
                "{kind}: {} observed {}s ago",
                self.count().unwrap_or(0),
                self.observed_at.elapsed().as_secs()
            ),
            ReadState::Unavailable(error) => format!("{kind}: unavailable, {error}"),
            ReadState::Stale => format!("{kind}: stale, discarded after scope change"),
        }
    }
}
#[derive(Clone, Debug)]
pub(crate) struct Inventory {
    pub(crate) workspaces: InventoryRead<WorkspaceRecord>,
    pub(crate) tabs: InventoryRead<TabRecord>,
    pub(crate) panes: InventoryRead<PaneRecord>,
    pub(crate) agents: InventoryRead<AgentRecord>,
}
impl Inventory {
    pub(crate) fn uncertain_error(&self) -> Option<&TransportError> {
        [
            self.workspaces.error(),
            self.tabs.error(),
            self.panes.error(),
            self.agents.error(),
        ]
        .into_iter()
        .flatten()
        .find(|error| error.uncertain_continuity())
    }
    pub(crate) fn discard_old_scope(&mut self, scope: &ServerScope) {
        self.workspaces.discard_old_scope(scope);
        self.tabs.discard_old_scope(scope);
        self.panes.discard_old_scope(scope);
        self.agents.discard_old_scope(scope);
    }
    pub(crate) fn descriptions(&self) -> Vec<String> {
        vec![
            self.workspaces.description("workspaces"),
            self.tabs.description("tabs"),
            self.panes.description("panes"),
            self.agents.description("agents"),
        ]
    }
}
