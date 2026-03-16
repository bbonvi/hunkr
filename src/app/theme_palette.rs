//! Runtime theme palette loading and config-directory live-reload tracking.
use std::{
    collections::BTreeSet,
    fs,
    path::{Path, PathBuf},
    time::SystemTime,
};

use anyhow::{Context, bail};
use ratatui::style::Color;
use serde::Deserialize;
use serde_yaml::{Mapping, Value};

use super::{ThemeMode, UiTheme};

pub(super) const THEME_FILE_NAME: &str = "theme.yaml";

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum ThemeReloadOutcome {
    Unchanged,
    LoadedFromFile { warnings: Vec<String> },
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

#[derive(Debug, Clone)]
struct ThemeLoadResult {
    catalog: ThemeCatalog,
    warnings: Vec<String>,
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
        let ThemeLoadResult { catalog, warnings } = ThemeCatalog::load_from_path(&self.theme_path)?;
        self.catalog = catalog;
        Ok(ThemeReloadOutcome::LoadedFromFile { warnings })
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

    fn load_from_path(path: &Path) -> anyhow::Result<ThemeLoadResult> {
        let raw = fs::read_to_string(path)
            .with_context(|| format!("failed to read theme file {}", path.display()))?;
        let parsed: Value = serde_yaml::from_str(&raw)
            .with_context(|| format!("failed to parse theme yaml {}", path.display()))?;
        let (catalog, warnings) = Self::from_yaml_value(parsed);
        Ok(ThemeLoadResult { catalog, warnings })
    }

    fn from_yaml_value(value: Value) -> (Self, Vec<String>) {
        let ThemeCatalog {
            dark: default_dark,
            light: default_light,
        } = Self::defaults();
        let mut warnings = Vec::new();
        let Some(root) = value.as_mapping() else {
            warnings.push(
                "theme: expected a mapping at the YAML root; using built-in defaults".to_owned(),
            );
            return (
                Self {
                    dark: default_dark,
                    light: default_light,
                },
                warnings,
            );
        };

        let unknown_sections = collect_unknown_fields(root, &["dark", "light"]);
        if !unknown_sections.is_empty() {
            warnings.push(format!(
                "theme: ignored unknown sections: {}",
                unknown_sections.join(", ")
            ));
        }

        let dark = load_theme_mode(
            "dark",
            mapping_value(root, "dark"),
            default_dark,
            &mut warnings,
        );
        let light = load_theme_mode(
            "light",
            mapping_value(root, "light"),
            default_light,
            &mut warnings,
        );
        (Self { dark, light }, warnings)
    }
}

const THEME_MODE_FIELDS: &[&str] = &[
    "border",
    "focus_border",
    "accent",
    "panel_title_bg",
    "panel_title_fg",
    "footer_chip_bg",
    "text",
    "muted",
    "dimmed",
    "cursor_bg",
    "focused_cursor_bg",
    "cursor_visual_overlap_weight",
    "block_cursor_fg",
    "block_cursor_bg",
    "visual_bg",
    "commit_selected_bg",
    "commit_selected_text",
    "search_match_fg",
    "search_match_bg",
    "search_current_fg",
    "search_current_bg",
    "reviewed",
    "unreviewed",
    "issue",
    "pushed",
    "unpushed",
    "diff_add",
    "diff_add_bg",
    "diff_remove",
    "diff_remove_bg",
    "diff_meta",
    "diff_header",
    "dir",
    "modal_bg",
    "modal_editor_bg",
    "modal_cursor_fg",
    "modal_cursor_bg",
];

fn load_theme_mode(
    mode_name: &str,
    raw_mode: Option<&Value>,
    mut theme: UiTheme,
    warnings: &mut Vec<String>,
) -> UiTheme {
    let Some(raw_mode) = raw_mode else {
        warnings.push(format!(
            "{mode_name}: missing section; using built-in defaults"
        ));
        return theme;
    };
    let Some(mapping) = raw_mode.as_mapping() else {
        warnings.push(format!(
            "{mode_name}: expected a mapping, got {}; using built-in defaults",
            yaml_value_kind(raw_mode)
        ));
        return theme;
    };

    let mut loader = ThemeModeLoader::new(mode_name, mapping, warnings);
    loader.color("border", &mut theme.border);
    loader.color("focus_border", &mut theme.focus_border);
    loader.color("accent", &mut theme.accent);
    loader.color("panel_title_bg", &mut theme.panel_title_bg);
    loader.color("panel_title_fg", &mut theme.panel_title_fg);
    loader.color("footer_chip_bg", &mut theme.footer_chip_bg);
    loader.color("text", &mut theme.text);
    loader.color("muted", &mut theme.muted);
    loader.color("dimmed", &mut theme.dimmed);
    loader.color("cursor_bg", &mut theme.cursor_bg);
    loader.color("focused_cursor_bg", &mut theme.focused_cursor_bg);
    loader.nonzero_u8(
        "cursor_visual_overlap_weight",
        &mut theme.cursor_visual_overlap_weight,
        "cursor_visual_overlap_weight must be between 1 and 255",
    );
    loader.color("block_cursor_fg", &mut theme.block_cursor_fg);
    loader.color("block_cursor_bg", &mut theme.block_cursor_bg);
    loader.color("visual_bg", &mut theme.visual_bg);
    loader.color("commit_selected_bg", &mut theme.commit_selected_bg);
    let commit_selected_text_present =
        loader.color("commit_selected_text", &mut theme.commit_selected_text);
    loader.color("search_match_fg", &mut theme.search_match_fg);
    loader.color("search_match_bg", &mut theme.search_match_bg);
    loader.color("search_current_fg", &mut theme.search_current_fg);
    loader.color("search_current_bg", &mut theme.search_current_bg);
    loader.color("reviewed", &mut theme.reviewed);
    loader.color("unreviewed", &mut theme.unreviewed);
    loader.color("issue", &mut theme.issue);
    loader.color("pushed", &mut theme.pushed);
    loader.color("unpushed", &mut theme.unpushed);
    loader.color("diff_add", &mut theme.diff_add);
    loader.color("diff_add_bg", &mut theme.diff_add_bg);
    loader.color("diff_remove", &mut theme.diff_remove);
    loader.color("diff_remove_bg", &mut theme.diff_remove_bg);
    loader.color("diff_meta", &mut theme.diff_meta);
    loader.color("diff_header", &mut theme.diff_header);
    loader.color("dir", &mut theme.dir);
    loader.color("modal_bg", &mut theme.modal_bg);
    loader.color("modal_editor_bg", &mut theme.modal_editor_bg);
    loader.color("modal_cursor_fg", &mut theme.modal_cursor_fg);
    loader.color("modal_cursor_bg", &mut theme.modal_cursor_bg);
    loader.finish();

    if !commit_selected_text_present {
        theme.commit_selected_text = theme.accent;
    }

    theme
}

struct ThemeModeLoader<'a> {
    mode_name: &'a str,
    mapping: &'a Mapping,
    seen_fields: BTreeSet<&'static str>,
    warnings: &'a mut Vec<String>,
}

