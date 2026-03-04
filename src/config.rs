//! Application configuration loaded from `~/.config/hunkr/config.yaml`.

use std::{
    env, fs,
    path::{Path, PathBuf},
};

use anyhow::Context;
use serde::Deserialize;

const DEFAULT_DIFF_WHEEL_SCROLL_LINES: isize = 1;
const DEFAULT_LIST_WHEEL_COALESCE_MS: u64 = 28;
const DEFAULT_HISTORY_LIMIT: usize = 400;
const DEFAULT_AUTO_REFRESH_EVERY_SECS: u64 = 2;
const DEFAULT_RELATIVE_TIME_REDRAW_EVERY_SECS: u64 = 30;
const DEFAULT_THEME_RELOAD_POLL_EVERY_MS: u64 = 1_000;
const DEFAULT_SELECTION_REBUILD_DEBOUNCE_MS: u64 = 120;
const DEFAULT_TERMINAL_CLEAR_EVERY_SECS: u64 = 120;
const DEFAULT_DIFF_CURSOR_SCROLL_OFF_LINES: usize = 3;

/// Startup UI theme name from config.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum StartupTheme {
    #[default]
    Dark,
    Light,
}

/// Minimal runtime settings for hunkr.
#[derive(Debug, Clone, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct AppConfig {
    pub startup_theme: StartupTheme,
    pub diff_wheel_scroll_lines: isize,
    pub list_wheel_coalesce_ms: u64,
    pub nerd_fonts: bool,
    pub history_limit: usize,
    pub auto_refresh_every_secs: u64,
    pub relative_time_redraw_every_secs: u64,
    pub theme_reload_poll_every_ms: u64,
    pub selection_rebuild_debounce_ms: u64,
    pub terminal_clear_every_secs: u64,
    pub diff_cursor_scroll_off_lines: usize,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            startup_theme: StartupTheme::Dark,
            diff_wheel_scroll_lines: DEFAULT_DIFF_WHEEL_SCROLL_LINES,
            list_wheel_coalesce_ms: DEFAULT_LIST_WHEEL_COALESCE_MS,
            nerd_fonts: true,
            history_limit: DEFAULT_HISTORY_LIMIT,
            auto_refresh_every_secs: DEFAULT_AUTO_REFRESH_EVERY_SECS,
            relative_time_redraw_every_secs: DEFAULT_RELATIVE_TIME_REDRAW_EVERY_SECS,
            theme_reload_poll_every_ms: DEFAULT_THEME_RELOAD_POLL_EVERY_MS,
            selection_rebuild_debounce_ms: DEFAULT_SELECTION_REBUILD_DEBOUNCE_MS,
            terminal_clear_every_secs: DEFAULT_TERMINAL_CLEAR_EVERY_SECS,
            diff_cursor_scroll_off_lines: DEFAULT_DIFF_CURSOR_SCROLL_OFF_LINES,
        }
    }
}

impl AppConfig {
    /// Load config from the default path. Missing file falls back to defaults.
    pub fn load() -> anyhow::Result<Self> {
        let path = config_path();
        Self::load_from_path(&path)
            .with_context(|| format!("failed to load config from {}", path.display()))
    }

    fn load_from_path(path: &Path) -> anyhow::Result<Self> {
        if !path.is_file() {
            return Ok(Self::default());
        }

        let raw = fs::read_to_string(path)
            .with_context(|| format!("failed to read config file {}", path.display()))?;
        let mut parsed: Self = serde_yaml::from_str(&raw)
            .with_context(|| format!("failed to parse config yaml {}", path.display()))?;
        parsed.validate();
        Ok(parsed)
    }

    fn validate(&mut self) {
        if self.diff_wheel_scroll_lines < 1 {
            self.diff_wheel_scroll_lines = DEFAULT_DIFF_WHEEL_SCROLL_LINES;
        }
        if self.list_wheel_coalesce_ms == 0 {
            self.list_wheel_coalesce_ms = DEFAULT_LIST_WHEEL_COALESCE_MS;
        }
        if self.history_limit == 0 {
            self.history_limit = DEFAULT_HISTORY_LIMIT;
        }
        if self.auto_refresh_every_secs == 0 {
            self.auto_refresh_every_secs = DEFAULT_AUTO_REFRESH_EVERY_SECS;
        }
        if self.relative_time_redraw_every_secs == 0 {
            self.relative_time_redraw_every_secs = DEFAULT_RELATIVE_TIME_REDRAW_EVERY_SECS;
        }
        if self.theme_reload_poll_every_ms == 0 {
            self.theme_reload_poll_every_ms = DEFAULT_THEME_RELOAD_POLL_EVERY_MS;
        }
        if self.selection_rebuild_debounce_ms == 0 {
            self.selection_rebuild_debounce_ms = DEFAULT_SELECTION_REBUILD_DEBOUNCE_MS;
        }
        if self.terminal_clear_every_secs == 0 {
            self.terminal_clear_every_secs = DEFAULT_TERMINAL_CLEAR_EVERY_SECS;
        }
        if self.diff_cursor_scroll_off_lines == 0 {
            self.diff_cursor_scroll_off_lines = DEFAULT_DIFF_CURSOR_SCROLL_OFF_LINES;
        }
    }
}

/// Resolve config file path (`$XDG_CONFIG_HOME/hunkr/config.yaml` fallback `~/.config/hunkr/config.yaml`).
pub fn config_path() -> PathBuf {
    config_base_dir().join("hunkr").join("config.yaml")
}

fn config_base_dir() -> PathBuf {
    if let Ok(xdg) = env::var("XDG_CONFIG_HOME")
        && !xdg.trim().is_empty()
    {
        return PathBuf::from(xdg);
    }

    if let Ok(home) = env::var("HOME")
        && !home.trim().is_empty()
    {
        return PathBuf::from(home).join(".config");
    }

    PathBuf::from(".config")
}

#[cfg(test)]
mod tests;
