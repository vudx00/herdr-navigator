pub(crate) mod inventory;
pub(crate) mod server;

use std::sync::atomic::{AtomicU64, Ordering};

use crate::model::{LiveIdentity, LiveTarget, ObjectKind, ServerScope, WorkspaceReuseKey};
use crate::paths::OwnedPath;

/// A process-local reference, never a durable host ID or an authorization token.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) struct ProcessHostRef;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ConnectionState {
    Unknown,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Freshness {
    Unknown,
    Stale,
}

fn next_generation() -> u64 {
    static NEXT: AtomicU64 = AtomicU64::new(1);
    NEXT.fetch_add(1, Ordering::Relaxed)
}

/// The frozen route has an ephemeral scope. Transport uncertainty invalidates it.
/// A silent server restart at the same socket cannot be detected by protocol 22.
#[derive(Debug)]
pub(crate) struct NavigationContext {
    scope: ServerScope,
    server: server::OrdinaryServer,
    process_colocated: bool,
    picker_owner: Option<LiveTarget>,
    current_client_selection: Option<LiveTarget>,
    connection: ConnectionState,
    freshness: Freshness,
}

impl NavigationContext {
    pub(crate) fn new() -> Self {
        let scope = ServerScope::ephemeral(next_generation());
        // Invocation metadata is an owner hint, never client identity or authorization.
        let picker_owner = std::env::var("HERDR_PLUGIN_CONTEXT_JSON")
            .ok()
            .and_then(|text| serde_json::from_str::<serde_json::Value>(&text).ok())
            .and_then(|value| {
                LiveIdentity::new(
                    &scope,
                    ObjectKind::Workspace,
                    value.get("workspace_id")?.as_str()?,
                )
            })
            .and_then(|identity| LiveTarget::new(identity, None, None));
        let server = server::OrdinaryServer::inherited();
        Self {
            scope,
            picker_owner,
            process_colocated: server.managed_plugin(),
            server,
            current_client_selection: None,
            connection: ConnectionState::Unknown,
            freshness: Freshness::Unknown,
        }
    }

    pub(crate) fn picker_workspace(&self) -> Option<&LiveIdentity> {
        let owner = self.picker_owner.as_ref()?;
        owner.workspace().or(Some(owner.identity()))
    }

    // Selection observations alone never grant a client route.
    #[allow(dead_code)]
    pub(crate) fn set_current_client_selection(
        &mut self,
        target: LiveTarget,
    ) -> Result<(), String> {
        self.require_current_scope(&target)?;
        self.current_client_selection = Some(target);
        Ok(())
    }

    #[allow(dead_code)]
    pub(crate) fn set_picker_owner(&mut self, target: LiveTarget) -> Result<(), String> {
        self.require_current_scope(&target)?;
        self.picker_owner = Some(target);
        Ok(())
    }

    fn require_current_scope(&self, target: &LiveTarget) -> Result<(), String> {
        if target.identity().scope() != &self.scope {
            return Err("context target belongs to a different server scope".into());
        }
        Ok(())
    }

    #[allow(dead_code)]
    pub(crate) fn current_client_selection(&self) -> Option<&LiveTarget> {
        self.current_client_selection.as_ref()
    }

    #[allow(dead_code)]
    pub(crate) fn connection(&self) -> ConnectionState {
        self.connection
    }

    #[allow(dead_code)]
    pub(crate) fn freshness(&self) -> Freshness {
        self.freshness
    }

    /// Clear observations when continuity cannot be established.
    #[allow(dead_code)]
    pub(crate) fn invalidate_scope(&mut self) {
        self.server.invalidate();
        self.scope.rotate_incarnation(next_generation());
        self.picker_owner = None;
        self.current_client_selection = None;
        self.process_colocated = false;
        self.connection = ConnectionState::Unknown;
        self.freshness = Freshness::Stale;
    }

    pub(crate) fn server(&self) -> &server::OrdinaryServer {
        &self.server
    }

    pub(crate) fn observe_failure(&mut self, error: &crate::herdr::TransportError) {
        if error.uncertain_continuity() {
            self.invalidate_scope();
        }
    }

    pub(crate) fn focus(
        &mut self,
        target: &LiveTarget,
    ) -> Result<(), crate::herdr::TransportError> {
        let result = self.server.focus(&self.scope, target);
        if let Err(error) = &result {
            self.observe_failure(error);
        }
        result
    }

