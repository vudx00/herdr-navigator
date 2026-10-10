use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use super::*;
use crate::{
    navigation_test_support::FixtureClient,
    picker::{search_project, Owner},
};

fn key(code: KeyCode) -> KeyEvent {
    KeyEvent::new(code, KeyModifiers::NONE)
}

fn ctrl(ch: char) -> KeyEvent {
    KeyEvent::new(KeyCode::Char(ch), KeyModifiers::CONTROL)
}

struct Harness {
    config: PickerConfig,
    input: crate::picker::PickerInput,
    picker: PickerState,
    state: State,
    query: String,
    template: bool,
    close: bool,
    update: bool,
}

#[derive(Debug, PartialEq, Eq)]
struct Snapshot {
    state: String,
    picker: String,
    query: String,
}

impl Harness {
    fn new() -> Self {
        let input = FixtureClient::sample().picker_input();
        let config = PickerConfig {
            preview: false,
            ..PickerConfig::default()
        };
        let picker = PickerState::new(&input);
        Self {
            state: State::merged(&config, ""),
            config,
            input,
            picker,
            query: String::new(),
            template: false,
            close: false,
            update: false,
        }
    }

    fn snapshot(&self) -> Snapshot {
        Snapshot {
            state: format!("{:?}", self.state),
            picker: format!("{:?}", self.picker),
            query: self.query.clone(),
        }
    }

    fn assert_ignored(&mut self, event: KeyEvent) {
        self.state.reconcile_query(&self.query);
        let before = self.snapshot();
        assert!(
            self.press(event).is_empty(),
            "unexpected effect for {event:?}"
        );
        assert_eq!(self.snapshot(), before, "state changed for {event:?}");
    }

    fn arm_merged_candidate(&mut self, command: &Command, value: &str) -> EntryIdentity {
        self.config.focus_cycle_key = "f9".into();
        self.config.collapse_key = "f10".into();
        self.config.expand_key = "f11".into();
        self.config.technical_details_key = "f12".into();
        match command {
            Command::FocusCycle => self.config.focus_cycle_key = value.into(),
            Command::Collapse => self.config.collapse_key = value.into(),
            Command::Expand => self.config.expand_key = value.into(),
            Command::TechnicalDetails => self.config.technical_details_key = value.into(),
            _ => panic!("not a merged command"),
        }
        self.state.focus = Focus::Browse;
        self.state.menu = None;
        self.state.help = None;
        self.state.details = Details {
            visible: false,
            technical: false,
        };
        let row = self
            .rows()
            .rows
            .into_iter()
            .find(|row| row.disclosure && row.action.is_some())
            .expect("fixture has a disclosure row with an action");
        let identity = row.action.unwrap().identity;
        self.input
            .destinations
            .iter_mut()
            .find(|destination| destination.entry.key() == &identity)
            .expect("displayed action belongs to fixture input")
            .action
            .unavailable = None;
        self.picker.browse.selection = Some(row.id.clone());
        if command == &Command::Collapse {
            self.picker.expanded.insert(row.id);
        } else if command == &Command::Expand {
            self.picker.expanded.remove(&row.id);
        }
        identity
    }

    fn rows(&self) -> Rows {
        let projection = search_project(&self.input, &self.picker, &self.query, &self.config);
        Rows::from_projection(&projection, &self.picker)
    }

    fn press(&mut self, event: KeyEvent) -> Vec<Effect> {
        let rows = self.rows();
        let selected = rows
            .selection
            .as_ref()
            .and_then(|id| rows.rows.iter().find(|row| &row.id == id))
            .and_then(|row| row.action.clone());
        let machines: Vec<_> = self
            .input
            .headings
            .iter()
            .filter_map(|heading| match &heading.owner {
                Owner::Machine(machine) => Some(machine.clone()),
                _ => None,
            })
            .collect();
        let visible = VisibleRegions::merged(self.state.details.visible);
        reduce(
            &mut self.state,
            &mut self.query,
            Some(&mut self.picker),
            event,
            Context {
                config: &self.config,
                visible,
                rows: &rows,
                actions: ActionAvailability {
                    selected,
                    template: self.template,
                    close: self.close,
                    update: self.update,
                    reject_unavailable: true,
                },
                legacy_filter: None,
                machine_options: &machines,
            },
        )
    }
}

