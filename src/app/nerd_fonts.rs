use std::path::Path;

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
    list_highlight_symbol(nerd_fonts).chars().count() as u16
}

/// Returns the unpushed suffix badge in commit rows.
pub(super) fn unpushed_marker(nerd_fonts: bool) -> &'static str {
    if nerd_fonts { " " } else { " [^]" }
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
pub(super) fn format_tree_dir_label(depth: usize, dir: &str, nerd_fonts: bool) -> String {
    let indent = "  ".repeat(depth);
    if nerd_fonts {
        format!("{indent} {dir}")
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
) -> String {
    let indent = "  ".repeat(depth);
    if nerd_fonts {
        let icon = nerd_file_icon_for_path(full_path);
        format!("{indent}{icon} {file_name}")
    } else {
        format!("{indent}[F] {file_name}")
    }
}

/// Prepends a file-type icon to file paths when Nerd Fonts are enabled.
pub(super) fn format_path_with_icon(path: &str, nerd_fonts: bool) -> String {
    if !nerd_fonts {
        return path.to_owned();
    }

    let icon = nerd_file_icon_for_path(path);
    format!("{icon} {path}")
}

fn nerd_file_icon_for_path(path: &str) -> &'static str {
    let file_name = Path::new(path)
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or(path);
    let lower_name = file_name.to_ascii_lowercase();

    if is_env_file_name(&lower_name) {
        return "";
    }

    if let Some(icon) = example_variant_icon(&lower_name) {
        return icon;
    }

    if let Some(icon) = special_file_icon(&lower_name) {
        return icon;
    }

    let extension = Path::new(file_name)
        .extension()
        .and_then(|ext| ext.to_str())
        .map(|ext| ext.to_ascii_lowercase());
    if let Some(icon) = extension.as_deref().and_then(file_extension_icon) {
        return icon;
    }

    "󰈔"
}

fn is_env_file_name(lower_name: &str) -> bool {
    lower_name == ".env" || lower_name.starts_with(".env.")
}

fn example_variant_icon(lower_name: &str) -> Option<&'static str> {
    let base_name = lower_name.strip_suffix(".example")?;
    if base_name.is_empty() {
        return None;
    }

    if is_env_file_name(base_name) {
        return Some("");
    }
    if let Some(icon) = special_file_icon(base_name) {
        return Some(icon);
    }

    Path::new(base_name)
        .extension()
        .and_then(|ext| ext.to_str())
        .and_then(file_extension_icon)
}

fn special_file_icon(lower_name: &str) -> Option<&'static str> {
    match lower_name {
        ".gitignore" | ".gitattributes" | ".gitmodules" => Some(""),
        ".dockerignore" => Some(""),
        "dockerfile" => Some(""),
        "makefile" => Some(""),
        "readme" | "readme.md" | "readme.txt" => Some(""),
        "license" | "copying" => Some(""),
        _ => None,
    }
}

fn file_extension_icon(ext: &str) -> Option<&'static str> {
    match ext {
        "rs" => Some(""),
        "c" | "h" => Some(""),
        "cc" | "cpp" | "cxx" | "hpp" | "hh" | "hxx" => Some(""),
        "cs" => Some("󰌛"),
        "go" => Some(""),
        "java" => Some(""),
        "kt" | "kts" => Some(""),
        "php" => Some(""),
        "py" => Some(""),
        "rb" => Some(""),
        "swift" => Some(""),
        "js" | "mjs" | "cjs" => Some(""),
        "jsx" | "tsx" => Some(""),
        "ts" => Some(""),
        "vue" => Some("󰡄"),
        "svelte" => Some(""),
        "html" | "htm" => Some(""),
        "css" | "scss" | "sass" | "less" => Some(""),
        "json" => Some(""),
        "toml" | "yaml" | "yml" | "ini" | "cfg" | "conf" => Some(""),
        "xml" => Some("󰗀"),
        "sql" => Some(""),
        "md" | "markdown" => Some(""),
        "sh" | "bash" | "zsh" | "fish" => Some(""),
        "diff" | "patch" => Some(""),
        "git" => Some(""),
        "lockb" => Some("󰌾"),
        "pem" | "crt" | "key" | "pub" => Some("󰌆"),
        "asc" | "sig" => Some("󰷃"),
        "pdf" => Some(""),
        "doc" | "docx" => Some("󰈬"),
        "xls" | "xlsx" | "csv" | "tsv" => Some("󱎏"),
        "ppt" | "pptx" => Some("󰈧"),
        "log" => Some(""),
        "bak" => Some("󰁯"),
        "zip" | "tar" | "gz" | "bz2" | "xz" | "7z" | "rar" | "zst" => Some(""),
        "svg" => Some("󰜡"),
        "png" | "jpg" | "jpeg" | "gif" | "webp" | "bmp" | "ico" | "tiff" => Some("󰈟"),
        "mp3" | "wav" | "flac" | "ogg" | "m4a" | "aac" => Some("󰎆"),
        "mp4" | "mov" | "mkv" | "avi" | "webm" => Some("󰕧"),
        "ttf" | "otf" | "woff" | "woff2" => Some(""),
        "wasm" => Some(""),
        "proto" => Some("󱘦"),
        "graphql" | "gql" => Some("󰡷"),
        "tf" | "tfvars" => Some(""),
        "nix" => Some(""),
        "lua" => Some(""),
        "r" => Some("󰟔"),
        "dart" => Some(""),
        "elm" => Some(""),
        "ex" | "exs" => Some(""),
        "erl" | "hrl" => Some(""),
        "clj" | "cljs" | "cljc" | "edn" => Some(""),
        "scala" => Some(""),
        "zig" => Some(""),
        "pl" | "pm" => Some(""),
        "ps1" => Some("󰨊"),
        "lock" => Some("󰌾"),
        "txt" => Some("󰈙"),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ascii_mode_uses_plain_labels() {
        assert_eq!(format_path_with_icon("src/app.rs", false), "src/app.rs");
        assert_eq!(format_tree_dir_label(1, "src", false), "  [D] src");
        assert_eq!(
            format_tree_file_label(1, "app.rs", "src/app.rs", false),
            "  [F] app.rs"
        );
    }

    #[test]
    fn nerd_mode_uses_language_specific_file_icons() {
        assert_eq!(format_path_with_icon("src/app.rs", true), " src/app.rs");
        assert_eq!(
            format_tree_file_label(0, "README.md", "README.md", true),
            " README.md"
        );
    }

    #[test]
    fn cargo_lock_uses_lock_icon_without_special_case() {
        assert_eq!(format_path_with_icon("Cargo.lock", true), "󰌾 Cargo.lock");
    }

    #[test]
    fn env_variants_map_to_env_icon() {
        assert_eq!(format_path_with_icon(".env", true), " .env");
        assert_eq!(format_path_with_icon(".env.dev", true), " .env.dev");
        assert_eq!(
            format_path_with_icon(".env.example", true),
            " .env.example"
        );
    }

    #[test]
    fn example_variants_inherit_base_file_type_icon() {
        assert_eq!(
            format_path_with_icon("config.yaml.example", true),
            " config.yaml.example"
        );
        assert_eq!(
            format_path_with_icon("Dockerfile.example", true),
            " Dockerfile.example"
        );
    }
}
