//! Runtime theme palette loading and config-directory live-reload tracking.
use std::{
    fs,
    path::{Path, PathBuf},
    time::SystemTime,
};

use anyhow::{Context, bail};
use ratatui::style::Color;
use serde::Deserialize;

use super::{ThemeMode, UiTheme};

pub(super) const THEME_FILE_NAME: &str = "theme.yaml";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ThemeReloadOutcome {
    Unchanged,
    LoadedFromFile,
    ResetToDefaults,
}

#[derive(Debug, Clone)]
pub(super) struct ThemeRuntimeState {
    theme_path: PathBuf,
    watch_dir: PathBuf,
    dir_present: bool,
    last_dir_modified: Option<SystemTime>,
    file_present: bool,
    last_theme_file_modified: Option<SystemTime>,
    catalog: ThemeCatalog,
}

impl ThemeRuntimeState {
    pub(super) fn new(theme_path: PathBuf) -> Self {
        let watch_dir = theme_path
            .parent()
            .map(Path::to_path_buf)
            .unwrap_or_else(|| PathBuf::from("."));
        Self {
            theme_path,
            watch_dir,
            dir_present: false,
            last_dir_modified: None,
            file_present: false,
            last_theme_file_modified: None,
            catalog: ThemeCatalog::defaults(),
        }
    }

    pub(super) fn for_mode(&self, mode: ThemeMode) -> &UiTheme {
        self.catalog.for_mode(mode)
    }

    pub(super) fn path(&self) -> &Path {
        &self.theme_path
    }

    pub(super) fn reload_if_changed(&mut self, force: bool) -> anyhow::Result<ThemeReloadOutcome> {
        let file_metadata = fs::metadata(&self.theme_path).ok();
        let file_modified = file_metadata
            .as_ref()
            .and_then(|entry| entry.modified().ok());

        if !self.source_changed(force, file_metadata.as_ref(), file_modified) {
            return Ok(ThemeReloadOutcome::Unchanged);
        }

        if file_metadata.is_none() {
            if self.file_present {
                self.file_present = false;
                self.last_theme_file_modified = None;
                self.catalog = ThemeCatalog::defaults();
                return Ok(ThemeReloadOutcome::ResetToDefaults);
            }
            return Ok(ThemeReloadOutcome::Unchanged);
        }

        self.file_present = true;
        self.last_theme_file_modified = file_modified;
        self.catalog = ThemeCatalog::load_from_path(&self.theme_path)?;
        Ok(ThemeReloadOutcome::LoadedFromFile)
    }

    fn source_changed(
        &mut self,
        force: bool,
        file_metadata: Option<&fs::Metadata>,
        file_modified: Option<SystemTime>,
    ) -> bool {
        let dir_metadata = fs::metadata(&self.watch_dir).ok();
        let dir_modified = dir_metadata
            .as_ref()
            .and_then(|entry| entry.modified().ok());

        let dir_changed = if dir_metadata.is_none() {
            let changed = !self.dir_present;
            self.dir_present = false;
            self.last_dir_modified = None;
            changed
        } else {
            let changed = !self.dir_present || self.last_dir_modified != dir_modified;
            self.dir_present = true;
            self.last_dir_modified = dir_modified;
            changed
        };

        let file_exists = file_metadata.is_some();
        let file_changed = self.file_present != file_exists
            || (file_exists && self.last_theme_file_modified != file_modified);

        force || dir_changed || file_changed
    }
}

#[derive(Debug, Clone)]
struct ThemeCatalog {
    dark: UiTheme,
    light: UiTheme,
}

impl ThemeCatalog {
    fn defaults() -> Self {
        Self {
            dark: UiTheme::from_mode(ThemeMode::Dark),
            light: UiTheme::from_mode(ThemeMode::Light),
        }
    }

    fn for_mode(&self, mode: ThemeMode) -> &UiTheme {
        match mode {
            ThemeMode::Dark => &self.dark,
            ThemeMode::Light => &self.light,
        }
    }

