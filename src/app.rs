use std::{
    collections::{HashMap, HashSet},
    thread,
    time::{Duration, Instant},
};
#[cfg(test)]
use {
    crate::paths::home,
    std::{env, fs, path::Path},
};

use crate::{
    actions::{self, Prepared, ResolvedAction, Slot},
    config::Config,
    herdr::{Failure, MutationOutcome, TransportError},
    history::Store,
    integrations::{command, herdr_plus, sessions},
    matcher::{match_quality, Query, SearchMatcher},
    model::{
        AgentState, Entry, EntryAction, EntryIdentity, LiveIdentity, Source, WorkspaceKind,
        WorkspaceRef, WorkspaceReuseKey,
    },
    navigation::{
        inventory::{Inventory, InventoryRead},
        NavigationContext,
    },
    paths::herdr_plus_quick_actions_dir,
    sources::{collect_roots, collect_zoxide, join_inventory, JoinedInventory, PaneOrdinals},
    theme::Theme,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum AgentSort {
    /// Surface agents that need attention first.
    Priority,
    /// Keep Herdr's own agent panel ordering.
    Spaces,
}

impl AgentSort {
    /// Resolved once per run: the fallback parses Herdr's config from disk.
    fn resolve(configured: &str) -> Self {
        match configured.trim().to_ascii_lowercase().as_str() {
            "priority" => Self::Priority,
            "spaces" => Self::Spaces,
            _ => Self::from_herdr_config(crate::paths::herdr_config().as_ref()),
        }
    }

    /// `ui.agent_panel_sort`, falling back to a top-level `agent_panel_sort`.
    fn from_herdr_config(herdr: Option<&toml::Value>) -> Self {
        let configured = herdr.and_then(|value| {
            value
                .get("ui")
                .and_then(toml::Value::as_table)
                .and_then(|ui| ui.get("agent_panel_sort"))
                .or_else(|| value.get("agent_panel_sort"))
                .and_then(toml::Value::as_str)
                .map(str::to_ascii_lowercase)
        });
        match configured.as_deref() {
            Some("priority") => Self::Priority,
            _ => Self::Spaces,
        }
    }
}

/// Raw output of one concurrent [`App::refresh`] collection pass.
struct Collected {
    inventory: Inventory,
    projects: Vec<Entry>,
    zoxide: Vec<Entry>,
    roots: Vec<Entry>,
    remotes: Vec<Entry>,
    sessions: Vec<Entry>,
    integrations: Vec<Entry>,
}

/// Disabled or panicking collectors degrade to no entries; a panic records
/// `name` in `panicked` so the caller can report it.
fn joined<T: Default>(
    handle: Option<thread::ScopedJoinHandle<'_, T>>,
    name: &'static str,
    panicked: &mut Vec<&'static str>,
) -> T {
    handle.map_or_else(T::default, |handle| {
        handle.join().unwrap_or_else(|_| {
            panicked.push(name);
            T::default()
        })
    })
}

/// Sort inputs resolved once per row: deriving them inside the comparator
/// instead costs `O(n log n)` mark and rank lookups per keystroke.
struct Candidate {
    index: usize,
    score: i64,
    match_quality: u8,
    field_rank: usize,
    path_len: usize,
    source_rank: usize,
    previous_pinned: bool,
    user_pinned: bool,
}
#[derive(Clone, Copy)]
struct SearchMatch {
    score: i64,
    quality: u8,
    field_rank: usize,
}

fn best_search_match(
    matcher: &mut SearchMatcher,
    query: &str,
    fields: &[String],
) -> Option<SearchMatch> {
    let mut best: Option<SearchMatch> = None;
    for (field_rank, field) in fields.iter().enumerate() {
        let Some(score) = matcher.score(field) else {
            continue;
        };
        let quality = match_quality(field, query);
        let candidate = SearchMatch {
            score,
            quality,
            field_rank,
        };
        if best.as_ref().is_none_or(|current| {
            candidate.quality > current.quality
                || (candidate.quality == current.quality
                    && (candidate.field_rank < current.field_rank
                        || (candidate.field_rank == current.field_rank
                            && candidate.score > current.score)))
        }) {
            best = Some(candidate);
        }
    }
    best
}

#[cfg(test)]
struct TempHistory(std::path::PathBuf);

#[cfg(test)]
impl TempHistory {
    fn new() -> Self {
        use std::sync::atomic::{AtomicU64, Ordering};
        static NEXT: AtomicU64 = AtomicU64::new(0);

        let base = std::path::absolute(env::temp_dir()).expect("absolute test directory");
        for _ in 0..128 {
            let path = base.join(format!(
                "navigator-test-history-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            let mut builder = fs::DirBuilder::new();
            #[cfg(unix)]
            {
                use std::os::unix::fs::DirBuilderExt;
                builder.mode(0o700);
            }
            match builder.create(&path) {
                Ok(()) => return Self(path),
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(error) => panic!("create test history directory: {error}"),
            }
        }
        panic!("test history directory allocation exhausted");
    }
}

#[cfg(test)]
impl Drop for TempHistory {
    fn drop(&mut self) {
        // Only a successful create gives this guard ownership. Never remove a collision.
        let _ = fs::remove_dir_all(&self.0);
    }
}

pub(crate) struct App {
    pub(crate) navigation: NavigationContext,
    history: Store,
    prepared_actions: HashMap<(EntryIdentity, Slot), Prepared>,
    displayed_actions: Vec<DisplayedAction>,
    action_revision: u64,
    close_token: Option<(EntryIdentity, LiveIdentity, String, ResolvedAction)>,
    pub(crate) config: Config,
    pub(crate) theme: Theme,
    pub(crate) entries: Vec<Entry>,
    search_fields: Vec<Vec<String>>,
    root_cache: Option<(Instant, Vec<Entry>)>,
    pub(crate) inventory: Option<Inventory>,
    pub(crate) joined_inventory: JoinedInventory,
    pane_ordinals: PaneOrdinals,
    pub(crate) filtered: Vec<usize>,
    pub(crate) filtered_scores: Vec<i64>,
    pub(crate) selected: usize,
    pub(crate) query: String,
    pub(crate) source_filter: Option<Source>,
    pub(crate) path_to_workspaces: HashMap<WorkspaceReuseKey, Vec<WorkspaceRef>>,
    pub(crate) previous_workspace_id: Option<EntryIdentity>,
    pub(crate) pinned_entries: HashSet<EntryIdentity>,
    pub(crate) spinner_tick: u32,
    pub(crate) update_available: Option<String>,
    pub(crate) agent_sort: AgentSort,
    pub(crate) animate_spinner: bool,
    #[cfg(test)]
    test_history: Option<TempHistory>,
}

#[derive(Clone, Debug)]
pub(crate) struct DisplayedAction {
    pub(crate) revision: u64,
    pub(crate) action: ResolvedAction,
}

impl App {
    pub(crate) fn new(config: Config, theme: Theme) -> Self {
        Self::with_history(config, theme, crate::paths::plugin_config_dir())
    }

    #[cfg(test)]
    pub(crate) fn for_test(config: Config, theme: Theme) -> Self {
        let directory = TempHistory::new();
        let mut app = Self::with_history(config, theme, directory.0.clone());
        app.test_history = Some(directory);
        assert!(
            app.history.diagnostic().is_none(),
            "test history must be writable: {:?}",
            app.history.diagnostic()
        );
        app
    }

    pub(crate) fn with_history(
        config: Config,
        theme: Theme,
        directory: std::path::PathBuf,
    ) -> Self {
        let history = Store::open(directory);
        if let Some(diagnostic) = history.diagnostic() {
            eprintln!("Navigator preferences unavailable: {diagnostic}");
        }
        let agent_sort = AgentSort::resolve(&config.picker.agent_sort);
        Self {
            navigation: NavigationContext::new(),
            history,
            prepared_actions: HashMap::new(),
            displayed_actions: Vec::new(),
            action_revision: 0,
            close_token: None,
            agent_sort,
            config,
            theme,
            entries: vec![],
            search_fields: vec![],
            root_cache: None,
            inventory: None,
            joined_inventory: JoinedInventory::default(),
            pane_ordinals: PaneOrdinals::default(),
            filtered: vec![],
            filtered_scores: vec![],
            selected: 0,
            query: String::new(),
            source_filter: None,
            path_to_workspaces: HashMap::new(),
            previous_workspace_id: None,
            pinned_entries: HashSet::new(),
            spinner_tick: 0,
            update_available: None,
            animate_spinner: false,
            #[cfg(test)]
            test_history: None,
        }
    }

    pub(crate) fn refresh(&mut self) {
        let selected_key = self.selected_entry().map(|entry| entry.key().clone());
        let cache_ttl = Duration::from_secs(self.config.picker.root_cache_seconds);
        let cached_roots = self
            .root_cache
            .as_ref()
            .filter(|(created, _)| !cache_ttl.is_zero() && created.elapsed() <= cache_ttl)
            .map(|(_, entries)| entries.clone());

        // Collectors each shell out or walk the filesystem: overlap them so
        // startup costs the slowest one, not their sum.
        let config = &self.config;
        let sources = &config.sources;
        let request_scope = self.navigation.scope().clone();
        let navigation = &self.navigation;
        let mut panicked = Vec::new();
        let mut collected = thread::scope(|scope| {
            let inventory = scope.spawn(|| {
                let server = navigation.server();
                let workspaces = InventoryRead::new(&request_scope, Some(server.workspaces()));
                let panes = InventoryRead::new(&request_scope, Some(server.panes()));
                let tabs = InventoryRead::new(&request_scope, Some(server.tabs()));
                let agents =
                    InventoryRead::new(&request_scope, sources.agents.then(|| server.agents()));
                Inventory {
                    workspaces,
                    panes,
                    tabs,
                    agents,
                }
            });
            let projects = sources
                .herdr_plus_projects
                .then(|| scope.spawn(herdr_plus::collect_projects));
            let zoxide = sources.zoxide.then(|| scope.spawn(collect_zoxide));
            let roots = (sources.roots && cached_roots.is_none())
                .then(|| scope.spawn(|| collect_roots(config)));
            let remotes = sources
                .servers
                .then(|| scope.spawn(|| sessions::collect_remotes(config)));
            let session_entries = sources
                .sessions
                .then(|| scope.spawn(|| sessions::collect_sessions(config)));
            let integrations = scope.spawn(|| command::collect(&config.integrations));
            Collected {
                inventory: inventory.join().unwrap_or_else(|_| {
                    fn failed<T>(scope: &crate::model::ServerScope) -> InventoryRead<T> {
                        let error = TransportError::new(Failure::Io, MutationOutcome::NotSent);
                        InventoryRead::new(scope, Some(Err(error)))
                    }
                    Inventory {
                        workspaces: failed(&request_scope),
                        panes: failed(&request_scope),
                        tabs: failed(&request_scope),
                        agents: failed(&request_scope),
                    }
                }),
                projects: joined(projects, "Herdr Plus projects", &mut panicked),
                zoxide: joined(zoxide, "zoxide", &mut panicked),
                roots: joined(roots, "roots", &mut panicked),
                remotes: joined(remotes, "servers", &mut panicked),
                sessions: joined(session_entries, "sessions", &mut panicked),
                integrations: joined(Some(integrations), "integrations", &mut panicked),
            }
        });
        if !panicked.is_empty() {
            self.navigation.server().notify(
                &format!("Failed to collect {}", panicked.join(", ")),
                false,
                &self.config.notifications,
            );
        }

        // Apply failures before conversion. A response collected in the old
        // generation must never acquire the new generation's identities.
        if let Some(error) = collected.inventory.uncertain_error() {
            self.navigation.observe_failure(error);
        }
        collected
            .inventory
            .discard_old_scope(self.navigation.scope());
        self.joined_inventory = join_inventory(
            &collected.inventory,
            &self.navigation,
            &mut self.pane_ordinals,
            &self.config.agent_aliases,
        );
        self.inventory = Some(collected.inventory);
        self.path_to_workspaces = self.joined_inventory.reuse.clone();
        let root_entries = match cached_roots {
            Some(entries) => entries,
            None if self.config.sources.roots => {
                self.root_cache = Some((Instant::now(), collected.roots.clone()));
                collected.roots
            }
            None => Vec::new(),
        };

        // Conflicting provider identities remain unavailable instead of first-wins.
        let mut entries = Vec::new();
        let mut seen = HashSet::new();
        if self.config.sources.open_workspaces {
            push_unique(
                &mut entries,
                &mut seen,
                self.joined_inventory.workspaces.clone(),
            );
        }
        push_unique(&mut entries, &mut seen, collected.projects);
        push_unique(&mut entries, &mut seen, collected.zoxide);
        push_unique(&mut entries, &mut seen, root_entries);
        push_unique(&mut entries, &mut seen, collected.remotes);
        push_unique(&mut entries, &mut seen, collected.sessions);
        push_unique(
            &mut entries,
            &mut seen,
            self.joined_inventory
                .visible_panes(self.config.sources.panes),
        );
        if self.config.sources.herdr_plus_quick_actions && herdr_plus_quick_actions_dir().is_dir() {
            entries.push(herdr_plus::quick_actions_entry());
        }
        push_unique(&mut entries, &mut seen, collected.integrations);

        self.animate_spinner = crate::tui::has_working_entry(&entries);
        self.entries = entries;
        self.prepare_actions();
        self.search_fields = self.entries.iter().map(Entry::search_fields).collect();
        self.project_pins();
        // Ordinary focus acknowledgements do not identify a client transition.
        self.previous_workspace_id = None;
        self.apply_filter();
        self.restore_selection(selected_key.as_ref());
    }

    fn restore_selection(&mut self, selected_key: Option<&EntryIdentity>) {
        let Some(selected_key) = selected_key else {
            return;
        };
        if let Some(position) = self.filtered.iter().position(|index| {
            self.entries
                .get(*index)
                .is_some_and(|entry| entry.key() == selected_key)
        }) {
            self.selected = position;
        }
    }

    pub(crate) fn apply_filter(&mut self) {
        let query = Query::parse(&self.query);
        let empty_query = query.plain.is_empty();
        let agent_view =
            query.all_agents || (self.source_filter == Some(Source::Agent) && empty_query);
        let use_agent_priority = empty_query
            && (agent_view || self.source_filter.is_none())
            && self.agent_sort == AgentSort::Priority;
        let pin_previous = self.config.jump_back.enabled
            && self.config.jump_back.pin_previous
            && self.query.trim().is_empty()
            && self.source_filter.is_none();
        let source_ranks = self.config.picker.source_ranks();
        let mut matcher =
            (!empty_query).then(|| SearchMatcher::new(&self.config.picker.engine, &query.plain));

        let mut candidates = Vec::new();
        for (index, entry) in self.entries.iter().enumerate() {
            if self
                .source_filter
                .is_some_and(|filter| filter != entry.source)
            {
                continue;
            }
            if !query.filters_match(entry) {
                continue;
            }
            let source_rank = source_ranks[entry.source.index()];
            let bonus = self.config.picker.bonus_for_rank(source_rank)
                + if entry.source == Source::Agent && use_agent_priority {
                    agent_status_bonus(entry)
                } else {
                    0
                };
            let (score, match_quality, field_rank) = match matcher.as_mut() {
                Some(matcher) => {
                    // Tests assign `entries` without cached fields; derive those directly.
                    let fallback;
                    let fields = match self.search_fields.get(index) {
                        Some(fields) => fields.as_slice(),
                        None => {
                            fallback = entry.search_fields();
                            fallback.as_slice()
                        }
                    };
                    match best_search_match(matcher, &query.plain, fields) {
                        Some(matched) => {
                            (matched.score + bonus, matched.quality, matched.field_rank)
                        }
                        None => continue,
                    }
                }
                None => (bonus, 0, usize::MAX),
            };
            candidates.push(Candidate {
                index,
                score,
                match_quality,
                field_rank,
                path_len: entry.path_text().len(),
                source_rank,
                previous_pinned: pin_previous
                    && entry.source == Source::Workspace
                    && Some(entry.key()) == self.previous_workspace_id.as_ref(),
                user_pinned: self.is_pinned(entry),
            });
        }

        candidates.sort_by(|a, b| {
            b.previous_pinned
                .cmp(&a.previous_pinned)
                .then_with(|| b.user_pinned.cmp(&a.user_pinned))
                .then_with(|| b.match_quality.cmp(&a.match_quality))
                .then_with(|| a.field_rank.cmp(&b.field_rank))
                .then_with(|| b.score.cmp(&a.score))
                .then_with(|| {
                    if empty_query {
                        std::cmp::Ordering::Equal
                    } else {
                        a.path_len.cmp(&b.path_len)
                    }
                })
                .then_with(|| a.source_rank.cmp(&b.source_rank))
                .then_with(|| {
                    if agent_view {
                        a.index.cmp(&b.index)
                    } else {
                        self.entries[a.index]
                            .title
                            .cmp(&self.entries[b.index].title)
                    }
                })
        });

        self.filtered_scores = candidates.iter().map(|c| c.score).collect();
        self.filtered = candidates.into_iter().map(|c| c.index).collect();
        self.selected = 0;
    }

    #[cfg(test)]
    pub(crate) fn set_filter(&mut self, source: Option<Source>) {
        self.source_filter = if self.source_filter == source {
            None
        } else {
            source
        };
        self.selected = 0;
    }

    pub(crate) fn cycle_filter(&mut self) {
        self.source_filter = crate::picker::input::next_source_filter(self.source_filter);
        self.selected = 0;
        self.apply_filter();
    }

    pub(crate) fn next(&mut self) {
        if !self.filtered.is_empty() {
            self.selected = (self.selected + 1).min(self.filtered.len() - 1);
        }
    }
    pub(crate) fn prev(&mut self) {
        self.selected = self.selected.saturating_sub(1);
    }
    pub(crate) fn selected_entry(&self) -> Option<&Entry> {
        self.filtered
            .get(self.selected)
            .and_then(|idx| self.entries.get(*idx))
    }

    // History key errors are deliberately not surfaced: a broken history
    // document is already reported once at startup, and the per-entry errors
    // mean the entry has no durable pin identity, so "not pinned" is correct.
    // Marking such an entry reports the error through `toggle_selected_pin`.
    fn project_pins(&mut self) {
        self.pinned_entries.retain(|key| matches!(key, EntryIdentity::Live(target) if target.identity().scope() == self.navigation.scope()));
        for entry in &self.entries {
            if let Ok(Some(key)) = self
                .history
                .key(entry, self.config.picker.directory_template.as_deref())
            {
                if self.history.contains(&key) {
                    self.pinned_entries.insert(entry.key().clone());
                }
            }
        }
    }

    pub(crate) fn is_pinned(&self, entry: &Entry) -> bool {
        if entry
            .target()
            .is_some_and(|target| target.identity().scope() != self.navigation.scope())
        {
            return false;
        }
        self.pinned_entries.contains(entry.key())
    }

    pub(crate) fn toggle_selected_pin(&mut self) -> Result<(), String> {
        let key = self
            .selected_entry()
            .map(|entry| entry.key().clone())
            .ok_or("nothing selected")?;
        let entry = self.selected_entry().ok_or("nothing selected")?;
        if entry
            .target()
            .is_some_and(|target| target.identity().scope() != self.navigation.scope())
        {
            return Err("live mark belongs to an expired observation".into());
        }
        if matches!(key, EntryIdentity::Machine(_)) {
            return Err("machine preference identity is unavailable".into());
        }
        if let Some(provider) = self
            .history
            .key(entry, self.config.picker.directory_template.as_deref())?
        {
            self.history.set_pin(provider, !self.is_pinned(entry))?;
        }
        if !self.pinned_entries.remove(&key) {
            self.pinned_entries.insert(key.clone());
        }
        self.apply_filter();
        // Marking re-sorts the list; keep the cursor on the row the user marked.
        self.restore_selection(Some(&key));
        Ok(())
    }

    pub(crate) fn directory_template_for_selected(&self) -> Option<&str> {
        let template = self
            .config
            .picker
            .directory_template
            .as_deref()
            .map(str::trim)
            .filter(|template| !template.is_empty())?;
        matches!(
            &self.selected_entry()?.action,
            EntryAction::FocusOrCreateDir
        )
        .then_some(template)
    }

    pub(crate) fn prepare_actions(&mut self) {
        self.prepared_actions.clear();
        for entry in &mut self.entries {
            for slot in [Slot::Primary, Slot::Template] {
                if slot == Slot::Template && !matches!(entry.action, EntryAction::FocusOrCreateDir)
                {
                    continue;
                }
                let prepared = Prepared::load(
                    entry,
                    slot,
                    &self.config,
                    &self.navigation,
                    None,
                    &actions::Files,
                );
                prepared.update_project_entry(entry);
                self.prepared_actions
                    .insert((entry.key().clone(), slot), prepared);
            }
        }
    }

    pub(crate) fn resolved_action(&self, entry: &Entry, slot: Slot) -> ResolvedAction {
        let empty = Prepared::default();
        let prepared = self
            .prepared_actions
            .get(&(entry.key().clone(), slot))
            .unwrap_or(&empty);
        actions::resolve(
            entry,
            slot,
            prepared,
            &self.config,
            actions::Observations {
                context: &self.navigation,
                inventory: self.inventory.as_ref(),
                joined: &self.joined_inventory,
                reuse: &self.path_to_workspaces,
            },
        )
    }

    /// N08 read-only input seam. N10/N11 will own the active picker state.
    #[allow(dead_code)]
    pub(crate) fn picker_input(&self) -> crate::picker::PickerInput {
        use crate::navigation::{
            inventory::{ReadState, TabNaming},
            ProcessHostRef,
        };
        use crate::picker::{Destination, Heading, Owner, PickerInput, TabInventory, TaskKind};
        let server = Owner::Server(self.navigation.scope().clone());
        let process = if self.navigation.require_process_server_colocation().is_ok() {
            server.clone()
        } else {
            Owner::Process(ProcessHostRef)
        };
        let mut input = PickerInput {
            headings: vec![Heading {
                owner: server.clone(),
                label: "Server scope: owner unknown".into(),
                local: false,
                machine: None,
            }],
            current: self
                .navigation
                .current_client_selection()
                .cloned()
                .map(EntryIdentity::live),
            pinned: self.pinned_entries.clone(),
            previous: self.previous_workspace_id.clone(),
            ..PickerInput::default()
        };
        if process != server {
            input.headings.push(Heading {
                owner: process.clone(),
                label: "Navigator process host".into(),
                local: false,
                machine: None,
            });
        }
        let sources = &self.config.sources;
        for (supported, kind) in [
            (
                sources.herdr_plus_projects || sources.zoxide || sources.roots,
                TaskKind::ProjectsAndFolders,
            ),
            (sources.servers || sources.sessions, TaskKind::Sessions),
        ] {
            if supported {
                input.groups.push((process.clone(), kind));
            }
        }
        for (supported, owner) in [
            (sources.herdr_plus_quick_actions, &server),
            (self.config.integrations.iter().any(|c| c.enabled), &process),
        ] {
            let group = (owner.clone(), TaskKind::Actions);
            if supported && !input.groups.contains(&group) {
                input.groups.push(group);
            }
        }
        for entry in &self.entries {
            let action = self.resolved_action(entry, Slot::Primary);
            let owner = if entry.target().is_some() {
                server.clone()
            } else if entry.destination_kind() == crate::model::DestinationKind::Action {
                match &action.owner {
                    actions::Owner::Server(scope) => Owner::Server(scope.clone()),
                    actions::Owner::Process(_) => process.clone(),
                }
            } else {
                process.clone()
            };
            input.destinations.push(Destination {
                entry: entry.clone(),
                owner,
                eligible: true,
                action,
            });
        }
        for entry in &self.joined_inventory.workspaces {
            if !input
                .destinations
                .iter()
                .any(|d| d.entry.key() == entry.key())
            {
                input.destinations.push(Destination {
                    entry: entry.clone(),
                    owner: server.clone(),
                    eligible: false,
                    action: self.resolved_action(entry, Slot::Primary),
                });
            }
        }
        for entry in &self.joined_inventory.tabs {
            let eligible = sources.open_workspaces
                || self.entries.iter().any(|pane| {
                    pane.target().is_some_and(|p| {
                        entry
                            .target()
                            .is_some_and(|t| p.tab() == Some(t.identity()))
                    })
                });
            input.destinations.push(Destination {
                entry: entry.clone(),
                owner: server.clone(),
                eligible,
                action: self.resolved_action(entry, Slot::Primary),
            });
            if let Some(target) = entry.target() {
                input
                    .naming
                    .insert(target.identity().clone(), TabNaming::Unknown);
            }
        }
        if let Some(inventory) = &self.inventory {
            let scope = self.navigation.scope();
            let complete = &inventory.tabs.scope == scope
                && matches!(inventory.tabs.state, ReadState::Ready(_))
                && inventory.tabs.count() == Some(self.joined_inventory.tabs.len());
            for ws in inventory.workspaces.rows(scope) {
                if let Some(identity) =
                    LiveIdentity::new(scope, crate::model::ObjectKind::Workspace, &ws.workspace_id)
                {
                    input.tabs.insert(
                        identity,
                        TabInventory {
                            expected: ws.tab_count,
                            complete,
                        },
                    );
                }
            }
        }
        input
    }

    /// N09 read-only search over a caller-owned snapshot and state.
    #[allow(dead_code)] // N10/N11 will connect input and rendering.
    pub(crate) fn picker_projection<'a>(
        &self,
        input: &'a crate::picker::PickerInput,
        state: &crate::picker::PickerState,
    ) -> crate::picker::Projection<'a> {
        crate::picker::search_project(input, state, &self.query, &self.config.picker)
    }

    /// Call only after a successful draw. A failed draw must retain the old token.
    pub(crate) fn commit_displayed_actions(&mut self) {
        let mut next = Vec::new();
        if let Some(entry) = self.selected_entry() {
            for slot in [Slot::Primary, Slot::Template] {
                if slot == Slot::Template && self.directory_template_for_selected().is_none() {
                    continue;
                }
                let action = self.resolved_action(entry, slot);
                let revision = self
                    .displayed_actions
                    .iter()
                    .find(|old| old.action == action)
                    .map(|old| old.revision)
                    .unwrap_or(self.action_revision + next.len() as u64 + 1);
                next.push(DisplayedAction { revision, action });
            }
        }
        if let Some(revision) = next.iter().map(|token| token.revision).max() {
            self.action_revision = self.action_revision.max(revision);
        }
        self.displayed_actions = next;
    }

    fn observe_action_inventory(&mut self) {
        let scope = self.navigation.scope().clone();
        let server = self.navigation.server();
        let mut inventory = Inventory {
            workspaces: InventoryRead::new(&scope, Some(server.workspaces())),
            panes: InventoryRead::new(&scope, Some(server.panes())),
            tabs: InventoryRead::new(&scope, Some(server.tabs())),
            agents: self
                .inventory
                .as_ref()
                .map(|i| i.agents.clone())
                .unwrap_or_else(|| InventoryRead::new(&scope, None)),
        };
        for error in [
            inventory.workspaces.error(),
            inventory.panes.error(),
            inventory.tabs.error(),
        ]
        .into_iter()
        .flatten()
        {
            self.navigation.observe_failure(error);
        }
        inventory.discard_old_scope(self.navigation.scope());
        self.joined_inventory = join_inventory(
            &inventory,
            &self.navigation,
            &mut self.pane_ordinals,
            &self.config.agent_aliases,
        );
        self.path_to_workspaces = self.joined_inventory.reuse.clone();
        self.inventory = Some(inventory);
    }

    fn revalidate_action(&mut self, retained: &ResolvedAction) -> Result<ResolvedAction, String> {
        let entry = self.selected_entry().ok_or("nothing selected")?.clone();
        if entry.key() != &retained.identity {
            return Err("selection changed; redraw before activation".into());
        }
        // Changes already visible in memory need no transport or filesystem access.
        if let Err(error) = retained.revalidate(&self.resolved_action(&entry, retained.slot)) {
            // Keep the displayed token unchanged. A later successful draw can
            // acknowledge preparation under the new config, never this press.
            if error.starts_with("action changed") {
                let prepared = Prepared::load(
                    &entry,
                    retained.slot,
                    &self.config,
                    &self.navigation,
                    None,
                    &actions::Files,
                );
                self.prepared_actions
                    .insert((entry.key().clone(), retained.slot), prepared);
            }
            return Err(error);
        }
        if matches!(retained.owner, actions::Owner::Server(_))
            && !matches!(retained.payload, actions::Payload::Plugin { .. })
        {
            self.observe_action_inventory();
        }
        let mut entry = entry;
        if entry.target().is_some() {
            let fresh = self
                .joined_inventory
                .workspaces
                .iter()
                .chain(&self.joined_inventory.panes)
                .find(|row| row.key() == entry.key());
            if let Some(fresh) = fresh {
                entry = fresh.clone();
            }
        }
        let prepared = Prepared::load(
            &entry,
            retained.slot,
            &self.config,
            &self.navigation,
            retained.template_file(),
            &actions::Files,
        );
        prepared.update_project_entry(&mut entry);
        self.prepared_actions
            .insert((entry.key().clone(), retained.slot), prepared);
        let fresh = self.resolved_action(&entry, retained.slot);
        if let Some(index) = self.filtered.get(self.selected).copied() {
            if let Some(fields) = self.search_fields.get_mut(index) {
                *fields = entry.search_fields();
            }
            self.entries[index] = entry;
        }
        retained.revalidate(&fresh)?;
        Ok(fresh)
    }

    pub(crate) fn displayed_action_needs_terminal(&self, template: bool) -> bool {
        let slot = if template {
            Slot::Template
        } else {
            Slot::Primary
        };
        self.displayed_actions.iter().any(|token| {
            token.action.slot == slot
                && matches!(
                    token.action.payload,
                    actions::Payload::Attach { .. } | actions::Payload::Run { .. }
                )
        })
    }

    #[cfg(test)]
    pub(crate) fn open_selected(&mut self, use_directory_template: bool) -> Result<(), String> {
        let slot = if use_directory_template {
            Slot::Template
        } else {
            Slot::Primary
        };
        let identity = self
            .selected_entry()
            .map(|entry| entry.key().clone())
            .ok_or("nothing selected")?;
        self.open_intent(&crate::picker::input::ActivationIntent { identity, slot })
    }

    pub(crate) fn open_intent(
        &mut self,
        intent: &crate::picker::input::ActivationIntent,
    ) -> Result<(), String> {
        let retained = self
            .displayed_actions
            .iter()
            .find(|token| {
                token.action.identity == intent.identity && token.action.slot == intent.slot
            })
            .ok_or("action has not been displayed for this identity and slot; redraw before activation")?
            .clone();
        self.activate_displayed(&retained)
    }

    pub(crate) fn activate_displayed(&mut self, token: &DisplayedAction) -> Result<(), String> {
        if !self
            .displayed_actions
            .iter()
            .any(|current| current.revision == token.revision && current.action == token.action)
        {
            return Err("displayed action changed; redraw before activation".into());
        }
        let action = self.revalidate_action(&token.action)?;
        self.displayed_actions.clear();
        let result = actions::dispatch(&action, &mut self.navigation);
        if let Some(inventory) = &mut self.inventory {
            inventory.discard_old_scope(self.navigation.scope());
        }
        let title = self
            .selected_entry()
            .map(|e| e.title.as_str())
            .unwrap_or("selected item");
        match &result {
            Ok(_) if action.notify_success => self.navigation.server().notify(
                &format!("Opened {title}"),
                true,
                &self.config.notifications,
            ),
            Err(error) if action.notify_error => self.navigation.server().notify(
                &format!("Failed {title}: {error}"),
                false,
                &self.config.notifications,
            ),
            _ => {}
        }
        // Neither completion kind carries invoking-client authorization.
        result.map(|_| ())
    }

    pub(crate) fn can_close_selected(&self) -> bool {
        self.selected_entry()
            .and_then(|entry| self.workspace_to_close(entry))
            .is_some_and(|id| {
                close_current_workspace_error(&id, self.navigation.picker_workspace()).is_none()
            })
    }

    pub(crate) fn retain_close_action(&mut self) -> Result<(), String> {
        self.close_token = None;
        let entry = self.selected_entry().ok_or("nothing selected")?;
        let id = self
            .workspace_to_close(entry)
            .ok_or("no open workspace for selected item")?;
        if let Some(error) = close_current_workspace_error(&id, self.navigation.picker_workspace())
        {
            return Err(error);
        }
        let action = self
            .displayed_actions
            .iter()
            .find(|token| {
                token.action.slot == Slot::Primary && &token.action.identity == entry.key()
            })
            .ok_or("close target has not been displayed")?
            .action
            .clone();
        let description = self
            .inventory_close_description(&id)
            .unwrap_or_else(|| close_description(&entry.title, &id));
        self.close_token = Some((entry.key().clone(), id, description, action));
        Ok(())
    }

    /// Describes workspace `id` as currently observed in the joined inventory.
    fn inventory_close_description(&self, id: &LiveIdentity) -> Option<String> {
        self.joined_inventory
            .workspaces
            .iter()
            .find(|row| row.target().is_some_and(|target| target.identity() == id))
            .map(|row| close_description(&row.title, id))
    }

    pub(crate) fn close_description(&self) -> Option<&str> {
        self.close_token
            .as_ref()
            .map(|(_, _, description, _)| description.as_str())
    }

    pub(crate) fn close_selected_workspace(&mut self) -> Result<(), String> {
        let (_, id, description, retained) =
            self.close_token.take().ok_or("no confirmed close target")?;
        if let Some(error) = close_current_workspace_error(&id, self.navigation.picker_workspace())
        {
            return Err(error);
        }
        self.revalidate_action(&retained)?;
        let entry = self.selected_entry().ok_or("nothing selected")?;
        let current_description = self.inventory_close_description(&id);
        if current_description.as_ref() != Some(&description) {
            return Err("close target changed; confirm again".into());
        }
        if self.workspace_to_close(entry).as_ref() != Some(&id) {
            return Err("close target changed; confirm again".into());
        }
        let title = entry.title.clone();
        // Not redundant with the check above: revalidation may observe a
        // transport failure that invalidates the scope and clears picker ownership.
        if let Some(error) = close_current_workspace_error(&id, self.navigation.picker_workspace())
        {
            return Err(error);
        }
        let result = self
            .navigation
            .server()
            .run(["workspace", "close", id.id()]);
        if let Some(error) = self.navigation.server().take_mutation_failure() {
            self.navigation.observe_failure(&error);
        }
        result?;
        self.navigation.server().notify(
            &format!("Closed {title}"),
            true,
            &self.config.notifications,
        );
        self.refresh();
        Ok(())
    }

    fn workspace_to_close(&self, e: &Entry) -> Option<LiveIdentity> {
        if matches!(
            e.destination_kind(),
            crate::model::DestinationKind::Machine
                | crate::model::DestinationKind::Pane
                | crate::model::DestinationKind::Tab
        ) {
            return None;
        }
        match e.source {
            Source::Workspace => e
                .target()
                .filter(|target| target.identity().scope() == self.navigation.scope())
                .map(|target| target.identity().clone()),
            Source::Agent => None,
            Source::Project => self
                .matching_project_workspace(e)
                .map(|ws| ws.identity.clone()),
            Source::Zoxide | Source::Root => {
                self.matching_dir_workspace(e).map(|ws| ws.identity.clone())
            }
            Source::Server | Source::Session | Source::QuickAction | Source::Integration => None,
        }
    }

    pub(crate) fn workspaces_for_entry(&self, e: &Entry) -> Vec<&WorkspaceRef> {
        let mut keys = Vec::new();
        if let Some(prepared) = self.prepared_actions.get(&(e.key().clone(), Slot::Primary)) {
            keys.extend(
                prepared.paths.iter().map(|path| {
                    WorkspaceReuseKey::new(self.navigation.scope().clone(), path.clone())
                }),
            );
        } else if let Some(key) = e
            .path
            .as_ref()
            .and_then(|path| self.navigation.display_reuse_key(path).ok())
        {
            keys.push(key);
        }
        keys.iter()
            .filter_map(|key| self.path_to_workspaces.get(key))
            .flatten()
            .collect()
    }

    pub(crate) fn matching_project_workspace(&self, e: &Entry) -> Option<&WorkspaceRef> {
        self.workspaces_for_entry(e).into_iter().find(|ws| {
            ws.identity.scope() == self.navigation.scope() && ws.kind == WorkspaceKind::Project
        })
    }

    pub(crate) fn matching_dir_workspace(&self, e: &Entry) -> Option<&WorkspaceRef> {
        let workspaces = self.workspaces_for_entry(e);
        workspaces
            .iter()
            .copied()
            .find(|ws| {
                ws.identity.scope() == self.navigation.scope() && ws.kind == WorkspaceKind::Dir
            })
            .or_else(|| {
                workspaces.into_iter().find(|ws| {
                    ws.identity.scope() == self.navigation.scope()
                        && ws.kind == WorkspaceKind::Unknown
                })
            })
    }

    #[cfg(test)]
    fn matching_template_workspace_by_key(&self, key: &WorkspaceReuseKey) -> Option<&WorkspaceRef> {
        let workspaces = self.path_to_workspaces.get(key)?;
        workspaces
            .iter()
            .find(|workspace| {
                workspace.identity.scope() == self.navigation.scope()
                    && workspace.kind == WorkspaceKind::Dir
            })
            .or_else(|| {
                workspaces
                    .iter()
                    .find(|workspace| workspace.identity.scope() == self.navigation.scope())
            })
    }
}

fn agent_status_bonus(entry: &Entry) -> i64 {
    let Some(agent) = &entry.agent else {
        return 0;
    };
    match AgentState::classify(&agent.status) {
        AgentState::Blocked => 4,
        AgentState::Attention => 3,
        AgentState::Done => 2,
        AgentState::Working => 1,
        AgentState::Idle | AgentState::Unknown => 0,
    }
}

/// Jump back is not available yet; returns why, for the caller to report as a failure.
pub(crate) fn jump_back(config: &Config) -> &'static str {
    if !config.jump_back.enabled {
        return "jump back is disabled in config";
    }
    "jump back requires qualified persistent history; legacy state was left untouched"
}

