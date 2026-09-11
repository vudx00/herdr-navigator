use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use crate::{
    app::{App, InputMode},
    config::{parse_ctrl_key, PickerConfig},
    model::Source,
};

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Command {
    Back,
    Open,
    OpenTemplate,
    MoveUp,
    MoveDown,
    StartSearch,
    CycleFilter,
    DeleteChar,
    Clear,
    CloseWorkspace,
    ToggleMark,
    TogglePreview,
    ToggleHelp,
    Update,
    Filter(Source),
}

#[derive(Clone, Copy)]
enum Scope {
    Always,
    VimNormal,
}

#[derive(Clone)]
struct KeySpec {
    code: KeyCode,
    modifiers: KeyModifiers,
    label: String,
    scope: Scope,
}

pub(crate) struct Keybind {
    pub(crate) command: Command,
    pub(crate) label: String,
    pub(crate) group: &'static str,
    compact_label: Option<&'static str>,
    keys: Vec<KeySpec>,
}

impl KeySpec {
    fn matches(&self, app: &App, key: KeyEvent) -> bool {
        self.matches_context(app.config.picker.vim_mode, app.input_mode, key)
    }

    fn matches_context(&self, vim_mode: bool, input_mode: InputMode, key: KeyEvent) -> bool {
        let modifiers_match = if self.modifiers.is_empty() {
            key.modifiers.is_empty()
        } else {
            key.modifiers.contains(self.modifiers)
        };
        self.scope.enabled(vim_mode, input_mode) && self.code == key.code && modifiers_match
    }

    fn visible(&self, app: &App) -> bool {
        match self.scope {
            Scope::Always => true,
            Scope::VimNormal => app.config.picker.vim_mode,
        }
    }
}

impl Scope {
    fn enabled(self, vim_mode: bool, input_mode: InputMode) -> bool {
        match self {
            Self::Always => true,
            Self::VimNormal => vim_mode && input_mode == InputMode::Normal,
        }
    }
}

impl Keybind {
    pub(crate) fn matches(&self, app: &App, key: KeyEvent) -> bool {
        if self.command == Command::CloseWorkspace && !app.can_close_selected() {
            return false;
        }
        self.keys.iter().any(|spec| spec.matches(app, key))
    }

    pub(crate) fn key_label(&self, app: &App) -> String {
        let mut labels = Vec::new();
        for label in self
            .keys
            .iter()
            .filter(|spec| spec.visible(app))
            .map(|spec| spec.label.as_str())
        {
            if !labels.contains(&label) {
                labels.push(label);
            }
        }
        labels.join("/")
    }

    pub(crate) fn is_active(&self, app: &App) -> bool {
        match &self.command {
            Command::StartSearch => app.input_mode == InputMode::Search,
            Command::CycleFilter => app.source_filter.is_some(),
            Command::ToggleMark => app
                .selected_entry()
                .is_some_and(|entry| app.is_pinned(entry)),
            Command::TogglePreview => app.preview,
            Command::ToggleHelp => app.input_mode == InputMode::Help,
            Command::Filter(source) => app.source_filter == Some(*source),
            _ => false,
        }
    }

    pub(crate) fn compact_hint(&self, app: &App) -> Option<(String, &'static str)> {
        let label = self.compact_label?;
        let key = match &self.command {
            Command::MoveDown if app.config.picker.vim_mode => "j/k".into(),
            Command::MoveDown => "↑/↓/⌥J/K".into(),
            Command::Filter(_) if app.config.picker.vim_mode => self
                .keys
                .iter()
                .find(|spec| matches!(spec.scope, Scope::VimNormal))
                .map(|spec| spec.label.clone())
                .unwrap_or_else(|| self.key_label(app)),
            _ => self.key_label(app),
        };
        Some((key, label))
    }
}

fn key(code: KeyCode, modifiers: KeyModifiers, label: impl Into<String>) -> KeySpec {
    KeySpec {
        code,
        modifiers,
        label: label.into(),
        scope: Scope::Always,
    }
}

