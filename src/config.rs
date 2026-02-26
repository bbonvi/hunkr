//! Application configuration loaded from `~/.config/hunkr/config.yaml`.

use std::{
    env, fs,
    path::{Path, PathBuf},
};

use anyhow::Context;
use serde::Deserialize;

const DEFAULT_DIFF_WHEEL_SCROLL_LINES: isize = 1;
const DEFAULT_LIST_WHEEL_COALESCE_MS: u64 = 28;

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
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            startup_theme: StartupTheme::Dark,
            diff_wheel_scroll_lines: DEFAULT_DIFF_WHEEL_SCROLL_LINES,
            list_wheel_coalesce_ms: DEFAULT_LIST_WHEEL_COALESCE_MS,
            nerd_fonts: true,
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
mod tests {
    use super::*;

    #[test]
    fn config_defaults_when_file_missing() {
        let temp = tempfile::tempdir().expect("tempdir");
        let path = temp.path().join("missing.yaml");
        let loaded = AppConfig::load_from_path(&path).expect("load");

        assert_eq!(loaded.startup_theme, StartupTheme::Dark);
        assert_eq!(loaded.diff_wheel_scroll_lines, 1);
        assert_eq!(loaded.list_wheel_coalesce_ms, 28);
        assert!(loaded.nerd_fonts);
    }

    #[test]
    fn config_parses_lowercase_keys() {
        let temp = tempfile::tempdir().expect("tempdir");
        let path = temp.path().join("config.yaml");
        fs::write(
            &path,
            "startup_theme: light\ndiff_wheel_scroll_lines: 3\nlist_wheel_coalesce_ms: 12\nnerd_fonts: false\n",
        )
        .expect("write");

        let loaded = AppConfig::load_from_path(&path).expect("load");
        assert_eq!(loaded.startup_theme, StartupTheme::Light);
        assert_eq!(loaded.diff_wheel_scroll_lines, 3);
        assert_eq!(loaded.list_wheel_coalesce_ms, 12);
        assert!(!loaded.nerd_fonts);
    }

    #[test]
    fn config_clamps_non_positive_wheel_scroll() {
        let temp = tempfile::tempdir().expect("tempdir");
        let path = temp.path().join("config.yaml");
        fs::write(&path, "diff_wheel_scroll_lines: 0\n").expect("write");

        let loaded = AppConfig::load_from_path(&path).expect("load");
        assert_eq!(loaded.diff_wheel_scroll_lines, 1);
        assert!(loaded.nerd_fonts);
    }

    #[test]
    fn config_clamps_zero_list_wheel_coalesce() {
        let temp = tempfile::tempdir().expect("tempdir");
        let path = temp.path().join("config.yaml");
        fs::write(&path, "list_wheel_coalesce_ms: 0\n").expect("write");

        let loaded = AppConfig::load_from_path(&path).expect("load");
        assert_eq!(loaded.list_wheel_coalesce_ms, 28);
    }
}