#[test]
fn query_cursor_edits_unicode_scalars_and_reconciles_replacement() {
    let mut harness = Harness::new();
    for ch in ['a', ' ', 'é', '\u{301}', '🙂'] {
        assert_eq!(
            harness.press(key(KeyCode::Char(ch))),
            [Effect::QueryChanged]
        );
        assert!(harness.query.is_char_boundary(harness.state.cursor));
    }
    assert_eq!(harness.query, "a é\u{301}🙂");
    harness.press(key(KeyCode::Left));
    harness.press(key(KeyCode::Backspace));
    assert_eq!(harness.query, "a é🙂");
    assert_eq!(harness.state.cursor, "a é".len());
    harness.press(key(KeyCode::Delete));
    assert_eq!(harness.query, "a é");
    harness.press(key(KeyCode::Home));
    harness.press(key(KeyCode::Char('中')));
    harness.press(key(KeyCode::End));
    harness.press(key(KeyCode::Backspace));
    assert_eq!(harness.query, "中a ");
    assert!(harness.query.is_char_boundary(harness.state.cursor));

    harness.query = "replacement🙂".into();
    harness.state.reconcile_query(&harness.query);
    assert_eq!(harness.state.cursor, harness.query.len());
}

#[test]
fn query_editing_registry_runs_in_normal_and_search_states() {
    for vim in [VimState::Normal, VimState::Search] {
        let mut harness = Harness::new();
        harness.query = "é\u{301}🙂z".into();
        harness.state.reconcile_query(&harness.query);
        harness.state.vim = vim;
        harness.press(key(KeyCode::Home));
        harness.press(key(KeyCode::Right));
        harness.press(key(KeyCode::Delete));
        assert_eq!(harness.query, "é🙂z");
        harness.press(key(KeyCode::End));
        harness.press(key(KeyCode::Left));
        harness.press(key(KeyCode::Backspace));
        assert_eq!(harness.query, "éz");
        assert!(harness.query.is_char_boundary(harness.state.cursor));
    }
}

#[test]
fn movement_keeps_focus_and_browse_disclosure_uses_visible_parent() {
    let mut harness = Harness::new();
    let initial = harness.rows().selection.unwrap();
    harness.state.focus = Focus::Query;
    harness.press(key(KeyCode::Up));
    assert_eq!(harness.state.focus, Focus::Query);
    assert_ne!(harness.rows().selection, Some(initial));

    harness.state.focus = Focus::Browse;
    let rows = harness.rows();
    let pane = rows
        .rows
        .iter()
        .find(|row| row.action.is_some() && !row.ancestors.is_empty())
        .unwrap();
    harness.picker.browse.selection = Some(pane.id.clone());
    let visible_parent = pane
        .ancestors
        .iter()
        .rev()
        .find(|parent| rows.rows.iter().any(|row| &row.id == *parent))
        .unwrap()
        .clone();
    harness.press(key(KeyCode::Left));
    assert_eq!(
        harness.picker.browse.selection,
        Some(visible_parent.clone())
    );

    harness.picker.expanded.remove(&visible_parent);
    harness.press(key(KeyCode::Right));
    assert!(harness.picker.expanded.contains(&visible_parent));
    harness.press(key(KeyCode::Char(' ')));
    assert_eq!(harness.state.focus, Focus::Query);
    assert_eq!(harness.query, " ");
}

