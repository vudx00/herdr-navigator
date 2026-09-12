use std::{collections::HashMap, env, fs, path::PathBuf};

use serde::Deserialize;

use crate::{
    model::Source,
    paths::{migrate_legacy_plugin_config, plugin_config_dir},
};

const DEFAULT_CONFIG: &str = include_str!("../examples/default-config.toml");

#[derive(Clone, Deserialize)]
pub(crate) struct Config {
    #[serde(default)]
    pub(crate) picker: PickerConfig,
    #[serde(default)]
    pub(crate) jump_back: JumpBackConfig,
    #[serde(default)]
    pub(crate) notifications: NotificationsConfig,
    #[serde(default)]
    pub(crate) sources: SourcesConfig,
    #[serde(default)]
    pub(crate) theme: ThemeConfig,
    #[serde(default)]
    pub(crate) roots: Vec<RootConfig>,
    #[serde(default)]
    pub(crate) sessions: SessionsConfig,
    #[serde(default)]
    pub(crate) integrations: Vec<IntegrationConfig>,
    #[serde(default)]
    pub(crate) agent_aliases: Vec<AgentAliasConfig>,
}

#[derive(Clone, Deserialize)]
pub(crate) struct PickerConfig {
    #[serde(default)]
    pub(crate) presentation: Presentation,
    #[serde(default = "default_focus_cycle_key")]
    pub(crate) focus_cycle_key: String,
    #[serde(default = "default_refresh_key")]
    pub(crate) refresh_key: String,
    #[serde(default = "default_collapse_key")]
    pub(crate) collapse_key: String,
    #[serde(default = "default_expand_key")]
    pub(crate) expand_key: String,
    #[serde(default = "default_technical_details_key")]
    pub(crate) technical_details_key: String,
    #[serde(default = "yes")]
    pub(crate) reuse_existing: bool,
    #[serde(default = "yes")]
    pub(crate) create_missing: bool,
    #[serde(default = "default_engine")]
    pub(crate) engine: String,
    #[serde(default = "default_source_order")]
    pub(crate) source_order: Vec<String>,
    #[serde(default = "default_source_priority_boost")]
    pub(crate) source_priority_boost: i64,
    #[serde(default = "default_agent_sort")]
    pub(crate) agent_sort: String,
    #[serde(default = "yes")]
    pub(crate) preview: bool,
    #[serde(default = "yes")]
    pub(crate) detailed_rows: bool,
    #[serde(default)]
    pub(crate) check_updates: bool,
    #[serde(default = "yes")]
    pub(crate) confirm_close_workspace: bool,
    #[serde(default = "default_root_cache_seconds")]
    pub(crate) root_cache_seconds: u64,
    #[serde(default)]
    pub(crate) directory_template: Option<String>,
    #[serde(default = "default_directory_template_key")]
    pub(crate) directory_template_key: String,
    #[serde(default)]
    pub(crate) vim_mode: bool,
    #[serde(default)]
    pub(crate) vim_filter_search: bool,
    #[serde(default)]
    pub(crate) filter_keys: HashMap<String, String>,
    #[serde(skip)]
    pub(crate) known_host_ctrl_prefix: Option<char>,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub(crate) enum Presentation {
    #[default]
    Legacy,
    Merged,
}

#[derive(Clone, Deserialize)]
pub(crate) struct JumpBackConfig {
    #[serde(default = "yes")]
    pub(crate) enabled: bool,
    #[serde(default = "yes")]
    pub(crate) pin_previous: bool,
}

#[derive(Clone, Deserialize)]
pub(crate) struct NotificationsConfig {
    #[serde(default = "yes")]
    pub(crate) enabled: bool,
    #[serde(default)]
    pub(crate) audio: bool,
    #[serde(default = "default_notification_sound")]
    pub(crate) sound: String,
    #[serde(default)]
    pub(crate) custom_sound: Option<String>,
}

#[derive(Clone, Deserialize)]
pub(crate) struct SourcesConfig {
    #[serde(default)]
    pub(crate) panes: bool,
    #[serde(default = "yes")]
    pub(crate) open_workspaces: bool,
    #[serde(default = "yes")]
    pub(crate) herdr_plus_projects: bool,
    #[serde(default = "yes")]
    pub(crate) zoxide: bool,
    #[serde(default = "yes")]
    pub(crate) roots: bool,
    #[serde(default = "yes")]
    pub(crate) agents: bool,
    #[serde(default = "yes")]
    pub(crate) servers: bool,
    #[serde(default = "yes")]
    pub(crate) sessions: bool,
    #[serde(default = "yes")]
    pub(crate) herdr_plus_quick_actions: bool,
}

#[derive(Clone, Deserialize)]
pub(crate) struct SessionsConfig {
    #[serde(default = "yes")]
    pub(crate) local: bool,
    #[serde(default)]
    pub(crate) entries: Vec<SessionEntryConfig>,
}

#[derive(Clone, Deserialize)]
pub(crate) struct SessionEntryConfig {
    pub(crate) name: String,
    pub(crate) remote: Option<String>,
    pub(crate) session: Option<String>,
    #[serde(default)]
    pub(crate) tags: Vec<String>,
}

#[derive(Clone, Deserialize)]
pub(crate) struct IntegrationConfig {
    pub(crate) id: String,
    pub(crate) label: String,
    #[serde(default = "yes")]
    pub(crate) enabled: bool,
    pub(crate) collect: String,
    pub(crate) open: String,
    #[serde(default = "default_collect_timeout_ms")]
    pub(crate) collect_timeout_ms: u64,
    #[serde(default)]
    pub(crate) open_timeout_ms: u64,
    #[serde(default = "default_max_output_bytes")]
    pub(crate) max_output_bytes: usize,
    #[serde(default = "yes")]
    pub(crate) notify_success: bool,
    #[serde(default = "yes")]
    pub(crate) notify_error: bool,
}

#[derive(Clone, Deserialize)]
pub(crate) struct ThemeConfig {
    #[serde(default = "yes")]
    pub(crate) inherit_herdr: bool,
}

#[derive(Clone, Deserialize)]
pub(crate) struct AgentAliasConfig {
    pub(crate) alias: String,
    pub(crate) agent: Option<String>,
    pub(crate) workspace: Option<String>,
    pub(crate) path: Option<String>,
}

impl AgentAliasConfig {
    pub(crate) fn matches(&self, agent: &str, workspace: &str, path: &str) -> bool {
        opt_matches(self.agent.as_deref(), agent)
            && opt_matches(self.workspace.as_deref(), workspace)
            && opt_matches(self.path.as_deref(), path)
    }
}

fn opt_matches(needle: Option<&str>, haystack: &str) -> bool {
    needle
        .map(|value| haystack.to_lowercase().contains(&value.to_lowercase()))
        .unwrap_or(true)
}
#[derive(Clone, Deserialize)]
pub(crate) struct RootConfig {
    pub(crate) path: String,
    #[serde(default = "default_depth")]
    pub(crate) max_depth: usize,
    #[serde(default = "default_root_excludes")]
    pub(crate) exclude: Vec<String>,
    #[serde(default)]
    pub(crate) follow_symlinks: bool,
}
fn yes() -> bool {
    true
}
fn default_depth() -> usize {
    3
}
fn default_root_excludes() -> Vec<String> {
    ["node_modules", "target", "vendor", ".venv", "dist", "build"]
        .into_iter()
        .map(String::from)
        .collect()
}
fn default_root_cache_seconds() -> u64 {
    60
}
fn default_collect_timeout_ms() -> u64 {
    5_000
}
fn default_max_output_bytes() -> usize {
    1_048_576
}
fn default_engine() -> String {
    "nucleo".into()
}
fn default_source_order() -> Vec<String> {
    [
        "workspace",
        "agent",
        "project",
        "session",
        "zoxide",
        "root",
        "server",
        "quick",
        "plugin",
    ]
    .into_iter()
    .map(String::from)
    .collect()
}
fn default_source_priority_boost() -> i64 {
    5
}
fn default_agent_sort() -> String {
    "herdr".into()
}
fn default_focus_cycle_key() -> String {
    "f6".into()
}
fn default_refresh_key() -> String {
    "f8".into()
}
fn default_collapse_key() -> String {
    "left".into()
}
fn default_expand_key() -> String {
    "right".into()
}
fn default_technical_details_key() -> String {
    "f7".into()
}
fn default_directory_template_key() -> String {
    "alt-enter".into()
}
fn default_notification_sound() -> String {
    "default".into()
}
fn default_filter_key(source: Source) -> Option<char> {
    match source {
        Source::Agent => Some('a'),
        Source::Server => Some('s'),
        Source::QuickAction => Some('q'),
        Source::Workspace => Some('w'),
        Source::Project => Some('p'),
        Source::Zoxide => Some('z'),
        Source::Root => Some('r'),
        Source::Session => Some('l'),
        Source::Integration => None,
    }
}

fn default_filter_keys() -> Vec<(Source, char)> {
    Source::all()
        .into_iter()
        .filter_map(|source| default_filter_key(source).map(|key| (source, key)))
        .collect()
}

pub(crate) fn parse_ctrl_key(value: &str) -> Option<char> {
    let key = value
        .trim()
        .to_ascii_lowercase()
        .replace("ctrl+", "")
        .replace("ctrl-", "")
        .replace(['^', '⌃'], "");
    let mut chars = key.chars();
    let ch = chars.next()?;
    (chars.next().is_none() && ch.is_ascii_alphanumeric()).then_some(ch)
}

impl Default for PickerConfig {
    fn default() -> Self {
        Self {
            presentation: Presentation::Legacy,
            focus_cycle_key: default_focus_cycle_key(),
            refresh_key: default_refresh_key(),
            collapse_key: default_collapse_key(),
            expand_key: default_expand_key(),
            technical_details_key: default_technical_details_key(),
            reuse_existing: true,
            create_missing: true,
            engine: default_engine(),
            source_order: default_source_order(),
            source_priority_boost: default_source_priority_boost(),
            agent_sort: default_agent_sort(),
            preview: true,
            detailed_rows: true,
            check_updates: false,
            confirm_close_workspace: true,
            root_cache_seconds: default_root_cache_seconds(),
            directory_template: None,
            directory_template_key: default_directory_template_key(),
            vim_mode: false,
            vim_filter_search: false,
            filter_keys: HashMap::new(),
            known_host_ctrl_prefix: None,
        }
    }
}

impl PickerConfig {
    pub(crate) fn filter_key(&self, source: Source) -> Option<char> {
        let key = self
            .custom_filter_keys()
            .into_iter()
            .find_map(|(custom_source, key)| (custom_source == source).then_some(key))
            .or_else(|| default_filter_key(source))?;
        (self.filter_source_for_key(key) == Some(source)).then_some(key)
    }

