//! Application configuration loaded from `~/.config/hunkr/config.yaml`.

use std::{
    collections::BTreeMap,
    env, fs,
    path::{Path, PathBuf},
};

use anyhow::Context;
use serde::Deserialize;

const DEFAULT_DIFF_WHEEL_SCROLL_LINES: isize = 1;

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
    pub nerd_fonts: bool,
    pub nerd_font_icons: NerdFontIconConfig,
}

/// Optional Nerd Font icon overrides loaded from config.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct NerdFontIconConfig {
    pub directory_icon: Option<String>,
    pub default_file_icon: Option<String>,
    pub env_icon: Option<String>,
    pub docker_icon: Option<String>,
    pub special_files: BTreeMap<String, String>,
    pub extensions: BTreeMap<String, String>,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            startup_theme: StartupTheme::Dark,
            diff_wheel_scroll_lines: DEFAULT_DIFF_WHEEL_SCROLL_LINES,
            nerd_fonts: true,
            nerd_font_icons: NerdFontIconConfig::default(),
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
        assert!(loaded.nerd_fonts);
        assert!(loaded.nerd_font_icons.extensions.is_empty());
    }

    #[test]
    fn config_parses_lowercase_keys() {
        let temp = tempfile::tempdir().expect("tempdir");
        let path = temp.path().join("config.yaml");
        fs::write(
            &path,
            "startup_theme: light\ndiff_wheel_scroll_lines: 3\nnerd_fonts: false\nnerd_font_icons:\n  env_icon: \"\"\n  extensions:\n    rs: \"\"\n",
        )
        .expect("write");

        let loaded = AppConfig::load_from_path(&path).expect("load");
        assert_eq!(loaded.startup_theme, StartupTheme::Light);
        assert_eq!(loaded.diff_wheel_scroll_lines, 3);
        assert!(!loaded.nerd_fonts);
        assert_eq!(loaded.nerd_font_icons.env_icon.as_deref(), Some(""));
        assert_eq!(
            loaded
                .nerd_font_icons
                .extensions
                .get("rs")
                .map(String::as_str),
            Some("")
        );
    }

    #[test]
    fn config_clamps_non_positive_wheel_scroll() {
        let temp = tempfile::tempdir().expect("tempdir");
        let path = temp.path().join("config.yaml");
        fs::write(&path, "diff_wheel_scroll_lines: 0\n").expect("write");

        let loaded = AppConfig::load_from_path(&path).expect("load");
        assert_eq!(loaded.diff_wheel_scroll_lines, 1);
        assert!(loaded.nerd_fonts);
        assert!(loaded.nerd_font_icons.special_files.is_empty());
    }
}
