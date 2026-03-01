use std::{collections::HashMap, path::Path};

use unicode_width::UnicodeWidthStr;

use crate::model::{FileChangeKind, FileChangeSummary, ReviewStatus};

const DEFAULT_DIRECTORY_ICON: &str = "";
const DEFAULT_FILE_ICON: &str = "󰈔";
const DEFAULT_ENV_ICON: &str = "";
const DEFAULT_DOCKER_ICON: &str = "";

#[derive(Debug, Clone)]
pub(super) struct NerdFontTheme {
    directory_icon: String,
    default_file_icon: String,
    env_icon: String,
    docker_icon: String,
    special_files: HashMap<String, String>,
    extensions: HashMap<String, String>,
}

impl Default for NerdFontTheme {
    fn default() -> Self {
        Self {
            directory_icon: DEFAULT_DIRECTORY_ICON.to_owned(),
            default_file_icon: DEFAULT_FILE_ICON.to_owned(),
            env_icon: DEFAULT_ENV_ICON.to_owned(),
            docker_icon: DEFAULT_DOCKER_ICON.to_owned(),
            special_files: default_special_file_icons(),
            extensions: default_extension_icons(),
        }
    }
}

/// Returns the header title label with optional Nerd Font icon.
pub(super) fn app_title_label(nerd_fonts: bool) -> &'static str {
    if nerd_fonts { "  HUNKR " } else { " HUNKR " }
}

/// Returns commit selection marker for list rows.
pub(super) fn commit_selection_marker(selected: bool, nerd_fonts: bool) -> &'static str {
    match (selected, nerd_fonts) {
        (true, true) => "",
        (false, true) => "",
        (true, false) => "[x]",
        (false, false) => "[ ]",
    }
}

/// Returns list highlight symbol for focused list items.
pub(super) fn list_highlight_symbol(nerd_fonts: bool) -> &'static str {
    if nerd_fonts { "" } else { ">> " }
}

/// Returns the width reserved for list highlight symbols.
pub(super) fn list_highlight_symbol_width(nerd_fonts: bool) -> u16 {
    UnicodeWidthStr::width(list_highlight_symbol(nerd_fonts)) as u16
}

/// Returns the unpushed suffix badge in commit rows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum CommitPushChainMarkerKind {
    Pushed,
    FirstPushed,
    FirstUnpushed,
    TopPushed,
    TopUnpushed,
    Unpushed,
}

/// Returns the commit-chain marker used to visualize pushed/unpushed topology.
pub(super) fn commit_push_chain_marker(
    kind: CommitPushChainMarkerKind,
    nerd_fonts: bool,
) -> &'static str {
    if nerd_fonts {
        return match kind {
            CommitPushChainMarkerKind::Pushed => "󰜘",
            CommitPushChainMarkerKind::FirstPushed => "󰜙",
            CommitPushChainMarkerKind::FirstUnpushed => "󰜚",
            CommitPushChainMarkerKind::TopPushed => "󰜝",
            CommitPushChainMarkerKind::TopUnpushed => "󰜞",
            CommitPushChainMarkerKind::Unpushed => "󰜛",
        };
    }

    match kind {
        CommitPushChainMarkerKind::Pushed => "[=]",
        CommitPushChainMarkerKind::FirstPushed => "[v]",
        CommitPushChainMarkerKind::FirstUnpushed => "[^]",
        CommitPushChainMarkerKind::TopPushed => "[T]",
        CommitPushChainMarkerKind::TopUnpushed => "[!]",
        CommitPushChainMarkerKind::Unpushed => "[+]",
    }
}

/// Returns the draft badge used for uncommitted pseudo-commit rows.
pub(super) fn uncommitted_badge(nerd_fonts: bool) -> &'static str {
    if nerd_fonts {
        "[ DRAFT]"
    } else {
        "[UNCOMMITTED]"
    }
}

/// Formats a file-tree directory label with optional icon.
pub(super) fn format_tree_dir_label(
    depth: usize,
    dir: &str,
    nerd_fonts: bool,
    theme: &NerdFontTheme,
) -> String {
    let indent = "  ".repeat(depth);
    if nerd_fonts {
        format!("{indent}{} {dir}", theme.directory_icon)
    } else {
        format!("{indent}[D] {dir}")
    }
}

/// Formats a file-tree file label with optional file-type icon.
pub(super) fn format_tree_file_label(
    depth: usize,
    file_name: &str,
    full_path: &str,
    nerd_fonts: bool,
    theme: &NerdFontTheme,
) -> String {
    let indent = "  ".repeat(depth);
    if nerd_fonts {
        let icon = nerd_file_icon_for_path(full_path, theme);
        format!("{indent}{icon} {file_name}")
    } else {
        format!("{indent}[F] {file_name}")
    }
}

/// Prepends a file-type icon to file paths when Nerd Fonts are enabled.
pub(super) fn format_path_with_icon(path: &str, nerd_fonts: bool, theme: &NerdFontTheme) -> String {
    if !nerd_fonts {
        return path.to_owned();
    }

    let icon = nerd_file_icon_for_path(path, theme);
    format!("{icon} {path}")
}