    pub(crate) fn filter_source_for_key(&self, key: char) -> Option<Source> {
        let key = key.to_ascii_lowercase();
        let custom = self.custom_filter_keys();
        custom
            .iter()
            .find(|(_, custom_key)| *custom_key == key)
            .map(|(source, _)| *source)
            .or_else(|| {
                default_filter_keys()
                    .into_iter()
                    .find_map(|(source, default_key)| {
                        (default_key == key && !custom.iter().any(|(s, _)| s == &source))
                            .then_some(source)
                    })
            })
    }

    fn sorted_filter_keys(&self) -> Vec<(&str, &str)> {
        let mut keys: Vec<_> = self
            .filter_keys
            .iter()
            .map(|(name, key)| (name.as_str(), key.as_str()))
            .collect();
        keys.sort_unstable_by_key(|(name, _)| *name);
        keys
    }

    fn custom_filter_keys(&self) -> Vec<(Source, char)> {
        let mut custom = Vec::new();
        for (name, value) in self.sorted_filter_keys() {
            let (Some(source), Some(key)) = (Source::from_config(name), parse_ctrl_key(value))
            else {
                continue;
            };
            // First valid sorted alias owns the source. Duplicate keys keep
            // their source claimed, but only the first sorted source dispatches.
            if !custom.iter().any(|(existing, _)| *existing == source) {
                custom.push((source, key));
            }
        }
        custom
    }