#[test]
fn n10_host_prefix_blocks_only_each_new_merged_command_without_state_changes() {
    let cases = [
        ("focus", 'g'),
        ("collapse", 'h'),
        ("expand", 'i'),
        ("technical", 'j'),
    ];

    for (command, prefix) in cases {
        let mut config = crate::config::Config::default();
        match command {
            "focus" => config.picker.focus_cycle_key = format!("CTRL+{prefix}"),
            "collapse" => config.picker.collapse_key = format!("ctrl-{prefix}"),
            "expand" => config.picker.expand_key = format!("^{prefix}"),
            "technical" => config.picker.technical_details_key = format!("⌃{prefix}"),
            _ => unreachable!(),
        }
        let diagnostics = config.avoid_default_prefix_conflict(prefix.to_ascii_uppercase());

        let mut harness = Harness::new();
        harness.config = config.picker;
        harness.state.reconcile_query(&harness.query);
        harness.state.focus = Focus::Browse;
        harness.state.details = Details {
            visible: true,
            technical: false,
        };

        if matches!(command, "collapse" | "expand") {
            let row = harness
                .rows()
                .rows
                .into_iter()
                .find(|row| row.disclosure)
                .expect("fixture has a disclosure row");
            harness.picker.browse.selection = Some(row.id.clone());
            if command == "collapse" {
                harness.picker.expanded.insert(row.id);
            } else {
                harness.picker.expanded.remove(&row.id);
            }
        }

        harness.assert_ignored(ctrl(prefix));
        assert!(
            diagnostics
                .iter()
                .any(|line| line.contains("merged binding inactive")),
            "missing host-prefix diagnostic for {command}: {diagnostics:?}"
        );
    }
}

#[test]
fn spec3_reducer_keeps_reservations_stable_as_runtime_owners_toggle() {
    let candidates = [
        Command::FocusCycle,
        Command::Collapse,
        Command::Expand,
        Command::TechnicalDetails,
    ];
    for candidate in candidates {
        let mut update = Harness::new();
        update.arm_merged_candidate(&candidate, "f5");
        update.assert_ignored(key(KeyCode::F(5)));
        update.update = true;
        assert_eq!(update.press(key(KeyCode::F(5))), [Effect::Update]);
        update.update = false;
        update.assert_ignored(key(KeyCode::F(5)));

        let mut close = Harness::new();
        close.arm_merged_candidate(&candidate, "ctrl-x");
        close.assert_ignored(ctrl('x'));
        close.close = true;
        assert_eq!(close.press(ctrl('x')), [Effect::CloseWorkspace]);
        close.close = false;
        close.assert_ignored(ctrl('x'));

        let mut template = Harness::new();
        let identity = template.arm_merged_candidate(&candidate, "alt-enter");
        let alt_enter = KeyEvent::new(KeyCode::Enter, KeyModifiers::ALT);
        template.assert_ignored(alt_enter);
        template.template = true;
        assert_eq!(
            template.press(alt_enter),
            [Effect::OpenTemplate(ActivationIntent {
                identity,
                slot: Slot::Template,
            })]
        );
        template.template = false;
        template.assert_ignored(alt_enter);

        let mut refresh = Harness::new();
        refresh.arm_merged_candidate(&candidate, "f8");
        refresh.template = true;
        refresh.close = true;
        refresh.update = true;
        refresh.assert_ignored(key(KeyCode::F(8)));
    }
}

#[test]
fn spec3_menu_and_help_do_not_reactivate_reserved_candidates() {
    let mut menu = Harness::new();
    menu.arm_merged_candidate(&Command::FocusCycle, "f5");
    menu.state.focus = Focus::MachineFilter;
    menu.press(key(KeyCode::Enter));
    assert!(menu.state.menu.is_some());
    menu.update = true;
    menu.assert_ignored(key(KeyCode::F(5)));
    assert!(menu.state.menu.is_some());

    let mut help = Harness::new();
    help.arm_merged_candidate(&Command::FocusCycle, "ctrl-x");
    help.close = true;
    help.press(key(KeyCode::Char('?')));
    assert!(help.state.help_visible());
    help.assert_ignored(ctrl('x'));
    assert!(help.state.help_visible());
}

#[test]
fn focus_cycle_visits_visible_regions_skips_details_and_wraps() {
    let mut harness = Harness::new();
    let expected = [
        Focus::Browse,
        Focus::MachineFilter,
        Focus::TypeFilter,
        Focus::Query,
    ];
    for focus in expected {
        harness.press(key(KeyCode::F(6)));
        assert_eq!(harness.state.focus, focus);
    }
    harness.state.details.visible = true;
    harness.state.focus = Focus::TypeFilter;
    harness.press(key(KeyCode::F(6)));
    assert_eq!(harness.state.focus, Focus::Details);
    harness.press(key(KeyCode::F(6)));
    assert_eq!(harness.state.focus, Focus::Query);
}