fn close_description(title: &str, id: &LiveIdentity) -> String {
    format!("{title} [{}]", id.id())
}

fn close_current_workspace_error(
    id: &LiveIdentity,
    picker_owner: Option<&LiveIdentity>,
) -> Option<String> {
    let Some(picker_owner) = picker_owner else {
        return Some("can't close a workspace because picker ownership is unknown".into());
    };
    if picker_owner.scope() != id.scope() {
        return Some("can't close a workspace because picker ownership is unverified".into());
    }
    (picker_owner == id)
        .then(|| "can't close the workspace that owns this picker; switch away first".into())
}

fn push_unique(entries: &mut Vec<Entry>, seen: &mut HashSet<EntryIdentity>, incoming: Vec<Entry>) {
    for e in incoming {
        if seen.insert(e.key().clone()) {
            entries.push(e);
        } else if let Some(existing) = entries.iter_mut().find(|entry| entry.key() == e.key()) {
            // Identity equality above includes provider namespace, owner and action key.
            // Local Attach uses the name, not display metadata. The collector emits
            // discovery first, then config order, so discovery wins or the first alias does.
            let local_attach = matches!(
                existing.key(),
                EntryIdentity::Provider { provider, action, .. }
                    if provider.preference_parts() == ("builtin", "sessions") && action == "attach"
            ) && existing.source == Source::Session
                && e.source == Source::Session
                && matches!(
                    (&existing.action, &e.action),
                    (
                        EntryAction::AttachSession { name: first, remote: None },
                        EntryAction::AttachSession { name: second, remote: None },
                    ) if first == second
                );
            if let Some(conflict) = &e.action_conflict {
                match &mut existing.action_conflict {
                    Some(previous) if previous != conflict => {
                        previous.push_str("; ");
                        previous.push_str(conflict);
                    }
                    None => existing.action_conflict = Some(conflict.clone()),
                    _ => {}
                }
            }
            if existing.action == e.action
                && (local_attach || existing.path == e.path)
                && existing.project == e.project
                && existing.template_file == e.template_file
                && (local_attach || existing.title == e.title)
                && existing
                    .target()
                    .and_then(crate::model::LiveTarget::workspace)
                    == e.target().and_then(crate::model::LiveTarget::workspace)
                && existing.target().and_then(crate::model::LiveTarget::tab)
                    == e.target().and_then(crate::model::LiveTarget::tab)
            {
                existing.provenance.extend(e.data.provenance);
                if local_attach {
                    existing.search_terms.push(e.data.title);
                    existing.search_terms.extend(e.data.search_terms);
                    let mut terms = HashSet::new();
                    existing
                        .search_terms
                        .retain(|term| terms.insert(term.clone()));
                }
            } else {
                existing.action_conflict.get_or_insert_with(|| {
                    "conflicting action data for the same provider identity".into()
                });
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::{
        path::PathBuf,
        time::{SystemTime, UNIX_EPOCH},
    };

    use crate::model::{LiveIdentity, ObjectKind, ProviderId};

    use super::*;
    use crate::paths::OwnedPath;
    use crate::{config::Config, model::Project, theme::Theme};

    #[cfg(unix)]
    fn with_recorded_herdr(exit_code: u8, check: impl FnOnce(&std::path::Path)) {
        use std::os::unix::fs::PermissionsExt;

        let _lock = crate::herdr::bin_path_lock();
        struct Stub {
            previous: Option<std::ffi::OsString>,
            dir: PathBuf,
        }
        impl Drop for Stub {
            fn drop(&mut self) {
                match &self.previous {
                    Some(value) => env::set_var("HERDR_BIN_PATH", value),
                    None => env::remove_var("HERDR_BIN_PATH"),
                }
                let _ = fs::remove_dir_all(&self.dir);
            }
        }
        let stub = Stub {
            previous: env::var_os("HERDR_BIN_PATH"),
            dir: env::temp_dir().join(format!(
                "n03-dispatch-{}",
                SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap()
                    .as_nanos()
            )),
        };
        fs::create_dir_all(&stub.dir).unwrap();
        let log = stub.dir.join("calls");
        let executable = stub.dir.join("herdr");
        fs::write(
            &executable,
            format!(
                r#"#!/bin/sh
args=$(printf '%s\n' "$@")
printf 'call\n%s\n' "$args" >> '{}'
printf '{{"id":"stub","result":{{"type":"%s_list","workspaces":[],"panes":[],"tabs":[]}}}}\n' "$1"
exit {exit_code}
"#,
                log.display()
            ),
        )
        .unwrap();
        fs::set_permissions(&executable, fs::Permissions::from_mode(0o755)).unwrap();
        env::set_var("HERDR_BIN_PATH", executable);
        check(&log);
    }

    fn open_displayed(app: &mut App) -> Result<(), String> {
        app.commit_displayed_actions();
        app.open_selected(false)
    }

    fn test_app(config: Config, theme: Theme) -> App {
        let mut app = App::for_test(config, theme);
        app.navigation = NavigationContext::colocated();
        app
    }
    fn reuse_key(app: &App, path: &str) -> WorkspaceReuseKey {
        app.navigation
            .reuse_key(&OwnedPath::process(PathBuf::from(path)))
            .unwrap()
    }
    fn entry(source: Source, path: &str, title: &str) -> Entry {
        Entry::new(
            EntryIdentity::provider("test", path, "open-directory").unwrap(),
            crate::model::EntryData {
                source,
                title: title.into(),
                subtitle: String::new(),
                path: Some(OwnedPath::process(PathBuf::from(path))),
                workspace_id: None,
                workspace_label: None,
                agent: None,
                project: None,
                action: EntryAction::FocusOrCreateDir,
                source_label: None,
                search_terms: vec![],
                provenance: [ProviderId::new("test").unwrap()].into(),
            },
        )
    }

    fn workspace(
        context: &NavigationContext,
        id: &str,
        label: &str,
        kind: WorkspaceKind,
        path: &str,
    ) -> WorkspaceRef {
        WorkspaceRef {
            identity: LiveIdentity::new(context.scope(), ObjectKind::Workspace, id).unwrap(),
            label: label.into(),
            kind,
            path: Some(OwnedPath::process(PathBuf::from(path))),
            tab_count: 1,
            pane_count: 1,
        }
    }

    fn agent_entry() -> Entry {
        agent_entry_with_status("idle")
    }

    fn agent_entry_with_status(status: &str) -> Entry {
        Entry::new(
            EntryIdentity::provider("test", "agent", "focus-agent").unwrap(),
            crate::model::EntryData {
                source: Source::Agent,
                title: "claude · Dotfiles · dotfiles".into(),
                subtitle: format!("{status} · wF:p2 · wF:t2"),
                path: Some(OwnedPath::process(PathBuf::from("/home/fenix/dotfiles"))),
                workspace_id: Some("wF".into()),
                workspace_label: Some("Dotfiles".into()),
                agent: Some(crate::model::AgentMetadata {
                    agent: Some("claude".into()),
                    name: None,
                    display_agent: None,
                    aliases: vec!["main ai dot".into()],
                    status: status.into(),
                    revision: 0,
                    session: None,
                }),
                project: None,
                action: EntryAction::FocusPane,
                source_label: None,
                search_terms: vec!["main ai dot".into()],
                provenance: [ProviderId::new("test").unwrap()].into(),
            },
        )
    }

    #[test]
    fn test_apps_own_distinct_writable_history_until_drop() {
        let first = App::for_test(Config::default(), Theme::load(false));
        let second = App::for_test(Config::default(), Theme::load(false));
        let first_path = first.test_history.as_ref().unwrap().0.clone();
        let second_path = second.test_history.as_ref().unwrap().0.clone();
        assert!(first_path.is_absolute());
        assert_ne!(first_path, second_path);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                fs::metadata(&first_path).unwrap().permissions().mode() & 0o777,
                0o700
            );
        }
        let row = entry(Source::Project, "/fixture/project", "project");
        let first_key = first.history.key(&row, None).unwrap().unwrap();
        let second_key = second.history.key(&row, None).unwrap().unwrap();
        assert_ne!(
            first_key, second_key,
            "each store gets its own provider owner"
        );
        let mut moved = first;
        assert!(first_path.is_dir());
        moved.history.set_pin(first_key.clone(), true).unwrap();
        assert!(Store::open(first_path.clone()).contains(&first_key));
        assert!(!second.history.contains(&first_key));
        drop(moved);
        assert!(!first_path.exists());
        assert!(second_path.is_dir());
        assert!(second.history.diagnostic().is_none());
        drop(second);
        assert!(!second_path.exists());
    }

    #[test]
    fn test_app_history_is_removed_during_unwind() {
        let mut directory = None;
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let app = App::for_test(Config::default(), Theme::load(false));
            directory = Some(app.test_history.as_ref().unwrap().0.clone());
            panic!("exercise test history cleanup");
        }));
        assert!(result.is_err());
        assert!(!directory.unwrap().exists());
    }

    #[cfg(unix)]
    mod ambient_history {
        use super::*;
        use std::{
            collections::BTreeMap,
            ffi::OsString,
            io::Write,
            os::unix::{fs::OpenOptionsExt, fs::PermissionsExt, process::CommandExt},
            process::{Child, Command, ExitStatus, Stdio},
        };

        const PARENT: &str =
            "app::tests::ambient_history::ordinary_tests_leave_ambient_history_untouched";
        const POSITIVE: &str =
            "app::tests::ambient_history::production_constructor_imports_sentinel_history_child";
        const MARKER: &str = "NAVIGATOR_HISTORY_ISOLATION_CHILD";
        const STUB: &[u8] = b"#!/bin/sh\necho 'blocked offline command' >&2\nexit 125\n";
        const LEGACY: &[u8] = b"[\"isolation-sentinel-awaiting-import\"]\n";
        const HISTORY: &[u8] = include_bytes!("../tests/fixtures/history/provider-pins-v1.json");
        const PROGRAMS: &[&str] = &[
            "herdr",
            "herdr-plus",
            "ssh",
            "scp",
            "curl",
            "wget",
            "zoxide",
        ];
        const XDG: &[(&str, &str)] = &[
            ("XDG_CONFIG_HOME", "config"),
            ("XDG_CACHE_HOME", "cache"),
            ("XDG_DATA_HOME", "data"),
            ("XDG_STATE_HOME", "state"),
            ("XDG_RUNTIME_DIR", "runtime"),
        ];

        fn private_file(path: &Path, bytes: &[u8]) {
            let mut file = fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .mode(0o600)
                .open(path)
                .unwrap();
            file.write_all(bytes).unwrap();
        }

        fn snapshot(directory: &Path) -> BTreeMap<OsString, Vec<u8>> {
            fs::read_dir(directory)
                .unwrap()
                .map(|entry| {
                    let entry = entry.unwrap();
                    assert!(
                        entry.file_type().unwrap().is_file(),
                        "unexpected history directory entry"
                    );
                    (entry.file_name(), fs::read(entry.path()).unwrap())
                })
                .collect()
        }

        struct RunningChild(Child);
        impl Drop for RunningChild {
            fn drop(&mut self) {
                if !matches!(self.0.try_wait(), Ok(Some(_))) {
                    // Each child has its own process group, including any test stubs.
                    unsafe {
                        libc::kill(-(self.0.id() as i32), libc::SIGKILL);
                    }
                    let _ = self.0.kill();
                    let _ = self.0.wait();
                }
            }
        }

        fn run_child(
            root: &Path,
            sentinel: &Path,
            fallback: bool,
            positive: bool,
        ) -> (ExitStatus, String) {
            let args = if positive {
                vec!["--ignored", "--exact", POSITIVE]
            } else {
                // Run the real ordinary suite with libtest's default parallelism.
                vec!["--skip", PARENT]
            };
            run_isolated_child(
                root,
                sentinel,
                fallback,
                false,
                &args,
                if positive {
                    "positive.log"
                } else {
                    "ordinary.log"
                },
            )
        }

        fn run_isolated_child(
            root: &Path,
            sentinel: &Path,
            fallback: bool,
            managed_ambient: bool,
            args: &[&str],
            log_name: &str,
        ) -> (ExitStatus, String) {
            let log = root.join(log_name);
            let output = fs::File::create(&log).unwrap();
            let mut command = Command::new(env::current_exe().unwrap());
            // Freeze the filtered environment before sibling tests can add route variables.
            command.env_clear().envs(env::vars_os().filter(|(key, _)| {
                let name = key.to_string_lossy();
                !name.starts_with("HERDR_")
                    && !name.starts_with("XDG_")
                    && name != "RUST_TEST_THREADS"
            }));
            command
                .env(MARKER, root)
                .env("HOME", root.join("home"))
                .env("TMPDIR", root.join("tmp"))
                .env("PATH", format!("{}/bin:/usr/bin:/bin", root.display()))
                .env("HERDR_BIN_PATH", root.join("bin/herdr"))
                .current_dir(root.join("work"))
                .stdin(Stdio::null())
                .stdout(output.try_clone().unwrap())
                .stderr(output)
                .process_group(0);
            for (key, directory) in XDG {
                command.env(key, root.join(directory));
            }
            if !fallback {
                command.env("HERDR_PLUGIN_CONFIG_DIR", sentinel);
            }
            if managed_ambient {
                command
                    .env("HERDR_SOCKET_PATH", root.join("runtime/absent.sock"))
                    .env("HERDR_ENV", "1")
                    .env("HERDR_PLUGIN_ID", "ownership-fixture")
                    .env("HERDR_PLUGIN_ROOT", root.join("work"))
                    .env(
                        "HERDR_PLUGIN_CONTEXT_JSON",
                        r#"{"workspace_id":"ambient-owner-fixture","client_id":"untrusted"}"#,
                    );
            }
            command.args(args);
            let mut child = RunningChild(command.spawn().unwrap());
            let deadline = Instant::now() + Duration::from_secs(40);
            let status = loop {
                if let Some(status) = child.0.try_wait().unwrap() {
                    break status;
                }
                if Instant::now() >= deadline {
                    drop(child);
                    panic!(
                        "history isolation child exceeded 40 seconds: {}",
                        fs::read_to_string(log).unwrap()
                    );
                }
                thread::sleep(Duration::from_millis(10));
            };
            (status, fs::read_to_string(log).unwrap())
        }

        fn isolated_root() -> TempHistory {
            let root = TempHistory::new();
            for directory in [
                "home", "tmp", "bin", "work", "config", "cache", "data", "state", "runtime",
            ] {
                fs::create_dir(root.0.join(directory)).unwrap();
            }
            for program in PROGRAMS {
                let path = root.0.join("bin").join(program);
                private_file(&path, STUB);
                fs::set_permissions(path, fs::Permissions::from_mode(0o700)).unwrap();
            }
            root
        }

        #[test]
        fn ordinary_tests_leave_ambient_history_untouched() {
            for fallback in [false, true] {
                let root = isolated_root();
                private_file(
                    &root.0.join("child-marker"),
                    b"navigator-history-isolation-v1",
                );
                let sentinel = if fallback {
                    root.0
                        .join("home/.config/herdr/plugins/config/herdr-navigator")
                } else {
                    root.0.join("sentinel")
                };
                fs::create_dir_all(&sentinel).unwrap();
                fs::set_permissions(&sentinel, fs::Permissions::from_mode(0o700)).unwrap();
                private_file(&sentinel.join("navigation-history.json"), HISTORY);
                private_file(&sentinel.join("pinned-entries.json"), LEGACY);
                let before = snapshot(&sentinel);
                let (status, output) = run_child(&root.0, &sentinel, fallback, false);
                assert_eq!(
                    snapshot(&sentinel),
                    before,
                    "ordinary tests changed ambient history; fallback={fallback}\n{output}"
                );
                assert!(
                    status.success(),
                    "ordinary suite failed; fallback={fallback}\n{output}"
                );
                assert!(
                    fs::read_dir(root.0.join("tmp")).unwrap().all(|entry| {
                        !entry
                            .unwrap()
                            .file_name()
                            .to_string_lossy()
                            .starts_with("navigator-test-history-")
                    }),
                    "ordinary App test directories leaked"
                );

                let (status, output) = run_child(&root.0, &sentinel, fallback, true);
                assert!(
                    status.success(),
                    "production positive control failed: {output}"
                );
                let after = snapshot(&sentinel);
                assert_eq!(after.len(), before.len() + 1);
                assert_eq!(after[&OsString::from("pinned-entries.json")], LEGACY);
                assert_ne!(after[&OsString::from("navigation-history.json")], HISTORY);
                assert!(after.contains_key(&OsString::from("navigation-history.lock")));
                let path = root.0.clone();
                drop(root);
                assert!(!path.exists());
            }
        }

        #[test]
        fn ordinary_ownership_tests_ignore_managed_ambient_route() {
            const EXACT: &str =
                "app::tests::ambient_history::ordinary_ownership_tests_ignore_managed_ambient_route";
            const CHILD_FILE: &str = "managed-ownership-child";
            let child_root = env::var_os(MARKER).map(PathBuf::from).filter(|root| {
                fs::read(root.join(CHILD_FILE)).ok().as_deref() == Some(EXACT.as_bytes())
            });
            if let Some(root) = child_root {
                assert!(root.is_absolute());
                assert_eq!(fs::canonicalize(&root).unwrap(), root);
                assert_eq!(
                    fs::metadata(&root).unwrap().permissions().mode() & 0o777,
                    0o700
                );
                assert_eq!(
                    env::var_os("HOME"),
                    Some(root.join("home").into_os_string())
                );
                assert_eq!(
                    env::var_os("TMPDIR"),
                    Some(root.join("tmp").into_os_string())
                );
                assert_eq!(env::current_dir().unwrap(), root.join("work"));
                assert_eq!(crate::paths::plugin_config_dir(), root.join("sentinel"));
                for (key, directory) in XDG {
                    assert_eq!(
                        env::var_os(key),
                        Some(root.join(directory).into_os_string())
                    );
                }
                for program in PROGRAMS {
                    assert_eq!(fs::read(root.join("bin").join(program)).unwrap(), STUB);
                }
                let socket = root.join("runtime/absent.sock");
                assert!(!socket.exists());
                assert_eq!(
                    env::var_os("HERDR_SOCKET_PATH"),
                    Some(socket.clone().into_os_string())
                );
                assert_eq!(env::var("HERDR_ENV").unwrap(), "1");
                assert_eq!(env::var("HERDR_PLUGIN_ID").unwrap(), "ownership-fixture");
                assert_eq!(
                    env::var_os("HERDR_PLUGIN_ROOT"),
                    Some(root.join("work").into_os_string())
                );
                assert_eq!(
                    env::var_os("HERDR_BIN_PATH"),
                    Some(root.join("bin/herdr").into_os_string())
                );
                // Capture only. Never execute the stub or connect to the absent socket.
                let context = NavigationContext::new();
                assert!(context.server().managed_plugin());
                assert_eq!(
                    context.server().process_route().socket.as_ref(),
                    Some(&socket)
                );
                assert!(context.require_process_server_colocation().is_ok());
                assert_eq!(
                    context.picker_workspace().unwrap().id(),
                    "ambient-owner-fixture"
                );
                assert_eq!(context.picker_workspace().unwrap().scope(), context.scope());
                assert!(context.current_client_selection().is_none());
                for (fixture, colocated) in [
                    (NavigationContext::unmanaged(), false),
                    (NavigationContext::reported(context.scope().clone()), false),
                    (NavigationContext::colocated(), true),
                ] {
                    assert!(!fixture.server().managed_plugin());
                    assert!(fixture.server().process_route().socket.is_none());
                    assert!(fixture.picker_workspace().is_none());
                    assert!(fixture.current_client_selection().is_none());
                    assert_eq!(
                        fixture.require_process_server_colocation().is_ok(),
                        colocated
                    );
                }
                return;
            }

            let root = isolated_root();
            private_file(&root.0.join(CHILD_FILE), EXACT.as_bytes());
            let sentinel = root.0.join("sentinel");
            fs::create_dir(&sentinel).unwrap();
            let cases = [
                EXACT,
                "navigation::tests::context_targets_require_current_scope_and_valid_ancestry",
                "paths::ownership_tests::process_paths_keep_filesystem_access_but_do_not_imply_server_ownership",
                "app::tests::unknown_process_server_owner_blocks_create_and_template_before_any_io",
                "tui::tests::foreign_paths_render_as_reported_text_and_unknown_ownership_never_says_create",
                "app::tests::picker_task_groups_have_no_actions_and_provider_sources_do_not_invent_machines",
                "app::tests::ordinary_route_quick_actions_need_no_filesystem_colocation_and_propagate_failure",
                "navigation::tests::environment_owner_hint_never_sets_client_selection_or_path_authority",
            ];
            let results: Vec<_> = cases
                .iter()
                .enumerate()
                .map(|(index, name)| {
                    let (status, output) = run_isolated_child(
                        &root.0,
                        &sentinel,
                        false,
                        true,
                        &["--exact", name],
                        &format!("ownership-{index}.log"),
                    );
                    (*name, status, output)
                })
                .collect();
            let passed = results.iter().all(|(_, status, output)| {
                status.success() && output.contains("1 passed; 0 failed")
            });
            let report = results
                .iter()
                .map(|(name, status, output)| format!("{name}: {status}\n{output}"))
                .collect::<Vec<_>>()
                .join("\n");
            let path = root.0.clone();
            drop(root);
            assert!(!path.exists());
            assert!(
                passed,
                "managed ambient ownership children failed:\n{report}"
            );
        }

        #[test]
        fn picker_action_owner_refresh_projection() {
            use crate::picker::{Owner, PickerState, RowId, TaskKind};
            const EXACT: &str =
                "app::tests::ambient_history::picker_action_owner_refresh_projection";
            const CHILD_FILE: &str = "picker-action-owner-child";
            let child_root = env::var_os(MARKER).map(PathBuf::from).filter(|root| {
                fs::read(root.join(CHILD_FILE)).ok().as_deref() == Some(EXACT.as_bytes())
            });
            let Some(root) = child_root else {
                let root = isolated_root();
                private_file(&root.0.join(CHILD_FILE), EXACT.as_bytes());
                fs::create_dir(root.0.join("sentinel")).unwrap();
                let (status, output) = run_isolated_child(
                    &root.0,
                    &root.0.join("sentinel"),
                    false,
                    false,
                    &["--exact", EXACT],
                    "picker-owner.log",
                );
                assert!(status.success(), "picker owner child failed: {output}");
                let path = root.0.clone();
                drop(root);
                assert!(!path.exists());
                return;
            };
            assert!(root.is_absolute());
            assert_eq!(fs::canonicalize(&root).unwrap(), root);
            assert_eq!(
                fs::metadata(&root).unwrap().permissions().mode() & 0o777,
                0o700
            );
            assert_eq!(
                env::var_os("HOME"),
                Some(root.join("home").into_os_string())
            );
            assert_eq!(
                env::var_os("TMPDIR"),
                Some(root.join("tmp").into_os_string())
            );
            assert_eq!(env::current_dir().unwrap(), root.join("work"));
            assert_eq!(crate::paths::plugin_config_dir(), root.join("sentinel"));
            assert_eq!(
                env::var("PATH").unwrap(),
                format!("{}/bin:/usr/bin:/bin", root.display())
            );
            for (key, directory) in XDG {
                assert_eq!(
                    env::var_os(key),
                    Some(root.join(directory).into_os_string())
                );
            }
            for program in PROGRAMS {
                assert_eq!(fs::read(root.join("bin").join(program)).unwrap(), STUB);
            }
            // HOME is cached. Only this fresh child creates the collector directory.
            assert_eq!(home(), root.join("home"));
            fs::create_dir_all(herdr_plus_quick_actions_dir()).unwrap();
            let (mut app, runner) = picker_action_owner_app(false);
            app.config.sources.herdr_plus_quick_actions = true;
            let mut integration = picker_action_owner_integration(true);
            integration.collect = "printf 'collect\\n' >> collector.log; printf '%s' '[{\"id\":\"task\",\"title\":\"Server task\",\"kind\":\"server\"}]'".into();
            app.config.integrations = vec![integration];
            queue_action_lists(&runner, &action_lists(Some(("w1", "fixture workspace"))));
            app.refresh();
            assert!(app.navigation.require_process_server_colocation().is_err());
            assert_eq!(app.entries.len(), 3);
            let calls = format!("{:?}", runner.calls.lock().unwrap());
            assert_eq!(runner.calls.lock().unwrap().len(), 3);
            assert!(runner.replies.lock().unwrap().is_empty());
            assert_eq!(
                runner
                    .calls
                    .lock()
                    .unwrap()
                    .iter()
                    .map(|call| call.args.clone())
                    .collect::<Vec<_>>(),
                ["workspace", "pane", "tab"]
                    .map(|kind| vec![OsString::from(kind), OsString::from("list")])
            );
            let collector = fs::read(root.join("work/collector.log")).unwrap();
            assert_eq!(collector, b"collect\n");
            let history = app.test_history.as_ref().unwrap().0.clone();
            let history_before = snapshot(&history);
            let ambient_before = snapshot(&root.join("sentinel"));
            let legacy = (
                app.selected,
                app.filtered.clone(),
                app.filtered_scores.clone(),
                app.source_filter,
                app.query.clone(),
            );
            let server = Owner::Server(app.navigation.scope().clone());
            let process = Owner::Process(crate::navigation::ProcessHostRef);
            let mut previous = None;
            for _ in 0..3 {
                let input = app.picker_input();
                assert_eq!(
                    input.groups,
                    vec![
                        (server.clone(), TaskKind::Actions),
                        (process.clone(), TaskKind::Actions)
                    ]
                );
                assert!(input
                    .headings
                    .iter()
                    .all(|h| h.machine.is_none() && !h.local));
                let mut state = PickerState::new(&input);
                for owner in [&server, &process] {
                    state.toggle(RowId::TaskGroup(owner.clone(), TaskKind::Actions));
                }
                let projection = app.picker_projection(&input, &state);
                for row in projection.rows.iter().filter(|r| r.destination.is_some()) {
                    let destination = row.destination.unwrap();
                    let resolved = app.resolved_action(&destination.entry, Slot::Primary);
                    assert_eq!(row.action(), Some(&resolved));
                    match destination.entry.action {
                        EntryAction::InvokePluginAction { .. } => {
                            assert!(matches!(resolved.payload, actions::Payload::Plugin { .. }));
                            assert_eq!(
                                resolved.owner,
                                actions::Owner::Server(app.navigation.scope().clone())
                            );
                            assert_eq!(destination.owner, server);
                            assert_eq!(
                                row.ancestors,
                                vec![
                                    RowId::Heading(server.clone()),
                                    RowId::TaskGroup(server.clone(), TaskKind::Actions)
                                ]
                            );
                        }
                        EntryAction::RunCommand { .. } => {
                            assert_eq!(destination.entry.source, Source::Server);
                            assert!(matches!(resolved.payload, actions::Payload::Run { .. }));
                            assert_eq!(
                                resolved.owner,
                                actions::Owner::Process(crate::navigation::ProcessHostRef)
                            );
                            assert_eq!(destination.owner, process);
                            assert_eq!(
                                row.ancestors,
                                vec![
                                    RowId::Heading(process.clone()),
                                    RowId::TaskGroup(process.clone(), TaskKind::Actions)
                                ]
                            );
                        }
                        EntryAction::FocusWorkspace { .. } => assert_eq!(destination.owner, server),
                        _ => panic!("unexpected refresh destination"),
                    }
                }
                assert_eq!(
                    projection
                        .rows
                        .iter()
                        .filter(|r| r.destination.is_some())
                        .count(),
                    3
                );
                let current = format!("{input:?}\n{projection:?}");
                if let Some(previous) = &previous {
                    assert_eq!(&current, previous);
                }
                previous = Some(current);
            }
            assert_eq!(format!("{:?}", runner.calls.lock().unwrap()), calls);
            assert_eq!(
                fs::read(root.join("work/collector.log")).unwrap(),
                collector
            );
            assert_eq!(snapshot(&history), history_before);
            assert_eq!(snapshot(&root.join("sentinel")), ambient_before);
            assert_eq!(
                (
                    app.selected,
                    app.filtered.clone(),
                    app.filtered_scores.clone(),
                    app.source_filter,
                    app.query.clone()
                ),
                legacy
            );
        }

        #[test]
        #[ignore = "only the isolated parent may open production history"]
        fn production_constructor_imports_sentinel_history_child() {
            let Some(root) = env::var_os(MARKER).map(PathBuf::from) else {
                return;
            };
            // Refuse ambient execution, including a plain cargo test -- --ignored.
            assert!(root.is_absolute());
            assert_eq!(fs::canonicalize(&root).unwrap(), root);
            assert_eq!(
                fs::metadata(&root).unwrap().permissions().mode() & 0o777,
                0o700
            );
            assert_eq!(
                fs::read(root.join("child-marker")).unwrap(),
                b"navigator-history-isolation-v1"
            );
            assert_eq!(
                env::var_os("HOME"),
                Some(root.join("home").into_os_string())
            );
            assert_eq!(
                env::var_os("TMPDIR"),
                Some(root.join("tmp").into_os_string())
            );
            assert_eq!(env::current_dir().unwrap(), root.join("work"));
            assert_eq!(
                env::var("PATH").unwrap(),
                format!("{}/bin:/usr/bin:/bin", root.display())
            );
            assert_eq!(
                env::var_os("HERDR_BIN_PATH"),
                Some(root.join("bin/herdr").into_os_string())
            );
            for (key, directory) in XDG {
                assert_eq!(
                    env::var_os(key),
                    Some(root.join(directory).into_os_string())
                );
            }
            for program in PROGRAMS {
                assert_eq!(fs::read(root.join("bin").join(program)).unwrap(), STUB);
            }
            let directory = crate::paths::plugin_config_dir();
            let expected = if env::var_os("HERDR_PLUGIN_CONFIG_DIR").is_some() {
                root.join("sentinel")
            } else {
                root.join("home/.config/herdr/plugins/config/herdr-navigator")
            };
            assert_eq!(directory, expected);
            assert_eq!(fs::canonicalize(&directory).unwrap(), directory);
            assert_eq!(
                fs::read(directory.join("navigation-history.json")).unwrap(),
                HISTORY
            );
            assert_eq!(
                fs::read(directory.join("pinned-entries.json")).unwrap(),
                LEGACY
            );
            assert!(!directory.join("navigation-history.lock").exists());
            let app = App::new(Config::default(), Theme::load(false));
            assert!(app.history.diagnostic().is_none());
            assert!(app.test_history.is_none());
            assert_eq!(app.history.records().unwrap().len(), 2);
            let document: serde_json::Value = serde_json::from_slice(
                &fs::read(directory.join("navigation-history.json")).unwrap(),
            )
            .unwrap();
            assert!(document["records"]
                .as_array()
                .unwrap()
                .iter()
                .any(|record| {
                    record["preference"]["kind"] == "unresolved"
                        && record["preference"]["value"] == "isolation-sentinel-awaiting-import"
                }));
            assert_eq!(
                fs::read(directory.join("pinned-entries.json")).unwrap(),
                LEGACY
            );
        }
    }

    #[test]
    fn agent_token_filters_match_identity_parts() {
        let agent = agent_entry();

        assert!(Query::parse("!claude @dot /dot #idle").filters_match(&agent));
        assert!(Query::parse("@wF").filters_match(&agent));
        assert!(!Query::parse("!codex").filters_match(&agent));
        assert!(!Query::parse("!dotfiles").filters_match(&agent));
        assert!(!Query::parse("!claude").filters_match(&entry(Source::Project, "/tmp", "claude")));
    }

    #[test]
    fn agent_shortcut_shows_all_agents_and_priority_is_configurable() {
        let idle = agent_entry_with_status("idle");
        let working = agent_entry_with_status("working");
        let blocked = agent_entry_with_status("blocking");
        let attention = agent_entry_with_status("needs attention");
        let done = agent_entry_with_status("done");

        assert!(Query::parse("@").filters_match(&idle));
        assert!(Query::parse("@").filters_match(&blocked));
        assert!(Query::parse("@idle").filters_match(&idle));
        assert!(Query::parse("@Dotfiles").filters_match(&idle));
        assert_eq!(agent_status_bonus(&blocked), 4);
        assert_eq!(agent_status_bonus(&attention), 3);
        assert_eq!(agent_status_bonus(&done), 2);
        assert_eq!(agent_status_bonus(&working), 1);
        assert_eq!(agent_status_bonus(&idle), 0);
        assert_eq!(AgentSort::resolve("priority"), AgentSort::Priority);
        assert_eq!(AgentSort::resolve("spaces"), AgentSort::Spaces);
    }

    #[test]
    fn agent_sort_falls_back_to_herdr_agent_panel_sort() {
        let sort = |source: &str| {
            AgentSort::from_herdr_config(Some(&source.parse::<toml::Value>().unwrap()))
        };
        assert_eq!(AgentSort::from_herdr_config(None), AgentSort::Spaces);
        assert_eq!(sort(""), AgentSort::Spaces);
        assert_eq!(
            sort("[ui]\nagent_panel_sort = \"Priority\""),
            AgentSort::Priority
        );
        assert_eq!(
            sort("[ui]\nagent_panel_sort = \"spaces\""),
            AgentSort::Spaces
        );
        assert_eq!(sort("agent_panel_sort = \"priority\""), AgentSort::Priority);
        // `ui.agent_panel_sort` wins over the top-level key.
        assert_eq!(
            sort("agent_panel_sort = \"priority\"\n[ui]\nagent_panel_sort = \"spaces\""),
            AgentSort::Spaces
        );
        // A `[ui]` table without the key still consults the top level.
        assert_eq!(
            sort("agent_panel_sort = \"priority\"\n[ui]\nother = 1"),
            AgentSort::Priority
        );
        assert_eq!(sort("[ui]\nagent_panel_sort = 3"), AgentSort::Spaces);
    }

    #[test]
    fn agent_aliases_are_searchable_plain_text() {
        assert!(agent_entry()
            .search_fields()
            .iter()
            .any(|field| field.contains("main ai dot")));
    }

    #[test]
    fn search_prefers_exact_title_then_shorter_matching_paths() {
        for engine in ["nucleo", "skim", "simple"] {
            let mut app = test_app(Config::default(), Theme::load(false));
            app.config.picker.engine = engine.into();
            app.entries = vec![
                entry(Source::Zoxide, "/opt/work/align", "align"),
                entry(Source::Zoxide, "/opt/work/atredis", "atredis"),
                entry(Source::Zoxide, "/opt/work", "work"),
            ];
            app.query = "work".into();

            app.apply_filter();

            let paths = app
                .filtered
                .iter()
                .map(|index| {
                    app.entries[*index]
                        .path
                        .as_ref()
                        .unwrap()
                        .process_path()
                        .unwrap()
                })
                .collect::<Vec<_>>();
            assert_eq!(paths[0], Path::new("/opt/work"), "{engine}");
            assert_eq!(paths[1], Path::new("/opt/work/align"), "{engine}");
        }
    }

    #[test]
    fn search_does_not_fuzzy_match_across_unrelated_fields() {
        for engine in ["nucleo", "skim", "simple"] {
            let mut app = test_app(Config::default(), Theme::load(false));
            app.config.picker.engine = engine.into();
            app.entries = vec![
                entry(
                    Source::Root,
                    "/Projects/ruby/pickaxe/tut_containers/word_freq",
                    "word_freq",
                ),
                entry(Source::Zoxide, "/opt/work", "work"),
            ];
            app.query = "work".into();

            app.apply_filter();

            assert_eq!(app.filtered.len(), 1, "{engine}");
            assert_eq!(app.selected_entry().unwrap().title, "work", "{engine}");
        }
    }

    #[test]
    fn default_empty_picker_prioritizes_agent_status() {
        let mut app = test_app(Config::default(), Theme::load(false));
        app.agent_sort = AgentSort::Priority;
        app.entries = vec![
            agent_entry_with_status("idle"),
            agent_entry_with_status("done"),
        ];
        app.apply_filter();

        let first = &app.entries[app.filtered[0]];
        assert!(first.subtitle.starts_with("done"));
    }

    #[test]
    fn refresh_selection_is_restored_by_stable_entry_key() {
        let mut app = test_app(Config::default(), Theme::load(false));
        app.entries = vec![
            entry(Source::Root, "/alpha", "alpha"),
            entry(Source::Root, "/zulu", "zulu"),
        ];
        app.filtered = vec![1, 0];
        app.selected = 0;
        let selected_key = app.entries[1].key().clone();

        app.filtered = vec![0, 1];
        app.selected = 0;
        app.restore_selection(Some(&selected_key));

        assert_eq!(app.selected, 1);
        assert_eq!(app.selected_entry().unwrap().title, "zulu");
    }

    #[test]
    fn previous_workspace_is_pinned_only_on_initial_unfiltered_view() {
        let mut app = test_app(Config::default(), Theme::load(false));
        let mut alpha = entry(Source::Workspace, "/alpha", "alpha");
        alpha.workspace_id = Some("w1".into());
        alpha.action = EntryAction::FocusWorkspace { id: "w1".into() };
        let mut zulu = entry(Source::Workspace, "/zulu", "zulu");
        zulu.workspace_id = Some("w2".into());
        zulu.action = EntryAction::FocusWorkspace { id: "w2".into() };
        app.entries = vec![alpha, zulu];
        app.previous_workspace_id = Some(
            app.entries
                .iter()
                .find(|entry| entry.workspace_id.as_deref() == Some("w2"))
                .unwrap()
                .key()
                .clone(),
        );

        app.apply_filter();
        assert_eq!(
            app.selected_entry().unwrap().workspace_id.as_deref(),
            Some("w2")
        );

        app.source_filter = Some(Source::Workspace);
        app.apply_filter();
        assert_eq!(
            app.selected_entry().unwrap().workspace_id.as_deref(),
            Some("w1")
        );

        app.source_filter = None;
        app.config.jump_back.pin_previous = false;
        app.apply_filter();
        assert_eq!(
            app.selected_entry().unwrap().workspace_id.as_deref(),
            Some("w1")
        );
    }

    #[test]
    fn cycle_filter_walks_every_source_then_clears_and_resets_selection() {
        let mut app = test_app(Config::default(), Theme::load(false));
        app.entries = vec![
            entry(Source::Workspace, "/alpha", "alpha"),
            entry(Source::Workspace, "/beta", "beta"),
        ];
        app.apply_filter();
        let mut seen = Vec::new();
        for _ in 0..=Source::all().len() {
            app.selected = 1;
            app.cycle_filter();
            assert_eq!(app.selected, 0);
            seen.push(app.source_filter);
        }
        let mut expected: Vec<_> = Source::all().into_iter().map(Some).collect();
        expected.push(None);
        assert_eq!(seen, expected);
        assert_eq!(app.filtered.len(), 2);
    }

    #[test]
    fn invocation_marks_sort_first() {
        let mut app = test_app(Config::default(), Theme::load(false));
        app.entries = vec![
            entry(Source::Root, "/alpha", "alpha"),
            entry(Source::Root, "/zulu", "zulu"),
        ];
        app.pinned_entries.insert(app.entries[1].key().clone());

        app.apply_filter();

        assert_eq!(app.selected_entry().unwrap().title, "zulu");
    }

    #[test]
    fn marking_keeps_the_cursor_on_the_marked_row() {
        let suffix = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = env::temp_dir().join(format!("herdr-mark-{suffix}"));
        let mut app = App::with_history(Config::default(), Theme::load(false), dir.clone());
        app.entries = vec![
            entry(Source::Root, "/alpha", "alpha"),
            entry(Source::Root, "/zulu", "zulu"),
        ];
        app.apply_filter();
        app.selected = 1;

        app.toggle_selected_pin().unwrap();
        assert_eq!(app.selected, 0, "marking sorts the row to the top");
        assert_eq!(app.selected_entry().unwrap().title, "zulu");

        app.toggle_selected_pin().unwrap();
        assert_eq!(app.selected, 1, "unmarking sorts the row back down");
        assert_eq!(app.selected_entry().unwrap().title, "zulu");

        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn previous_workspace_sorts_before_marked_entries() {
        let mut app = test_app(Config::default(), Theme::load(false));
        let marked = entry(Source::Root, "/marked", "marked");
        let mut previous = entry(Source::Workspace, "/previous", "previous");
        previous.workspace_id = Some("w2".into());
        previous.action = EntryAction::FocusWorkspace { id: "w2".into() };
        app.entries = vec![marked, previous];
        app.pinned_entries.insert(app.entries[0].key().clone());
        app.previous_workspace_id = Some(
            app.entries
                .iter()
                .find(|entry| entry.workspace_id.as_deref() == Some("w2"))
                .unwrap()
                .key()
                .clone(),
        );

        app.apply_filter();

        assert_eq!(
            app.selected_entry().unwrap().workspace_id.as_deref(),
            Some("w2")
        );
    }

    #[test]
    fn source_specific_reuse_distinguishes_same_path_workspaces() {
        let mut app = test_app(Config::default(), Theme::load(false));
        app.path_to_workspaces.insert(
            reuse_key(&app, "/tmp"),
            vec![
                workspace(
                    &app.navigation,
                    "w1",
                    "project: tmp",
                    WorkspaceKind::Project,
                    "/tmp",
                ),
                workspace(
                    &app.navigation,
                    "w2",
                    "dir: tmp",
                    WorkspaceKind::Dir,
                    "/tmp",
                ),
            ],
        );

        let mut project = entry(Source::Project, "/tmp", "tmp");
        project.project = Some(Project {
            name: "tmp".into(),
            description: String::new(),
            working_dir: "/tmp".into(),
            tabs: vec![],
        });
        let dir = entry(Source::Zoxide, "/tmp", "tmp");

        assert_eq!(
            app.matching_project_workspace(&project)
                .unwrap()
                .identity
                .id(),
            "w1"
        );
        assert_eq!(
            app.matching_dir_workspace(&dir).unwrap().identity.id(),
            "w2"
        );
    }

    #[test]
    fn offers_directory_template_for_new_and_existing_directory_workspaces() {
        let mut config = Config::default();
        config.picker.directory_template = Some("default.toml".into());
        let mut app = test_app(config, Theme::load(false));
        app.entries = vec![entry(Source::Zoxide, "/tmp", "tmp")];
        app.apply_filter();

        assert_eq!(app.directory_template_for_selected(), Some("default.toml"));

        app.path_to_workspaces.insert(
            reuse_key(&app, "/tmp"),
            vec![workspace(
                &app.navigation,
                "w2",
                "dir: tmp",
                WorkspaceKind::Dir,
                "/tmp",
            )],
        );
        assert_eq!(app.directory_template_for_selected(), Some("default.toml"));

        app.config.picker.create_missing = false;
        assert_eq!(app.directory_template_for_selected(), Some("default.toml"));

        app.path_to_workspaces.insert(
            reuse_key(&app, "/tmp"),
            vec![workspace(
                &app.navigation,
                "w1",
                "project: tmp",
                WorkspaceKind::Project,
                "/tmp",
            )],
        );
        assert_eq!(
            app.matching_template_workspace_by_key(&reuse_key(&app, "/tmp"))
                .unwrap()
                .identity
                .id(),
            "w1"
        );
    }

    #[test]
    fn directory_workspaces_reuse_unprefixed_labels() {
        let mut app = test_app(Config::default(), Theme::load(false));
        let key = reuse_key(&app, "/tmp");
        // Herdr's own workspaces carry no `dir:`/`project:` prefix.
        app.path_to_workspaces.insert(
            key.clone(),
            vec![workspace(
                &app.navigation,
                "w1",
                "tmp",
                WorkspaceKind::Unknown,
                "/tmp",
            )],
        );
        let dir = entry(Source::Zoxide, "/tmp", "tmp");
        assert_eq!(
            app.matching_dir_workspace(&dir).unwrap().identity.id(),
            "w1"
        );

        // An explicit dir workspace still wins over an unprefixed one.
        app.path_to_workspaces.insert(
            key,
            vec![
                workspace(&app.navigation, "w1", "tmp", WorkspaceKind::Unknown, "/tmp"),
                workspace(
                    &app.navigation,
                    "w2",
                    "dir: tmp",
                    WorkspaceKind::Dir,
                    "/tmp",
                ),
            ],
        );
        assert_eq!(
            app.matching_dir_workspace(&dir).unwrap().identity.id(),
            "w2"
        );
    }

    #[test]
    fn project_workspaces_are_not_reused_as_directory_workspaces() {
        let mut app = test_app(Config::default(), Theme::load(false));
        app.path_to_workspaces.insert(
            reuse_key(&app, "/tmp"),
            vec![workspace(
                &app.navigation,
                "w1",
                "project: tmp",
                WorkspaceKind::Project,
                "/tmp",
            )],
        );

        let dir = entry(Source::Zoxide, "/tmp", "tmp");
        assert!(
            app.matching_dir_workspace(&dir).is_none(),
            "the `project:` prefix is what keeps the two kinds apart"
        );
    }

    #[test]
    fn new_directory_workspaces_are_labelled_with_the_plain_directory_name() {
        let (mut app, runner) = inventory_app();
        let mut route = crate::herdr::invocation();
        route.managed_plugin = true;
        app.navigation =
            NavigationContext::with_server(crate::navigation::server::OrdinaryServer::new(
                route,
                Box::new(runner.clone()),
                crate::herdr::Limits::default(),
            ));
        let scope = app.navigation.scope();
        app.inventory = Some(Inventory {
            workspaces: InventoryRead::new(scope, Some(Ok(vec![]))),
            panes: InventoryRead::new(scope, Some(Ok(vec![]))),
            tabs: InventoryRead::new(scope, Some(Ok(vec![]))),
            agents: InventoryRead::new(scope, None),
        });
        app.entries = vec![entry(Source::Root, "/tmp", "loom-proxy")];
        app.prepare_actions();
        let action = app.resolved_action(&app.entries[0], Slot::Primary);
        assert_eq!(action.description(), "Open folder /tmp");
        runner.replies.lock().unwrap().push_back(Ok(vec![]));
        assert_eq!(
            actions::dispatch(&action, &mut app.navigation).unwrap(),
            actions::Completion::ProviderCompleted
        );
        assert!(app.previous_workspace_id.is_none());
        let calls = runner.calls.lock().unwrap();
        assert_eq!(
            calls[0].args,
            [
                "workspace",
                "create",
                "--cwd",
                "/tmp",
                "--label",
                "loom-proxy",
                "--focus"
            ]
            .map(std::ffi::OsString::from)
        );
    }

    #[test]
    fn close_target_matches_entry_kind() {
        let mut app = test_app(Config::default(), Theme::load(false));
        app.path_to_workspaces.insert(
            reuse_key(&app, "/tmp"),
            vec![
                workspace(
                    &app.navigation,
                    "w1",
                    "project: tmp",
                    WorkspaceKind::Project,
                    "/tmp",
                ),
                workspace(
                    &app.navigation,
                    "w2",
                    "dir: tmp",
                    WorkspaceKind::Dir,
                    "/tmp",
                ),
            ],
        );

        let mut project = entry(Source::Project, "/tmp", "tmp");
        project.project = Some(Project {
            name: "tmp".into(),
            description: String::new(),
            working_dir: "/tmp".into(),
            tabs: vec![],
        });
        let dir = entry(Source::Root, "/tmp", "tmp");

        assert_eq!(app.workspace_to_close(&project).unwrap().id(), "w1");
        assert_eq!(app.workspace_to_close(&dir).unwrap().id(), "w2");
    }

    #[test]
    fn refuses_to_close_picker_owning_workspace() {
        let context = NavigationContext::new();
        let one = LiveIdentity::new(context.scope(), ObjectKind::Workspace, "w1").unwrap();
        let two = LiveIdentity::new(context.scope(), ObjectKind::Workspace, "w2").unwrap();
        assert_eq!(
            close_current_workspace_error(&one, Some(&one)),
            Some("can't close the workspace that owns this picker; switch away first".into())
        );
        assert_eq!(close_current_workspace_error(&one, Some(&two)), None);
        assert_eq!(
            close_current_workspace_error(&one, None),
            Some("can't close a workspace because picker ownership is unknown".into())
        );
        let foreign = NavigationContext::new();
        for id in ["w1", "w2"] {
            let owner = LiveIdentity::new(foreign.scope(), ObjectKind::Workspace, id).unwrap();
            assert_eq!(
                close_current_workspace_error(&one, Some(&owner)),
                Some("can't close a workspace because picker ownership is unverified".into())
            );
        }
    }

    #[test]
    fn workspace_rows_are_not_deduped_by_path() {
        let mut entries = Vec::new();
        let mut seen = HashSet::new();
        push_unique(
            &mut entries,
            &mut seen,
            ["w1", "w2"]
                .into_iter()
                .map(|id| {
                    let scope = crate::model::ServerScope::ephemeral(99);
                    let identity = LiveIdentity::new(&scope, ObjectKind::Workspace, id).unwrap();
                    let target =
                        crate::model::LiveTarget::new(identity.clone(), None, None).unwrap();
                    let mut row = entry(Source::Workspace, "/tmp", "same label");
                    row.workspace_id = Some(id.into());
                    row.action = EntryAction::FocusWorkspace { id: id.into() };
                    Entry::new(EntryIdentity::live(target), row.data)
                })
                .collect(),
        );

        assert_eq!(entries.len(), 2);
    }

    #[test]
    fn collision_fixture_drives_production_dedupe_search_selection_and_reuse() {
        let fixture: serde_json::Value =
            serde_json::from_str(crate::navigation_test_support::COLLISIONS).unwrap();
        let shared = &fixture["input"]["shared"];
        let id = shared["workspace_id"].as_str().unwrap();
        let cwd = shared["cwd"].as_str().unwrap();
        let mut app = App::for_test(Config::default(), Theme::load(false));
        let mut seen = HashSet::new();
        for owner in fixture["input"]["owners"].as_array().unwrap() {
            let scope = crate::model::ServerScope::reported(
                owner[0].as_str().unwrap(),
                owner[1].as_str().unwrap(),
                owner[2].as_str().unwrap(),
            )
            .unwrap();
            let context = NavigationContext::reported(scope);
            let json = serde_json::json!({"result":{"workspaces":[{"workspace_id":id, "label":shared["label"]}]}});
            let (entries, index) = crate::sources::workspaces_from_json(
                &json,
                &HashMap::from([(id.into(), cwd.into())]),
                &context,
            );
            push_unique(&mut app.entries, &mut seen, entries);
            app.path_to_workspaces.extend(index);
            app.navigation = context;
        }
        assert_eq!(app.entries.len(), 4);
        assert_eq!(app.path_to_workspaces.len(), 4);
        app.query = "/fixture/shared".into();
        app.apply_filter();
        assert_eq!(app.filtered.len(), 4);
        // Only the current ordinary scope can receive an invocation-only mark.
        let selected = app.entries[3].key().clone();
        app.pinned_entries.insert(selected.clone());
        app.entries.reverse();
        app.entries
            .iter_mut()
            .for_each(|entry| entry.title = "renamed".into());
        app.apply_filter();
        app.restore_selection(Some(&selected));
        assert_eq!(app.selected_entry().unwrap().key(), &selected);
        assert_eq!(
            app.entries
                .iter()
                .filter(|entry| app.is_pinned(entry))
                .count(),
            1
        );
        assert_eq!(
            app.entries
                .iter()
                .map(|entry| app.workspaces_for_entry(entry).len())
                .sum::<usize>(),
            1
        );
    }

    #[cfg(unix)]
    #[test]
    fn ordinary_route_quick_actions_need_no_filesystem_colocation_and_propagate_failure() {
        for exit_code in [0, 7] {
            with_recorded_herdr(exit_code, |log| {
                let mut config = Config::default();
                config.notifications.enabled = false;
                let mut app = App::for_test(config, Theme::load(false));
                let directory = log.parent().unwrap();
                app.navigation =
                    NavigationContext::with_server(crate::navigation::server::OrdinaryServer::new(
                        crate::herdr::Invocation::for_test(
                            directory.join("herdr"),
                            directory.to_path_buf(),
                            Vec::new(),
                        ),
                        Box::new(crate::herdr::ProcessRunner),
                        crate::herdr::Limits::default(),
                    ));
                assert!(app.navigation.require_process_server_colocation().is_err());
                app.entries = vec![herdr_plus::quick_actions_entry()];
                assert!(app.entries[0].path.is_none());
                app.apply_filter();
                let result = open_displayed(&mut app);
                if exit_code == 0 {
                    result.unwrap();
                } else {
                    assert!(result.unwrap_err().contains("Herdr CLI exited"));
                }
                assert_eq!(
                    fs::read_to_string(log).unwrap(),
                    "call\nplugin\naction\ninvoke\ncloudmanic.herdr-plus.quick-actions\n"
                );
            });
        }
    }

    #[cfg(unix)]
    #[test]
    fn machine_rows_cannot_dispatch_even_with_mutated_action_metadata() {
        with_recorded_herdr(0, |log| {
            let mut config = Config::default();
            config.notifications.enabled = false;
            let mut app = App::for_test(config, Theme::load(false));
            let identity = crate::model::MachineIdentity::new(
                crate::model::MachineKey::new("machine").unwrap(),
                None,
            )
            .unwrap();
            let row = Entry::machine(identity, "machine".into(), "unknown".into());
            app.entries = vec![row];
            app.apply_filter();
            for action in [
                EntryAction::UnavailableNativeWorkspace,
                EntryAction::FocusWorkspace { id: "w1".into() },
                EntryAction::FocusPane,
                EntryAction::OpenProject,
                EntryAction::FocusOrCreateDir,
                EntryAction::OpenRemote {
                    target: "forbidden".into(),
                },
                EntryAction::AttachSession {
                    name: "forbidden".into(),
                    remote: None,
                },
                EntryAction::InvokePluginAction {
                    action: "forbidden".into(),
                },
                EntryAction::RunCommand {
                    command: format!("touch '{}'", log.display()),
                    timeout_ms: 100,
                    notify_success: false,
                    notify_error: false,
                },
            ] {
                app.entries[0].action = action;
                assert!(open_displayed(&mut app)
                    .unwrap_err()
                    .contains("naming flow is unavailable"));
                assert!(app.close_selected_workspace().is_err());
            }
            assert!(!log.exists());
        });
    }

    #[cfg(unix)]
    #[test]
    fn retained_target_after_explicit_invalidation_is_rejected_before_dispatch() {
        with_recorded_herdr(0, |log| {
            let mut config = Config::default();
            config.notifications.enabled = false;
            let mut app = App::for_test(config, Theme::load(false));
            let json = serde_json::json!({"result":{"workspaces":[{"workspace_id":"w1"}]}});
            let (entries, _) =
                crate::sources::workspaces_from_json(&json, &HashMap::new(), &app.navigation);
            app.entries = entries;
            app.apply_filter();
            let retained = app.selected_entry().unwrap().key().clone();
            app.navigation.invalidate_scope();
            assert_eq!(app.selected_entry().unwrap().key(), &retained);
            assert!(open_displayed(&mut app)
                .unwrap_err()
                .contains("different server scope"));
            assert!(app.close_selected_workspace().is_err());
            assert!(!log.exists());
        });
    }

    #[cfg(unix)]
    #[test]
    fn successful_refresh_keeps_scope_without_claiming_connection_evidence() {
        with_recorded_herdr(0, |log| {
            let mut config = Config::default();
            config.notifications.enabled = false;
            config.sources.herdr_plus_projects = false;
            config.sources.herdr_plus_quick_actions = false;
            config.sources.zoxide = false;
            config.sources.roots = false;
            config.sources.servers = false;
            config.sources.sessions = false;
            config.sources.agents = false;
            let mut app = App::for_test(config, Theme::load(false));
            let scope = app.navigation.scope().clone();
            for _ in 0..2 {
                app.refresh();
                assert_eq!(app.navigation.scope(), &scope);
                assert_eq!(
                    app.navigation.connection(),
                    crate::navigation::ConnectionState::Unknown
                );
                assert_eq!(
                    app.navigation.freshness(),
                    crate::navigation::Freshness::Unknown
                );
                assert!(app.navigation.current_client_selection().is_none());
            }
            let calls = fs::read_to_string(log).unwrap();
            let mut calls: Vec<_> = calls.split("call\n").skip(1).collect();
            calls.sort_unstable();
            assert_eq!(
                calls,
                [
                    "pane\nlist\n",
                    "pane\nlist\n",
                    "tab\nlist\n",
                    "tab\nlist\n",
                    "workspace\nlist\n",
                    "workspace\nlist\n"
                ]
            );
        });
    }

    #[test]
    fn unknown_process_server_owner_blocks_create_and_template_before_any_io() {
        struct NoFiles;
        impl actions::ProcessFiles for NoFiles {
            fn read(&self, _: &Path) -> Result<String, String> {
                panic!("unowned read")
            }
            fn canonical(&self, _: &Path) -> Option<PathBuf> {
                panic!("unowned canonicalization")
            }
        }
        let mut app = App::for_test(Config::default(), Theme::load(false));
        app.navigation = NavigationContext::unmanaged();
        let mut row = entry(Source::Root, "/tmp", "test");
        for slot in [Slot::Primary, Slot::Template] {
            let prepared = Prepared::load(&row, slot, &app.config, &app.navigation, None, &NoFiles);
            assert!(prepared.error.unwrap().contains("ownership is unverified"));
        }
        row.action = EntryAction::OpenProject;
        let prepared = Prepared::load(
            &row,
            Slot::Primary,
            &app.config,
            &app.navigation,
            None,
            &NoFiles,
        );
        assert!(prepared.error.unwrap().contains("ownership is unverified"));
        row.path = Some(OwnedPath::reported(app.navigation.scope(), "/tmp").unwrap());
        let context = NavigationContext::colocated();
        row.action = EntryAction::FocusOrCreateDir;
        let prepared = Prepared::load(&row, Slot::Primary, &app.config, &context, None, &NoFiles);
        assert!(prepared
            .error
            .unwrap()
            .contains("not a process filesystem path"));
    }

    #[test]
    fn reuse_and_picker_protection_reject_other_scopes_with_identical_ids() {
        let mut app = test_app(Config::default(), Theme::load(false));
        let foreign = NavigationContext::new();
        let own = workspace(
            &app.navigation,
            "w1",
            "dir: tmp",
            WorkspaceKind::Dir,
            "/tmp",
        );
        let other = workspace(&foreign, "w1", "dir: tmp", WorkspaceKind::Dir, "/tmp");
        assert!(close_current_workspace_error(&own.identity, Some(&own.identity)).is_some());
        assert_eq!(
            close_current_workspace_error(&other.identity, Some(&own.identity)),
            Some("can't close a workspace because picker ownership is unverified".into())
        );
        app.path_to_workspaces
            .insert(reuse_key(&app, "/tmp"), vec![other]);
        let dir = entry(Source::Root, "/tmp", "tmp");
        assert!(app.matching_dir_workspace(&dir).is_none());
        assert!(app.workspace_to_close(&dir).is_none());
    }

    #[test]
    fn marks_and_jump_back_leave_legacy_state_files_untouched() {
        let _lock = crate::herdr::bin_path_lock();
        let dir = env::temp_dir().join(format!(
            "n03-state-{}",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&dir).unwrap();
        let pins = b"[\"workspace:w1\",\"unknown:opaque\"]";
        fs::write(dir.join("pinned-entries.json"), pins).unwrap();
        fs::write(dir.join("jump-back-workspace"), "w1").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&dir, fs::Permissions::from_mode(0o700)).unwrap();
            for name in ["pinned-entries.json", "jump-back-workspace"] {
                fs::set_permissions(dir.join(name), fs::Permissions::from_mode(0o600)).unwrap();
            }
        }
        let mut app = App::with_history(Config::default(), Theme::load(false), dir.clone());
        app.entries = vec![entry(Source::Root, "/tmp", "tmp")];
        app.apply_filter();
        let result = app.toggle_selected_pin();
        let jump = jump_back(&app.config);
        result.unwrap();
        assert_eq!(app.pinned_entries.len(), 1);
        assert!(jump.contains("qualified persistent history"));
        assert_eq!(fs::read(dir.join("pinned-entries.json")).unwrap(), pins);
        assert_eq!(
            fs::read_to_string(dir.join("jump-back-workspace")).unwrap(),
            "w1"
        );
        fs::remove_dir_all(dir).unwrap();
    }
    #[test]
    fn live_dispatch_rejects_wrong_scope_or_changed_workspace_action_and_panes_cannot_close() {
        let mut app = App::for_test(Config::default(), Theme::load(false));
        let foreign = NavigationContext::new();
        let json = serde_json::json!({"result":{"workspaces":[{"workspace_id":"w1"}]}});
        let (foreign_entries, _) =
            crate::sources::workspaces_from_json(&json, &HashMap::new(), &foreign);
        app.entries = foreign_entries;
        app.apply_filter();
        assert!(open_displayed(&mut app)
            .unwrap_err()
            .contains("different server scope"));
        let (own_entries, _) =
            crate::sources::workspaces_from_json(&json, &HashMap::new(), &app.navigation);
        app.entries = own_entries;
        app.entries[0].action = EntryAction::FocusWorkspace { id: "w2".into() };
        app.apply_filter();
        assert!(open_displayed(&mut app)
            .unwrap_err()
            .contains("does not match"));
        let mut panes = crate::sources::test_inventory_entries(&app.navigation);
        assert!(app.workspace_to_close(&panes[0]).is_none());
        panes[0].source = Source::Workspace;
        assert!(app.workspace_to_close(&panes[0]).is_none());
    }

    #[test]
    fn dedupe_merges_provenance_only_for_identical_qualified_identity() {
        let one = entry(Source::Root, "/tmp", "same");
        let mut duplicate = one.clone();
        duplicate
            .provenance
            .insert(ProviderId::new("another-source").unwrap());
        let different = entry(Source::Root, "/different", "same");
        let mut entries = Vec::new();
        push_unique(
            &mut entries,
            &mut HashSet::new(),
            vec![one, duplicate, different],
        );
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].provenance.len(), 2);
        assert_eq!(entries[1].provenance.len(), 1);
    }
    #[cfg(unix)]
    #[test]
    fn collector_failure_rotates_scope_before_conversion_and_cannot_regrant_colocation() {
        use crate::herdr::{Limits, RecordingRunner};
        use crate::navigation::server::OrdinaryServer;
        use std::sync::Arc;
        let runner = Arc::new(RecordingRunner::default());
        let workspace = crate::navigation_test_support::WORKSPACE_LIST
            .as_bytes()
            .to_vec();
        let panes = br#"{"id":"cli","result":{"type":"pane_list","panes":[]}}"#.to_vec();
        let tabs = br#"{"id":"cli","result":{"type":"tab_list","tabs":[]}}"#.to_vec();
        runner.replies.lock().unwrap().extend([
            Ok(workspace.clone()),
            Ok(panes.clone()),
            Ok(tabs.clone()),
            Ok(workspace.clone()),
            Err(TransportError::new(
                Failure::Timeout,
                MutationOutcome::Unknown,
            )),
            Ok(tabs.clone()),
            Ok(workspace),
            Ok(panes),
            Ok(tabs),
        ]);
        let mut route = crate::herdr::invocation();
        // Model the documented unchanged managed-plugin invocation contract.
        route.managed_plugin = true;
        let mut config = Config::default();
        config.sources.herdr_plus_projects = false;
        config.sources.herdr_plus_quick_actions = false;
        config.sources.zoxide = false;
        config.sources.roots = false;
        config.sources.servers = false;
        config.sources.sessions = false;
        config.sources.agents = false;
        let mut app = App::for_test(config, Theme::load(false));
        app.navigation = NavigationContext::with_server(OrdinaryServer::new(
            route,
            Box::new(runner.clone()),
            Limits::default(),
        ));
        let scope = app.navigation.scope().clone();
        app.refresh();
        assert_eq!(app.navigation.scope(), &scope);
        assert_eq!(app.entries.len(), 2);
        app.navigation.require_process_server_colocation().unwrap();
        let retained = app.entries[0].clone();
        app.refresh();
        assert_ne!(app.navigation.scope(), &scope);
        assert!(
            app.entries.is_empty(),
            "old workspace response must not receive the new scope"
        );
        assert!(app.path_to_workspaces.is_empty());
        assert!(app.navigation.require_process_server_colocation().is_err());
        let new_scope = app.navigation.scope().clone();
        app.refresh();
        assert_eq!(app.navigation.scope(), &new_scope);
        assert_eq!(app.entries.len(), 2);
        assert_ne!(app.entries[0].key(), retained.key());
        assert!(app.navigation.require_process_server_colocation().is_err());
        assert_eq!(runner.calls.lock().unwrap().len(), 9);
    }

    #[test]
    fn n09_provider_types_provenance_and_resolved_actions_use_the_app_snapshot() {
        use crate::picker::{MachineFilter, PickerState, TypeFilter};
        let mut app = test_app(Config::default(), Theme::load(false));
        let mut project = entry(Source::Project, "/fixture/project", "Project");
        project.action = EntryAction::OpenProject;
        let folder = entry(Source::Zoxide, "/fixture/folder", "Folder");
        let mut remote = Entry::new(
            EntryIdentity::provider("configured-remotes", "remote", "attach").unwrap(),
            entry(Source::Server, "/fixture/remote", "Attach remote").data,
        );
        remote.action = EntryAction::OpenRemote {
            target: "fixture.invalid".into(),
        };
        let mut run = Entry::new(
            EntryIdentity::command("commands", "remote-task", "run").unwrap(),
            entry(Source::Server, "/fixture/run", "Run server task").data,
        );
        run.action = EntryAction::RunCommand {
            command: "blocked-offline-command".into(),
            timeout_ms: 20,
            notify_success: false,
            notify_error: false,
        };
        let mut session = Entry::new(
            EntryIdentity::provider("sessions", "session", "attach").unwrap(),
            entry(Source::Session, "/fixture/session", "Attach session").data,
        );
        session.action = EntryAction::AttachSession {
            name: "fixture".into(),
            remote: None,
        };
        let expected = [
            (TypeFilter::Projects, vec![project.key().clone()]),
            (TypeFilter::Folders, vec![folder.key().clone()]),
            (
                TypeFilter::Sessions,
                vec![remote.key().clone(), session.key().clone()],
            ),
            (TypeFilter::Actions, vec![run.key().clone()]),
        ];
        let remote_id = remote.key().clone();
        let run_id = run.key().clone();
        app.entries = vec![project, folder, remote, run, session];
        let input = app.picker_input();
        let mut state = PickerState::new(&input);
        for (kind, expected) in expected {
            state.set_type(kind);
            let projection = app.picker_projection(&input, &state);
            assert!(projection.flat);
            assert_eq!(
                projection
                    .rows
                    .iter()
                    .map(|r| r.destination.unwrap().entry.key().clone())
                    .collect::<HashSet<_>>(),
                expected.into_iter().collect()
            );
            for row in projection.rows {
                assert_eq!(
                    row.action(),
                    Some(&app.resolved_action(&row.destination.unwrap().entry, Slot::Primary))
                );
            }
        }
        state.set_type(TypeFilter::Machines);
        assert!(app.picker_projection(&input, &state).rows.is_empty());
        state.set_type(TypeFilter::Sessions);
        state.set_provenance(Some(Source::Server));
        assert_eq!(
            app.picker_projection(&input, &state).rows[0].id,
            crate::picker::RowId::Destination(remote_id)
        );
        state.set_type(TypeFilter::Actions);
        assert_eq!(
            app.picker_projection(&input, &state).rows[0].id,
            crate::picker::RowId::Destination(run_id.clone())
        );
        state.set_provenance(Some(Source::Integration));
        assert_eq!(
            app.picker_projection(&input, &state).rows[0].id,
            crate::picker::RowId::Destination(run_id)
        );
        state.set_machine(MachineFilter::Machine(
            crate::model::MachineKey::new("local").unwrap(),
        ));
        assert!(app.picker_projection(&input, &state).rows.is_empty());
        assert_eq!(
            app.config.picker.filter_source_for_key('s'),
            Some(Source::Server)
        );
    }

    #[test]
    fn picker_task_groups_have_no_actions_and_provider_sources_do_not_invent_machines() {
        use crate::picker::{self, Owner, PickerState, Request, RowId, TaskKind};
        for colocated in [false, true] {
            let (mut app, runner) = inventory_app();
            if colocated {
                app.navigation = NavigationContext::colocated();
            }
            let mut run = entry(Source::Server, "run", "server-kind command");
            run.action = EntryAction::RunCommand {
                command: "never executed".into(),
                timeout_ms: 10,
                notify_success: false,
                notify_error: false,
            };
            let mut attach = entry(Source::Server, "attach", "remote attachment");
            attach.action = EntryAction::OpenRemote {
                target: "fixture-host".into(),
            };
            app.entries = vec![
                entry(Source::Root, "/missing-fixture-folder", "folder"),
                attach,
                run,
            ];
            let input = app.picker_input();
            let expected_owner = if colocated {
                Owner::Server(app.navigation.scope().clone())
            } else {
                Owner::Process(crate::navigation::ProcessHostRef)
            };
            assert!(input.destinations.iter().all(|d| d.owner == expected_owner));
            assert!(input
                .headings
                .iter()
                .all(|h| h.machine.is_none() && !h.local));
            let eligible = input.eligible_ids();
            let mut state = PickerState::new(&input);
            let tree = picker::project(
                &input,
                &state,
                Request {
                    query: "",
                    non_live_type: false,
                    eligible: &eligible,
                },
            );
            let groups: Vec<_> = tree
                .rows
                .iter()
                .filter(|r| matches!(r.id, RowId::TaskGroup(..)))
                .collect();
            assert_eq!(groups.len(), 3);
            assert_eq!(
                groups.iter().map(|r| r.label).collect::<Vec<_>>(),
                ["Projects and folders", "Sessions", "Actions"]
            );
            assert!(groups
                .iter()
                .all(|r| r.disclosure && r.destination.is_none() && r.action().is_none()));
            assert!(tree.rows.iter().all(|r| r.destination.is_none()));
            state.toggle(RowId::TaskGroup(expected_owner.clone(), TaskKind::Actions));
            let expanded = picker::project(
                &input,
                &state,
                Request {
                    query: "",
                    non_live_type: false,
                    eligible: &eligible,
                },
            );
            let command = expanded.rows.iter().find_map(|r| r.destination).unwrap();
            assert_eq!(
                command.entry.destination_kind(),
                crate::model::DestinationKind::Action
            );
            assert!(matches!(
                command.action.payload,
                actions::Payload::Run { .. }
            ));
            let flat = picker::project(
                &input,
                &state,
                Request {
                    query: "",
                    non_live_type: true,
                    eligible: &eligible,
                },
            );
            assert!(flat.flat);
            assert_eq!(flat.rows.len(), 3);
            assert!(flat
                .rows
                .iter()
                .all(|r| r.depth == 0 && r.destination.is_some() && !r.disclosure));
            assert!(runner.calls.lock().unwrap().is_empty());
        }
    }

    #[test]
    fn picker_supported_empty_task_group_is_explicit_not_inferred_from_an_empty_vector() {
        use crate::picker::{self, PickerState, Request, RowId};
        let (mut app, _) = inventory_app();
        let input = app.picker_input();
        assert!(input.groups.is_empty());
        assert!(picker::project(
            &input,
            &PickerState::new(&input),
            Request {
                query: "",
                non_live_type: false,
                eligible: &[]
            }
        )
        .rows
        .is_empty());
        app.config.sources.roots = true;
        let input = app.picker_input();
        let tree = picker::project(
            &input,
            &PickerState::new(&input),
            Request {
                query: "",
                non_live_type: false,
                eligible: &[],
            },
        );
        assert_eq!(tree.rows.len(), 2);
        assert!(matches!(tree.rows[1].id, RowId::TaskGroup(..)));
        assert_eq!(tree.rows[1].label, "Projects and folders");
        assert!(tree.rows[1].action().is_none());
    }

    fn picker_action_owner_integration(enabled: bool) -> crate::config::IntegrationConfig {
        crate::config::IntegrationConfig {
            id: "owner-fixture".into(),
            label: "Owner fixture".into(),
            enabled,
            collect: "exit 125".into(),
            open: "exit 125".into(),
            collect_timeout_ms: 2_000,
            open_timeout_ms: 2_000,
            max_output_bytes: 4_096,
            notify_success: false,
            notify_error: false,
        }
    }

    fn picker_action_owner_app(
        colocated: bool,
    ) -> (App, std::sync::Arc<crate::herdr::RecordingRunner>) {
        let (mut app, runner) = inventory_app();
        app.config.sources.agents = false;
        let mut route = crate::herdr::invocation();
        route.managed_plugin = colocated;
        route.socket = None;
        app.navigation =
            NavigationContext::with_server(crate::navigation::server::OrdinaryServer::new(
                route,
                Box::new(runner.clone()),
                crate::herdr::Limits::default(),
            ));
        (app, runner)
    }

    #[test]
    fn picker_action_owner_supported_empty_combinations_and_colocation() {
        use crate::picker::{Owner, PickerState, RowId, TaskKind};
        for colocated in [false, true] {
            for quick in [false, true] {
                for command in [false, true] {
                    let (mut app, runner) = picker_action_owner_app(colocated);
                    app.config.sources.herdr_plus_quick_actions = quick;
                    app.config.sources.roots = true;
                    app.config.sources.sessions = true;
                    app.config.integrations = vec![
                        picker_action_owner_integration(command),
                        picker_action_owner_integration(false),
                    ];
                    let server = Owner::Server(app.navigation.scope().clone());
                    let process = if colocated {
                        server.clone()
                    } else {
                        Owner::Process(crate::navigation::ProcessHostRef)
                    };
                    let input = app.picker_input();
                    let mut expected = vec![
                        (process.clone(), TaskKind::ProjectsAndFolders),
                        (process.clone(), TaskKind::Sessions),
                    ];
                    if quick {
                        expected.push((server.clone(), TaskKind::Actions));
                    }
                    if command && !(colocated && quick) {
                        expected.push((process.clone(), TaskKind::Actions));
                    }
                    assert_eq!(
                        input.groups, expected,
                        "colocated={colocated}, quick={quick}, command={command}"
                    );
                    assert!(input.destinations.is_empty());
                    let projection = app.picker_projection(&input, &PickerState::new(&input));
                    assert_eq!(
                        projection
                            .rows
                            .iter()
                            .filter(|r| matches!(r.id, RowId::TaskGroup(..)))
                            .map(|r| r.id.clone())
                            .collect::<HashSet<_>>(),
                        expected
                            .into_iter()
                            .map(|(owner, kind)| RowId::TaskGroup(owner, kind))
                            .collect()
                    );
                    assert!(projection.rows.iter().all(|r| r.action().is_none()));
                    assert!(input
                        .headings
                        .iter()
                        .all(|h| h.machine.is_none() && !h.local));
                    assert!(runner.calls.lock().unwrap().is_empty());
                }
            }
        }
    }

    #[test]
    fn picker_action_owner_unavailable_actions_keep_known_owners_without_relocating_folders() {
        use crate::picker::{Owner, PickerState, RowId, TaskKind};
        for colocated in [false, true] {
            let (mut app, runner) = picker_action_owner_app(colocated);
            let mut quick = herdr_plus::quick_actions_entry();
            quick.action_conflict = Some("fixture plugin conflict".into());
            let mut run = entry(Source::Server, "run", "server-kind command");
            run.action = EntryAction::RunCommand {
                command: "exit 125".into(),
                timeout_ms: 20,
                notify_success: false,
                notify_error: false,
            };
            run.action_conflict = Some("fixture command conflict".into());
            let mut attach = entry(Source::Server, "attach", "remote attachment");
            attach.action = EntryAction::OpenRemote {
                target: "fixture.invalid".into(),
            };
            let mut project = entry(Source::Project, "/missing-fixture-project", "project");
            project.action = EntryAction::OpenProject;
            app.entries = vec![
                quick,
                run,
                entry(Source::Root, "/missing-fixture-folder", "folder"),
                project,
                attach,
            ];
            app.prepare_actions();
            let input = app.picker_input();
            let server = Owner::Server(app.navigation.scope().clone());
            let process = if colocated {
                server.clone()
            } else {
                Owner::Process(crate::navigation::ProcessHostRef)
            };
            let mut state = PickerState::new(&input);
            for owner in [&server, &process] {
                state
                    .expanded
                    .insert(RowId::TaskGroup(owner.clone(), TaskKind::Actions));
            }
            let projection = app.picker_projection(&input, &state);
            for (index, destination) in input.destinations.iter().enumerate() {
                assert_eq!(
                    destination.owner,
                    if index == 0 {
                        server.clone()
                    } else {
                        process.clone()
                    }
                );
                assert_eq!(
                    destination.action,
                    app.resolved_action(&destination.entry, Slot::Primary)
                );
                if index < 4 {
                    assert!(destination.action.unavailable.is_some());
                    assert_eq!(destination.action.payload, actions::Payload::Unavailable);
                } else {
                    assert!(matches!(
                        destination.action.payload,
                        actions::Payload::Attach { .. }
                    ));
                    assert_eq!(
                        destination.action.owner,
                        actions::Owner::Process(crate::navigation::ProcessHostRef)
                    );
                }
                if index == 0 || index == 2 || index == 3 {
                    assert_eq!(
                        destination.action.owner,
                        actions::Owner::Server(app.navigation.scope().clone())
                    );
                }
                if index < 2 {
                    let row = projection
                        .rows
                        .iter()
                        .find(|r| r.id == RowId::Destination(destination.entry.key().clone()))
                        .unwrap();
                    assert_eq!(row.action(), Some(&destination.action));
                    assert_eq!(
                        row.ancestors,
                        vec![
                            RowId::Heading(destination.owner.clone()),
                            RowId::TaskGroup(destination.owner.clone(), TaskKind::Actions)
                        ]
                    );
                }
            }
            assert!(input
                .headings
                .iter()
                .all(|h| h.machine.is_none() && !h.local));
            assert!(runner.calls.lock().unwrap().is_empty());
        }
    }

    fn inventory_app() -> (App, std::sync::Arc<crate::herdr::RecordingRunner>) {
        let mut config = Config::default();
        config.notifications.enabled = false;
        config.sources.herdr_plus_projects = false;
        config.sources.herdr_plus_quick_actions = false;
        config.sources.zoxide = false;
        config.sources.roots = false;
        config.sources.servers = false;
        config.sources.sessions = false;
        let mut app = App::for_test(config, Theme::load(false));
        let runner = std::sync::Arc::new(crate::herdr::RecordingRunner::default());
        let directory = &app.test_history.as_ref().unwrap().0;
        app.navigation =
            NavigationContext::with_server(crate::navigation::server::OrdinaryServer::new(
                crate::herdr::Invocation::for_test(
                    directory.join("herdr"),
                    directory.clone(),
                    Vec::new(),
                ),
                Box::new(runner.clone()),
                crate::herdr::Limits::default(),
            ));
        (app, runner)
    }
    fn inventory_replies() -> [serde_json::Value; 4] {
        let mut lists = crate::navigation_test_support::ordinary_lists();
        let mut ordinary = lists[1]["result"]["panes"][0].clone();
        ordinary["pane_id"] = "opaque-ordinary-pane".into();
        ordinary["terminal_id"] = "not-a-focus-id".into();
        ordinary["agent"] = serde_json::Value::Null;
        ordinary["label"] = serde_json::Value::Null;
        lists[1]["result"]["panes"]
            .as_array_mut()
            .unwrap()
            .push(ordinary);
        lists
    }
    #[test]
    fn pane_and_agent_toggles_project_one_destination_and_workspace_visibility_keeps_ancestry() {
        for all_panes in [false, true] {
            for agents in [false, true] {
                for workspaces in [false, true] {
                    let (mut app, runner) = inventory_app();
                    app.config.sources.panes = all_panes;
                    app.config.sources.agents = agents;
                    app.config.sources.open_workspaces = workspaces;
                    let lists = inventory_replies();
                    runner.replies.lock().unwrap().extend(
                        lists
                            .into_iter()
                            .take(if agents { 4 } else { 3 })
                            .map(|list| Ok(serde_json::to_vec(&list).unwrap())),
                    );
                    let scope = app.navigation.scope().clone();
                    app.refresh();
                    assert_eq!(app.navigation.scope(), &scope);
                    let panes: Vec<_> = app
                        .entries
                        .iter()
                        .filter(|e| e.destination_kind() == crate::model::DestinationKind::Pane)
                        .collect();
                    assert_eq!(panes.len(), if all_panes { 2 } else { usize::from(agents) });
                    assert_eq!(
                        app.entries.len(),
                        panes.len() + if workspaces { 2 } else { 0 }
                    );
                    assert_eq!(app.joined_inventory.workspaces.len(), 2);
                    assert!(!app.path_to_workspaces.is_empty());
                    assert_eq!(
                        app.entries
                            .iter()
                            .map(Entry::key)
                            .collect::<HashSet<_>>()
                            .len(),
                        app.entries.len()
                    );
                    for pane in panes {
                        assert_eq!(pane.target().unwrap().workspace().unwrap().id(), "w43");
                        assert_eq!(pane.target().unwrap().tab().unwrap().id(), "w43:t1");
                        assert!(app.workspace_to_close(pane).is_none());
                        assert_eq!(
                            pane.source,
                            if pane.agent.is_some() {
                                Source::Agent
                            } else {
                                Source::Workspace
                            }
                        );
                        assert_eq!(
                            pane.provenance.len(),
                            if pane.agent.is_some() { 2 } else { 1 }
                        );
                        if !agents {
                            assert!(pane.agent.is_none());
                        }
                    }
                    assert_eq!(
                        runner.calls.lock().unwrap().len(),
                        if agents { 4 } else { 3 }
                    );
                }
            }
        }
    }

    #[test]
    fn refresh_keeps_selection_and_metadata_queries_under_manual_title_and_agent_removal() {
        let (mut app, runner) = inventory_app();
        app.config.sources.panes = true;
        app.config.sources.open_workspaces = false;
        app.config.agent_aliases = vec![crate::config::AgentAliasConfig {
            alias: "reviewer".into(),
            agent: Some("claude".into()),
            workspace: None,
            path: None,
        }];
        let mut lists = inventory_replies();
        runner.replies.lock().unwrap().extend(
            lists
                .clone()
                .map(|list| Ok(serde_json::to_vec(&list).unwrap())),
        );
        app.refresh();
        app.query = "!claude #working".into();
        app.apply_filter();
        let selected = app.selected_entry().unwrap();
        assert_eq!(selected.title, "Review key handling");
        assert!(Query::parse("!reviewer").filters_match(selected));
        assert!(selected.search_fields().contains(&"58f4-session".into()));
        let key = selected.key().clone();
        app.query.clear();
        app.apply_filter();
        app.restore_selection(Some(&key));
        lists[1]["result"]["panes"][0]["label"] = "New manual title".into();
        lists[3]["result"]["agents"][0]["agent_status"] = "blocked".into();
        lists[3]["result"]["agents"][0]["name"] = "new name".into();
        lists[3]["result"]["agents"][0]["revision"] = 9.into();
        runner.replies.lock().unwrap().extend(
            lists
                .clone()
                .map(|list| Ok(serde_json::to_vec(&list).unwrap())),
        );
        app.refresh();
        assert_eq!(app.selected_entry().unwrap().key(), &key);
        assert_eq!(app.selected_entry().unwrap().title, "New manual title");
        assert!(Query::parse("!new #blocked").filters_match(app.selected_entry().unwrap()));
        lists[3]["result"]["agents"] = serde_json::json!([]);
        runner
            .replies
            .lock()
            .unwrap()
            .extend(lists.map(|list| Ok(serde_json::to_vec(&list).unwrap())));
        app.refresh();
        assert_eq!(app.selected_entry().unwrap().key(), &key);
        assert!(app.selected_entry().unwrap().agent.is_none());
        assert_eq!(app.selected_entry().unwrap().source, Source::Workspace);
        assert!(!Query::parse("@").filters_match(app.selected_entry().unwrap()));
        assert!(app
            .workspace_to_close(app.selected_entry().unwrap())
            .is_none());
    }

    #[test]
    fn optional_structured_agent_rejection_keeps_panes_but_uncertainty_discards_all_old_reads() {
        use crate::navigation::inventory::ReadState;
        for structured in [true, false] {
            let (mut app, runner) = inventory_app();
            app.config.sources.panes = true;
            app.config.sources.open_workspaces = false;
            runner.replies.lock().unwrap().extend(
                inventory_replies()
                    .into_iter()
                    .take(3)
                    .map(|list| Ok(serde_json::to_vec(&list).unwrap())),
            );
            runner.replies.lock().unwrap().push_back(if structured {
                Ok(br#"{"id":"agent-rejection","error":{"code":"unsupported_method","message":"optional agent list rejected"}}"#.to_vec())
            } else { Err(TransportError::new(Failure::Timeout, MutationOutcome::Unknown)) });
            let scope = app.navigation.scope().clone();
            app.refresh();
            let inventory = app.inventory.as_ref().unwrap();
            assert_eq!(inventory.agents.scope, scope);
            assert!(matches!(inventory.agents.state, ReadState::Unavailable(_)));
            assert_eq!(inventory.agents.count(), None);
            if structured {
                assert_eq!(app.navigation.scope(), &scope);
                assert_eq!(app.entries.len(), 2);
                assert_eq!(inventory.panes.count(), Some(2));
                assert!(app.entries.iter().all(|e| e.agent.is_none()
                    && e.pane.as_ref().unwrap().agent_freshness
                        == crate::model::AgentFreshness::Unknown));
            } else {
                assert_ne!(app.navigation.scope(), &scope);
                assert!(app.entries.is_empty());
                assert_eq!(inventory.panes.count(), None);
                assert!(matches!(inventory.panes.state, ReadState::Stale));
                assert_eq!(inventory.panes.scope, scope);
            }
        }
    }

    #[test]
    fn independent_required_inventory_rejection_never_invents_ancestry_or_empty_counts() {
        for rejected in 0..3 {
            let (mut app, runner) = inventory_app();
            app.config.sources.panes = true;
            let mut lists = inventory_replies();
            lists[rejected] = serde_json::json!({"id":"rejected","error":{"code":"unsupported_method","message":"inventory unavailable"}});
            runner
                .replies
                .lock()
                .unwrap()
                .extend(lists.map(|list| Ok(serde_json::to_vec(&list).unwrap())));
            let scope = app.navigation.scope().clone();
            app.refresh();
            assert_eq!(app.navigation.scope(), &scope);
            assert!(app.joined_inventory.panes.is_empty());
            assert!(app
                .entries
                .iter()
                .all(|e| e.destination_kind() == crate::model::DestinationKind::Workspace));
            let inventory = app.inventory.as_ref().unwrap();
            assert_eq!(
                inventory.workspaces.count(),
                if rejected == 0 { None } else { Some(2) }
            );
            assert_eq!(
                inventory.panes.count(),
                if rejected == 1 { None } else { Some(2) }
            );
            assert_eq!(
                inventory.tabs.count(),
                if rejected == 2 { None } else { Some(1) }
            );
        }
    }

    #[test]
    fn removed_pane_and_wrong_action_cannot_dispatch_from_a_retained_row() {
        let (mut app, runner) = inventory_app();
        app.config.sources.panes = true;
        app.config.sources.open_workspaces = false;
        let mut lists = crate::navigation_test_support::ordinary_lists();
        runner.replies.lock().unwrap().extend(
            lists
                .clone()
                .map(|list| Ok(serde_json::to_vec(&list).unwrap())),
        );
        app.refresh();
        let retained = app.entries[0].clone();
        app.entries[0].action = EntryAction::FocusOrCreateDir;
        assert!(open_displayed(&mut app)
            .unwrap_err()
            .contains("does not match"));
        lists[1]["result"]["panes"] = serde_json::json!([]);
        runner
            .replies
            .lock()
            .unwrap()
            .extend(lists.map(|list| Ok(serde_json::to_vec(&list).unwrap())));
        app.refresh();
        assert!(app.entries.is_empty());
        app.entries = vec![retained];
        app.apply_filter();
        assert!(open_displayed(&mut app)
            .unwrap_err()
            .contains("inventory is unavailable"));
        assert!(app.close_selected_workspace().is_err());
        assert_eq!(runner.calls.lock().unwrap().len(), 8);
    }

    #[cfg(unix)]
    #[test]
    fn collected_panes_focus_exact_pane_ids_in_app_even_when_agents_fail_and_never_close() {
        use crate::navigation::server::tests::{snapshot, Reply, Socket};
        for optional_failure in [false, true] {
            let socket = Socket::new(vec![
                Reply::Result(snapshot()),
                Reply::Result(serde_json::json!({"type":"ok"})),
            ]);
            let (mut app, runner) = inventory_app();
            app.config.sources.panes = true;
            app.config.sources.open_workspaces = false;
            let mut route = crate::herdr::invocation();
            route.socket = Some(socket.path.clone());
            app.navigation =
                NavigationContext::with_server(crate::navigation::server::OrdinaryServer::new(
                    route,
                    Box::new(runner.clone()),
                    crate::herdr::Limits::default(),
                ));
            let mut lists = crate::navigation_test_support::ordinary_lists();
            if optional_failure {
                lists[3] = serde_json::json!({"id":"optional","error":{"code":"unsupported_method","message":"rejected"}});
            }
            runner
                .replies
                .lock()
                .unwrap()
                .extend(lists.map(|list| Ok(serde_json::to_vec(&list).unwrap())));
            runner.replies.lock().unwrap().extend(
                crate::navigation_test_support::ordinary_lists()
                    .into_iter()
                    .take(3)
                    .map(|list| Ok(serde_json::to_vec(&list).unwrap())),
            );
            app.refresh();
            assert_eq!(app.entries.len(), 1);
            assert!(app.close_selected_workspace().is_err());
            assert!(socket.requests.lock().unwrap().is_empty());
            open_displayed(&mut app).unwrap();
            let requests = socket.requests.lock().unwrap();
            assert_eq!(requests.len(), 2);
            assert_eq!(requests[1]["method"], "pane.focus");
            assert_eq!(
                requests[1]["params"],
                serde_json::json!({"pane_id":"w43:p1"})
            );
            assert_ne!(requests[1]["params"]["pane_id"], "term_1");
            assert_eq!(runner.calls.lock().unwrap().len(), 7);
            assert!(app.previous_workspace_id.is_none());
        }
    }

    #[cfg(unix)]
    #[test]
    fn workspace_row_uses_raw_focus_and_failure_invalidates_retained_target() {
        use crate::navigation::server::tests::{snapshot, Reply, Socket};
        let socket = Socket::new(vec![Reply::Result(snapshot()), Reply::WrongId]);
        let (mut app, runner) = inventory_app();
        let mut route = crate::herdr::invocation();
        route.socket = Some(socket.path.clone());
        app.navigation =
            NavigationContext::with_server(crate::navigation::server::OrdinaryServer::new(
                route,
                Box::new(runner.clone()),
                crate::herdr::Limits::default(),
            ));
        let scope = app.navigation.scope().clone();
        let lists = crate::navigation_test_support::ordinary_lists();
        runner.replies.lock().unwrap().extend(
            lists
                .clone()
                .map(|list| Ok(serde_json::to_vec(&list).unwrap())),
        );
        runner.replies.lock().unwrap().extend(
            lists
                .into_iter()
                .take(3)
                .map(|list| Ok(serde_json::to_vec(&list).unwrap())),
        );
        app.refresh();
        let index = app
            .entries
            .iter()
            .position(|e| {
                e.workspace_id.as_deref() == Some("w43")
                    && e.destination_kind() == crate::model::DestinationKind::Workspace
            })
            .unwrap();
        app.restore_selection(Some(&app.entries[index].key().clone()));
        assert!(open_displayed(&mut app).unwrap_err().contains("Unknown"));
        assert_ne!(app.navigation.scope(), &scope);
        assert!(open_displayed(&mut app)
            .unwrap_err()
            .contains("different server scope"));
        let requests = socket.requests.lock().unwrap();
        assert_eq!(requests.len(), 2);
        assert_eq!(requests[1]["method"], "workspace.focus");
        assert_eq!(
            requests[1]["params"],
            serde_json::json!({"workspace_id":"w43"})
        );
        assert!(app.previous_workspace_id.is_none());
    }
    fn action_lists(workspace: Option<(&str, &str)>) -> [serde_json::Value; 4] {
        let mut lists = crate::navigation_test_support::ordinary_lists();
        match workspace {
            Some((id, label)) => {
                lists[0]["result"]["workspaces"]
                    .as_array_mut()
                    .unwrap()
                    .truncate(1);
                lists[0]["result"]["workspaces"][0]["workspace_id"] = id.into();
                lists[0]["result"]["workspaces"][0]["label"] = label.into();
                lists[0]["result"]["workspaces"][0]["active_tab_id"] = format!("{id}:t1").into();
                lists[1]["result"]["panes"][0]["workspace_id"] = id.into();
                lists[1]["result"]["panes"][0]["pane_id"] = format!("{id}:p1").into();
                lists[1]["result"]["panes"][0]["tab_id"] = format!("{id}:t1").into();
                lists[1]["result"]["panes"][0]["cwd"] = "/tmp".into();
                lists[1]["result"]["panes"][0]["foreground_cwd"] = serde_json::Value::Null;
                lists[2]["result"]["tabs"][0]["workspace_id"] = id.into();
                lists[2]["result"]["tabs"][0]["tab_id"] = format!("{id}:t1").into();
            }
            None => {
                lists[0]["result"]["workspaces"] = serde_json::json!([]);
                lists[1]["result"]["panes"] = serde_json::json!([]);
                lists[2]["result"]["tabs"] = serde_json::json!([]);
            }
        }
        lists[3]["result"]["agents"] = serde_json::json!([]);
        lists
    }

    fn queue_action_lists(runner: &crate::herdr::RecordingRunner, lists: &[serde_json::Value; 4]) {
        runner.replies.lock().unwrap().extend(
            lists
                .iter()
                .take(3)
                .map(|list| Ok(serde_json::to_vec(list).unwrap())),
        );
    }

    fn action_app(
        lists: &[serde_json::Value; 4],
        socket: Option<PathBuf>,
    ) -> (App, std::sync::Arc<crate::herdr::RecordingRunner>) {
        let (mut app, runner) = inventory_app();
        app.config.sources.agents = false;
        let mut route = crate::herdr::invocation();
        route.managed_plugin = true;
        route.socket = socket.or_else(|| Some("/synthetic/n06-unused.sock".into()));
        app.navigation =
            NavigationContext::with_server(crate::navigation::server::OrdinaryServer::new(
                route,
                Box::new(runner.clone()),
                crate::herdr::Limits::default(),
            ));
        queue_action_lists(&runner, lists);
        app.refresh();
        (app, runner)
    }

    fn select_directory(app: &mut App) {
        app.entries = vec![entry(Source::Root, "/tmp", "tmp")];
        app.prepare_actions();
        app.apply_filter();
        app.commit_displayed_actions();
    }

    #[cfg(unix)]
    #[test]
    fn displayed_reuse_changes_require_a_new_draw_and_a_new_activation() {
        use crate::navigation::server::tests::{snapshot, Reply, Socket};
        for (before, after) in [
            (Some(("w1", "dir: tmp")), None),
            (None, Some(("w1", "dir: tmp"))),
            (Some(("w1", "dir: tmp")), Some(("w2", "dir: tmp"))),
        ] {
            let socket = after.map(|_| {
                Socket::new(vec![
                    Reply::Result(snapshot()),
                    Reply::Result(serde_json::json!({"type":"ok"})),
                ])
            });
            let (mut app, runner) = action_app(
                &action_lists(before),
                socket.as_ref().map(|s| s.path.clone()),
            );
            select_directory(&mut app);
            let token = app.displayed_actions[0].clone();
            assert!(token.action.description().starts_with(if before.is_some() {
                "Focus workspace"
            } else {
                "Open folder"
            }));
            queue_action_lists(&runner, &action_lists(after));
            assert!(app
                .open_selected(false)
                .unwrap_err()
                .contains("action changed"));
            assert_eq!(runner.calls.lock().unwrap().len(), 6);
            assert_eq!(app.displayed_actions[0].revision, token.revision);
            assert!(app
                .open_selected(false)
                .unwrap_err()
                .contains("action changed"));
            assert_eq!(runner.calls.lock().unwrap().len(), 6);
            app.commit_displayed_actions();
            assert!(app.displayed_actions[0].revision > token.revision);
            queue_action_lists(&runner, &action_lists(after));
            if after.is_none() {
                runner.replies.lock().unwrap().push_back(Ok(vec![]));
            }
            app.open_selected(false).unwrap();
            if let Some(socket) = socket {
                let requests = socket.requests.lock().unwrap();
                assert_eq!(requests[1]["method"], "workspace.focus");
                assert_eq!(requests[1]["params"]["workspace_id"], after.unwrap().0);
            } else {
                let calls = runner.calls.lock().unwrap();
                assert_eq!(calls.last().unwrap().args[0], "workspace");
                assert_eq!(calls.last().unwrap().args[1], "create");
            }
        }
    }

    #[test]
    fn unavailable_inventory_and_missing_cwd_never_resolve_as_no_match() {
        for missing in ["workspace", "pane", "cwd"] {
            let mut lists = action_lists(Some(("w1", "dir: tmp")));
            match missing {
                "workspace" => {
                    lists[0] = serde_json::json!({"id":"x","error":{"code":"unsupported_method","message":"no"}})
                }
                "pane" => {
                    lists[1] = serde_json::json!({"id":"x","error":{"code":"unsupported_method","message":"no"}})
                }
                _ => lists[1]["result"]["panes"][0]["cwd"] = serde_json::Value::Null,
            }
            let (mut app, runner) = action_app(&lists, None);
            select_directory(&mut app);
            assert!(app.displayed_actions[0].action.unavailable.is_some());
            assert!(app.open_selected(false).is_err());
            assert_eq!(runner.calls.lock().unwrap().len(), 3);
        }
    }

    #[test]
    fn hidden_workspace_rows_still_resolve_reuse_and_status_does_not_change_revision() {
        let lists = action_lists(Some(("w1", "dir: tmp")));
        let (mut app, _) = action_app(&lists, None);
        app.config.sources.open_workspaces = false;
        select_directory(&mut app);
        let revision = app.displayed_actions[0].revision;
        assert!(app.displayed_actions[0].action.description().contains("w1"));
        app.spinner_tick += 1;
        app.entries[0].subtitle = "working".into();
        app.commit_displayed_actions();
        assert_eq!(app.displayed_actions[0].revision, revision);
    }

    #[test]
    fn config_owner_label_and_provider_command_changes_reject_before_dispatch() {
        for change in ["config", "owner", "label", "command", "timeout"] {
            let (mut app, runner) = action_app(&action_lists(None), None);
            select_directory(&mut app);
            if matches!(change, "command" | "timeout") {
                app.entries[0].action = EntryAction::RunCommand {
                    command: "exit 0".into(),
                    timeout_ms: 0,
                    notify_success: false,
                    notify_error: false,
                };
                app.prepare_actions();
                app.commit_displayed_actions();
            }
            match change {
                "config" => app.config.picker.create_missing = false,
                "owner" => app.navigation.invalidate_scope(),
                "label" => app.entries[0].title = "other".into(),
                "command" => {
                    if let EntryAction::RunCommand { command, .. } = &mut app.entries[0].action {
                        *command = "exit 1".into();
                    }
                }
                _ => {
                    if let EntryAction::RunCommand { timeout_ms, .. } = &mut app.entries[0].action {
                        *timeout_ms = 5;
                    }
                }
            }
            assert!(app
                .open_selected(false)
                .unwrap_err()
                .contains("action changed"));
            assert_eq!(runner.calls.lock().unwrap().len(), 3);
        }
    }

    fn close_test_app(directory: PathBuf) -> (App, std::sync::Arc<crate::herdr::RecordingRunner>) {
        let mut config = Config::default();
        config.picker.agent_sort = "spaces".into();
        config.sources.herdr_plus_projects = false;
        config.sources.herdr_plus_quick_actions = false;
        config.sources.zoxide = false;
        config.sources.roots = false;
        config.sources.servers = false;
        config.sources.sessions = false;
        config.sources.agents = false;
        config.sources.panes = false;
        let mut app = App::with_history(config, Theme::load(false), directory);
        let runner = std::sync::Arc::new(crate::herdr::RecordingRunner::default());
        let mut route = crate::herdr::invocation();
        route.socket = Some("/synthetic/fix1-unused.sock".into());
        route.managed_plugin = false;
        app.navigation =
            NavigationContext::with_server(crate::navigation::server::OrdinaryServer::new(
                route,
                Box::new(runner.clone()),
                crate::herdr::Limits::default(),
            ));
        (app, runner)
    }

    fn select_close_workspace(app: &mut App, id: &str) {
        app.selected = app
            .filtered
            .iter()
            .position(|index| {
                app.entries[*index]
                    .target()
                    .is_some_and(|target| target.identity().id() == id)
            })
            .unwrap();
        app.commit_displayed_actions();
    }

    #[test]
    fn healthy_refresh_with_unknown_picker_owner_cannot_authorize_workspace_close() {
        let directory = preference_dir();
        let (mut app, runner) = close_test_app(directory.clone());
        let lists = crate::navigation_test_support::ordinary_lists();
        let owner = crate::model::LiveTarget::new(
            LiveIdentity::new(app.navigation.scope(), ObjectKind::Workspace, "w43").unwrap(),
            None,
            None,
        )
        .unwrap();
        app.navigation.set_picker_owner(owner.clone()).unwrap();
        queue_action_lists(&runner, &lists);
        app.refresh();
        select_close_workspace(&mut app, "w41");
        assert!(app.can_close_selected());
        app.retain_close_action().unwrap();
        assert!(app.close_description().is_some());

        app.navigation.invalidate_scope();
        let scope = app.navigation.scope().clone();
        assert_ne!(owner.identity().scope(), &scope);
        queue_action_lists(&runner, &lists);
        app.refresh();
        assert_eq!(app.navigation.scope(), &scope);
        assert!(app.navigation.picker_workspace().is_none());
        for id in ["w43", "w41"] {
            select_close_workspace(&mut app, id);
            let selected = app.selected_entry().unwrap().key().clone();
            assert_eq!(
                app.workspace_to_close(app.selected_entry().unwrap())
                    .unwrap()
                    .scope(),
                &scope
            );
            let entries = app.entries.len();
            assert!(!app.can_close_selected());
            assert_eq!(
                app.retain_close_action().unwrap_err(),
                "can't close a workspace because picker ownership is unknown"
            );
            assert!(app.close_description().is_none());
            assert_eq!(
                app.close_selected_workspace().unwrap_err(),
                "no confirmed close target"
            );
            assert_eq!(app.selected_entry().unwrap().key(), &selected);
            assert_eq!(app.entries.len(), entries);
            let calls = runner.calls.lock().unwrap();
            assert_eq!(calls.len(), 6);
            assert!(calls
                .iter()
                .all(|call| call.args.get(1).is_some_and(|arg| arg == "list")));
        }
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn retained_close_loses_authorization_before_revalidation_after_scope_invalidation() {
        let directory = preference_dir();
        let (mut app, runner) = close_test_app(directory.clone());
        let lists = crate::navigation_test_support::ordinary_lists();
        let owner = crate::model::LiveTarget::new(
            LiveIdentity::new(app.navigation.scope(), ObjectKind::Workspace, "w43").unwrap(),
            None,
            None,
        )
        .unwrap();
        app.navigation.set_picker_owner(owner).unwrap();
        queue_action_lists(&runner, &lists);
        app.refresh();
        select_close_workspace(&mut app, "w41");
        app.retain_close_action().unwrap();
        app.navigation.invalidate_scope();
        assert_eq!(
            app.close_selected_workspace().unwrap_err(),
            "can't close a workspace because picker ownership is unknown"
        );
        assert!(app.close_description().is_none());
        assert_eq!(
            app.close_selected_workspace().unwrap_err(),
            "no confirmed close target"
        );
        let calls = runner.calls.lock().unwrap();
        assert_eq!(calls.len(), 3);
        assert!(calls
            .iter()
            .all(|call| call.args.get(1).is_some_and(|arg| arg == "list")));
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn retained_close_target_is_rechecked_after_confirmation_and_protects_picker() {
        let before = action_lists(Some(("w1", "dir: tmp")));
        let (mut app, runner) = action_app(&before, None);
        let owner = crate::model::LiveTarget::new(
            LiveIdentity::new(
                app.navigation.scope(),
                ObjectKind::Workspace,
                "picker-owner",
            )
            .unwrap(),
            None,
            None,
        )
        .unwrap();
        app.navigation.set_picker_owner(owner).unwrap();
        select_directory(&mut app);
        app.retain_close_action().unwrap();
        queue_action_lists(&runner, &action_lists(Some(("w2", "dir: tmp"))));
        assert!(app
            .close_selected_workspace()
            .unwrap_err()
            .contains("action changed"));
        assert_eq!(runner.calls.lock().unwrap().len(), 6);
        app.commit_displayed_actions();
        let owner = crate::model::LiveTarget::new(
            LiveIdentity::new(app.navigation.scope(), ObjectKind::Workspace, "w2").unwrap(),
            None,
            None,
        )
        .unwrap();
        app.navigation.set_picker_owner(owner).unwrap();
        assert!(!app.can_close_selected());
        assert!(app.retain_close_action().unwrap_err().contains("picker"));
    }

    fn session_test_config() -> Config {
        let mut config = Config::default();
        config.sources = crate::config::SourcesConfig {
            panes: false,
            open_workspaces: false,
            herdr_plus_projects: false,
            zoxide: false,
            roots: false,
            agents: false,
            servers: false,
            sessions: true,
            herdr_plus_quick_actions: false,
        };
        config.notifications.enabled = false;
        config.sessions.entries = vec![crate::config::SessionEntryConfig {
            name: "Work alias".into(),
            session: Some("work".into()),
            remote: None,
            tags: vec!["uniquesessiontag".into()],
        }];
        config
    }

    #[cfg(unix)]
    fn with_session_cli(check: impl FnOnce(&std::path::Path)) {
        use std::os::unix::fs::PermissionsExt;

        let _lock = crate::herdr::bin_path_lock();
        struct Stub {
            previous: Option<std::ffi::OsString>,
            directory: PathBuf,
        }
        impl Drop for Stub {
            fn drop(&mut self) {
                match &self.previous {
                    Some(value) => env::set_var("HERDR_BIN_PATH", value),
                    None => env::remove_var("HERDR_BIN_PATH"),
                }
                let _ = fs::remove_dir_all(&self.directory);
            }
        }
        let stub = Stub {
            previous: env::var_os("HERDR_BIN_PATH"),
            directory: preference_dir(),
        };
        fs::create_dir_all(stub.directory.join("session-dir")).unwrap();
        fs::write(
            stub.directory.join("sessions.json"),
            serde_json::json!({"sessions": [{
                "name": "work", "running": true,
                "session_dir": stub.directory.join("session-dir")
            }]})
            .to_string(),
        )
        .unwrap();
        let executable = stub.directory.join("herdr");
        fs::write(
            &executable,
            format!(
                r#"#!/bin/sh
args=$(printf '<%s>' "$@")
printf '%s\n' "$args" >> '{directory}/calls'
case "$args" in
  '<session><list><--json>') /bin/cat '{directory}/sessions.json' ;;
  '<session><attach><work>'|'<session><attach><other>') exit 0 ;;
  *) printf 'blocked session-test command\n' >&2; exit 125 ;;
esac
"#,
                directory = stub.directory.display(),
            ),
        )
        .unwrap();
        fs::set_permissions(&executable, fs::Permissions::from_mode(0o755)).unwrap();
        env::set_var("HERDR_BIN_PATH", executable);
        check(&stub.directory);
    }

    #[cfg(unix)]
    #[test]
    fn discovered_and_configured_local_attach_refresh_search_dispatch_and_pin() {
        with_session_cli(|directory| {
            let config = session_test_config();
            let history = directory.join("history");
            let mut app = App::with_history(config.clone(), Theme::load(false), history.clone());
            app.refresh();
            assert_eq!(app.entries.len(), 1);
            let merged = app.entries[0].clone();
            assert_eq!(merged.action_conflict, None);
            assert_eq!(merged.title, "work");
            assert!(merged.subtitle.contains("running"));
            assert_eq!(
                merged.path,
                Some(OwnedPath::process(directory.join("session-dir")))
            );
            let action = app.resolved_action(&merged, Slot::Primary);
            assert!(action.unavailable.is_none());
            assert!(matches!(action.owner, actions::Owner::Process(_)));
            assert!(
                matches!(action.payload, actions::Payload::Attach { ref target, remote: false } if target == "work")
            );
            for query in ["Work alias", "uniquesessiontag", "work"] {
                app.query = query.into();
                app.apply_filter();
                assert_eq!(app.filtered.len(), 1, "{query}");
                assert_eq!(app.selected_entry().unwrap().key(), merged.key());
                let input = app.picker_input();
                let projection =
                    app.picker_projection(&input, &crate::picker::PickerState::new(&input));
                assert_eq!(projection.rows.len(), 1, "{query}");
                assert_eq!(
                    projection.rows[0].id,
                    crate::picker::RowId::Destination(merged.key().clone())
                );
            }
            app.commit_displayed_actions();
            let token = app.displayed_actions[0].clone();
            let before = fs::read_to_string(directory.join("calls")).unwrap();
            app.activate_displayed(&token).unwrap();
            let after = fs::read_to_string(directory.join("calls")).unwrap();
            assert_eq!(after, format!("{before}<session><attach><work>\n"));
            assert!(app.activate_displayed(&token).is_err());
            assert_eq!(fs::read_to_string(directory.join("calls")).unwrap(), after);

            let mut discovered_config = config.clone();
            discovered_config.sessions.entries.clear();
            let discovered = sessions::collect_sessions(&discovered_config).remove(0);
            let mut configured_config = config;
            configured_config.sessions.local = false;
            let configured = sessions::collect_sessions(&configured_config).remove(0);
            let key = app.history.key(&merged, None).unwrap().unwrap();
            assert_eq!(
                app.history.key(&discovered, None).unwrap(),
                Some(key.clone())
            );
            assert_eq!(app.history.key(&configured, None).unwrap(), Some(key));
            app.toggle_selected_pin().unwrap();
            assert!(app.is_pinned(&merged));
            drop(app);

            // Reconstruct through refresh with discovery absent, not a replacement row.
            fs::write(directory.join("sessions.json"), r#"{"sessions":[]}"#).unwrap();
            configured_config.sessions.local = true;
            let mut app = App::with_history(configured_config, Theme::load(false), history);
            app.refresh();
            assert_eq!(app.entries.len(), 1);
            assert_eq!(app.entries[0].title, "Work alias");
            assert!(app.entries[0].path.is_none());
            assert!(app.is_pinned(&app.entries[0]));
            assert!(app
                .resolved_action(&app.entries[0], Slot::Primary)
                .unavailable
                .is_none());
            app.config.sessions.entries[0].session = Some("other".into());
            app.refresh();
            assert_eq!(app.entries.len(), 1);
            assert!(!app.is_pinned(&app.entries[0]));
            assert_eq!(app.history.records().unwrap().len(), 1);
            assert_eq!(
                fs::read_to_string(directory.join("calls"))
                    .unwrap()
                    .matches("<session><attach>")
                    .count(),
                1
            );
        });
    }

    #[cfg(unix)]
    #[test]
    fn merged_local_attach_changes_still_require_a_new_displayed_token() {
        with_session_cli(|directory| {
            for change in ["title", "path", "target"] {
                let mut app = App::with_history(
                    session_test_config(),
                    Theme::load(false),
                    directory.join(change),
                );
                app.refresh();
                assert_eq!(app.entries.len(), 1);
                assert_eq!(app.entries[0].action_conflict, None);
                app.commit_displayed_actions();
                let token = app.displayed_actions[0].clone();
                match change {
                    "title" => app.entries[0].title = "Changed alias".into(),
                    "path" => {
                        app.entries[0].path = Some(OwnedPath::process(directory.join("changed")))
                    }
                    _ => {
                        app.entries[0].action = EntryAction::AttachSession {
                            name: "other".into(),
                            remote: None,
                        }
                    }
                }
                let before = fs::read_to_string(directory.join("calls")).unwrap();
                assert!(app
                    .activate_displayed(&token)
                    .unwrap_err()
                    .contains("action changed"));
                assert!(app.activate_displayed(&token).is_err());
                assert_eq!(fs::read_to_string(directory.join("calls")).unwrap(), before);
                app.commit_displayed_actions();
                assert!(app.activate_displayed(&token).is_err());
                let fresh = app.displayed_actions[0].clone();
                app.activate_displayed(&fresh).unwrap();
                let target = if change == "target" { "other" } else { "work" };
                let after = format!("{before}<session><attach><{target}>\n");
                assert_eq!(fs::read_to_string(directory.join("calls")).unwrap(), after);
                assert!(app.activate_displayed(&fresh).is_err());
                assert_eq!(fs::read_to_string(directory.join("calls")).unwrap(), after);
            }
        });
    }

    fn configured_local_session() -> Entry {
        let mut config = session_test_config();
        config.sessions.local = false;
        sessions::collect_sessions(&config).remove(0)
    }

    #[test]
    fn local_attach_dedupe_keeps_first_configured_display_and_all_aliases() {
        for reverse in [false, true] {
            let mut config = session_test_config();
            config.sessions.local = false;
            let mut alias = config.sessions.entries[0].clone();
            alias.name = "Second alias".into();
            alias.tags = vec!["secondtag".into(), "secondtag".into()];
            config.sessions.entries.push(alias);
            if reverse {
                config.sessions.entries.reverse();
            }
            let mut incoming = sessions::collect_sessions(&config);
            let first = incoming[0].clone();
            incoming[1]
                .provenance
                .insert(ProviderId::new("extra").unwrap());
            incoming.push(incoming[1].clone());
            let mut rows = Vec::new();
            push_unique(&mut rows, &mut HashSet::new(), incoming);
            assert_eq!(rows.len(), 1);
            assert_eq!(rows[0].action_conflict, None);
            assert_eq!(rows[0].title, first.title);
            assert_eq!(rows[0].subtitle, first.subtitle);
            assert_eq!(rows[0].path, first.path);
            assert_eq!(rows[0].provenance.len(), 2);
            for term in [
                "session",
                "work",
                "uniquesessiontag",
                "secondtag",
                if reverse {
                    "Work alias"
                } else {
                    "Second alias"
                },
            ] {
                assert_eq!(
                    rows[0]
                        .search_terms
                        .iter()
                        .filter(|value| value.as_str() == term)
                        .count(),
                    1,
                    "{term}"
                );
            }
            assert_eq!(
                rows[0].search_terms.iter().collect::<HashSet<_>>().len(),
                rows[0].search_terms.len()
            );
        }
    }

    #[test]
    fn local_attach_dedupe_rejects_changed_action_project_and_template() {
        let first = configured_local_session();
        for change in [
            "name",
            "remote",
            "run",
            "open-remote",
            "project",
            "template",
        ] {
            let mut second = first.clone();
            match change {
                "name" => {
                    second.action = EntryAction::AttachSession {
                        name: "other".into(),
                        remote: None,
                    }
                }
                "remote" => {
                    second.action = EntryAction::AttachSession {
                        name: "work".into(),
                        remote: Some("host".into()),
                    }
                }
                "run" => {
                    second.action = EntryAction::RunCommand {
                        command: "exit 0".into(),
                        timeout_ms: 0,
                        notify_success: false,
                        notify_error: false,
                    }
                }
                "open-remote" => {
                    second.action = EntryAction::OpenRemote {
                        target: "work".into(),
                    }
                }
                "project" => {
                    second.project = Some(Project {
                        name: "project".into(),
                        description: String::new(),
                        working_dir: "/tmp".into(),
                        tabs: vec![],
                    })
                }
                _ => second.template_file = Some(OwnedPath::process("/tmp/template.toml".into())),
            }
            let mut rows = Vec::new();
            // A later compatible duplicate must not erase the earlier conflict.
            push_unique(
                &mut rows,
                &mut HashSet::new(),
                vec![first.clone(), second, first.clone()],
            );
            assert_eq!(rows.len(), 1);
            assert!(
                rows[0]
                    .action_conflict
                    .as_ref()
                    .unwrap()
                    .contains("conflicting"),
                "{change}"
            );
        }
    }

    #[test]
    fn attach_metadata_exception_requires_builtin_sessions_source_and_action_key() {
        let session = configured_local_session();
        for identity in [
            EntryIdentity::provider("other-provider", "work", "attach").unwrap(),
            EntryIdentity::command("sessions", "work", "attach").unwrap(),
            EntryIdentity::provider("sessions", "work", "other-action").unwrap(),
        ] {
            let first = Entry::new(identity, session.data.clone());
            let mut second = first.clone();
            second.title = "different".into();
            second.path = Some(OwnedPath::process("/tmp/work".into()));
            let mut rows = Vec::new();
            push_unique(&mut rows, &mut HashSet::new(), vec![first, second]);
            assert!(rows[0].action_conflict.is_some());
        }
        for wrong_source_first in [false, true] {
            let mut first = session.clone();
            let mut second = session.clone();
            if wrong_source_first {
                first.source = Source::Server;
            } else {
                second.source = Source::Server;
            }
            second.title = "different".into();
            let mut rows = Vec::new();
            push_unique(&mut rows, &mut HashSet::new(), vec![first, second]);
            assert!(rows[0].action_conflict.is_some());
        }
        for action in [
            EntryAction::AttachSession {
                name: "work".into(),
                remote: Some("host".into()),
            },
            EntryAction::OpenRemote {
                target: "host".into(),
            },
            EntryAction::RunCommand {
                command: "exit 0".into(),
                timeout_ms: 0,
                notify_success: false,
                notify_error: false,
            },
            EntryAction::FocusOrCreateDir,
            EntryAction::OpenProject,
        ] {
            for change in ["title", "path", "project", "template"] {
                let mut first = session.clone();
                first.action = action.clone();
                let mut second = first.clone();
                match change {
                    "title" => second.title = "different".into(),
                    "path" => second.path = Some(OwnedPath::process("/tmp/work".into())),
                    "project" => {
                        second.project = Some(Project {
                            name: "project".into(),
                            description: String::new(),
                            working_dir: "/tmp".into(),
                            tabs: vec![],
                        })
                    }
                    _ => {
                        second.template_file = Some(OwnedPath::process("/tmp/template.toml".into()))
                    }
                }
                let mut rows = Vec::new();
                push_unique(&mut rows, &mut HashSet::new(), vec![first, second]);
                assert!(rows[0].action_conflict.is_some(), "{action:?} {change}");
            }
        }
    }

    #[test]
    fn session_dedupe_keeps_distinct_qualified_identities_separate() {
        let first = configured_local_session();
        let mut incoming = vec![first.clone()];
        let mut config = session_test_config();
        config.sessions.local = false;
        config.sessions.entries[0].session = Some("other".into());
        incoming.extend(sessions::collect_sessions(&config));
        config.sessions.entries[0].remote = Some("work".into());
        incoming.extend(sessions::collect_remotes(&config));
        for identity in [
            EntryIdentity::provider("other-provider", "work", "attach").unwrap(),
            EntryIdentity::command("sessions", "work", "attach").unwrap(),
            EntryIdentity::provider("sessions", "work", "other-action").unwrap(),
        ] {
            incoming.push(Entry::new(identity, first.data.clone()));
        }
        let json = serde_json::json!({"result":{"workspaces":[{"workspace_id":"w1"}]}});
        for _ in 0..2 {
            let context = NavigationContext::new();
            incoming
                .extend(crate::sources::workspaces_from_json(&json, &HashMap::new(), &context).0);
        }
        let count = incoming.len();
        let mut rows = Vec::new();
        push_unique(&mut rows, &mut HashSet::new(), incoming);
        assert_eq!(rows.len(), count);
        assert!(rows.iter().all(|row| row.action_conflict.is_none()));
    }

    #[test]
    fn dedupe_preserves_incoming_and_existing_conflict_diagnostics() {
        let first = configured_local_session();
        for incompatible in [false, true] {
            for existing_conflict in [false, true] {
                let mut first = first.clone();
                if existing_conflict {
                    first.action_conflict = Some("earlier conflict".into());
                }
                let mut second = first.clone();
                second.action_conflict = Some("incoming conflict".into());
                if incompatible {
                    second.action = EntryAction::OpenRemote {
                        target: "host".into(),
                    };
                }
                let mut rows = Vec::new();
                let mut seen = HashSet::new();
                push_unique(&mut rows, &mut seen, vec![first, second]);
                push_unique(&mut rows, &mut seen, vec![configured_local_session()]);
                let diagnostic = rows[0].action_conflict.as_deref().unwrap();
                assert!(diagnostic.contains("incoming conflict"));
                if existing_conflict {
                    assert!(diagnostic.contains("earlier conflict"));
                }
            }
        }
    }

    #[test]
    fn duplicate_live_identity_with_changed_ancestry_still_conflicts() {
        let context = NavigationContext::new();
        for parent in [ObjectKind::Workspace, ObjectKind::Tab] {
            let mut incoming = Vec::new();
            for changed in [false, true] {
                let target = crate::model::LiveTarget::new(
                    LiveIdentity::new(context.scope(), ObjectKind::Pane, "pane").unwrap(),
                    LiveIdentity::new(
                        context.scope(),
                        ObjectKind::Workspace,
                        if changed && parent == ObjectKind::Workspace {
                            "other"
                        } else {
                            "workspace"
                        },
                    ),
                    LiveIdentity::new(
                        context.scope(),
                        ObjectKind::Tab,
                        if changed && parent == ObjectKind::Tab {
                            "other"
                        } else {
                            "tab"
                        },
                    ),
                )
                .unwrap();
                let mut row =
                    Entry::new(EntryIdentity::live(target), configured_local_session().data);
                row.action = EntryAction::FocusPane;
                incoming.push(row);
            }
            assert_eq!(incoming[0].key(), incoming[1].key());
            let mut rows = Vec::new();
            push_unique(&mut rows, &mut HashSet::new(), incoming);
            assert_eq!(rows.len(), 1);
            assert!(rows[0].action_conflict.is_some());
        }
    }

    #[test]
    fn duplicate_provider_id_with_conflicting_semantics_is_unavailable() {
        let first = entry(Source::Root, "/tmp", "tmp");
        let mut second = first.clone();
        second.action = EntryAction::RunCommand {
            command: "never execute".into(),
            timeout_ms: 0,
            notify_success: false,
            notify_error: false,
        };
        let mut entries = Vec::new();
        push_unique(&mut entries, &mut HashSet::new(), vec![first, second]);
        assert_eq!(entries.len(), 1);
        assert!(entries[0]
            .action_conflict
            .as_ref()
            .unwrap()
            .contains("conflicting"));
    }

    #[test]
    fn a_reparented_pane_cannot_execute_the_displayed_focus() {
        let before = action_lists(Some(("w1", "dir: tmp")));
        let (mut app, runner) = action_app(&before, None);
        app.entries = app.joined_inventory.panes.clone();
        app.prepare_actions();
        app.apply_filter();
        app.commit_displayed_actions();
        let mut after = before.clone();
        after[2]["result"]["tabs"][0]["tab_id"] = "w1:other-tab".into();
        after[1]["result"]["panes"][0]["tab_id"] = "w1:other-tab".into();
        queue_action_lists(&runner, &after);
        assert!(app
            .open_selected(false)
            .unwrap_err()
            .contains("action changed"));
        assert_eq!(runner.calls.lock().unwrap().len(), 6);
        assert!(app.retain_close_action().is_err());
    }

    #[test]
    fn exact_template_contents_are_reloaded_once_then_changes_require_a_new_draw() {
        let original = "name = 'demo'\nworking_dir = '/tmp'\n[[tabs]]\nname = 'shell'\ncommand = 'echo original'\n";
        for change in [
            "cwd",
            "label",
            "tabs",
            "command",
            "comment",
            "disappear",
            "invalid",
        ] {
            let dir = env::temp_dir().join(format!("n06-template-{}-{change}", std::process::id()));
            fs::create_dir_all(&dir).unwrap();
            let file = dir.join("project.toml");
            fs::write(&file, original).unwrap();
            let (mut app, runner) = action_app(&action_lists(None), None);
            let mut row = entry(Source::Project, "/tmp", "demo");
            row.action = EntryAction::OpenProject;
            row.template_file = Some(OwnedPath::process(file.clone()));
            app.entries = vec![row];
            app.prepare_actions();
            app.apply_filter();
            app.commit_displayed_actions();
            let changed = match change {
                "cwd" => original.replace("/tmp", "/another-cwd"),
                "label" => original.replace("demo", "renamed"),
                "tabs" => format!("{original}\n[[tabs]]\nname='second'"),
                "command" => original.replace("echo original", "echo changed"),
                "comment" => format!("{original}# new contents\n"),
                "invalid" => "not = [valid".into(),
                _ => String::new(),
            };
            if change == "disappear" {
                fs::remove_file(&file).unwrap();
            } else {
                fs::write(&file, changed).unwrap();
            }
            queue_action_lists(&runner, &action_lists(None));
            assert!(
                app.open_selected(false)
                    .unwrap_err()
                    .contains("action changed"),
                "{change}"
            );
            assert_eq!(runner.calls.lock().unwrap().len(), 6);
            assert!(app.open_selected(false).is_err());
            assert_eq!(runner.calls.lock().unwrap().len(), 6);
            app.commit_displayed_actions();
            assert!(app.displayed_actions[0].revision > 1);
            fs::remove_dir_all(dir).unwrap();
        }
    }

    #[cfg(unix)]
    #[test]
    fn projects_and_explicit_directory_templates_dispatch_the_displayed_route() {
        use crate::navigation::server::tests::{snapshot, Reply, Socket};
        for project in [false, true] {
            for existing in [false, true] {
                let dir = env::temp_dir().join(format!(
                    "n06-dispatch-{}-{project}-{existing}",
                    std::process::id()
                ));
                fs::create_dir_all(&dir).unwrap();
                let file = dir.join("template.toml");
                fs::write(&file, "name='demo'\nworking_dir='/tmp'\n[[tabs]]\nname='shell'\ncommand=\"printf 'quoted startup'\"\n").unwrap();
                let socket = existing.then(|| {
                    Socket::new(vec![
                        Reply::Result(snapshot()),
                        Reply::Result(serde_json::json!({"type":"ok"})),
                    ])
                });
                let lists = action_lists(existing.then_some((
                    "w1",
                    if project {
                        "project: demo"
                    } else {
                        "project: another"
                    },
                )));
                let (mut app, runner) = action_app(&lists, socket.as_ref().map(|s| s.path.clone()));
                let slot = if project {
                    Slot::Primary
                } else {
                    Slot::Template
                };
                let mut row = entry(
                    if project {
                        Source::Project
                    } else {
                        Source::Root
                    },
                    "/tmp",
                    "demo",
                );
                if project {
                    row.action = EntryAction::OpenProject;
                    row.template_file = Some(OwnedPath::process(file.clone()));
                } else {
                    app.config.picker.reuse_existing = false;
                    app.config.picker.create_missing = false;
                    app.config.picker.directory_template = Some("template.toml".into());
                }
                app.entries = vec![row];
                app.prepare_actions();
                let prepared = Prepared::load(
                    &app.entries[0],
                    slot,
                    &app.config,
                    &app.navigation,
                    Some(&OwnedPath::process(file.clone())),
                    &actions::Files,
                );
                app.prepared_actions
                    .insert((app.entries[0].key().clone(), slot), prepared);
                app.apply_filter();
                app.commit_displayed_actions();
                let description = app
                    .displayed_actions
                    .iter()
                    .find(|token| token.action.slot == slot)
                    .unwrap()
                    .action
                    .description();
                assert!(description.starts_with(match (project, existing) {
                    (true, true) => "Focus workspace",
                    (true, false) => "Create project",
                    (false, true) => "Apply demo to workspace",
                    (false, false) => "Create template",
                }));
                queue_action_lists(&runner, &lists);
                if !existing {
                    runner.replies.lock().unwrap().push_back(Ok(serde_json::to_vec(&serde_json::json!({"result":{"workspace":{"workspace_id":"created"},"root_pane":{"pane_id":"created:p1"}}})).unwrap()));
                    runner.replies.lock().unwrap().push_back(Ok(vec![])); // rename first tab
                } else if !project {
                    runner
                        .replies
                        .lock()
                        .unwrap()
                        .push_back(Ok(serde_json::to_vec(
                            &serde_json::json!({"result":{"root_pane":{"pane_id":"w1:new-pane"}}}),
                        )
                        .unwrap()));
                }
                if !existing || !project {
                    runner.replies.lock().unwrap().push_back(Ok(vec![]));
                }
                app.open_selected(!project).unwrap();
                let calls = runner.calls.lock().unwrap();
                let mutations = &calls[6..];
                if project && existing {
                    assert!(mutations.is_empty());
                } else {
                    assert_eq!(mutations.last().unwrap().args[0], "pane");
                    assert_eq!(mutations.last().unwrap().args[1], "run");
                    assert_eq!(mutations.last().unwrap().args[3], "printf 'quoted startup'");
                    assert_eq!(
                        mutations[0].args[0],
                        if existing { "tab" } else { "workspace" }
                    );
                    assert_eq!(mutations[0].args[1], "create");
                    if !existing {
                        assert_eq!(mutations[1].args[0], "tab");
                        assert_eq!(mutations[1].args[2], "created:t1");
                    }
                }
                if let Some(socket) = socket {
                    let requests = socket.requests.lock().unwrap();
                    assert_eq!(requests[1]["method"], "workspace.focus");
                    assert_eq!(requests[1]["params"]["workspace_id"], "w1");
                }
                fs::remove_dir_all(dir).unwrap();
            }
        }
    }

    #[test]
    fn failed_template_mutations_stop_and_never_retry_or_destroy_partial_workspaces() {
        let dir = env::temp_dir().join(format!("n06-partial-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let file = dir.join("template.toml");
        fs::write(&file, "name='demo'\nworking_dir='/tmp'\n[[tabs]]\nname='shell'\ncommand='echo first'\n[[tabs]]\nname='second'\ncommand='echo second'\n").unwrap();
        let replies = [
            serde_json::json!({"result":{"workspace":{"workspace_id":"new"},"root_pane":{"pane_id":"new:p1"}}}),
            serde_json::Value::Null,
            serde_json::json!({"result":{"root_pane":{"pane_id":"new:p2"}}}),
            serde_json::Value::Null,
            serde_json::Value::Null,
        ];
        for failed_at in 0..replies.len() {
            let lists = action_lists(None);
            let (mut app, runner) = action_app(&lists, None);
            let scope = app.navigation.scope().clone();
            let mut row = entry(Source::Project, "/tmp", "demo");
            row.action = EntryAction::OpenProject;
            row.template_file = Some(OwnedPath::process(file.clone()));
            app.entries = vec![row];
            app.prepare_actions();
            app.apply_filter();
            app.commit_displayed_actions();
            queue_action_lists(&runner, &lists);
            runner.replies.lock().unwrap().extend(
                replies
                    .iter()
                    .take(failed_at)
                    .map(|v| Ok(serde_json::to_vec(v).unwrap())),
            );
            runner
                .replies
                .lock()
                .unwrap()
                .push_back(Err(TransportError::new(
                    Failure::Timeout,
                    MutationOutcome::Unknown,
                )));
            let error = app.open_selected(false).unwrap_err();
            assert!(error.contains("Unknown"));
            if failed_at > 0 {
                assert!(error.contains("partially created"));
            }
            assert_ne!(app.navigation.scope(), &scope);
            assert_eq!(runner.calls.lock().unwrap().len(), 7 + failed_at);
            assert!(app.open_selected(false).is_err());
            assert_eq!(runner.calls.lock().unwrap().len(), 7 + failed_at);
        }
        fs::remove_dir_all(dir).unwrap();
    }
    #[test]
    fn confirmed_workspace_close_uses_one_frozen_request_and_failure_is_not_retried() {
        for succeed in [false, true] {
            let before = action_lists(Some(("w1", "dir: tmp")));
            let (mut app, runner) = action_app(&before, None);
            let owner = crate::model::LiveTarget::new(
                LiveIdentity::new(
                    app.navigation.scope(),
                    ObjectKind::Workspace,
                    "picker-owner",
                )
                .unwrap(),
                None,
                None,
            )
            .unwrap();
            app.navigation.set_picker_owner(owner).unwrap();
            select_directory(&mut app);
            app.retain_close_action().unwrap();
            assert_eq!(app.close_description(), Some("dir: tmp [w1]"));
            queue_action_lists(&runner, &before);
            runner.replies.lock().unwrap().push_back(if succeed {
                Ok(vec![])
            } else {
                Err(TransportError::new(
                    Failure::Timeout,
                    MutationOutcome::Unknown,
                ))
            });
            if succeed {
                queue_action_lists(&runner, &action_lists(None));
            }
            assert_eq!(app.close_selected_workspace().is_ok(), succeed);
            assert!(app.close_selected_workspace().is_err());
            let calls = runner.calls.lock().unwrap();
            assert_eq!(calls.len(), if succeed { 10 } else { 7 });
            assert_eq!(
                calls[6].args,
                ["workspace", "close", "w1"].map(std::ffi::OsString::from)
            );
        }
    }

    #[test]
    fn resolved_reuse_keeps_project_folder_and_explicit_template_preferences() {
        struct TemplateFiles;
        impl actions::ProcessFiles for TemplateFiles {
            fn read(&self, _: &Path) -> Result<String, String> {
                Ok("name='demo'\nworking_dir='/tmp'".into())
            }
            fn canonical(&self, path: &Path) -> Option<PathBuf> {
                Some(path.to_path_buf())
            }
        }
        for project in [false, true] {
            for label in ["project: demo", "dir: tmp", "plain"] {
                for reuse in [false, true] {
                    let (mut app, _) = action_app(&action_lists(Some(("w1", label))), None);
                    app.config.picker.reuse_existing = reuse;
                    let mut row = entry(
                        if project {
                            Source::Project
                        } else {
                            Source::Root
                        },
                        "/tmp",
                        "demo",
                    );
                    if project {
                        row.action = EntryAction::OpenProject;
                        row.template_file =
                            Some(OwnedPath::process("/synthetic/template.toml".into()));
                    }
                    let prepared = Prepared::load(
                        &row,
                        Slot::Primary,
                        &app.config,
                        &app.navigation,
                        None,
                        &TemplateFiles,
                    );
                    app.prepared_actions
                        .insert((row.key().clone(), Slot::Primary), prepared);
                    let action = app.resolved_action(&row, Slot::Primary);
                    let focus = reuse && (project == label.starts_with("project:"));
                    assert_eq!(matches!(action.payload, actions::Payload::Focus(_)), focus);
                    assert!(action.unavailable.is_none());
                    if !focus {
                        assert!(matches!(
                            action.payload,
                            actions::Payload::CreateProject { .. }
                                | actions::Payload::OpenFolder { .. }
                        ));
                    }
                    app.config.picker.create_missing = false;
                    assert_eq!(
                        app.resolved_action(&row, Slot::Primary)
                            .unavailable
                            .is_none(),
                        focus
                    );
                }
            }
        }
    }
    #[test]
    fn displayed_tokens_cannot_be_replayed_or_submitted_with_another_revision() {
        let (mut app, runner) = action_app(&action_lists(None), None);
        app.entries = vec![herdr_plus::quick_actions_entry()];
        app.prepare_actions();
        app.apply_filter();
        app.commit_displayed_actions();
        let token = app.displayed_actions[0].clone();
        let mut wrong_revision = token.clone();
        wrong_revision.revision += 1;
        assert!(app.activate_displayed(&wrong_revision).is_err());
        assert_eq!(runner.calls.lock().unwrap().len(), 3);
        runner.replies.lock().unwrap().push_back(Ok(vec![]));
        app.activate_displayed(&token).unwrap();
        assert!(app.activate_displayed(&token).is_err());
        assert_eq!(runner.calls.lock().unwrap().len(), 4);
        app.commit_displayed_actions();
        assert!(app.displayed_actions[0].revision > token.revision);
    }
    #[test]
    fn merged_intents_require_the_exact_committed_identity_and_slot() {
        let (mut app, runner) = action_app(&action_lists(None), None);
        app.entries = vec![herdr_plus::quick_actions_entry()];
        app.prepare_actions();
        app.apply_filter();
        app.commit_displayed_actions();
        let committed = app.displayed_actions[0].action.identity.clone();
        let fixture = crate::navigation_test_support::FixtureClient::sample().picker_input();
        let merged_tab = fixture
            .destinations
            .iter()
            .find(|destination| {
                destination.entry.destination_kind() == crate::model::DestinationKind::Tab
            })
            .unwrap()
            .entry
            .key()
            .clone();
        for intent in [
            crate::picker::input::ActivationIntent {
                identity: merged_tab,
                slot: Slot::Primary,
            },
            crate::picker::input::ActivationIntent {
                identity: committed.clone(),
                slot: Slot::Template,
            },
        ] {
            assert!(app
                .open_intent(&intent)
                .unwrap_err()
                .contains("has not been displayed for this identity and slot"));
            assert_eq!(runner.calls.lock().unwrap().len(), 3);
        }
        runner.replies.lock().unwrap().push_back(Ok(vec![]));
        app.open_intent(&crate::picker::input::ActivationIntent {
            identity: committed,
            slot: Slot::Primary,
        })
        .unwrap();
        assert_eq!(runner.calls.lock().unwrap().len(), 4);
    }

    fn preference_dir() -> std::path::PathBuf {
        env::temp_dir().join(format!(
            "navigator-app-history-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ))
    }

    #[test]
    fn provider_pins_survive_restart_absence_and_display_changes_without_replacement_rows() {
        let dir = preference_dir();
        let mut app = App::with_history(Config::default(), Theme::load(false), dir.clone());
        app.entries = vec![entry(Source::Root, "/repo", "first label")];
        app.apply_filter();
        app.toggle_selected_pin().unwrap();
        drop(app);
        let mut app = App::with_history(Config::default(), Theme::load(false), dir.clone());
        app.project_pins();
        assert!(app.entries.is_empty());
        assert!(app.pinned_entries.is_empty());
        app.entries = vec![entry(Source::Root, "/repo", "renamed")];
        app.entries[0].workspace_id = Some("reused-workspace".into());
        app.entries[0].workspace_label = Some("current status".into());
        app.project_pins();
        app.apply_filter();
        assert_eq!(app.entries.len(), 1);
        assert!(app.is_pinned(&app.entries[0]));
        assert_eq!(app.selected_entry().unwrap().title, "renamed");
        app.entries.clear();
        app.project_pins();
        assert_eq!(app.history.records().unwrap().len(), 1);
        app.entries = vec![entry(Source::Root, "/repo", "returned")];
        app.project_pins();
        app.apply_filter();
        assert!(app.is_pinned(&app.entries[0]));
        app.toggle_selected_pin().unwrap();
        assert!(Store::open(dir.clone()).records().unwrap().is_empty());
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn provider_action_template_and_attach_identity_changes_do_not_transfer_marks() {
        let dir = preference_dir();
        let mut app = App::with_history(Config::default(), Theme::load(false), dir.clone());
        let folder = entry(Source::Root, "/repo", "repo");
        let plain = app.history.key(&folder, None).unwrap().unwrap();
        assert_ne!(
            plain,
            app.history
                .key(&folder, Some("template.toml"))
                .unwrap()
                .unwrap()
        );
        let mut project = Entry::new(
            EntryIdentity::provider("herdr-plus-projects", "one.toml", "open-project").unwrap(),
            folder.data.clone(),
        );
        project.action = EntryAction::OpenProject;
        project.template_file = Some(OwnedPath::process("/templates/one.toml".into()));
        let first = app.history.key(&project, None).unwrap().unwrap();
        assert_ne!(plain, first);
        project.template_file = Some(OwnedPath::process("/templates/two.toml".into()));
        assert_ne!(first, app.history.key(&project, None).unwrap().unwrap());
        let mut session = Entry::new(
            EntryIdentity::provider("sessions", "work", "attach").unwrap(),
            folder.data.clone(),
        );
        session.action = EntryAction::AttachSession {
            name: "work".into(),
            remote: None,
        };
        let local = app.history.key(&session, None).unwrap().unwrap();
        session.action = EntryAction::AttachSession {
            name: "work".into(),
            remote: Some("remote".into()),
        };
        assert_ne!(local, app.history.key(&session, None).unwrap().unwrap());
        app.entries = vec![folder];
        app.apply_filter();
        app.toggle_selected_pin().unwrap();
        app.config.picker.directory_template = Some("changed.toml".into());
        app.project_pins();
        assert!(!app.is_pinned(&app.entries[0]));
        app.config.picker.directory_template = None;
        app.project_pins();
        assert!(app.is_pinned(&app.entries[0]));
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn failed_provider_pin_write_preserves_selection_and_visible_mark() {
        let dir = preference_dir();
        let mut app = App::with_history(Config::default(), Theme::load(false), dir.clone());
        app.entries = vec![entry(Source::Root, "/repo", "repo")];
        app.apply_filter();
        app.toggle_selected_pin().unwrap();
        let key = app.selected_entry().unwrap().key().clone();
        fs::write(dir.join("navigation-history.json"), b"corrupt").unwrap();
        assert!(app.toggle_selected_pin().is_err());
        assert!(app.is_pinned(app.selected_entry().unwrap()));
        assert_eq!(app.selected_entry().unwrap().key(), &key);
        assert_eq!(
            fs::read(dir.join("navigation-history.json")).unwrap(),
            b"corrupt"
        );
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn live_marks_never_persist_and_expired_scope_cannot_mark_or_transfer() {
        let dir = preference_dir();
        let mut app = App::with_history(Config::default(), Theme::load(false), dir.clone());
        let before = fs::read(dir.join("navigation-history.json")).unwrap();
        let json = serde_json::json!({"result":{"workspaces":[{"workspace_id":"w1"}]}});
        let (entries, _) =
            crate::sources::workspaces_from_json(&json, &HashMap::new(), &app.navigation);
        app.entries = entries;
        app.apply_filter();
        app.toggle_selected_pin().unwrap();
        assert!(app.is_pinned(&app.entries[0]));
        assert_eq!(
            fs::read(dir.join("navigation-history.json")).unwrap(),
            before
        );
        app.navigation.invalidate_scope();
        assert!(!app.is_pinned(&app.entries[0]));
        assert!(app.toggle_selected_pin().is_err());
        let (entries, _) =
            crate::sources::workspaces_from_json(&json, &HashMap::new(), &app.navigation);
        app.entries = entries;
        app.project_pins();
        assert!(!app.is_pinned(&app.entries[0]));
        assert!(app.pinned_entries.is_empty());
        assert!(app.history.records().unwrap().is_empty());
        fs::remove_dir_all(dir).unwrap();
    }
}