    pub(crate) fn filter_setting_path(&self, source: Source) -> String {
        self.sorted_filter_keys()
            .into_iter()
            .find(|(name, value)| {
                Source::from_config(name) == Some(source) && parse_ctrl_key(value).is_some()
            })
            .map(|(name, _)| format!("picker.filter_keys.{name}"))
            .unwrap_or_else(|| format!("picker.filter_keys.{} [default]", source.label()))
    }

    fn filter_diagnostics(&self) -> Vec<String> {
        let mut diagnostics = Vec::new();
        let mut accepted = Vec::new();
        for (name, value) in self.sorted_filter_keys() {
            let path = format!("picker.filter_keys.{name}");
            let Some(source) = Source::from_config(name) else {
                let key = parse_ctrl_key(value)
                    .map(|ch| format!("ctrl-{ch}"))
                    .unwrap_or_else(|| format!("{value:?}"));
                diagnostics.push(format!(
                    "{path}: {key} has unknown source; binding inactive"
                ));
                continue;
            };
            let Some(key) = parse_ctrl_key(value) else {
                diagnostics.push(format!(
                    "{path}: invalid Ctrl key {value:?}; entry ignored, binding inactive"
                ));
                continue;
            };
            if let Some((winner, _, winner_key)) = accepted.iter().find(|(_, s, _)| *s == source) {
                diagnostics.push(format!("{path}: ctrl-{key} duplicates source alias picker.filter_keys.{winner} at ctrl-{winner_key}; first valid sorted alias wins, binding inactive"));
                continue;
            }
            if let Some((winner, _, _)) = accepted.iter().find(|(_, _, k)| *k == key) {
                diagnostics.push(format!("{path}: ctrl-{key} conflicts with picker.filter_keys.{winner}; first sorted custom key wins, binding inactive, source default suppressed"));
            }
            accepted.push((name, source, key));
        }
        for (source, key) in default_filter_keys() {
            if accepted.iter().any(|(_, s, _)| *s == source) {
                continue;
            }
            if let Some((winner, _, _)) = accepted.iter().find(|(_, _, k)| *k == key) {
                diagnostics.push(format!("picker.filter_keys.{} [default]: ctrl-{key} conflicts with picker.filter_keys.{winner}; custom wins, default inactive", source.label()));
            }
        }
        diagnostics
    }

    /// Resolved once per filter pass, not per candidate: `source_order` holds
    /// config aliases, so each lookup is a string scan.
    pub(crate) fn source_ranks(&self) -> [usize; Source::COUNT] {
        let mut ranks = [Source::COUNT; Source::COUNT];
        for (rank, source) in self
            .source_order
            .iter()
            .filter_map(|name| Source::from_config(name))
            .enumerate()
        {
            let slot = &mut ranks[source.index()];
            *slot = (*slot).min(rank);
        }
        ranks
    }

    /// Merged search compares this separately after relevance. No i64 multiplication.
    pub(crate) fn merged_preference(&self, rank: usize) -> i128 {
        Source::COUNT.saturating_sub(rank) as i128 * i128::from(self.source_priority_boost)
    }

    pub(crate) fn bonus_for_rank(&self, rank: usize) -> i64 {
        (Source::COUNT as i64 - rank as i64).max(0) * self.source_priority_boost
    }
}
impl Default for JumpBackConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            pin_previous: true,
        }
    }
}
impl Default for SourcesConfig {
    fn default() -> Self {
        Self {
            panes: false,
            open_workspaces: true,
            herdr_plus_projects: true,
            zoxide: true,
            roots: true,
            agents: true,
            servers: true,
            sessions: true,
            herdr_plus_quick_actions: true,
        }
    }
}
impl Default for SessionsConfig {
    fn default() -> Self {
        Self {
            local: true,
            entries: vec![],
        }
    }
}
impl Default for NotificationsConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            audio: false,
            sound: default_notification_sound(),
            custom_sound: None,
        }
    }
}
impl Default for ThemeConfig {
    fn default() -> Self {
        Self {
            inherit_herdr: true,
        }
    }
}
impl Default for Config {
    fn default() -> Self {
        Self {
            picker: PickerConfig::default(),
            jump_back: JumpBackConfig::default(),
            notifications: NotificationsConfig::default(),
            sources: SourcesConfig::default(),
            theme: ThemeConfig::default(),
            sessions: SessionsConfig::default(),
            integrations: vec![],
            agent_aliases: vec![],
            roots: vec![
                RootConfig {
                    path: "~/workspace".into(),
                    max_depth: 3,
                    exclude: default_root_excludes(),
                    follow_symlinks: false,
                },
                RootConfig {
                    path: "~/work".into(),
                    max_depth: 3,
                    exclude: default_root_excludes(),
                    follow_symlinks: false,
                },
                RootConfig {
                    path: "~/projects".into(),
                    max_depth: 3,
                    exclude: default_root_excludes(),
                    follow_symlinks: false,
                },
            ],
        }
    }
}