#[test]
fn filter_menus_cache_commit_cancel_and_typing_returns_to_query() {
    let mut harness = Harness::new();
    harness.state.focus = Focus::MachineFilter;
    harness.press(key(KeyCode::Enter));
    assert!(matches!(harness.state.menu, Some(Menu::Machine { .. })));
    harness.press(key(KeyCode::Down));
    assert_eq!(harness.picker.filters.machine, MachineFilter::All);
    harness.press(key(KeyCode::Enter));
    assert_ne!(harness.picker.filters.machine, MachineFilter::All);

    harness.state.focus = Focus::TypeFilter;
    harness.press(key(KeyCode::Enter));
    harness.press(key(KeyCode::Down));
    harness.press(key(KeyCode::Esc));
    assert_eq!(harness.picker.filters.kind, TypeFilter::All);
    harness.press(key(KeyCode::Enter));
    harness.press(key(KeyCode::Char('x')));
    assert!(harness.state.menu.is_none());
    assert_eq!(harness.state.focus, Focus::Query);
    assert_eq!(harness.query, "x");
}

#[test]
fn menu_dispatch_suppresses_non_menu_registry_commands() {
    let mut harness = Harness::new();
    harness.template = true;
    harness.close = true;
    harness.update = true;
    harness.state.focus = Focus::MachineFilter;
    harness.press(key(KeyCode::Enter));
    for event in [
        ctrl('p'),
        key(KeyCode::Tab),
        ctrl('b'),
        ctrl('x'),
        ctrl('u'),
        key(KeyCode::F(5)),
        KeyEvent::new(KeyCode::Char('j'), KeyModifiers::ALT),
        KeyEvent::new(KeyCode::Enter, KeyModifiers::ALT),
    ] {
        harness.assert_ignored(event);
    }
    harness.press(key(KeyCode::Down));
    assert!(matches!(
        harness.state.menu,
        Some(Menu::Machine { pending: 1, .. })
    ));
    harness.press(key(KeyCode::Enter));
    assert!(harness.state.menu.is_none());
}

#[test]
fn help_restores_menu_focus_vim_cursor_selection_and_details() {
    let mut harness = Harness::new();
    harness.state.focus = Focus::MachineFilter;
    harness.state.vim = VimState::Normal;
    harness.state.details.visible = true;
    harness.press(key(KeyCode::Enter));
    let menu = harness.state.menu.clone();
    let selection = harness.picker.browse.selection.clone();
    harness.press(key(KeyCode::Char('?')));
    assert!(harness.state.help_visible());
    let origin = harness.state.descriptor().key_context;
    assert_eq!(origin.focus, Focus::MachineFilter);
    assert_eq!(origin.vim, VimState::Normal);
    assert!(origin.menu);
    harness.press(key(KeyCode::Down));
    harness.press(key(KeyCode::Esc));
    assert!(!harness.state.help_visible());
    assert_eq!(harness.state.focus, Focus::MachineFilter);
    assert_eq!(harness.state.vim, VimState::Normal);
    assert_eq!(harness.state.menu, menu);
    assert!(harness.state.details.visible);
    assert_eq!(harness.picker.browse.selection, selection);
}

#[test]
fn provenance_shortcuts_and_full_reset_preserve_or_clear_the_right_state() {
    for engine in ["nucleo", "skim", "simple"] {
        let mut harness = Harness::new();
        harness.config.engine = engine.into();
        harness.query = "void".into();
        harness.state.reconcile_query(&harness.query);
        harness.picker.set_machine(MachineFilter::Machine(
            harness
                .input
                .headings
                .iter()
                .filter_map(|heading| match &heading.owner {
                    Owner::Machine(machine) => Some(machine.clone()),
                    _ => None,
                })
                .nth(1)
                .unwrap(),
        ));
        harness.picker.set_type(TypeFilter::Sessions);
        for (event, source) in [
            (ctrl('p'), Source::Project),
            (ctrl('r'), Source::Root),
            (ctrl('s'), Source::Server),
        ] {
            harness.press(event);
            assert_eq!(harness.picker.filters.provenance, Some(source));
            assert_eq!(harness.query, "void");
            assert_eq!(harness.picker.filters.kind, TypeFilter::Sessions);
        }
        harness.press(key(KeyCode::Tab));
        assert_eq!(harness.query, "void");
        harness.press(ctrl('u'));
        assert!(harness.query.is_empty());
        assert_eq!(harness.picker.filters, crate::picker::Filters::default());
        assert_eq!(harness.state.focus, Focus::Query);
        assert_eq!(harness.state.vim, VimState::Search);
    }
}