fn directory_template_key(value: &str) -> Option<KeySpec> {
    match value.trim().to_ascii_lowercase().as_str() {
        "alt-enter" | "alt+enter" | "option-enter" | "option+enter" => {
            Some(key(KeyCode::Enter, KeyModifiers::ALT, "⌥↵"))
        }
        _ => {
            let key_char = parse_ctrl_key(value)?;
            Some(key(
                KeyCode::Char(key_char),
                KeyModifiers::CONTROL,
                format!("⌃{}", key_char.to_ascii_uppercase()),
            ))
        }
    }
}

fn vim_key(code: KeyCode, label: impl Into<String>) -> KeySpec {
    KeySpec {
        code,
        modifiers: KeyModifiers::NONE,
        label: label.into(),
        scope: Scope::VimNormal,
    }
}

fn binding(
    command: Command,
    keys: Vec<KeySpec>,
    label: impl Into<String>,
    group: &'static str,
    compact_label: Option<&'static str>,
) -> Keybind {
    Keybind {
        command,
        keys,
        label: label.into(),
        group,
        compact_label,
    }
}

pub(crate) fn keybindings(app: &App) -> Vec<Keybind> {
    registry(
        &app.config.picker,
        app.directory_template_for_selected().is_some(),
        app.update_available.is_some(),
    )
}

// Dispatch, help, and config diagnostics share this order. Availability is
// supplied by the caller so config checks never load templates or build an App.
fn registry(
    picker: &PickerConfig,
    template_available: bool,
    update_available: bool,
) -> Vec<Keybind> {
    let mut bindings = vec![
        binding(
            Command::Back,
            vec![
                key(KeyCode::Esc, KeyModifiers::NONE, "Esc"),
                key(KeyCode::Char('c'), KeyModifiers::CONTROL, "⌃C"),
            ],
            "back / close",
            "Actions",
            None,
        ),
        binding(
            Command::MoveUp,
            vec![
                key(KeyCode::Up, KeyModifiers::NONE, "↑"),
                key(KeyCode::Char('k'), KeyModifiers::ALT, "⌥K"),
                vim_key(KeyCode::Char('k'), "k"),
            ],
            "move up",
            "Navigation",
            None,
        ),
        binding(
            Command::MoveDown,
            vec![
                key(KeyCode::Down, KeyModifiers::NONE, "↓"),
                key(KeyCode::Char('j'), KeyModifiers::ALT, "⌥J"),
                vim_key(KeyCode::Char('j'), "j"),
            ],
            "move down",
            "Navigation",
            Some("up/down"),
        ),
    ];

    for source in Source::all() {
        let Some(filter_key) = picker.filter_key(source) else {
            continue;
        };
        bindings.push(binding(
            Command::Filter(source),
            vec![
                key(
                    KeyCode::Char(filter_key),
                    KeyModifiers::CONTROL,
                    format!("⌃{}", filter_key.to_ascii_uppercase()),
                ),
                vim_key(KeyCode::Char(filter_key), filter_key.to_string()),
            ],
            source_help_label(source),
            "Filters",
            Some(source.label()),
        ));
    }

    bindings.extend([
        binding(
            Command::Open,
            vec![key(KeyCode::Enter, KeyModifiers::NONE, "↵")],
            "open selected",
            "Actions",
            Some("open"),
        ),
        binding(
            Command::StartSearch,
            vec![vim_key(KeyCode::Char('/'), "/")],
            "search",
            "Actions",
            Some("search"),
        ),
        binding(
            Command::CycleFilter,
            vec![key(KeyCode::Tab, KeyModifiers::NONE, "Tab")],
            "cycle filters",
            "Filters",
            None,
        ),
        binding(
            Command::DeleteChar,
            vec![key(KeyCode::Backspace, KeyModifiers::NONE, "⌫")],
            "delete query character",
            "Actions",
            None,
        ),
        binding(
            Command::Clear,
            vec![key(KeyCode::Char('u'), KeyModifiers::CONTROL, "⌃U")],
            "clear query and filter",
            "Actions",
            None,
        ),
        binding(
            Command::CloseWorkspace,
            vec![key(KeyCode::Char('x'), KeyModifiers::CONTROL, "⌃X")],
            "close workspace",
            "Actions",
            None,
        ),
        binding(
            Command::ToggleMark,
            vec![key(KeyCode::Char('b'), KeyModifiers::CONTROL, "⌃B")],
            "mark / unmark selected",
            "Actions",
            Some("mark"),
        ),
        binding(
            Command::TogglePreview,
            vec![key(KeyCode::Char('o'), KeyModifiers::CONTROL, "⌃O")],
            "toggle preview",
            "View",
            Some("preview"),
        ),
        binding(
            Command::ToggleHelp,
            vec![
                key(KeyCode::Char('?'), KeyModifiers::NONE, "?"),
                key(KeyCode::Char('?'), KeyModifiers::SHIFT, "?"),
            ],
            "keybindings",
            "View",
            Some("keys"),
        ),
    ]);
    if template_available {
        if let Some(template_key) = directory_template_key(&picker.directory_template_key) {
            bindings.push(binding(
                Command::OpenTemplate,
                vec![template_key],
                "open selected with directory template",
                "Actions",
                Some("template"),
            ));
        }
    }
    if update_available {
        bindings.push(binding(
            Command::Update,
            vec![key(KeyCode::F(5), KeyModifiers::NONE, "F5")],
            "install available update",
            "Actions",
            None,
        ));
    }
    bindings
}