impl<'a> ThemeModeLoader<'a> {
    fn new(mode_name: &'a str, mapping: &'a Mapping, warnings: &'a mut Vec<String>) -> Self {
        Self {
            mode_name,
            mapping,
            seen_fields: BTreeSet::new(),
            warnings,
        }
    }

    fn color(&mut self, field: &'static str, slot: &mut Color) -> bool {
        let Some(value) = self.field_value(field) else {
            return false;
        };
        match serde_yaml::from_value::<ThemeColor>(value) {
            Ok(color) => *slot = color.into_color(),
            Err(err) => self.warn_invalid(field, err),
        }
        true
    }

    fn nonzero_u8(&mut self, field: &'static str, slot: &mut u8, rule: &str) -> bool {
        let Some(value) = self.field_value(field) else {
            return false;
        };
        match serde_yaml::from_value::<u8>(value) {
            Ok(parsed) if parsed > 0 => *slot = parsed,
            Ok(_) => self.warn_invalid(field, rule.to_owned()),
            Err(err) => self.warn_invalid(field, err),
        }
        true
    }

    fn finish(self) {
        let missing_fields: Vec<_> = THEME_MODE_FIELDS
            .iter()
            .copied()
            .filter(|field| !self.seen_fields.contains(field))
            .collect();
        if !missing_fields.is_empty() {
            self.warnings.push(format!(
                "{}: missing fields defaulted: {}",
                self.mode_name,
                missing_fields.join(", ")
            ));
        }

        let unknown_fields = collect_unknown_fields(self.mapping, THEME_MODE_FIELDS);
        if !unknown_fields.is_empty() {
            self.warnings.push(format!(
                "{}: ignored unknown fields: {}",
                self.mode_name,
                unknown_fields.join(", ")
            ));
        }
    }