#[test]
fn next_source_filter_starts_at_first_source_advances_in_order_and_wraps_to_none() {
    let all = Source::all();
    assert_eq!(next_source_filter(None), Some(all[0]));
    assert_eq!(next_source_filter(None), Some(Source::Workspace));
    for pair in all.windows(2) {
        assert_eq!(next_source_filter(Some(pair[0])), Some(pair[1]));
    }
    assert_eq!(next_source_filter(all.last().copied()), None);
}

#[test]
fn cycle_filter_steps_merged_provenance_and_keeps_query_and_other_filters() {
    let mut harness = Harness::new();
    harness.query = "void".into();
    harness.state.reconcile_query(&harness.query);
    harness.picker.set_type(TypeFilter::Sessions);
    let mut seen = Vec::new();
    for _ in 0..=Source::all().len() {
        assert!(harness.press(key(KeyCode::Tab)).is_empty());
        seen.push(harness.picker.filters.provenance);
        assert_eq!(harness.query, "void");
        assert_eq!(harness.picker.filters.kind, TypeFilter::Sessions);
    }
    let mut expected: Vec<_> = Source::all().into_iter().map(Some).collect();
    expected.push(None);
    assert_eq!(seen, expected);
}

#[test]
fn cycle_filter_without_picker_defers_to_legacy_cycling() {
    let config = PickerConfig::default();
    let mut state = State::legacy(&config, "");
    let mut query = String::new();
    let rows = Rows::default();
    let effects = reduce(
        &mut state,
        &mut query,
        None,
        key(KeyCode::Tab),
        Context {
            config: &config,
            visible: VisibleRegions::legacy(false),
            rows: &rows,
            actions: ActionAvailability::default(),
            legacy_filter: None,
            machine_options: &[],
        },
    );
    assert_eq!(effects, [Effect::CycleLegacyFilter]);
    assert!(query.is_empty());
}

#[test]
fn vim_merged_preserves_slash_query_and_legacy_keeps_clear_and_normal_rules() {
    let mut harness = Harness::new();
    harness.config.vim_mode = true;
    harness.query = "kept".into();
    harness.state.reconcile_query(&harness.query);
    harness.state.vim = VimState::Normal;
    harness.press(key(KeyCode::Char('/')));
    assert_eq!(harness.query, "kept");
    assert_eq!(harness.state.vim, VimState::Search);
    harness.state.vim = VimState::Normal;
    harness.press(key(KeyCode::Char('v')));
    assert_eq!(harness.query, "keptv");
    assert_eq!(harness.state.vim, VimState::Search);

    let config = PickerConfig {
        vim_mode: true,
        preview: false,
        ..PickerConfig::default()
    };
    let mut state = State::legacy(&config, "old");
    let mut query = String::from("old");
    let rows = Rows::default();
    let context = Context {
        config: &config,
        visible: VisibleRegions::legacy(false),
        rows: &rows,
        actions: ActionAvailability::default(),
        legacy_filter: None,
        machine_options: &[],
    };
    reduce(
        &mut state,
        &mut query,
        None,
        key(KeyCode::Char('/')),
        context,
    );
    assert!(query.is_empty());
    assert_eq!(state.vim, VimState::Search);
}

