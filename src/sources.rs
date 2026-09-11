use std::{
    collections::HashMap,
    fs,
    path::{Path, PathBuf},
    process::Command,
};

#[cfg(test)]
use serde_json::Value;

use crate::{
    config::Config,
    model::{
        AgentFreshness, AgentMetadata, Entry, EntryAction, EntryIdentity, LiveIdentity, LiveTarget,
        ObjectKind, PaneMetadata, ProviderId, Source, WorkspaceKind, WorkspaceRef,
        WorkspaceReuseKey,
    },
    navigation::{
        inventory::{Inventory, ReadState, TabNaming},
        NavigationContext,
    },
    paths::{basename, expand_path, OwnedPath},
};

/// Ordinals are assigned by Navigator within one scope and survive refresh order changes.
#[derive(Default)]
pub(crate) struct PaneOrdinals {
    scope: Option<crate::model::ServerScope>,
    values: HashMap<LiveIdentity, usize>,
}
impl PaneOrdinals {
    fn assign(
        &mut self,
        scope: &crate::model::ServerScope,
        panes: &[crate::navigation::inventory::PaneRecord],
    ) {
        if self.scope.as_ref() != Some(scope) {
            self.values.clear();
            self.scope = Some(scope.clone());
        }
        let mut ids: Vec<_> = panes.iter().map(|pane| pane.pane_id.as_str()).collect();
        ids.sort_unstable();
        ids.dedup();
        for id in ids {
            if let Some(identity) = LiveIdentity::new(scope, ObjectKind::Pane, id) {
                let next = self.values.len() + 1;
                self.values.entry(identity).or_insert(next);
            }
        }
    }
}
#[derive(Default)]
pub(crate) struct JoinedInventory {
    pub(crate) workspaces: Vec<Entry>,
    pub(crate) tabs: Vec<Entry>,
    pub(crate) panes: Vec<Entry>,
    pub(crate) reuse: HashMap<WorkspaceReuseKey, Vec<WorkspaceRef>>,
    // Invalid records remain diagnostics, never guessed focus targets.
    pub(crate) unavailable: Vec<String>,
}
impl JoinedInventory {
    pub(crate) fn visible_panes(&self, all_panes: bool) -> Vec<Entry> {
        self.panes
            .iter()
            .filter(|pane| all_panes || pane.agent.is_some())
            .cloned()
            .collect()
    }
}
fn nonblank(value: Option<&str>) -> Option<&str> {
    value
        .map(str::trim)
        .filter(|text| !text.is_empty() && !text.chars().any(char::is_control))
}
fn useful_title<'a>(
    value: Option<&'a str>,
    pane: &crate::navigation::inventory::PaneRecord,
) -> Option<&'a str> {
    nonblank(value).filter(|text| {
        ![
            pane.pane_id.as_str(),
            pane.terminal_id.as_str(),
            pane.tab_id.as_str(),
            pane.workspace_id.as_str(),
        ]
        .contains(text)
            && !matches!(
                text.to_ascii_lowercase().as_str(),
                "unknown" | "untitled" | "pane" | "terminal"
            )
    })
}

