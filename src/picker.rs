//! Pure merged search and N08 projection over a caller-owned snapshot.
//! No renderer owns this state yet. Rows confer no dispatch authority.
use std::collections::{HashMap, HashSet};

pub(crate) mod input;

#[cfg(test)]
mod tests;

use crate::{
    actions::ResolvedAction,
    config::PickerConfig,
    matcher::{match_quality, Query, SearchMatcher},
    model::{
        DestinationKind, Entry, EntryIdentity, LiveIdentity, MachineKey, ObjectKind, ServerScope,
        Source,
    },
    navigation::{inventory::TabNaming, ProcessHostRef},
};

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub(crate) enum Owner {
    Machine(MachineKey),
    Server(ServerScope),
    Process(ProcessHostRef),
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum TaskKind {
    ProjectsAndFolders,
    Sessions,
    Actions,
}
impl TaskKind {
    fn label(self) -> &'static str {
        match self {
            Self::ProjectsAndFolders => "Projects and folders",
            Self::Sessions => "Sessions",
            Self::Actions => "Actions",
        }
    }
    fn for_entry(entry: &Entry) -> Option<Self> {
        match entry.destination_kind() {
            DestinationKind::Project | DestinationKind::Folder => Some(Self::ProjectsAndFolders),
            DestinationKind::Session => Some(Self::Sessions),
            DestinationKind::Action => Some(Self::Actions),
            _ => None,
        }
    }
}
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub(crate) enum RowId {
    Destination(EntryIdentity),
    TaskGroup(Owner, TaskKind),
    Heading(Owner),
}
#[derive(Clone, Debug)]
pub(crate) struct Heading {
    pub(crate) owner: Owner,
    pub(crate) label: String,
    /// Only explicit client evidence may put a machine first.
    pub(crate) local: bool,
    pub(crate) machine: Option<EntryIdentity>,
}
impl Heading {
    fn id(&self) -> RowId {
        self.machine
            .clone()
            .map_or_else(|| RowId::Heading(self.owner.clone()), RowId::Destination)
    }
}
#[derive(Clone, Debug)]
pub(crate) struct Destination {
    pub(crate) entry: Entry,
    pub(crate) owner: Owner,
    /// False for a retained workspace used only to parent enabled panes.
    pub(crate) eligible: bool,
    pub(crate) action: ResolvedAction,
}
#[derive(Clone, Debug)]
pub(crate) struct TabInventory {
    pub(crate) expected: u64,
    pub(crate) complete: bool,
}
#[derive(Clone, Debug, Default)]
pub(crate) struct PickerInput {
    /// Client sidebar order. Ordinary server/process headings are explicit too.
    pub(crate) headings: Vec<Heading>,
    pub(crate) destinations: Vec<Destination>,
    /// Presence means supported, even when empty. Absence means unknown/disabled.
    pub(crate) groups: Vec<(Owner, TaskKind)>,
    pub(crate) tabs: HashMap<LiveIdentity, TabInventory>,
    pub(crate) naming: HashMap<LiveIdentity, TabNaming>,
    pub(crate) current: Option<EntryIdentity>,
    pub(crate) pinned: HashSet<EntryIdentity>,
    pub(crate) previous: Option<EntryIdentity>,
}
impl PickerInput {
    pub(crate) fn eligible_ids(&self) -> Vec<EntryIdentity> {
        self.destinations
            .iter()
            .filter(|d| d.eligible)
            .map(|d| d.entry.key().clone())
            .collect()
    }
    fn destination(&self, id: &EntryIdentity) -> Option<&Destination> {
        self.destinations.iter().find(|d| d.entry.key() == id)
    }
    fn live(&self, id: &LiveIdentity) -> Option<&Destination> {
        self.destinations
            .iter()
            .find(|d| d.entry.target().is_some_and(|t| t.identity() == id))
    }
    fn ancestry(&self, destination: &Destination) -> Vec<RowId> {
        let mut ancestors = Vec::new();
        if let Some(heading) = self.headings.iter().find(|h| h.owner == destination.owner) {
            if heading.machine.as_ref() == Some(destination.entry.key()) {
                return ancestors;
            }
            ancestors.push(heading.id());
        }
        if let Some(target) = destination.entry.target() {
            for parent in [target.workspace(), target.tab()].into_iter().flatten() {
                if let Some(parent) = self.live(parent) {
                    ancestors.push(RowId::Destination(parent.entry.key().clone()));
                }
            }
        } else if let Some(kind) = TaskKind::for_entry(&destination.entry) {
            ancestors.push(RowId::TaskGroup(destination.owner.clone(), kind));
        }
        ancestors
    }
    fn compressed(&self, destination: &Destination) -> bool {
        let Some(target) = destination.entry.target() else {
            return false;
        };
        if target.identity().kind() != ObjectKind::Tab
            || self.naming.get(target.identity()) != Some(&TabNaming::Unnamed)
        {
            return false;
        }
        let Some(workspace) = target.workspace() else {
            return false;
        };
        self.tabs
            .get(workspace)
            .is_some_and(|tabs| tabs.complete && tabs.expected == 1)
            && self
                .destinations
                .iter()
                .filter(|d| {
                    d.entry.target().is_some_and(|t| {
                        t.identity().kind() == ObjectKind::Tab && t.workspace() == Some(workspace)
                    })
                })
                .count()
                == 1
    }
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ViewportAnchor {
    pub(crate) row: RowId,
    pub(crate) intra_row_offset: usize,
}
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct Position {
    pub(crate) selection: Option<RowId>,
    pub(crate) viewport: Option<ViewportAnchor>,
}
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) enum MachineFilter {
    #[default]
    All,
    Machine(MachineKey),
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum TypeFilter {
    #[default]
    All,
    Machines,
    Workspaces,
    Panes,
    Agents,
    Projects,
    Folders,
    Sessions,
    Actions,
}
impl TypeFilter {
    fn matches(self, entry: &Entry) -> bool {
        match self {
            Self::All => true,
            Self::Machines => entry.destination_kind() == DestinationKind::Machine,
            Self::Workspaces => entry.destination_kind() == DestinationKind::Workspace,
            Self::Panes => entry.destination_kind() == DestinationKind::Pane,
            Self::Agents => {
                entry.destination_kind() == DestinationKind::Pane && entry.agent.is_some()
            }
            Self::Projects => entry.destination_kind() == DestinationKind::Project,
            Self::Folders => entry.destination_kind() == DestinationKind::Folder,
            Self::Sessions => entry.destination_kind() == DestinationKind::Session,
            Self::Actions => entry.destination_kind() == DestinationKind::Action,
        }
    }
    fn non_live(self) -> bool {
        matches!(
            self,
            Self::Projects | Self::Folders | Self::Sessions | Self::Actions
        )
    }
}
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct Filters {
    pub(crate) machine: MachineFilter,
    pub(crate) kind: TypeFilter,
    pub(crate) provenance: Option<Source>,
}
impl Filters {
    fn matches(&self, destination: &Destination) -> bool {
        let owner_matches = match &self.machine {
            MachineFilter::All => true,
            MachineFilter::Machine(key) => {
                matches!(&destination.owner, Owner::Machine(owner) if owner == key)
            }
        };
        destination.eligible
            && owner_matches
            && self.kind.matches(&destination.entry)
            && self
                .provenance
                .is_none_or(|source| destination.entry.has_provenance(source))
    }
}

#[derive(Clone, Debug)]
pub(crate) struct PickerState {
    pub(crate) filters: Filters,
    pub(crate) expanded: HashSet<RowId>,
    pub(crate) browse: Position,
    pub(crate) flat: Position,
    pub(crate) current: Option<EntryIdentity>,
}
impl PickerState {
    /// Initialize once per picker invocation, not once per query or refresh.
    pub(crate) fn new(input: &PickerInput) -> Self {
        let mut state = Self {
            filters: Filters::default(),
            expanded: input.headings.iter().map(Heading::id).collect(),
            browse: Position::default(),
            flat: Position::default(),
            current: input.current.clone(),
        };
        if let Some(current) = input.current.as_ref().and_then(|id| input.destination(id)) {
            state.expanded.extend(input.ancestry(current));
            if matches!(
                current.entry.destination_kind(),
                DestinationKind::Workspace | DestinationKind::Tab
            ) {
                state
                    .expanded
                    .insert(RowId::Destination(current.entry.key().clone()));
            }
            if current.eligible && current.entry.destination_kind() == DestinationKind::Pane {
                let row = RowId::Destination(current.entry.key().clone());
                state.browse.selection = Some(row.clone());
                state.browse.viewport = Some(ViewportAnchor {
                    row,
                    intra_row_offset: 0,
                });
            }
        }
        state
    }
    pub(crate) fn set_machine(&mut self, machine: MachineFilter) {
        self.filters.machine = machine;
    }
    pub(crate) fn set_type(&mut self, kind: TypeFilter) {
        self.filters.kind = kind;
    }
    pub(crate) fn set_provenance(&mut self, source: Option<Source>) {
        self.filters.provenance = source;
    }
    pub(crate) fn reset_filters(&mut self) {
        self.filters = Filters::default();
    }
    pub(crate) fn toggle(&mut self, id: RowId) {
        if !self.expanded.remove(&id) {
            self.expanded.insert(id);
        }
    }
}
/// Internal N08 projection request. Search derives eligibility and order above it.
pub(crate) struct Request<'a> {
    pub(crate) query: &'a str,
    pub(crate) non_live_type: bool,
    pub(crate) eligible: &'a [EntryIdentity],
}
#[derive(Debug)]
pub(crate) struct Row<'a> {
    pub(crate) id: RowId,
    pub(crate) label: &'a str,
    /// Logical ancestry includes compressed tabs. Depth is only presentation.
    pub(crate) ancestors: Vec<RowId>,
    pub(crate) depth: usize,
    pub(crate) destination: Option<&'a Destination>,
    pub(crate) disclosure: bool,
    pub(crate) current: bool,
    pub(crate) pinned: bool,
    pub(crate) previous: bool,
}
impl Row<'_> {
    pub(crate) fn action(&self) -> Option<&ResolvedAction> {
        self.destination.map(|d| &d.action)
    }
}
#[derive(Debug)]
pub(crate) struct Projection<'a> {
    pub(crate) rows: Vec<Row<'a>>,
    pub(crate) flat: bool,
    /// Hidden or vanished references have no displayed replacement. Saved state is untouched.
    pub(crate) selection: Option<RowId>,
    pub(crate) viewport: Option<ViewportAnchor>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum FieldClass {
    Supporting,
    Inherited,
    Direct,
}
struct SearchField {
    class: FieldClass,
    text: String,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
struct FieldMatch {
    class: FieldClass,
    quality: u8,
    score: i64,
}

/// Qualified lookups are built once per search, never from a joined breadcrumb.
struct SearchIndex<'a> {
    live: HashMap<&'a LiveIdentity, &'a Entry>,
    owners: HashMap<&'a Owner, &'a str>,
}
impl<'a> SearchIndex<'a> {
    fn new(input: &'a PickerInput) -> Self {
        Self {
            live: input
                .destinations
                .iter()
                .filter_map(|d| d.entry.target().map(|target| (target.identity(), &d.entry)))
                .collect(),
            owners: input
                .headings
                .iter()
                .map(|h| (&h.owner, h.label.as_str()))
                .collect(),
        }
    }
    fn fields(&self, destination: &Destination) -> Vec<SearchField> {
        let entry = &destination.entry;
        let mut fields = Vec::new();
        let mut push = |class, text: &str| {
            if !text.is_empty() {
                fields.push(SearchField {
                    class,
                    text: text.to_lowercase(),
                });
            }
        };
        push(FieldClass::Direct, &entry.title);
        if let Some(agent) = &entry.agent {
            for value in [
                agent.agent.as_deref(),
                agent.name.as_deref(),
                agent.display_agent.as_deref(),
            ]
            .into_iter()
            .flatten()
            .chain(agent.aliases.iter().map(String::as_str))
            {
                push(FieldClass::Direct, value);
            }
            push(FieldClass::Supporting, &agent.status);
            if let Some(session) = &agent.session {
                push(FieldClass::Supporting, &session.value);
            }
        }
        if let Some(target) = entry.target() {
            push(FieldClass::Supporting, target.identity().id());
            for parent in [target.workspace(), target.tab()].into_iter().flatten() {
                if let Some(parent) = self.live.get(parent) {
                    push(FieldClass::Inherited, &parent.title);
                }
            }
        } else {
            // Live search_terms contain copied ancestry. Only providers own these terms.
            for term in &entry.search_terms {
                push(FieldClass::Supporting, term);
            }
            if entry.destination_kind() == DestinationKind::Machine {
                push(FieldClass::Supporting, &entry.subtitle);
            }
        }
        if let Some(owner) = self.owners.get(&destination.owner) {
            push(FieldClass::Inherited, owner);
        }
        if let Some(path) = &entry.path {
            let text = path.text();
            push(FieldClass::Inherited, &text);
            for component in text.split('/') {
                push(FieldClass::Inherited, component);
            }
        }
        push(FieldClass::Supporting, entry.source_name());
        for provider in &entry.provenance {
            push(FieldClass::Supporting, provider.preference_parts().1);
        }
        if let EntryIdentity::Provider { provider, .. } = entry.key() {
            push(FieldClass::Supporting, provider.preference_parts().1);
        }
        fields
    }
}
fn best_field(
    matcher: &mut SearchMatcher,
    plain: &str,
    fields: Vec<SearchField>,
) -> Option<FieldMatch> {
    fields
        .into_iter()
        .filter_map(|field| {
            let score = matcher.score(&field.text)?;
            let quality = match_quality(&field.text, plain);
            Some(FieldMatch {
                class: field.class,
                quality,
                score,
            })
        })
        .max()
}
fn semantic_order(entry: &Entry) -> u8 {
    match entry.destination_kind() {
        DestinationKind::Workspace => 0,
        DestinationKind::Machine => 1,
        DestinationKind::Tab => 2,
        DestinationKind::Pane => 3,
        DestinationKind::Project => 4,
        DestinationKind::Folder => 5,
        DestinationKind::Session => 6,
        DestinationKind::Action => 7,
    }
}