#[test]
fn structural_enter_guides_and_destination_enter_is_typed_or_rejected() {
    let mut harness = Harness::new();
    let owner = harness.input.headings[0].owner.clone();
    harness
        .input
        .groups
        .push((owner, crate::picker::TaskKind::Actions));
    let rows = harness.rows();
    let group = rows
        .rows
        .iter()
        .find(|row| row.action.is_none())
        .unwrap()
        .id
        .clone();
    harness.picker.browse.selection = Some(group);
    assert!(matches!(
        harness.press(key(KeyCode::Enter)).as_slice(),
        [Effect::Guidance(_)]
    ));
    let rows = harness.rows();
    let stale_action = rows.rows.iter().find_map(|row| row.action.clone()).unwrap();
    assert!(matches!(
        activate(
            &rows,
            &ActionAvailability {
                selected: Some(stale_action),
                ..ActionAvailability::default()
            },
            Slot::Primary,
        )
        .as_slice(),
        [Effect::Guidance(_)]
    ));

    let available_id = harness
        .rows()
        .rows
        .iter()
        .find(|row| row.action.is_some())
        .unwrap()
        .id
        .clone();
    let crate::picker::RowId::Destination(identity) = &available_id else {
        panic!("destination");
    };
    harness
        .input
        .destinations
        .iter_mut()
        .find(|destination| destination.entry.key() == identity)
        .unwrap()
        .action
        .unavailable = None;
    harness.picker.browse.selection = Some(available_id);
    assert!(matches!(
        harness.press(key(KeyCode::Enter)).as_slice(),
        [Effect::Activate(ActivationIntent {
            slot: Slot::Primary,
            ..
        })]
    ));

    let rows = harness.rows();
    let unavailable = rows
        .rows
        .iter()
        .find(|row| {
            row.action
                .as_ref()
                .is_some_and(|action| action.unavailable.is_some())
        })
        .unwrap();
    harness.picker.browse.selection = Some(unavailable.id.clone());
    assert!(matches!(
        harness.press(key(KeyCode::Enter)).as_slice(),
        [Effect::Rejected(_)]
    ));
}

#[test]
fn n10_modifier_reducer_rejects_modified_editing_escape_enter_and_menu_keys() {
    let mut harness = Harness::new();
    let row = harness.rows().selection.unwrap();
    harness.query = "aé🙂z".into();
    harness.state.reconcile_query(&harness.query);
    harness.press(key(KeyCode::Left));
    harness.press(key(KeyCode::Left));
    assert!(harness.state.cursor < harness.query.len());
    harness.state.details = Details {
        visible: true,
        technical: true,
    };
    harness.picker.filters.kind = TypeFilter::Sessions;
    harness.picker.filters.provenance = Some(Source::Project);
    harness.picker.browse.viewport = Some(crate::picker::ViewportAnchor {
        row: row.clone(),
        intra_row_offset: 2,
    });
    harness.picker.flat.selection = Some(row.clone());
    harness.picker.flat.viewport = Some(crate::picker::ViewportAnchor {
        row,
        intra_row_offset: 1,
    });

    let unsupported = KeyModifiers::ALT | KeyModifiers::CONTROL | KeyModifiers::SHIFT;
    for code in [
        KeyCode::Left,
        KeyCode::Right,
        KeyCode::Home,
        KeyCode::End,
        KeyCode::Backspace,
        KeyCode::Delete,
    ] {
        harness.assert_ignored(KeyEvent::new(code, unsupported));
    }
    for modifiers in [KeyModifiers::ALT, KeyModifiers::SHIFT, KeyModifiers::SUPER] {
        harness.assert_ignored(KeyEvent::new(KeyCode::Esc, modifiers));
    }

    for focus in [Focus::MachineFilter, Focus::TypeFilter] {
        harness.state.focus = focus;
        harness.assert_ignored(KeyEvent::new(KeyCode::Enter, unsupported));
        assert!(harness.state.menu.is_none());
    }

    harness.state.focus = Focus::MachineFilter;
    harness.press(key(KeyCode::Enter));
    harness.press(key(KeyCode::Down));
    assert!(matches!(
        harness.state.menu,
        Some(Menu::Machine { pending: 1, .. })
    ));
    for code in [KeyCode::Up, KeyCode::Down, KeyCode::Enter, KeyCode::Esc] {
        harness.assert_ignored(KeyEvent::new(code, unsupported));
    }
}