/// Join only actual inventory membership. Neither a label nor a terminal ID is an identity.
pub(crate) fn join_inventory(
    inventory: &Inventory,
    context: &NavigationContext,
    ordinals: &mut PaneOrdinals,
    aliases: &[crate::config::AgentAliasConfig],
) -> JoinedInventory {
    let scope = context.scope();
    let mut out = JoinedInventory::default();
    let workspaces = inventory.workspaces.rows(scope);
    let tabs = inventory.tabs.rows(scope);
    let panes = inventory.panes.rows(scope);
    ordinals.assign(scope, panes);
    let mut ws_by_id = HashMap::new();
    let mut duplicate_ws = std::collections::HashSet::new();
    for ws in workspaces {
        if ws_by_id.insert(ws.workspace_id.as_str(), ws).is_some() {
            duplicate_ws.insert(ws.workspace_id.as_str());
        }
    }
    let mut tab_by_id = HashMap::new();
    let mut duplicate_tabs = std::collections::HashSet::new();
    for tab in tabs {
        if tab_by_id.insert(tab.tab_id.as_str(), tab).is_some() {
            duplicate_tabs.insert(tab.tab_id.as_str());
        }
    }
    let mut pane_by_id = HashMap::new();
    let mut duplicate_panes = std::collections::HashSet::new();
    for pane in panes {
        if pane_by_id.insert(pane.pane_id.as_str(), pane).is_some() {
            duplicate_panes.insert(pane.pane_id.as_str());
        }
    }
    for id in &duplicate_ws {
        out.unavailable
            .push(format!("workspace {id}: unavailable, duplicate identity"));
    }
    for tab in tabs {
        if duplicate_tabs.contains(tab.tab_id.as_str())
            || duplicate_ws.contains(tab.workspace_id.as_str())
            || !ws_by_id.contains_key(tab.workspace_id.as_str())
        {
            out.unavailable.push(format!(
                "tab {}: unavailable ancestry or duplicate identity",
                tab.tab_id
            ));
            continue;
        }
        let Some(identity) = LiveIdentity::new(scope, ObjectKind::Tab, &tab.tab_id) else {
            continue;
        };
        let Some(target) = LiveTarget::new(
            identity,
            LiveIdentity::new(scope, ObjectKind::Workspace, &tab.workspace_id),
            None,
        ) else {
            continue;
        };
        out.tabs.push(Entry::new(
            EntryIdentity::live(target),
            crate::model::EntryData {
                source: Source::Workspace,
                title: tab.label.clone(),
                subtitle: ws_by_id[tab.workspace_id.as_str()].label.clone(),
                path: None,
                workspace_id: Some(tab.workspace_id.clone()),
                workspace_label: Some(ws_by_id[tab.workspace_id.as_str()].label.clone()),
                agent: None,
                project: None,
                action: EntryAction::FocusTab,
                source_label: None,
                search_terms: vec![tab.tab_id.clone(), tab.workspace_id.clone()],
                provenance: [ProviderId::new("tabs").unwrap()].into(),
            },
        ));
    }
    let mut agent_by_pane: HashMap<&str, &crate::navigation::inventory::AgentRecord> =
        HashMap::new();
    for agent in inventory.agents.rows(scope) {
        let valid = pane_by_id.get(agent.pane_id.as_str()).is_some_and(|pane| {
            pane.workspace_id == agent.workspace_id
                && pane.tab_id == agent.tab_id
                && !duplicate_panes.contains(agent.pane_id.as_str())
        });
        if !valid {
            out.unavailable
                .push(format!("agent on {}: unavailable pane link", agent.pane_id));
            continue;
        }
        // Highest revision wins. Equal revisions use the typed record's total order,
        // so arrival order cannot change which metadata is displayed.
        agent_by_pane
            .entry(&agent.pane_id)
            .and_modify(|old| {
                if (agent.revision, agent) > (old.revision, *old) {
                    *old = agent;
                }
            })
            .or_insert(agent);
    }
    let agent_freshness = match &inventory.agents.state {
        ReadState::Disabled => AgentFreshness::Disabled,
        ReadState::Ready(_) if &inventory.agents.scope == scope => AgentFreshness::Observed,
        _ => AgentFreshness::Unknown,
    };
    let mut cwd_by_ws = HashMap::new();
    for pane in panes {
        let ws = ws_by_id.get(pane.workspace_id.as_str());
        let tab = tab_by_id.get(pane.tab_id.as_str());
        let valid = ws.is_some()
            && tab.is_some_and(|tab| tab.workspace_id == pane.workspace_id)
            && !duplicate_ws.contains(pane.workspace_id.as_str())
            && !duplicate_tabs.contains(pane.tab_id.as_str())
            && !duplicate_panes.contains(pane.pane_id.as_str());
        if !valid {
            out.unavailable.push(format!(
                "pane {}: unavailable ancestry or duplicate identity",
                pane.pane_id
            ));
            continue;
        }
        let (Some(ws), Some(tab)) = (ws, tab) else {
            continue;
        };
        let Some(identity) = LiveIdentity::new(scope, ObjectKind::Pane, &pane.pane_id) else {
            continue;
        };
        let Some(target) = LiveTarget::new(
            identity.clone(),
            LiveIdentity::new(scope, ObjectKind::Workspace, &ws.workspace_id),
            LiveIdentity::new(scope, ObjectKind::Tab, &tab.tab_id),
        ) else {
            continue;
        };
        let cwd = pane
            .foreground_cwd
            .as_deref()
            .filter(|cwd| !cwd.trim().is_empty())
            .or(pane.cwd.as_deref());
        let path = cwd.and_then(|cwd| OwnedPath::reported(scope, cwd));
        if let Some(path) = &path {
            cwd_by_ws
                .entry(ws.workspace_id.as_str())
                .or_insert_with(|| path.clone());
        }
        let agent = agent_by_pane.get(pane.pane_id.as_str()).map(|agent| {
            let name = nonblank(agent.agent.as_deref());
            AgentMetadata {
                agent: name.map(str::to_owned),
                name: agent.name.clone(),
                display_agent: agent.display_agent.clone(),
                aliases: aliases
                    .iter()
                    .filter(|alias| alias.matches(name.unwrap_or(""), &ws.label, cwd.unwrap_or("")))
                    .map(|alias| alias.alias.clone())
                    .collect(),
                status: agent.agent_status.text().into(),
                revision: agent.revision,
                session: agent.agent_session.clone(),
            }
        });
        let ordinal = ordinals.values[&identity];
        let title = nonblank(pane.label.as_deref())
            .or_else(|| {
                agent.as_ref().and_then(|a| {
                    nonblank(a.name.as_deref())
                        .or_else(|| nonblank(a.display_agent.as_deref()))
                        .or(a.agent.as_deref())
                })
            })
            .or_else(|| useful_title(pane.title.as_deref(), pane))
            .or_else(|| useful_title(pane.terminal_title_stripped.as_deref(), pane))
            .or_else(|| useful_title(pane.terminal_title.as_deref(), pane))
            .map(str::to_owned)
            .unwrap_or_else(|| format!("Pane {ordinal}"));
        let mut search_terms = vec![
            pane.pane_id.clone(),
            tab.tab_id.clone(),
            tab.label.clone(),
            ws.workspace_id.clone(),
            ws.label.clone(),
        ];
        let mut provenance = std::collections::HashSet::from([ProviderId::new("panes").unwrap()]);
        if let Some(agent) = &agent {
            provenance.insert(ProviderId::new("agents").unwrap());
            search_terms.extend(agent.agent.iter().cloned());
            search_terms.push(agent.status.clone());
            search_terms.extend(agent.name.iter().chain(agent.display_agent.iter()).cloned());
            search_terms.extend(agent.aliases.clone());
            if let Some(session) = &agent.session {
                search_terms.push(session.value.clone());
            }
        }
        let mut entry = Entry::new(
            EntryIdentity::live(target),
            crate::model::EntryData {
                source: if agent.is_some() {
                    Source::Agent
                } else {
                    Source::Workspace
                },
                title,
                subtitle: format!("{} · {}", ws.label, tab.label),
                path,
                workspace_id: Some(ws.workspace_id.clone()),
                workspace_label: Some(ws.label.clone()),
                agent,
                project: None,
                action: EntryAction::FocusPane,
                source_label: None,
                search_terms,
                provenance,
            },
        );
        entry.pane = Some(PaneMetadata {
            tab_label: tab.label.clone(),
            tab_naming: TabNaming::Unknown,
            terminal_id: pane.terminal_id.clone(),
            ordinal,
            agent_freshness,
        });
        out.panes.push(entry);
    }
    for ws in workspaces {
        if duplicate_ws.contains(ws.workspace_id.as_str()) {
            continue;
        }
        let Some(identity) = LiveIdentity::new(scope, ObjectKind::Workspace, &ws.workspace_id)
        else {
            continue;
        };
        let path = cwd_by_ws.get(ws.workspace_id.as_str()).cloned();
        if let Some(key) = path.as_ref().and_then(|path| context.reuse_key(path).ok()) {
            out.reuse.entry(key).or_default().push(WorkspaceRef {
                identity: identity.clone(),
                label: ws.label.clone(),
                kind: workspace_kind(&ws.label),
                path: path.clone(),
                tab_count: ws.tab_count,
                pane_count: ws.pane_count,
            });
        }
        let status = ws.agent_status.text();
        let mut search_terms = vec![ws.workspace_id.clone(), ws.label.clone(), status.into()];
        if ws.focused {
            search_terms.push("focused".into());
        }
        out.workspaces.push(Entry::new(
            EntryIdentity::live(LiveTarget::new(identity, None, None).unwrap()),
            crate::model::EntryData {
                source: Source::Workspace,
                title: ws.label.clone(),
                subtitle: format!(
                    "agent:{status} · {} tabs:{} panes:{}",
                    ws.workspace_id, ws.tab_count, ws.pane_count
                ),
                path,
                workspace_id: Some(ws.workspace_id.clone()),
                workspace_label: Some(ws.label.clone()),
                agent: None,
                project: None,
                action: EntryAction::FocusWorkspace {
                    id: ws.workspace_id.clone(),
                },
                source_label: None,
                search_terms,
                provenance: [ProviderId::new("workspaces").unwrap()].into(),
            },
        ));
    }
    out
}