/// Derive candidates and order without collection, transport, state changes or action commits.
pub(crate) fn search_project<'a>(
    input: &'a PickerInput,
    state: &PickerState,
    raw_query: &str,
    ranking: &PickerConfig,
) -> Projection<'a> {
    let query = Query::parse(raw_query);
    let index = SearchIndex::new(input);
    let mut matcher =
        (!query.plain.is_empty()).then(|| SearchMatcher::new(&ranking.engine, &query.plain));
    let ranks = ranking.source_ranks();
    let mut candidates = Vec::new();
    for (position, destination) in input.destinations.iter().enumerate() {
        if !state.filters.matches(destination) || !query.filters_match(&destination.entry) {
            continue;
        }
        let matched = match &mut matcher {
            Some(matcher) => match best_field(matcher, &query.plain, index.fields(destination)) {
                Some(matched) => Some(matched),
                None => continue,
            },
            None => None,
        };
        candidates.push((position, destination, matched));
    }
    if !raw_query.is_empty() || state.filters.kind.non_live() {
        candidates.sort_by(|(ai, a, am), (bi, b, bm)| {
            bm.cmp(am)
                .then_with(|| semantic_order(&a.entry).cmp(&semantic_order(&b.entry)))
                .then_with(|| {
                    input
                        .pinned
                        .contains(b.entry.key())
                        .cmp(&input.pinned.contains(a.entry.key()))
                })
                .then_with(|| {
                    ranking
                        .merged_preference(ranks[b.entry.source.index()])
                        .cmp(&ranking.merged_preference(ranks[a.entry.source.index()]))
                })
                .then_with(|| a.entry.title.cmp(&b.entry.title))
                .then_with(|| ai.cmp(bi))
        });
    }
    let eligible: Vec<_> = candidates
        .into_iter()
        .map(|(_, d, _)| d.entry.key().clone())
        .collect();
    project(
        input,
        state,
        Request {
            query: raw_query,
            non_live_type: state.filters.kind.non_live(),
            eligible: &eligible,
        },
    )
}

