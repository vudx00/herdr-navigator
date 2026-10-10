use std::{
    io,
    sync::mpsc::{Receiver, TryRecvError},
    time::Duration,
};

use crossterm::{
    event::{self, Event, KeyEvent, KeyEventKind},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{
    backend::CrosstermBackend,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span, Text},
    widgets::{Block, Borders, Clear, List, ListItem, ListState, Paragraph, Wrap},
    Frame, Terminal,
};

use crate::{
    app::App,
    keymap::{effective_bindings, Availability as KeyAvailability, Command},
    model::{Entry, EntryAction, Source},
    picker::input::{
        self, ActionAvailability, ActivationIntent, Context as InputContext, Descriptor,
        Effect as InputEffect, Rows, SelectedAction, State as InputState, VimState, VisibleRegions,
    },
    sources::status_icon_at,
    theme::Theme,
};

// All terminal acquisition happens inside the guard, including partial setup.
trait TerminalInput {
    fn raw(&mut self, enabled: bool) -> io::Result<()>;
    fn alternate(&mut self, enabled: bool) -> io::Result<()>;
    fn cursor(&mut self) -> io::Result<()>;
    fn initialize(&mut self) -> io::Result<()>;
    fn clear(&mut self) -> io::Result<()>;
    fn draw(&mut self, app: &App, input: &Descriptor, status: Option<&str>) -> io::Result<()>;
    fn poll(&mut self, timeout: Duration) -> io::Result<bool>;
    fn read(&mut self) -> io::Result<Event>;
    fn confirm(&mut self, prompt: &str) -> io::Result<bool>;
}

#[derive(Default)]
struct Console {
    terminal: Option<Terminal<CrosstermBackend<io::Stdout>>>,
}

impl Console {
    fn terminal(&mut self) -> io::Result<&mut Terminal<CrosstermBackend<io::Stdout>>> {
        self.terminal
            .as_mut()
            .ok_or_else(|| io::Error::other("terminal not initialized"))
    }
}

impl TerminalInput for Console {
    fn raw(&mut self, enabled: bool) -> io::Result<()> {
        if enabled {
            enable_raw_mode()
        } else {
            disable_raw_mode()
        }
    }
    fn alternate(&mut self, enabled: bool) -> io::Result<()> {
        if enabled {
            execute!(io::stdout(), EnterAlternateScreen)
        } else {
            execute!(io::stdout(), LeaveAlternateScreen)
        }
    }
    fn cursor(&mut self) -> io::Result<()> {
        execute!(io::stdout(), crossterm::cursor::Show)
    }
    fn initialize(&mut self) -> io::Result<()> {
        self.terminal = Some(Terminal::new(CrosstermBackend::new(io::stdout()))?);
        Ok(())
    }
    fn clear(&mut self) -> io::Result<()> {
        self.terminal()?.clear()
    }
    fn draw(&mut self, app: &App, input: &Descriptor, status: Option<&str>) -> io::Result<()> {
        self.terminal()?
            .draw(|frame| draw_with_status(frame, app, input, status))?;
        Ok(())
    }
    fn poll(&mut self, timeout: Duration) -> io::Result<bool> {
        event::poll(timeout)
    }
    fn read(&mut self) -> io::Result<Event> {
        event::read()
    }
    fn confirm(&mut self, prompt: &str) -> io::Result<bool> {
        confirm(prompt)
    }
}

struct TerminalGuard<'a, T: TerminalInput> {
    terminal: &'a mut T,
    finished: bool,
}

impl<T: TerminalInput> TerminalGuard<'_, T> {
    fn restore(&mut self) -> io::Result<()> {
        // Do not short-circuit restoration when an earlier operation fails.
        let raw = self.terminal.raw(false);
        let alternate = self.terminal.alternate(false);
        let cursor = self.terminal.cursor();
        raw.and(alternate).and(cursor)
    }
    fn resume(&mut self) -> io::Result<()> {
        self.terminal.raw(true)?;
        self.terminal.alternate(true)?;
        self.terminal.clear()
    }
    fn finish(mut self, result: io::Result<()>) -> io::Result<()> {
        let cleanup = self.restore();
        self.finished = true;
        // Retain the original failure rather than replacing it with cleanup failure.
        result.and(cleanup)
    }
}

impl<T: TerminalInput> Drop for TerminalGuard<'_, T> {
    fn drop(&mut self) {
        if !self.finished {
            let _ = self.restore();
        }
    }
}

pub(crate) fn tui_loop(
    app: &mut App,
    persist: bool,
    update_check: Option<Receiver<Option<String>>>,
) -> io::Result<()> {
    run_terminal(app, persist, update_check, &mut Console::default())
}

fn run_terminal(
    app: &mut App,
    persist: bool,
    update_check: Option<Receiver<Option<String>>>,
    terminal: &mut impl TerminalInput,
) -> io::Result<()> {
    let mut guard = TerminalGuard {
        terminal,
        finished: false,
    };
    let result = (|| {
        guard.terminal.raw(true)?;
        guard.terminal.alternate(true)?;
        guard.terminal.initialize()?;
        action_loop(app, persist, update_check, &mut guard)
    })();
    guard.finish(result)
}

struct TuiInputSession {
    state: InputState,
    // The production renderer is still legacy. N11 may populate this only after
    // merged rendering and client capability gates exist.
    merged: Option<(crate::picker::PickerInput, crate::picker::PickerState)>,
}

impl TuiInputSession {
    fn legacy(app: &App) -> Self {
        Self {
            state: InputState::legacy(&app.config.picker, &app.query),
            merged: None,
        }
    }

    fn descriptor(&self) -> Descriptor {
        self.state.descriptor()
    }
}

fn action_loop(
    app: &mut App,
    persist: bool,
    mut update_check: Option<Receiver<Option<String>>>,
    terminal: &mut TerminalGuard<'_, impl TerminalInput>,
) -> io::Result<()> {
    let mut status = None;
    let mut input_session = TuiInputSession::legacy(app);
    loop {
        if let Some(result) = update_check.as_ref().map(Receiver::try_recv) {
            match result {
                Ok(version) => {
                    app.update_available = version;
                    update_check = None;
                }
                Err(TryRecvError::Disconnected) => update_check = None,
                Err(TryRecvError::Empty) => {}
            }
        }
        let input_descriptor = input_session.descriptor();
        terminal
            .terminal
            .draw(app, &input_descriptor, status.as_deref())?;
        app.commit_displayed_actions();
        let animate = app.animate_spinner;
        if (animate || update_check.is_some())
            && !terminal.terminal.poll(Duration::from_millis(125))?
        {
            if animate {
                app.spinner_tick = app.spinner_tick.wrapping_add(1);
            }
            continue;
        }
        match terminal.terminal.read()? {
            Event::Key(key) if key.kind == KeyEventKind::Press => {
                match handle_key(app, &mut input_session, key) {
                    Action::Continue => {}
                    Action::Status(message) => status = Some(message),
                    Action::Quit => return Ok(()),
                    Action::Open(intent) => {
                        let template = intent.slot == crate::actions::Slot::Template;
                        let suspended = app.displayed_action_needs_terminal(template);
                        if suspended {
                            terminal.restore()?;
                        }
                        let outcome = app.open_intent(&intent);
                        if suspended {
                            if let Err(resume) = terminal.resume() {
                                return Err(match outcome {
                                    Err(error) => io::Error::other(format!(
                                        "{error}; terminal resume failed: {resume}"
                                    )),
                                    Ok(()) => resume,
                                });
                            }
                        }
                        match outcome {
                            Ok(()) if !persist => return Ok(()),
                            Ok(()) => {
                                status = None;
                                app.refresh();
                            }
                            Err(error) => status = Some(format!("Open failed: {error}")),
                        }
                    }
                    Action::Update => {
                        terminal.restore()?;
                        if let Some(version) = app.update_available.clone() {
                            if terminal
                                .terminal
                                .confirm(&format!("Update Herdr Navigator to v{version}?"))?
                            {
                                // Explicit F5 confirmation is the only install path.
                                match crate::update::install(&version) {
                                    Ok(()) => eprintln!("Updated Herdr Navigator to v{version}."),
                                    Err(error) => eprintln!("Update failed: {error}"),
                                }
                                return Ok(());
                            }
                        }
                        terminal.resume()?;
                    }
                    Action::Mark => {
                        if let Err(error) = app.toggle_selected_pin() {
                            let message = format!("Mark failed: {error}");
                            app.navigation.server().notify(
                                &message,
                                false,
                                &app.config.notifications,
                            );
                            status = Some(message);
                        }
                    }
                    Action::CloseWorkspace => {
                        if let Err(error) = app.retain_close_action() {
                            status = Some(format!("Close failed: {error}"));
                            continue;
                        }
                        let confirmed = if app.config.picker.confirm_close_workspace {
                            let title = app
                                .close_description()
                                .unwrap_or("selected workspace")
                                .to_string();
                            terminal.restore()?;
                            let confirmed = terminal.terminal.confirm(&format!(
                                "Close workspace {title:?}? Running processes may stop."
                            ))?;
                            terminal.resume()?;
                            confirmed
                        } else {
                            true
                        };
                        if confirmed {
                            if let Err(error) = app.close_selected_workspace() {
                                let message = format!("Close failed: {error}");
                                app.navigation.server().notify(
                                    &message,
                                    false,
                                    &app.config.notifications,
                                );
                                status = Some(message);
                            }
                        }
                    }
                }
            }
            _ => {}
        }
    }
}

fn confirm(prompt: &str) -> io::Result<bool> {
    eprintln!("{prompt} [y/N]");
    let mut answer = String::new();
    io::stdin().read_line(&mut answer)?;
    Ok(matches!(
        answer.trim().to_ascii_lowercase().as_str(),
        "y" | "yes"
    ))
}

enum Action {
    Continue,
    Status(String),
    Quit,
    Open(ActivationIntent),
    Update,
    CloseWorkspace,
    Mark,
}

fn handle_key(app: &mut App, session: &mut TuiInputSession, key: KeyEvent) -> Action {
    debug_assert!(session.merged.is_none());
    let selected = app.selected_entry().map(|entry| {
        let resolved = app.resolved_action(entry, crate::actions::Slot::Primary);
        SelectedAction {
            identity: entry.key().clone(),
            unavailable: resolved.unavailable,
        }
    });
    let rows = match &selected {
        Some(action) => Rows {
            rows: vec![input::RowView {
                id: crate::picker::RowId::Destination(action.identity.clone()),
                ancestors: Vec::new(),
                disclosure: false,
                expanded: false,
                action: Some(action.clone()),
            }],
            selection: Some(crate::picker::RowId::Destination(action.identity.clone())),
            flat: true,
        },
        None => Rows::default(),
    };
    let actions = ActionAvailability {
        selected,
        template: app.directory_template_for_selected().is_some(),
        close: app.can_close_selected(),
        update: app.update_available.is_some(),
        reject_unavailable: false,
    };
    let config = app.config.picker.clone();
    let visible = VisibleRegions::legacy(session.state.details.visible);
    let effects = input::reduce(
        &mut session.state,
        &mut app.query,
        None,
        key,
        InputContext {
            config: &config,
            visible,
            rows: &rows,
            actions,
            legacy_filter: app.source_filter,
            machine_options: &[],
        },
    );
    let mut action = Action::Continue;
    let mut filter_changed = false;
    for effect in effects {
        match effect {
            InputEffect::Quit => action = Action::Quit,
            InputEffect::QueryChanged => filter_changed = true,
            InputEffect::MoveLegacy(delta) if delta < 0 => app.prev(),
            InputEffect::MoveLegacy(_) => app.next(),
            InputEffect::SetLegacyFilter(filter) => {
                app.source_filter = filter;
                app.selected = 0;
                filter_changed = true;
            }
            InputEffect::CycleLegacyFilter => app.cycle_filter(),
            InputEffect::Activate(intent) | InputEffect::OpenTemplate(intent) => {
                action = Action::Open(intent)
            }
            InputEffect::Rejected(message) | InputEffect::Guidance(message) => {
                action = Action::Status(message)
            }
            InputEffect::ToggleMark => action = Action::Mark,
            InputEffect::CloseWorkspace => action = Action::CloseWorkspace,
            InputEffect::Update => action = Action::Update,
        }
    }
    if filter_changed {
        app.apply_filter();
    }
    action
}

#[cfg(test)]
fn draw(f: &mut Frame, app: &App) {
    let input = InputState::legacy(&app.config.picker, &app.query).descriptor();
    draw_with_status(f, app, &input, None);
}

fn draw_with_status(f: &mut Frame, app: &App, input: &Descriptor, status: Option<&str>) {
    let area = f.area();
    f.render_widget(Clear, area);
    // Herdr already frames and titles the plugin pane; a second border here
    // renders as a box inside a box.
    let outer = Block::default().style(Style::default().bg(app.theme.panel_bg));
    let inner = outer.inner(area);
    f.render_widget(outer, area);

    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(4),
            Constraint::Min(3),
            Constraint::Length(if status.is_some() { 2 } else { 0 }),
            Constraint::Length(2),
        ])
        .split(inner);

    let filter = app
        .source_filter
        .as_ref()
        .map(|s| s.label())
        .unwrap_or("all");
    let mut query_block = Block::default()
        .style(Style::default().bg(app.theme.panel_bg))
        .borders(Borders::BOTTOM);
    // The badge used to ride on the outer border's title.
    if let Some(version) = &app.update_available {
        query_block = query_block.title_top(
            Line::from(Span::styled(
                format!(" ↑ v{version} available · F5 update "),
                Style::default()
                    .fg(app.theme.yellow)
                    .add_modifier(Modifier::BOLD),
            ))
            .right_aligned(),
        );
    }
    let query_line = Line::from(vec![
        Span::styled("query ", Style::default().fg(app.theme.overlay0)),
        Span::styled(
            &app.query,
            Style::default()
                .fg(app.theme.text)
                .add_modifier(Modifier::BOLD),
        ),
        Span::raw("   "),
        Span::styled(
            format!("filter:{filter}"),
            Style::default().fg(app.theme.accent),
        ),
    ]);
    let inventory_line = app
        .inventory
        .as_ref()
        .map(|inventory| {
            let mut descriptions = inventory.descriptions();
            descriptions.extend(app.joined_inventory.unavailable.clone());
            descriptions
                .sort_by_key(|text| !text.contains("unavailable") && !text.contains("stale"));
            descriptions.join(" · ")
        })
        .unwrap_or_default();
    let lines = vec![
        query_line,
        Line::from("Server scope: owner unknown"),
        Line::from(inventory_line),
    ];
    let search = Paragraph::new(lines).block(query_block);
    f.render_widget(search, rows[0]);

    let body = if input.details.visible {
        Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(48), Constraint::Percentage(52)])
            .split(rows[1])
    } else {
        Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(100)])
            .split(rows[1])
    };

    draw_list(f, app, body[0]);
    if input.details.visible {
        draw_preview(f, app, body[1]);
    }

    if let Some(status) = status {
        f.render_widget(
            Paragraph::new(status)
                .style(Style::default().fg(app.theme.red))
                .wrap(Wrap { trim: false }),
            rows[2],
        );
    }
    draw_key_hints(f, app, input, rows[3]);
    if input.help {
        draw_keybindings_help(f, app, input, area);
    }
}

fn key_availability(app: &App) -> KeyAvailability {
    KeyAvailability {
        template: app.directory_template_for_selected().is_some(),
        update: app.update_available.is_some(),
        close: app.can_close_selected(),
    }
}

fn binding_active(command: &Command, app: &App, input: &Descriptor) -> bool {
    match command {
        Command::StartSearch => input.vim == VimState::Search,
        Command::CycleFilter => app.source_filter.is_some(),
        Command::ToggleMark => app
            .selected_entry()
            .is_some_and(|entry| app.is_pinned(entry)),
        Command::ToggleDetails => input.details.visible,
        Command::ToggleHelp => input.help,
        Command::Filter(source) => app.source_filter == Some(*source),
        Command::TechnicalDetails => input.details.technical,
        _ => false,
    }
}

fn draw_key_hints(f: &mut Frame, app: &App, input: &Descriptor, area: Rect) {
    let mut command_spans = Vec::new();
    let mut filter_spans = Vec::new();
    let availability = key_availability(app);
    for binding in effective_bindings(&app.config.picker, input.key_context, availability) {
        let Some((key, label)) = binding.compact_hint() else {
            continue;
        };
        if key.is_empty() {
            continue;
        }
        let spans = if binding.group == "Filters" {
            &mut filter_spans
        } else {
            &mut command_spans
        };
        let active = binding_active(&binding.command, app, input);
        let key_style = if active {
            Style::default()
                .fg(app.theme.panel_bg)
                .bg(app.theme.accent)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default()
                .fg(app.theme.accent)
                .add_modifier(Modifier::BOLD)
        };
        spans.push(Span::styled(format!(" {key} "), key_style));
        let label = match binding.command {
            Command::Open if binding.label == "open selected" => app
                .selected_entry()
                .map(|entry| {
                    app.resolved_action(entry, crate::actions::Slot::Primary)
                        .description()
                })
                .unwrap_or_else(|| "nothing selected".into()),
            Command::OpenTemplate => app
                .selected_entry()
                .map(|entry| {
                    app.resolved_action(entry, crate::actions::Slot::Template)
                        .description()
                })
                .unwrap_or_default(),
            _ => label.to_string(),
        };
        spans.push(Span::styled(
            format!("{label}  "),
            Style::default().fg(if active {
                app.theme.text
            } else {
                app.theme.overlay0
            }),
        ));
    }
    f.render_widget(
        Paragraph::new(Text::from(vec![
            Line::from(command_spans),
            Line::from(filter_spans),
        ]))
        .style(Style::default().bg(app.theme.panel_bg)),
        area,
    );
}

fn help_text_width(text: &str) -> usize {
    Span::raw(text).width()
}

fn wrap_help_description(description: &str, width: usize) -> Vec<String> {
    if description.is_empty() || width == 0 {
        return vec![description.to_string()];
    }

    let mut lines = Vec::new();
    let mut line = String::new();
    let mut line_width = 0;
    for word in description.split_whitespace() {
        let word_width = help_text_width(word);
        if word_width <= width {
            let separator = usize::from(!line.is_empty());
            if line_width + separator + word_width <= width {
                if separator == 1 {
                    line.push(' ');
                }
                line.push_str(word);
                line_width += separator + word_width;
                continue;
            }
            lines.push(std::mem::take(&mut line));
            line.push_str(word);
            line_width = word_width;
            continue;
        }

        if !line.is_empty() {
            lines.push(std::mem::take(&mut line));
            line_width = 0;
        }
        for character in word.chars() {
            let character_width = help_text_width(character.encode_utf8(&mut [0; 4]));
            if line_width > 0 && line_width + character_width > width {
                lines.push(std::mem::take(&mut line));
                line_width = 0;
            }
            line.push(character);
            line_width += character_width;
        }
    }
    if !line.is_empty() {
        lines.push(line);
    }
    if lines.is_empty() {
        lines.push(String::new());
    }
    lines
}