fn source_help_label(source: Source) -> &'static str {
    match source {
        Source::Workspace => "workspaces",
        Source::Project => "projects",
        Source::Zoxide => "zoxide",
        Source::Root => "roots",
        Source::Agent => "agents",
        Source::Server => "servers",
        Source::Session => "sessions",
        Source::QuickAction => "quick actions",
        Source::Integration => "plugins",
    }
}

fn reserved_key(value: &str) -> Option<KeySpec> {
    let normalized = value.trim().to_ascii_lowercase();
    if let Some(number) = normalized
        .strip_prefix('f')
        .and_then(|n| n.parse::<u8>().ok())
    {
        return (1..=12)
            .contains(&number)
            .then(|| key(KeyCode::F(number), KeyModifiers::NONE, format!("F{number}")));
    }
    directory_template_key(value)
}

fn reserved_settings(picker: &PickerConfig) -> [(&'static str, &str); 2] {
    [
        ("picker.focus_cycle_key", &picker.focus_cycle_key),
        ("picker.refresh_key", &picker.refresh_key),
    ]
}

fn normalized_key(spec: &KeySpec) -> String {
    match (spec.code, spec.modifiers) {
        (KeyCode::Char(ch), KeyModifiers::CONTROL) => format!("ctrl-{ch}"),
        (KeyCode::Enter, KeyModifiers::ALT) => "alt-enter".into(),
        (KeyCode::Char(ch), KeyModifiers::NONE) => format!("{ch} [vim Normal]"),
        _ => spec.label.to_ascii_lowercase(),
    }
}

fn binding_path(picker: &PickerConfig, binding: &Keybind) -> String {
    match binding.command {
        Command::Filter(source) => picker.filter_setting_path(source),
        Command::OpenTemplate => "picker.directory_template_key [when template available]".into(),
        Command::Update => "legacy Update [when update available]".into(),
        _ => format!("legacy {:?}", binding.command),
    }
}

pub(crate) fn config_diagnostics(picker: &PickerConfig) -> Vec<String> {
    let mut diagnostics = Vec::new();
    // Include conditional Update to diagnose reservations before an update is found.
    let bindings = registry(picker, picker.directory_template.is_some(), true);
    if directory_template_key(&picker.directory_template_key).is_none() {
        diagnostics.push(format!(
            "picker.directory_template_key: invalid key {:?}; template binding inactive",
            picker.directory_template_key
        ));
    }
    for (index, binding) in bindings.iter().enumerate() {
        for spec in &binding.keys {
            if !spec.scope.enabled(picker.vim_mode, InputMode::Normal) {
                continue;
            }
            let event = KeyEvent::new(spec.code, spec.modifiers);
            if let Some(winner) = bindings[..index].iter().find(|prior| {
                prior
                    .keys
                    .iter()
                    .any(|k| k.matches_context(picker.vim_mode, InputMode::Normal, event))
            }) {
                diagnostics.push(format!("{}: {} conflicts with {}; earlier registry command wins, binding inactive for this key", binding_path(picker, binding), normalized_key(spec), binding_path(picker, winner)));
            }
        }
    }
    let mut reserved: Vec<(&str, KeySpec)> = Vec::new();
    for (path, value) in reserved_settings(picker) {
        let Some(spec) = reserved_key(value) else {
            diagnostics.push(format!("{path}: invalid reserved key {value:?}; expected Ctrl, F1-F12, or Alt-Enter; reserved binding inactive"));
            continue;
        };
        let normalized = normalized_key(&spec);
        if let Some((other, _)) = reserved
            .iter()
            .find(|(_, k)| k.code == spec.code && k.modifiers == spec.modifiers)
        {
            diagnostics.push(format!("{path}: {normalized} conflicts with {other}; both reserved bindings inactive, neither takes precedence"));
        }
        let event = KeyEvent::new(spec.code, spec.modifiers);
        if let Some(winner) = bindings.iter().find(|binding| {
            binding
                .keys
                .iter()
                .any(|k| k.matches_context(picker.vim_mode, InputMode::Normal, event))
        }) {
            diagnostics.push(format!("{path}: {normalized} conflicts with {}; existing registry command retains precedence, reserved binding inactive", binding_path(picker, winner)));
        }
        reserved.push((path, spec));
    }
    diagnostics
}

pub(crate) fn claimed_template_or_reserved_ctrl(picker: &PickerConfig, ch: char) -> bool {
    let event = KeyEvent::new(KeyCode::Char(ch), KeyModifiers::CONTROL);
    let template = picker
        .directory_template
        .as_ref()
        .and_then(|_| directory_template_key(&picker.directory_template_key));
    template
        .into_iter()
        .chain(
            reserved_settings(picker)
                .into_iter()
                .filter_map(|(_, value)| reserved_key(value)),
        )
        .any(|spec| spec.matches_context(false, InputMode::Normal, event))
}

pub(crate) fn host_prefix_diagnostics(picker: &PickerConfig, prefix: char) -> Vec<String> {
    let event = KeyEvent::new(KeyCode::Char(prefix), KeyModifiers::CONTROL);
    let mut diagnostics = Vec::new();
    for binding in registry(picker, picker.directory_template.is_some(), true) {
        // Config owns filter remapping and its explicit-custom diagnostics.
        if !matches!(binding.command, Command::Filter(_))
            && binding
                .keys
                .iter()
                .any(|k| k.matches_context(picker.vim_mode, InputMode::Normal, event))
        {
            diagnostics.push(format!("{}: ctrl-{prefix} conflicts with Herdr keys.prefix; binding unchanged, host interception unresolved", binding_path(picker, &binding)));
        }
    }
    for (path, value) in reserved_settings(picker) {
        if reserved_key(value).is_some_and(|k| k.matches_context(false, InputMode::Normal, event)) {
            diagnostics.push(format!("{path}: ctrl-{prefix} conflicts with Herdr keys.prefix; reserved binding inactive, host interception unresolved for future activation"));
        }
    }
    diagnostics
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{Config, Presentation};

    fn dispatch(
        picker: &PickerConfig,
        mode: InputMode,
        template: bool,
        update: bool,
        event: KeyEvent,
    ) -> Option<Command> {
        registry(picker, template, update)
            .into_iter()
            .find(|binding| {
                binding
                    .keys
                    .iter()
                    .any(|spec| spec.matches_context(picker.vim_mode, mode, event))
            })
            .map(|binding| binding.command)
    }

    fn ctrl(ch: char) -> KeyEvent {
        KeyEvent::new(KeyCode::Char(ch), KeyModifiers::CONTROL)
    }

    #[test]
    fn legacy_commands_and_conditional_actions_keep_their_keys() {
        let picker = PickerConfig::default();
        for mode in [InputMode::Normal, InputMode::Search] {
            for (event, expected) in [
                (ctrl('p'), Command::Filter(Source::Project)),
                (ctrl('r'), Command::Filter(Source::Root)),
                (ctrl('s'), Command::Filter(Source::Server)),
                (ctrl('o'), Command::TogglePreview),
                (
                    KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE),
                    Command::CycleFilter,
                ),
                (
                    KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE),
                    Command::Open,
                ),
            ] {
                assert_eq!(dispatch(&picker, mode, false, false, event), Some(expected));
            }
            let alt_enter = KeyEvent::new(KeyCode::Enter, KeyModifiers::ALT);
            assert_eq!(dispatch(&picker, mode, false, false, alt_enter), None);
            assert_eq!(
                dispatch(&picker, mode, true, false, alt_enter),
                Some(Command::OpenTemplate)
            );
            let f5 = KeyEvent::new(KeyCode::F(5), KeyModifiers::NONE);
            assert_eq!(dispatch(&picker, mode, false, false, f5), None);
            assert_eq!(
                dispatch(&picker, mode, false, true, f5),
                Some(Command::Update)
            );
        }
    }

    #[test]
    fn reserved_keys_never_add_commands_or_hints_even_for_merged_requests() {
        for presentation in [Presentation::Legacy, Presentation::Merged] {
            let picker = PickerConfig {
                presentation,
                ..PickerConfig::default()
            };
            for available in [false, true] {
                for code in [KeyCode::F(6), KeyCode::F(8)] {
                    assert_eq!(
                        dispatch(
                            &picker,
                            InputMode::Search,
                            available,
                            available,
                            KeyEvent::new(code, KeyModifiers::NONE)
                        ),
                        None
                    );
                }
                let bindings = registry(&picker, available, available);
                assert!(bindings
                    .iter()
                    .flat_map(|b| &b.keys)
                    .all(|k| k.label != "F6" && k.label != "F8"));
                assert!(bindings
                    .iter()
                    .all(|b| !b.label.contains("refresh") && !b.label.contains("focus cycle")));
            }
        }
        let picker = PickerConfig {
            focus_cycle_key: "ctrl-p".into(),
            refresh_key: "ctrl-o".into(),
            ..PickerConfig::default()
        };
        assert_eq!(
            dispatch(&picker, InputMode::Normal, false, false, ctrl('p')),
            Some(Command::Filter(Source::Project))
        );
        assert_eq!(
            dispatch(&picker, InputMode::Normal, false, false, ctrl('o')),
            Some(Command::TogglePreview)
        );
    }

    #[test]
    fn collision_diagnostics_follow_actual_registry_precedence() {
        let config: Config = toml::from_str(include_str!(
            "../tests/fixtures/config/binding-collisions.toml"
        ))
        .unwrap();
        let picker = &config.picker;
        for (ch, command) in [
            ('c', Command::Back),
            ('p', Command::Filter(Source::Project)),
            ('u', Command::Filter(Source::Integration)),
            ('x', Command::Filter(Source::Session)),
            ('b', Command::Filter(Source::Workspace)),
            ('o', Command::Filter(Source::Server)),
            ('j', Command::Filter(Source::Root)),
        ] {
            assert_eq!(
                dispatch(picker, InputMode::Normal, true, true, ctrl(ch)),
                Some(command)
            );
        }
        assert_eq!(
            dispatch(
                picker,
                InputMode::Normal,
                true,
                true,
                KeyEvent::new(KeyCode::Char('j'), KeyModifiers::NONE)
            ),
            Some(Command::MoveDown)
        );
        assert_eq!(
            dispatch(
                picker,
                InputMode::Search,
                true,
                true,
                KeyEvent::new(KeyCode::Char('j'), KeyModifiers::NONE)
            ),
            None
        );
    }

    #[test]
    fn vim_keys_and_alt_movement_keep_their_scopes() {
        let mut picker = PickerConfig {
            vim_mode: true,
            ..PickerConfig::default()
        };
        for (ch, expected) in [('j', Command::MoveDown), ('k', Command::MoveUp)] {
            let plain = KeyEvent::new(KeyCode::Char(ch), KeyModifiers::NONE);
            assert_eq!(
                dispatch(&picker, InputMode::Normal, false, false, plain),
                Some(expected.clone())
            );
            assert_eq!(
                dispatch(&picker, InputMode::Search, false, false, plain),
                None
            );
            for mode in [InputMode::Normal, InputMode::Search] {
                assert_eq!(
                    dispatch(
                        &picker,
                        mode,
                        false,
                        false,
                        KeyEvent::new(KeyCode::Char(ch), KeyModifiers::ALT)
                    ),
                    Some(expected.clone())
                );
            }
        }
        assert_eq!(
            dispatch(
                &picker,
                InputMode::Normal,
                false,
                false,
                KeyEvent::new(KeyCode::Char('/'), KeyModifiers::NONE)
            ),
            Some(Command::StartSearch)
        );
        picker.filter_keys.insert("project".into(), "ctrl-k".into());
        assert!(config_diagnostics(&picker).iter().any(|line| line
            .contains("picker.filter_keys.project: k [vim Normal] conflicts with legacy MoveUp")));
        picker.vim_mode = false;
        assert!(config_diagnostics(&picker).is_empty());
    }

    #[test]
    fn reserved_parser_extends_only_reserved_settings() {
        for value in ["a", "CTRL+A", "ctrl-a", "^A", "⌃a"] {
            assert_eq!(reserved_key(value).unwrap().code, KeyCode::Char('a'));
            assert_eq!(
                directory_template_key(value).unwrap().modifiers,
                KeyModifiers::CONTROL
            );
        }
        for value in ["alt-enter", "alt+enter", "option-enter", "OPTION+ENTER"] {
            let parsed = reserved_key(value).unwrap();
            assert_eq!(parsed.code, KeyCode::Enter);
            assert_eq!(parsed.modifiers, KeyModifiers::ALT);
            assert!(directory_template_key(value).is_some());
        }
        for number in 1..=12 {
            let value = format!(" F{number} ");
            assert_eq!(reserved_key(&value).unwrap().code, KeyCode::F(number));
            assert!(directory_template_key(&value).is_none());
            assert!(parse_ctrl_key(&value).is_none());
        }
        for value in ["f0", "f13", "shift-f6", "alt-j", "ctrl-enter", "", "tab"] {
            assert!(reserved_key(value).is_none(), "{value}");
        }
        let picker = PickerConfig {
            focus_cycle_key: "f13".into(),
            refresh_key: "alt-j".into(),
            directory_template_key: "f6".into(),
            ..PickerConfig::default()
        };
        let diagnostics = config_diagnostics(&picker);
        assert_eq!(diagnostics.len(), 3);
        assert!(diagnostics[0].starts_with("picker.directory_template_key: invalid key"));
        assert!(diagnostics[1].starts_with("picker.focus_cycle_key: invalid reserved key"));
        assert!(diagnostics[2].starts_with("picker.refresh_key: invalid reserved key"));
    }

    #[test]
    fn reserved_collisions_cover_templates_updates_and_host_prefix() {
        let mut picker = PickerConfig {
            directory_template: Some("fixture.toml".into()),
            focus_cycle_key: "option+enter".into(),
            refresh_key: "F5".into(),
            ..PickerConfig::default()
        };
        let diagnostics = config_diagnostics(&picker);
        assert!(diagnostics.iter().any(|line| line.contains(
            "picker.focus_cycle_key: alt-enter conflicts with picker.directory_template_key"
        )));
        assert!(diagnostics.iter().any(|line| line.contains(
            "picker.refresh_key: f5 conflicts with legacy Update [when update available]"
        )));
        picker.directory_template_key = "ctrl-g".into();
        picker.focus_cycle_key = "^g".into();
        picker.refresh_key = "CTRL+G".into();
        assert_eq!(host_prefix_diagnostics(&picker, 'g'), [
            "picker.directory_template_key [when template available]: ctrl-g conflicts with Herdr keys.prefix; binding unchanged, host interception unresolved",
            "picker.focus_cycle_key: ctrl-g conflicts with Herdr keys.prefix; reserved binding inactive, host interception unresolved for future activation",
            "picker.refresh_key: ctrl-g conflicts with Herdr keys.prefix; reserved binding inactive, host interception unresolved for future activation",
        ]);
        assert!(
            config_diagnostics(&picker)
                .iter()
                .any(|line| line
                    .contains("both reserved bindings inactive, neither takes precedence"))
        );
        assert!(host_prefix_diagnostics(&picker, 'o')[0]
            .contains("legacy TogglePreview: ctrl-o conflicts with Herdr keys.prefix"));
        assert_eq!(
            dispatch(&picker, InputMode::Normal, true, false, ctrl('g')),
            Some(Command::OpenTemplate)
        );
    }
}