/// Formats an idiomatic git file-change badge (kind + +/- stats) for list/header rows.
pub(super) fn format_file_change_badge(change: &FileChangeSummary, nerd_fonts: bool) -> String {
    let kind = file_change_kind_symbol(change.kind, nerd_fonts);
    let mut parts = Vec::new();
    if change.additions > 0 {
        parts.push(format!("+{}", change.additions));
    }
    if change.deletions > 0 {
        parts.push(format!("-{}", change.deletions));
    }
    if parts.is_empty() {
        kind.to_owned()
    } else {
        format!("{kind} {}", parts.join(" "))
    }
}

/// Returns the compact per-status badge shown in commit rows.
pub(super) fn commit_status_badge(status: ReviewStatus, nerd_fonts: bool) -> &'static str {
    if nerd_fonts {
        return match status {
            ReviewStatus::Unreviewed => "",
            ReviewStatus::Reviewed => "",
            ReviewStatus::IssueFound => "",
            ReviewStatus::Resolved => "",
        };
    }
    match status {
        ReviewStatus::Unreviewed => "U",
        ReviewStatus::Reviewed => "R",
        ReviewStatus::IssueFound => "I",
        ReviewStatus::Resolved => "S",
    }
}

pub(super) fn file_change_kind_symbol(kind: FileChangeKind, nerd_fonts: bool) -> &'static str {
    if nerd_fonts {
        return match kind {
            FileChangeKind::Added => "",
            FileChangeKind::Modified => "",
            FileChangeKind::Deleted => "",
            FileChangeKind::Renamed => "󰁕",
            FileChangeKind::Copied => "",
            FileChangeKind::TypeChanged => "󰆩",
            FileChangeKind::Unmerged => "",
            FileChangeKind::Untracked => "",
            FileChangeKind::Unknown => "",
        };
    }
    match kind {
        FileChangeKind::Added => "A",
        FileChangeKind::Modified => "M",
        FileChangeKind::Deleted => "D",
        FileChangeKind::Renamed => "R",
        FileChangeKind::Copied => "C",
        FileChangeKind::TypeChanged => "T",
        FileChangeKind::Unmerged => "U",
        FileChangeKind::Untracked => "?",
        FileChangeKind::Unknown => "X",
    }
}

fn nerd_file_icon_for_path<'a>(path: &str, theme: &'a NerdFontTheme) -> &'a str {
    let file_name = Path::new(path)
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or(path);
    let lower_name = file_name.to_ascii_lowercase();

    if is_env_file_name(&lower_name) {
        return theme.env_icon.as_str();
    }
    if let Some(icon) = example_variant_icon(&lower_name, theme) {
        return icon;
    }
    if is_docker_compose_file_name(&lower_name) || is_dockerfile_name(&lower_name) {
        return theme.docker_icon.as_str();
    }
    if let Some(icon) = theme.special_files.get(lower_name.as_str()) {
        return icon.as_str();
    }

    let extension = Path::new(file_name)
        .extension()
        .and_then(|ext| ext.to_str())
        .map(normalize_extension_key);
    if let Some(icon) = extension
        .as_deref()
        .and_then(|ext| theme.extensions.get(ext))
    {
        return icon.as_str();
    }

    theme.default_file_icon.as_str()
}

fn is_env_file_name(lower_name: &str) -> bool {
    lower_name == ".env" || lower_name.starts_with(".env.")
}

fn example_variant_icon<'a>(lower_name: &str, theme: &'a NerdFontTheme) -> Option<&'a str> {
    let base_name = lower_name.strip_suffix(".example")?;
    if base_name.is_empty() {
        return None;
    }

    if is_env_file_name(base_name) {
        return Some(theme.env_icon.as_str());
    }
    if is_docker_compose_file_name(base_name) || is_dockerfile_name(base_name) {
        return Some(theme.docker_icon.as_str());
    }
    if let Some(icon) = theme.special_files.get(base_name) {
        return Some(icon.as_str());
    }

    let extension = Path::new(base_name)
        .extension()
        .and_then(|ext| ext.to_str())
        .map(normalize_extension_key);
    extension
        .as_deref()
        .and_then(|ext| theme.extensions.get(ext))
        .map(String::as_str)
}

fn is_dockerfile_name(lower_name: &str) -> bool {
    lower_name == "dockerfile"
        || lower_name.starts_with("dockerfile.")
        || lower_name.starts_with("dockerfile-")
}

fn is_docker_compose_file_name(lower_name: &str) -> bool {
    matches!(lower_name, "compose.yml" | "compose.yaml")
        || lower_name.starts_with("docker-compose.")
}

fn normalize_extension_key(value: &str) -> String {
    value.trim().trim_start_matches('.').to_ascii_lowercase()
}