fn draw_keybindings_help(f: &mut Frame, app: &App, input: &Descriptor, area: Rect) {
    let bindings = effective_bindings(&app.config.picker, input.key_context, key_availability(app));
    let block = Block::default()
        .title(" Keybindings ")
        .borders(Borders::ALL)
        .border_style(Style::default().fg(app.theme.accent));
    let width_probe = area.centered(Constraint::Percentage(94), Constraint::Length(1));
    let probe_columns = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
        .split(block.inner(width_probe));
    let build_lines = |groups: &[&str], column_width: u16| {
        let column_bindings: Vec<_> = bindings
            .iter()
            .filter(|binding| groups.contains(&binding.group))
            .collect();
        let indent = "  ";
        let separator = " ";
        let fixed_width = help_text_width(indent) + help_text_width(separator);
        let key_width = column_bindings
            .iter()
            .map(|binding| help_text_width(&binding.key_label()))
            .max()
            .unwrap_or_default()
            .min(usize::from(column_width).saturating_sub(fixed_width));
        let description_width = usize::from(column_width).saturating_sub(fixed_width + key_width);
        let continuation = " ".repeat(fixed_width + key_width);
        let mut lines = Vec::new();
        for group in groups {
            let start = lines.len();
            lines.push(Line::styled(
                format!(" {group}"),
                Style::default()
                    .fg(app.theme.accent)
                    .add_modifier(Modifier::BOLD),
            ));
            for binding in column_bindings
                .iter()
                .filter(|binding| binding.group == *group)
            {
                let key = binding.key_label();
                let active = binding_active(&binding.command, app, input);
                let key_style = if active {
                    Style::default()
                        .fg(app.theme.panel_bg)
                        .bg(app.theme.accent)
                        .add_modifier(Modifier::BOLD)
                } else {
                    Style::default().fg(app.theme.accent)
                };
                let mut descriptions =
                    wrap_help_description(&binding.label, description_width).into_iter();
                let first = descriptions.next().unwrap_or_default();
                let padding = " ".repeat(key_width.saturating_sub(help_text_width(&key)));
                lines.push(Line::from(vec![
                    Span::raw(indent),
                    Span::styled(format!("{key}{padding}"), key_style),
                    Span::raw(separator),
                    Span::styled(first, Style::default().fg(app.theme.text)),
                ]));
                lines.extend(descriptions.map(|description| {
                    Line::from(vec![
                        Span::raw(continuation.clone()),
                        Span::styled(description, Style::default().fg(app.theme.text)),
                    ])
                }));
            }
            if lines.len() == start + 1 {
                lines.pop();
            } else {
                lines.push(Line::default());
            }
        }
        lines.pop();
        lines
    };
    let left = build_lines(&["Navigation", "Editing"], probe_columns[0].width);
    let right = build_lines(&["Actions", "View", "Filters"], probe_columns[1].width);
    let height = (left.len().max(right.len()) as u16 + 2).min(area.height.saturating_sub(2).max(1));
    let popup = area.centered(Constraint::Percentage(94), Constraint::Length(height));
    let content = block.inner(popup);
    let columns = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
        .split(content);
    f.render_widget(Clear, popup);
    f.render_widget(block, popup);
    for (lines, column) in [(left, columns[0]), (right, columns[1])] {
        f.render_widget(
            Paragraph::new(Text::from(lines)).style(Style::default().bg(app.theme.panel_bg)),
            column,
        );
    }
}

/// Whether any row needs the spinner animated. Entries only change on refresh,
/// so this is resolved there rather than on every 125 ms tick.
pub(crate) fn has_working_entry(entries: &[Entry]) -> bool {
    entries.iter().filter_map(entry_status).any(|status| {
        let status = status.to_lowercase();
        status.contains("work") || status.contains("run")
    })
}

fn agent_status_color(theme: &Theme, status: &str) -> Color {
    let status = status.to_lowercase();
    if status.contains("block")
        || status.contains("error")
        || status.contains("fail")
        || status.contains("attention")
        || status.contains("request")
        || status.contains("wait")
    {
        theme.red
    } else if status.contains("work") || status.contains("run") {
        theme.yellow
    } else if status.contains("done") || status.contains("complete") {
        theme.teal
    } else if status.contains("idle") {
        theme.green
    } else {
        theme.overlay0
    }
}

/// Workspace titles are live Herdr labels. Navigator no longer writes `dir: `
/// labels, but workspaces created by older versions keep them (and
/// `workspace_kind` still recognises them), so the prefix is still stripped.
fn display_title(entry: &Entry) -> &str {
    if entry.destination_kind() == crate::model::DestinationKind::Workspace {
        entry
            .title
            .strip_prefix("dir: ")
            .or_else(|| entry.title.strip_prefix("project: "))
            .unwrap_or(&entry.title)
    } else {
        &entry.title
    }
}

fn entry_status(entry: &Entry) -> Option<&str> {
    if let Some(agent) = &entry.agent {
        return Some(&agent.status);
    }
    if entry.pane.is_some() {
        return None;
    }
    match entry.source {
        Source::Workspace => entry.subtitle.strip_prefix("agent:").map(|rest| {
            rest.split_once(" · ")
                .map(|(status, _)| status)
                .unwrap_or(rest)
        }),
        _ => None,
    }
}

fn entry_metadata(entry: &Entry) -> String {
    if let Some(pane) = &entry.pane {
        let mut text = format!(
            "{} · {}",
            entry
                .workspace_label
                .as_deref()
                .unwrap_or("unknown workspace"),
            pane.tab_label
        );
        if let Some(agent) = &entry.agent {
            text.push_str(&format!(
                " · {} · {}",
                agent
                    .agent
                    .as_deref()
                    .or(agent.name.as_deref())
                    .or(agent.display_agent.as_deref())
                    .unwrap_or("unknown agent"),
                agent.status
            ));
        }
        if pane.agent_freshness == crate::model::AgentFreshness::Unknown {
            text.push_str(" · agents unknown");
        }
        return text;
    }
    match entry.source {
        Source::Workspace => {
            let metadata = entry
                .subtitle
                .strip_prefix("agent:")
                .and_then(|rest| rest.split_once(" · ").map(|(_, metadata)| metadata))
                .unwrap_or(&entry.subtitle);
            let mut parts = metadata.split_whitespace();
            match (parts.next(), parts.next(), parts.next()) {
                (Some(_), Some(tabs), Some(panes)) => {
                    match (tabs.strip_prefix("tabs:"), panes.strip_prefix("panes:")) {
                        (Some(tabs), Some(panes)) => format!("{tabs} tabs · {panes} panes"),
                        _ => metadata.to_string(),
                    }
                }
                _ => metadata.to_string(),
            }
        }
        _ => entry.subtitle.clone(),
    }
}

fn display_path(entry: &Entry) -> String {
    entry
        .path
        .as_ref()
        .map(crate::paths::OwnedPath::display)
        .unwrap_or_default()
}

fn metadata_width(width: u16) -> usize {
    if width >= 90 {
        28
    } else if width >= 68 {
        20
    } else if width >= 52 {
        14
    } else {
        0
    }
}

fn truncate_end(value: &str, max_chars: usize) -> String {
    if value.chars().count() <= max_chars {
        return value.into();
    }
    if max_chars == 0 {
        return String::new();
    }
    value
        .chars()
        .take(max_chars.saturating_sub(1))
        .chain(std::iter::once('…'))
        .collect()
}

fn entry_branch(app: &App, entry: &Entry, group_end: bool) -> (&'static str, Color) {
    let is_workspace = entry.destination_kind() == crate::model::DestinationKind::Workspace;
    let is_current = is_workspace && entry.search_terms.iter().any(|term| term == "focused");
    let is_previous = is_workspace
        && app.config.jump_back.pin_previous
        && app.query.trim().is_empty()
        && app.source_filter.is_none()
        && entry.workspace_id.is_some()
        && Some(entry.key()) == app.previous_workspace_id.as_ref();
    if app.is_pinned(entry) {
        ("  ◆  ", app.theme.yellow)
    } else if is_current {
        ("  ◆  ", app.theme.accent)
    } else if is_previous {
        ("  ◆  ", app.theme.red)
    } else if group_end {
        ("  └─ ", app.theme.overlay0)
    } else {
        ("  ├─ ", app.theme.overlay0)
    }
}

/// Status glyph and color shared by the detailed and compact row layouts.
struct RowStatus<'a> {
    text: Option<&'a str>,
    icon: String,
    color: Color,
}

fn row_status<'a>(app: &App, e: &'a Entry, fallback: Color) -> RowStatus<'a> {
    let text = entry_status(e);
    RowStatus {
        text,
        icon: text
            .map(|status| format!("{} ", status_icon_at(&e.source, status, app.spinner_tick)))
            .unwrap_or_default(),
        color: text
            .map(|status| agent_status_color(&app.theme, status))
            .unwrap_or(fallback),
    }
}

fn draw_list(f: &mut Frame, app: &App, area: Rect) {
    let show_scores = !app.query.trim().is_empty();
    let row_width = area.width.saturating_sub(3) as usize;
    let mut items = Vec::new();
    let mut selected_row = None;
    for (row, idx) in app.filtered.iter().enumerate() {
        let e = &app.entries[*idx];
        let color = source_color(&app.theme, &e.source);
        let group_start =
            row == 0 || app.entries[app.filtered[row - 1]].source_name() != e.source_name();
        let group_end = row + 1 == app.filtered.len()
            || app.entries[app.filtered[row + 1]].source_name() != e.source_name();
        if group_start {
            items.push(ListItem::new(Line::from(Span::styled(
                format!(" ▾ {} ", e.source_name()),
                Style::default().fg(color).add_modifier(Modifier::BOLD),
            ))));
        }

        if row == app.selected {
            selected_row = Some(items.len());
        }
        let (branch, branch_color) = entry_branch(app, e, group_end);
        let status = row_status(app, e, color);
        let branch = Span::styled(branch, Style::default().fg(branch_color));

        items.push(if app.config.picker.detailed_rows {
            detailed_row(app, e, area.width, row_width, group_end, branch, status)
        } else {
            let score = show_scores
                .then(|| app.filtered_scores.get(row).map(|s| format!("score {s}")))
                .flatten();
            compact_row(app, e, row_width, branch, status, score)
        });
    }
    let mut state = ListState::default();
    state.select(selected_row);
    let list = List::new(items)
        .block(Block::default().title(" Results ").borders(Borders::RIGHT))
        .highlight_style(
            Style::default()
                .bg(app.theme.surface0)
                .add_modifier(Modifier::BOLD),
        )
        .highlight_symbol("→ ");
    f.render_stateful_widget(list, area, &mut state);
}

/// Title line with a right-aligned status/metadata column, plus a detail line
/// (pane metadata or directory path) for panes and directory sources.
fn detailed_row<'a>(
    app: &App,
    e: &Entry,
    area_width: u16,
    row_width: usize,
    group_end: bool,
    branch: Span<'a>,
    status: RowStatus<'_>,
) -> ListItem<'a> {
    let status_label = status.text.filter(|status| *status != "unknown");
    let status_label_width = status_label.map_or(0, |s| s.chars().count());
    let raw_path = e.path_text().to_string();
    let raw_metadata = entry_metadata(e);
    let meta_width = metadata_width(area_width);
    let show_metadata = e.pane.is_none()
        && !matches!(e.source, Source::Zoxide | Source::Root)
        && meta_width > 0
        && !raw_metadata.is_empty()
        && raw_metadata != raw_path;
    let separator_width = usize::from(show_metadata && status_label.is_some()) * 3;
    let metadata_budget = meta_width
        .saturating_sub(status_label_width)
        .saturating_sub(separator_width);
    let metadata = if show_metadata {
        truncate_end(&raw_metadata, metadata_budget)
    } else {
        String::new()
    };
    let right_width = metadata.chars().count() + separator_width + status_label_width;
    let fixed_width = branch.content.chars().count() + status.icon.chars().count();
    let right_column_width = if right_width > 0 {
        meta_width.max(right_width)
    } else {
        0
    };
    let title_budget = row_width
        .saturating_sub(fixed_width)
        .saturating_sub(right_column_width)
        .saturating_sub(usize::from(right_width > 0));
    let title = truncate_end(display_title(e), title_budget);
    let spacer = if right_width == 0 {
        String::new()
    } else {
        " ".repeat(
            row_width
                .saturating_sub(fixed_width)
                .saturating_sub(title.chars().count())
                .saturating_sub(right_column_width),
        )
    };
    let mut title_spans = vec![
        branch,
        Span::styled(status.icon, Style::default().fg(status.color)),
        Span::styled(title, Style::default().fg(app.theme.text)),
    ];
    if right_width > 0 {
        title_spans.push(Span::raw(spacer));
        if let Some(status_label) = status_label {
            title_spans.push(Span::styled(
                status_label.to_string(),
                Style::default().fg(status.color),
            ));
            if !metadata.is_empty() {
                title_spans.push(Span::styled(" · ", Style::default().fg(app.theme.overlay0)));
            }
        }
        if !metadata.is_empty() {
            title_spans.push(Span::styled(
                metadata,
                Style::default().fg(app.theme.overlay0),
            ));
        }
    }

    if e.pane.is_none() && !matches!(e.source, Source::Zoxide | Source::Root) {
        return ListItem::new(Line::from(title_spans));
    }
    let detail_branch = if group_end { "     " } else { "  │  " };
    let path_budget = row_width.saturating_sub(detail_branch.chars().count());
    let detail = if e.pane.is_some() {
        entry_metadata(e)
    } else {
        display_path(e)
    };
    let path = truncate_end(&detail, path_budget);
    ListItem::new(vec![
        Line::from(title_spans),
        Line::from(vec![
            Span::styled(detail_branch, Style::default().fg(app.theme.overlay0)),
            Span::styled(path, Style::default().fg(app.theme.subtext0)),
        ]),
    ])
}

/// Single line: title and subtitle, with the match score right-aligned while
/// a query is active.
fn compact_row<'a>(
    app: &App,
    e: &Entry,
    row_width: usize,
    branch: Span<'a>,
    status: RowStatus<'_>,
    score: Option<String>,
) -> ListItem<'a> {
    let metadata = if e.pane.is_some() {
        entry_metadata(e)
    } else {
        e.subtitle.clone()
    };
    let subtitle = if metadata.is_empty() {
        String::new()
    } else {
        format!("  {metadata}")
    };
    let left_len = branch.content.chars().count()
        + status.icon.chars().count()
        + e.title.chars().count()
        + subtitle.chars().count();
    let mut spans = vec![
        branch,
        Span::styled(status.icon, Style::default().fg(status.color)),
        Span::styled(e.title.clone(), Style::default().fg(app.theme.text)),
        Span::styled(subtitle, Style::default().fg(app.theme.subtext0)),
    ];
    if let Some(score) = score {
        let spacer = " ".repeat(
            row_width
                .saturating_sub(left_len + score.chars().count())
                .max(2),
        );
        spans.push(Span::raw(spacer));
        spans.push(Span::styled(score, Style::default().fg(app.theme.overlay0)));
    }
    ListItem::new(Line::from(spans))
}

fn draw_preview(f: &mut Frame, app: &App, area: Rect) {
    let text = if let Some(e) = app.selected_entry() {
        preview_text(app, e)
    } else {
        "No results".into()
    };
    let p = Paragraph::new(text)
        .style(Style::default().fg(app.theme.text))
        .wrap(Wrap { trim: false })
        .block(
            Block::default()
                .title(" Preview ")
                .borders(Borders::LEFT)
                .border_style(Style::default().fg(app.theme.surface_dim)),
        );
    f.render_widget(p, area);
}

fn preview_text(app: &App, e: &Entry) -> String {
    let mut lines = vec![
        format!("type: {:?}", e.destination_kind()),
        format!("source: {}", e.source_name()),
        format!("title: {}", e.title),
        format!("path: {}", e.path_text()),
    ];
    if matches!(
        e.action,
        EntryAction::OpenProject | EntryAction::FocusOrCreateDir
    ) {
        if let Some(error) = e
            .path
            .as_ref()
            .and_then(|path| app.navigation.process_path(path).err())
        {
            lines.push(format!("unavailable: {error}"));
        }
    }
    if !e.subtitle.is_empty() {
        lines.push(format!("info: {}", e.subtitle));
    }
    if let Some(label) = &e.workspace_label {
        lines.push(format!("workspace: {label}"));
    }
    if let Some(id) = &e.workspace_id {
        lines.push(format!("workspace_id: {id}"));
    }
    if let Some(target) = e.target() {
        if target.identity().scope() != app.navigation.scope() {
            lines.push("stale: target belongs to an old server scope".into());
        }
        if let Some(workspace) = target.workspace() {
            lines.push(format!("parent workspace: {}", workspace.id()));
        }
        if let Some(tab) = target.tab() {
            lines.push(format!("parent tab: {}", tab.id()));
        }
    }
    if let Some(pane) = &e.pane {
        lines.push(format!("pane ordinal: {}", pane.ordinal));
        lines.push(format!(
            "tab: {} · naming {:?}",
            pane.tab_label, pane.tab_naming
        ));
        lines.push(format!("terminal metadata: {}", pane.terminal_id));
        lines.push(format!("agent freshness: {:?}", pane.agent_freshness));
    }
    if let Some(agent) = &e.agent {
        lines.push(format!(
            "agent: {} · {} · revision {}",
            agent
                .agent
                .as_deref()
                .or(agent.name.as_deref())
                .or(agent.display_agent.as_deref())
                .unwrap_or("unknown agent"),
            agent.status,
            agent.revision
        ));
        if let Some(session) = &agent.session {
            lines.push(format!("agent session: {}", session.value));
        }
    }
    if e.source == Source::Agent {
        lines.push(
            "agent filters: @ all agents (configured sort), !agent, @workspace/status, /path"
                .into(),
        );
    }
    if !e.search_terms.is_empty() {
        lines.push(format!("search terms: {}", e.search_terms.join(", ")));
    }
    let workspaces = app.workspaces_for_entry(e);
    if !workspaces.is_empty() {
        lines.push("existing workspaces:".into());
        for ws in workspaces {
            lines.push(format!(
                "  - {} [{}] tabs:{} panes:{} {}",
                ws.identity.id(),
                ws.label,
                ws.tab_count,
                ws.pane_count,
                ws.path
                    .as_ref()
                    .map(crate::paths::OwnedPath::text)
                    .unwrap_or_default()
            ));
        }
    }
    if let Some(p) = &e.project {
        lines.push("".into());
        lines.push("project tabs:".into());
        for tab in &p.tabs {
            let cmd = tab.command.as_deref().unwrap_or("shell");
            lines.push(format!("  - {}: {}", tab.name, cmd));
        }
    }
    lines.push("".into());
    lines.push(format!(
        "enter: {}",
        app.resolved_action(e, crate::actions::Slot::Primary)
            .description()
    ));
    if app.directory_template_for_selected().is_some() {
        lines.push(format!(
            "{}: {}",
            app.config.picker.directory_template_key,
            app.resolved_action(e, crate::actions::Slot::Template)
                .description()
        ));
    }
    lines.join("\n")
}

