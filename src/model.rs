use std::collections::HashSet;

use serde::{Deserialize, Serialize};

use crate::{navigation::ProcessHostRef, paths::OwnedPath};

/// Ephemeral references are meaningful only in this process, not in saved state.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
enum IdentityKey {
    Ephemeral(u64),
    Reported(String),
}
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub(crate) struct MachineKey(IdentityKey);
impl MachineKey {
    // Machine inventory remains unavailable.
    #[allow(dead_code)]
    pub(crate) fn new(id: &str) -> Option<Self> {
        (!id.trim().is_empty()).then(|| Self(IdentityKey::Reported(id.into())))
    }
}

/// The selected session is action context, not part of machine identity.
#[derive(Clone, Debug)]
pub(crate) struct MachineIdentity {
    key: MachineKey,
    selected_session: Option<SessionKey>,
}
impl MachineIdentity {
    // No production machine collector exists yet.
    #[allow(dead_code)]
    pub(crate) fn new(key: MachineKey, selected_session: Option<SessionKey>) -> Option<Self> {
        if selected_session
            .as_ref()
            .is_some_and(|session| session.machine != key)
        {
            return None;
        }
        Some(Self {
            key,
            selected_session,
        })
    }

    #[allow(dead_code)] // N08 fixture identity; production client inventory is unavailable.
    pub(crate) fn key(&self) -> &MachineKey {
        &self.key
    }

