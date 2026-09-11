//! Resolve from prepared process files and ordinary inventory. Rendering does no I/O.
use std::{collections::HashMap, path::PathBuf, sync::Arc};

use crate::{
    config::Config,
    model::{
        DestinationKind, Entry, EntryAction, EntryIdentity, LiveIdentity, LiveTarget, ObjectKind,
        Project, ServerScope, WorkspaceKind, WorkspaceRef, WorkspaceReuseKey,
    },
    navigation::{
        inventory::{Inventory, ReadState},
        NavigationContext, ProcessHostRef,
    },
    paths::OwnedPath,
    sources::JoinedInventory,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum Slot {
    Primary,
    Template,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Owner {
    Server(ServerScope),
    Process(ProcessHostRef),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Template {
    pub(crate) file: OwnedPath,
    source: String,
    pub(crate) project: Project,
}

// Both operations accept only process paths. Reported paths are rejected before
// a filesystem adapter is called, even when their text names a local file.
pub(crate) trait ProcessFiles {
    fn read(&self, path: &std::path::Path) -> Result<String, String>;
    fn canonical(&self, path: &std::path::Path) -> Option<PathBuf>;
}
pub(crate) struct Files;
impl ProcessFiles for Files {
    fn read(&self, path: &std::path::Path) -> Result<String, String> {
        std::fs::read_to_string(path).map_err(|_| "template disappeared or cannot be read".into())
    }
    fn canonical(&self, path: &std::path::Path) -> Option<PathBuf> {
        std::fs::canonicalize(path).ok()
    }
}
impl Template {
    pub(crate) fn load(file: &OwnedPath, files: &impl ProcessFiles) -> Result<Self, String> {
        let source = files.read(file.process_path()?)?;
        let project =
            toml::from_str(&source).map_err(|_| "invalid project template".to_string())?;
        Ok(Self {
            file: file.clone(),
            source,
            project,
        })
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct Prepared {
    pub(crate) cwd: Option<PathBuf>,
    pub(crate) paths: Vec<String>,
    pub(crate) template: Option<Arc<Template>>,
    pub(crate) error: Option<String>,
}
impl Prepared {
    pub(crate) fn update_project_entry(&self, entry: &mut Entry) {
        if !matches!(entry.action, EntryAction::OpenProject) || self.error.is_some() {
            return;
        }
        if let Some(template) = &self.template {
            entry.title = template.project.name.clone();
            entry.project = Some(template.project.clone());
            entry.path = self.cwd.clone().map(OwnedPath::process);
        }
    }

    pub(crate) fn load(
        entry: &Entry,
        slot: Slot,
        config: &Config,
        context: &NavigationContext,
        retained_file: Option<&OwnedPath>,
        files: &impl ProcessFiles,
    ) -> Self {
        let mut prepared = Self::default();
        if !matches!(
            entry.action,
            EntryAction::OpenProject | EntryAction::FocusOrCreateDir
        ) {
            return prepared;
        }
        let result = (|| {
            context.require_process_server_colocation()?;
            let file = if let Some(file) = retained_file {
                Some(file.clone())
            } else if slot == Slot::Template {
                let name = config
                    .picker
                    .directory_template
                    .as_deref()
                    .ok_or("no directory_template configured")?;
                Some(OwnedPath::process(
                    crate::paths::herdr_plus_projects_dir()
                        .join(crate::integrations::herdr_plus::template_filename(name)?),
                ))
            } else {
                entry.template_file.clone()
            };
            if let Some(file) = file {
                let file = OwnedPath::process(
                    context
                        .server()
                        .process_route()
                        .absolute_path(file.process_path()?),
                );
                prepared.template = Some(Arc::new(Template::load(&file, files)?));
            }
            let path = if matches!(entry.action, EntryAction::OpenProject) {
                let template = prepared
                    .template
                    .as_ref()
                    .ok_or("project template file is unavailable")?;
                if template.project.working_dir.trim().is_empty() {
                    return Err("project cwd is empty".into());
                }
                OwnedPath::process(crate::paths::expand_path(&template.project.working_dir))
            } else {
                entry.path.clone().ok_or("directory path is unknown")?
            };
            let path = context.process_path(&path)?;
            let path = context.server().process_route().absolute_path(path);
            prepared.paths.push(path.display().to_string());
            if let Some(canonical) = files.canonical(&path) {
                let text = canonical.display().to_string();
                if !prepared.paths.contains(&text) {
                    prepared.paths.push(text);
                }
            }
            prepared.cwd = Some(path);
            Ok(())
        })();
        prepared.error = result.err();
        prepared
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Payload {
    Focus(LiveTarget),
    NativeNewWorkspace,
    CreateProject {
        cwd: PathBuf,
        label: String,
        template: Arc<Template>,
    },
    OpenFolder {
        cwd: PathBuf,
        label: String,
    },
    ApplyTemplate {
        target: LiveTarget,
        cwd: PathBuf,
        template: Arc<Template>,
    },
    CreateTemplate {
        cwd: PathBuf,
        label: String,
        template: Arc<Template>,
    },
    Attach {
        target: String,
        remote: bool,
    },
    Run {
        command: String,
        timeout_ms: u64,
    },
    Plugin {
        action: String,
    },
    Unavailable,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ResolvedAction {
    pub(crate) identity: EntryIdentity,
    pub(crate) slot: Slot,
    pub(crate) owner: Owner,
    pub(crate) payload: Payload,
    pub(crate) unavailable: Option<String>,
    pub(crate) description: String,
    pub(crate) notify_success: bool,
    pub(crate) notify_error: bool,
    // These observations are semantic. Status, spinners, counts and read times
    // must not invalidate an otherwise unchanged action.
    title: String,
    path: Option<OwnedPath>,
    target_path: Option<OwnedPath>,
    target_label: Option<String>,
    ancestry: (Option<LiveIdentity>, Option<LiveIdentity>),
    prepared: Prepared,
    config: String,
}

impl ResolvedAction {
    pub(crate) fn description(&self) -> String {
        match &self.unavailable {
            Some(reason) => format!("{} unavailable: {reason}", self.description),
            None => self.description.clone(),
        }
    }
    pub(crate) fn revalidate(&self, fresh: &Self) -> Result<(), String> {
        if self != fresh {
            return Err("action changed; review the redrawn action and activate again".into());
        }
        if let Some(reason) = &self.unavailable {
            return Err(reason.clone());
        }
        Ok(())
    }
    pub(crate) fn template_file(&self) -> Option<&OwnedPath> {
        self.prepared.template.as_ref().map(|t| &t.file)
    }
}

pub(crate) struct Observations<'a> {
    pub(crate) context: &'a NavigationContext,
    pub(crate) inventory: Option<&'a Inventory>,
    pub(crate) joined: &'a JoinedInventory,
    pub(crate) reuse: &'a HashMap<WorkspaceReuseKey, Vec<WorkspaceRef>>,
}

pub(crate) fn resolve(
    entry: &Entry,
    slot: Slot,
    prepared: &Prepared,
    config: &Config,
    observations: Observations<'_>,
) -> ResolvedAction {
    let context = observations.context;
    let process = matches!(
        entry.action,
        EntryAction::OpenRemote { .. }
            | EntryAction::AttachSession { .. }
            | EntryAction::RunCommand { .. }
    );
    let config_stamp = match &entry.action {
        EntryAction::RunCommand { .. } => format!(
            "{:?}",
            config
                .integrations
                .iter()
                .map(|c| (
                    &c.id,
                    &c.open,
                    c.enabled,
                    c.open_timeout_ms,
                    c.notify_success,
                    c.notify_error
                ))
                .collect::<Vec<_>>()
        ),
        EntryAction::OpenRemote { .. } | EntryAction::AttachSession { .. } => format!(
            "{:?}",
            (
                config.sessions.local,
                config
                    .sessions
                    .entries
                    .iter()
                    .map(|c| (&c.name, &c.remote, &c.session))
                    .collect::<Vec<_>>()
            )
        ),
        EntryAction::OpenProject | EntryAction::FocusOrCreateDir => format!(
            "{}:{}:{:?}",
            config.picker.reuse_existing,
            config.picker.create_missing,
            config.picker.directory_template
        ),
        _ => String::new(),
    };
    let mut action = ResolvedAction {
        identity: entry.key().clone(),
        slot,
        owner: if process {
            Owner::Process(ProcessHostRef)
        } else {
            Owner::Server(context.scope().clone())
        },
        payload: Payload::Unavailable,
        unavailable: entry.action_conflict.clone(),
        description: "Action".into(),
        notify_success: true,
        notify_error: true,
        title: entry.title.clone(),
        path: entry.path.clone(),
        target_path: None,
        target_label: None,
        ancestry: (
            entry.target().and_then(LiveTarget::workspace).cloned(),
            entry.target().and_then(LiveTarget::tab).cloned(),
        ),
        prepared: prepared.clone(),
        config: format!(
            "{config_stamp}:{:?}:{:?}",
            (
                config.notifications.enabled,
                config.notifications.audio,
                &config.notifications.sound,
                &config.notifications.custom_sound
            ),
            context.scope()
        ),
    };
    let result = (|| {
        if entry.destination_kind() == DestinationKind::Machine {
            action.payload = Payload::NativeNewWorkspace;
            action.description = format!("New workspace on {}", entry.title);
            return Err("native new-workspace naming flow is unavailable".into());
        }
        if let Some(reason) = &action.unavailable {
            return Err(reason.clone());
        }
        if let Some(target) = entry.target() {
            action.description = format!(
                "Focus {} {}",
                match target.identity().kind() {
                    ObjectKind::Workspace => "workspace",
                    ObjectKind::Tab => "tab",
                    ObjectKind::Pane => "pane",
                },
                entry.title
            );
            if target.identity().scope() != context.scope() {
                return Err("selected target belongs to a different server scope".into());
            }
            let valid = match target.identity().kind() {
                ObjectKind::Workspace => {
                    matches!(&entry.action, EntryAction::FocusWorkspace { id } if id == target.identity().id())
                }
                ObjectKind::Pane => {
                    matches!(entry.action, EntryAction::FocusPane) && entry.pane.is_some()
                }
                ObjectKind::Tab => matches!(entry.action, EntryAction::FocusTab),
            };
            if !valid {
                return Err("action does not match qualified target".into());
            }
            if !live_available(target, &observations) {
                return Err("target inventory is unavailable or ancestry changed".into());
            }
            if target.identity().kind() == ObjectKind::Tab {
                action.target_label = observations
                    .inventory
                    .and_then(|inventory| {
                        inventory
                            .tabs
                            .rows(context.scope())
                            .iter()
                            .find(|row| row.tab_id == target.identity().id())
                    })
                    .map(|row| row.label.clone());
            }
            action.payload = Payload::Focus(target.clone());
            return Ok(());
        }
        match &entry.action {
            EntryAction::OpenProject | EntryAction::FocusOrCreateDir => {
                action.description = if matches!(entry.action, EntryAction::OpenProject) {
                    "Create project"
                } else if slot == Slot::Template {
                    "Apply template"
                } else {
                    "Open folder"
                }
                .into();
                context.require_process_server_colocation()?;
                if let Some(error) = &prepared.error {
                    return Err(error.clone());
                }
                let cwd = prepared
                    .cwd
                    .clone()
                    .ok_or("process path has not been prepared")?;
                let existing = matching(entry, slot, prepared, &observations)?;
                let template = prepared.template.clone();
                if let Some(ws) =
                    existing.filter(|_| slot == Slot::Template || config.picker.reuse_existing)
                {
                    action.target_path = ws.path.clone();
                    action.target_label = Some(ws.label.clone());
                    let target = LiveTarget::new(ws.identity.clone(), None, None)
                        .ok_or("invalid workspace")?;
                    if slot == Slot::Template {
                        let template = template.ok_or("template is unavailable")?;
                        action.description = format!(
                            "Apply {} to workspace {} [{}]",
                            template.project.name,
                            ws.label,
                            ws.identity.id()
                        );
                        action.payload = Payload::ApplyTemplate {
                            target,
                            cwd,
                            template,
                        };
                    } else {
                        action.description =
                            format!("Focus workspace {} [{}]", ws.label, ws.identity.id());
                        action.payload = Payload::Focus(target);
                    }
                } else {
                    if slot != Slot::Template && !config.picker.create_missing {
                        return Err("create_missing=false and no reusable workspace exists".into());
                    }
                    if slot == Slot::Template {
                        let template = template.ok_or("template is unavailable")?;
                        action.description = format!(
                            "Create template {} in {}",
                            template.project.name,
                            cwd.display()
                        );
                        action.payload = Payload::CreateTemplate {
                            cwd,
                            label: entry.title.clone(),
                            template,
                        };
                    } else if matches!(entry.action, EntryAction::OpenProject) {
                        let template = template.ok_or("template is unavailable")?;
                        let label = format!("project: {}", template.project.name);
                        action.description = format!(
                            "Create project {} in {}",
                            template.project.name,
                            cwd.display()
                        );
                        action.payload = Payload::CreateProject {
                            cwd,
                            label,
                            template,
                        };
                    } else {
                        action.description = format!("Open folder {}", cwd.display());
                        action.payload = Payload::OpenFolder {
                            cwd,
                            label: entry.title.clone(),
                        };
                    }
                }
            }
            EntryAction::OpenRemote { target } => {
                action.description = format!("Attach remote {target}");
                action.notify_success = false;
                action.payload = Payload::Attach {
                    target: target.clone(),
                    remote: true,
                };
            }
            EntryAction::AttachSession { name, remote } => {
                action.description = format!("Attach {}", remote.as_deref().unwrap_or(name));
                action.notify_success = false;
                action.payload = Payload::Attach {
                    target: remote.as_ref().unwrap_or(name).clone(),
                    remote: remote.is_some(),
                };
            }
            EntryAction::RunCommand {
                command,
                timeout_ms,
                notify_success,
                notify_error,
            } => {
                action.description = format!("Run on process host: {command}");
                action.notify_success = *notify_success;
                action.notify_error = *notify_error;
                action.payload = Payload::Run {
                    command: command.clone(),
                    timeout_ms: *timeout_ms,
                };
            }
            EntryAction::InvokePluginAction { action: plugin } => {
                action.description = format!("Run plugin {plugin} on server scope");
                action.payload = Payload::Plugin {
                    action: plugin.clone(),
                };
            }
            _ => return Err("qualified target is unavailable".into()),
        }
        Ok(())
    })();
    action.unavailable = result.err();
    if action.unavailable.is_none() {
        if let Payload::Focus(target) | Payload::ApplyTemplate { target, .. } = &action.payload {
            action.unavailable = context
                .server()
                .focus_unavailable(target.identity().kind())
                .map(str::to_string);
        }
    }
    action
}

fn live_available(target: &LiveTarget, observations: &Observations<'_>) -> bool {
    let Some(inventory) = observations.inventory else {
        return false;
    };
    let scope = observations.context.scope();
    match target.identity().kind() {
        ObjectKind::Workspace => {
            inventory
                .workspaces
                .rows(scope)
                .iter()
                .filter(|row| row.workspace_id == target.identity().id())
                .count()
                == 1
        }
        ObjectKind::Tab => {
            let rows: Vec<_> = inventory
                .tabs
                .rows(scope)
                .iter()
                .filter(|row| row.tab_id == target.identity().id())
                .collect();
            rows.len() == 1
                && target.workspace().is_some_and(|ws| {
                    rows[0].workspace_id == ws.id()
                        && inventory
                            .workspaces
                            .rows(scope)
                            .iter()
                            .filter(|row| row.workspace_id == ws.id())
                            .count()
                            == 1
                })
        }
        ObjectKind::Pane => observations.joined.panes.iter().any(|row| {
            row.target().is_some_and(|current| {
                current.identity() == target.identity()
                    && current.workspace() == target.workspace()
                    && current.tab() == target.tab()
            })
        }),
    }
}

fn matching<'a>(
    entry: &Entry,
    slot: Slot,
    prepared: &Prepared,
    observations: &'a Observations<'_>,
) -> Result<Option<&'a WorkspaceRef>, String> {
    let scope = observations.context.scope();
    let Some(inventory) = observations.inventory else {
        return Err("workspace inventory is unavailable".into());
    };
    if &inventory.workspaces.scope != scope
        || &inventory.panes.scope != scope
        || !matches!(inventory.workspaces.state, ReadState::Ready(_))
        || !matches!(inventory.panes.state, ReadState::Ready(_))
    {
        return Err("workspace/path inventory is unavailable".into());
    }
    let candidates: Vec<_> = prepared
        .paths
        .iter()
        .filter_map(|path| {
            observations
                .reuse
                .get(&WorkspaceReuseKey::new(scope.clone(), path.clone()))
        })
        .flatten()
        .filter(|ws| ws.identity.scope() == scope)
        .collect();
    let preferred = if matches!(entry.action, EntryAction::OpenProject) {
        WorkspaceKind::Project
    } else {
        WorkspaceKind::Dir
    };
    let matched = candidates
        .iter()
        .copied()
        .find(|ws| ws.kind == preferred)
        .or_else(|| {
            candidates.iter().copied().find(|ws| {
                slot == Slot::Template
                    || (preferred == WorkspaceKind::Dir && ws.kind == WorkspaceKind::Unknown)
            })
        });
    if matched.is_none()
        && (observations
            .joined
            .workspaces
            .iter()
            .any(|ws| ws.path.is_none())
            || observations
                .joined
                .unavailable
                .iter()
                .any(|reason| !reason.starts_with("agent on "))
            || observations.joined.workspaces.len() != inventory.workspaces.rows(scope).len())
    {
        return Err("workspace cwd or inventory is unknown; cannot infer no match".into());
    }
    Ok(matched)
}

/// Completion is not evidence of an invoking-client workspace transition.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Completion {
    OrdinaryFocus(Box<crate::model::LiveTarget>),
    ProviderCompleted,
}

pub(crate) fn dispatch(
    action: &ResolvedAction,
    context: &mut NavigationContext,
) -> Result<Completion, String> {
    if let Some(reason) = &action.unavailable {
        return Err(reason.clone());
    }
    if matches!(&action.owner, Owner::Server(scope) if scope != context.scope()) {
        return Err("action owner changed".into());
    }
    let result = (|| match &action.payload {
        Payload::Focus(target) => context.focus(target).map_err(|e| e.to_string()),
        Payload::CreateProject {
            cwd,
            label,
            template,
        }
        | Payload::CreateTemplate {
            cwd,
            label,
            template,
        } => {
            context.require_process_server_colocation()?;
            let server = context.server();
            let json = server.json([
                "workspace",
                "create",
                "--cwd",
                &cwd.display().to_string(),
                "--label",
                label,
                "--focus",
            ])?;
            crate::integrations::herdr_plus::bootstrap_project_tabs(
                server,
                &template.project,
                &json,
                cwd,
            )
            .map_err(|error| {
                format!("workspace may be partially created; no retry or rollback: {error}")
            })
        }
        Payload::OpenFolder { cwd, label } => {
            context.require_process_server_colocation()?;
            context.server().run([
                "workspace",
                "create",
                "--cwd",
                &cwd.display().to_string(),
                "--label",
                label,
                "--focus",
            ])
        }
        Payload::ApplyTemplate {
            target,
            cwd,
            template,
        } => {
            context.require_process_server_colocation()?;
            context.focus(target).map_err(|e| e.to_string())?;
            crate::integrations::herdr_plus::append_project_tabs(context.server(), &template.project, target.identity().id(), cwd)
                .map_err(|error| format!("workspace focused; template may be partially applied; no retry or rollback: {error}"))
        }
        Payload::Attach { target, remote } => {
            if *remote {
                crate::integrations::sessions::open_remote(context.server().process_route(), target)
            } else {
                crate::integrations::sessions::attach_session(
                    context.server().process_route(),
                    target,
                )
            }
        }
        Payload::Run {
            command,
            timeout_ms,
        } => crate::integrations::command::run_command(
            context.server().process_route(),
            command,
            *timeout_ms,
        ),
        Payload::Plugin { action } => context.server().run(["plugin", "action", "invoke", action]),
        Payload::NativeNewWorkspace | Payload::Unavailable => Err("action is unavailable".into()),
    })();
    if let Some(error) = context.server().take_mutation_failure() {
        context.observe_failure(&error);
    }
    result.map(|()| match &action.payload {
        Payload::Focus(target) => Completion::OrdinaryFocus(Box::new(target.clone())),
        _ => Completion::ProviderCompleted,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{cell::RefCell, path::Path};

    #[test]
    fn process_file_preparation_is_the_only_filesystem_seam_and_rejects_reported_paths() {
        #[derive(Default)]
        struct Memory {
            reads: RefCell<Vec<PathBuf>>,
            canonical: RefCell<Vec<PathBuf>>,
        }
        impl ProcessFiles for Memory {
            fn read(&self, path: &Path) -> Result<String, String> {
                self.reads.borrow_mut().push(path.to_path_buf());
                Ok("name='demo'\nworking_dir='/literal'".into())
            }
            fn canonical(&self, path: &Path) -> Option<PathBuf> {
                self.canonical.borrow_mut().push(path.to_path_buf());
                Some("/canonical".into())
            }
        }
        let context = NavigationContext::colocated();
        let files = Memory::default();
        let reported = OwnedPath::reported(context.scope(), "/process-looking-file").unwrap();
        assert!(Template::load(&reported, &files).is_err());
        assert!(files.reads.borrow().is_empty());
        let mut row = crate::integrations::herdr_plus::quick_actions_entry();
        row.action = EntryAction::OpenProject;
        row.template_file = Some(OwnedPath::process("/exact/template.toml".into()));
        let config = Config::default();
        let prepared = Prepared::load(&row, Slot::Primary, &config, &context, None, &files);
        assert!(prepared.error.is_none());
        assert_eq!(prepared.paths, ["/literal", "/canonical"]);
        assert_eq!(
            files.reads.borrow().as_slice(),
            [PathBuf::from("/exact/template.toml")]
        );
        assert_eq!(
            files.canonical.borrow().as_slice(),
            [PathBuf::from("/literal")]
        );
        let joined = JoinedInventory::default();
        let reuse = HashMap::new();
        for _ in 0..10 {
            let _ = resolve(
                &row,
                Slot::Primary,
                &prepared,
                &config,
                Observations {
                    context: &context,
                    inventory: None,
                    joined: &joined,
                    reuse: &reuse,
                },
            );
        }
        assert_eq!(files.reads.borrow().len(), 1);
        assert_eq!(files.canonical.borrow().len(), 1);
        row.action = EntryAction::FocusOrCreateDir;
        row.template_file = None;
        row.path = Some(reported);
        assert!(
            Prepared::load(&row, Slot::Primary, &config, &context, None, &files)
                .error
                .is_some()
        );
        assert_eq!(files.canonical.borrow().len(), 1);
    }

    #[test]
    fn descriptions_keep_attach_run_and_ordinary_plugins_distinct() {
        let context = NavigationContext::colocated();
        let config = Config::default();
        let joined = JoinedInventory::default();
        let reuse = HashMap::new();
        for (entry_action, expected) in [
            (
                EntryAction::OpenRemote {
                    target: "saved-remote".into(),
                },
                "Attach remote saved-remote",
            ),
            (
                EntryAction::AttachSession {
                    name: "session".into(),
                    remote: None,
                },
                "Attach session",
            ),
            (
                EntryAction::RunCommand {
                    command: "echo 'quoted'".into(),
                    timeout_ms: 0,
                    notify_success: false,
                    notify_error: false,
                },
                "Run on process host: echo 'quoted'",
            ),
            (
                EntryAction::InvokePluginAction {
                    action: "quick".into(),
                },
                "Run plugin quick on server scope",
            ),
        ] {
            for source in [crate::model::Source::Session, crate::model::Source::Server] {
                let mut row = crate::integrations::herdr_plus::quick_actions_entry();
                row.source = source;
                row.action = entry_action.clone();
                let action = resolve(
                    &row,
                    Slot::Primary,
                    &Prepared::default(),
                    &config,
                    Observations {
                        context: &context,
                        inventory: None,
                        joined: &joined,
                        reuse: &reuse,
                    },
                );
                assert_eq!(action.description(), expected);
                match &entry_action {
                    EntryAction::RunCommand { .. } => {
                        assert!(matches!(action.payload, Payload::Run { timeout_ms: 0, .. }));
                        assert!(matches!(action.owner, Owner::Process(_)));
                    }
                    EntryAction::OpenRemote { .. } | EntryAction::AttachSession { .. } => {
                        assert!(matches!(action.payload, Payload::Attach { .. }));
                        assert!(matches!(action.owner, Owner::Process(_)));
                    }
                    _ => {
                        assert!(matches!(action.payload, Payload::Plugin { .. }));
                        assert!(matches!(action.owner, Owner::Server(_)));
                    }
                }
            }
        }
    }
    #[cfg(unix)]
    #[test]
    fn tab_actions_validate_unique_ancestry_and_dispatch_exact_tab_focus() {
        use crate::navigation::{
            inventory::{AgentStatus, InventoryRead, TabRecord, WorkspaceRecord},
            server::tests::{snapshot, Reply, Socket},
        };
        let socket = Socket::new(vec![
            Reply::Result(snapshot()),
            Reply::Result(serde_json::json!({"type":"ok"})),
        ]);
        let mut context =
            NavigationContext::with_server(socket.server(crate::herdr::Limits::default()));
        let scope = context.scope();
        let workspace = LiveIdentity::new(scope, ObjectKind::Workspace, "w1").unwrap();
        let target = LiveTarget::new(
            LiveIdentity::new(scope, ObjectKind::Tab, "exact-tab").unwrap(),
            Some(workspace),
            None,
        )
        .unwrap();
        let mut data = crate::integrations::herdr_plus::quick_actions_entry().data;
        data.action = EntryAction::FocusTab;
        data.title = "Named tab".into();
        let row = Entry::new(EntryIdentity::live(target), data);
        let mut inventory = Inventory {
            workspaces: InventoryRead::new(
                scope,
                Some(Ok(vec![WorkspaceRecord {
                    workspace_id: "w1".into(),
                    label: "workspace".into(),
                    focused: false,
                    tab_count: 1,
                    pane_count: 0,
                    agent_status: AgentStatus::Idle,
                }])),
            ),
            tabs: InventoryRead::new(
                scope,
                Some(Ok(vec![TabRecord {
                    tab_id: "exact-tab".into(),
                    workspace_id: "w1".into(),
                    label: "Named tab".into(),
                }])),
            ),
            panes: InventoryRead::new(scope, Some(Ok(vec![]))),
            agents: InventoryRead::new(scope, None),
        };
        let joined = JoinedInventory::default();
        let reuse = HashMap::new();
        let config = Config::default();
        let resolve_tab = |inventory: &Inventory| {
            resolve(
                &row,
                Slot::Primary,
                &Prepared::default(),
                &config,
                Observations {
                    context: &context,
                    inventory: Some(inventory),
                    joined: &joined,
                    reuse: &reuse,
                },
            )
        };
        let action = resolve_tab(&inventory);
        assert_eq!(action.description(), "Focus tab Named tab");
        if let ReadState::Ready(tabs) = &mut inventory.tabs.state {
            tabs[0].label = "Renamed".into();
        }
        assert!(action.revalidate(&resolve_tab(&inventory)).is_err());
        if let ReadState::Ready(tabs) = &mut inventory.tabs.state {
            tabs[0].label = "Named tab".into();
            tabs.push(tabs[0].clone());
        }
        assert!(resolve_tab(&inventory).unavailable.is_some());
        let completion = dispatch(&action, &mut context).unwrap();
        let Payload::Focus(target) = &action.payload else {
            panic!("expected exact focus")
        };
        assert_eq!(
            completion,
            Completion::OrdinaryFocus(Box::new(target.clone()))
        );
        let requests = socket.requests.lock().unwrap();
        assert_eq!(requests[1]["method"], "tab.focus");
        assert_eq!(
            requests[1]["params"],
            serde_json::json!({"tab_id":"exact-tab"})
        );
    }
}
