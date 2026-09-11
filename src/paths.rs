use std::{
    env, fs, io,
    path::{Path, PathBuf},
    sync::OnceLock,
};

pub(crate) fn plugin_config_dir() -> PathBuf {
    env::var("HERDR_PLUGIN_CONFIG_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| home().join(".config/herdr/plugins/config/herdr-navigator"))
}

pub(crate) fn migrate_legacy_plugin_config() {
    let current = plugin_config_dir();
    let legacy = home().join(".config/herdr/plugins/config/herdr-picker-plus");
    if current != legacy {
        let _ = copy_missing_files(&legacy, &current);
    }
}

fn copy_missing_files(from: &Path, to: &Path) -> io::Result<()> {
    if !from.is_dir() {
        return Ok(());
    }
    fs::create_dir_all(to)?;
    for entry in fs::read_dir(from)? {
        let entry = entry?;
        let destination = to.join(entry.file_name());
        if entry.file_type()?.is_file() && !destination.exists() {
            fs::copy(entry.path(), destination)?;
        }
    }
    Ok(())
}
pub(crate) fn herdr_plus_projects_dir() -> PathBuf {
    home().join(".config/herdr/plugins/config/cloudmanic.herdr-plus/projects")
}
pub(crate) fn herdr_plus_quick_actions_dir() -> PathBuf {
    home().join(".config/herdr/plugins/config/cloudmanic.herdr-plus/quick-actions")
}
/// Cached: `HOME` is fixed for the process, and the row renderer asks for it
/// once per visible path per frame.
pub(crate) fn home() -> PathBuf {
    static HOME: OnceLock<PathBuf> = OnceLock::new();
    HOME.get_or_init(|| {
        env::var("HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|_| PathBuf::from("/"))
    })
    .clone()
}
pub(crate) fn expand_path(s: &str) -> PathBuf {
    if let Some(rest) = s.strip_prefix("~/") {
        home().join(rest)
    } else if s == "~" {
        home()
    } else {
        PathBuf::from(s.replace("$HOME", &home().display().to_string()))
    }
}
pub(crate) fn basename(path: &Path) -> String {
    path.file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("workspace")
        .to_string()
}
pub(crate) fn canonical_str(path: &Path) -> Option<String> {
    fs::canonicalize(path).ok().map(|p| p.display().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn legacy_config_files_copy_without_overwriting_new_values() {
        let suffix = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let old = env::temp_dir().join(format!("herdr-navigator-old-{suffix}"));
        let new = env::temp_dir().join(format!("herdr-navigator-new-{suffix}"));
        fs::create_dir_all(&old).unwrap();
        fs::create_dir_all(&new).unwrap();
        fs::write(old.join("config.toml"), "old").unwrap();
        fs::write(old.join("jump-back-workspace"), "w1").unwrap();
        fs::write(new.join("config.toml"), "new").unwrap();

        copy_missing_files(&old, &new).unwrap();

        assert_eq!(fs::read_to_string(new.join("config.toml")).unwrap(), "new");
        assert_eq!(
            fs::read_to_string(new.join("jump-back-workspace")).unwrap(),
            "w1"
        );
        let _ = fs::remove_dir_all(old);
        let _ = fs::remove_dir_all(new);
    }
}

/// Reported paths remain text even when they happen to name a process-host file.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum OwnedPath {
    Process {
        owner: crate::navigation::ProcessHostRef,
        path: PathBuf,
    },
    Reported {
        scope: crate::model::ServerScope,
        text: String,
        home: Option<String>,
    },
}
impl OwnedPath {
    pub(crate) fn process(path: PathBuf) -> Self {
        Self::Process {
            owner: crate::navigation::ProcessHostRef,
            path,
        }
    }
    pub(crate) fn reported(scope: &crate::model::ServerScope, text: &str) -> Option<Self> {
        (!text.is_empty()).then(|| Self::Reported {
            scope: scope.clone(),
            text: text.into(),
            home: None,
        })
    }
    pub(crate) fn process_path(&self) -> Result<&Path, String> {
        match self {
            Self::Process { path, .. } => Ok(path),
            Self::Reported { .. } => {
                Err("reported server path is not a process filesystem path".into())
            }
        }
    }
    pub(crate) fn text(&self) -> String {
        match self {
            Self::Process { path, .. } => path.display().to_string(),
            Self::Reported { text, .. } => text.clone(),
        }
    }
    pub(crate) fn display(&self) -> String {
        let text = self.text();
        let owner_home = match self {
            Self::Process { .. } => Some(home().display().to_string()),
            Self::Reported { home, .. } => home.clone(),
        };
        if let Some(home) = owner_home.filter(|home| !home.is_empty() && home != "/") {
            if text == home {
                return "~".into();
            }
            if let Some(rest) = text.strip_prefix(&format!("{home}/")) {
                return format!("~/{rest}");
            }
        }
        text
    }
}

#[cfg(test)]
mod ownership_tests {
    use super::*;
    use crate::{model::ServerScope, navigation::NavigationContext};

    #[test]
    fn foreign_paths_cannot_cross_the_process_filesystem_interface() {
        let scope = ServerScope::ephemeral(1);
        for text in [
            "~",
            "~/repo",
            "$HOME/repo",
            "/tmp",
            &home().display().to_string(),
        ] {
            let path = OwnedPath::reported(&scope, text).unwrap();
            assert!(path.process_path().is_err());
            assert_eq!(path.text(), text);
            assert_eq!(path.display(), text);
        }
        assert!(OwnedPath::reported(&scope, "").is_none());
    }

    #[test]
    fn only_an_explicit_owner_home_shortens_a_reported_path() {
        let path = OwnedPath::Reported {
            scope: ServerScope::ephemeral(1),
            text: "/owner/home/repo".into(),
            home: Some("/owner/home".into()),
        };
        assert_eq!(path.display(), "~/repo");
        assert!(path.process_path().is_err());
    }

    #[test]
    fn process_paths_keep_filesystem_access_but_do_not_imply_server_ownership() {
        let path = OwnedPath::process(std::env::temp_dir());
        assert!(path.process_path().unwrap().is_dir());
        assert!(NavigationContext::unmanaged().reuse_key(&path).is_err());
        assert!(NavigationContext::colocated().reuse_key(&path).is_ok());
    }
}