    // N14 resolves native naming against this session once that capability exists.
    #[allow(dead_code)]
    pub(crate) fn selected_session(&self) -> Option<&SessionKey> {
        self.selected_session.as_ref()
    }
}
impl PartialEq for MachineIdentity {
    fn eq(&self, other: &Self) -> bool {
        self.key == other.key
    }
}
impl Eq for MachineIdentity {}
impl std::hash::Hash for MachineIdentity {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.key.hash(state);
    }
}
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub(crate) struct SessionKey {
    machine: MachineKey,
    id: IdentityKey,
}
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub(crate) struct ServerScope {
    session: SessionKey,
    incarnation: IdentityKey,
}
impl SessionKey {
    #[allow(dead_code)] // N08 fixture identity; ordinary scopes do not identify machines.
    pub(crate) fn machine(&self) -> &MachineKey {
        &self.machine
    }
}
impl ServerScope {
    pub(crate) fn ephemeral(invocation: u64) -> Self {
        Self {
            session: SessionKey {
                machine: MachineKey(IdentityKey::Ephemeral(invocation)),
                id: IdentityKey::Ephemeral(invocation),
            },
            incarnation: IdentityKey::Ephemeral(invocation),
        }
    }
    // Protocol 22 has no durable session identity. Invalidation keeps this reference.
    #[allow(dead_code)]
    pub(crate) fn session(&self) -> &SessionKey {
        &self.session
    }
    pub(crate) fn rotate_incarnation(&mut self, generation: u64) {
        self.incarnation = IdentityKey::Ephemeral(generation);
    }
    /// Fixture construction exercises the production scope shape without a reported-identity transport.
    #[cfg(test)]
    pub(crate) fn reported(machine: &str, session: &str, incarnation: &str) -> Option<Self> {
        if [machine, session, incarnation]
            .iter()
            .any(|id| id.trim().is_empty())
        {
            return None;
        }
        Some(Self {
            session: SessionKey {
                machine: MachineKey(IdentityKey::Reported(machine.into())),
                id: IdentityKey::Reported(session.into()),
            },
            incarnation: IdentityKey::Reported(incarnation.into()),
        })
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum ObjectKind {
    Workspace,
    Tab,
    Pane,
}
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub(crate) struct LiveIdentity {
    scope: ServerScope,
    kind: ObjectKind,
    id: String,
}
impl LiveIdentity {
    pub(crate) fn new(scope: &ServerScope, kind: ObjectKind, id: &str) -> Option<Self> {
        (!id.trim().is_empty()).then(|| Self {
            scope: scope.clone(),
            kind,
            id: id.into(),
        })
    }
    pub(crate) fn kind(&self) -> ObjectKind {
        self.kind
    }
    pub(crate) fn id(&self) -> &str {
        &self.id
    }
    pub(crate) fn scope(&self) -> &ServerScope {
        &self.scope
    }
}
#[derive(Clone, Debug)]
pub(crate) struct LiveTarget {
    identity: LiveIdentity,
    workspace: Option<LiveIdentity>,
    tab: Option<LiveIdentity>,
}
impl LiveTarget {
    pub(crate) fn new(
        identity: LiveIdentity,
        workspace: Option<LiveIdentity>,
        tab: Option<LiveIdentity>,
    ) -> Option<Self> {
        let valid_parent =
            |parent: &LiveIdentity, kind| parent.kind == kind && parent.scope == identity.scope;
        let valid = match identity.kind {
            ObjectKind::Workspace => workspace.is_none() && tab.is_none(),
            ObjectKind::Tab => {
                workspace
                    .as_ref()
                    .is_some_and(|p| valid_parent(p, ObjectKind::Workspace))
                    && tab.is_none()
            }
            ObjectKind::Pane => {
                workspace
                    .as_ref()
                    .is_some_and(|p| valid_parent(p, ObjectKind::Workspace))
                    && tab
                        .as_ref()
                        .is_some_and(|p| valid_parent(p, ObjectKind::Tab))
            }
        };
        valid.then_some(Self {
            identity,
            workspace,
            tab,
        })
    }
    pub(crate) fn identity(&self) -> &LiveIdentity {
        &self.identity
    }
    pub(crate) fn workspace(&self) -> Option<&LiveIdentity> {
        self.workspace.as_ref()
    }
    pub(crate) fn tab(&self) -> Option<&LiveIdentity> {
        self.tab.as_ref()
    }
}
impl PartialEq for LiveTarget {
    fn eq(&self, other: &Self) -> bool {
        self.identity == other.identity
    }
}
impl Eq for LiveTarget {}
impl std::hash::Hash for LiveTarget {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.identity.hash(state);
    }
}
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub(crate) struct ProviderId {
    namespace: ProviderNamespace,
    id: String,
}
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
enum ProviderNamespace {
    Builtin,
    Command,
}
impl ProviderId {
    pub(crate) fn preference_parts(&self) -> (&str, &str) {
        let namespace = match self.namespace {
            ProviderNamespace::Builtin => "builtin",
            ProviderNamespace::Command => "command",
        };
        (namespace, &self.id)
    }
    pub(crate) fn command(id: &str) -> Option<Self> {
        (!id.trim().is_empty()).then(|| Self {
            namespace: ProviderNamespace::Command,
            id: id.into(),
        })
    }
    pub(crate) fn new(id: &str) -> Option<Self> {
        (!id.trim().is_empty()).then(|| Self {
            namespace: ProviderNamespace::Builtin,
            id: id.into(),
        })
    }
}
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub(crate) enum EntryIdentity {
    Machine(MachineIdentity),
    Live(Box<LiveTarget>),
    Provider {
        owner: ProcessHostRef,
        provider: ProviderId,
        item: String,
        action: String,
    },
}
impl EntryIdentity {
    pub(crate) fn live(target: LiveTarget) -> Self {
        Self::Live(Box::new(target))
    }
    pub(crate) fn command(provider: &str, item: &str, action: &str) -> Option<Self> {
        if item.trim().is_empty() || action.trim().is_empty() {
            return None;
        }
        Some(Self::Provider {
            owner: ProcessHostRef,
            provider: ProviderId::command(provider)?,
            item: item.into(),
            action: action.into(),
        })
    }
    pub(crate) fn provider(provider: &str, item: &str, action: &str) -> Option<Self> {
        if item.trim().is_empty() || action.trim().is_empty() {
            return None;
        }
        Some(Self::Provider {
            owner: ProcessHostRef,
            provider: ProviderId::new(provider)?,
            item: item.into(),
            action: action.into(),
        })
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum DestinationKind {
    Machine,
    Workspace,
    Tab,
    Pane,
    Project,
    Folder,
    Session,
    Action,
}
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub(crate) struct WorkspaceReuseKey {
    scope: ServerScope,
    path: String,
}
impl WorkspaceReuseKey {
    pub(crate) fn new(scope: ServerScope, path: String) -> Self {
        Self { scope, path }
    }
}

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

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum EntryAction {
    UnavailableNativeWorkspace,
    FocusWorkspace {
        id: String,
    },
    FocusPane,
    FocusTab,
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
    pub(crate) identity: LiveIdentity,
    pub(crate) label: String,
    pub(crate) kind: WorkspaceKind,
    pub(crate) path: Option<OwnedPath>,
    pub(crate) tab_count: u64,
    pub(crate) pane_count: u64,
}

#[derive(Clone, Debug)]
pub(crate) struct Entry {
    identity: EntryIdentity,
    pub(crate) data: EntryData,
    pub(crate) pane: Option<PaneMetadata>,
    pub(crate) template_file: Option<OwnedPath>,
    pub(crate) action_conflict: Option<String>,
}

#[derive(Clone, Debug)]
pub(crate) struct AgentMetadata {
    pub(crate) agent: Option<String>,
    pub(crate) name: Option<String>,
    pub(crate) display_agent: Option<String>,
    pub(crate) aliases: Vec<String>,
    pub(crate) status: String,
    pub(crate) revision: u64,
    pub(crate) session: Option<crate::navigation::inventory::AgentSession>,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum AgentFreshness {
    Disabled,
    Observed,
    Unknown,
}
#[derive(Clone, Debug)]
pub(crate) struct PaneMetadata {
    pub(crate) tab_label: String,
    pub(crate) tab_naming: crate::navigation::inventory::TabNaming,
    pub(crate) terminal_id: String,
    pub(crate) ordinal: usize,
    pub(crate) agent_freshness: AgentFreshness,
}

#[derive(Clone, Debug)]
pub(crate) struct EntryData {
    pub(crate) source: Source,
    pub(crate) title: String,
    pub(crate) subtitle: String,
    pub(crate) path: Option<OwnedPath>,
    pub(crate) workspace_id: Option<String>,
    pub(crate) workspace_label: Option<String>,
    pub(crate) agent: Option<AgentMetadata>,
    pub(crate) project: Option<Project>,
    pub(crate) action: EntryAction,
    pub(crate) source_label: Option<String>,
    pub(crate) search_terms: Vec<String>,
    pub(crate) provenance: HashSet<ProviderId>,
}

impl std::ops::Deref for Entry {
    type Target = EntryData;
    fn deref(&self) -> &Self::Target {
        &self.data
    }
}
impl std::ops::DerefMut for Entry {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.data
    }
}
impl PartialEq for Entry {
    fn eq(&self, other: &Self) -> bool {
        self.identity == other.identity
    }
}
impl Eq for Entry {}
impl std::hash::Hash for Entry {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.identity.hash(state);
    }
}
impl Entry {
    // No production machine inventory is available.
    #[allow(dead_code)]
    pub(crate) fn machine(identity: MachineIdentity, title: String, status: String) -> Self {
        Self::new(
            EntryIdentity::Machine(identity),
            EntryData {
                source: Source::Server,
                title,
                subtitle: status,
                path: None,
                workspace_id: None,
                workspace_label: None,
                agent: None,
                project: None,
                action: EntryAction::UnavailableNativeWorkspace,
                source_label: None,
                search_terms: Vec::new(),
                provenance: HashSet::new(),
            },
        )
    }
    pub(crate) fn new(identity: EntryIdentity, data: EntryData) -> Self {
        Self {
            identity,
            data,
            pane: None,
            template_file: None,
            action_conflict: None,
        }
    }
    pub(crate) fn target(&self) -> Option<&LiveTarget> {
        match &self.identity {
            EntryIdentity::Live(target) => Some(target),
            _ => None,
        }
    }
    pub(crate) fn key(&self) -> &EntryIdentity {
        &self.identity
    }
    pub(crate) fn destination_kind(&self) -> DestinationKind {
        if matches!(self.identity, EntryIdentity::Machine(_)) {
            return DestinationKind::Machine;
        }
        if let Some(target) = self.target() {
            return match target.identity.kind {
                ObjectKind::Workspace => DestinationKind::Workspace,
                ObjectKind::Tab => DestinationKind::Tab,
                ObjectKind::Pane => DestinationKind::Pane,
            };
        }
        match self.action {
            EntryAction::FocusWorkspace { .. } => DestinationKind::Workspace,
            EntryAction::FocusPane => DestinationKind::Pane,
            EntryAction::FocusTab => DestinationKind::Tab,
            EntryAction::OpenProject => DestinationKind::Project,
            EntryAction::FocusOrCreateDir => DestinationKind::Folder,
            EntryAction::AttachSession { .. } | EntryAction::OpenRemote { .. } => {
                DestinationKind::Session
            }
            _ => DestinationKind::Action,
        }
    }
    /// Compatibility provenance includes merged providers, not destination type.
    pub(crate) fn has_provenance(&self, source: Source) -> bool {
        // Discovered machines are not configured remotes despite their legacy category.
        if source == Source::Server && matches!(self.key(), EntryIdentity::Machine(_)) {
            return false;
        }
        if self.source == source
            || (source == Source::Workspace && self.target().is_some())
            || (source == Source::Agent && self.agent.is_some())
        {
            return true;
        }
        let matches = |provider: &ProviderId| {
            let (namespace, id) = provider.preference_parts();
            if namespace == "command" {
                return source == Source::Integration;
            }
            match source {
                Source::Workspace => matches!(id, "workspaces" | "tabs" | "panes"),
                Source::Agent => id == "agents",
                Source::Project => id == "herdr-plus-projects",
                Source::Zoxide => id == "zoxide",
                Source::Root => id == "roots",
                Source::Server => id == "configured-remotes",
                Source::Session => id == "sessions",
                Source::QuickAction => id == "herdr-plus",
                Source::Integration => false,
            }
        };
        self.provenance.iter().any(matches)
            || matches!(&self.identity, EntryIdentity::Provider { provider, .. } if matches(provider))
    }