    #[cfg(test)]
    pub(crate) fn with_server(server: server::OrdinaryServer) -> Self {
        Self {
            scope: ServerScope::ephemeral(next_generation()),
            process_colocated: server.managed_plugin(),
            server,
            picker_owner: None,
            current_client_selection: None,
            connection: ConnectionState::Unknown,
            freshness: Freshness::Unknown,
        }
    }

    #[cfg(test)]
    pub(crate) fn unmanaged() -> Self {
        let directory = std::env::temp_dir();
        Self::with_server(server::OrdinaryServer::new(
            crate::herdr::Invocation::for_test(
                directory.join("navigator-unmanaged-herdr"),
                directory,
                Vec::new(),
            ),
            Box::new(crate::herdr::RecordingRunner::default()),
            crate::herdr::Limits::default(),
        ))
    }

    pub(crate) fn scope(&self) -> &ServerScope {
        &self.scope
    }

    pub(crate) fn require_process_server_colocation(&self) -> Result<(), String> {
        if self.process_colocated {
            return Ok(());
        }
        Err("process/server path ownership is unverified; cannot reuse or create".into())
    }

    pub(crate) fn process_path<'a>(
        &self,
        path: &'a OwnedPath,
    ) -> Result<&'a std::path::Path, String> {
        let path = path.process_path()?;
        self.require_process_server_colocation()?;
        Ok(path)
    }

    pub(crate) fn display_reuse_key(&self, path: &OwnedPath) -> Result<WorkspaceReuseKey, String> {
        match path {
            OwnedPath::Reported { scope, text, .. } if scope == &self.scope => {
                Ok(WorkspaceReuseKey::new(scope.clone(), text.clone()))
            }
            OwnedPath::Process { path, .. } if self.process_colocated => Ok(
                WorkspaceReuseKey::new(self.scope.clone(), path.display().to_string()),
            ),
            _ => Err("process/server path ownership is unverified".into()),
        }
    }

    pub(crate) fn reuse_key(&self, path: &OwnedPath) -> Result<WorkspaceReuseKey, String> {
        match path {
            OwnedPath::Reported { scope, text, .. } if scope == &self.scope => {
                Ok(WorkspaceReuseKey::new(scope.clone(), text.clone()))
            }
            OwnedPath::Process { .. } if self.process_colocated => {
                let path = path.process_path()?;
                let text =
                    crate::paths::canonical_str(path).unwrap_or_else(|| path.display().to_string());
                Ok(WorkspaceReuseKey::new(self.scope.clone(), text))
            }
            _ => Err("process/server path ownership is unverified; cannot reuse or create".into()),
        }
    }

    #[cfg(test)]
    pub(crate) fn reported(scope: ServerScope) -> Self {
        Self {
            scope,
            ..Self::unmanaged()
        }
    }

    #[cfg(test)]
    pub(crate) fn colocated() -> Self {
        Self {
            process_colocated: true,
            ..Self::unmanaged()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn workspace(scope: &ServerScope, id: &str) -> LiveTarget {
        LiveTarget::new(
            LiveIdentity::new(scope, ObjectKind::Workspace, id).unwrap(),
            None,
            None,
        )
        .unwrap()
    }

    #[test]
    fn environment_owner_hint_never_sets_client_selection_or_path_authority() {
        let _lock = crate::herdr::bin_path_lock();
        struct Restore(Vec<(&'static str, Option<std::ffi::OsString>)>);
        impl Drop for Restore {
            fn drop(&mut self) {
                for (key, value) in &self.0 {
                    match value {
                        Some(value) => std::env::set_var(key, value),
                        None => std::env::remove_var(key),
                    }
                }
            }
        }
        let keys = [
            "HERDR_SOCKET_PATH",
            "HERDR_ENV",
            "HERDR_PLUGIN_ID",
            "HERDR_PLUGIN_ROOT",
            "HERDR_PLUGIN_CONTEXT_JSON",
        ];
        let _restore = Restore(
            keys.iter()
                .map(|key| (*key, std::env::var_os(key)))
                .collect(),
        );
        for key in keys {
            std::env::remove_var(key);
        }
        for (hint, expected_owner) in [
            (None, None),
            (Some("not-json"), None),
            (Some(r#"{"workspace_id":" "}"#), None),
            (
                Some(r#"{"workspace_id":"w1","client_id":"untrusted","pane_id":"p1"}"#),
                Some("w1"),
            ),
        ] {
            match hint {
                Some(text) => std::env::set_var("HERDR_PLUGIN_CONTEXT_JSON", text),
                None => std::env::remove_var("HERDR_PLUGIN_CONTEXT_JSON"),
            }
            let context = NavigationContext::new();
            assert_eq!(context.connection(), ConnectionState::Unknown);
            assert_eq!(context.freshness(), Freshness::Unknown);
            assert!(context.current_client_selection().is_none());
            assert!(context.require_process_server_colocation().is_err());
            assert_eq!(
                context.picker_workspace().map(LiveIdentity::id),
                expected_owner
            );
            if let Some(owner) = context.picker_workspace() {
                assert_eq!(owner.scope(), context.scope());
            }
        }
    }

    #[test]
    fn context_targets_require_current_scope_and_valid_ancestry() {
        let mut context = NavigationContext::unmanaged();
        let other = NavigationContext::unmanaged();
        let owner = workspace(context.scope(), "w1");
        let foreign = workspace(other.scope(), "w1");
        context.set_picker_owner(owner.clone()).unwrap();
        context.set_current_client_selection(owner.clone()).unwrap();
        assert!(context.set_picker_owner(foreign.clone()).is_err());
        assert!(context
            .set_current_client_selection(foreign.clone())
            .is_err());
        assert_eq!(context.picker_workspace(), Some(owner.identity()));
        assert_eq!(context.current_client_selection(), Some(&owner));
        let tab = LiveIdentity::new(context.scope(), ObjectKind::Tab, "t1").unwrap();
        let pane = LiveIdentity::new(context.scope(), ObjectKind::Pane, "p1").unwrap();
        assert!(LiveTarget::new(
            pane.clone(),
            Some(foreign.identity().clone()),
            Some(tab.clone())
        )
        .is_none());
        assert!(LiveTarget::new(pane.clone(), Some(owner.identity().clone()), None).is_none());
        let pane = LiveTarget::new(pane, Some(owner.identity().clone()), Some(tab)).unwrap();
        context.set_picker_owner(pane.clone()).unwrap();
        context.set_current_client_selection(pane.clone()).unwrap();
        assert_eq!(context.picker_workspace(), Some(owner.identity()));
        assert_eq!(context.current_client_selection(), Some(&pane));
        assert!(context.require_process_server_colocation().is_err());
    }

    #[test]
    fn invalidation_rotates_only_incarnation_and_clears_scope_local_evidence() {
        let mut context = NavigationContext::colocated();
        let old_scope = context.scope().clone();
        let old_target = workspace(&old_scope, "w1");
        let old_path = OwnedPath::reported(&old_scope, "/same/path").unwrap();
        let old_reuse = context.reuse_key(&old_path).unwrap();
        context.set_picker_owner(old_target.clone()).unwrap();
        context
            .set_current_client_selection(old_target.clone())
            .unwrap();
        context.invalidate_scope();
        assert_eq!(context.scope().session(), old_scope.session());
        assert_ne!(context.scope(), &old_scope);
        assert_ne!(workspace(context.scope(), "w1"), old_target);
        assert!(context.picker_workspace().is_none());
        assert!(context.current_client_selection().is_none());
        assert_eq!(context.connection(), ConnectionState::Unknown);
        assert_eq!(context.freshness(), Freshness::Stale);
        assert!(context.require_process_server_colocation().is_err());
        assert!(context.reuse_key(&old_path).is_err());
        assert!(context
            .reuse_key(&OwnedPath::process("/same/path".into()))
            .is_err());
        let new_path = OwnedPath::reported(context.scope(), "/same/path").unwrap();
        assert_ne!(context.reuse_key(&new_path).unwrap(), old_reuse);
        assert!(context
            .set_current_client_selection(old_target.clone())
            .is_err());
        assert!(context.set_picker_owner(old_target).is_err());
        let second = context.scope().clone();
        context.invalidate_scope();
        assert_eq!(context.scope().session(), second.session());
        assert_ne!(context.scope(), &second);
    }
}