impl Config {
    pub(crate) fn load() -> Self {
        migrate_legacy_plugin_config();
        let dir = plugin_config_dir();
        let _ = fs::create_dir_all(&dir);
        let path = dir.join("config.toml");
        if !path.exists() {
            let _ = fs::write(&path, DEFAULT_CONFIG);
        }
        let mut config = match fs::read_to_string(path) {
            Ok(source) => Self::parse_or_report(&source),
            Err(_) => Self::default(),
        };
        let mut diagnostics = config.diagnostics();
        if let Some(prefix) = herdr_ctrl_prefix() {
            diagnostics.extend(config.avoid_default_prefix_conflict(prefix));
        }
        for diagnostic in diagnostics {
            eprintln!("Navigator config: {diagnostic}");
        }
        config
    }

    // Keep invalid TOML's existing notification policy separate from pure parsing.
    fn parse_or_report(source: &str) -> Self {
        let (config, error) = Self::parse_or_default(source);
        if let Some(message) = error {
            eprintln!("{message}");
            crate::herdr::notify_error(&message, &config.notifications);
        }
        config
    }

    fn parse_or_default(source: &str) -> (Self, Option<String>) {
        match toml::from_str(source) {
            Ok(config) => (config, None),
            Err(error) => (
                Self::default(),
                Some(format!("Invalid config.toml, using defaults: {error}")),
            ),
        }
    }

    fn diagnostics(&self) -> Vec<String> {
        let mut diagnostics = Vec::new();
        if self.picker.presentation == Presentation::Merged {
            diagnostics.push("picker.presentation: merged is unavailable; request retained, legacy renderer remains active".into());
        }
        diagnostics.extend(self.picker.filter_diagnostics());
        diagnostics.extend(crate::keymap::config_diagnostics(&self.picker));
        diagnostics
    }

    pub(crate) fn avoid_default_prefix_conflict(&mut self, prefix: char) -> Vec<String> {
        let prefix = prefix.to_ascii_lowercase();
        self.picker.known_host_ctrl_prefix = Some(prefix);
        let mut diagnostics = crate::keymap::host_prefix_diagnostics(&self.picker, prefix);
        let Some(source) = self.picker.filter_source_for_key(prefix) else {
            return diagnostics;
        };
        if self
            .picker
            .custom_filter_keys()
            .iter()
            .any(|(s, _)| *s == source)
        {
            diagnostics.push(format!("{}: ctrl-{prefix} conflicts with Herdr keys.prefix; explicit custom binding retained, host interception unresolved", self.picker.filter_setting_path(source)));
            return diagnostics;
        }
        if let Some(key) = ['g', 'e', 'd', 'f', 'y'].into_iter().find(|key| {
            *key != prefix
                && self.picker.filter_source_for_key(*key).is_none()
                && !crate::keymap::claimed_template_or_reserved_ctrl(&self.picker, *key)
        }) {
            diagnostics.push(format!("{}: ctrl-{prefix} conflicts with Herdr keys.prefix; default remapped to ctrl-{key}", self.picker.filter_setting_path(source)));
            self.picker
                .filter_keys
                .insert(source.label().into(), format!("ctrl-{key}"));
        } else {
            diagnostics.push(format!("{}: ctrl-{prefix} conflicts with Herdr keys.prefix; fallback keys ctrl-g/e/d/f/y exhausted, binding unchanged and host interception unresolved", self.picker.filter_setting_path(source)));
        }
        diagnostics
    }
}