    pub(crate) fn path_text(&self) -> String {
        self.path.as_ref().map(OwnedPath::text).unwrap_or_default()
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
        let path = self.path_text();
        if let Some(name) = path.rsplit('/').find(|part| !part.is_empty()) {
            push(&mut fields, name);
        }
        push(&mut fields, &path);
        for component in path.split('/') {
            push(&mut fields, component);
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

/// Coarse agent state derived from Herdr's free-form status text. Every
/// consumer (row icon, row color, priority sort, spinner animation) maps this
/// one classification instead of matching keywords on its own.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum AgentState {
    /// Blocked, failed or errored.
    Blocked,
    /// Waiting on the user: needs attention, review, a question or a request.
    Attention,
    Working,
    Done,
    Idle,
    Unknown,
}

impl AgentState {
    const BLOCKED: &'static [&'static str] = &["block", "fail", "error"];
    const ATTENTION: &'static [&'static str] =
        &["need", "attention", "review", "request", "question", "wait"];

    /// Case-insensitive keyword classification. Earlier variants win when a
    /// status mentions several states.
    pub(crate) fn classify(status: &str) -> Self {
        let status = status.to_lowercase();
        let any = |needles: &[&str]| needles.iter().any(|needle| status.contains(needle));
        if any(Self::BLOCKED) {
            Self::Blocked
        } else if any(Self::ATTENTION) {
            Self::Attention
        } else if any(&["work", "run"]) {
            Self::Working
        } else if any(&["done", "complete"]) {
            Self::Done
        } else if status.contains("idle") {
            Self::Idle
        } else {
            Self::Unknown
        }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub(crate) struct Project {
    pub(crate) name: String,
    #[serde(default)]
    pub(crate) description: String,
    pub(crate) working_dir: String,
    #[serde(default)]
    pub(crate) tabs: Vec<ProjectTab>,
}
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub(crate) struct ProjectTab {
    pub(crate) name: String,
    pub(crate) command: Option<String>,
    #[serde(default)]
    pub(crate) panes: Vec<ProjectPane>,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
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

#[cfg(test)]
mod domain_tests {
    use super::*;

    #[test]
    fn agent_status_classify_covers_each_state() {
        for (status, expected) in [
            ("blocked", AgentState::Blocked),
            ("Failed", AgentState::Blocked),
            ("error: exit 1", AgentState::Blocked),
            ("needs input", AgentState::Attention),
            ("Needs Attention", AgentState::Attention),
            ("review", AgentState::Attention),
            ("permission request", AgentState::Attention),
            ("question", AgentState::Attention),
            ("waiting", AgentState::Attention),
            ("working", AgentState::Working),
            ("Running", AgentState::Working),
            ("done", AgentState::Done),
            ("completed", AgentState::Done),
            ("idle", AgentState::Idle),
            ("unknown", AgentState::Unknown),
            ("", AgentState::Unknown),
        ] {
            assert_eq!(AgentState::classify(status), expected, "{status:?}");
        }
    }

    #[test]
    fn agent_status_classify_prefers_more_urgent_state() {
        assert_eq!(
            AgentState::classify("blocked, waiting"),
            AgentState::Blocked
        );
        assert_eq!(
            AgentState::classify("waiting while running"),
            AgentState::Attention
        );
        assert_eq!(AgentState::classify("work done"), AgentState::Working);
    }

    #[test]
    fn provenance_includes_provider_identity_and_merged_sources_without_changing_kind() {
        let machine = Entry::machine(
            MachineIdentity::new(MachineKey::new("fixture").unwrap(), None).unwrap(),
            "Fixture".into(),
            String::new(),
        );
        assert!(!machine.has_provenance(Source::Server));
        assert!(!machine.has_provenance(Source::Session));
        assert_eq!(machine.source, Source::Server);
        assert_eq!(machine.destination_kind(), DestinationKind::Machine);
        assert!(matches!(machine.key(), EntryIdentity::Machine(_)));
        assert!(machine.provenance.is_empty());
        assert!(matches!(
            machine.action,
            EntryAction::UnavailableNativeWorkspace
        ));
        let mut folder = Entry::new(
            EntryIdentity::provider("roots", "folder", "open").unwrap(),
            machine.data,
        );
        folder.source = Source::Project;
        folder.action = EntryAction::FocusOrCreateDir;
        folder.provenance.insert(ProviderId::new("zoxide").unwrap());
        folder
            .provenance
            .insert(ProviderId::command("custom").unwrap());
        for source in [
            Source::Project,
            Source::Root,
            Source::Zoxide,
            Source::Integration,
        ] {
            assert!(folder.has_provenance(source));
        }
        for source in [
            Source::Server,
            Source::Workspace,
            Source::Agent,
            Source::Session,
            Source::QuickAction,
        ] {
            assert!(!folder.has_provenance(source));
        }
        assert_eq!(folder.destination_kind(), DestinationKind::Folder);
    }

    #[test]
    fn machine_constructor_rejects_blank_keys_and_foreign_selected_sessions() {
        assert!(MachineKey::new(" ").is_none());
        let scope = ServerScope::reported("machine", "session", "epoch").unwrap();
        assert!(MachineIdentity::new(
            MachineKey::new("other").unwrap(),
            Some(scope.session.clone())
        )
        .is_none());
        let machine = MachineIdentity::new(
            MachineKey::new("machine").unwrap(),
            Some(scope.session.clone()),
        )
        .unwrap();
        assert_eq!(machine.selected_session(), Some(&scope.session));
    }

    #[test]
    fn machine_key_ignores_session_incarnation_and_display_changes() {
        let key = MachineKey::new("machine").unwrap();
        let mut entries = HashSet::new();
        for session in ["one", "two"] {
            for epoch in ["old", "new"] {
                let scope = ServerScope::reported("machine", session, epoch).unwrap();
                let machine = MachineIdentity::new(key.clone(), Some(scope.session)).unwrap();
                let mut entry = Entry::machine(machine, session.into(), epoch.into());
                assert_eq!(entry.destination_kind(), DestinationKind::Machine);
                assert!(entry.target().is_none());
                assert!(entry.workspace_id.is_none());
                assert!(entry.path.is_none());
                assert!(matches!(
                    entry.action,
                    EntryAction::UnavailableNativeWorkspace
                ));
                let original = entry.key().clone();
                entry.title = "renamed".into();
                entry.subtitle = "offline".into();
                assert_eq!(entry.key(), &original);
                entries.insert(entry);
            }
        }
        entries.insert(Entry::machine(
            MachineIdentity::new(key, None).unwrap(),
            "unknown session".into(),
            String::new(),
        ));
        assert_eq!(entries.len(), 1);
    }

    #[test]
    fn machine_live_kinds_and_provider_sessions_do_not_collide() {
        let mut keys = HashSet::new();
        for machine in ["one", "two"] {
            let scope = ServerScope::reported(machine, "same", "same").unwrap();
            keys.insert(EntryIdentity::Machine(
                MachineIdentity::new(
                    MachineKey::new(machine).unwrap(),
                    Some(scope.session.clone()),
                )
                .unwrap(),
            ));
            let workspace = LiveIdentity::new(&scope, ObjectKind::Workspace, "same").unwrap();
            let tab = LiveIdentity::new(&scope, ObjectKind::Tab, "same").unwrap();
            let pane = LiveIdentity::new(&scope, ObjectKind::Pane, "same").unwrap();
            keys.insert(EntryIdentity::live(
                LiveTarget::new(workspace.clone(), None, None).unwrap(),
            ));
            keys.insert(EntryIdentity::live(
                LiveTarget::new(tab.clone(), Some(workspace.clone()), None).unwrap(),
            ));
            keys.insert(EntryIdentity::live(
                LiveTarget::new(pane, Some(workspace), Some(tab)).unwrap(),
            ));
        }
        keys.insert(EntryIdentity::provider("sessions", "same", "attach").unwrap());
        assert_eq!(keys.len(), 9);
    }

    #[test]
    fn collision_fixture_has_twelve_distinct_production_live_keys() {
        let fixture: serde_json::Value =
            serde_json::from_str(crate::navigation_test_support::COLLISIONS).unwrap();
        let mut keys = HashSet::new();
        for row in fixture["expected"]["destinations"].as_array().unwrap() {
            let parts = row["identity"].as_array().unwrap();
            let scope = ServerScope::reported(
                parts[0].as_str().unwrap(),
                parts[1].as_str().unwrap(),
                parts[2].as_str().unwrap(),
            )
            .unwrap();
            let kind = match parts[3].as_str().unwrap() {
                "workspace" => ObjectKind::Workspace,
                "tab" => ObjectKind::Tab,
                "pane" => ObjectKind::Pane,
                _ => panic!("unexpected fixture kind"),
            };
            let identity = LiveIdentity::new(&scope, kind, parts[4].as_str().unwrap()).unwrap();
            let parents = row["ancestors"].as_array().unwrap();
            let workspace = parents.first().map(|id| {
                LiveIdentity::new(&scope, ObjectKind::Workspace, id.as_str().unwrap()).unwrap()
            });
            let tab = parents.get(1).map(|id| {
                LiveIdentity::new(&scope, ObjectKind::Tab, id.as_str().unwrap()).unwrap()
            });
            let target = LiveTarget::new(identity, workspace, tab).unwrap();
            assert!(keys.insert(EntryIdentity::live(target)));
        }
        assert_eq!(keys.len(), 12);
    }

    #[test]
    fn constructors_reject_empty_ids_wrong_parent_kinds_and_cross_scope_ancestry() {
        let scope = ServerScope::ephemeral(1);
        let other = ServerScope::ephemeral(2);
        assert!(LiveIdentity::new(&scope, ObjectKind::Pane, " ").is_none());
        assert!(ServerScope::reported("machine", "", "epoch").is_none());
        assert!(EntryIdentity::provider("", "item", "run").is_none());
        assert!(EntryIdentity::provider("provider", "", "run").is_none());
        assert!(EntryIdentity::provider("provider", "item", "").is_none());
        let workspace = LiveIdentity::new(&scope, ObjectKind::Workspace, "w:opaque").unwrap();
        let tab = LiveIdentity::new(&scope, ObjectKind::Tab, "no-colon").unwrap();
        let pane = LiveIdentity::new(&scope, ObjectKind::Pane, "w:looks:like:a:path").unwrap();
        let foreign_tab = LiveIdentity::new(&other, ObjectKind::Tab, "no-colon").unwrap();
        assert!(LiveTarget::new(workspace.clone(), Some(tab.clone()), None).is_none());
        assert!(LiveTarget::new(tab.clone(), None, None).is_none());
        assert!(
            LiveTarget::new(pane.clone(), Some(workspace.clone()), Some(foreign_tab)).is_none()
        );
        assert!(
            LiveTarget::new(pane.clone(), Some(tab.clone()), Some(workspace.clone())).is_none()
        );
        assert!(LiveTarget::new(pane, Some(workspace), Some(tab)).is_some());
    }

    #[test]
    fn machine_session_and_incarnation_are_separate_references() {
        let first = ServerScope::reported("machine", "one", "epoch").unwrap();
        let second = ServerScope::reported("machine", "two", "epoch").unwrap();
        let restarted = ServerScope::reported("machine", "one", "next").unwrap();
        assert_eq!(first.session.machine, second.session.machine);
        assert_ne!(first.session, second.session);
        assert_eq!(first.session, restarted.session);
        assert_ne!(first, restarted);
        assert_ne!(first, ServerScope::ephemeral(1));
    }

    #[test]
    fn provider_item_action_and_namespace_all_contribute_to_identity() {
        let keys: HashSet<_> = [
            EntryIdentity::provider("sessions", "work", "attach"),
            EntryIdentity::provider("sessions", "other", "attach"),
            EntryIdentity::provider("sessions", "work", "different-action"),
            EntryIdentity::provider("other", "work", "attach"),
            EntryIdentity::command("sessions", "work", "attach"),
        ]
        .into_iter()
        .map(Option::unwrap)
        .collect();
        assert_eq!(keys.len(), 5);
    }
}