fn source_color(theme: &Theme, source: &Source) -> Color {
    match source {
        Source::Workspace => theme.green,
        Source::Project => theme.mauve,
        Source::Zoxide => theme.blue,
        Source::Root => theme.teal,
        Source::Agent => theme.yellow,
        Source::Server => theme.green,
        Source::Session => theme.green,
        Source::QuickAction => theme.mauve,
        Source::Integration => theme.red,
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        model::{EntryIdentity, LiveIdentity, ObjectKind, ProviderId},
        paths::OwnedPath,
    };
    use std::path::PathBuf;

    use crossterm::event::{KeyCode, KeyModifiers};
    use ratatui::backend::TestBackend;

    use super::*;
    use crate::{config::Config, theme::Theme};

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }

    fn entry(source: Source, title: &str) -> Entry {
        Entry::new(
            EntryIdentity::provider("test", title, "open-directory").unwrap(),
            crate::model::EntryData {
                source,
                title: title.into(),
                subtitle: String::new(),
                path: Some(OwnedPath::process(PathBuf::from(title))),
                workspace_id: None,
                workspace_label: None,
                agent: (source == Source::Agent).then(|| crate::model::AgentMetadata {
                    agent: Some(title.into()),
                    name: None,
                    display_agent: None,
                    aliases: vec![],
                    status: "unknown".into(),
                    revision: 0,
                    session: None,
                }),
                project: None,
                action: if source == Source::Workspace {
                    EntryAction::FocusWorkspace { id: title.into() }
                } else {
                    EntryAction::FocusOrCreateDir
                },
                source_label: None,
                search_terms: vec![],
                provenance: [ProviderId::new("test").unwrap()].into(),
            },
        )
    }

    fn buffer_text(terminal: &Terminal<TestBackend>) -> String {
        let buffer = terminal.backend().buffer();
        (0..buffer.area.height)
            .map(|y| {
                (0..buffer.area.width)
                    .map(|x| buffer[(x, y)].symbol())
                    .collect::<String>()
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    fn set_merged_key(config: &mut Config, command: &Command, value: &str) {
        match command {
            Command::FocusCycle => config.picker.focus_cycle_key = value.into(),
            Command::Collapse => config.picker.collapse_key = value.into(),
            Command::Expand => config.picker.expand_key = value.into(),
            Command::TechnicalDetails => config.picker.technical_details_key = value.into(),
            _ => panic!("not a merged command"),
        }
    }

    fn merged_help_text(app: &mut App, focus: crate::keymap::Focus, menu: bool) -> String {
        let mut input = TuiInputSession {
            state: InputState::merged(&app.config.picker, &app.query),
            merged: None,
        };
        input.state.focus = focus;
        if menu {
            assert!(matches!(
                handle_key(app, &mut input, key(KeyCode::Enter)),
                Action::Continue
            ));
            assert!(input.state.menu.is_some());
        }
        assert!(matches!(
            handle_key(app, &mut input, key(KeyCode::Char('?'))),
            Action::Continue
        ));
        assert!(input.state.help_visible());
        let mut terminal = Terminal::new(TestBackend::new(160, 32)).unwrap();
        terminal
            .draw(|frame| draw_with_status(frame, app, &input.descriptor(), None))
            .unwrap();
        buffer_text(&terminal)
    }

    struct TestState(PathBuf);

    impl TestState {
        fn new() -> Self {
            use std::sync::atomic::{AtomicU64, Ordering};
            static NEXT: AtomicU64 = AtomicU64::new(0);
            let directory = std::env::temp_dir().join(format!(
                "s01-state-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            std::fs::create_dir(&directory).unwrap();
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                std::fs::set_permissions(&directory, std::fs::Permissions::from_mode(0o700))
                    .unwrap();
            }
            Self(directory)
        }
        fn bytes(&self) -> std::collections::BTreeMap<String, Vec<u8>> {
            std::fs::read_dir(&self.0)
                .unwrap()
                .map(|entry| {
                    let path = entry.unwrap().path();
                    (
                        path.file_name().unwrap().to_string_lossy().into_owned(),
                        std::fs::read(path).unwrap(),
                    )
                })
                .collect()
        }
    }
    impl Drop for TestState {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    #[derive(Debug, PartialEq, Eq)]
    struct ViewState {
        identity: Option<EntryIdentity>,
        title: Option<String>,
        query: String,
        filter: Option<Source>,
        details: bool,
        vim: VimState,
        help: bool,
    }
    impl ViewState {
        fn capture(app: &App, input: &Descriptor) -> Self {
            Self {
                identity: app.selected_entry().map(|e| e.key().clone()),
                title: app.selected_entry().map(|e| e.title.clone()),
                query: app.query.clone(),
                filter: app.source_filter,
                details: input.details.visible,
                vim: input.vim,
                help: input.help,
            }
        }

        fn capture_default(app: &App) -> Self {
            let input = InputState::legacy(&app.config.picker, &app.query).descriptor();
            Self::capture(app, &input)
        }
    }

    struct FakeTerminal {
        terminal: Terminal<TestBackend>,
        events: std::collections::VecDeque<Event>,
        operations: Vec<&'static str>,
        failures: Vec<(&'static str, usize)>,
        frames: Vec<(ViewState, String)>,
        prompts: Vec<String>,
        idle_poll: bool,
    }
    impl FakeTerminal {
        fn new(events: impl IntoIterator<Item = Event>) -> Self {
            Self {
                terminal: Terminal::new(TestBackend::new(160, 24)).unwrap(),
                events: events.into_iter().collect(),
                operations: vec![],
                failures: vec![],
                frames: vec![],
                prompts: vec![],
                idle_poll: false,
            }
        }
        fn step(&mut self, operation: &'static str) -> io::Result<()> {
            self.operations.push(operation);
            let count = self
                .operations
                .iter()
                .filter(|op| **op == operation)
                .count();
            if self.failures.contains(&(operation, count)) {
                Err(io::Error::other(format!("injected {operation} {count}")))
            } else {
                Ok(())
            }
        }
    }
    impl TerminalInput for FakeTerminal {
        fn raw(&mut self, enabled: bool) -> io::Result<()> {
            self.step(if enabled { "raw-on" } else { "raw-off" })
        }
        fn alternate(&mut self, enabled: bool) -> io::Result<()> {
            self.step(if enabled { "alt-on" } else { "alt-off" })
        }
        fn cursor(&mut self) -> io::Result<()> {
            self.step("cursor")
        }
        fn initialize(&mut self) -> io::Result<()> {
            self.step("initialize")
        }
        fn clear(&mut self) -> io::Result<()> {
            self.step("clear")
        }
        fn draw(&mut self, app: &App, input: &Descriptor, status: Option<&str>) -> io::Result<()> {
            self.step("draw")?;
            self.terminal
                .draw(|frame| draw_with_status(frame, app, input, status))
                .unwrap();
            self.frames
                .push((ViewState::capture(app, input), buffer_text(&self.terminal)));
            Ok(())
        }
        fn poll(&mut self, _: Duration) -> io::Result<bool> {
            self.step("poll")?;
            Ok(!std::mem::take(&mut self.idle_poll))
        }
        fn read(&mut self) -> io::Result<Event> {
            self.step("read")?;
            self.events
                .pop_front()
                .ok_or_else(|| io::Error::other("test input exhausted"))
        }
        fn confirm(&mut self, prompt: &str) -> io::Result<bool> {
            self.prompts.push(prompt.into());
            self.step("confirm")?;
            Ok(false)
        }
    }

    fn press(code: KeyCode) -> Event {
        Event::Key(key(code))
    }

    fn queue_lists(
        runner: &crate::herdr::RecordingRunner,
        lists: &[serde_json::Value; 4],
        count: usize,
    ) {
        runner.replies.lock().unwrap().extend(
            lists
                .iter()
                .take(count)
                .map(|list| Ok(serde_json::to_vec(list).unwrap())),
        );
    }

    fn loop_app(
        state: &TestState,
        socket: Option<PathBuf>,
    ) -> (App, std::sync::Arc<crate::herdr::RecordingRunner>) {
        let mut route = crate::herdr::invocation();
        route.socket = socket;
        route.managed_plugin = false;
        loop_app_with_invocation(state, route)
    }

    fn fixture_invocation(state: &TestState) -> crate::herdr::Invocation {
        crate::herdr::Invocation::for_test(
            state.0.join("herdr-fixture"),
            state.0.clone(),
            Vec::new(),
        )
    }

    fn loop_app_with_invocation(
        state: &TestState,
        route: crate::herdr::Invocation,
    ) -> (App, std::sync::Arc<crate::herdr::RecordingRunner>) {
        let mut config = Config::default();
        config.picker.preview = false;
        config.notifications.enabled = false;
        config.sources.herdr_plus_projects = false;
        config.sources.herdr_plus_quick_actions = false;
        config.sources.zoxide = false;
        config.sources.roots = false;
        config.sources.servers = false;
        config.sources.sessions = false;
        config.sources.open_workspaces = false;
        config.sources.panes = true;
        let mut app = App::with_history(config, Theme::load(false), state.0.clone());
        let runner = std::sync::Arc::new(crate::herdr::RecordingRunner::default());
        app.navigation = crate::navigation::NavigationContext::with_server(
            crate::navigation::server::OrdinaryServer::new(
                route,
                Box::new(runner.clone()),
                crate::herdr::Limits::default(),
            ),
        );
        (app, runner)
    }

    #[cfg(unix)]
    #[test]
    fn failed_focus_keeps_overlay_and_side_picker_usable_without_replay_or_history_writes() {
        use crate::navigation::server::tests::{snapshot, Reply, Socket};
        for persist in [false, true] {
            for case in [
                "rejected",
                "lost-ack",
                "malformed",
                "timeout",
                "unsupported",
            ] {
                let reply = match case {
                    "rejected" => Reply::Error("target_not_found"),
                    "lost-ack" => Reply::WrongId,
                    "malformed" => Reply::Bytes(b"invalid\n".to_vec()),
                    "timeout" => Reply::Hang,
                    _ => Reply::Error("unknown_method"),
                };
                let socket = Socket::new(vec![Reply::Result(snapshot()), reply]);
                let state = TestState::new();
                std::fs::write(state.0.join("pinned-entries.json"), b"[\"workspace:w43\"]")
                    .unwrap();
                std::fs::write(state.0.join("jump-back-workspace"), b"w43\n").unwrap();
                use std::os::unix::fs::PermissionsExt;
                for name in ["pinned-entries.json", "jump-back-workspace"] {
                    std::fs::set_permissions(
                        state.0.join(name),
                        std::fs::Permissions::from_mode(0o600),
                    )
                    .unwrap();
                }
                let (mut app, runner) = loop_app(&state, Some(socket.path.clone()));
                let lists = crate::navigation_test_support::ordinary_lists();
                queue_lists(&runner, &lists, 4);
                app.refresh();
                app.query = "w43".into();
                app.set_filter(Some(app.entries[0].source));
                app.apply_filter();
                app.config.picker.preview = persist;
                let before = ViewState::capture_default(&app);
                let history = state.bytes();
                queue_lists(&runner, &lists, 3);
                app.config.notifications.enabled = true;
                runner.replies.lock().unwrap().push_back(Ok(vec![]));
                let mut terminal = FakeTerminal::new([
                    press(KeyCode::Enter),
                    Event::Resize(160, 24),
                    press(KeyCode::Esc),
                    press(KeyCode::Esc),
                ]);
                let start = std::time::Instant::now();
                run_terminal(&mut app, persist, None, &mut terminal).unwrap();
                if case == "timeout" {
                    assert!(start.elapsed() >= crate::herdr::Limits::default().deadline);
                    assert!(start.elapsed() < Duration::from_secs(5));
                }
                assert_eq!(terminal.frames.len(), if persist { 4 } else { 3 }, "{case}");
                for (view, text) in terminal.frames.iter().take(3) {
                    assert_eq!(view, &before, "{case}");
                    assert!(text.contains("Server scope: owner unknown"));
                    assert!(!text.contains(&socket.path.display().to_string()));
                }
                let failure = &terminal.frames[1].1;
                assert!(failure.contains("Open failed:"), "{failure}");
                if case == "timeout" {
                    assert!(
                        failure.contains("ordinary-server deadline exceeded (Unknown)"),
                        "{failure}"
                    );
                }
                assert!(
                    failure.contains(if matches!(case, "rejected" | "unsupported") {
                        "Rejected"
                    } else {
                        "Unknown"
                    }),
                    "{failure}"
                );
                assert!(!failure.contains("press enter to continue"));
                assert_eq!(state.bytes(), history);
                assert!(app.previous_workspace_id.is_none());
                assert_eq!(
                    terminal
                        .operations
                        .iter()
                        .filter(|op| **op == "alt-on")
                        .count(),
                    1
                );
                let requests = socket.requests.lock().unwrap();
                assert_eq!(requests.len(), 2);
                assert_eq!(requests[0]["method"], "session.snapshot");
                assert_eq!(requests[1]["method"], "pane.focus");
                assert_eq!(
                    requests[1]["params"],
                    serde_json::json!({"pane_id":"w43:p1"})
                );
                let calls = runner.calls.lock().unwrap();
                assert_eq!(calls.len(), 8);
                assert!(calls[..7].iter().all(|call| call.args[1] == "list"));
                assert_eq!(calls[7].args[0], "notification");
                assert!(calls
                    .iter()
                    .all(|call| call.invocation.socket.as_ref() == Some(&socket.path)));
                assert!(runner.replies.lock().unwrap().is_empty());
            }
        }
    }

    #[test]
    fn removed_reparented_stale_and_unavailable_panes_fail_before_any_mutation() {
        for persist in [false, true] {
            for case in ["removed", "reparented", "stale", "unavailable"] {
                let state = TestState::new();
                let route =
                    (case != "unavailable").then(|| state.0.join("must-not-be-contacted.sock"));
                let (mut app, runner) = loop_app(&state, route);
                let mut lists = crate::navigation_test_support::ordinary_lists();
                queue_lists(&runner, &lists, 4);
                app.refresh();
                let before = ViewState::capture_default(&app);
                let history = state.bytes();
                match case {
                    "removed" => lists[1]["result"]["panes"] = serde_json::json!([]),
                    "reparented" => lists[1]["result"]["panes"][0]["tab_id"] = "another-tab".into(),
                    "stale" => app.navigation.invalidate_scope(),
                    _ => {}
                }
                if matches!(case, "removed" | "reparented") {
                    queue_lists(&runner, &lists, 3);
                }
                let count = if matches!(case, "removed" | "reparented") {
                    7
                } else {
                    4
                };
                let mut terminal = FakeTerminal::new([
                    press(KeyCode::Enter),
                    Event::Resize(160, 24),
                    press(KeyCode::Esc),
                ]);
                run_terminal(&mut app, persist, None, &mut terminal).unwrap();
                assert_eq!(terminal.frames[1].0, before);
                assert!(terminal.frames[1].1.contains("Open failed:"));
                assert_eq!(runner.calls.lock().unwrap().len(), count);
                assert_eq!(state.bytes(), history);
            }
        }
    }

    #[cfg(unix)]
    #[test]
    fn successful_focus_closes_overlay_but_side_refresh_retains_the_selected_identity() {
        use crate::navigation::server::tests::{snapshot, Reply, Socket};
        for persist in [false, true] {
            let socket = Socket::new(vec![
                Reply::Result(snapshot()),
                Reply::Result(serde_json::json!({"type":"ok"})),
            ]);
            let state = TestState::new();
            let (mut app, runner) = loop_app(&state, Some(socket.path.clone()));
            let lists = crate::navigation_test_support::ordinary_lists();
            queue_lists(&runner, &lists, 4);
            app.refresh();
            app.query = "w43".into();
            app.apply_filter();
            app.config.picker.preview = false;
            let before = ViewState::capture_default(&app);
            queue_lists(&runner, &lists, 3);
            if persist {
                queue_lists(&runner, &lists, 4);
            }
            let mut terminal = FakeTerminal::new([press(KeyCode::Enter), press(KeyCode::Esc)]);
            run_terminal(&mut app, persist, None, &mut terminal).unwrap();
            assert_eq!(terminal.frames.len(), if persist { 2 } else { 1 });
            assert_eq!(ViewState::capture_default(&app), before);
            assert_eq!(
                runner.calls.lock().unwrap().len(),
                if persist { 11 } else { 7 }
            );
            assert_eq!(socket.requests.lock().unwrap().len(), 2);
            assert!(terminal
                .operations
                .ends_with(&["raw-off", "alt-off", "cursor"]));
        }
    }

    #[test]
    fn every_early_terminal_failure_attempts_all_restorations_and_keeps_original_error() {
        for operation in ["raw-on", "alt-on", "initialize", "draw", "poll", "read"] {
            let state = TestState::new();
            let (mut app, runner) = loop_app(&state, None);
            app.animate_spinner = true;
            let mut terminal = FakeTerminal::new([press(KeyCode::Esc)]);
            terminal.failures = vec![
                (operation, 1),
                ("raw-off", 1),
                ("alt-off", 1),
                ("cursor", 1),
            ];
            let error = run_terminal(&mut app, false, None, &mut terminal).unwrap_err();
            assert_eq!(error.to_string(), format!("injected {operation} 1"));
            assert!(terminal
                .operations
                .ends_with(&["raw-off", "alt-off", "cursor"]));
            assert!(runner.calls.lock().unwrap().is_empty());
            assert!(app.open_selected(false).is_err());
        }
        for failed in ["raw-off", "alt-off", "cursor"] {
            let state = TestState::new();
            let (mut app, _) = loop_app(&state, None);
            let mut terminal = FakeTerminal::new([press(KeyCode::Esc)]);
            terminal.failures.push((failed, 1));
            assert_eq!(
                run_terminal(&mut app, false, None, &mut terminal)
                    .unwrap_err()
                    .to_string(),
                format!("injected {failed} 1")
            );
            assert!(terminal
                .operations
                .ends_with(&["raw-off", "alt-off", "cursor"]));
        }
    }

    #[test]
    fn confirmation_suspend_and_resume_failures_restore_the_terminal_without_installing_updates() {
        for (operation, occurrence) in [
            ("raw-off", 1),
            ("alt-off", 1),
            ("cursor", 1),
            ("confirm", 1),
            ("raw-on", 2),
            ("alt-on", 2),
            ("clear", 1),
        ] {
            let state = TestState::new();
            let (mut app, runner) = loop_app(&state, None);
            app.update_available = Some("test-only-never-installed".into());
            let mut terminal = FakeTerminal::new([press(KeyCode::F(5))]);
            terminal.failures.push((operation, occurrence));
            assert_eq!(
                run_terminal(&mut app, false, None, &mut terminal)
                    .unwrap_err()
                    .to_string(),
                format!("injected {operation} {occurrence}")
            );
            assert!(terminal
                .operations
                .ends_with(&["raw-off", "alt-off", "cursor"]));
            assert!(runner.calls.lock().unwrap().is_empty());
        }
    }

    #[test]
    fn failed_process_run_resumes_and_draws_error_before_reading_next_key() {
        for persist in [false, true] {
            let state = TestState::new();
            let (mut app, runner) = loop_app(&state, None);
            let mut row = entry(Source::Integration, "test command");
            row.action = EntryAction::RunCommand {
                command: "exit 7".into(),
                timeout_ms: 100,
                notify_success: false,
                notify_error: false,
            };
            app.entries = vec![row];
            app.prepare_actions();
            app.apply_filter();
            let mut terminal = FakeTerminal::new([press(KeyCode::Enter), press(KeyCode::Esc)]);
            run_terminal(&mut app, persist, None, &mut terminal).unwrap();
            assert_eq!(
                terminal.operations,
                [
                    "raw-on",
                    "alt-on",
                    "initialize",
                    "draw",
                    "read",
                    "raw-off",
                    "alt-off",
                    "cursor",
                    "raw-on",
                    "alt-on",
                    "clear",
                    "draw",
                    "read",
                    "raw-off",
                    "alt-off",
                    "cursor"
                ]
            );
            assert!(terminal.frames[1].1.contains("Open failed:"));
            assert_eq!(terminal.frames[0].0, terminal.frames[1].0);
            assert!(runner.calls.lock().unwrap().is_empty());
        }
    }

    #[test]
    fn idle_redraw_does_not_commit_or_execute_an_action_before_a_successful_draw() {
        let state = TestState::new();
        let (mut app, runner) = loop_app(&state, None);
        app.animate_spinner = true;
        let mut terminal = FakeTerminal::new([press(KeyCode::Esc)]);
        terminal.idle_poll = true;
        run_terminal(&mut app, false, None, &mut terminal).unwrap();
        assert_eq!(terminal.frames.len(), 2);
        assert_eq!(app.spinner_tick, 1);
        assert!(runner.calls.lock().unwrap().is_empty());
    }

    #[test]
    fn changed_focus_to_create_is_only_displayed_after_failure_never_dispatched_by_redraw() {
        for persist in [false, true] {
            let state = TestState::new();
            let (mut app, runner) = loop_app(&state, None);
            let mut route = crate::herdr::invocation();
            route.socket = Some(state.0.join("must-not-be-contacted.sock"));
            route.managed_plugin = true;
            app.navigation = crate::navigation::NavigationContext::with_server(
                crate::navigation::server::OrdinaryServer::new(
                    route,
                    Box::new(runner.clone()),
                    crate::herdr::Limits::default(),
                ),
            );
            let mut lists = crate::navigation_test_support::ordinary_lists();
            lists[0]["result"]["workspaces"]
                .as_array_mut()
                .unwrap()
                .truncate(1);
            lists[0]["result"]["workspaces"][0]["workspace_id"] = "w43".into();
            lists[0]["result"]["workspaces"][0]["active_tab_id"] = "w43:t1".into();
            lists[0]["result"]["workspaces"][0]["label"] = "dir: tmp".into();
            lists[1]["result"]["panes"][0]["cwd"] = "/tmp".into();
            lists[1]["result"]["panes"][0]["foreground_cwd"] = serde_json::Value::Null;
            queue_lists(&runner, &lists, 4);
            app.refresh();
            let mut row = entry(Source::Root, "tmp");
            row.path = Some(OwnedPath::process(PathBuf::from("/tmp")));
            app.entries = vec![row];
            app.prepare_actions();
            app.apply_filter();
            let before = ViewState::capture_default(&app);
            let history = state.bytes();
            lists[0]["result"]["workspaces"] = serde_json::json!([]);
            lists[1]["result"]["panes"] = serde_json::json!([]);
            lists[2]["result"]["tabs"] = serde_json::json!([]);
            queue_lists(&runner, &lists, 3);
            let mut terminal = FakeTerminal::new([
                press(KeyCode::Enter),
                Event::Resize(160, 24),
                press(KeyCode::Esc),
            ]);
            run_terminal(&mut app, persist, None, &mut terminal).unwrap();
            assert!(
                terminal.frames[0].1.contains("Focus workspace"),
                "{}",
                terminal.frames[0].1
            );
            assert!(terminal.frames[1].1.contains("action changed"));
            assert!(terminal.frames[1].1.contains("Open folder"));
            assert!(terminal.frames.iter().all(|(view, _)| view == &before));
            assert_eq!(runner.calls.lock().unwrap().len(), 7);
            assert_eq!(state.bytes(), history);
        }
    }

    #[test]
    fn failed_draw_never_authorizes_the_selected_action() {
        let state = TestState::new();
        let (mut app, runner) = loop_app(&state, None);
        let mut row = entry(Source::QuickAction, "test action");
        row.action = EntryAction::InvokePluginAction {
            action: "test.only".into(),
        };
        app.entries = vec![row];
        app.prepare_actions();
        app.apply_filter();
        let mut terminal = FakeTerminal::new([press(KeyCode::Enter)]);
        terminal.failures.push(("draw", 1));
        assert!(run_terminal(&mut app, false, None, &mut terminal).is_err());
        assert!(app
            .open_selected(false)
            .unwrap_err()
            .contains("has not been displayed"));
        assert!(runner.calls.lock().unwrap().is_empty());
        assert_eq!(terminal.events.len(), 1);
    }

    #[test]
    fn mark_error_uses_one_frozen_notification_and_remains_visible_when_notification_fails() {
        let state = TestState::new();
        let socket = state.0.join("frozen-route.sock");
        let (mut app, runner) = loop_app(&state, Some(socket.clone()));
        app.config.notifications.enabled = true;
        runner
            .replies
            .lock()
            .unwrap()
            .push_back(Err(crate::herdr::TransportError::new(
                crate::herdr::Failure::Timeout,
                crate::herdr::MutationOutcome::NotSent,
            )));
        let mut terminal = FakeTerminal::new([
            Event::Key(KeyEvent::new(KeyCode::Char('b'), KeyModifiers::CONTROL)),
            press(KeyCode::Esc),
        ]);
        run_terminal(&mut app, false, None, &mut terminal).unwrap();
        assert!(terminal.frames[1]
            .1
            .contains("Mark failed: nothing selected"));
        let calls = runner.calls.lock().unwrap();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].args[0], "notification");
        assert_eq!(calls[0].invocation.socket.as_ref(), Some(&socket));
    }

    #[cfg(unix)]
    struct AttachFixture {
        state: TestState,
        executable: PathBuf,
        decoy: PathBuf,
        cwd: PathBuf,
        calls: PathBuf,
        context: PathBuf,
    }

    #[cfg(unix)]
    impl AttachFixture {
        fn new() -> Self {
            use std::os::unix::fs::PermissionsExt;
            let state = TestState::new();
            let root = state.0.canonicalize().unwrap();
            let executable = root.join("fake-herdr");
            let decoy = root.join("decoy-herdr");
            let cwd = root.join("attach cwd");
            std::fs::create_dir(&cwd).unwrap();
            std::fs::write(
                &executable,
                "#!/bin/sh\nset -eu\nprintf '%s\\n' \"$@\" >> \"$ATTACH_CALLS\"\n{ printf '%s\\n' intended \"$ATTACH_TOKEN\"; pwd -P; } >> \"$ATTACH_CONTEXT\"\nexit 7\n",
            ).unwrap();
            std::fs::write(
                &decoy,
                "#!/bin/sh\nset -eu\nprintf 'decoy\\n%s\\n' \"$*\" >> \"${0%/*}/decoy-calls\"\nexit 23\n",
            ).unwrap();
            for path in [&executable, &decoy] {
                std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700)).unwrap();
            }
            Self {
                state,
                executable,
                decoy,
                cwd,
                calls: root.join("attach calls"),
                context: root.join("attach context"),
            }
        }

        fn invocation(&self) -> crate::herdr::Invocation {
            crate::herdr::Invocation::for_test(
                self.executable.clone(),
                self.cwd.clone(),
                vec![
                    ("ATTACH_CALLS".into(), self.calls.clone().into_os_string()),
                    (
                        "ATTACH_CONTEXT".into(),
                        self.context.clone().into_os_string(),
                    ),
                    ("ATTACH_TOKEN".into(), "fixture-only".into()),
                ],
            )
        }

        fn assert_failed_attach(
            &self,
            mut app: App,
            runner: &crate::herdr::RecordingRunner,
            persist: bool,
        ) {
            let mut row = entry(Source::Session, "test session");
            row.action = EntryAction::AttachSession {
                name: "synthetic-only".into(),
                remote: None,
            };
            app.entries = vec![row];
            app.query = "test".into();
            app.set_filter(Some(Source::Session));
            app.config.picker.preview = persist;
            app.prepare_actions();
            app.apply_filter();
            assert!(app
                .open_selected(false)
                .unwrap_err()
                .contains("has not been displayed"));
            assert!(!self.calls.exists());
            let before = ViewState::capture_default(&app);
            let mut events = vec![
                press(KeyCode::Enter),
                Event::Resize(160, 24),
                press(KeyCode::Esc),
            ];
            if persist {
                events.push(press(KeyCode::Esc));
            }
            let mut terminal = FakeTerminal::new(events);
            run_terminal(&mut app, persist, None, &mut terminal).unwrap();
            assert_eq!(terminal.frames.len(), if persist { 4 } else { 3 });
            for (view, _) in terminal.frames.iter().take(3) {
                assert_eq!(view, &before);
            }
            if persist {
                assert!(!terminal.frames[3].0.details);
                assert_eq!(terminal.frames[3].0.identity, before.identity);
                assert_eq!(terminal.frames[3].0.query, before.query);
            }
            assert!(
                terminal.frames[1]
                    .1
                    .contains("Open failed: herdr exited with exit status: 7"),
                "{}",
                terminal.frames[1].1
            );
            assert_eq!(terminal.frames[1].1, terminal.frames[2].1);
            assert!(terminal.operations.windows(10).any(|ops| ops
                == [
                    "draw", "read", "raw-off", "alt-off", "cursor", "raw-on", "alt-on", "clear",
                    "draw", "read"
                ]));
            assert!(terminal
                .operations
                .ends_with(&["raw-off", "alt-off", "cursor"]));
            assert_eq!(
                terminal
                    .operations
                    .iter()
                    .filter(|op| **op == "raw-off")
                    .count(),
                2
            );
            assert!(terminal.events.is_empty());
            assert_eq!(
                std::fs::read_to_string(&self.calls).unwrap(),
                "session\nattach\nsynthetic-only\n"
            );
            assert_eq!(
                std::fs::read_to_string(&self.context).unwrap(),
                format!("intended\nfixture-only\n{}\n", self.cwd.display())
            );
            assert!(!self.decoy.with_file_name("decoy-calls").exists());
            assert!(runner.calls.lock().unwrap().is_empty());
        }
    }

    #[cfg(unix)]
    #[test]
    fn failed_interactive_attach_resumes_and_never_replays_on_redraw() {
        for persist in [false, true] {
            let fixture = AttachFixture::new();
            let root = fixture.state.0.clone();
            let (app, runner) = loop_app_with_invocation(&fixture.state, fixture.invocation());
            fixture.assert_failed_attach(app, &runner, persist);
            drop(fixture);
            assert!(!root.exists());
        }
    }

    #[cfg(unix)]
    #[test]
    fn interactive_attach_ignores_ambient_executable_changes() {
        let _lock = crate::herdr::bin_path_lock();
        struct RestoreExecutable(Option<std::ffi::OsString>);
        impl Drop for RestoreExecutable {
            fn drop(&mut self) {
                match &self.0 {
                    Some(value) => std::env::set_var("HERDR_BIN_PATH", value),
                    None => std::env::remove_var("HERDR_BIN_PATH"),
                }
            }
        }
        for persist in [false, true] {
            let fixture = AttachFixture::new();
            let root = fixture.state.0.clone();
            let previous = std::env::var_os("HERDR_BIN_PATH");
            let restore = RestoreExecutable(previous.clone());
            let route = fixture.invocation();
            // Contaminate ambient capture before App construction and keep it through dispatch.
            std::env::set_var("HERDR_BIN_PATH", &fixture.decoy);
            let (app, runner) = loop_app_with_invocation(&fixture.state, route);
            fixture.assert_failed_attach(app, &runner, persist);
            drop(restore);
            assert_eq!(std::env::var_os("HERDR_BIN_PATH"), previous);
            drop(fixture);
            assert!(!root.exists());
        }
    }

    #[test]
    fn unknown_picker_owner_ctrl_x_after_healthy_refresh_keeps_view_and_history() {
        for confirm in [true, false] {
            for id in ["w43", "w41"] {
                let state = TestState::new();
                let (mut app, runner) = loop_app(&state, None);
                app.config.notifications.enabled = true;
                app.config.sources.open_workspaces = true;
                app.config.sources.panes = false;
                app.config.sources.agents = false;
                app.config.picker.confirm_close_workspace = confirm;
                let owner = crate::model::LiveTarget::new(
                    LiveIdentity::new(app.navigation.scope(), ObjectKind::Workspace, "w43")
                        .unwrap(),
                    None,
                    None,
                )
                .unwrap();
                app.navigation.set_picker_owner(owner.clone()).unwrap();
                let lists = crate::navigation_test_support::ordinary_lists();
                queue_lists(&runner, &lists, 3);
                app.refresh();
                app.navigation.invalidate_scope();
                let scope = app.navigation.scope().clone();
                assert_ne!(owner.identity().scope(), &scope);
                queue_lists(&runner, &lists, 3);
                app.refresh();
                assert_eq!(app.navigation.scope(), &scope);
                assert!(app.navigation.picker_workspace().is_none());
                app.source_filter = Some(Source::Workspace);
                app.query = app
                    .entries
                    .iter()
                    .find(|entry| {
                        entry
                            .target()
                            .is_some_and(|target| target.identity().id() == id)
                    })
                    .unwrap()
                    .title
                    .clone();
                app.apply_filter();
                app.selected = app
                    .filtered
                    .iter()
                    .position(|index| {
                        app.entries[*index]
                            .target()
                            .is_some_and(|target| target.identity().id() == id)
                    })
                    .unwrap();
                let before = ViewState::capture_default(&app);
                let filtered = app.filtered.clone();
                let history = state.bytes();
                let mut terminal = FakeTerminal::new([
                    Event::Key(KeyEvent::new(KeyCode::Char('x'), KeyModifiers::CONTROL)),
                    Event::Resize(160, 24),
                    press(KeyCode::Esc),
                ]);
                run_terminal(&mut app, false, None, &mut terminal).unwrap();
                assert!(terminal.prompts.is_empty());
                assert_eq!(terminal.frames.len(), 3);
                assert!(terminal
                    .frames
                    .iter()
                    .all(|(view, text)| { view == &before && !text.contains("Close failed") }));
                assert_eq!(ViewState::capture_default(&app), before);
                assert_eq!(app.filtered, filtered);
                assert!(app.close_description().is_none());
                assert_eq!(state.bytes(), history);
                let calls = runner.calls.lock().unwrap();
                assert_eq!(calls.len(), 6);
                assert!(calls
                    .iter()
                    .all(|call| call.args.get(1).is_some_and(|arg| arg == "list")));
            }
        }
    }

    #[test]
    fn close_confirmation_failure_and_decline_restore_without_closing_a_workspace() {
        for failure in [None, Some(("confirm", 1)), Some(("clear", 1))] {
            let state = TestState::new();
            let (mut app, runner) =
                loop_app(&state, Some(state.0.join("must-not-be-contacted.sock")));
            app.config.sources.open_workspaces = true;
            app.config.sources.panes = false;
            app.config.sources.agents = false;
            app.config.picker.confirm_close_workspace = true;
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
            queue_lists(
                &runner,
                &crate::navigation_test_support::ordinary_lists(),
                3,
            );
            app.refresh();
            let before = ViewState::capture_default(&app);
            let mut terminal = FakeTerminal::new([
                Event::Key(KeyEvent::new(KeyCode::Char('x'), KeyModifiers::CONTROL)),
                press(KeyCode::Esc),
            ]);
            terminal.failures.extend(failure);
            let result = run_terminal(&mut app, false, None, &mut terminal);
            assert_eq!(result.is_err(), failure.is_some());
            assert_eq!(terminal.prompts.len(), 1);
            assert!(terminal.prompts[0].starts_with("Close workspace"));
            assert!(terminal
                .operations
                .ends_with(&["raw-off", "alt-off", "cursor"]));
            assert_eq!(ViewState::capture_default(&app), before);
            assert_eq!(runner.calls.lock().unwrap().len(), 3);
        }
    }

    #[test]
    fn action_error_survives_resume_failure_and_all_final_restorations_are_attempted() {
        for failure in [("raw-on", 2), ("alt-on", 2), ("clear", 1)] {
            let state = TestState::new();
            let (mut app, runner) = loop_app(&state, None);
            let mut row = entry(Source::Integration, "test command");
            row.action = EntryAction::RunCommand {
                command: "exit 7".into(),
                timeout_ms: 100,
                notify_success: false,
                notify_error: false,
            };
            app.entries = vec![row];
            app.prepare_actions();
            app.apply_filter();
            let mut terminal = FakeTerminal::new([press(KeyCode::Enter)]);
            terminal.failures = vec![failure, ("raw-off", 2), ("alt-off", 2), ("cursor", 2)];
            let error = run_terminal(&mut app, false, None, &mut terminal)
                .unwrap_err()
                .to_string();
            assert!(error.contains("exit status: 7"), "{error}");
            assert!(error.contains(&format!(
                "terminal resume failed: injected {} {}",
                failure.0, failure.1
            )));
            assert!(terminal
                .operations
                .ends_with(&["raw-off", "alt-off", "cursor"]));
            assert!(runner.calls.lock().unwrap().is_empty());
        }
    }

    #[test]
    fn herdr_plus_sources_share_color() {
        let theme = Theme::load(false);
        assert_eq!(
            source_color(&theme, &Source::Project),
            source_color(&theme, &Source::QuickAction)
        );
    }

    #[derive(Debug, PartialEq, Eq)]
    struct ModifierAdapterSnapshot {
        input: String,
        selected: usize,
        selected_identity: Option<EntryIdentity>,
        entry_identities: Vec<EntryIdentity>,
        filtered: Vec<usize>,
        filtered_scores: Vec<i64>,
        query: String,
        filter: Option<Source>,
        pinned: std::collections::HashSet<EntryIdentity>,
        update_available: Option<String>,
        history: std::collections::BTreeMap<String, Vec<u8>>,
        runner_calls: usize,
    }

    fn modifier_adapter_snapshot(
        app: &App,
        input: &TuiInputSession,
        state: &TestState,
        runner: &crate::herdr::RecordingRunner,
    ) -> ModifierAdapterSnapshot {
        ModifierAdapterSnapshot {
            input: format!("{:?}", input.state),
            selected: app.selected,
            selected_identity: app.selected_entry().map(|entry| entry.key().clone()),
            entry_identities: app
                .entries
                .iter()
                .map(|entry| entry.key().clone())
                .collect(),
            filtered: app.filtered.clone(),
            filtered_scores: app.filtered_scores.clone(),
            query: app.query.clone(),
            filter: app.source_filter,
            pinned: app.pinned_entries.clone(),
            update_available: app.update_available.clone(),
            history: state.bytes(),
            runner_calls: runner.calls.lock().unwrap().len(),
        }
    }

    #[test]
    fn n10_modifier_legacy_adapter_rejects_unsupported_combinations_without_side_effects() {
        let state = TestState::new();
        let (mut app, runner) = loop_app_with_invocation(&state, fixture_invocation(&state));
        app.config.picker.directory_template = Some("default.toml".into());
        app.config.picker.directory_template_key = "ctrl-g".into();
        app.entries = vec![
            entry(Source::Zoxide, "alpha"),
            entry(Source::Zoxide, "bravo"),
        ];
        app.query = "aé🙂z".into();
        app.update_available = Some("fixture-update".into());
        app.apply_filter();
        let mut input = TuiInputSession::legacy(&app);
        assert!(matches!(
            handle_key(&mut app, &mut input, key(KeyCode::Left)),
            Action::Continue
        ));

        let ctrl_alt = KeyModifiers::CONTROL | KeyModifiers::ALT;
        let ctrl_shift = KeyModifiers::CONTROL | KeyModifiers::SHIFT;
        let alt_ctrl_shift = KeyModifiers::ALT | KeyModifiers::CONTROL | KeyModifiers::SHIFT;
        for event in [
            KeyEvent::new(KeyCode::Char('c'), ctrl_alt),
            KeyEvent::new(KeyCode::Char('c'), ctrl_shift),
            KeyEvent::new(KeyCode::Char('g'), ctrl_alt),
            KeyEvent::new(KeyCode::Char('g'), ctrl_shift),
            KeyEvent::new(KeyCode::Char('b'), ctrl_alt),
            KeyEvent::new(KeyCode::Enter, KeyModifiers::ALT | KeyModifiers::SHIFT),
            KeyEvent::new(KeyCode::Enter, KeyModifiers::ALT | KeyModifiers::CONTROL),
            KeyEvent::new(KeyCode::Tab, KeyModifiers::SHIFT),
            KeyEvent::new(KeyCode::Left, alt_ctrl_shift),
            KeyEvent::new(KeyCode::Right, alt_ctrl_shift),
            KeyEvent::new(KeyCode::Home, alt_ctrl_shift),
            KeyEvent::new(KeyCode::End, alt_ctrl_shift),
            KeyEvent::new(KeyCode::Backspace, alt_ctrl_shift),
            KeyEvent::new(KeyCode::Delete, alt_ctrl_shift),
            KeyEvent::new(KeyCode::Esc, KeyModifiers::ALT),
            KeyEvent::new(KeyCode::F(5), KeyModifiers::ALT),
            KeyEvent::new(KeyCode::F(6), KeyModifiers::ALT),
            KeyEvent::new(KeyCode::F(7), KeyModifiers::ALT),
            KeyEvent::new(KeyCode::Char('?'), KeyModifiers::META),
            KeyEvent::new(KeyCode::Char('h'), KeyModifiers::SUPER),
            KeyEvent::new(
                KeyCode::Char('h'),
                KeyModifiers::HYPER | KeyModifiers::CONTROL,
            ),
        ] {
            let before = modifier_adapter_snapshot(&app, &input, &state, &runner);
            assert!(matches!(
                handle_key(&mut app, &mut input, event),
                Action::Continue
            ));
            assert_eq!(
                modifier_adapter_snapshot(&app, &input, &state, &runner),
                before,
                "adapter state changed for {event:?}"
            );
        }
    }

    #[test]
    fn n10_modifier_legacy_adapter_rejects_extra_ctrl_x_when_close_is_available() {
        let state = TestState::new();
        let (mut app, runner) = loop_app_with_invocation(&state, fixture_invocation(&state));
        app.config.sources.open_workspaces = true;
        app.config.sources.panes = false;
        app.config.sources.agents = false;
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
        let lists = crate::navigation_test_support::ordinary_lists();
        queue_lists(&runner, &lists, 3);
        app.refresh();
        let workspace_positions: Vec<_> = app
            .filtered
            .iter()
            .enumerate()
            .filter_map(|(position, index)| {
                (app.entries[*index].source == Source::Workspace).then_some(position)
            })
            .collect();
        let mut closable = None;
        for position in workspace_positions {
            app.selected = position;
            if app.can_close_selected() {
                closable = Some(position);
                break;
            }
        }
        app.selected = closable.expect("fixture must contain a closable workspace");
        app.update_available = Some("fixture-update".into());
        let mut input = TuiInputSession::legacy(&app);
        let before = modifier_adapter_snapshot(&app, &input, &state, &runner);
        assert!(matches!(
            handle_key(
                &mut app,
                &mut input,
                KeyEvent::new(
                    KeyCode::Char('x'),
                    KeyModifiers::CONTROL | KeyModifiers::ALT
                )
            ),
            Action::Continue
        ));
        assert_eq!(
            modifier_adapter_snapshot(&app, &input, &state, &runner),
            before
        );
    }

    #[test]
    fn n10_modifier_legacy_adapter_preserves_supported_binding_actions() {
        let state = TestState::new();
        let (mut app, runner) = loop_app_with_invocation(&state, fixture_invocation(&state));
        app.config.picker.directory_template = Some("default.toml".into());
        app.entries = vec![
            entry(Source::Zoxide, "alpha"),
            entry(Source::Zoxide, "bravo"),
        ];
        app.apply_filter();
        let identity = app.selected_entry().unwrap().key().clone();
        let mut input = TuiInputSession::legacy(&app);
        assert!(matches!(
            handle_key(&mut app, &mut input, key(KeyCode::Enter)),
            Action::Open(ActivationIntent { identity: actual, slot: crate::actions::Slot::Primary })
                if actual == identity
        ));
        assert!(matches!(
            handle_key(
                &mut app,
                &mut input,
                KeyEvent::new(KeyCode::Enter, KeyModifiers::ALT)
            ),
            Action::Open(ActivationIntent { identity: actual, slot: crate::actions::Slot::Template })
                if actual == identity
        ));
        app.config.picker.directory_template_key = "ctrl-g".into();
        assert!(matches!(
            handle_key(
                &mut app,
                &mut input,
                KeyEvent::new(KeyCode::Char('g'), KeyModifiers::CONTROL)
            ),
            Action::Open(ActivationIntent { identity: actual, slot: crate::actions::Slot::Template })
                if actual == identity
        ));

        input.state.vim = VimState::Search;
        assert!(matches!(
            handle_key(
                &mut app,
                &mut input,
                KeyEvent::new(KeyCode::Char('j'), KeyModifiers::ALT)
            ),
            Action::Continue
        ));
        assert_eq!(app.selected, 1);
        assert!(matches!(
            handle_key(
                &mut app,
                &mut input,
                KeyEvent::new(KeyCode::Char('k'), KeyModifiers::ALT)
            ),
            Action::Continue
        ));
        assert_eq!(app.selected, 0);
        assert!(matches!(
            handle_key(&mut app, &mut input, key(KeyCode::Char('?'))),
            Action::Continue
        ));
        assert!(input.state.help_visible());
        assert!(matches!(
            handle_key(
                &mut app,
                &mut input,
                KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL)
            ),
            Action::Quit
        ));
        assert!(input.state.help_visible());
        assert!(runner.calls.lock().unwrap().is_empty());
    }

    #[test]
    fn alt_enter_opens_selected_directory_with_configured_template() {
        let mut config = Config::default();
        config.picker.directory_template = Some("default.toml".into());
        let mut app = App::for_test(config, Theme::load(false));
        app.entries = vec![entry(Source::Zoxide, "/tmp")];
        app.apply_filter();
        let mut input = TuiInputSession::legacy(&app);

        assert!(matches!(
            handle_key(&mut app, &mut input, key(KeyCode::Enter)),
            Action::Open(ActivationIntent {
                slot: crate::actions::Slot::Primary,
                ..
            })
        ));
        assert!(matches!(
            handle_key(
                &mut app,
                &mut input,
                KeyEvent::new(KeyCode::Enter, KeyModifiers::ALT)
            ),
            Action::Open(ActivationIntent {
                slot: crate::actions::Slot::Template,
                ..
            })
        ));

        app.config.picker.directory_template_key = "ctrl-g".into();
        assert!(matches!(
            handle_key(
                &mut app,
                &mut input,
                KeyEvent::new(KeyCode::Char('g'), KeyModifiers::CONTROL)
            ),
            Action::Open(ActivationIntent {
                slot: crate::actions::Slot::Template,
                ..
            })
        ));
    }

    #[test]
    fn alt_j_and_alt_k_move_selection_while_searching() {
        let mut config = Config::default();
        config.picker.vim_mode = true;
        let mut app = App::for_test(config, Theme::load(false));
        app.entries = vec![
            entry(Source::Zoxide, "alpha"),
            entry(Source::Zoxide, "bravo"),
        ];
        app.apply_filter();
        let mut input = TuiInputSession::legacy(&app);
        assert_eq!(input.state.vim, crate::picker::input::VimState::Normal);
        assert_eq!(input.state.focus, crate::picker::input::Focus::Query);

        assert!(matches!(
            handle_key(&mut app, &mut input, key(KeyCode::Char('/'))),
            Action::Continue
        ));
        assert_eq!(input.state.vim, crate::picker::input::VimState::Search);
        assert!(matches!(
            handle_key(&mut app, &mut input, key(KeyCode::Char('a'))),
            Action::Continue
        ));
        assert_eq!(app.query, "a");
        assert_eq!(app.filtered.len(), 2);
        assert_eq!(input.state.cursor, 1);
        assert_eq!(input.state.focus, crate::picker::input::Focus::Query);
        let query_state = (
            app.query.clone(),
            input.state.cursor,
            input.state.focus,
            input.state.vim,
        );

        assert!(matches!(
            handle_key(
                &mut app,
                &mut input,
                KeyEvent::new(KeyCode::Char('j'), KeyModifiers::ALT),
            ),
            Action::Continue
        ));
        assert_eq!(app.selected, 1);
        assert_eq!(
            (
                app.query.clone(),
                input.state.cursor,
                input.state.focus,
                input.state.vim,
            ),
            query_state
        );

        assert!(matches!(
            handle_key(
                &mut app,
                &mut input,
                KeyEvent::new(KeyCode::Char('k'), KeyModifiers::ALT),
            ),
            Action::Continue
        ));
        assert_eq!(app.selected, 0);
        assert_eq!(
            (
                app.query.clone(),
                input.state.cursor,
                input.state.focus,
                input.state.vim,
            ),
            query_state
        );
    }

    #[test]
    fn legacy_adapter_uses_shared_cursor_editing_without_mirrored_app_mode() {
        let mut config = Config::default();
        config.picker.preview = false;
        let mut app = App::for_test(config, Theme::load(false));
        let mut input = TuiInputSession::legacy(&app);
        for ch in ['a', ' ', '🙂'] {
            assert!(matches!(
                handle_key(&mut app, &mut input, key(KeyCode::Char(ch))),
                Action::Continue
            ));
        }
        handle_key(&mut app, &mut input, key(KeyCode::Left));
        handle_key(&mut app, &mut input, key(KeyCode::Char('é')));
        assert_eq!(app.query, "a é🙂");
        assert_eq!(input.state.cursor, "a é".len());
        handle_key(&mut app, &mut input, key(KeyCode::Delete));
        assert_eq!(app.query, "a é");
        handle_key(&mut app, &mut input, key(KeyCode::Backspace));
        assert_eq!(app.query, "a ");
        assert!(app.query.is_char_boundary(input.state.cursor));
    }

    #[test]
    fn production_handler_unicode_and_shadowed_ctrl_o_only_change_query_and_filter() {
        let state = TestState::new();
        let (mut app, runner) = loop_app(&state, None);
        app.config.picker.preview = false;
        app.config
            .picker
            .filter_keys
            .insert("project".into(), "ctrl-o".into());
        let mut input = TuiInputSession::legacy(&app);
        let history = state.bytes();
        let calls = runner.calls.lock().unwrap().len();

        for ch in ['é', '\u{301}', '🙂'] {
            assert!(matches!(
                handle_key(&mut app, &mut input, key(KeyCode::Char(ch))),
                Action::Continue
            ));
        }
        handle_key(&mut app, &mut input, key(KeyCode::Left));
        handle_key(&mut app, &mut input, key(KeyCode::Backspace));
        assert_eq!(app.query, "é🙂");
        let details = input.state.details;
        assert!(matches!(
            handle_key(
                &mut app,
                &mut input,
                KeyEvent::new(KeyCode::Char('o'), KeyModifiers::CONTROL)
            ),
            Action::Continue
        ));
        assert_eq!(app.source_filter, Some(Source::Project));
        assert_eq!(app.query, "é🙂");
        assert_eq!(input.state.details, details);
        assert_eq!(runner.calls.lock().unwrap().len(), calls);
        assert_eq!(state.bytes(), history);
    }

    #[test]
    fn update_badge_renders_action_and_f5_triggers_it() {
        let mut app = App::for_test(Config::default(), Theme::load(false));
        let mut input = TuiInputSession::legacy(&app);
        assert!(matches!(
            handle_key(
                &mut app,
                &mut input,
                KeyEvent::new(KeyCode::F(5), crossterm::event::KeyModifiers::NONE)
            ),
            Action::Continue
        ));
        app.update_available = Some("0.3.2".into());

        let backend = TestBackend::new(70, 8);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal.draw(|f| draw(f, &app)).unwrap();

        assert!(buffer_text(&terminal).contains("↑ v0.3.2 available · F5 update"));
        assert!(matches!(
            handle_key(
                &mut app,
                &mut input,
                KeyEvent::new(KeyCode::F(5), crossterm::event::KeyModifiers::NONE)
            ),
            Action::Update
        ));
    }

    #[test]
    fn pane_rows_show_manual_title_agent_status_and_numeric_tab_breadcrumb() {
        for detailed in [false, true] {
            let mut config = Config::default();
            config.picker.detailed_rows = detailed;
            let mut app = App::for_test(config, Theme::load(false));
            app.entries = crate::sources::test_inventory_entries(&app.navigation);
            app.entries[0].title = "Review key handling".into();
            app.apply_filter();
            let mut terminal = Terminal::new(TestBackend::new(140, 12)).unwrap();
            terminal
                .draw(|frame| draw_list(frame, &app, frame.area()))
                .unwrap();
            let text = buffer_text(&terminal);
            assert!(text.contains("Review key handling"));
            assert!(text.contains("1 · claude · working"));
            assert!(text.contains(app.entries[0].workspace_label.as_deref().unwrap()));
            let preview = preview_text(&app, &app.entries[0]);
            assert!(preview.contains("Focus pane"));
            assert!(preview.contains("terminal metadata: term_1"));
            assert!(preview.contains("naming Unknown"));
            assert!(preview.contains("58f4-session"));
            assert!(!preview.contains("agent target"));
        }
    }

    #[test]
    fn inventory_errors_render_without_selected_rows_or_preview_and_empty_is_not_unknown() {
        use crate::navigation::inventory::ReadState;
        let mut app = App::for_test(Config::default(), Theme::load(false));
        app.config.picker.preview = false;
        let mut inventory = crate::sources::test_inventory(&app.navigation);
        inventory.agents.state = ReadState::Unavailable(crate::herdr::TransportError::new(
            crate::herdr::Failure::Unsupported,
            crate::herdr::MutationOutcome::NotSent,
        ));
        app.inventory = Some(inventory);
        let mut terminal = Terminal::new(TestBackend::new(140, 12)).unwrap();
        terminal.draw(|frame| draw(frame, &app)).unwrap();
        assert!(buffer_text(&terminal).contains("agents: unavailable"));
        app.inventory.as_mut().unwrap().agents.state = ReadState::Ready(Vec::new());
        assert_eq!(app.inventory.as_ref().unwrap().agents.count(), Some(0));
        assert!(app
            .inventory
            .as_ref()
            .unwrap()
            .agents
            .description("agents")
            .starts_with("agents: 0 observed"));
        app.navigation.invalidate_scope();
        app.inventory
            .as_mut()
            .unwrap()
            .discard_old_scope(app.navigation.scope());
        terminal.draw(|frame| draw(frame, &app)).unwrap();
        assert!(buffer_text(&terminal).contains("stale, discarded after scope change"));
    }

    #[test]
    fn status_colors_match_herdr() {
        let theme = Theme::load(false);

        assert_eq!(agent_status_color(&theme, "blocked"), theme.red);
        assert_eq!(agent_status_color(&theme, "working"), theme.yellow);
        assert_eq!(agent_status_color(&theme, "done"), theme.teal);
        assert_eq!(agent_status_color(&theme, "idle"), theme.green);
        assert_eq!(agent_status_color(&theme, "unknown"), theme.overlay0);
    }

    #[test]
    fn open_and_pinned_workspaces_replace_tree_branches_with_diamonds() {
        let mut app = App::for_test(Config::default(), Theme::load(false));
        let mut current = entry(Source::Workspace, "Current");
        current.workspace_id = Some("w1".into());
        current.search_terms.push("focused".into());
        let mut previous = entry(Source::Workspace, "Previous");
        previous.workspace_id = Some("w2".into());
        app.entries = vec![current, previous];
        app.filtered = vec![0, 1];
        app.filtered_scores = vec![0; 2];
        app.previous_workspace_id = Some(app.entries[1].key().clone());
        app.selected = 1;

        let backend = TestBackend::new(40, 10);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal.draw(|f| draw_list(f, &app, f.area())).unwrap();
        let text = buffer_text(&terminal);
        let buffer = terminal.backend().buffer();

        assert!(text.contains("  ◆  Current"));
        assert!(text.contains("  ◆  Previous"));
        assert!(!text.contains("├─ ◆"));
        assert!(buffer
            .content()
            .iter()
            .any(|cell| cell.symbol() == "◆" && cell.fg == app.theme.accent));
        assert!(buffer
            .content()
            .iter()
            .any(|cell| cell.symbol() == "◆" && cell.fg == app.theme.red));
    }

    #[test]
    fn current_workspace_marker_wins_over_stale_pin() {
        let mut app = App::for_test(Config::default(), Theme::load(false));
        let mut current = entry(Source::Workspace, "Current");
        current.workspace_id = Some("w1".into());
        current.search_terms.push("focused".into());
        app.previous_workspace_id = Some(current.key().clone());

        assert_eq!(
            entry_branch(&app, &current, false),
            ("  ◆  ", app.theme.accent)
        );
    }

    #[test]
    fn marked_entry_uses_a_yellow_diamond() {
        let mut app = App::for_test(Config::default(), Theme::load(false));
        let marked = entry(Source::Root, "Marked");
        app.pinned_entries.insert(marked.key().clone());

        assert_eq!(
            entry_branch(&app, &marked, false),
            ("  ◆  ", app.theme.yellow)
        );
    }

    #[test]
    fn mark_marker_wins_over_previous_workspace() {
        let mut app = App::for_test(Config::default(), Theme::load(false));
        let mut previous = entry(Source::Workspace, "Previous");
        previous.workspace_id = Some("w2".into());
        previous.action = EntryAction::FocusWorkspace { id: "w2".into() };
        app.previous_workspace_id = Some(previous.key().clone());
        app.pinned_entries.insert(previous.key().clone());

        assert_eq!(
            entry_branch(&app, &previous, false),
            ("  ◆  ", app.theme.yellow)
        );
    }

    #[test]
    fn mark_marker_wins_over_current_workspace() {
        let mut app = App::for_test(Config::default(), Theme::load(false));
        let mut current = entry(Source::Workspace, "Current");
        current.workspace_id = Some("w1".into());
        current.action = EntryAction::FocusWorkspace { id: "w1".into() };
        current.search_terms.push("focused".into());
        app.pinned_entries.insert(current.key().clone());

        assert_eq!(
            entry_branch(&app, &current, false),
            ("  ◆  ", app.theme.yellow)
        );
    }

    #[test]
    fn detailed_rows_expand_directories_and_pane_breadcrumbs() {
        let mut app = App::for_test(Config::default(), Theme::load(false));
        let mut workspace = entry(Source::Workspace, "dir: demo");
        workspace.path = Some(OwnedPath::process(PathBuf::from("/work/demo")));
        workspace.subtitle = "agent:blocked · w1 tabs:2 panes:3".into();
        let mut agent = crate::sources::test_inventory_entries(&app.navigation).remove(0);
        agent.title = "claude · demo".into();
        agent.workspace_label = Some("demo".into());
        let mut root = entry(Source::Root, "root-demo");
        root.path = Some(OwnedPath::process(PathBuf::from("/projects/root-demo")));
        root.subtitle = "/projects/root-demo".into();
        app.entries = vec![workspace, agent, root];
        app.filtered = vec![0, 1, 2];
        app.filtered_scores = vec![0, 0, 0];

        let backend = TestBackend::new(90, 10);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal.draw(|f| draw_list(f, &app, f.area())).unwrap();
        let text = buffer_text(&terminal);
        let workspace_line = text.lines().find(|line| line.contains("● demo")).unwrap();
        let agent_line = text
            .lines()
            .find(|line| line.contains("⠋ claude · demo"))
            .unwrap();

        assert!(!text.contains("/work/demo"));
        assert!(!workspace_line.contains("demo  blocked"));
        assert!(workspace_line.find("blocked · 2 tabs · 3 panes").unwrap() > 50);
        assert!(agent_line.find("working").unwrap() > 50);
        assert!(text.contains("demo · 1 · claude · working"));
        assert!(text.contains("/projects/root-demo"));
    }

    #[test]
    fn list_renders_source_groups_as_a_tree() {
        let mut app = App::for_test(Config::default(), Theme::load(false));
        app.entries = vec![
            entry(Source::Agent, "Claude"),
            entry(Source::Agent, "Codex"),
            entry(Source::Root, "Dotfiles"),
        ];
        app.filtered = vec![0, 1, 2];
        app.filtered_scores = vec![0; 3];

        let backend = TestBackend::new(40, 12);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal.draw(|f| draw_list(f, &app, f.area())).unwrap();
        let text = buffer_text(&terminal);

        assert!(text.contains(" ▾ agent "));
        assert!(text.contains(&format!(
            "  ├─ {} Claude",
            status_icon_at(&Source::Agent, "", 0)
        )));
        assert!(text.contains(&format!(
            "  └─ {} Codex",
            status_icon_at(&Source::Agent, "", 0)
        )));
        assert!(text.contains(" ▾ root "));
        assert!(text.contains("  └─ Dotfiles"));
    }

    #[test]
    fn vim_mode_uses_normal_keys_then_searches_with_slash() {
        let mut app = App::for_test(Config::default(), Theme::load(false));
        app.config.picker.vim_mode = true;
        app.config.picker.preview = false;
        let mut input = TuiInputSession::legacy(&app);
        handle_key(&mut app, &mut input, key(KeyCode::Char('j')));
        assert!(app.query.is_empty());

        handle_key(&mut app, &mut input, key(KeyCode::Char('a')));
        assert_eq!(app.source_filter, Some(Source::Agent));

        handle_key(&mut app, &mut input, key(KeyCode::Char('/')));
        assert_eq!(input.state.vim, VimState::Search);
        assert_eq!(app.source_filter, Some(Source::Agent));

        handle_key(&mut app, &mut input, key(KeyCode::Char('j')));
        assert_eq!(app.query, "j");

        handle_key(&mut app, &mut input, key(KeyCode::Esc));
        assert_eq!(input.state.vim, VimState::Normal);
    }

    #[test]
    fn vim_filter_search_starts_search_after_source_key() {
        let mut app = App::for_test(Config::default(), Theme::load(false));
        app.config.picker.vim_mode = true;
        app.config.picker.vim_filter_search = true;
        let mut input = TuiInputSession::legacy(&app);

        handle_key(&mut app, &mut input, key(KeyCode::Char('a')));
        assert_eq!(app.source_filter, Some(Source::Agent));
        assert_eq!(input.state.vim, VimState::Search);

        handle_key(&mut app, &mut input, key(KeyCode::Char('c')));
        assert_eq!(app.query, "c");
    }

    #[test]
    fn n10_host_prefix_blocks_dispatch_and_help_from_the_same_saved_menu_config() {
        let state = TestState::new();
        let (mut app, _runner) = loop_app_with_invocation(&state, fixture_invocation(&state));
        app.config.picker.focus_cycle_key = "CTRL+G".into();
        let diagnostics = app.config.avoid_default_prefix_conflict('G');

        let mut input = TuiInputSession {
            state: InputState::merged(&app.config.picker, &app.query),
            merged: None,
        };
        let before = input.state.focus;
        assert!(matches!(
            handle_key(
                &mut app,
                &mut input,
                KeyEvent::new(KeyCode::Char('g'), KeyModifiers::CONTROL),
            ),
            Action::Continue
        ));
        assert_eq!(input.state.focus, before);

        input.state.focus = crate::keymap::Focus::MachineFilter;
        handle_key(&mut app, &mut input, key(KeyCode::Enter));
        assert!(input.state.menu.is_some());
        handle_key(&mut app, &mut input, key(KeyCode::Char('?')));
        let descriptor = input.descriptor();
        assert_eq!(
            descriptor.key_context.focus,
            crate::keymap::Focus::MachineFilter
        );
        assert!(descriptor.key_context.menu);

        let mut terminal = Terminal::new(TestBackend::new(80, 30)).unwrap();
        terminal
            .draw(|frame| draw_with_status(frame, &app, &descriptor, None))
            .unwrap();
        let text = buffer_text(&terminal);
        assert!(
            !text.contains("cycle focus"),
            "host key leaked into Help:\n{text}"
        );
        for expected in ["choose filter", "back / close"] {
            assert!(text.contains(expected), "missing {expected}:\n{text}");
        }
        assert!(diagnostics.iter().any(|line| {
            line.contains("picker.focus_cycle_key: ctrl-g conflicts with Herdr keys.prefix")
                && line.contains("merged binding inactive")
        }));
    }

    #[test]
    fn question_mark_toggles_registry_help_overlay() {
        let mut app = App::for_test(Config::default(), Theme::load(false));
        let mut input = TuiInputSession::legacy(&app);
        handle_key(&mut app, &mut input, key(KeyCode::Char('?')));
        assert!(input.state.help_visible());

        let backend = TestBackend::new(80, 30);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|f| draw_with_status(f, &app, &input.descriptor(), None))
            .unwrap();
        let text = buffer_text(&terminal);
        assert!(text.contains(" Keybindings "));
        assert!(text.contains("toggle Details"));
        assert!(text.contains("agents"));
        for label in [
            "query cursor left",
            "query cursor right",
            "query cursor start",
            "query cursor end",
            "backspace query",
            "delete query",
        ] {
            assert!(text.contains(label), "missing {label}:\n{text}");
        }
        assert!(
            text.contains(" Filters"),
            "Filters clipped at 80x30:\n{text}"
        );
        assert!(!text.contains("?/?"));

        handle_key(&mut app, &mut input, key(KeyCode::Char('?')));
        assert!(!input.state.help_visible());
    }

    #[test]
    fn help_conditional_actions_are_complete_at_80x30() {
        assert_eq!(help_text_width("⌥↵"), 2);
        assert_eq!(help_text_width("↑/⌥K/k"), 6);
        assert_eq!(
            wrap_help_description("directory template", 0),
            ["directory template"]
        );
        let narrow = wrap_help_description("directory template", 1);
        assert!(narrow.len() <= "directorytemplate".chars().count());
        assert!(narrow.iter().all(|line| help_text_width(line) <= 1));

        fn rect_lines(terminal: &Terminal<TestBackend>, area: Rect) -> Vec<String> {
            let buffer = terminal.backend().buffer();
            (area.y..area.bottom())
                .map(|y| {
                    (area.x..area.right())
                        .map(|x| buffer[(x, y)].symbol())
                        .collect()
                })
                .collect()
        }

        fn rendered_description(lines: &[String], key: &str, expected: &str) -> String {
            let row = lines
                .iter()
                .position(|line| line.trim_start().starts_with(key))
                .unwrap_or_else(|| panic!("missing key {key:?} in column:\n{}", lines.join("\n")));
            let mut rendered = lines[row].split_once(key).unwrap().1.trim().to_string();
            let mut next = row + 1;
            while rendered.len() < expected.len() && next < lines.len() {
                let continuation = lines[next].trim();
                if continuation.is_empty() {
                    break;
                }
                rendered.push(' ');
                rendered.push_str(continuation);
                next += 1;
            }
            rendered
        }

        for template_key in [None, Some("option+enter")] {
            for vim in [None, Some(VimState::Normal), Some(VimState::Search)] {
                for (template, update) in
                    [(false, false), (true, false), (false, true), (true, true)]
                {
                    let mut config = Config::default();
                    config.picker.vim_mode = vim.is_some();
                    config.picker.directory_template =
                        template.then(|| "fixture-template.toml".into());
                    if let Some(template_key) = template_key {
                        config.picker.directory_template_key = template_key.into();
                    }
                    let mut app = App::for_test(config, Theme::load(false));
                    app.entries = vec![entry(Source::Project, "template fixture")];
                    app.apply_filter();
                    app.update_available = update.then(|| "fixture-update".into());
                    let mut input = TuiInputSession::legacy(&app);
                    if vim == Some(VimState::Search) {
                        assert!(matches!(
                            handle_key(&mut app, &mut input, key(KeyCode::Char('/'))),
                            Action::Continue
                        ));
                    }
                    assert_eq!(input.state.vim, vim.unwrap_or(VimState::Normal));
                    assert!(matches!(
                        handle_key(&mut app, &mut input, key(KeyCode::Char('?'))),
                        Action::Continue
                    ));
                    assert!(input.state.help_visible());
                    let descriptor = input.descriptor();
                    let bindings = effective_bindings(
                        &app.config.picker,
                        descriptor.key_context,
                        key_availability(&app),
                    );

                    let area = Rect::new(0, 0, 80, 30);
                    let mut terminal =
                        Terminal::new(TestBackend::new(area.width, area.height)).unwrap();
                    terminal
                        .draw(|frame| draw_with_status(frame, &app, &descriptor, None))
                        .unwrap();
                    let popup = area.centered(
                        Constraint::Percentage(94),
                        Constraint::Length(area.height.saturating_sub(2).max(1)),
                    );
                    let content = Block::default().borders(Borders::ALL).inner(popup);
                    let columns = Layout::default()
                        .direction(Direction::Horizontal)
                        .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
                        .split(content);
                    let left = rect_lines(&terminal, columns[0]);
                    let right = rect_lines(&terminal, columns[1]);
                    let all = format!("{}\n{}", left.join("\n"), right.join("\n"));

                    for heading in ["Navigation", "Editing"] {
                        assert!(
                            left.iter().any(|line| line.contains(heading)),
                            "missing {heading}:\n{all}"
                        );
                    }
                    for heading in ["Actions", "View", "Filters"] {
                        assert!(
                            right.iter().any(|line| line.contains(heading)),
                            "missing {heading}:\n{all}"
                        );
                    }
                    for binding in &bindings {
                        let column = if matches!(binding.group, "Navigation" | "Editing") {
                            &left
                        } else {
                            &right
                        };
                        assert_eq!(
                            rendered_description(column, &binding.key_label(), &binding.label),
                            binding.label,
                            "incomplete {:?} for {} with template={template}, update={update}, vim={vim:?}:\n{all}",
                            binding.key_label(),
                            binding.label,
                        );
                    }
                    for label in [
                        "query cursor left",
                        "query cursor right",
                        "query cursor start",
                        "query cursor end",
                        "backspace query",
                        "delete query",
                    ] {
                        assert!(
                            left.iter().any(|line| line.contains(label)),
                            "missing {label}:\n{all}"
                        );
                    }
                    if template {
                        assert_eq!(
                            rendered_description(
                                &right,
                                "⌥↵",
                                "open selected with directory template"
                            ),
                            "open selected with directory template"
                        );
                    } else {
                        assert!(
                            !right
                                .iter()
                                .any(|line| line.contains("open selected with directory")),
                            "unavailable template action rendered:\n{all}"
                        );
                    }
                    if update {
                        assert_eq!(
                            rendered_description(&right, "F5", "install available update"),
                            "install available update"
                        );
                    } else {
                        assert!(
                            !right
                                .iter()
                                .any(|line| line.contains("install available update")),
                            "unavailable update action rendered:\n{all}"
                        );
                    }
                    let last_filter = bindings
                        .iter()
                        .rev()
                        .find(|binding| binding.group == "Filters")
                        .unwrap();
                    assert_eq!(
                        rendered_description(&right, &last_filter.key_label(), &last_filter.label),
                        last_filter.label
                    );
                    assert!(
                        (0..area.height)
                            .any(|y| terminal.backend().buffer()[(popup.x, y)].symbol() == "└"),
                        "missing Help bottom border:\n{all}"
                    );
                }
            }
        }
    }

    #[test]
    fn registry_reports_active_toggle_state() {
        let app = App::for_test(Config::default(), Theme::load(false));
        let input = TuiInputSession::legacy(&app).descriptor();
        assert!(binding_active(&Command::ToggleDetails, &app, &input));
    }

    #[test]
    fn help_uses_live_shadowed_keys_and_the_saved_origin_context() {
        let mut config = Config::default();
        config
            .picker
            .filter_keys
            .insert("project".into(), "ctrl-o".into());
        config.picker.vim_mode = true;
        config.picker.directory_template = Some("default.toml".into());
        let mut app = App::for_test(config, Theme::load(false));
        app.update_available = Some("9.9.9".into());

        let mut query = TuiInputSession {
            state: InputState::merged(&app.config.picker, &app.query),
            merged: None,
        };
        handle_key(&mut app, &mut query, key(KeyCode::Char('?')));
        let mut terminal = Terminal::new(TestBackend::new(80, 30)).unwrap();
        terminal
            .draw(|frame| draw_with_status(frame, &app, &query.descriptor(), None))
            .unwrap();
        let text = buffer_text(&terminal);
        assert!(text.contains("⌃O"));
        assert!(text.contains("projects"));
        assert!(!text.contains("toggle Details"));
        assert!(text.contains("query cursor left"));

        handle_key(&mut app, &mut query, key(KeyCode::Char('?')));
        handle_key(&mut app, &mut query, key(KeyCode::F(6)));
        handle_key(&mut app, &mut query, key(KeyCode::Char('?')));
        terminal
            .draw(|frame| draw_with_status(frame, &app, &query.descriptor(), None))
            .unwrap();
        let text = buffer_text(&terminal);
        assert!(text.contains("collapse / parent"));
        assert!(!text.contains("query cursor left"));

        app.config.picker.focus_cycle_key = "left".into();
        app.config.picker.collapse_key = "f9".into();
        terminal
            .draw(|frame| draw_with_status(frame, &app, &query.descriptor(), None))
            .unwrap();
        let text = buffer_text(&terminal);
        assert!(text.contains("F9"));
        assert!(text.contains("collapse / parent"));
        assert!(!text.contains("cycle focus"));
        assert!(!text.contains('←'));
        app.config.picker.focus_cycle_key = "f6".into();
        app.config.picker.collapse_key = "left".into();

        handle_key(&mut app, &mut query, key(KeyCode::Char('?')));
        let rows = Rows::default();
        let picker_config = app.config.picker.clone();
        let reduce = |query: &mut TuiInputSession, app: &mut App, event| {
            input::reduce(
                &mut query.state,
                &mut app.query,
                None,
                event,
                InputContext {
                    config: &picker_config,
                    visible: VisibleRegions::merged(false),
                    rows: &rows,
                    actions: ActionAvailability::default(),
                    legacy_filter: None,
                    machine_options: &[],
                },
            );
        };
        reduce(&mut query, &mut app, key(KeyCode::F(6)));
        let mut footer_terminal = Terminal::new(TestBackend::new(140, 30)).unwrap();
        footer_terminal
            .draw(|frame| draw_with_status(frame, &app, &query.descriptor(), None))
            .unwrap();
        assert!(buffer_text(&footer_terminal).contains("open filter menu"));
        reduce(&mut query, &mut app, key(KeyCode::Enter));
        reduce(&mut query, &mut app, key(KeyCode::Char('?')));
        let descriptor = query.descriptor();
        assert_eq!(
            descriptor.key_context.focus,
            crate::keymap::Focus::MachineFilter
        );
        assert_eq!(descriptor.key_context.vim, VimState::Search);
        assert!(descriptor.key_context.menu);
        terminal
            .draw(|frame| draw_with_status(frame, &app, &descriptor, None))
            .unwrap();
        let text = buffer_text(&terminal);
        for expected in ["choose filter", "cycle focus", "back / close"] {
            assert!(text.contains(expected), "missing {expected}:\n{text}");
        }
        for suppressed in [
            "cycle provenance",
            "projects",
            "open selected",
            "mark / unmark",
            "install available update",
            "open selected with directory template",
            "⌥J",
        ] {
            assert!(
                !text.contains(suppressed),
                "menu Help leaked {suppressed}:\n{text}"
            );
        }
    }

    #[test]
    fn spec3_help_and_dispatch_follow_reserved_key_owners() {
        for (command, label) in [
            (Command::FocusCycle, "cycle focus"),
            (Command::Collapse, "collapse / parent"),
            (Command::Expand, "expand"),
            (Command::TechnicalDetails, "technical Details"),
        ] {
            let mut refresh_config = Config::default();
            set_merged_key(&mut refresh_config, &command, "f8");
            let mut refresh_app = App::for_test(refresh_config, Theme::load(false));
            let text = merged_help_text(&mut refresh_app, crate::keymap::Focus::Browse, false);
            assert!(!text.contains(label), "Refresh leaked {label}:\n{text}");
            assert!(!text.to_ascii_lowercase().contains("refresh"));
            assert!(!text.contains("F8"));

            let mut update_config = Config::default();
            set_merged_key(&mut update_config, &command, "f5");
            let mut update_app = App::for_test(update_config, Theme::load(false));
            let text = merged_help_text(&mut update_app, crate::keymap::Focus::Browse, false);
            assert!(!text.contains(label));
            assert!(!text.contains("install available update"));
            update_app.update_available = Some("fixture".into());
            let text = merged_help_text(&mut update_app, crate::keymap::Focus::Browse, false);
            assert!(!text.contains(label));
            assert!(text.contains("install available update"));
            let mut update_input = TuiInputSession {
                state: InputState::merged(&update_app.config.picker, &update_app.query),
                merged: None,
            };
            assert!(matches!(
                handle_key(&mut update_app, &mut update_input, key(KeyCode::F(5))),
                Action::Update
            ));

            let mut close_config = Config::default();
            set_merged_key(&mut close_config, &command, "ctrl-x");
            let mut close_app = App::for_test(close_config, Theme::load(false));
            assert!(!close_app.can_close_selected());
            let text = merged_help_text(&mut close_app, crate::keymap::Focus::Browse, false);
            assert!(!text.contains(label));
            assert!(!text.contains("close workspace"));

            let mut template_config = Config::default();
            set_merged_key(&mut template_config, &command, "alt-enter");
            let mut template_app = App::for_test(template_config, Theme::load(false));
            template_app.entries = vec![entry(Source::Project, "template-target")];
            template_app.apply_filter();
            let text = merged_help_text(&mut template_app, crate::keymap::Focus::Browse, false);
            assert!(!text.contains(label));
            assert!(!text.contains("open selected with directory template"));
            template_app.config.picker.directory_template = Some("default.toml".into());
            let text = merged_help_text(&mut template_app, crate::keymap::Focus::Browse, false);
            assert!(!text.contains(label));
            assert!(text.contains("open selected with directory template"));
            let expected = template_app.selected_entry().unwrap().key().clone();
            let mut template_input = TuiInputSession {
                state: InputState::merged(&template_app.config.picker, &template_app.query),
                merged: None,
            };
            assert!(matches!(
                handle_key(
                    &mut template_app,
                    &mut template_input,
                    KeyEvent::new(KeyCode::Enter, KeyModifiers::ALT),
                ),
                Action::Open(ActivationIntent {
                    identity,
                    slot: crate::actions::Slot::Template,
                }) if identity == expected
            ));
        }

        let mut menu_config = Config::default();
        menu_config.picker.focus_cycle_key = "f5".into();
        let mut menu_app = App::for_test(menu_config, Theme::load(false));
        menu_app.update_available = Some("fixture".into());
        let text = merged_help_text(&mut menu_app, crate::keymap::Focus::MachineFilter, true);
        assert!(!text.contains("cycle focus"));
        assert!(!text.contains("install available update"));
        for expected in ["choose filter", "back / close"] {
            assert!(text.contains(expected), "missing {expected}:\n{text}");
        }
    }

    #[test]
    fn vim_search_help_hides_normal_aliases_but_keeps_plain_filter_shortcuts() {
        let mut config = Config::default();
        config.picker.vim_mode = true;
        let mut app = App::for_test(config, Theme::load(false));
        let mut input = TuiInputSession {
            state: InputState::merged(&app.config.picker, &app.query),
            merged: None,
        };
        handle_key(&mut app, &mut input, key(KeyCode::Char('?')));
        let mut terminal = Terminal::new(TestBackend::new(80, 30)).unwrap();
        terminal
            .draw(|frame| draw_with_status(frame, &app, &input.descriptor(), None))
            .unwrap();
        let text = buffer_text(&terminal);
        assert!(text.contains("⌃P"));
        assert!(text.contains("⌥J"));
        assert!(!text.contains("/           search"));
        assert!(!text.contains("↓/⌥J/j"));
    }

    #[test]
    fn registry_maps_ctrl_b_to_mark_without_stealing_enter() {
        let app = App::for_test(Config::default(), Theme::load(false));
        let context = crate::keymap::InputContext {
            presentation: crate::keymap::Presentation::Legacy,
            focus: crate::keymap::Focus::Query,
            vim: VimState::Search,
            menu: false,
        };
        let available = key_availability(&app);
        assert_eq!(
            crate::keymap::resolve(
                &app.config.picker,
                context,
                available,
                KeyEvent::new(KeyCode::Char('b'), KeyModifiers::CONTROL),
            ),
            Some(Command::ToggleMark)
        );
        assert_eq!(
            crate::keymap::resolve(
                &app.config.picker,
                context,
                available,
                KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE),
            ),
            Some(Command::Open)
        );
    }

    #[test]
    fn compact_footer_lists_only_live_command_aliases() {
        let mut app = App::for_test(Config::default(), Theme::load(false));
        app.config.picker.vim_mode = true;
        let backend = TestBackend::new(110, 20);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal.draw(|f| draw(f, &app)).unwrap();
        let text = buffer_text(&terminal);

        assert!(text.contains("↑/⌥K/k up"));
        assert!(text.contains("↓/⌥J/j down"));
        assert!(text.contains("⌃A/a agent"));
        assert!(text.contains("⌃Z/z zoxide"));
        assert!(!text.contains("j/k"));
    }

    #[test]
    fn input_modes_transition_exclusively() {
        let mut app = App::for_test(Config::default(), Theme::load(false));
        app.config.picker.vim_mode = true;
        app.config.picker.vim_filter_search = true;
        let mut input = TuiInputSession::legacy(&app);
        assert_eq!(input.state.vim, VimState::Normal);

        handle_key(&mut app, &mut input, key(KeyCode::Char('a')));
        assert_eq!(input.state.vim, VimState::Search);

        handle_key(&mut app, &mut input, key(KeyCode::Char('?')));
        assert!(input.state.help_visible());

        handle_key(&mut app, &mut input, key(KeyCode::Esc));
        assert_eq!(input.state.vim, VimState::Search);
        assert!(!input.state.help_visible());
    }
    #[test]
    fn foreign_paths_render_as_reported_text_and_unknown_ownership_never_says_create() {
        let mut app = App::for_test(Config::default(), Theme::load(false));
        app.navigation = crate::navigation::NavigationContext::unmanaged();
        let mut row = entry(Source::Root, "foreign");
        let text = format!("{}/repo", crate::paths::home().display());
        row.path = OwnedPath::reported(app.navigation.scope(), &text);
        assert_eq!(display_path(&row), text);
        let mut local = entry(Source::Root, "process");
        local.path = Some(OwnedPath::process(PathBuf::from("/tmp")));
        let preview = preview_text(&app, &local);
        assert!(preview.contains("unavailable: process/server path ownership is unverified"));
        assert!(!preview.contains("enter: create"));
        let mut render_app = app;
        render_app.entries = vec![row];
        render_app.apply_filter();
        let mut terminal = Terminal::new(TestBackend::new(160, 10)).unwrap();
        terminal
            .draw(|frame| draw_list(frame, &render_app, frame.area()))
            .unwrap();
        assert!(buffer_text(&terminal).contains(&text));
    }
    #[test]
    fn displayed_description_and_enter_share_a_token_and_movement_cannot_retarget_it() {
        let mut config = Config::default();
        config.notifications.enabled = false;
        let mut app = App::for_test(config, Theme::load(false));
        let runner = std::sync::Arc::new(crate::herdr::RecordingRunner::default());
        app.navigation = crate::navigation::NavigationContext::with_server(
            crate::navigation::server::OrdinaryServer::new(
                crate::herdr::invocation(),
                Box::new(runner.clone()),
                crate::herdr::Limits::default(),
            ),
        );
        let first = crate::integrations::herdr_plus::quick_actions_entry();
        let mut second = Entry::new(
            crate::model::EntryIdentity::provider("test", "second", "second").unwrap(),
            first.data.clone(),
        );
        second.action = EntryAction::InvokePluginAction {
            action: "second".into(),
        };
        second.title = "Second action".into();
        app.entries = vec![first, second];
        app.prepare_actions();
        app.apply_filter();
        assert!(app
            .open_selected(false)
            .unwrap_err()
            .contains("has not been displayed"));
        let mut terminal = Terminal::new(TestBackend::new(240, 45)).unwrap();
        terminal.draw(|frame| draw(frame, &app)).unwrap();
        app.commit_displayed_actions();
        let description = app
            .resolved_action(app.selected_entry().unwrap(), crate::actions::Slot::Primary)
            .description();
        assert!(preview_text(&app, app.selected_entry().unwrap()).contains(&description));
        assert!(buffer_text(&terminal).contains(&description));
        let mut input = TuiInputSession::legacy(&app);
        assert!(matches!(
            handle_key(&mut app, &mut input, key(KeyCode::Down)),
            Action::Continue
        ));
        assert!(matches!(
            handle_key(&mut app, &mut input, key(KeyCode::Enter)),
            Action::Open(_)
        ));
        assert!(app
            .open_selected(false)
            .unwrap_err()
            .contains("has not been displayed for this identity and slot"));
        assert!(runner.calls.lock().unwrap().is_empty());
        terminal.draw(|frame| draw(frame, &app)).unwrap();
        app.commit_displayed_actions();
        runner.replies.lock().unwrap().push_back(Ok(vec![]));
        let expected = match &app.selected_entry().unwrap().action {
            EntryAction::InvokePluginAction { action } => action.clone(),
            _ => panic!("plugin action"),
        };
        app.open_selected(false).unwrap();
        assert_eq!(runner.calls.lock().unwrap()[0].args[3], expected.as_str());
    }

    #[test]
    fn pane_close_binding_is_unavailable_in_both_legacy_source_groups() {
        let mut app = App::for_test(Config::default(), Theme::load(false));
        let pane = crate::sources::test_inventory_entries(&app.navigation).remove(0);
        for source in [Source::Workspace, Source::Agent] {
            app.entries = vec![pane.clone()];
            app.entries[0].source = source;
            app.apply_filter();
            let mut input = TuiInputSession::legacy(&app);
            assert!(matches!(
                handle_key(
                    &mut app,
                    &mut input,
                    KeyEvent::new(KeyCode::Char('x'), KeyModifiers::CONTROL)
                ),
                Action::Continue
            ));
            assert!(app.retain_close_action().is_err());
            assert!(app.query.is_empty());
        }
    }
    fn picker_fixture() -> crate::picker::PickerInput {
        crate::navigation_test_support::FixtureClient::sample().picker_input()
    }
    fn picker_id(input: &crate::picker::PickerInput, machine: usize, id: &str) -> EntryIdentity {
        input
            .destinations
            .iter()
            .find(|d| {
                d.owner == input.headings[machine].owner
                    && d.entry.target().is_some_and(|t| t.identity().id() == id)
            })
            .unwrap()
            .entry
            .key()
            .clone()
    }
    fn picker_expand_all(input: &crate::picker::PickerInput) -> crate::picker::PickerState {
        let mut state = crate::picker::PickerState::new(input);
        state.expanded.extend(
            input
                .destinations
                .iter()
                .map(|d| crate::picker::RowId::Destination(d.entry.key().clone())),
        );
        state
    }
    fn picker_browse<'a>(
        input: &'a crate::picker::PickerInput,
        state: &crate::picker::PickerState,
    ) -> crate::picker::Projection<'a> {
        crate::picker::project(
            input,
            state,
            crate::picker::Request {
                query: "",
                non_live_type: false,
                eligible: &input.eligible_ids(),
            },
        )
    }

    #[test]
    fn n08_fixture_input_joins_all_destinations_and_matches_expected_ancestry() {
        use crate::{
            model::{
                DestinationKind, LiveIdentity, MachineIdentity, MachineKey, ObjectKind, ServerScope,
            },
            picker::*,
        };
        let input = picker_fixture();
        let expected: serde_json::Value =
            serde_json::from_str(crate::navigation_test_support::INVENTORY).unwrap();
        let mut remaining: std::collections::HashSet<_> = input
            .destinations
            .iter()
            .map(|d| d.entry.key().clone())
            .collect();
        assert_eq!(remaining.len(), 24);
        for row in expected["expected"]["destinations"].as_array().unwrap() {
            let identity = row["identity"].as_array().unwrap();
            let text = |index: usize| identity[index].as_str().unwrap();
            let scope = ServerScope::reported(text(0), text(1), text(2)).unwrap();
            let entry = if text(3) == "machine" {
                input
                    .destinations
                    .iter()
                    .find(|d| {
                        d.entry.key()
                            == &EntryIdentity::Machine(
                                MachineIdentity::new(MachineKey::new(text(0)).unwrap(), None)
                                    .unwrap(),
                            )
                    })
                    .unwrap()
            } else {
                let kind = match text(3) {
                    "workspace" => ObjectKind::Workspace,
                    "tab" => ObjectKind::Tab,
                    "pane" => ObjectKind::Pane,
                    _ => panic!("invalid expected kind"),
                };
                let live = LiveIdentity::new(&scope, kind, text(4)).unwrap();
                input
                    .destinations
                    .iter()
                    .find(|d| d.entry.target().is_some_and(|t| t.identity() == &live))
                    .unwrap()
            };
            assert!(remaining.remove(entry.entry.key()));
            let ancestors: Vec<_> = entry
                .entry
                .target()
                .into_iter()
                .flat_map(|t| [t.workspace(), t.tab()])
                .flatten()
                .map(|id| id.id())
                .collect();
            assert_eq!(
                ancestors,
                row["ancestors"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|v| v.as_str().unwrap())
                    .collect::<Vec<_>>()
            );
        }
        assert!(remaining.is_empty());
        for (kind, count) in [
            (DestinationKind::Machine, 2),
            (DestinationKind::Workspace, 6),
            (DestinationKind::Tab, 7),
            (DestinationKind::Pane, 9),
        ] {
            assert_eq!(
                input
                    .destinations
                    .iter()
                    .filter(|d| d.entry.destination_kind() == kind)
                    .count(),
                count
            );
        }
        assert_eq!(
            input
                .destinations
                .iter()
                .filter(|d| d.entry.agent.is_some())
                .count(),
            2
        );
        let state = picker_expand_all(&input);
        let browse = picker_browse(&input, &state);
        assert_eq!(browse.rows.len(), 21); // Three proven unnamed tabs are compressed.
        assert_eq!(
            browse
                .rows
                .iter()
                .map(|row| &row.id)
                .collect::<std::collections::HashSet<_>>()
                .len(),
            browse.rows.len()
        );
        assert_eq!(
            browse
                .rows
                .iter()
                .filter(|r| r
                    .destination
                    .is_some_and(|d| d.entry.destination_kind() == DestinationKind::Pane))
                .count(),
            9
        );
        let ids = input.eligible_ids();
        let flat = project(
            &input,
            &state,
            Request {
                query: "caller already matched",
                non_live_type: false,
                eligible: &ids,
            },
        );
        assert_eq!(flat.rows.len(), 24);
        assert!(flat
            .rows
            .iter()
            .all(|r| r.depth == 0 && r.action().is_some()));
    }

    #[test]
    fn n08_named_one_flat_and_filter_restore_exact_browse_position() {
        use crate::picker::*;
        let input = picker_fixture();
        let tab = picker_id(&input, 0, "w4:t1");
        let pane = picker_id(&input, 0, "w4:p1");
        let mut state = picker_expand_all(&input);
        state.browse.selection = Some(RowId::Destination(tab.clone()));
        state.browse.viewport = Some(ViewportAnchor {
            row: RowId::Destination(tab.clone()),
            intra_row_offset: 2,
        });
        let saved = state.browse.clone();
        let expansion = state.expanded.clone();
        let ordered = vec![pane.clone(), tab.clone(), pane];
        state.flat.selection = Some(RowId::Destination(ordered[0].clone()));
        let flat = project(
            &input,
            &state,
            Request {
                query: "1",
                non_live_type: false,
                eligible: &ordered,
            },
        );
        assert!(flat.flat);
        assert_eq!(flat.rows.len(), 2);
        assert_eq!(flat.rows[0].id, RowId::Destination(ordered[0].clone()));
        assert_eq!(flat.selection, state.flat.selection);
        let mut filtered = input.eligible_ids();
        filtered.retain(|id| {
            !input.destinations.iter().any(|d| {
                d.entry.key() == id
                    && d.entry
                        .target()
                        .is_some_and(|t| t.workspace().is_some_and(|w| w.id() == "w4"))
            })
        });
        let hidden = project(
            &input,
            &state,
            Request {
                query: "",
                non_live_type: false,
                eligible: &filtered,
            },
        );
        assert!(hidden.selection.is_none());
        assert!(hidden.viewport.is_none());
        assert_eq!(state.browse, saved);
        let restored = picker_browse(&input, &state);
        assert_eq!(restored.selection, saved.selection);
        assert_eq!(restored.viewport, saved.viewport);
        assert_eq!(state.expanded, expansion);
        assert_eq!(
            restored
                .rows
                .iter()
                .find(|r| r.id == RowId::Destination(tab.clone()))
                .unwrap()
                .label,
            "1"
        );
    }

    #[test]
    fn n08_compression_requires_complete_consistent_inventory_and_explicit_unnamed() {
        use crate::{navigation::inventory::TabNaming, picker::*};
        let mut input = picker_fixture();
        let tab = picker_id(&input, 0, "w2:t1");
        let pane = picker_id(&input, 0, "w2:p1");
        let target = input
            .destinations
            .iter()
            .find(|d| d.entry.key() == &tab)
            .unwrap()
            .entry
            .target()
            .unwrap()
            .clone();
        let state = picker_expand_all(&input);
        let browse = picker_browse(&input, &state);
        assert!(!browse
            .rows
            .iter()
            .any(|r| r.id == RowId::Destination(tab.clone())));
        let row = browse
            .rows
            .iter()
            .find(|r| r.id == RowId::Destination(pane.clone()))
            .unwrap();
        assert_eq!(row.depth, 2);
        assert!(row.ancestors.contains(&RowId::Destination(tab.clone())));
        for (naming, complete, expected) in [
            (TabNaming::Unknown, true, 1),
            (TabNaming::Named, true, 1),
            (TabNaming::Unnamed, false, 1),
            (TabNaming::Unnamed, true, 2),
        ] {
            input.naming.insert(target.identity().clone(), naming);
            let tabs = input.tabs.get_mut(target.workspace().unwrap()).unwrap();
            tabs.complete = complete;
            tabs.expected = expected;
            let browse = picker_browse(&input, &state);
            assert!(browse
                .rows
                .iter()
                .any(|r| r.id == RowId::Destination(tab.clone())));
            assert_eq!(
                browse
                    .rows
                    .iter()
                    .find(|r| r.id == RowId::Destination(pane.clone()))
                    .unwrap()
                    .depth,
                3
            );
        }
    }

    #[test]
    fn n08_current_is_separate_from_marks_selection_and_unknown_context() {
        use crate::picker::*;
        let mut input = picker_fixture();
        let current = input.current.clone().unwrap();
        input.pinned.extend(input.eligible_ids());
        input.previous = Some(picker_id(&input, 0, "w4:p1"));
        input.headings.reverse(); // Local first comes from evidence, not input or pin order.
        let mut state = PickerState::new(&input);
        assert_eq!(state.expanded.len(), 4); // Two heads plus current workspace and tab.
        assert_eq!(
            state.browse.selection,
            Some(RowId::Destination(current.clone()))
        );
        let initial = picker_browse(&input, &state);
        assert_eq!(initial.rows[0].label, "Local");
        assert_eq!(initial.rows.iter().filter(|r| r.current).count(), 1);
        assert!(initial.rows.iter().all(|r| r.pinned));
        let expanded = picker_expand_all(&input);
        assert_eq!(
            picker_browse(&input, &expanded)
                .rows
                .iter()
                .filter(|r| r.previous)
                .count(),
            1
        );
        state.browse.selection = Some(initial.rows[0].id.clone());
        assert_eq!(state.current, Some(current.clone()));
        let filtered: Vec<_> = input
            .eligible_ids()
            .into_iter()
            .filter(|id| id != &current)
            .collect();
        assert!(!project(
            &input,
            &state,
            Request {
                query: "",
                non_live_type: false,
                eligible: &filtered
            }
        )
        .rows
        .iter()
        .any(|r| r.current));
        input.current = None;
        let unknown = PickerState::new(&input);
        assert_eq!(unknown.expanded.len(), 2);
        assert!(unknown.browse.selection.is_none());
        assert!(unknown.browse.viewport.is_none());
        assert!(!picker_browse(&input, &unknown)
            .rows
            .iter()
            .any(|r| r.current));
    }

    #[test]
    fn n08_machine_expansion_survives_session_change_but_live_identity_does_not() {
        use crate::picker::*;
        let mut client = crate::navigation_test_support::FixtureClient::sample();
        let old = client.picker_input();
        let machine = RowId::Destination(old.headings[0].machine.clone().unwrap());
        let pane = picker_id(&old, 0, "w1:p1");
        assert_ne!(pane, picker_id(&old, 1, "w1:p1"));
        let mut state = picker_expand_all(&old);
        state.toggle(machine.clone());
        state.browse.selection = Some(RowId::Destination(pane.clone()));
        state.browse.viewport = Some(ViewportAnchor {
            row: RowId::Destination(pane.clone()),
            intra_row_offset: 1,
        });
        client.machines[0].owner[1] = "other-session".into();
        let other_session = client.picker_input();
        assert_eq!(
            Some(machine.clone()),
            other_session.headings[0]
                .machine
                .clone()
                .map(RowId::Destination)
        );
        assert_ne!(pane, picker_id(&other_session, 0, "w1:p1"));
        client.machines[0].owner[1] = "session-local".into();
        client.machines[0].owner[2] = "other-incarnation".into();
        let restarted = client.picker_input();
        assert_ne!(pane, picker_id(&restarted, 0, "w1:p1"));
        let browse = picker_browse(&restarted, &state);
        assert!(browse.selection.is_none());
        assert!(browse.viewport.is_none());
        assert!(!state.expanded.contains(&machine));
        assert_eq!(state.browse.selection, Some(RowId::Destination(pane)));
    }

    #[test]
    fn n08_empty_tabs_survive_join_and_unknown_empty_groups_are_not_invented() {
        use crate::picker::*;
        let mut client = crate::navigation_test_support::FixtureClient::sample();
        client.machines[0].workspaces[0].tabs[1].panes.clear();
        let mut input = client.picker_input();
        let state = picker_expand_all(&input);
        let tab = picker_id(&input, 0, "w1:t2");
        let browse = picker_browse(&input, &state);
        let row = browse
            .rows
            .iter()
            .find(|r| r.id == RowId::Destination(tab.clone()))
            .unwrap();
        assert!(matches!(
            row.action().unwrap().payload,
            crate::actions::Payload::Focus(_)
        ));
        assert!(!browse
            .rows
            .iter()
            .any(|r| matches!(r.id, RowId::TaskGroup(..))));
        input
            .groups
            .push((input.headings[0].owner.clone(), TaskKind::Sessions));
        let browse = picker_browse(&input, &state);
        let group = browse
            .rows
            .iter()
            .find(|r| matches!(r.id, RowId::TaskGroup(..)))
            .unwrap();
        assert!(group.disclosure);
        assert!(group.action().is_none());
        assert!(group.destination.is_none());
    }

    #[test]
    fn n09_app_search_after_refresh_has_no_io_history_token_commit_or_legacy_selection_changes() {
        use crate::picker::*;
        let directory = TestState::new();
        let (mut app, runner) = loop_app(&directory, None);
        app.config.picker.presentation = crate::config::Presentation::Merged;
        queue_lists(
            &runner,
            &crate::navigation_test_support::ordinary_lists(),
            4,
        );
        app.refresh();
        let calls = runner.calls.lock().unwrap().len();
        let bytes = directory.bytes();
        let view = ViewState::capture_default(&app);
        let filtered = app.filtered.clone();
        let input = app.picker_input();
        let mut state = picker_expand_all(&input);
        let browse = state.browse.clone();
        let expanded = state.expanded.clone();
        for engine in ["nucleo", "skim", "simple"] {
            app.config.picker.engine = engine.into();
            for query in ["@", "Dotfiles", "!claude @dot #idle", "  東京 α  ", ""] {
                app.query = query.into();
                for kind in [
                    TypeFilter::All,
                    TypeFilter::Panes,
                    TypeFilter::Agents,
                    TypeFilter::Sessions,
                ] {
                    state.set_type(kind);
                    for source in [None, Some(Source::Workspace), Some(Source::Agent)] {
                        state.set_provenance(source);
                        let projection = app.picker_projection(&input, &state);
                        assert_eq!(
                            projection.flat,
                            !query.is_empty() || kind == TypeFilter::Sessions
                        );
                        for row in &projection.rows {
                            if let Some(destination) = row.destination {
                                assert_eq!(row.action(), Some(&destination.action));
                            }
                        }
                        // Selection movement changes only caller-owned anchors, not the matcher or App.
                        state.flat.selection = projection.rows.first().map(|row| row.id.clone());
                        assert_eq!(runner.calls.lock().unwrap().len(), calls);
                        assert_eq!(app.query, query);
                    }
                }
            }
        }
        assert_eq!(state.browse, browse);
        assert_eq!(state.expanded, expanded);
        assert_eq!(directory.bytes(), bytes);
        assert_eq!(app.filtered, filtered);
        app.query = view.query.clone();
        assert_eq!(ViewState::capture_default(&app), view);
        assert!(app
            .open_selected(false)
            .unwrap_err()
            .contains("has not been displayed"));
        assert_eq!(runner.calls.lock().unwrap().len(), calls);
        assert_eq!(directory.bytes(), bytes);
        assert_eq!(
            app.config.picker.presentation,
            crate::config::Presentation::Merged
        );
    }

    #[test]
    fn n08_app_refresh_input_projection_has_no_io_or_legacy_state_changes() {
        use crate::picker::*;
        let directory = TestState::new();
        let (mut app, runner) = loop_app(&directory, None);
        app.config.picker.presentation = crate::config::Presentation::Merged;
        let lists = crate::navigation_test_support::ordinary_lists();
        queue_lists(&runner, &lists, 4);
        app.refresh();
        let calls = runner.calls.lock().unwrap().len();
        let bytes = directory.bytes();
        let view = ViewState::capture_default(&app);
        let legacy_ids: Vec<_> = app.entries.iter().map(|e| e.key().clone()).collect();
        assert!(app
            .entries
            .iter()
            .all(|e| e.destination_kind() != crate::model::DestinationKind::Tab));
        for _ in 0..3 {
            let input = app.picker_input();
            assert!(input.current.is_none()); // Workspace focused and picker owner are not client selection.
            assert!(input
                .headings
                .iter()
                .all(|h| !h.local && h.machine.is_none()));
            assert!(input
                .headings
                .iter()
                .any(|h| h.label == "Server scope: owner unknown"));
            for destination in &input.destinations {
                assert_eq!(
                    destination.action,
                    app.resolved_action(&destination.entry, crate::actions::Slot::Primary)
                );
            }
            assert_eq!(input.naming.len(), app.joined_inventory.tabs.len());
            assert!(input
                .naming
                .values()
                .all(|n| *n == crate::navigation::inventory::TabNaming::Unknown));
            let state = picker_expand_all(&input);
            let browse = picker_browse(&input, &state);
            let workspaces: Vec<_> = browse
                .rows
                .iter()
                .filter(|r| r.id.clone() == RowId::Destination(picker_id_server(&input, "w43")))
                .collect();
            assert_eq!(workspaces.len(), 1);
            assert!(workspaces[0].action().is_none()); // Hidden workspace is structural only.
            let ids = input.eligible_ids();
            let flat = project(
                &input,
                &state,
                Request {
                    query: "w43",
                    non_live_type: false,
                    eligible: &ids,
                },
            );
            assert!(flat.rows.iter().all(|r| r
                .destination
                .is_some_and(
                    |d| d.entry.destination_kind() != crate::model::DestinationKind::Workspace
                )));
            let mut hidden = input.clone();
            hidden
                .destinations
                .iter_mut()
                .for_each(|d| d.eligible = false);
            assert!(picker_browse(&hidden, &state).rows.is_empty());
        }
        assert_eq!(runner.calls.lock().unwrap().len(), calls);
        assert_eq!(directory.bytes(), bytes);
        assert_eq!(ViewState::capture_default(&app), view);
        assert_eq!(
            app.entries
                .iter()
                .map(|e| e.key().clone())
                .collect::<Vec<_>>(),
            legacy_ids
        );
        assert!(app.previous_workspace_id.is_none());
        let mut terminal = Terminal::new(TestBackend::new(100, 24)).unwrap();
        terminal
            .draw(|frame| {
                let input = InputState::legacy(&app.config.picker, &app.query).descriptor();
                draw_with_status(frame, &app, &input, None);
            })
            .unwrap();
        let text = buffer_text(&terminal);
        assert!(text.contains("Server scope: owner unknown"));
        assert!(!text.contains("Projects and folders"));
    }
    fn picker_id_server(input: &crate::picker::PickerInput, id: &str) -> EntryIdentity {
        input
            .destinations
            .iter()
            .find(|d| d.entry.target().is_some_and(|t| t.identity().id() == id))
            .unwrap()
            .entry
            .key()
            .clone()
    }

    #[test]
    fn n08_provider_groups_use_explicit_ownership_and_flat_non_live_request() {
        use crate::picker::*;
        let directory = TestState::new();
        let (mut app, runner) = loop_app(&directory, None);
        let mut remote = entry(Source::Server, "Configured remote");
        remote.action = EntryAction::OpenRemote {
            target: "example.invalid".into(),
        };
        let mut action = entry(Source::QuickAction, "A task");
        action.action = EntryAction::InvokePluginAction {
            action: "fixture-task".into(),
        };
        let folder = entry(Source::Project, "Folder");
        let folder_id = folder.key().clone();
        let remote_id = remote.key().clone();
        let action_id = action.key().clone();
        app.entries = vec![folder, remote, action];
        let input = app.picker_input();
        let server = Owner::Server(app.navigation.scope().clone());
        let process = Owner::Process(crate::navigation::ProcessHostRef);
        assert!(runner.calls.lock().unwrap().is_empty());
        assert_eq!(input.destinations.len(), 3);
        for (id, owner) in [
            (&folder_id, &process),
            (&remote_id, &process),
            (&action_id, &server),
        ] {
            let destination = input
                .destinations
                .iter()
                .find(|d| d.entry.key() == id)
                .unwrap();
            assert_eq!(&destination.owner, owner);
            assert_eq!(
                destination.action,
                app.resolved_action(&destination.entry, crate::actions::Slot::Primary)
            );
        }
        assert_eq!(input.headings.len(), 2);
        assert!(input
            .headings
            .iter()
            .all(|h| h.machine.is_none() && !h.local));
        let actions = RowId::TaskGroup(server.clone(), TaskKind::Actions);
        let projects = RowId::TaskGroup(process.clone(), TaskKind::ProjectsAndFolders);
        let sessions = RowId::TaskGroup(process.clone(), TaskKind::Sessions);
        let mut state = PickerState::new(&input);
        let browse = picker_browse(&input, &state);
        assert_eq!(
            browse.rows.iter().map(|r| r.id.clone()).collect::<Vec<_>>(),
            [
                RowId::Heading(server.clone()),
                actions.clone(),
                RowId::Heading(process.clone()),
                projects.clone(),
                sessions.clone(),
            ]
        );
        for (id, owner, label) in [
            (&actions, &server, "Actions"),
            (&projects, &process, "Projects and folders"),
            (&sessions, &process, "Sessions"),
        ] {
            let group = browse.rows.iter().find(|r| &r.id == id).unwrap();
            assert_eq!(group.label, label);
            assert_eq!(group.ancestors, vec![RowId::Heading(owner.clone())]);
            assert!(group.disclosure);
        }
        assert!(browse.rows.iter().all(|r| r.action().is_none()));
        state.toggle(sessions.clone());
        let open = picker_browse(&input, &state);
        let remote = open
            .rows
            .iter()
            .find(|r| r.id == RowId::Destination(remote_id.clone()))
            .unwrap();
        assert_eq!(remote.label, "Configured remote");
        assert_eq!(
            remote.ancestors,
            vec![RowId::Heading(process.clone()), sessions.clone()]
        );
        assert!(matches!(
            remote.action().unwrap().payload,
            crate::actions::Payload::Attach { .. }
        ));
        state.toggle(actions.clone());
        state.toggle(projects.clone());
        let expanded = picker_browse(&input, &state);
        for (id, owner, group) in [
            (&action_id, &server, &actions),
            (&folder_id, &process, &projects),
            (&remote_id, &process, &sessions),
        ] {
            let row = expanded
                .rows
                .iter()
                .find(|r| r.id == RowId::Destination(id.clone()))
                .unwrap();
            assert_eq!(
                row.ancestors,
                vec![RowId::Heading(owner.clone()), group.clone()]
            );
            let destination = input
                .destinations
                .iter()
                .find(|d| d.entry.key() == id)
                .unwrap();
            assert_eq!(row.action(), Some(&destination.action));
        }
        let mut ids = input.eligible_ids();
        ids.reverse();
        let flat = project(
            &input,
            &state,
            Request {
                query: "",
                non_live_type: true,
                eligible: &ids,
            },
        );
        assert!(flat.flat);
        assert_eq!(flat.rows.len(), 3);
        assert!(flat
            .rows
            .iter()
            .all(|r| r.action().is_some() && r.depth == 0));
        assert_eq!(flat.rows[0].label, "A task");
        assert_eq!(
            flat.rows.iter().map(|r| r.id.clone()).collect::<Vec<_>>(),
            [action_id, remote_id, folder_id].map(RowId::Destination)
        );
        assert!(flat.selection.is_none());
        // An explicit co-location relation is required to place process providers under a machine.
        let mut client = picker_fixture();
        let known_owner = client.headings[0].owner.clone();
        client
            .destinations
            .extend(input.destinations.into_iter().map(|mut d| {
                d.owner = known_owner.clone();
                d
            }));
        let state = PickerState::new(&client);
        let rows = picker_browse(&client, &state).rows;
        let group = rows.iter().find(|r| r.label == "Sessions").unwrap();
        assert_eq!(
            group.ancestors,
            vec![RowId::Destination(
                client.headings[0].machine.clone().unwrap()
            )]
        );
        let machine = &rows[0];
        assert!(matches!(
            machine.action().unwrap().payload,
            crate::actions::Payload::NativeNewWorkspace
        ));
        assert!(machine.action().unwrap().unavailable.is_some());
        assert!(runner.calls.lock().unwrap().is_empty());
    }
}