// Older tests supply minimal workspace objects. Add synthetic ancestry, then use
// the same typed join as production rather than maintaining a second projection.
#[cfg(test)]
pub(crate) fn workspaces_from_json(
    ws_json: &Value,
    cwd_by_ws: &HashMap<String, String>,
    context: &NavigationContext,
) -> (Vec<Entry>, HashMap<WorkspaceReuseKey, Vec<WorkspaceRef>>) {
    use crate::navigation::inventory::{
        AgentStatus, InventoryRead, PaneRecord, TabRecord, WorkspaceRecord,
    };
    let mut workspaces = Vec::new();
    let mut panes = Vec::new();
    let mut tabs = Vec::new();
    for w in ws_json["result"]["workspaces"]
        .as_array()
        .into_iter()
        .flatten()
    {
        let id = w["workspace_id"].as_str().unwrap_or("");
        if id.trim().is_empty() {
            continue;
        }
        workspaces.push(WorkspaceRecord {
            workspace_id: id.into(),
            label: w["label"].as_str().unwrap_or(id).into(),
            focused: w["focused"].as_bool().unwrap_or(false),
            tab_count: w["tab_count"].as_u64().unwrap_or(0),
            pane_count: w["pane_count"].as_u64().unwrap_or(0),
            agent_status: serde_json::from_value(w["agent_status"].clone())
                .unwrap_or(AgentStatus::Unknown),
        });
        if let Some(cwd) = cwd_by_ws.get(id) {
            let tab_id = format!("synthetic-tab-{id}");
            tabs.push(TabRecord {
                tab_id: tab_id.clone(),
                workspace_id: id.into(),
                label: "1".into(),
            });
            panes.push(PaneRecord {
                pane_id: format!("synthetic-pane-{id}"),
                workspace_id: id.into(),
                tab_id,
                terminal_id: format!("synthetic-terminal-{id}"),
                revision: 0,
                label: None,
                title: None,
                terminal_title: None,
                terminal_title_stripped: None,
                cwd: Some(cwd.clone()),
                foreground_cwd: None,
            });
        }
    }
    let inventory = Inventory {
        workspaces: InventoryRead::new(context.scope(), Some(Ok(workspaces))),
        tabs: InventoryRead::new(context.scope(), Some(Ok(tabs))),
        panes: InventoryRead::new(context.scope(), Some(Ok(panes))),
        agents: InventoryRead::new(context.scope(), None),
    };
    let joined = join_inventory(&inventory, context, &mut PaneOrdinals::default(), &[]);
    (joined.workspaces, joined.reuse)
}

