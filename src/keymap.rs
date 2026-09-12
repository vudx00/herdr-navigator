use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use crate::{
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
    CursorLeft,
    CursorRight,
    CursorHome,
    CursorEnd,
    DeleteChar,
    Delete,
    Clear,
    CloseWorkspace,
    ToggleMark,
    ToggleDetails,
    ToggleHelp,
    Update,
    Filter(Source),
    FocusCycle,
    Collapse,
    Expand,
    TechnicalDetails,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Presentation {
    Legacy,
    Merged,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Focus {
    Query,
    Browse,
    MachineFilter,
    TypeFilter,
    Details,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum VimState {
    Normal,
    Search,
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct InputContext {
    pub(crate) presentation: Presentation,
    pub(crate) focus: Focus,
    pub(crate) vim: VimState,
    pub(crate) menu: bool,
}

#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct Availability {
    pub(crate) template: bool,
    pub(crate) update: bool,
    pub(crate) close: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Scope {
    Always,
    Query,
    VimNormal,
    Merged,
    MergedBrowse,
}

#[derive(Clone)]
struct KeySpec {
    code: KeyCode,
    modifiers: KeyModifiers,
    label: String,
    scope: Scope,
}

#[derive(Clone)]
pub(crate) struct Keybind {
    pub(crate) command: Command,
    pub(crate) label: String,
    pub(crate) group: &'static str,
    compact_label: Option<&'static str>,
    keys: Vec<KeySpec>,
}

impl Scope {
    fn enabled(self, picker: &PickerConfig, context: InputContext) -> bool {
        match self {
            Self::Always => true,
            Self::Query => context.focus == Focus::Query,
            Self::VimNormal => picker.vim_mode && context.vim == VimState::Normal,
            Self::Merged => context.presentation == Presentation::Merged,
            Self::MergedBrowse => {
                context.presentation == Presentation::Merged && context.focus == Focus::Browse
            }
        }
    }
}

impl Keybind {
    pub(crate) fn key_label(&self) -> String {
        let mut labels = Vec::new();
        for label in self.keys.iter().map(|spec| spec.label.as_str()) {
            if !labels.contains(&label) {
                labels.push(label);
            }
        }
        labels.join("/")
    }

    pub(crate) fn compact_hint(&self) -> Option<(String, &'static str)> {
        let label = self.compact_label?;
        let key = self.key_label();
        (!key.is_empty()).then_some((key, label))
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

fn scoped_key(
    code: KeyCode,
    modifiers: KeyModifiers,
    label: impl Into<String>,
    scope: Scope,
) -> KeySpec {
    KeySpec {
        code,
        modifiers,
        label: label.into(),
        scope,
    }
}

fn ctrl_or_alt_enter(value: &str) -> Option<KeySpec> {
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

fn configured_key(value: &str, scope: Scope) -> Option<KeySpec> {
    let normalized = value.trim().to_ascii_lowercase();
    let (code, modifiers, label) = match normalized.as_str() {
        "left" => (KeyCode::Left, KeyModifiers::NONE, "←".into()),
        "right" => (KeyCode::Right, KeyModifiers::NONE, "→".into()),
        "up" => (KeyCode::Up, KeyModifiers::NONE, "↑".into()),
        "down" => (KeyCode::Down, KeyModifiers::NONE, "↓".into()),
        _ => {
            if let Some(number) = normalized
                .strip_prefix('f')
                .and_then(|number| number.parse::<u8>().ok())
                .filter(|number| (1..=12).contains(number))
            {
                (KeyCode::F(number), KeyModifiers::NONE, format!("F{number}"))
            } else {
                let spec = ctrl_or_alt_enter(value)?;
                (spec.code, spec.modifiers, spec.label)
            }
        }
    };
    Some(scoped_key(code, modifiers, label, scope))
}

fn query_key(code: KeyCode, label: impl Into<String>) -> KeySpec {
    scoped_key(code, KeyModifiers::NONE, label, Scope::Query)
}

fn vim_key(code: KeyCode, label: impl Into<String>) -> KeySpec {
    scoped_key(code, KeyModifiers::NONE, label, Scope::VimNormal)
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

fn base_registry(picker: &PickerConfig, availability: Availability) -> Vec<Keybind> {
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
            Some("up"),
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
            Some("down"),
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
            "cycle provenance",
            "Filters",
            None,
        ),
        binding(
            Command::CursorLeft,
            vec![query_key(KeyCode::Left, "←")],
            "query cursor left",
            "Editing",
            None,
        ),
        binding(
            Command::CursorRight,
            vec![query_key(KeyCode::Right, "→")],
            "query cursor right",
            "Editing",
            None,
        ),
        binding(
            Command::CursorHome,
            vec![query_key(KeyCode::Home, "Home")],
            "query cursor start",
            "Editing",
            None,
        ),
        binding(
            Command::CursorEnd,
            vec![query_key(KeyCode::End, "End")],
            "query cursor end",
            "Editing",
            None,
        ),
        binding(
            Command::DeleteChar,
            vec![query_key(KeyCode::Backspace, "⌫")],
            "backspace query",
            "Editing",
            None,
        ),
        binding(
            Command::Delete,
            vec![query_key(KeyCode::Delete, "Del")],
            "delete query",
            "Editing",
            None,
        ),
        binding(
            Command::Clear,
            vec![key(KeyCode::Char('u'), KeyModifiers::CONTROL, "⌃U")],
            "full reset",
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
            Command::ToggleDetails,
            vec![key(KeyCode::Char('o'), KeyModifiers::CONTROL, "⌃O")],
            "toggle Details",
            "View",
            Some("details"),
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
    if availability.close {
        bindings.push(binding(
            Command::CloseWorkspace,
            vec![key(KeyCode::Char('x'), KeyModifiers::CONTROL, "⌃X")],
            "close workspace",
            "Actions",
            None,
        ));
    }
    if availability.template {
        if let Some(template_key) = ctrl_or_alt_enter(&picker.directory_template_key) {
            bindings.push(binding(
                Command::OpenTemplate,
                vec![template_key],
                "open selected with directory template",
                "Actions",
                Some("template"),
            ));
        }
    }
    if availability.update {
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

fn scopes_overlap(picker: &PickerConfig, left: Scope, right: Scope) -> bool {
    [Presentation::Legacy, Presentation::Merged]
        .into_iter()
        .any(|presentation| {
            [
                Focus::Query,
                Focus::Browse,
                Focus::MachineFilter,
                Focus::TypeFilter,
                Focus::Details,
            ]
            .into_iter()
            .any(|focus| {
                [VimState::Normal, VimState::Search].into_iter().any(|vim| {
                    let context = InputContext {
                        presentation,
                        focus,
                        vim,
                        menu: false,
                    };
                    left.enabled(picker, context) && right.enabled(picker, context)
                })
            })
        })
}

fn key_collision<'a>(
    picker: &PickerConfig,
    bindings: &'a [Keybind],
    spec: &KeySpec,
) -> Option<&'a Keybind> {
    bindings.iter().find(|binding| {
        binding.keys.iter().any(|key| {
            key.code == spec.code
                && key.modifiers == spec.modifiers
                && scopes_overlap(picker, key.scope, spec.scope)
        })
    })
}

fn merged_candidates(picker: &PickerConfig) -> Vec<(Keybind, &'static str)> {
    let values = [
        (
            Command::FocusCycle,
            &picker.focus_cycle_key,
            "cycle focus",
            "Navigation",
            "picker.focus_cycle_key",
            Scope::Merged,
        ),
        (
            Command::Collapse,
            &picker.collapse_key,
            "collapse / parent",
            "Navigation",
            "picker.collapse_key",
            Scope::MergedBrowse,
        ),
        (
            Command::Expand,
            &picker.expand_key,
            "expand",
            "Navigation",
            "picker.expand_key",
            Scope::MergedBrowse,
        ),
        (
            Command::TechnicalDetails,
            &picker.technical_details_key,
            "technical Details",
            "View",
            "picker.technical_details_key",
            Scope::Merged,
        ),
    ];
    values
        .into_iter()
        .filter_map(|(command, value, label, group, path, scope)| {
            let spec = configured_key(value, scope)?;
            Some((binding(command, vec![spec], label, group, None), path))
        })
        .collect()
}

fn matches_host_prefix(spec: &KeySpec, prefix: Option<char>) -> bool {
    let Some(prefix) = prefix.map(|value| value.to_ascii_lowercase()) else {
        return false;
    };
    spec.modifiers == KeyModifiers::CONTROL && spec.code == KeyCode::Char(prefix)
}

fn same_key(left: &KeySpec, right: &KeySpec) -> bool {
    left.code == right.code && left.modifiers == right.modifiers
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum AdmissionConflictKind {
    Registry,
    Refresh,
    Merged,
    HostPrefix,
}

#[derive(Clone, Debug)]
struct AdmissionConflict {
    kind: AdmissionConflictKind,
    owner: String,
    key: String,
}

struct MergedAdmission {
    binding: Keybind,
    path: &'static str,
    conflict: Option<AdmissionConflict>,
}

fn merged_admissions(picker: &PickerConfig) -> Vec<MergedAdmission> {
    let base = base_registry(
        picker,
        Availability {
            template: true,
            update: true,
            close: true,
        },
    );
    let refresh = configured_key(&picker.refresh_key, Scope::Merged);
    let mut admitted: Vec<(&'static str, Keybind)> = Vec::new();
    let mut decisions = Vec::new();

    for (candidate, path) in merged_candidates(picker) {
        let Some(spec) = candidate.keys.first() else {
            continue;
        };
        let key = normalized_key(spec);
        let refresh_conflict = refresh.as_ref().is_some_and(|other| same_key(spec, other));
        let conflict = if matches_host_prefix(spec, picker.known_host_ctrl_prefix) {
            Some(AdmissionConflict {
                kind: AdmissionConflictKind::HostPrefix,
                owner: "Herdr keys.prefix".into(),
                key,
            })
        } else if refresh_conflict {
            Some(AdmissionConflict {
                kind: AdmissionConflictKind::Refresh,
                owner: "picker.refresh_key".into(),
                key,
            })
        } else if let Some(winner) = key_collision(picker, &base, spec) {
            Some(AdmissionConflict {
                kind: AdmissionConflictKind::Registry,
                owner: binding_path(picker, winner),
                key,
            })
        } else if let Some((winner, _)) = admitted.iter().find(|(_, prior)| {
            prior.keys.iter().any(|prior| {
                same_key(prior, spec) && scopes_overlap(picker, prior.scope, spec.scope)
            })
        }) {
            Some(AdmissionConflict {
                kind: AdmissionConflictKind::Merged,
                owner: (*winner).into(),
                key,
            })
        } else {
            None
        };

        if conflict.is_none() {
            admitted.push((path, candidate.clone()));
        }
        decisions.push(MergedAdmission {
            binding: candidate,
            path,
            conflict,
        });
    }
    decisions
}

fn registered_bindings(picker: &PickerConfig, availability: Availability) -> Vec<Keybind> {
    let mut bindings = base_registry(picker, availability);
    bindings.extend(
        merged_admissions(picker)
            .into_iter()
            .filter(|admission| admission.conflict.is_none())
            .map(|admission| admission.binding),
    );
    bindings
}

fn menu_key_enabled(command: &Command, spec: &KeySpec) -> bool {
    match command {
        Command::Back | Command::ToggleHelp | Command::FocusCycle => true,
        Command::MoveUp => spec.code == KeyCode::Up && spec.modifiers == KeyModifiers::NONE,
        Command::MoveDown => spec.code == KeyCode::Down && spec.modifiers == KeyModifiers::NONE,
        Command::Open => spec.code == KeyCode::Enter && spec.modifiers == KeyModifiers::NONE,
        _ => false,
    }
}

pub(crate) fn effective_bindings(
    picker: &PickerConfig,
    context: InputContext,
    availability: Availability,
) -> Vec<Keybind> {
    let mut claimed = Vec::new();
    registered_bindings(picker, availability)
        .into_iter()
        .filter_map(|mut binding| {
            let command = binding.command.clone();
            binding.keys.retain(|spec| {
                if !spec.scope.enabled(picker, context)
                    || (context.menu && !menu_key_enabled(&command, spec))
                {
                    return false;
                }
                let event = (spec.code, spec.modifiers);
                if claimed.contains(&event) {
                    return false;
                }
                claimed.push(event);
                true
            });
            if binding.keys.is_empty() {
                return None;
            }
            if binding.command == Command::Open {
                if context.menu {
                    binding.label = "choose filter".into();
                    binding.compact_label = Some("choose filter");
                } else if matches!(context.focus, Focus::MachineFilter | Focus::TypeFilter) {
                    binding.label = "open filter menu".into();
                    binding.compact_label = Some("open filter menu");
                }
            }
            Some(binding)
        })
        .collect()
}

pub(crate) fn resolve(
    picker: &PickerConfig,
    context: InputContext,
    availability: Availability,
    event: KeyEvent,
) -> Option<Command> {
    effective_bindings(picker, context, availability)
        .into_iter()
        .find(|binding| {
            binding
                .keys
                .iter()
                .any(|spec| spec.code == event.code && spec.modifiers == event.modifiers)
        })
        .map(|binding| binding.command)
}

fn normalized_key(spec: &KeySpec) -> String {
    match (spec.code, spec.modifiers) {
        (KeyCode::Char(ch), KeyModifiers::CONTROL) => format!("ctrl-{ch}"),
        (KeyCode::Enter, KeyModifiers::ALT) => "alt-enter".into(),
        (KeyCode::Char(ch), KeyModifiers::NONE) => format!("{ch} [vim Normal]"),
        (KeyCode::Left, _) => "left".into(),
        (KeyCode::Right, _) => "right".into(),
        _ => spec.label.to_ascii_lowercase(),
    }
}

fn binding_path(picker: &PickerConfig, binding: &Keybind) -> String {
    match binding.command {
        Command::Filter(source) => picker.filter_setting_path(source),
        Command::OpenTemplate => "picker.directory_template_key [when template available]".into(),
        Command::Update => "legacy Update [when update available]".into(),
        Command::ToggleDetails => "legacy TogglePreview".into(),
        _ => format!("legacy {:?}", binding.command),
    }
}

pub(crate) fn config_diagnostics(picker: &PickerConfig) -> Vec<String> {
    let bindings = base_registry(
        picker,
        Availability {
            template: true,
            update: true,
            close: true,
        },
    );
    let mut diagnostics = Vec::new();
    if ctrl_or_alt_enter(&picker.directory_template_key).is_none() {
        diagnostics.push(format!(
            "picker.directory_template_key: invalid key {:?}; template binding inactive",
            picker.directory_template_key
        ));
    }
    for (index, binding) in bindings.iter().enumerate() {
        for spec in &binding.keys {
            let Some(winner) = key_collision(picker, &bindings[..index], spec) else {
                continue;
            };
            diagnostics.push(format!(
                "{}: {} conflicts with {}; earlier registry command wins, binding inactive for this key",
                binding_path(picker, binding),
                normalized_key(spec),
                binding_path(picker, winner)
            ));
        }
    }

    let admissions = merged_admissions(picker);
    for (path, value) in [
        ("picker.focus_cycle_key", &picker.focus_cycle_key),
        ("picker.refresh_key", &picker.refresh_key),
        ("picker.collapse_key", &picker.collapse_key),
        ("picker.expand_key", &picker.expand_key),
        (
            "picker.technical_details_key",
            &picker.technical_details_key,
        ),
    ] {
        let scope = if path == "picker.collapse_key" || path == "picker.expand_key" {
            Scope::MergedBrowse
        } else {
            Scope::Merged
        };
        let Some(spec) = configured_key(value, scope) else {
            diagnostics.push(format!(
                "{path}: invalid key {value:?}; merged binding inactive"
            ));
            continue;
        };
        if path == "picker.refresh_key" {
            if let Some(winner) = key_collision(picker, &bindings, &spec) {
                diagnostics.push(format!(
                    "{path}: {} conflicts with {}; existing registry command retains precedence, merged binding inactive",
                    normalized_key(&spec),
                    binding_path(picker, winner)
                ));
            }
            continue;
        }

        let Some(conflict) = admissions
            .iter()
            .find(|admission| admission.path == path)
            .and_then(|admission| admission.conflict.as_ref())
        else {
            continue;
        };
        match conflict.kind {
            AdmissionConflictKind::Registry => diagnostics.push(format!(
                "{path}: {} conflicts with {}; existing registry command retains precedence, merged binding inactive",
                conflict.key, conflict.owner
            )),
            AdmissionConflictKind::Refresh => {
                if path == "picker.focus_cycle_key" {
                    diagnostics.push(format!(
                        "{path}: {} conflicts with {}; both bindings inactive; Refresh remains unimplemented until N15",
                        conflict.key, conflict.owner
                    ));
                } else {
                    diagnostics.push(format!(
                        "{path}: {} conflicts with {}; Refresh is reserved for N15 and the merged binding is inactive",
                        conflict.key, conflict.owner
                    ));
                }
                if let Some(winner) = key_collision(picker, &bindings, &spec) {
                    diagnostics.push(format!(
                        "{path}: {} conflicts with {}; existing registry command retains precedence, merged binding inactive",
                        conflict.key,
                        binding_path(picker, winner)
                    ));
                }
            }
            AdmissionConflictKind::Merged => diagnostics.push(format!(
                "{path}: {} conflicts with {}; earlier merged command wins, binding inactive",
                conflict.key, conflict.owner
            )),
            AdmissionConflictKind::HostPrefix => {}
        }
    }
    diagnostics
}

pub(crate) fn claimed_template_or_reserved_ctrl(picker: &PickerConfig, ch: char) -> bool {
    let event = KeyEvent::new(KeyCode::Char(ch), KeyModifiers::CONTROL);
    let template = picker
        .directory_template
        .as_ref()
        .and_then(|_| ctrl_or_alt_enter(&picker.directory_template_key));
    template
        .into_iter()
        .chain(
            [
                &picker.focus_cycle_key,
                &picker.refresh_key,
                &picker.collapse_key,
                &picker.expand_key,
                &picker.technical_details_key,
            ]
            .into_iter()
            .filter_map(|value| configured_key(value, Scope::Merged)),
        )
        .any(|spec| spec.code == event.code && spec.modifiers == event.modifiers)
}

pub(crate) fn host_prefix_diagnostics(picker: &PickerConfig, prefix: char) -> Vec<String> {
    let prefix = prefix.to_ascii_lowercase();
    let availability = Availability {
        template: picker.directory_template.is_some(),
        update: true,
        close: true,
    };
    let mut diagnostics = Vec::new();
    for binding in base_registry(picker, availability) {
        if !matches!(binding.command, Command::Filter(_))
            && binding
                .keys
                .iter()
                .any(|key| matches_host_prefix(key, Some(prefix)))
        {
            diagnostics.push(format!(
                "{}: ctrl-{prefix} conflicts with Herdr keys.prefix; binding unchanged, host interception unresolved",
                binding_path(picker, &binding)
            ));
        }
    }
    for (path, value) in [
        ("picker.focus_cycle_key", &picker.focus_cycle_key),
        ("picker.refresh_key", &picker.refresh_key),
        ("picker.collapse_key", &picker.collapse_key),
        ("picker.expand_key", &picker.expand_key),
        (
            "picker.technical_details_key",
            &picker.technical_details_key,
        ),
    ] {
        if configured_key(value, Scope::Merged)
            .is_some_and(|key| matches_host_prefix(&key, Some(prefix)))
        {
            diagnostics.push(format!(
                "{path}: ctrl-{prefix} conflicts with Herdr keys.prefix; merged binding inactive, host interception unresolved"
            ));
        }
    }
    diagnostics
}

#[cfg(test)]
mod tests {
    use super::*;

    fn context(presentation: Presentation, vim: VimState, focus: Focus) -> InputContext {
        InputContext {
            presentation,
            vim,
            focus,
            menu: false,
        }
    }

    fn ctrl(ch: char) -> KeyEvent {
        KeyEvent::new(KeyCode::Char(ch), KeyModifiers::CONTROL)
    }

    fn isolated_merged_key(command: &Command, value: &str) -> PickerConfig {
        let mut picker = PickerConfig {
            focus_cycle_key: "f9".into(),
            collapse_key: "f10".into(),
            expand_key: "f11".into(),
            technical_details_key: "f12".into(),
            ..PickerConfig::default()
        };
        match command {
            Command::FocusCycle => picker.focus_cycle_key = value.into(),
            Command::Collapse => picker.collapse_key = value.into(),
            Command::Expand => picker.expand_key = value.into(),
            Command::TechnicalDetails => picker.technical_details_key = value.into(),
            _ => panic!("not a merged command"),
        }
        picker
    }

    fn effective_commands_for_event(
        picker: &PickerConfig,
        context: InputContext,
        availability: Availability,
        event: KeyEvent,
    ) -> Vec<Command> {
        effective_bindings(picker, context, availability)
            .into_iter()
            .filter(|binding| {
                binding
                    .keys
                    .iter()
                    .any(|spec| spec.code == event.code && spec.modifiers == event.modifiers)
            })
            .map(|binding| binding.command)
            .collect()
    }

    #[test]
    fn configured_key_grammar_preserves_aliases_and_setting_limits() {
        let merged = context(Presentation::Merged, VimState::Search, Focus::Browse);
        let available = Availability {
            template: true,
            update: true,
            close: true,
        };

        for spelling in ["CTRL+A", "ctrl-a", "^A", "⌃a"] {
            let mut picker = isolated_merged_key(&Command::FocusCycle, spelling);
            picker.filter_keys.insert("agent".into(), "ctrl-g".into());
            assert_eq!(
                resolve(&picker, merged, available, ctrl('a')),
                Some(Command::FocusCycle)
            );
            assert_eq!(
                effective_commands_for_event(&picker, merged, available, ctrl('a')),
                [Command::FocusCycle]
            );
            assert!(!config_diagnostics(&picker)
                .iter()
                .any(|line| line.contains("picker.focus_cycle_key: invalid key")));
        }

        for spelling in ["alt-enter", "alt+enter", "option-enter", "OPTION+ENTER"] {
            let picker = PickerConfig {
                directory_template: Some("fixture.toml".into()),
                directory_template_key: spelling.into(),
                focus_cycle_key: spelling.into(),
                ..PickerConfig::default()
            };
            let event = KeyEvent::new(KeyCode::Enter, KeyModifiers::ALT);
            assert_eq!(
                resolve(&picker, merged, available, event),
                Some(Command::OpenTemplate)
            );
            assert_eq!(
                effective_commands_for_event(&picker, merged, available, event),
                [Command::OpenTemplate]
            );
            let diagnostics = config_diagnostics(&picker);
            assert!(!diagnostics
                .iter()
                .any(|line| line.contains("picker.directory_template_key: invalid key")));
            assert!(diagnostics.iter().any(|line| {
                line.contains("picker.focus_cycle_key: alt-enter")
                    && line.contains("picker.directory_template_key")
                    && line.contains("inactive")
            }));
        }

        for number in 1..=12 {
            let value = format!("  F{number}  ");
            let picker = isolated_merged_key(&Command::FocusCycle, &value);
            let diagnostics = config_diagnostics(&picker);
            assert!(
                !diagnostics
                    .iter()
                    .any(|line| { line.contains("picker.focus_cycle_key: invalid key") }),
                "F{number} was rejected: {diagnostics:#?}"
            );
            let event = KeyEvent::new(KeyCode::F(number), KeyModifiers::NONE);
            let expected = match number {
                5 => Some(Command::Update),
                8 => None,
                _ => Some(Command::FocusCycle),
            };
            assert_eq!(
                resolve(&picker, merged, available, event),
                expected,
                "F{number}"
            );
        }

        for arrow in ["left", "right", "up", "down"] {
            let picker = isolated_merged_key(&Command::FocusCycle, arrow);
            assert!(
                !config_diagnostics(&picker)
                    .iter()
                    .any(|line| { line.contains("picker.focus_cycle_key: invalid key") }),
                "arrow {arrow} was rejected"
            );
        }

        for invalid in ["f0", "f13", "shift-f6", "alt-j", "ctrl-enter", "", "tab"] {
            let picker = isolated_merged_key(&Command::FocusCycle, invalid);
            assert!(
                config_diagnostics(&picker).iter().any(|line| {
                    line.contains("picker.focus_cycle_key: invalid key")
                        && line.contains(&format!("{invalid:?}"))
                }),
                "missing invalid merged-key diagnostic for {invalid:?}"
            );
            assert!(effective_bindings(&picker, merged, available)
                .iter()
                .all(|binding| binding.command != Command::FocusCycle));
        }

        let mut config: crate::config::Config = toml::from_str(
            r#"
            [picker]
            directory_template = "fixture.toml"
            directory_template_key = "ctrl-enter"
            [picker.filter_keys]
            project = "tab"
            "#,
        )
        .unwrap();
        assert_eq!(config.picker.filter_key(Source::Project), Some('p'));
        assert_eq!(
            resolve(&config.picker, merged, available, ctrl('p')),
            Some(Command::Filter(Source::Project))
        );
        assert!(config_diagnostics(&config.picker).iter().any(|line| {
            line.contains("picker.directory_template_key: invalid key \"ctrl-enter\"")
        }));
        assert_eq!(
            resolve(
                &config.picker,
                merged,
                available,
                KeyEvent::new(KeyCode::Enter, KeyModifiers::CONTROL)
            ),
            None
        );
        config.avoid_default_prefix_conflict('a');
        assert_eq!(config.picker.filter_key(Source::Project), Some('p'));

        let invalid = PickerConfig {
            directory_template: Some("fixture.toml".into()),
            directory_template_key: "f6".into(),
            focus_cycle_key: "f13".into(),
            refresh_key: "alt-j".into(),
            ..PickerConfig::default()
        };
        assert_eq!(
            config_diagnostics(&invalid),
            [
                "picker.directory_template_key: invalid key \"f6\"; template binding inactive",
                "picker.focus_cycle_key: invalid key \"f13\"; merged binding inactive",
                "picker.refresh_key: invalid key \"alt-j\"; merged binding inactive",
            ]
        );
    }

    #[test]
    fn spec3_conditional_keys_reserve_every_merged_command_when_unavailable() {
        let merged = context(Presentation::Merged, VimState::Search, Focus::Browse);
        let mut leaks = Vec::new();
        for candidate in [
            Command::FocusCycle,
            Command::Collapse,
            Command::Expand,
            Command::TechnicalDetails,
        ] {
            for (key, event) in [
                ("f5", KeyEvent::new(KeyCode::F(5), KeyModifiers::NONE)),
                (
                    "alt-enter",
                    KeyEvent::new(KeyCode::Enter, KeyModifiers::ALT),
                ),
                ("ctrl-x", ctrl('x')),
            ] {
                let picker = isolated_merged_key(&candidate, key);
                if resolve(&picker, merged, Availability::default(), event)
                    == Some(candidate.clone())
                    || effective_bindings(&picker, merged, Availability::default())
                        .iter()
                        .any(|binding| binding.command == candidate)
                {
                    leaks.push(format!("{candidate:?} took unavailable reserved key {key}"));
                }
            }
        }
        assert!(leaks.is_empty(), "{}", leaks.join("\n"));
    }

    #[test]
    fn spec3_refresh_reserves_f8_and_configured_keys_for_every_merged_command() {
        let merged = context(Presentation::Merged, VimState::Search, Focus::Browse);
        let mut leaks = Vec::new();
        for candidate in [
            Command::FocusCycle,
            Command::Collapse,
            Command::Expand,
            Command::TechnicalDetails,
        ] {
            for (refresh, event) in [
                ("f8", KeyEvent::new(KeyCode::F(8), KeyModifiers::NONE)),
                ("f5", KeyEvent::new(KeyCode::F(5), KeyModifiers::NONE)),
                ("ctrl-g", ctrl('g')),
                ("left", KeyEvent::new(KeyCode::Left, KeyModifiers::NONE)),
            ] {
                let mut picker = isolated_merged_key(&candidate, refresh);
                picker.refresh_key = refresh.into();
                if resolve(&picker, merged, Availability::default(), event)
                    == Some(candidate.clone())
                    || effective_bindings(&picker, merged, Availability::default())
                        .iter()
                        .any(|binding| binding.command == candidate)
                {
                    leaks.push(format!("{candidate:?} took refresh reservation {refresh}"));
                }
            }
        }
        assert!(leaks.is_empty(), "{}", leaks.join("\n"));
    }

    #[test]
    fn spec3_runtime_availability_matrix_never_changes_merged_admission() {
        let candidates = [
            Command::FocusCycle,
            Command::Collapse,
            Command::Expand,
            Command::TechnicalDetails,
        ];
        for candidate in candidates {
            for (key, event, owner) in [
                (
                    "f5",
                    KeyEvent::new(KeyCode::F(5), KeyModifiers::NONE),
                    Command::Update,
                ),
                (
                    "alt-enter",
                    KeyEvent::new(KeyCode::Enter, KeyModifiers::ALT),
                    Command::OpenTemplate,
                ),
                ("ctrl-x", ctrl('x'), Command::CloseWorkspace),
            ] {
                let picker = isolated_merged_key(&candidate, key);
                let admission = merged_admissions(&picker)
                    .into_iter()
                    .find(|admission| admission.binding.command == candidate)
                    .expect("configured merged candidate has an admission decision");
                let conflict = admission.conflict.expect("reserved key rejects candidate");
                assert_eq!(conflict.kind, AdmissionConflictKind::Registry);
                assert_eq!(conflict.key, key);

                for template in [false, true] {
                    for update in [false, true] {
                        for close in [false, true] {
                            let availability = Availability {
                                template,
                                update,
                                close,
                            };
                            for presentation in [Presentation::Legacy, Presentation::Merged] {
                                for focus in [
                                    Focus::Query,
                                    Focus::Browse,
                                    Focus::MachineFilter,
                                    Focus::TypeFilter,
                                    Focus::Details,
                                ] {
                                    for vim in [VimState::Normal, VimState::Search] {
                                        for menu in [false, true] {
                                            let context = InputContext {
                                                presentation,
                                                focus,
                                                vim,
                                                menu,
                                            };
                                            assert!(effective_bindings(
                                                &picker,
                                                context,
                                                availability
                                            )
                                            .iter()
                                            .all(|binding| binding.command != candidate));
                                            let expected = match owner {
                                                Command::Update if update && !menu => {
                                                    Some(Command::Update)
                                                }
                                                Command::OpenTemplate if template && !menu => {
                                                    Some(Command::OpenTemplate)
                                                }
                                                Command::CloseWorkspace if close && !menu => {
                                                    Some(Command::CloseWorkspace)
                                                }
                                                _ => None,
                                            };
                                            assert_eq!(
                                                resolve(&picker, context, availability, event),
                                                expected,
                                                "wrong owner for {candidate:?} on {key} in {context:?} with {availability:?}"
                                            );
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn spec3_refresh_admission_reports_every_candidate_and_has_no_runtime_owner() {
        for candidate in [
            Command::FocusCycle,
            Command::Collapse,
            Command::Expand,
            Command::TechnicalDetails,
        ] {
            let mut picker = isolated_merged_key(&candidate, "ctrl-g");
            picker.refresh_key = "CTRL+G".into();
            let admission = merged_admissions(&picker)
                .into_iter()
                .find(|admission| admission.binding.command == candidate)
                .expect("configured merged candidate has an admission decision");
            let conflict = admission.conflict.expect("refresh rejects candidate");
            assert_eq!(conflict.kind, AdmissionConflictKind::Refresh);
            assert_eq!(conflict.owner, "picker.refresh_key");
            assert_eq!(conflict.key, "ctrl-g");

            for availability in [
                Availability::default(),
                Availability {
                    template: true,
                    update: true,
                    close: true,
                },
            ] {
                for focus in [
                    Focus::Query,
                    Focus::Browse,
                    Focus::MachineFilter,
                    Focus::TypeFilter,
                    Focus::Details,
                ] {
                    let context = context(Presentation::Merged, VimState::Search, focus);
                    assert_eq!(resolve(&picker, context, availability, ctrl('g')), None);
                    assert!(effective_bindings(&picker, context, availability)
                        .iter()
                        .all(|binding| binding.command != candidate));
                }
            }
        }
    }

    #[test]
    fn spec3_diagnostics_name_normalized_reservation_keys_and_owners() {
        let mut template_absent = Vec::new();
        let mut template_present = Vec::new();
        for candidate in [
            Command::FocusCycle,
            Command::Collapse,
            Command::Expand,
            Command::TechnicalDetails,
        ] {
            for (key, owner) in [
                ("f5", "legacy Update [when update available]"),
                (
                    "alt-enter",
                    "picker.directory_template_key [when template available]",
                ),
                ("ctrl-x", "legacy CloseWorkspace"),
            ] {
                let picker = isolated_merged_key(&candidate, key);
                let path = merged_admissions(&picker)
                    .into_iter()
                    .find(|admission| admission.binding.command == candidate)
                    .unwrap()
                    .path;
                assert!(config_diagnostics(&picker).iter().any(|line| {
                    line.contains(path)
                        && line.contains(key)
                        && line.contains(owner)
                        && line.contains("inactive")
                }));
            }

            let mut picker = isolated_merged_key(&candidate, "ctrl-g");
            picker.directory_template_key = "CTRL+G".into();
            let absent = config_diagnostics(&picker);
            assert!(absent.iter().any(|line| {
                line.contains("ctrl-g")
                    && line.contains("picker.directory_template_key [when template available]")
                    && line.contains("inactive")
            }));
            template_absent.push(absent);
            picker.directory_template = Some("default.toml".into());
            template_present.push(config_diagnostics(&picker));

            let mut picker = isolated_merged_key(&candidate, "ctrl-g");
            picker.refresh_key = "CTRL+G".into();
            let diagnostics = config_diagnostics(&picker);
            assert!(diagnostics.iter().any(|line| {
                line.contains("ctrl-g")
                    && line.contains("picker.refresh_key")
                    && line.contains("inactive")
                    && (line.contains("N15") || line.contains("both bindings inactive"))
            }));
        }
        assert_eq!(template_absent, template_present);

        let picker = PickerConfig {
            refresh_key: "  F5  ".into(),
            ..PickerConfig::default()
        };
        assert!(config_diagnostics(&picker).iter().any(|line| {
            line.contains("picker.refresh_key: f5")
                && line.contains("legacy Update [when update available]")
                && line.contains("inactive")
        }));
    }

    #[test]
    fn spec3_invalid_and_blocked_candidates_do_not_change_admission_precedence() {
        let picker = PickerConfig {
            focus_cycle_key: "invalid".into(),
            collapse_key: "f9".into(),
            expand_key: "f9".into(),
            technical_details_key: "f10".into(),
            refresh_key: "invalid".into(),
            ..PickerConfig::default()
        };
        let diagnostics = config_diagnostics(&picker);
        assert!(diagnostics
            .iter()
            .any(|line| line.contains("picker.focus_cycle_key: invalid key")));
        assert!(diagnostics
            .iter()
            .any(|line| line.contains("picker.refresh_key: invalid key")));
        assert!(diagnostics.iter().any(|line| {
            line.contains("picker.expand_key: f9 conflicts with picker.collapse_key")
                && line.contains("earlier merged command wins")
        }));
        assert_eq!(
            resolve(
                &picker,
                context(Presentation::Merged, VimState::Search, Focus::Browse),
                Availability::default(),
                KeyEvent::new(KeyCode::F(9), KeyModifiers::NONE),
            ),
            Some(Command::Collapse)
        );

        let mut blocked = isolated_merged_key(&Command::FocusCycle, "ctrl-g");
        blocked.collapse_key = "ctrl-g".into();
        blocked.known_host_ctrl_prefix = Some('g');
        for admission in merged_admissions(&blocked).into_iter().filter(|admission| {
            matches!(
                admission.binding.command,
                Command::FocusCycle | Command::Collapse
            )
        }) {
            assert_eq!(
                admission.conflict.unwrap().kind,
                AdmissionConflictKind::HostPrefix,
                "a host-blocked candidate became a merged collision winner"
            );
        }
    }

    #[test]
    fn legacy_keys_and_conditional_actions_keep_precedence() {
        let picker = PickerConfig {
            directory_template: Some("fixture.toml".into()),
            ..PickerConfig::default()
        };
        let available = Availability {
            template: true,
            update: true,
            close: true,
        };
        for vim in [VimState::Normal, VimState::Search] {
            let ctx = context(Presentation::Legacy, vim, Focus::Query);
            for (event, expected) in [
                (ctrl('p'), Command::Filter(Source::Project)),
                (ctrl('r'), Command::Filter(Source::Root)),
                (ctrl('s'), Command::Filter(Source::Server)),
                (ctrl('o'), Command::ToggleDetails),
                (
                    KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE),
                    Command::CycleFilter,
                ),
                (
                    KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE),
                    Command::Open,
                ),
                (
                    KeyEvent::new(KeyCode::F(5), KeyModifiers::NONE),
                    Command::Update,
                ),
                (
                    KeyEvent::new(KeyCode::Enter, KeyModifiers::ALT),
                    Command::OpenTemplate,
                ),
            ] {
                assert_eq!(resolve(&picker, ctx, available, event), Some(expected));
            }
            assert_eq!(
                resolve(
                    &picker,
                    ctx,
                    Availability::default(),
                    KeyEvent::new(KeyCode::F(5), KeyModifiers::NONE)
                ),
                None
            );
            assert_eq!(
                resolve(
                    &picker,
                    ctx,
                    Availability::default(),
                    KeyEvent::new(KeyCode::Enter, KeyModifiers::ALT)
                ),
                None
            );
        }
    }

    #[test]
    fn merged_keys_are_active_only_in_actual_merged_context() {
        for requested in [
            crate::config::Presentation::Legacy,
            crate::config::Presentation::Merged,
        ] {
            let picker = PickerConfig {
                presentation: requested,
                ..PickerConfig::default()
            };
            let legacy = context(Presentation::Legacy, VimState::Search, Focus::Query);
            for available in [
                Availability::default(),
                Availability {
                    template: true,
                    update: true,
                    close: true,
                },
            ] {
                for code in [KeyCode::F(6), KeyCode::F(8)] {
                    assert_eq!(
                        resolve(
                            &picker,
                            legacy,
                            available,
                            KeyEvent::new(code, KeyModifiers::NONE)
                        ),
                        None
                    );
                }
                let commands: Vec<_> = effective_bindings(&picker, legacy, available)
                    .into_iter()
                    .map(|binding| binding.command)
                    .collect();
                assert!(!commands.iter().any(|command| matches!(
                    command,
                    Command::FocusCycle
                        | Command::Collapse
                        | Command::Expand
                        | Command::TechnicalDetails
                )));
            }
        }

        let picker = PickerConfig::default();
        let available = Availability::default();
        let merged = context(Presentation::Merged, VimState::Search, Focus::Query);
        let browse = context(Presentation::Merged, VimState::Search, Focus::Browse);
        assert_eq!(
            resolve(
                &picker,
                merged,
                available,
                KeyEvent::new(KeyCode::F(6), KeyModifiers::NONE)
            ),
            Some(Command::FocusCycle)
        );
        assert_eq!(
            resolve(
                &picker,
                merged,
                available,
                KeyEvent::new(KeyCode::F(7), KeyModifiers::NONE)
            ),
            Some(Command::TechnicalDetails)
        );
        assert_eq!(
            resolve(
                &picker,
                browse,
                available,
                KeyEvent::new(KeyCode::Left, KeyModifiers::NONE)
            ),
            Some(Command::Collapse)
        );
        assert_eq!(
            resolve(
                &picker,
                merged,
                available,
                KeyEvent::new(KeyCode::Left, KeyModifiers::NONE)
            ),
            Some(Command::CursorLeft)
        );
        assert_eq!(
            resolve(
                &picker,
                merged,
                available,
                KeyEvent::new(KeyCode::F(8), KeyModifiers::NONE)
            ),
            None
        );
        let labels: Vec<_> = effective_bindings(
            &picker,
            context(Presentation::Merged, VimState::Search, Focus::Browse),
            available,
        )
        .into_iter()
        .map(|binding| binding.label)
        .collect();
        for expected in [
            "cycle focus",
            "collapse / parent",
            "expand",
            "technical Details",
            "full reset",
        ] {
            assert!(labels.iter().any(|label| label == expected));
        }
        assert!(!labels.iter().any(|label| label.contains("refresh")));
    }

    #[test]
    fn existing_commands_and_focus_refresh_conflicts_fail_closed() {
        let picker = PickerConfig {
            focus_cycle_key: "ctrl-p".into(),
            collapse_key: "ctrl-r".into(),
            expand_key: "ctrl-s".into(),
            technical_details_key: "ctrl-o".into(),
            ..PickerConfig::default()
        };
        let ctx = context(Presentation::Merged, VimState::Search, Focus::Browse);
        let available = Availability::default();
        assert_eq!(
            resolve(&picker, ctx, available, ctrl('p')),
            Some(Command::Filter(Source::Project))
        );
        assert_eq!(
            resolve(&picker, ctx, available, ctrl('r')),
            Some(Command::Filter(Source::Root))
        );
        assert_eq!(
            resolve(&picker, ctx, available, ctrl('s')),
            Some(Command::Filter(Source::Server))
        );
        assert_eq!(
            resolve(&picker, ctx, available, ctrl('o')),
            Some(Command::ToggleDetails)
        );
        let picker = PickerConfig {
            focus_cycle_key: "f8".into(),
            refresh_key: "F8".into(),
            ..PickerConfig::default()
        };
        assert_eq!(
            resolve(
                &picker,
                ctx,
                available,
                KeyEvent::new(KeyCode::F(8), KeyModifiers::NONE)
            ),
            None
        );
        assert!(config_diagnostics(&picker)
            .iter()
            .any(|line| line.contains("both bindings inactive")));
    }

    #[test]
    fn host_prefix_suppresses_only_new_merged_commands() {
        let merged = context(Presentation::Merged, VimState::Search, Focus::Browse);
        let available = Availability {
            template: true,
            update: true,
            close: true,
        };

        for (prefix, configure, command) in [
            ('g', "focus", Command::FocusCycle),
            ('h', "collapse", Command::Collapse),
            ('i', "expand", Command::Expand),
            ('j', "technical", Command::TechnicalDetails),
        ] {
            let mut picker = PickerConfig::default();
            match configure {
                "focus" => picker.focus_cycle_key = format!("CTRL+{prefix}"),
                "collapse" => picker.collapse_key = format!("ctrl-{prefix}"),
                "expand" => picker.expand_key = format!("^{prefix}"),
                "technical" => picker.technical_details_key = format!("⌃{prefix}"),
                _ => unreachable!(),
            }
            assert_eq!(
                resolve(&picker, merged, available, ctrl(prefix)),
                Some(command.clone())
            );
            let mut expected: Vec<_> = effective_bindings(&picker, merged, available)
                .into_iter()
                .map(|binding| binding.command)
                .collect();
            expected.retain(|candidate| candidate != &command);

            picker.known_host_ctrl_prefix = Some(prefix.to_ascii_uppercase());
            assert_eq!(resolve(&picker, merged, available, ctrl(prefix)), None);
            assert_eq!(
                effective_bindings(&picker, merged, available)
                    .into_iter()
                    .map(|binding| binding.command)
                    .collect::<Vec<_>>(),
                expected
            );
            assert!(
                host_prefix_diagnostics(&picker, prefix.to_ascii_uppercase())
                    .iter()
                    .any(|line| line.contains("merged binding inactive"))
            );
        }

        for host_prefix in ['?', 'z'] {
            let mut config = crate::config::Config::default();
            config.picker.focus_cycle_key = "CTRL+G".into();
            config.avoid_default_prefix_conflict(host_prefix);
            assert_eq!(
                resolve(&config.picker, merged, available, ctrl('g')),
                Some(Command::FocusCycle)
            );
        }
    }

    #[test]
    fn host_prefix_does_not_veto_legacy_filter_back_details_or_template_commands() {
        let merged = context(Presentation::Merged, VimState::Search, Focus::Browse);
        let available = Availability {
            template: true,
            update: true,
            close: true,
        };

        let mut joint = crate::config::Config::default();
        joint.picker.directory_template = Some("fixture.toml".into());
        joint.picker.directory_template_key = "ctrl-g".into();
        joint.picker.focus_cycle_key = "^g".into();
        joint.picker.refresh_key = "CTRL+G".into();
        assert_eq!(
            joint.avoid_default_prefix_conflict('G'),
            [
                "picker.directory_template_key [when template available]: ctrl-g conflicts with Herdr keys.prefix; binding unchanged, host interception unresolved",
                "picker.focus_cycle_key: ctrl-g conflicts with Herdr keys.prefix; merged binding inactive, host interception unresolved",
                "picker.refresh_key: ctrl-g conflicts with Herdr keys.prefix; merged binding inactive, host interception unresolved",
            ]
        );
        assert_eq!(
            resolve(&joint.picker, merged, available, ctrl('g')),
            Some(Command::OpenTemplate)
        );
        assert_eq!(
            effective_commands_for_event(&joint.picker, merged, available, ctrl('g')),
            [Command::OpenTemplate]
        );

        let mut filter = crate::config::Config::default();
        filter
            .picker
            .filter_keys
            .insert("project".into(), "CTRL+V".into());
        let filter_diagnostics = filter.avoid_default_prefix_conflict('V');
        assert_eq!(
            resolve(&filter.picker, merged, available, ctrl('v')),
            Some(Command::Filter(Source::Project))
        );
        assert!(filter_diagnostics.iter().any(|line| {
            line.contains("picker.filter_keys.project: ctrl-v conflicts")
                && line.contains("explicit custom binding retained")
        }));

        for (prefix, picker, expected, warning) in [
            (
                'c',
                PickerConfig {
                    known_host_ctrl_prefix: Some('c'),
                    ..PickerConfig::default()
                },
                Command::Back,
                "legacy Back",
            ),
            (
                'o',
                PickerConfig {
                    known_host_ctrl_prefix: Some('o'),
                    ..PickerConfig::default()
                },
                Command::ToggleDetails,
                "legacy TogglePreview",
            ),
            (
                'g',
                PickerConfig {
                    known_host_ctrl_prefix: Some('g'),
                    directory_template: Some("default.toml".into()),
                    directory_template_key: "CTRL+G".into(),
                    ..PickerConfig::default()
                },
                Command::OpenTemplate,
                "picker.directory_template_key",
            ),
        ] {
            assert_eq!(
                resolve(&picker, merged, available, ctrl(prefix)),
                Some(expected)
            );
            assert!(host_prefix_diagnostics(&picker, prefix)
                .iter()
                .any(|line| line.contains(warning) && line.contains("binding unchanged")));
        }
    }

    #[test]
    fn n10_modifier_registry_requires_exact_modifiers_and_preserves_supported_bindings() {
        let picker = PickerConfig {
            vim_mode: true,
            directory_template: Some("default.toml".into()),
            directory_template_key: "ctrl-g".into(),
            ..PickerConfig::default()
        };
        let available = Availability {
            template: true,
            update: true,
            close: true,
        };
        let browse = context(Presentation::Merged, VimState::Search, Focus::Browse);

        for (event, command) in [
            (ctrl('c'), Command::Back),
            (ctrl('p'), Command::Filter(Source::Project)),
            (ctrl('r'), Command::Filter(Source::Root)),
            (ctrl('s'), Command::Filter(Source::Server)),
            (ctrl('u'), Command::Clear),
            (ctrl('b'), Command::ToggleMark),
            (ctrl('o'), Command::ToggleDetails),
            (ctrl('x'), Command::CloseWorkspace),
            (ctrl('g'), Command::OpenTemplate),
        ] {
            assert_eq!(resolve(&picker, browse, available, event), Some(command));
            for extra in [KeyModifiers::ALT, KeyModifiers::SHIFT] {
                assert_eq!(
                    resolve(
                        &picker,
                        browse,
                        available,
                        KeyEvent::new(event.code, KeyModifiers::CONTROL | extra)
                    ),
                    None,
                    "accepted extra {extra:?} for {:?}",
                    event.code
                );
            }
        }
        assert_eq!(
            resolve(
                &picker,
                browse,
                available,
                KeyEvent::new(KeyCode::Char('G'), KeyModifiers::CONTROL)
            ),
            None,
            "character case is part of the configured key event"
        );

        let alt_enter_picker = PickerConfig {
            directory_template: Some("default.toml".into()),
            directory_template_key: "alt-enter".into(),
            ..picker.clone()
        };
        assert_eq!(
            resolve(
                &alt_enter_picker,
                browse,
                available,
                KeyEvent::new(KeyCode::Enter, KeyModifiers::ALT)
            ),
            Some(Command::OpenTemplate)
        );
        for modifiers in [
            KeyModifiers::ALT | KeyModifiers::CONTROL,
            KeyModifiers::ALT | KeyModifiers::SHIFT,
        ] {
            assert_eq!(
                resolve(
                    &alt_enter_picker,
                    browse,
                    available,
                    KeyEvent::new(KeyCode::Enter, modifiers)
                ),
                None
            );
        }

        for (code, command) in [
            (KeyCode::Char('j'), Command::MoveDown),
            (KeyCode::Char('k'), Command::MoveUp),
        ] {
            assert_eq!(
                resolve(
                    &picker,
                    browse,
                    available,
                    KeyEvent::new(code, KeyModifiers::ALT)
                ),
                Some(command)
            );
            for extra in [KeyModifiers::CONTROL, KeyModifiers::SHIFT] {
                assert_eq!(
                    resolve(
                        &picker,
                        browse,
                        available,
                        KeyEvent::new(code, KeyModifiers::ALT | extra)
                    ),
                    None
                );
            }
        }

        for (code, command) in [
            (KeyCode::Enter, Command::Open),
            (KeyCode::Up, Command::MoveUp),
            (KeyCode::Down, Command::MoveDown),
            (KeyCode::Tab, Command::CycleFilter),
            (KeyCode::Esc, Command::Back),
            (KeyCode::F(5), Command::Update),
            (KeyCode::F(6), Command::FocusCycle),
            (KeyCode::F(7), Command::TechnicalDetails),
        ] {
            assert_eq!(
                resolve(
                    &picker,
                    browse,
                    available,
                    KeyEvent::new(code, KeyModifiers::NONE)
                ),
                Some(command)
            );
            assert_eq!(
                resolve(
                    &picker,
                    browse,
                    available,
                    KeyEvent::new(code, KeyModifiers::SHIFT)
                ),
                None,
                "accepted modified plain key {code:?}"
            );
        }

        let query = context(Presentation::Merged, VimState::Normal, Focus::Query);
        for (code, command) in [
            (KeyCode::Left, Command::CursorLeft),
            (KeyCode::Right, Command::CursorRight),
            (KeyCode::Home, Command::CursorHome),
            (KeyCode::End, Command::CursorEnd),
            (KeyCode::Backspace, Command::DeleteChar),
            (KeyCode::Delete, Command::Delete),
        ] {
            assert_eq!(
                resolve(
                    &picker,
                    query,
                    available,
                    KeyEvent::new(code, KeyModifiers::NONE)
                ),
                Some(command)
            );
            assert_eq!(
                resolve(
                    &picker,
                    query,
                    available,
                    KeyEvent::new(code, KeyModifiers::SHIFT)
                ),
                None
            );
        }

        for presentation in [Presentation::Legacy, Presentation::Merged] {
            for vim in [VimState::Normal, VimState::Search] {
                let ctx = context(presentation, vim, Focus::Query);
                for modifiers in [KeyModifiers::NONE, KeyModifiers::SHIFT] {
                    assert_eq!(
                        resolve(
                            &picker,
                            ctx,
                            available,
                            KeyEvent::new(KeyCode::Char('?'), modifiers)
                        ),
                        Some(Command::ToggleHelp)
                    );
                }
                for modifiers in [
                    KeyModifiers::ALT,
                    KeyModifiers::META,
                    KeyModifiers::SUPER,
                    KeyModifiers::HYPER,
                ] {
                    assert_eq!(
                        resolve(
                            &picker,
                            ctx,
                            available,
                            KeyEvent::new(KeyCode::Char('?'), modifiers)
                        ),
                        None
                    );
                }
            }
        }
    }

    #[test]
    fn query_editing_is_registered_without_stealing_browse_disclosure() {
        let picker = PickerConfig::default();
        let available = Availability::default();
        let query = context(Presentation::Merged, VimState::Normal, Focus::Query);
        let browse = context(Presentation::Merged, VimState::Normal, Focus::Browse);

        for code in [
            KeyCode::Left,
            KeyCode::Right,
            KeyCode::Home,
            KeyCode::End,
            KeyCode::Backspace,
            KeyCode::Delete,
        ] {
            assert!(
                resolve(
                    &picker,
                    query,
                    available,
                    KeyEvent::new(code, KeyModifiers::NONE)
                )
                .is_some(),
                "query editing key {code:?} is outside the registry"
            );
        }
        assert_eq!(
            resolve(
                &picker,
                browse,
                available,
                KeyEvent::new(KeyCode::Left, KeyModifiers::NONE)
            ),
            Some(Command::Collapse)
        );
        assert_eq!(
            resolve(
                &picker,
                browse,
                available,
                KeyEvent::new(KeyCode::Right, KeyModifiers::NONE)
            ),
            Some(Command::Expand)
        );
        for code in [
            KeyCode::Home,
            KeyCode::End,
            KeyCode::Backspace,
            KeyCode::Delete,
        ] {
            assert_eq!(
                resolve(
                    &picker,
                    browse,
                    available,
                    KeyEvent::new(code, KeyModifiers::NONE)
                ),
                None,
                "browse claimed query editing key {code:?}"
            );
        }
    }

    #[test]
    fn configured_scope_collisions_use_every_supported_context() {
        let picker = PickerConfig {
            focus_cycle_key: "left".into(),
            collapse_key: "f9".into(),
            ..PickerConfig::default()
        };
        let available = Availability::default();
        let query = context(Presentation::Merged, VimState::Search, Focus::Query);
        let browse = context(Presentation::Merged, VimState::Search, Focus::Browse);
        assert!(resolve(
            &picker,
            query,
            available,
            KeyEvent::new(KeyCode::Left, KeyModifiers::NONE)
        )
        .is_some());
        assert_eq!(
            resolve(
                &picker,
                browse,
                available,
                KeyEvent::new(KeyCode::Left, KeyModifiers::NONE)
            ),
            None
        );
        assert_eq!(
            resolve(
                &picker,
                browse,
                available,
                KeyEvent::new(KeyCode::F(9), KeyModifiers::NONE)
            ),
            Some(Command::Collapse)
        );
        assert!(config_diagnostics(&picker)
            .iter()
            .any(|line| line.contains("picker.focus_cycle_key") && line.contains("Cursor")));
    }

    #[test]
    fn disabled_vim_aliases_do_not_block_configured_merged_keys() {
        let mut picker = PickerConfig {
            collapse_key: "ctrl-o".into(),
            ..PickerConfig::default()
        };
        picker.filter_keys.insert("project".into(), "ctrl-o".into());
        let available = Availability::default();
        let browse = context(Presentation::Merged, VimState::Normal, Focus::Browse);
        assert_eq!(
            resolve(&picker, browse, available, ctrl('o')),
            Some(Command::Filter(Source::Project))
        );
        let ctrl_o_claims = effective_bindings(&picker, browse, available)
            .iter()
            .filter(|binding| {
                binding.keys.iter().any(|spec| {
                    spec.code == KeyCode::Char('o') && spec.modifiers == KeyModifiers::CONTROL
                })
            })
            .count();
        assert_eq!(ctrl_o_claims, 1, "shadowed Ctrl-O leaked into Help");

        let mut picker = PickerConfig {
            vim_mode: false,
            ..PickerConfig::default()
        };
        picker.filter_keys.insert("project".into(), "ctrl-j".into());
        assert_eq!(
            resolve(&picker, browse, available, ctrl('j')),
            Some(Command::Filter(Source::Project))
        );
        assert!(!config_diagnostics(&picker)
            .iter()
            .any(|line| line.contains("j [vim Normal]")));

        picker.vim_mode = true;
        assert!(config_diagnostics(&picker)
            .iter()
            .any(|line| line.contains("j [vim Normal]") && line.contains("legacy MoveDown")));
        assert_eq!(
            resolve(&picker, browse, available, ctrl('j')),
            Some(Command::Filter(Source::Project))
        );
        assert_eq!(
            resolve(
                &picker,
                browse,
                available,
                KeyEvent::new(KeyCode::Char('j'), KeyModifiers::NONE)
            ),
            Some(Command::MoveDown)
        );
    }

    #[test]
    fn effective_bindings_match_dispatch_and_menu_behavior() {
        let picker = PickerConfig {
            vim_mode: true,
            directory_template: Some("default.toml".into()),
            ..PickerConfig::default()
        };
        let available = Availability {
            template: true,
            update: true,
            close: true,
        };
        for presentation in [Presentation::Legacy, Presentation::Merged] {
            for focus in [
                Focus::Query,
                Focus::Browse,
                Focus::MachineFilter,
                Focus::TypeFilter,
                Focus::Details,
            ] {
                for vim in [VimState::Normal, VimState::Search] {
                    for menu in [false, true] {
                        let context = InputContext {
                            presentation,
                            focus,
                            vim,
                            menu,
                        };
                        let bindings = effective_bindings(&picker, context, available);
                        let mut emitted = Vec::new();
                        for binding in bindings {
                            for spec in binding.keys {
                                let event = (spec.code, spec.modifiers);
                                assert!(!emitted.contains(&event), "duplicate key in {context:?}");
                                emitted.push(event);
                                assert_eq!(
                                    resolve(
                                        &picker,
                                        context,
                                        available,
                                        KeyEvent::new(spec.code, spec.modifiers)
                                    ),
                                    Some(binding.command.clone()),
                                    "Help and dispatch differ in {context:?}"
                                );
                            }
                        }
                    }
                }
            }
        }

        let menu = InputContext {
            presentation: Presentation::Merged,
            focus: Focus::MachineFilter,
            vim: VimState::Normal,
            menu: true,
        };
        let bindings = effective_bindings(&picker, menu, available);
        let commands: Vec<_> = bindings
            .iter()
            .map(|binding| binding.command.clone())
            .collect();
        assert_eq!(
            commands,
            [
                Command::Back,
                Command::MoveUp,
                Command::MoveDown,
                Command::Open,
                Command::ToggleHelp,
                Command::FocusCycle,
            ]
        );
        assert_eq!(
            bindings
                .iter()
                .find(|binding| binding.command == Command::Open)
                .map(|binding| binding.label.as_str()),
            Some("choose filter")
        );
        for binding in bindings
            .iter()
            .filter(|binding| matches!(binding.command, Command::MoveUp | Command::MoveDown))
        {
            assert_eq!(binding.keys.len(), 1);
            assert!(matches!(binding.keys[0].code, KeyCode::Up | KeyCode::Down));
        }

        let filter_focus = InputContext {
            menu: false,
            ..menu
        };
        assert_eq!(
            effective_bindings(&picker, filter_focus, available)
                .iter()
                .find(|binding| binding.command == Command::Open)
                .map(|binding| binding.label.as_str()),
            Some("open filter menu")
        );
    }

    #[test]
    fn a_partially_shadowed_alias_keeps_only_its_live_key() {
        let mut picker = PickerConfig {
            vim_mode: true,
            ..PickerConfig::default()
        };
        picker.filter_keys.insert("project".into(), "ctrl-c".into());
        let normal = context(Presentation::Merged, VimState::Normal, Focus::Query);
        let binding = effective_bindings(&picker, normal, Availability::default())
            .into_iter()
            .find(|binding| binding.command == Command::Filter(Source::Project))
            .expect("Vim alias remains active");
        assert_eq!(binding.key_label(), "c");
        assert_eq!(
            resolve(&picker, normal, Availability::default(), ctrl('c')),
            Some(Command::Back)
        );
        assert_eq!(
            resolve(
                &picker,
                normal,
                Availability::default(),
                KeyEvent::new(KeyCode::Char('c'), KeyModifiers::NONE)
            ),
            Some(Command::Filter(Source::Project))
        );
    }

    #[test]
    fn vim_commands_have_normal_scope() {
        let mut picker = PickerConfig {
            vim_mode: true,
            ..PickerConfig::default()
        };
        picker.filter_keys.insert("project".into(), "ctrl-k".into());
        let available = Availability::default();
        for presentation in [Presentation::Legacy, Presentation::Merged] {
            for (vim, plain_expected) in [(VimState::Normal, true), (VimState::Search, false)] {
                let ctx = context(presentation, vim, Focus::Query);
                for (ch, command) in [('j', Command::MoveDown), ('k', Command::MoveUp)] {
                    assert_eq!(
                        resolve(
                            &picker,
                            ctx,
                            available,
                            KeyEvent::new(KeyCode::Char(ch), KeyModifiers::NONE)
                        ),
                        plain_expected.then_some(command.clone())
                    );
                    assert_eq!(
                        resolve(
                            &picker,
                            ctx,
                            available,
                            KeyEvent::new(KeyCode::Char(ch), KeyModifiers::ALT)
                        ),
                        Some(command)
                    );
                }
                assert_eq!(
                    resolve(&picker, ctx, available, ctrl('k')),
                    Some(Command::Filter(Source::Project))
                );
            }
            assert_eq!(
                resolve(
                    &picker,
                    context(presentation, VimState::Normal, Focus::Query),
                    available,
                    KeyEvent::new(KeyCode::Char('/'), KeyModifiers::NONE)
                ),
                Some(Command::StartSearch)
            );
        }
        assert!(config_diagnostics(&picker)
            .iter()
            .any(|line| line.contains("k [vim Normal]") && line.contains("legacy MoveUp")));
        picker.vim_mode = false;
        assert!(!config_diagnostics(&picker)
            .iter()
            .any(|line| line.contains("k [vim Normal]")));
        assert_eq!(
            resolve(
                &picker,
                context(Presentation::Merged, VimState::Normal, Focus::Query),
                available,
                ctrl('k')
            ),
            Some(Command::Filter(Source::Project))
        );
    }

    #[test]
    fn collision_fixture_dispatch_matches_effective_bindings() {
        let config: crate::config::Config = toml::from_str(include_str!(
            "../tests/fixtures/config/binding-collisions.toml"
        ))
        .unwrap();
        let available = Availability {
            template: true,
            update: true,
            close: true,
        };
        let cases = [
            (ctrl('c'), Command::Back),
            (ctrl('p'), Command::Filter(Source::Project)),
            (ctrl('u'), Command::Filter(Source::Integration)),
            (ctrl('x'), Command::Filter(Source::Session)),
            (ctrl('b'), Command::Filter(Source::Workspace)),
            (ctrl('o'), Command::Filter(Source::Server)),
            (ctrl('j'), Command::Filter(Source::Root)),
        ];

        for presentation in [Presentation::Legacy, Presentation::Merged] {
            for vim in [VimState::Normal, VimState::Search] {
                let actual = context(presentation, vim, Focus::Browse);
                for (event, expected) in &cases {
                    assert_eq!(
                        resolve(&config.picker, actual, available, *event),
                        Some(expected.clone())
                    );
                    assert_eq!(
                        effective_commands_for_event(&config.picker, actual, available, *event),
                        std::slice::from_ref(expected),
                        "fixture competitor leaked for {event:?} in {actual:?}"
                    );
                }

                let plain_j = KeyEvent::new(KeyCode::Char('j'), KeyModifiers::NONE);
                let expected = (vim == VimState::Normal).then_some(Command::MoveDown);
                assert_eq!(
                    resolve(&config.picker, actual, available, plain_j),
                    expected
                );
                assert_eq!(
                    effective_commands_for_event(&config.picker, actual, available, plain_j),
                    expected.into_iter().collect::<Vec<_>>()
                );
            }
        }
    }
}