fn default_special_file_icons() -> HashMap<String, String> {
    let entries = [
        (".gitignore", ""),
        (".gitattributes", ""),
        (".gitmodules", ""),
        (".dockerignore", DEFAULT_DOCKER_ICON),
        ("makefile", ""),
        ("readme", ""),
        ("readme.md", ""),
        ("readme.txt", ""),
        ("license", ""),
        ("copying", ""),
    ];
    entries
        .into_iter()
        .map(|(name, icon)| (name.to_owned(), icon.to_owned()))
        .collect()
}

fn default_extension_icons() -> HashMap<String, String> {
    let entries = [
        ("rs", ""),
        ("c", ""),
        ("h", ""),
        ("cc", ""),
        ("cpp", ""),
        ("cxx", ""),
        ("hpp", ""),
        ("hh", ""),
        ("hxx", ""),
        ("cs", "󰌛"),
        ("go", ""),
        ("java", ""),
        ("kt", ""),
        ("kts", ""),
        ("php", ""),
        ("py", ""),
        ("rb", ""),
        ("swift", ""),
        ("js", ""),
        ("mjs", ""),
        ("cjs", ""),
        ("jsx", ""),
        ("tsx", ""),
        ("ts", ""),
        ("vue", "󰡄"),
        ("svelte", ""),
        ("html", ""),
        ("htm", ""),
        ("css", ""),
        ("scss", ""),
        ("sass", ""),
        ("less", ""),
        ("json", ""),
        ("toml", ""),
        ("yaml", ""),
        ("yml", ""),
        ("ini", ""),
        ("cfg", ""),
        ("conf", ""),
        ("xml", "󰗀"),
        ("sql", ""),
        ("md", ""),
        ("markdown", ""),
        ("sh", ""),
        ("bash", ""),
        ("zsh", ""),
        ("fish", ""),
        ("diff", ""),
        ("patch", ""),
        ("env", DEFAULT_ENV_ICON),
        ("git", ""),
        ("lock", "󰌾"),
        ("lockb", "󰌾"),
        ("pem", "󰌆"),
        ("crt", "󰌆"),
        ("key", "󰌆"),
        ("pub", "󰌆"),
        ("asc", "󰷃"),
        ("sig", "󰷃"),
        ("pdf", ""),
        ("doc", "󰈬"),
        ("docx", "󰈬"),
        ("xls", "󱎏"),
        ("xlsx", "󱎏"),
        ("csv", "󱎏"),
        ("tsv", "󱎏"),
        ("ppt", "󰈧"),
        ("pptx", "󰈧"),
        ("log", ""),
        ("bak", "󰁯"),
        ("zip", ""),
        ("tar", ""),
        ("gz", ""),
        ("bz2", ""),
        ("xz", ""),
        ("7z", ""),
        ("rar", ""),
        ("zst", ""),
        ("svg", "󰜡"),
        ("png", "󰈟"),
        ("jpg", "󰈟"),
        ("jpeg", "󰈟"),
        ("gif", "󰈟"),
        ("webp", "󰈟"),
        ("bmp", "󰈟"),
        ("ico", "󰈟"),
        ("tiff", "󰈟"),
        ("mp3", "󰎆"),
        ("wav", "󰎆"),
        ("flac", "󰎆"),
        ("ogg", "󰎆"),
        ("m4a", "󰎆"),
        ("aac", "󰎆"),
        ("mp4", "󰕧"),
        ("mov", "󰕧"),
        ("mkv", "󰕧"),
        ("avi", "󰕧"),
        ("webm", "󰕧"),
        ("ttf", ""),
        ("otf", ""),
        ("woff", ""),
        ("woff2", ""),
        ("wasm", ""),
        ("proto", "󱘦"),
        ("graphql", "󰡷"),
        ("gql", "󰡷"),
        ("tf", ""),
        ("tfvars", ""),
        ("nix", ""),
        ("lua", ""),
        ("r", "󰟔"),
        ("dart", ""),
        ("elm", ""),
        ("ex", ""),
        ("exs", ""),
        ("erl", ""),
        ("hrl", ""),
        ("clj", ""),
        ("cljs", ""),
        ("cljc", ""),
        ("edn", ""),
        ("scala", ""),
        ("zig", ""),
        ("pl", ""),
        ("pm", ""),
        ("ps1", "󰨊"),
        ("txt", "󰈙"),
    ];

    entries
        .into_iter()
        .map(|(extension, icon)| (extension.to_owned(), icon.to_owned()))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ascii_mode_keeps_path_unchanged() {
        let theme = NerdFontTheme::default();
        assert_eq!(
            format_path_with_icon("src/app.rs", false, &theme),
            "src/app.rs"
        );
    }

    #[test]
    fn nerd_mode_prefixes_icon_and_preserves_path() {
        let theme = NerdFontTheme::default();
        let rendered = format_path_with_icon("src/app.rs", true, &theme);
        assert_ne!(rendered, "src/app.rs");
        assert!(rendered.ends_with("src/app.rs"));
        assert!(rendered.contains(' '));
    }
}