fn workspace_kind(label: &str) -> WorkspaceKind {
    let label = label.trim().to_ascii_lowercase();
    if label.starts_with("project:") {
        WorkspaceKind::Project
    } else if label.starts_with("dir:") {
        WorkspaceKind::Dir
    } else {
        WorkspaceKind::Unknown
    }
}

const AGENT_SPINNER: &[&str] = &["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];

// Mirrors Herdr's workspace `state_dot` and agent `agent_icon` mappings.
pub(crate) fn status_icon_at(source: &Source, status: &str, tick: u32) -> &'static str {
    let workspace = *source == Source::Workspace;
    let status = status.to_lowercase();
    if status.contains("block")
        || status.contains("error")
        || status.contains("fail")
        || status.contains("attention")
        || status.contains("request")
        || status.contains("wait")
    {
        if workspace {
            "●"
        } else {
            "◉"
        }
    } else if status.contains("work") || status.contains("run") {
        if workspace {
            "●"
        } else {
            AGENT_SPINNER[tick as usize % AGENT_SPINNER.len()]
        }
    } else if status.contains("done") || status.contains("complete") {
        "●"
    } else if status.contains("idle") {
        if workspace {
            "○"
        } else {
            "✓"
        }
    } else if workspace {
        "·"
    } else {
        "○"
    }
}

pub(crate) fn collect_zoxide() -> Vec<Entry> {
    let Ok(out) = Command::new("zoxide").args(["query", "-l"]).output() else {
        return vec![];
    };
    if !out.status.success() {
        return vec![];
    }
    String::from_utf8_lossy(&out.stdout)
        .lines()
        .filter(|l| !l.trim().is_empty())
        .map(|line| {
            let path = PathBuf::from(line);
            Entry::new(
                EntryIdentity::provider("zoxide", line, "open-directory").unwrap(),
                crate::model::EntryData {
                    source: Source::Zoxide,
                    title: basename(&path),
                    subtitle: line.into(),
                    path: Some(OwnedPath::process(path)),
                    workspace_id: None,
                    workspace_label: None,
                    agent: None,
                    project: None,
                    action: EntryAction::FocusOrCreateDir,
                    source_label: None,
                    search_terms: vec![],
                    provenance: [ProviderId::new("zoxide").unwrap()].into(),
                },
            )
        })
        .collect()
}

const PROJECT_MARKERS: [&str; 3] = [".git", "package.json", "Cargo.toml"];

pub(crate) fn collect_roots(config: &Config) -> Vec<Entry> {
    let mut out = Vec::new();
    let mut visited = HashMap::new();
    for root in &config.roots {
        walk_dirs(
            &expand_path(&root.path),
            root.max_depth,
            &root.exclude,
            root.follow_symlinks,
            &mut visited,
            &mut out,
        );
    }
    out
}

/// `visited` maps a directory to the most depth any walk has had left to spend
/// there, so overlapping roots re-walk rather than truncate each other.
fn walk_dirs(
    path: &Path,
    depth: usize,
    excludes: &[String],
    follow_symlinks: bool,
    visited: &mut HashMap<PathBuf, usize>,
    out: &mut Vec<Entry>,
) {
    if depth == 0 {
        return;
    }
    // Only symlink traversal can reach one directory by two routes, so the
    // canonicalize syscall is worth paying only then.
    let key = if follow_symlinks {
        fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf())
    } else {
        path.to_path_buf()
    };
    if visited.get(&key).is_some_and(|walked| *walked >= depth) {
        return;
    }
    visited.insert(key, depth);

    // One `read_dir` answers both "is this a project?" and "what do I recurse
    // into?"; probing each marker with `exists` costs a syscall apiece.
    let Ok(read) = fs::read_dir(path) else {
        return;
    };
    let mut children = Vec::new();
    let mut is_project = false;
    for entry in read.flatten() {
        let name = entry.file_name();
        let Some(name) = name.to_str() else {
            continue;
        };
        is_project |= PROJECT_MARKERS.contains(&name);
        if name.starts_with('.') || excludes.iter().any(|excluded| excluded == name) {
            continue;
        }
        let Ok(file_type) = entry.file_type() else {
            continue;
        };
        if file_type.is_dir() || (follow_symlinks && file_type.is_symlink()) {
            children.push(entry.path());
        }
    }

    if is_project {
        out.push(Entry::new(
            EntryIdentity::provider("roots", &path.display().to_string(), "open-directory")
                .unwrap(),
            crate::model::EntryData {
                source: Source::Root,
                title: basename(path),
                subtitle: path.display().to_string(),
                path: Some(OwnedPath::process(path.to_path_buf())),
                workspace_id: None,
                workspace_label: None,
                agent: None,
                project: None,
                action: EntryAction::FocusOrCreateDir,
                source_label: None,
                search_terms: vec![],
                provenance: [ProviderId::new("roots").unwrap()].into(),
            },
        ));
    }
    for child in children {
        walk_dirs(&child, depth - 1, excludes, follow_symlinks, visited, out);
    }
}

