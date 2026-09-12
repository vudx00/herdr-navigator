use std::hash::{DefaultHasher, Hash, Hasher};

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use crate::{
    actions::Slot,
    config::PickerConfig,
    keymap::{self, Command},
    model::{EntryIdentity, MachineKey, Source},
    picker::{MachineFilter, PickerState, Projection, RowId, TypeFilter},
};

#[cfg(test)]
mod tests;

pub(crate) use crate::keymap::{Focus, Presentation, VimState};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Details {
    pub(crate) visible: bool,
    pub(crate) technical: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Menu {
    Machine {
        options: Vec<MachineFilter>,
        pending: usize,
    },
    Type {
        options: Vec<TypeFilter>,
        pending: usize,
    },
}

#[derive(Clone, Debug)]
struct HelpReturn {
    focus: Focus,
    vim: VimState,
    menu: Option<Menu>,
    details: Details,
    cursor: usize,
    selection: Option<RowId>,
}

#[derive(Clone, Debug)]
pub(crate) struct State {
    presentation: Presentation,
    pub(crate) cursor: usize,
    pub(crate) focus: Focus,
    pub(crate) vim: VimState,
    pub(crate) menu: Option<Menu>,
    pub(crate) details: Details,
    help: Option<HelpReturn>,
    query_fingerprint: u64,
}

impl State {
    pub(crate) fn legacy(config: &PickerConfig, query: &str) -> Self {
        Self {
            presentation: Presentation::Legacy,
            cursor: query.len(),
            focus: Focus::Query,
            vim: VimState::Normal,
            menu: None,
            details: Details {
                visible: config.preview,
                technical: false,
            },
            help: None,
            query_fingerprint: query_fingerprint(query),
        }
    }

    pub(crate) fn merged(config: &PickerConfig, query: &str) -> Self {
        Self {
            presentation: Presentation::Merged,
            cursor: query.len(),
            focus: Focus::Query,
            vim: VimState::Search,
            menu: None,
            details: Details {
                visible: config.preview,
                technical: false,
            },
            help: None,
            query_fingerprint: query_fingerprint(query),
        }
    }

    pub(crate) fn presentation(&self) -> Presentation {
        self.presentation
    }

    pub(crate) fn help_visible(&self) -> bool {
        self.help.is_some()
    }

    pub(crate) fn reconcile_query(&mut self, query: &str) {
        let fingerprint = query_fingerprint(query);
        if fingerprint != self.query_fingerprint
            || self.cursor > query.len()
            || !query.is_char_boundary(self.cursor)
        {
            self.cursor = query.len();
        }
        self.query_fingerprint = fingerprint;
    }

    fn observe_query(&mut self, query: &str) {
        self.query_fingerprint = query_fingerprint(query);
    }

    pub(crate) fn descriptor(&self) -> Descriptor {
        let key_context = self.help.as_ref().map_or(
            keymap::InputContext {
                presentation: self.presentation,
                focus: self.focus,
                vim: self.vim,
                menu: self.menu.is_some(),
            },
            |saved| keymap::InputContext {
                presentation: self.presentation,
                focus: saved.focus,
                vim: saved.vim,
                menu: saved.menu.is_some(),
            },
        );
        Descriptor {
            presentation: self.presentation,
            cursor: self.cursor,
            focus: self.focus,
            vim: self.vim,
            help: self.help.is_some(),
            menu: self.menu.clone(),
            details: self.details,
            key_context,
        }
    }
}

#[derive(Clone, Debug)]
pub(crate) struct Descriptor {
    pub(crate) presentation: Presentation,
    pub(crate) cursor: usize,
    pub(crate) focus: Focus,
    pub(crate) vim: VimState,
    pub(crate) help: bool,
    pub(crate) menu: Option<Menu>,
    pub(crate) details: Details,
    pub(crate) key_context: keymap::InputContext,
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct VisibleRegions {
    pub(crate) browse: bool,
    pub(crate) machine_filter: bool,
    pub(crate) type_filter: bool,
    pub(crate) details: bool,
}

impl VisibleRegions {
    pub(crate) fn legacy(details: bool) -> Self {
        Self {
            browse: true,
            machine_filter: false,
            type_filter: false,
            details,
        }
    }

    pub(crate) fn merged(details: bool) -> Self {
        Self {
            browse: true,
            machine_filter: true,
            type_filter: true,
            details,
        }
    }
}

#[derive(Clone, Debug)]
pub(crate) struct RowView {
    pub(crate) id: RowId,
    pub(crate) ancestors: Vec<RowId>,
    pub(crate) disclosure: bool,
    pub(crate) expanded: bool,
    pub(crate) action: Option<SelectedAction>,
}

#[derive(Clone, Debug, Default)]
pub(crate) struct Rows {
    pub(crate) rows: Vec<RowView>,
    pub(crate) selection: Option<RowId>,
    pub(crate) flat: bool,
}

impl Rows {
    pub(crate) fn from_projection(projection: &Projection<'_>, state: &PickerState) -> Self {
        Self {
            rows: projection
                .rows
                .iter()
                .map(|row| RowView {
                    id: row.id.clone(),
                    ancestors: row.ancestors.clone(),
                    disclosure: row.disclosure,
                    expanded: state.expanded.contains(&row.id),
                    action: row.action().map(|action| SelectedAction {
                        identity: action.identity.clone(),
                        unavailable: action.unavailable.clone(),
                    }),
                })
                .collect(),
            selection: projection.selection.clone(),
            flat: projection.flat,
        }
    }
}

#[derive(Clone, Debug)]
pub(crate) struct SelectedAction {
    pub(crate) identity: EntryIdentity,
    pub(crate) unavailable: Option<String>,
}

#[derive(Clone, Debug, Default)]
pub(crate) struct ActionAvailability {
    pub(crate) selected: Option<SelectedAction>,
    pub(crate) template: bool,
    pub(crate) close: bool,
    pub(crate) update: bool,
    pub(crate) reject_unavailable: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ActivationIntent {
    pub(crate) identity: EntryIdentity,
    pub(crate) slot: Slot,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Effect {
    Quit,
    QueryChanged,
    MoveLegacy(i8),
    SetLegacyFilter(Option<Source>),
    CycleLegacyFilter,
    Activate(ActivationIntent),
    Rejected(String),
    OpenTemplate(ActivationIntent),
    ToggleMark,
    CloseWorkspace,
    Update,
    Guidance(String),
}

pub(crate) struct Context<'a> {
    pub(crate) config: &'a PickerConfig,
    pub(crate) visible: VisibleRegions,
    pub(crate) rows: &'a Rows,
    pub(crate) actions: ActionAvailability,
    pub(crate) legacy_filter: Option<Source>,
    pub(crate) machine_options: &'a [MachineKey],
}

fn registry_availability(actions: &ActionAvailability) -> keymap::Availability {
    keymap::Availability {
        template: actions.template,
        update: actions.update,
        close: actions.close,
    }
}

pub(crate) fn reduce(
    state: &mut State,
    query: &mut String,
    mut picker: Option<&mut PickerState>,
    event: KeyEvent,
    context: Context<'_>,
) -> Vec<Effect> {
    state.reconcile_query(query);

    if state.help.is_some() {
        if event.modifiers == KeyModifiers::CONTROL && event.code == KeyCode::Char('c') {
            return vec![Effect::Quit];
        }
        if (event.code == KeyCode::Esc && event.modifiers.is_empty())
            || (event.code == KeyCode::Char('?') && text_modifiers(event.modifiers))
        {
            if let Some(saved) = state.help.take() {
                if let (Some(picker), Some(selection)) =
                    (picker.as_deref_mut(), saved.selection.clone())
                {
                    set_selection(picker, context.rows.flat, selection);
                }
                state.focus = saved.focus;
                state.vim = saved.vim;
                state.menu = saved.menu;
                state.details = saved.details;
                state.cursor = saved.cursor;
            }
        }
        return Vec::new();
    }

    let command = keymap::resolve(
        context.config,
        keymap::InputContext {
            presentation: state.presentation,
            focus: state.focus,
            vim: state.vim,
            menu: state.menu.is_some(),
        },
        registry_availability(&context.actions),
        event,
    );

    if state.menu.is_some() {
        match command {
            Some(Command::Back) if event.code == KeyCode::Char('c') => {
                return vec![Effect::Quit];
            }
            Some(Command::Back) => state.menu = None,
            Some(Command::FocusCycle) => {
                state.menu = None;
                cycle_focus(state, context.visible);
            }
            Some(Command::MoveUp) => {
                if let Some(menu) = state.menu.as_mut() {
                    move_menu(menu, -1);
                }
            }
            Some(Command::MoveDown) => {
                if let Some(menu) = state.menu.as_mut() {
                    move_menu(menu, 1);
                }
            }
            Some(Command::Open) => {
                if let Some(menu) = state.menu.as_ref() {
                    if let Some(picker) = picker.as_deref_mut() {
                        commit_menu(menu, picker);
                    }
                }
                state.menu = None;
            }
            Some(Command::ToggleHelp) => {
                return execute(state, query, picker, Command::ToggleHelp, event, context);
            }
            None => {
                if let KeyCode::Char(ch) = event.code {
                    if text_modifiers(event.modifiers) {
                        state.menu = None;
                        state.focus = Focus::Query;
                        state.vim = VimState::Search;
                        insert(query, &mut state.cursor, ch);
                        state.observe_query(query);
                        return vec![Effect::QueryChanged];
                    }
                }
            }
            Some(_) => {}
        }
        return Vec::new();
    }

    if command == Some(Command::Back) && event.code == KeyCode::Char('c') {
        return vec![Effect::Quit];
    }
    if command == Some(Command::Back) {
        if state.details.visible {
            state.details.visible = false;
            state.details.technical = false;
            if state.focus == Focus::Details {
                state.focus = Focus::Query;
            }
            return Vec::new();
        }
        if state.vim == VimState::Search {
            state.vim = VimState::Normal;
            return Vec::new();
        }
        return vec![Effect::Quit];
    }

    if command == Some(Command::Open) {
        match state.focus {
            Focus::MachineFilter => {
                let current = picker
                    .as_deref()
                    .map(|picker| picker.filters.machine.clone())
                    .unwrap_or_default();
                let mut options = vec![MachineFilter::All];
                options.extend(
                    context
                        .machine_options
                        .iter()
                        .cloned()
                        .map(MachineFilter::Machine),
                );
                let pending = options
                    .iter()
                    .position(|item| item == &current)
                    .unwrap_or(0);
                state.menu = Some(Menu::Machine { options, pending });
                return Vec::new();
            }
            Focus::TypeFilter => {
                let options = type_options();
                let current = picker
                    .as_deref()
                    .map(|picker| picker.filters.kind)
                    .unwrap_or_default();
                let pending = options
                    .iter()
                    .position(|item| *item == current)
                    .unwrap_or(0);
                state.menu = Some(Menu::Type { options, pending });
                return Vec::new();
            }
            _ => {}
        }
    }

    if let Some(command) = command {
        return execute(state, query, picker, command, event, context);
    }

    if context.config.vim_mode && state.vim == VimState::Normal {
        if state.presentation == Presentation::Legacy {
            return Vec::new();
        }
        if let KeyCode::Char(ch) = event.code {
            if text_modifiers(event.modifiers) {
                state.vim = VimState::Search;
                state.focus = Focus::Query;
                insert(query, &mut state.cursor, ch);
                state.observe_query(query);
                return vec![Effect::QueryChanged];
            }
        }
        return Vec::new();
    }

    if let KeyCode::Char(ch) = event.code {
        if text_modifiers(event.modifiers) {
            state.focus = Focus::Query;
            state.vim = VimState::Search;
            insert(query, &mut state.cursor, ch);
            state.observe_query(query);
            return vec![Effect::QueryChanged];
        }
    }
    Vec::new()
}

fn execute(
    state: &mut State,
    query: &mut String,
    picker: Option<&mut PickerState>,
    command: Command,
    event: KeyEvent,
    context: Context<'_>,
) -> Vec<Effect> {
    match command {
        Command::Back => vec![Effect::Quit],
        Command::Open => activate(context.rows, &context.actions, Slot::Primary),
        Command::OpenTemplate => activate(context.rows, &context.actions, Slot::Template),
        Command::MoveUp => move_selection(state, picker, context.rows, -1),
        Command::MoveDown => move_selection(state, picker, context.rows, 1),
        Command::StartSearch => {
            if state.presentation == Presentation::Legacy {
                query.clear();
                state.cursor = 0;
            }
            state.vim = VimState::Search;
            state.focus = Focus::Query;
            state.observe_query(query);
            vec![Effect::QueryChanged]
        }
        Command::CycleFilter => {
            if let Some(picker) = picker {
                let all = Source::all();
                let next = picker
                    .filters
                    .provenance
                    .and_then(|current| {
                        all.iter()
                            .position(|source| *source == current)
                            .and_then(|index| all.get(index + 1).copied())
                    })
                    .or_else(|| {
                        picker
                            .filters
                            .provenance
                            .is_none()
                            .then_some(Source::Workspace)
                    });
                picker.set_provenance(next);
                Vec::new()
            } else {
                vec![Effect::CycleLegacyFilter]
            }
        }
        Command::CursorLeft => {
            move_cursor_left(query, &mut state.cursor);
            Vec::new()
        }
        Command::CursorRight => {
            move_cursor_right(query, &mut state.cursor);
            Vec::new()
        }
        Command::CursorHome => {
            state.cursor = 0;
            Vec::new()
        }
        Command::CursorEnd => {
            state.cursor = query.len();
            Vec::new()
        }
        Command::DeleteChar => {
            if backspace(query, &mut state.cursor) {
                state.observe_query(query);
                vec![Effect::QueryChanged]
            } else {
                Vec::new()
            }
        }
        Command::Delete => {
            if delete(query, state.cursor) {
                state.observe_query(query);
                vec![Effect::QueryChanged]
            } else {
                Vec::new()
            }
        }
        Command::Clear => {
            query.clear();
            state.cursor = 0;
            state.menu = None;
            state.details.visible = false;
            state.details.technical = false;
            state.focus = Focus::Query;
            state.observe_query(query);
            if state.presentation == Presentation::Merged {
                state.vim = VimState::Search;
                if let Some(picker) = picker {
                    picker.reset_filters();
                }
                vec![Effect::QueryChanged]
            } else {
                state.vim = VimState::Normal;
                vec![Effect::SetLegacyFilter(None), Effect::QueryChanged]
            }
        }
        Command::CloseWorkspace => vec![Effect::CloseWorkspace],
        Command::ToggleMark => vec![Effect::ToggleMark],
        Command::ToggleDetails => {
            state.details.visible = !state.details.visible;
            if !state.details.visible {
                state.details.technical = false;
                if state.focus == Focus::Details {
                    state.focus = Focus::Query;
                }
            }
            Vec::new()
        }
        Command::ToggleHelp => {
            state.help = Some(HelpReturn {
                focus: state.focus,
                vim: state.vim,
                menu: state.menu.clone(),
                details: state.details,
                cursor: state.cursor,
                selection: context.rows.selection.clone(),
            });
            Vec::new()
        }
        Command::Update => vec![Effect::Update],
        Command::Filter(source) => {
            let vim_filter = context.config.vim_mode
                && state.vim == VimState::Normal
                && event.modifiers == KeyModifiers::NONE;
            if let Some(picker) = picker {
                let next = (picker.filters.provenance != Some(source)).then_some(source);
                picker.set_provenance(next);
                if vim_filter {
                    state.vim = if context.config.vim_filter_search {
                        VimState::Search
                    } else {
                        VimState::Normal
                    };
                }
                Vec::new()
            } else {
                if vim_filter {
                    query.clear();
                    state.cursor = 0;
                    state.vim = if context.config.vim_filter_search {
                        VimState::Search
                    } else {
                        VimState::Normal
                    };
                    state.observe_query(query);
                }
                let next = (context.legacy_filter != Some(source)).then_some(source);
                vec![Effect::SetLegacyFilter(next), Effect::QueryChanged]
            }
        }
        Command::FocusCycle => {
            cycle_focus(state, context.visible);
            Vec::new()
        }
        Command::Collapse => collapse(picker, context.rows),
        Command::Expand => expand(picker, context.rows),
        Command::TechnicalDetails => {
            state.details.visible = true;
            state.details.technical = !state.details.technical;
            Vec::new()
        }
    }
}

fn activate(rows: &Rows, actions: &ActionAvailability, slot: Slot) -> Vec<Effect> {
    let row_action = rows
        .selection
        .as_ref()
        .and_then(|id| rows.rows.iter().find(|row| &row.id == id))
        .and_then(|row| row.action.as_ref());
    let selected = match (row_action, actions.selected.as_ref()) {
        (Some(row), Some(available)) if row.identity == available.identity => Some(available),
        (Some(row), None) => Some(row),
        _ => None,
    };
    let Some(selected) = selected else {
        return if rows.selection.is_some() {
            vec![Effect::Guidance(
                "Use disclosure keys to expand this group".into(),
            )]
        } else {
            vec![Effect::Rejected("nothing selected".into())]
        };
    };
    if slot == Slot::Template && !actions.template {
        return vec![Effect::Rejected("template action is unavailable".into())];
    }
    if actions.reject_unavailable {
        if let Some(reason) = &selected.unavailable {
            return vec![Effect::Rejected(reason.clone())];
        }
    }
    let intent = ActivationIntent {
        identity: selected.identity.clone(),
        slot,
    };
    if slot == Slot::Template {
        vec![Effect::OpenTemplate(intent)]
    } else {
        vec![Effect::Activate(intent)]
    }
}

fn move_selection(
    state: &State,
    picker: Option<&mut PickerState>,
    rows: &Rows,
    delta: i8,
) -> Vec<Effect> {
    let Some(picker) = picker else {
        return vec![Effect::MoveLegacy(delta)];
    };
    if rows.rows.is_empty() {
        return Vec::new();
    }
    let current = rows
        .selection
        .as_ref()
        .and_then(|selected| rows.rows.iter().position(|row| &row.id == selected));
    let index = match (current, delta.is_negative()) {
        (Some(index), true) => index.saturating_sub(1),
        (Some(index), false) => (index + 1).min(rows.rows.len() - 1),
        (None, true) => rows.rows.len() - 1,
        (None, false) => 0,
    };
    set_selection(picker, rows.flat, rows.rows[index].id.clone());
    debug_assert!(matches!(
        state.focus,
        Focus::Query | Focus::Browse | Focus::MachineFilter | Focus::TypeFilter | Focus::Details
    ));
    Vec::new()
}

fn collapse(picker: Option<&mut PickerState>, rows: &Rows) -> Vec<Effect> {
    let Some(picker) = picker else {
        return Vec::new();
    };
    let Some(selected) = rows.selection.as_ref() else {
        return Vec::new();
    };
    let Some(row) = rows.rows.iter().find(|row| &row.id == selected) else {
        return Vec::new();
    };
    if row.disclosure && row.expanded {
        picker.expanded.remove(selected);
        return Vec::new();
    }
    if let Some(parent) = row
        .ancestors
        .iter()
        .rev()
        .find(|parent| rows.rows.iter().any(|row| &row.id == *parent))
    {
        set_selection(picker, rows.flat, parent.clone());
    }
    Vec::new()
}

fn expand(picker: Option<&mut PickerState>, rows: &Rows) -> Vec<Effect> {
    let Some(picker) = picker else {
        return Vec::new();
    };
    let Some(selected) = rows.selection.as_ref() else {
        return Vec::new();
    };
    if rows
        .rows
        .iter()
        .any(|row| &row.id == selected && row.disclosure && !row.expanded)
    {
        picker.expanded.insert(selected.clone());
    }
    Vec::new()
}

fn set_selection(picker: &mut PickerState, flat: bool, id: RowId) {
    let position = if flat {
        &mut picker.flat
    } else {
        &mut picker.browse
    };
    position.selection = Some(id);
}

fn cycle_focus(state: &mut State, visible: VisibleRegions) {
    let mut regions = vec![Focus::Query];
    if visible.browse {
        regions.push(Focus::Browse);
    }
    if visible.machine_filter {
        regions.push(Focus::MachineFilter);
    }
    if visible.type_filter {
        regions.push(Focus::TypeFilter);
    }
    if visible.details && state.details.visible {
        regions.push(Focus::Details);
    }
    let next = regions
        .iter()
        .position(|focus| *focus == state.focus)
        .map_or(0, |index| (index + 1) % regions.len());
    state.focus = regions[next];
}

fn move_menu(menu: &mut Menu, delta: i8) {
    let (length, pending) = match menu {
        Menu::Machine { options, pending } => (options.len(), pending),
        Menu::Type { options, pending } => (options.len(), pending),
    };
    if length == 0 {
        return;
    }
    *pending = if delta.is_negative() {
        pending.saturating_sub(1)
    } else {
        (*pending + 1).min(length - 1)
    };
}

fn commit_menu(menu: &Menu, picker: &mut PickerState) {
    match menu {
        Menu::Machine { options, pending } => {
            if let Some(filter) = options.get(*pending) {
                picker.set_machine(filter.clone());
            }
        }
        Menu::Type { options, pending } => {
            if let Some(filter) = options.get(*pending) {
                picker.set_type(*filter);
            }
        }
    }
}

fn type_options() -> Vec<TypeFilter> {
    vec![
        TypeFilter::All,
        TypeFilter::Machines,
        TypeFilter::Workspaces,
        TypeFilter::Panes,
        TypeFilter::Agents,
        TypeFilter::Projects,
        TypeFilter::Folders,
        TypeFilter::Sessions,
        TypeFilter::Actions,
    ]
}

fn text_modifiers(modifiers: KeyModifiers) -> bool {
    modifiers.is_empty() || modifiers == KeyModifiers::SHIFT
}

fn query_fingerprint(query: &str) -> u64 {
    let mut hasher = DefaultHasher::new();
    query.hash(&mut hasher);
    hasher.finish()
}

fn insert(query: &mut String, cursor: &mut usize, ch: char) {
    query.insert(*cursor, ch);
    *cursor += ch.len_utf8();
}

fn move_cursor_left(query: &str, cursor: &mut usize) {
    if *cursor == 0 {
        return;
    }
    *cursor = query[..*cursor]
        .char_indices()
        .next_back()
        .map_or(0, |(index, _)| index);
}

fn move_cursor_right(query: &str, cursor: &mut usize) {
    if *cursor >= query.len() {
        return;
    }
    *cursor += query[*cursor..].chars().next().map_or(0, char::len_utf8);
}

fn backspace(query: &mut String, cursor: &mut usize) -> bool {
    if *cursor == 0 {
        return false;
    }
    let previous = query[..*cursor]
        .char_indices()
        .next_back()
        .map_or(0, |(index, _)| index);
    query.drain(previous..*cursor);
    *cursor = previous;
    true
}

fn delete(query: &mut String, cursor: usize) -> bool {
    let Some(ch) = query[cursor..].chars().next() else {
        return false;
    };
    query.drain(cursor..cursor + ch.len_utf8());
    true
}