fn herdr_ctrl_prefix() -> Option<char> {
    let path = env::var("XDG_CONFIG_HOME")
        .map(|root| PathBuf::from(root).join("herdr/config.toml"))
        .unwrap_or_else(|_| crate::paths::home().join(".config/herdr/config.toml"));
    let source = fs::read_to_string(path).ok()?;
    let value: toml::Value = toml::from_str(&source).ok()?;
    let prefix = value.get("keys")?.get("prefix")?.as_str()?;
    parse_ctrl_key(prefix)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rollout_defaults_and_checked_in_examples_remain_legacy() {
        for config in [
            Config::default(),
            toml::from_str("").unwrap(),
            toml::from_str("[picker]\n[sources]").unwrap(),
            toml::from_str(DEFAULT_CONFIG).unwrap(),
            toml::from_str(include_str!("../examples/config.toml")).unwrap(),
        ] {
            assert_eq!(config.picker.presentation, Presentation::Legacy);
            assert!(!config.sources.panes);
            assert_eq!(config.picker.focus_cycle_key, "f6");
            assert_eq!(config.picker.collapse_key, "left");
            assert_eq!(config.picker.expand_key, "right");
            assert_eq!(config.picker.technical_details_key, "f7");
            assert_eq!(config.picker.refresh_key, "f8");
            assert!(config.diagnostics().is_empty());
            assert_eq!(config.picker.filter_key(Source::Project), Some('p'));
            assert_eq!(config.picker.filter_key(Source::Root), Some('r'));
            assert_eq!(config.picker.filter_key(Source::Server), Some('s'));
        }
        assert_eq!(Source::COUNT, 9);
        assert_eq!(
            Source::all(),
            [
                Source::Workspace,
                Source::Project,
                Source::Server,
                Source::Session,
                Source::Zoxide,
                Source::Root,
                Source::Agent,
                Source::QuickAction,
                Source::Integration
            ]
        );
        assert_eq!(Source::Workspace.index(), 0);
        assert_eq!(Source::Integration.index(), 8);
    }

    #[test]
    fn legacy_partial_fixture_retains_settings_and_aliases() {
        let config: Config =
            toml::from_str(include_str!("../tests/fixtures/config/legacy-partial.toml")).unwrap();
        let picker = &config.picker;
        assert_eq!(picker.presentation, Presentation::Legacy);
        assert!(!config.sources.panes);
        assert!(!config.sources.open_workspaces);
        assert!(config.sources.agents && config.sources.servers);
        assert!(!config.sources.sessions);
        assert!(!picker.reuse_existing && !picker.create_missing);
        assert_eq!(picker.source_ranks()[Source::Server.index()], 2);
        assert_eq!(picker.source_priority_boost, 7);
        assert_eq!(picker.agent_sort, "priority");
        assert_eq!(picker.root_cache_seconds, 90);
        assert!(!picker.preview && !picker.detailed_rows && !picker.check_updates);
        assert!(picker.vim_mode && picker.vim_filter_search);
        assert_eq!(picker.directory_template.as_deref(), Some("default.toml"));
        assert_eq!(picker.directory_template_key, "option+enter");
        assert_eq!(picker.filter_source_for_key('g'), Some(Source::Server));
        assert!(!config.jump_back.pin_previous);
        assert!(!config.notifications.enabled && !config.notifications.audio);
        assert!(!config.sessions.local);
        assert_eq!(
            config.sessions.entries[0].remote.as_deref(),
            Some("fixture-remote")
        );
        assert!(config.diagnostics().is_empty());
    }

    #[test]
    fn unsupported_rollout_requests_are_retained_and_diagnosed_purely() {
        let (config, error) =
            Config::parse_or_default(include_str!("../tests/fixtures/config/merged-request.toml"));
        assert!(error.is_none());
        assert_eq!(config.picker.presentation, Presentation::Merged);
        assert!(config.sources.panes);
        assert_eq!(config.diagnostics(), [
            "picker.presentation: merged is unavailable; request retained, legacy renderer remains active",
        ]);
        // These remain readable, not translations into future merged behavior.
        // The fixture README assigns pin order, Query focus, and owner density
        // to later tasks rather than claiming they work in N02.
        assert!(config.jump_back.pin_previous);
        assert!(config.picker.vim_mode);
        assert!(!config.picker.detailed_rows);
        assert_eq!(config.picker.source_priority_boost, 999);
        assert_eq!(config.picker.filter_key(Source::Server), Some('s'));
    }

    #[test]
    fn pane_and_presentation_requests_are_independent() {
        for presentation in ["legacy", "merged"] {
            for panes in [false, true] {
                let config: Config = toml::from_str(&format!(
                    "[picker]\npresentation = '{presentation}'\n[sources]\npanes = {panes}"
                ))
                .unwrap();
                assert_eq!(config.sources.panes, panes);
                assert_eq!(
                    config.picker.presentation == Presentation::Merged,
                    presentation == "merged"
                );
                assert_eq!(
                    config.diagnostics().len(),
                    usize::from(presentation == "merged")
                );
                assert_eq!(
                    config.picker.source_ranks(),
                    PickerConfig::default().source_ranks()
                );
            }
        }
    }

    #[test]
    fn unknown_presentation_uses_existing_invalid_config_fallback() {
        let (config, error) =
            Config::parse_or_default("[picker]\npresentation = 'future'\npreview = false");
        let error = error.unwrap();
        assert!(error.starts_with("Invalid config.toml, using defaults:"));
        assert!(error.contains("unknown variant `future`"));
        assert!(error.contains("`legacy` or `merged`"));
        assert_eq!(config.picker.presentation, Presentation::Legacy);
        assert!(config.picker.preview);
        assert!(config.notifications.enabled);
        assert!(!config.sources.panes);
    }

    #[test]
    fn disabled_vim_aliases_do_not_report_impossible_collisions() {
        let disabled: Config = toml::from_str(
            r#"
            [picker]
            vim_mode = false
            [picker.filter_keys]
            project = "ctrl-j"
        "#,
        )
        .unwrap();
        assert!(!disabled
            .diagnostics()
            .iter()
            .any(|line| line.contains("j [vim Normal]")));

        let enabled: Config = toml::from_str(
            r#"
            [picker]
            vim_mode = true
            [picker.filter_keys]
            project = "ctrl-j"
        "#,
        )
        .unwrap();
        assert!(enabled
            .diagnostics()
            .iter()
            .any(|line| { line.contains("j [vim Normal]") && line.contains("legacy MoveDown") }));
    }

    #[test]
    fn collision_fixture_is_independent_of_hashmap_insertion_order() {
        let original: Config = toml::from_str(include_str!(
            "../tests/fixtures/config/binding-collisions.toml"
        ))
        .unwrap();
        let diagnostics = original.diagnostics();
        let keys = original.picker.sorted_filter_keys();
        for reverse in [false, true] {
            for offset in 0..keys.len() {
                let mut config = original.clone();
                config.picker.filter_keys.clear();
                let mut reordered = keys.clone();
                reordered.rotate_left(offset);
                if reverse {
                    reordered.reverse();
                }
                config.picker.filter_keys.extend(
                    reordered
                        .into_iter()
                        .map(|(name, value)| (name.into(), value.into())),
                );
                assert_eq!(config.diagnostics(), diagnostics);
                assert_eq!(
                    config.picker.custom_filter_keys(),
                    original.picker.custom_filter_keys()
                );
                for source in Source::all() {
                    assert_eq!(
                        config.picker.filter_key(source),
                        original.picker.filter_key(source)
                    );
                }
                for ch in 'a'..='z' {
                    assert_eq!(
                        config.picker.filter_source_for_key(ch),
                        original.picker.filter_source_for_key(ch)
                    );
                }
            }
        }
        assert_eq!(original.picker.filter_key(Source::Agent), Some('c'));
        assert_eq!(original.picker.filter_key(Source::Project), Some('p'));
        assert_eq!(original.picker.filter_key(Source::Zoxide), None);
        assert_eq!(original.picker.filter_source_for_key('z'), None);
        for fragment in [
            "picker.filter_keys.agents: ctrl-e duplicates source alias picker.filter_keys.agent at ctrl-c",
            "picker.filter_keys.projects: ctrl-g duplicates source alias picker.filter_keys.project at ctrl-p",
            "picker.filter_keys.zoxide: ctrl-p conflicts with picker.filter_keys.project",
            "picker.filter_keys.unknown: ctrl-v has unknown source",
            "picker.filter_keys.quick: invalid Ctrl key",
            "ctrl-c conflicts with legacy Back",
            "j [vim Normal] conflicts with legacy MoveDown",
            "legacy Clear: ctrl-u conflicts with picker.filter_keys.plugin",
            "legacy CloseWorkspace: ctrl-x conflicts with picker.filter_keys.session",
            "legacy ToggleMark: ctrl-b conflicts with picker.filter_keys.open",
            "legacy TogglePreview: ctrl-o conflicts with picker.filter_keys.server",
            "picker.directory_template_key [when template available]: ctrl-p conflicts with picker.filter_keys.project",
            "picker.focus_cycle_key: ctrl-p conflicts with picker.refresh_key; both bindings inactive",
            "picker.refresh_key: ctrl-p conflicts with picker.filter_keys.project",
            "picker.collapse_key: ctrl-p conflicts with picker.filter_keys.project",
            "picker.expand_key: ctrl-o conflicts with picker.filter_keys.server",
            "picker.technical_details_key: ctrl-o conflicts with picker.filter_keys.server",
        ] {
            assert!(diagnostics.iter().any(|line| line.contains(fragment)), "missing {fragment}: {diagnostics:#?}");
        }
    }

    #[test]
    fn spec3_reserved_key_diagnostics_are_stable_without_a_runtime_template() {
        let mut absent = Config::default();
        absent.picker.focus_cycle_key = "F5".into();
        absent.picker.collapse_key = "F8".into();
        absent.picker.expand_key = "option+enter".into();
        absent.picker.technical_details_key = "CTRL+X".into();
        let absent_diagnostics = absent.diagnostics();

        let mut present = absent.clone();
        present.picker.directory_template = Some("default.toml".into());
        let present_diagnostics = present.diagnostics();
        assert_eq!(absent_diagnostics, present_diagnostics);

        for fragment in [
            "picker.focus_cycle_key: f5 conflicts with legacy Update [when update available]",
            "picker.collapse_key: f8 conflicts with picker.refresh_key",
            "picker.expand_key: alt-enter conflicts with picker.directory_template_key [when template available]",
            "picker.technical_details_key: ctrl-x conflicts with legacy CloseWorkspace",
        ] {
            assert!(
                absent_diagnostics
                    .iter()
                    .any(|line| line.contains(fragment) && line.contains("inactive")),
                "missing {fragment}: {absent_diagnostics:#?}"
            );
        }
    }

    #[test]
    fn first_valid_sorted_alias_and_custom_over_default_are_explicit() {
        let config: Config = toml::from_str(
            r#"
            [picker.filter_keys]
            agent = "invalid"
            agents = "CTRL+P"
            remote = "^g"
            server = "ctrl-e"
        "#,
        )
        .unwrap();
        assert_eq!(
            config.picker.filter_source_for_key('p'),
            Some(Source::Agent)
        );
        assert_eq!(config.picker.filter_key(Source::Project), None);
        assert_eq!(config.picker.filter_key(Source::Server), Some('g'));
        assert_eq!(config.picker.filter_source_for_key('e'), None);
        assert!(config.diagnostics().iter().any(|line| line == "picker.filter_keys.project [default]: ctrl-p conflicts with picker.filter_keys.agents; custom wins, default inactive"));

        let mut picker = PickerConfig::default();
        picker.filter_keys.insert("agent".into(), "ctrl-e".into());
        picker.filter_keys.insert("Agents".into(), "ctrl-g".into());
        assert_eq!(picker.filter_key(Source::Agent), Some('g'));
        picker.filter_keys.insert("project".into(), "ctrl-g".into());
        // Sorted original config names win, not Source::all registry order.
        assert_eq!(picker.filter_source_for_key('g'), Some(Source::Agent));
        assert_eq!(picker.filter_key(Source::Project), None);
    }

    #[test]
    fn n10_host_prefix_is_normalized_and_blocks_the_conflicting_merged_binding() {
        let mut config: Config = toml::from_str(
            r#"
            [picker]
            presentation = "merged"
            focus_cycle_key = "CTRL+G"
            "#,
        )
        .unwrap();

        let diagnostics = config.avoid_default_prefix_conflict('G');
        assert_eq!(config.picker.focus_cycle_key, "CTRL+G");
        assert_eq!(
            crate::keymap::resolve(
                &config.picker,
                crate::keymap::InputContext {
                    presentation: crate::keymap::Presentation::Merged,
                    focus: crate::keymap::Focus::Query,
                    vim: crate::keymap::VimState::Search,
                    menu: false,
                },
                crate::keymap::Availability::default(),
                crossterm::event::KeyEvent::new(
                    crossterm::event::KeyCode::Char('g'),
                    crossterm::event::KeyModifiers::CONTROL,
                ),
            ),
            None
        );
        assert!(diagnostics.iter().any(|line| {
            line.contains("picker.focus_cycle_key: ctrl-g conflicts with Herdr keys.prefix")
                && line.contains("merged binding inactive")
        }));
    }

    #[test]
    fn host_prefix_runtime_metadata_is_skipped_cloned_and_instance_local() {
        let forged: Config = toml::from_str(
            r#"
            [picker]
            known_host_ctrl_prefix = "x"
            "#,
        )
        .unwrap();
        assert_eq!(forged.picker.known_host_ctrl_prefix, None);

        let mut first = Config::default();
        first.avoid_default_prefix_conflict('G');
        assert_eq!(first.picker.known_host_ctrl_prefix, Some('g'));
        assert_eq!(
            first.clone().picker.known_host_ctrl_prefix,
            first.picker.known_host_ctrl_prefix
        );

        let mut second = Config::default();
        second.avoid_default_prefix_conflict('H');
        assert_eq!(second.picker.known_host_ctrl_prefix, Some('h'));
        assert_eq!(first.picker.known_host_ctrl_prefix, Some('g'));
    }

    #[test]
    fn host_prefix_fallback_skips_template_reserved_and_custom_claims() {
        let mut config = Config::default();
        config.picker.directory_template = Some("fixture.toml".into());
        config.picker.directory_template_key = "^g".into();
        config.picker.focus_cycle_key = "ctrl-e".into();
        config.picker.refresh_key = "CTRL+D".into();
        config
            .picker
            .filter_keys
            .insert("project".into(), "ctrl-f".into());
        assert_eq!(config.avoid_default_prefix_conflict('a'), ["picker.filter_keys.agent [default]: ctrl-a conflicts with Herdr keys.prefix; default remapped to ctrl-y"]);
        assert_eq!(
            config.picker.filter_source_for_key('y'),
            Some(Source::Agent)
        );
        assert_eq!(config.picker.filter_source_for_key('a'), None);
    }

    #[test]
    fn host_prefix_exhaustion_and_explicit_custom_are_not_silently_remapped() {
        let mut config = Config::default();
        config.picker.directory_template = Some("fixture.toml".into());
        config.picker.directory_template_key = "ctrl-g".into();
        config.picker.focus_cycle_key = "ctrl-e".into();
        config.picker.refresh_key = "ctrl-d".into();
        config
            .picker
            .filter_keys
            .insert("project".into(), "ctrl-f".into());
        config
            .picker
            .filter_keys
            .insert("root".into(), "ctrl-y".into());
        let diagnostic = config.avoid_default_prefix_conflict('a');
        assert_eq!(config.picker.filter_key(Source::Agent), Some('a'));
        assert!(diagnostic[0].contains("fallback keys ctrl-g/e/d/f/y exhausted"));

        config
            .picker
            .filter_keys
            .insert("agent".into(), "ctrl-a".into());
        assert!(config.avoid_default_prefix_conflict('a')[0]
            .contains("explicit custom binding retained, host interception unresolved"));
        assert_eq!(config.picker.filter_key(Source::Agent), Some('a'));
        // A custom binding can conflict with a host key that has no default filter.
        config
            .picker
            .filter_keys
            .insert("agent".into(), "ctrl-v".into());
        assert!(config.avoid_default_prefix_conflict('v')[0]
            .contains("picker.filter_keys.agent: ctrl-v conflicts"));
        assert_eq!(config.picker.filter_key(Source::Agent), Some('v'));
    }

    #[test]
    fn merged_source_preference_handles_extreme_boosts_without_changing_legacy_bonus() {
        for boost in [i64::MIN, 0, i64::MAX] {
            let picker = PickerConfig {
                source_priority_boost: boost,
                ..PickerConfig::default()
            };
            assert_eq!(
                picker.merged_preference(0),
                Source::COUNT as i128 * i128::from(boost)
            );
            assert_eq!(picker.merged_preference(usize::MAX), 0);
        }
        let picker = PickerConfig::default();
        for rank in 0..=Source::COUNT {
            assert_eq!(
                picker.merged_preference(rank),
                i128::from(picker.bonus_for_rank(rank))
            );
        }
    }

    #[test]
    fn default_source_order_prioritizes_open_workspaces_then_agents() {
        let picker = PickerConfig::default();
        let ranks = picker.source_ranks();
        let rank = |source: Source| ranks[source.index()];

        assert_eq!(rank(Source::Workspace), 0);
        assert_eq!(rank(Source::Agent), 1);
        assert_eq!(rank(Source::Session), 3);
        assert_eq!(rank(Source::Root), 5);
        assert_eq!(rank(Source::Server), 6);
        assert!(
            picker.bonus_for_rank(rank(Source::Root)) > picker.bonus_for_rank(rank(Source::Server))
        );

        let unlisted = PickerConfig {
            source_order: vec!["agent".into()],
            ..PickerConfig::default()
        };
        assert_eq!(unlisted.source_ranks()[Source::Root.index()], Source::COUNT);
        assert_eq!(unlisted.bonus_for_rank(Source::COUNT), 0);
    }

    #[test]
    fn detailed_rows_default_on_and_can_be_disabled() {
        assert!(Config::default().picker.detailed_rows);

        let config: Config = toml::from_str(
            r#"
            [picker]
            detailed_rows = false
            "#,
        )
        .unwrap();

        assert!(!config.picker.detailed_rows);
    }

    #[test]
    fn directory_template_defaults_off_and_accepts_herdr_plus_filename() {
        assert!(Config::default().picker.directory_template.is_none());
        assert_eq!(Config::default().picker.directory_template_key, "alt-enter");

        let config: Config = toml::from_str(
            r#"
            [picker]
            directory_template = "default.toml"
            directory_template_key = "ctrl-g"
            "#,
        )
        .unwrap();

        assert_eq!(
            config.picker.directory_template.as_deref(),
            Some("default.toml")
        );
        assert_eq!(config.picker.directory_template_key, "ctrl-g");
    }

    #[test]
    fn update_checks_default_off_and_can_be_enabled() {
        assert!(!Config::default().picker.check_updates);

        let config: Config = toml::from_str(
            r#"
            [picker]
            check_updates = true
            "#,
        )
        .unwrap();

        assert!(config.picker.check_updates);
    }

    #[test]
    fn conflicting_host_prefix_remaps_only_the_default_filter() {
        let mut config = Config::default();
        config.avoid_default_prefix_conflict('a');
        assert_eq!(
            config.picker.filter_source_for_key('g'),
            Some(Source::Agent)
        );
        assert_eq!(config.picker.filter_source_for_key('a'), None);

        let mut customized = Config::default();
        customized
            .picker
            .filter_keys
            .insert("agent".into(), "ctrl-e".into());
        customized.avoid_default_prefix_conflict('a');
        assert_eq!(
            customized.picker.filter_source_for_key('e'),
            Some(Source::Agent)
        );
    }

    #[test]
    fn root_scanning_is_cached_and_excludes_build_trees_by_default() {
        let config = Config::default();
        assert_eq!(config.picker.root_cache_seconds, 60);
        assert!(config.picker.confirm_close_workspace);
        assert!(config.roots[0].exclude.contains(&"node_modules".into()));
        assert!(!config.roots[0].follow_symlinks);
    }

    #[test]
    fn notification_audio_defaults_off_and_supports_custom() {
        let default = Config::default().notifications;
        assert!(!default.audio);
        assert_eq!(default.sound, "default");

        let custom: Config = toml::from_str(
            r#"
            [notifications]
            audio = true
            sound = "custom"
            custom_sound = "~/sounds/navigator.wav"
            "#,
        )
        .unwrap();
        assert!(custom.notifications.audio);
        assert_eq!(custom.notifications.sound, "custom");
        assert_eq!(
            custom.notifications.custom_sound.as_deref(),
            Some("~/sounds/navigator.wav")
        );

        let silent: Config = toml::from_str(
            r#"
            [notifications]
            audio = false
            "#,
        )
        .unwrap();
        assert!(!silent.notifications.audio);
    }

    #[test]
    fn jump_back_defaults_to_enabled_and_pinned() {
        let config = Config::default();

        assert!(config.jump_back.enabled);
        assert!(config.jump_back.pin_previous);
    }

    #[test]
    fn parses_jump_back_config() {
        let config: Config = toml::from_str(
            r#"
            [jump_back]
            enabled = false
            pin_previous = false
            "#,
        )
        .unwrap();

        assert!(!config.jump_back.enabled);
        assert!(!config.jump_back.pin_previous);
    }

    #[test]
    fn parses_command_integration_config() {
        let config: Config = toml::from_str(
            r#"
            [[integrations]]
            id = "bookmarks"
            label = "Bookmarks"
            collect = "bookmarks list --json"
            open = "bookmarks open {{id}}"
            collect_timeout_ms = 2500
            open_timeout_ms = 30000
            max_output_bytes = 4096
            notify_success = false
            "#,
        )
        .unwrap();

        assert_eq!(config.integrations.len(), 1);
        assert_eq!(config.integrations[0].id, "bookmarks");
        assert_eq!(config.integrations[0].label, "Bookmarks");
        assert_eq!(config.integrations[0].collect_timeout_ms, 2500);
        assert_eq!(config.integrations[0].open_timeout_ms, 30000);
        assert_eq!(config.integrations[0].max_output_bytes, 4096);
        assert!(!config.integrations[0].notify_success);
        assert!(config.integrations[0].notify_error);
    }

    #[test]
    fn custom_filter_key_overrides_default_source_key() {
        let config: Config = toml::from_str(
            r#"
            [picker.filter_keys]
            server = "ctrl-g"
            "#,
        )
        .unwrap();

        assert_eq!(
            config.picker.filter_source_for_key('g'),
            Some(Source::Server)
        );
        assert_eq!(config.picker.filter_source_for_key('s'), None);
        assert_eq!(
            config.picker.filter_source_for_key('a'),
            Some(Source::Agent)
        );
        assert_eq!(config.picker.filter_key(Source::Server), Some('g'));
    }

    #[test]
    fn parses_agent_aliases() {
        let config: Config = toml::from_str(
            r#"
            [[agent_aliases]]
            alias = "main ai dot"
            agent = "claude"
            workspace = "Dotfiles"
            path = "dotfiles"
            "#,
        )
        .unwrap();

        assert_eq!(config.agent_aliases.len(), 1);
        assert!(config.agent_aliases[0].matches("claude", "Dotfiles", "/home/fenix/dotfiles"));
        assert!(!config.agent_aliases[0].matches("codex", "Dotfiles", "/home/fenix/dotfiles"));
    }

    #[test]
    fn parses_builtin_session_config() {
        let config: Config = toml::from_str(
            r#"
            [sessions]
            local = false

            [[sessions.entries]]
            name = "prod"
            remote = "prod-host"
            session = "default"
            tags = ["api"]
            "#,
        )
        .unwrap();

        assert!(config.sources.sessions);
        assert!(!config.sessions.local);
        assert_eq!(config.sessions.entries[0].name, "prod");
        assert_eq!(
            config.sessions.entries[0].remote.as_deref(),
            Some("prod-host")
        );
        assert_eq!(
            config.sessions.entries[0].session.as_deref(),
            Some("default")
        );
    }
}