pub(crate) fn project<'a>(
    input: &'a PickerInput,
    state: &PickerState,
    request: Request<'_>,
) -> Projection<'a> {
    let flat = !request.query.is_empty() || request.non_live_type;
    let filtered = state.filters != Filters::default();
    let eligible: HashSet<_> = request.eligible.iter().collect();
    let enabled = |d: &Destination| d.eligible && eligible.contains(d.entry.key());
    let mut rows = Vec::new();
    if flat {
        let mut seen = HashSet::new();
        for id in request.eligible {
            if let Some(d) = input.destination(id).filter(|d| d.eligible) {
                if seen.insert(id) {
                    rows.push(destination_row(
                        input,
                        state,
                        d,
                        input.ancestry(d),
                        0,
                        false,
                        true,
                    ));
                }
            }
        }
    } else {
        // Keep structural ancestors only when an eligible descendant needs them.
        let mut needed = HashSet::new();
        for d in input.destinations.iter().filter(|d| enabled(d)) {
            needed.insert(RowId::Destination(d.entry.key().clone()));
            needed.extend(input.ancestry(d));
        }
        let mut headings: Vec<_> = input.headings.iter().collect();
        headings.sort_by_key(|h| !(h.local && matches!(h.owner, Owner::Machine(_))));
        for heading in headings {
            let root = heading.id();
            if !needed.contains(&root)
                && (filtered || !input.groups.iter().any(|(o, _)| o == &heading.owner))
            {
                continue;
            }
            let machine = heading
                .machine
                .as_ref()
                .and_then(|id| input.destination(id))
                .filter(|d| enabled(d));
            rows.push(Row {
                id: root.clone(),
                label: &heading.label,
                ancestors: Vec::new(),
                depth: 0,
                destination: machine,
                disclosure: true,
                current: machine.is_some_and(|d| state.current.as_ref() == Some(d.entry.key())),
                pinned: machine.is_some_and(|d| input.pinned.contains(d.entry.key())),
                previous: machine.is_some_and(|d| input.previous.as_ref() == Some(d.entry.key())),
            });
            if !state.expanded.contains(&root) {
                continue;
            }
            let owner_entries: Vec<_> = input
                .destinations
                .iter()
                .filter(|d| d.owner == heading.owner)
                .collect();
            for workspace in owner_entries
                .iter()
                .copied()
                .filter(|d| d.entry.destination_kind() == DestinationKind::Workspace)
            {
                let wid = RowId::Destination(workspace.entry.key().clone());
                if !needed.contains(&wid) {
                    continue;
                }
                rows.push(destination_row(
                    input,
                    state,
                    workspace,
                    vec![root.clone()],
                    1,
                    true,
                    enabled(workspace),
                ));
                if !state.expanded.contains(&wid) {
                    continue;
                }
                for tab in owner_entries.iter().copied().filter(|d| {
                    d.entry.target().is_some_and(|t| {
                        t.identity().kind() == ObjectKind::Tab
                            && workspace
                                .entry
                                .target()
                                .is_some_and(|w| t.workspace() == Some(w.identity()))
                    })
                }) {
                    let tid = RowId::Destination(tab.entry.key().clone());
                    if !needed.contains(&tid) {
                        continue;
                    }
                    let compressed = input.compressed(tab);
                    if !compressed {
                        rows.push(destination_row(
                            input,
                            state,
                            tab,
                            vec![root.clone(), wid.clone()],
                            2,
                            true,
                            enabled(tab),
                        ));
                        if !state.expanded.contains(&tid) {
                            continue;
                        }
                    }
                    for pane in owner_entries.iter().copied().filter(|d| {
                        enabled(d)
                            && d.entry.target().is_some_and(|p| {
                                tab.entry
                                    .target()
                                    .is_some_and(|t| p.tab() == Some(t.identity()))
                            })
                    }) {
                        rows.push(destination_row(
                            input,
                            state,
                            pane,
                            vec![root.clone(), wid.clone(), tid.clone()],
                            if compressed { 2 } else { 3 },
                            false,
                            true,
                        ));
                    }
                }
            }
            for kind in [
                TaskKind::ProjectsAndFolders,
                TaskKind::Sessions,
                TaskKind::Actions,
            ] {
                let group = RowId::TaskGroup(heading.owner.clone(), kind);
                let supported = input.groups.contains(&(heading.owner.clone(), kind));
                if !needed.contains(&group) && (filtered || !supported) {
                    continue;
                }
                rows.push(Row {
                    id: group.clone(),
                    label: kind.label(),
                    ancestors: vec![root.clone()],
                    depth: 1,
                    destination: None,
                    disclosure: true,
                    current: false,
                    pinned: false,
                    previous: false,
                });
                if state.expanded.contains(&group) {
                    for d in owner_entries
                        .iter()
                        .copied()
                        .filter(|d| enabled(d) && TaskKind::for_entry(&d.entry) == Some(kind))
                    {
                        rows.push(destination_row(
                            input,
                            state,
                            d,
                            vec![root.clone(), group.clone()],
                            2,
                            false,
                            true,
                        ));
                    }
                }
            }
        }
    }
    let position = if flat { &state.flat } else { &state.browse };
    let visible = |id: &RowId| rows.iter().any(|row| &row.id == id);
    let selection = position.selection.clone().filter(|id| {
        rows.iter()
            .any(|row| &row.id == id && (!filtered || row.destination.is_some()))
    });
    let viewport = position
        .viewport
        .clone()
        .filter(|anchor| visible(&anchor.row));
    Projection {
        rows,
        flat,
        selection,
        viewport,
    }
}
fn destination_row<'a>(
    input: &'a PickerInput,
    state: &PickerState,
    d: &'a Destination,
    ancestors: Vec<RowId>,
    depth: usize,
    disclosure: bool,
    eligible: bool,
) -> Row<'a> {
    Row {
        id: RowId::Destination(d.entry.key().clone()),
        label: &d.entry.title,
        ancestors,
        depth,
        destination: eligible.then_some(d),
        disclosure,
        current: eligible && state.current.as_ref() == Some(d.entry.key()),
        pinned: eligible && input.pinned.contains(d.entry.key()),
        previous: eligible && input.previous.as_ref() == Some(d.entry.key()),
    }
}