    fn field_value(&mut self, field: &'static str) -> Option<Value> {
        let value = mapping_value(self.mapping, field)?.clone();
        self.seen_fields.insert(field);
        Some(value)
    }

    fn warn_invalid(&mut self, field: &str, error: impl std::fmt::Display) {
        self.warnings.push(format!(
            "{}.{field}: {error}; using built-in default",
            self.mode_name
        ));
    }
}

fn mapping_value<'a>(mapping: &'a Mapping, field: &str) -> Option<&'a Value> {
    mapping.get(Value::String(field.to_owned()))
}

fn collect_unknown_fields(mapping: &Mapping, known_fields: &[&str]) -> Vec<String> {
    let mut unknown_fields = Vec::new();
    for key in mapping.keys() {
        match key.as_str() {
            Some(field) if !known_fields.contains(&field) => unknown_fields.push(field.to_owned()),
            Some(_) => {}
            None => unknown_fields.push(format!("{key:?}")),
        }
    }
    unknown_fields.sort();
    unknown_fields
}

fn yaml_value_kind(value: &Value) -> &'static str {
    match value {
        Value::Null => "null",
        Value::Bool(_) => "bool",
        Value::Number(_) => "number",
        Value::String(_) => "string",
        Value::Sequence(_) => "sequence",
        Value::Mapping(_) => "mapping",
        Value::Tagged(_) => "tagged value",
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

        assert_loaded_without_warnings(&outcome);
        assert_eq!(
            state.for_mode(ThemeMode::Light).cursor_bg,
            Color::Rgb(226, 226, 226),
        );
        assert_eq!(
            state.for_mode(ThemeMode::Light).commit_selected_text,
            Color::Rgb(0, 123, 184),
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
        assert_loaded_without_warnings(&first);

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

    #[test]
    fn reload_if_changed_defaults_selected_commit_text_to_accent_when_missing() {
        let temp = tempfile::tempdir().expect("tempdir");
        let config_dir = temp.path().join("hunkr");
        fs::create_dir_all(&config_dir).expect("create config dir");
        let theme_path = config_dir.join(THEME_FILE_NAME);
        let legacy_theme = include_str!("../../theme.example.yaml")
            .replace("  commit_selected_text: \"#d3e9ff\"\n", "")
            .replace("  commit_selected_text: \"#123f5e\"\n", "");
        fs::write(&theme_path, legacy_theme).expect("write theme");

        let mut state = ThemeRuntimeState::new(theme_path);
        let outcome = state.reload_if_changed(true).expect("reload");

        assert!(matches!(
            outcome,
            ThemeReloadOutcome::LoadedFromFile { warnings } if warnings.len() == 2
        ));
        assert_eq!(
            state.for_mode(ThemeMode::Dark).commit_selected_text,
            state.for_mode(ThemeMode::Dark).accent,
        );
        assert_eq!(
            state.for_mode(ThemeMode::Light).commit_selected_text,
            state.for_mode(ThemeMode::Light).accent,
        );
    }

    #[test]
    fn reload_if_changed_loads_explicit_push_state_colors() {
        let temp = tempfile::tempdir().expect("tempdir");
        let config_dir = temp.path().join("hunkr");
        fs::create_dir_all(&config_dir).expect("create config dir");
        let theme_path = config_dir.join(THEME_FILE_NAME);
        fs::write(&theme_path, include_str!("../../theme.example.yaml")).expect("write theme");

        let mut state = ThemeRuntimeState::new(theme_path);
        let outcome = state.reload_if_changed(true).expect("reload");

        assert_loaded_without_warnings(&outcome);
        assert_eq!(
            state.for_mode(ThemeMode::Dark).pushed,
            Color::Rgb(122, 176, 202),
        );
        assert_eq!(
            state.for_mode(ThemeMode::Light).pushed,
            Color::Rgb(44, 113, 131),
        );
        assert_eq!(
            state.for_mode(ThemeMode::Dark).unpushed,
            Color::Rgb(170, 170, 170),
        );
        assert_eq!(
            state.for_mode(ThemeMode::Light).unpushed,
            Color::Rgb(165, 165, 165),
        );
    }

    fn assert_loaded_without_warnings(outcome: &ThemeReloadOutcome) {
        assert!(matches!(
            outcome,
            ThemeReloadOutcome::LoadedFromFile { warnings } if warnings.is_empty()
        ));
    }
}