    fn load_from_path(path: &Path) -> anyhow::Result<Self> {
        let raw = fs::read_to_string(path)
            .with_context(|| format!("failed to read theme file {}", path.display()))?;
        let parsed: ThemeFile = serde_yaml::from_str(&raw)
            .with_context(|| format!("failed to parse theme yaml {}", path.display()))?;
        let dark = parsed.dark.into_ui_theme()?;
        let light = parsed.light.into_ui_theme()?;
        Ok(Self { dark, light })
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct ThemeFile {
    dark: ThemeFileMode,
    light: ThemeFileMode,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct ThemeFileMode {
    border: ThemeColor,
    focus_border: ThemeColor,
    accent: ThemeColor,
    panel_title_bg: ThemeColor,
    panel_title_fg: ThemeColor,
    footer_chip_bg: ThemeColor,
    text: ThemeColor,
    muted: ThemeColor,
    dimmed: ThemeColor,
    cursor_bg: ThemeColor,
    focused_cursor_bg: ThemeColor,
    cursor_visual_overlap_weight: u8,
    block_cursor_fg: ThemeColor,
    block_cursor_bg: ThemeColor,
    visual_bg: ThemeColor,
    commit_selected_bg: ThemeColor,
    search_match_fg: ThemeColor,
    search_match_bg: ThemeColor,
    search_current_fg: ThemeColor,
    search_current_bg: ThemeColor,
    reviewed: ThemeColor,
    unreviewed: ThemeColor,
    issue: ThemeColor,
    resolved: ThemeColor,
    unpushed: ThemeColor,
    diff_add: ThemeColor,
    diff_add_bg: ThemeColor,
    diff_remove: ThemeColor,
    diff_remove_bg: ThemeColor,
    diff_meta: ThemeColor,
    diff_header: ThemeColor,
    dir: ThemeColor,
    modal_bg: ThemeColor,
    modal_editor_bg: ThemeColor,
    modal_cursor_fg: ThemeColor,
    modal_cursor_bg: ThemeColor,
}

impl ThemeFileMode {
    fn into_ui_theme(self) -> anyhow::Result<UiTheme> {
        if self.cursor_visual_overlap_weight == 0 {
            bail!("cursor_visual_overlap_weight must be between 1 and 255");
        }

        Ok(UiTheme {
            border: self.border.into_color(),
            focus_border: self.focus_border.into_color(),
            accent: self.accent.into_color(),
            panel_title_bg: self.panel_title_bg.into_color(),
            panel_title_fg: self.panel_title_fg.into_color(),
            footer_chip_bg: self.footer_chip_bg.into_color(),
            text: self.text.into_color(),
            muted: self.muted.into_color(),
            dimmed: self.dimmed.into_color(),
            cursor_bg: self.cursor_bg.into_color(),
            focused_cursor_bg: self.focused_cursor_bg.into_color(),
            cursor_visual_overlap_weight: self.cursor_visual_overlap_weight,
            block_cursor_fg: self.block_cursor_fg.into_color(),
            block_cursor_bg: self.block_cursor_bg.into_color(),
            visual_bg: self.visual_bg.into_color(),
            commit_selected_bg: self.commit_selected_bg.into_color(),
            search_match_fg: self.search_match_fg.into_color(),
            search_match_bg: self.search_match_bg.into_color(),
            search_current_fg: self.search_current_fg.into_color(),
            search_current_bg: self.search_current_bg.into_color(),
            reviewed: self.reviewed.into_color(),
            unreviewed: self.unreviewed.into_color(),
            issue: self.issue.into_color(),
            resolved: self.resolved.into_color(),
            unpushed: self.unpushed.into_color(),
            diff_add: self.diff_add.into_color(),
            diff_add_bg: self.diff_add_bg.into_color(),
            diff_remove: self.diff_remove.into_color(),
            diff_remove_bg: self.diff_remove_bg.into_color(),
            diff_meta: self.diff_meta.into_color(),
            diff_header: self.diff_header.into_color(),
            dir: self.dir.into_color(),
            modal_bg: self.modal_bg.into_color(),
            modal_editor_bg: self.modal_editor_bg.into_color(),
            modal_cursor_fg: self.modal_cursor_fg.into_color(),
            modal_cursor_bg: self.modal_cursor_bg.into_color(),
        })
    }
}

#[derive(Debug, Clone, Copy)]
enum ThemeColor {
    Rgb(u8, u8, u8),
    Reset,
}

impl ThemeColor {
    fn into_color(self) -> Color {
        match self {
            Self::Rgb(r, g, b) => Color::Rgb(r, g, b),
            Self::Reset => Color::Reset,
        }
    }
}

impl<'de> Deserialize<'de> for ThemeColor {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(untagged)]
        enum ThemeColorRepr {
            Rgb([u8; 3]),
            Text(String),
        }

        let repr = ThemeColorRepr::deserialize(deserializer)?;
        match repr {
            ThemeColorRepr::Rgb([r, g, b]) => Ok(Self::Rgb(r, g, b)),
            ThemeColorRepr::Text(text) => parse_text_color(&text).map_err(serde::de::Error::custom),
        }
    }
}

fn parse_text_color(value: &str) -> anyhow::Result<ThemeColor> {
    let normalized = value.trim().to_ascii_lowercase();
    if normalized == "reset" {
        return Ok(ThemeColor::Reset);
    }
    if let Some(hex) = normalized.strip_prefix('#')
        && hex.len() == 6
    {
        let r = u8::from_str_radix(&hex[0..2], 16)
            .with_context(|| format!("invalid red channel in {value}"))?;
        let g = u8::from_str_radix(&hex[2..4], 16)
            .with_context(|| format!("invalid green channel in {value}"))?;
        let b = u8::from_str_radix(&hex[4..6], 16)
            .with_context(|| format!("invalid blue channel in {value}"))?;
        return Ok(ThemeColor::Rgb(r, g, b));
    }

    bail!("theme color must be \"reset\", #RRGGBB, or [r,g,b], got {value}");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reload_if_changed_loads_valid_theme_file() {
        let temp = tempfile::tempdir().expect("tempdir");
        let config_dir = temp.path().join("hunkr");
        fs::create_dir_all(&config_dir).expect("create config dir");
        let theme_path = config_dir.join(THEME_FILE_NAME);
        fs::write(&theme_path, include_str!("../../theme.example.yaml")).expect("write theme");

        let mut state = ThemeRuntimeState::new(theme_path);
        let outcome = state.reload_if_changed(true).expect("reload");

        assert_eq!(outcome, ThemeReloadOutcome::LoadedFromFile);
        assert_eq!(
            state.for_mode(ThemeMode::Light).cursor_bg,
            Color::Rgb(236, 236, 236),
        );
    }

    #[test]
    fn reload_if_changed_resets_to_defaults_when_theme_file_removed() {
        let temp = tempfile::tempdir().expect("tempdir");
        let config_dir = temp.path().join("hunkr");
        fs::create_dir_all(&config_dir).expect("create config dir");
        let theme_path = config_dir.join(THEME_FILE_NAME);
        fs::write(&theme_path, include_str!("../../theme.example.yaml")).expect("write theme");

        let mut state = ThemeRuntimeState::new(theme_path.clone());
        let first = state.reload_if_changed(true).expect("first reload");
        assert_eq!(first, ThemeReloadOutcome::LoadedFromFile);

        fs::remove_file(&theme_path).expect("remove theme");
        let second = state.reload_if_changed(true).expect("reload after remove");
        assert_eq!(second, ThemeReloadOutcome::ResetToDefaults);
        assert_eq!(
            state.for_mode(ThemeMode::Light).cursor_bg,
            UiTheme::from_mode(ThemeMode::Light).cursor_bg,
        );
    }

    #[test]
    fn parse_color_accepts_reset_and_hex() {
        assert!(matches!(
            parse_text_color("reset").expect("reset"),
            ThemeColor::Reset
        ));
        assert!(matches!(
            parse_text_color("#ffffff").expect("hex"),
            ThemeColor::Rgb(255, 255, 255)
        ));
    }
}