#[cfg(test)]
pub(crate) fn test_inventory(context: &NavigationContext) -> Inventory {
    use crate::navigation::inventory::InventoryRead;
    let workspaces: Value =
        serde_json::from_str(crate::navigation_test_support::WORKSPACE_LIST).unwrap();
    let agents: Value = serde_json::from_str(crate::navigation_test_support::AGENT_LIST).unwrap();
    // Synthetic tab and pane reads complement the inherited agent/workspace values.
    let tabs = serde_json::json!([{"tab_id":"w43:t1","workspace_id":"w43","label":"1"}]);
    Inventory {
        workspaces: InventoryRead::new(
            context.scope(),
            Some(Ok(serde_json::from_value(
                workspaces["result"]["workspaces"].clone(),
            )
            .unwrap())),
        ),
        tabs: InventoryRead::new(
            context.scope(),
            Some(Ok(serde_json::from_value(tabs).unwrap())),
        ),
        panes: InventoryRead::new(
            context.scope(),
            Some(Ok(serde_json::from_value(
                agents["result"]["agents"].clone(),
            )
            .unwrap())),
        ),
        agents: InventoryRead::new(
            context.scope(),
            Some(Ok(serde_json::from_value(
                agents["result"]["agents"].clone(),
            )
            .unwrap())),
        ),
    }
}
#[cfg(test)]
pub(crate) fn test_inventory_entries(context: &NavigationContext) -> Vec<Entry> {
    join_inventory(
        &test_inventory(context),
        context,
        &mut PaneOrdinals::default(),
        &[],
    )
    .panes
}

#[cfg(test)]
mod tests {
    use super::*;