#[test]
fn n10_modifier_reducer_rejects_extra_control_bits_and_preserves_help_overlay() {
    let ctrl_alt = KeyModifiers::CONTROL | KeyModifiers::ALT;
    let ctrl_shift = KeyModifiers::CONTROL | KeyModifiers::SHIFT;

    for setup in ["search", "normal", "details", "menu", "help"] {
        let mut harness = Harness::new();
        harness.query = "é🙂".into();
        harness.state.reconcile_query(&harness.query);
        match setup {
            "normal" => harness.state.vim = VimState::Normal,
            "details" => {
                harness.state.details.visible = true;
                harness.state.details.technical = true;
                harness.state.focus = Focus::Details;
            }
            "menu" => {
                harness.state.focus = Focus::TypeFilter;
                harness.press(key(KeyCode::Enter));
                harness.press(key(KeyCode::Down));
            }
            "help" => {
                harness.press(key(KeyCode::Char('?')));
            }
            _ => {}
        }
        harness.assert_ignored(KeyEvent::new(KeyCode::Char('c'), ctrl_alt));
        harness.assert_ignored(KeyEvent::new(KeyCode::Char('c'), ctrl_shift));
        harness.assert_ignored(KeyEvent::new(KeyCode::Esc, KeyModifiers::ALT));
    }

    let mut harness = Harness::new();
    harness.template = true;
    harness.close = true;
    harness.update = true;
    harness.press(key(KeyCode::Char('?')));
    assert!(harness.state.help_visible());
    for event in [
        key(KeyCode::Enter),
        KeyEvent::new(KeyCode::Enter, KeyModifiers::ALT),
        ctrl('b'),
        ctrl('x'),
        ctrl('u'),
        key(KeyCode::Up),
        KeyEvent::new(KeyCode::Char('j'), KeyModifiers::ALT),
        key(KeyCode::F(5)),
        key(KeyCode::F(6)),
    ] {
        harness.assert_ignored(event);
    }
    assert_eq!(harness.press(ctrl('c')), [Effect::Quit]);
    assert!(harness.state.help_visible());

    let mut menu = Harness::new();
    menu.state.focus = Focus::MachineFilter;
    menu.press(key(KeyCode::Enter));
    assert!(menu.state.menu.is_some());
    menu.press(key(KeyCode::F(6)));
    assert!(menu.state.menu.is_none());
    assert_eq!(menu.state.focus, Focus::TypeFilter);
}

#[test]
fn n10_modifier_reducer_accepts_only_plain_or_shift_text() {
    let mut harness = Harness::new();
    assert_eq!(
        harness.press(key(KeyCode::Char('界'))),
        [Effect::QueryChanged]
    );
    assert_eq!(
        harness.press(KeyEvent::new(KeyCode::Char('A'), KeyModifiers::SHIFT)),
        [Effect::QueryChanged]
    );
    assert_eq!(harness.query, "界A");

    for (code, modifiers) in [
        (KeyCode::Char('x'), KeyModifiers::ALT),
        (
            KeyCode::Char('?'),
            KeyModifiers::CONTROL | KeyModifiers::ALT,
        ),
        (KeyCode::Char('h'), KeyModifiers::SUPER),
        (KeyCode::Char('h'), KeyModifiers::HYPER),
        (KeyCode::Char('h'), KeyModifiers::META),
        (
            KeyCode::Char('h'),
            KeyModifiers::SUPER | KeyModifiers::CONTROL,
        ),
    ] {
        harness.assert_ignored(KeyEvent::new(code, modifiers));
    }
    assert_eq!(harness.query, "界A");
    assert!(!harness.state.help_visible());
}

#[test]
fn unknown_modified_keys_do_not_change_query_and_ctrl_c_only_quits() {
    let mut harness = Harness::new();
    harness.press(KeyEvent::new(KeyCode::Char('x'), KeyModifiers::ALT));
    harness.press(KeyEvent::new(KeyCode::Char('?'), KeyModifiers::ALT));
    harness.press(ctrl('9'));
    assert!(harness.query.is_empty());
    assert!(!harness.state.help_visible());
    assert_eq!(harness.press(ctrl('c')), [Effect::Quit]);
    assert!(harness.query.is_empty());
}