    // Inherited 0.7.3 samples, not a claim about current Herdr framing.
    #[test]
    fn parses_herdr_workspace_and_agent_list_json() {
        let ws_json = serde_json::from_str(crate::navigation_test_support::WORKSPACE_LIST).unwrap();
        let cwd_by_ws =
            HashMap::from([("w41".into(), "/tmp".into()), ("w43".into(), "/tmp".into())]);
        let context = NavigationContext::new();
        let (entries, _) = workspaces_from_json(&ws_json, &cwd_by_ws, &context);
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].subtitle, "agent:unknown · w41 tabs:1 panes:1");
        assert!(entries[0].search_terms.contains(&"unknown".to_string()));
        assert_eq!(entries[1].subtitle, "agent:working · w43 tabs:1 panes:1");
        assert!(entries[1].search_terms.contains(&"working".to_string()));
        assert!(entries[1].search_terms.contains(&"focused".to_string()));

        let agents = test_inventory_entries(&context);
        assert_eq!(agents.len(), 1);
        let target = agents[0].target().unwrap();
        assert_eq!(target.identity().id(), "w43:p1");
        assert_eq!(target.workspace().unwrap().id(), "w43");
        assert_eq!(target.tab().unwrap().id(), "w43:t1");
        assert_eq!(target.identity().scope(), context.scope());
        assert!(matches!(agents[0].action, EntryAction::FocusPane));
        assert_eq!(agents[0].pane.as_ref().unwrap().terminal_id, "term_1");
        assert!(agents[0].search_terms.contains(&"58f4-session".to_string()));
        assert_eq!(agents[0].agent.as_ref().unwrap().status, "working");
    }

    #[test]
    fn live_metadata_changes_do_not_change_entry_equality_hash_or_selection_key() {
        let context = NavigationContext::new();
        let json: Value =
            serde_json::from_str(crate::navigation_test_support::WORKSPACE_LIST).unwrap();
        let (entries, _) = workspaces_from_json(&json, &HashMap::new(), &context);
        let entry = entries[0].clone();
        assert!(entry.path.is_none());
        let mut changed = entry.clone();
        changed.title = "renamed".into();
        changed.subtitle = "agent:blocked tabs:7 panes:9".into();
        changed.workspace_label = Some("new label".into());

        changed.path = OwnedPath::reported(context.scope(), "$HOME/other");
        assert_eq!(entry, changed);
        assert_eq!(entry.key(), changed.key());
        assert_eq!(std::collections::HashSet::from([entry, changed]).len(), 1);
        let (refreshed, _) = workspaces_from_json(&json, &HashMap::new(), &context);
        assert_eq!(entries[0].key(), refreshed[0].key());
        let (different_invocation, _) =
            workspaces_from_json(&json, &HashMap::new(), &NavigationContext::new());
        assert_ne!(entries[0].key(), different_invocation[0].key());
    }

    #[test]
    fn missing_cwd_has_no_home_substitution_or_reuse_index() {
        let context = NavigationContext::new();
        let json = serde_json::json!({"result":{"workspaces":[{"workspace_id":"w1"}, {"workspace_id":""}]}});
        let (entries, index) = workspaces_from_json(&json, &HashMap::new(), &context);
        assert_eq!(entries.len(), 1);
        assert!(entries[0].path.is_none());
        assert!(index.is_empty());
        assert_eq!(entries[0].path_text(), "");
    }

    #[test]
    fn agents_use_pane_identity_and_never_join_another_scope_by_bare_workspace_id() {
        let context = NavigationContext::new();
        let other = NavigationContext::new();
        let mut inventory = test_inventory(&context);
        inventory.workspaces.scope = other.scope().clone();
        let joined = join_inventory(&inventory, &context, &mut PaneOrdinals::default(), &[]);
        assert!(joined.panes.is_empty());
        assert!(!joined.unavailable.is_empty());
    }

    #[test]
    fn joins_six_workspace_fixture_into_nine_qualified_panes_without_machine_or_agent_duplicates() {
        use crate::navigation::inventory::{AgentStatus, InventoryRead};
        let fixture: Value =
            serde_json::from_str(crate::navigation_test_support::INVENTORY).unwrap();
        let mut identities = std::collections::HashSet::new();
        let mut workspace_count = 0;
        let mut tab_count = 0;
        let mut linked = 0;
        let mut invalid_links = 0;
        for machine in fixture["input"]["machines"].as_array().unwrap() {
            let owner = &machine["owner"];
            let context = NavigationContext::reported(
                crate::model::ServerScope::reported(
                    owner[0].as_str().unwrap(),
                    owner[1].as_str().unwrap(),
                    owner[2].as_str().unwrap(),
                )
                .unwrap(),
            );
            let mut ws_rows = Vec::new();
            let mut tab_rows = Vec::new();
            let mut pane_rows = Vec::new();
            for ws in machine["workspaces"].as_array().unwrap() {
                let mut pane_count = 0;
                for tab in ws["tabs"].as_array().unwrap() {
                    tab_rows.push(serde_json::from_value(serde_json::json!({"tab_id":tab["id"], "workspace_id":ws["id"], "label":tab["label"]})).unwrap());
                    for pane in tab["panes"].as_array().unwrap() {
                        pane_count += 1;
                        pane_rows.push(serde_json::from_value::<crate::navigation::inventory::PaneRecord>(serde_json::json!({
                            "pane_id":pane["id"], "workspace_id":ws["id"], "tab_id":tab["id"], "terminal_id":pane["terminal_id"],
                            "revision":pane["revision"], "label":pane["label"], "cwd":pane["cwd"]
                        })).unwrap());
                    }
                }
                ws_rows.push(serde_json::from_value(serde_json::json!({"workspace_id":ws["id"], "label":ws["label"], "focused":false, "tab_count":ws["tabs"].as_array().unwrap().len(), "pane_count":pane_count, "agent_status":"unknown"})).unwrap());
            }
            let mut agents = Vec::new();
            for update in fixture["input"]["agent_updates"]
                .as_array()
                .unwrap()
                .iter()
                .filter(|update| update["owner"] == *owner)
            {
                let pane_id = update["pane_id"].as_str().unwrap_or("");
                let pane = pane_rows.iter().find(|pane| pane.pane_id == pane_id);
                // Synthetic "waiting" is not a protocol-22 status. Do not pretend it is raw evidence.
                let status = serde_json::from_value::<AgentStatus>(update["status"].clone())
                    .unwrap_or(AgentStatus::Unknown);
                agents.push(crate::navigation::inventory::AgentRecord {
                    pane_id: pane_id.into(),
                    workspace_id: pane.map(|p| p.workspace_id.clone()).unwrap_or_default(),
                    tab_id: pane.map(|p| p.tab_id.clone()).unwrap_or_default(),
                    terminal_id: pane.map(|p| p.terminal_id.clone()).unwrap_or_default(),
                    agent: update["agent"].as_str().map(str::to_owned),
                    name: None,
                    display_agent: None,
                    revision: update["revision"].as_u64().unwrap(),
                    agent_status: status,
                    agent_session: update["session"].as_str().map(|value| {
                        crate::navigation::inventory::AgentSession {
                            agent: update["agent"].as_str().unwrap().into(),
                            source: "synthetic".into(),
                            kind: crate::navigation::inventory::AgentSessionKind::Id,
                            value: value.into(),
                        }
                    }),
                });
            }
            let inventory = Inventory {
                workspaces: InventoryRead::new(context.scope(), Some(Ok(ws_rows))),
                tabs: InventoryRead::new(context.scope(), Some(Ok(tab_rows))),
                panes: InventoryRead::new(context.scope(), Some(Ok(pane_rows))),
                agents: InventoryRead::new(context.scope(), Some(Ok(agents))),
            };
            tab_count += inventory.tabs.count().unwrap();
            let aliases = [crate::config::AgentAliasConfig {
                alias: "reviewer".into(),
                agent: Some("claude".into()),
                workspace: None,
                path: None,
            }];
            let joined =
                join_inventory(&inventory, &context, &mut PaneOrdinals::default(), &aliases);
            workspace_count += joined.workspaces.len();
            invalid_links += joined.unavailable.len();
            for entry in joined.panes {
                assert_eq!(
                    entry.destination_kind(),
                    crate::model::DestinationKind::Pane
                );
                assert!(identities.insert(entry.key().clone()));
                assert_ne!(
                    entry.target().unwrap().identity().id(),
                    entry.pane.as_ref().unwrap().terminal_id
                );
                assert_eq!(entry.pane.as_ref().unwrap().tab_naming, TabNaming::Unknown);
                if entry.workspace_label.as_deref() == Some("work") {
                    assert!(entry.path.is_none());
                }
                if let Some(agent) = &entry.agent {
                    linked += 1;
                    assert_eq!(entry.provenance.len(), 2);
                    if agent.agent.as_deref() == Some("claude") {
                        assert_eq!(entry.title, "Review key handling");
                        assert_eq!(agent.revision, 2);
                        assert_eq!(agent.status, "unknown");
                        assert_eq!(agent.aliases, ["reviewer"]);
                        assert!(entry
                            .search_fields()
                            .contains(&"synthetic-session-a".into()));
                        assert_eq!(entry.pane.as_ref().unwrap().tab_label, "1");
                    }
                }
            }
        }
        assert_eq!(
            (
                workspace_count,
                tab_count,
                identities.len(),
                linked,
                invalid_links
            ),
            (6, 7, 9, 2, 2)
        );
    }

    #[test]
    fn title_precedence_and_ordinals_do_not_depend_on_ids_focus_or_refresh_order() {
        let context = NavigationContext::new();
        let mut inventory = test_inventory(&context);
        let mut ordinals = PaneOrdinals::default();
        let ReadState::Ready(panes) = &mut inventory.panes.state else {
            panic!()
        };
        panes[0].label = Some(" Manual ".into());
        panes[0].title = Some("reported title".into());
        assert_eq!(
            join_inventory(&inventory, &context, &mut ordinals, &[]).panes[0].title,
            "Manual"
        );
        let ReadState::Ready(panes) = &mut inventory.panes.state else {
            panic!()
        };
        panes[0].label = Some(" \n ".into());
        assert_eq!(
            join_inventory(&inventory, &context, &mut ordinals, &[]).panes[0].title,
            "claude"
        );
        inventory.agents.state = ReadState::Ready(Vec::new());
        assert_eq!(
            join_inventory(&inventory, &context, &mut ordinals, &[]).panes[0].title,
            "reported title"
        );
        let ReadState::Ready(panes) = &mut inventory.panes.state else {
            panic!()
        };
        panes[0].title = Some(panes[0].terminal_id.clone());
        panes[0].terminal_title = Some("\u{1b}[31munsafe".into());
        let first = panes[0].clone();
        let mut other = first.clone();
        other.pane_id = "opaque-not-a-number".into();
        panes.insert(0, other);
        let joined = join_inventory(&inventory, &context, &mut ordinals, &[]);
        assert_eq!(joined.panes[1].title, "Pane 1");
        assert_eq!(joined.panes[0].title, "Pane 2");
        let ReadState::Ready(panes) = &mut inventory.panes.state else {
            panic!()
        };
        panes.reverse();
        assert_eq!(
            join_inventory(&inventory, &context, &mut ordinals, &[]).panes[0].title,
            "Pane 1"
        );
    }

    #[test]
    fn agent_updates_resolve_revision_and_ties_and_empty_removes_metadata() {
        let context = NavigationContext::new();
        let mut inventory = test_inventory(&context);
        let ReadState::Ready(agents) = &mut inventory.agents.state else {
            panic!()
        };
        let mut newer = agents[0].clone();
        newer.revision = 2;
        newer.name = Some("newest".into());
        agents.push(newer.clone());
        newer.name = Some("tie".into());
        agents.push(newer);
        let joined = join_inventory(&inventory, &context, &mut PaneOrdinals::default(), &[]);
        assert_eq!(joined.panes.len(), 1);
        assert_eq!(joined.panes[0].title, "tie");
        let ReadState::Ready(agents) = &mut inventory.agents.state else {
            panic!()
        };
        agents.reverse();
        assert_eq!(
            join_inventory(&inventory, &context, &mut PaneOrdinals::default(), &[]).panes[0].title,
            "tie"
        );
        let ReadState::Ready(agents) = &mut inventory.agents.state else {
            panic!()
        };
        agents.truncate(1);
        agents[0].agent = None;
        agents[0].name = None;
        agents[0].display_agent = None;
        let unnamed = join_inventory(&inventory, &context, &mut PaneOrdinals::default(), &[]);
        assert!(unnamed.panes[0].agent.as_ref().unwrap().agent.is_none());
        assert!(unnamed.panes[0].title.starts_with("Pane "));
        assert_eq!(
            unnamed.visible_panes(false).len(),
            1,
            "a missing name does not remove a valid pane link"
        );
        inventory.agents.state = ReadState::Ready(Vec::new());
        let empty = join_inventory(&inventory, &context, &mut PaneOrdinals::default(), &[]);
        assert!(empty.panes[0].agent.is_none());
        assert_eq!(
            empty.panes[0].pane.as_ref().unwrap().agent_freshness,
            AgentFreshness::Observed
        );
        inventory.agents.state = ReadState::Unavailable(crate::herdr::TransportError::new(
            crate::herdr::Failure::Unsupported,
            crate::herdr::MutationOutcome::NotSent,
        ));
        let failed = join_inventory(&inventory, &context, &mut PaneOrdinals::default(), &[]);
        assert_eq!(failed.panes.len(), 1);
        assert_eq!(
            failed.panes[0].pane.as_ref().unwrap().agent_freshness,
            AgentFreshness::Unknown
        );
        assert_eq!(inventory.agents.count(), None);
    }

    #[test]
    fn missing_or_conflicting_ancestry_and_duplicate_inventory_never_supply_focus_targets() {
        let context = NavigationContext::new();
        for case in 0..6 {
            let mut inventory = test_inventory(&context);
            match case {
                0 => inventory.tabs.state = ReadState::Ready(Vec::new()),
                1 => {
                    let ReadState::Ready(tabs) = &mut inventory.tabs.state else {
                        panic!()
                    };
                    tabs[0].workspace_id = "w41".into();
                }
                2 => {
                    let ReadState::Ready(panes) = &mut inventory.panes.state else {
                        panic!()
                    };
                    panes[0].workspace_id = "w41".into();
                }
                3 => {
                    let ReadState::Ready(panes) = &mut inventory.panes.state else {
                        panic!()
                    };
                    panes.push(panes[0].clone());
                }
                4 => {
                    let ReadState::Ready(tabs) = &mut inventory.tabs.state else {
                        panic!()
                    };
                    tabs.push(tabs[0].clone());
                }
                _ => {
                    let ReadState::Ready(ws) = &mut inventory.workspaces.state else {
                        panic!()
                    };
                    ws.push(ws[1].clone());
                }
            }
            let joined = join_inventory(&inventory, &context, &mut PaneOrdinals::default(), &[]);
            assert!(joined.panes.is_empty(), "case {case}");
            assert!(!joined.unavailable.is_empty());
        }
        let mut inventory = test_inventory(&context);
        let ReadState::Ready(agents) = &mut inventory.agents.state else {
            panic!()
        };
        agents[0].workspace_id = "w41".into();
        let joined = join_inventory(&inventory, &context, &mut PaneOrdinals::default(), &[]);
        assert_eq!(joined.panes.len(), 1);
        assert!(joined.panes[0].agent.is_none());
        assert!(!joined.unavailable.is_empty());
    }

    #[test]
    fn root_scan_excludes_generated_trees() {
        use std::time::{SystemTime, UNIX_EPOCH};

        let suffix = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!("herdr-root-scan-{suffix}"));
        let project = root.join("project");
        let dependency = root.join("node_modules/dependency");
        fs::create_dir_all(&project).unwrap();
        fs::create_dir_all(&dependency).unwrap();
        fs::write(project.join("Cargo.toml"), "[package]").unwrap();
        fs::write(dependency.join("package.json"), "{}").unwrap();

        let mut config = Config::default();
        config.roots = vec![crate::config::RootConfig {
            path: root.display().to_string(),
            max_depth: 4,
            exclude: vec!["node_modules".into()],
            follow_symlinks: false,
        }];
        let entries = collect_roots(&config);
        fs::remove_dir_all(root).unwrap();

        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].title, "project");
    }

    #[test]
    fn a_shallow_root_does_not_truncate_a_deeper_overlapping_root() {
        use std::time::{SystemTime, UNIX_EPOCH};

        let suffix = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!("herdr-root-depth-{suffix}"));
        let nested = root.join("outer/inner");
        fs::create_dir_all(&nested).unwrap();
        fs::write(nested.join("Cargo.toml"), "[package]").unwrap();

        let shallow_first = |max_depth: usize| crate::config::RootConfig {
            path: root.join("outer").display().to_string(),
            max_depth,
            exclude: vec![],
            follow_symlinks: false,
        };
        let mut config = Config::default();
        // The shallow root reaches `outer` first with no depth left for `inner`.
        config.roots = vec![
            shallow_first(1),
            crate::config::RootConfig {
                path: root.display().to_string(),
                max_depth: 4,
                exclude: vec![],
                follow_symlinks: false,
            },
        ];
        let entries = collect_roots(&config);
        fs::remove_dir_all(root).unwrap();

        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].title, "inner");
    }

    #[test]
    fn status_icons_match_herdr() {
        assert_eq!(status_icon_at(&Source::Workspace, "blocked", 0), "●");
        assert_eq!(status_icon_at(&Source::Workspace, "working", 0), "●");
        assert_eq!(status_icon_at(&Source::Workspace, "done", 0), "●");
        assert_eq!(status_icon_at(&Source::Workspace, "idle", 0), "○");
        assert_eq!(status_icon_at(&Source::Workspace, "unknown", 0), "·");

        assert_eq!(status_icon_at(&Source::Agent, "blocked", 0), "◉");
        assert_eq!(status_icon_at(&Source::Agent, "working", 0), "⠋");
        assert_eq!(status_icon_at(&Source::Agent, "working", 1), "⠙");
        assert_eq!(status_icon_at(&Source::Agent, "done", 0), "●");
        assert_eq!(status_icon_at(&Source::Agent, "idle", 0), "✓");
        assert_eq!(status_icon_at(&Source::Agent, "unknown", 0), "○");
    }
}
