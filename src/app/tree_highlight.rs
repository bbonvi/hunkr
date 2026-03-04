//! File tree construction and syntax-highlighting cache for diff rendering.
use crate::app::*;
use std::path::Path;

const JS_FALLBACK_TOKENS: &[&str] = &["javascript", "js"];
const TS_FALLBACK_TOKENS: &[&str] = &["typescript", "tsx", "jsx", "javascript", "js"];
const TEMPLATE_FALLBACK_TOKENS: &[&str] = &["vue", "svelte", "astro", "html", "xml", "js"];
const MDX_FALLBACK_TOKENS: &[&str] = &["mdx", "markdown", "md", "jsx", "js"];
const JSON_FALLBACK_TOKENS: &[&str] = &["json", "js"];
const YAML_FALLBACK_TOKENS: &[&str] = &["yaml", "yml"];
const GRAPHQL_FALLBACK_TOKENS: &[&str] = &["graphql", "gql", "js"];
const TERRAFORM_FALLBACK_TOKENS: &[&str] = &["terraform", "hcl", "tf", "cfg", "ini"];
const DOCKER_FALLBACK_TOKENS: &[&str] = &["dockerfile", "docker", "sh", "shell", "bash"];
const MAKE_FALLBACK_TOKENS: &[&str] = &["makefile", "make", "mk", "sh"];
const SHELL_FALLBACK_TOKENS: &[&str] = &["bash", "zsh", "sh", "shell"];
const RUBY_FALLBACK_TOKENS: &[&str] = &["ruby", "rb"];
const IGNORE_FALLBACK_TOKENS: &[&str] = &["gitignore", "ignore", "conf", "cfg", "txt"];
const ENV_FALLBACK_TOKENS: &[&str] = &["dotenv", "sh", "bash", "conf", "ini"];
const CMAKE_FALLBACK_TOKENS: &[&str] = &["cmake", "make"];
const JENKINS_FALLBACK_TOKENS: &[&str] = &["groovy", "java"];

#[derive(Default)]
pub(super) struct FileTree {
    dirs: BTreeMap<String, FileTree>,
    files: BTreeMap<String, FileTreeFile>,
}

#[derive(Debug, Clone)]
struct FileTreeFile {
    modified_ts: i64,
    change: Option<FileChangeSummary>,
}

impl FileTree {
    #[cfg(test)]
    pub(super) fn insert(&mut self, path: &str, modified_ts: i64) {
        self.insert_with_change(path, modified_ts, None);
    }

    pub(super) fn insert_with_change(
        &mut self,
        path: &str,
        modified_ts: i64,
        change: Option<FileChangeSummary>,
    ) {
        let segments: Vec<&str> = path.split('/').collect();
        if segments.is_empty() {
            return;
        }

        let mut cursor = self;
        for segment in &segments[..segments.len().saturating_sub(1)] {
            cursor = cursor.dirs.entry((*segment).to_owned()).or_default();
        }

        if let Some(name) = segments.last() {
            let entry = cursor
                .files
                .entry((*name).to_owned())
                .or_insert_with(|| FileTreeFile {
                    modified_ts,
                    change: change.clone(),
                });
            entry.modified_ts = max(entry.modified_ts, modified_ts);
            if change.is_some() {
                entry.change = change;
            }
        }
    }

    pub(super) fn flattened_rows(
        &self,
        nerd_fonts: bool,
        nerd_font_theme: &NerdFontTheme,
    ) -> Vec<TreeRow> {
        let mut rows = Vec::new();
        self.flatten_into(&mut rows, String::new(), 0, nerd_fonts, nerd_font_theme);
        rows
    }

    fn flatten_into(
        &self,
        rows: &mut Vec<TreeRow>,
        prefix: String,
        depth: usize,
        nerd_fonts: bool,
        nerd_font_theme: &NerdFontTheme,
    ) {
        for (dir, child) in &self.dirs {
            let path = if prefix.is_empty() {
                dir.clone()
            } else {
                format!("{prefix}/{dir}")
            };
            rows.push(TreeRow {
                label: format_tree_dir_label(depth, dir, nerd_fonts, nerd_font_theme),
                path: None,
                depth,
                selectable: false,
                modified_ts: None,
                change: None,
            });
            child.flatten_into(rows, path, depth + 1, nerd_fonts, nerd_font_theme);
        }

        for (file, meta) in &self.files {
            let full = if prefix.is_empty() {
                file.clone()
            } else {
                format!("{prefix}/{file}")
            };
            rows.push(TreeRow {
                label: format_tree_file_label(depth, file, &full, nerd_fonts, nerd_font_theme),
                path: Some(full),
                depth,
                selectable: true,
                modified_ts: Some(meta.modified_ts),
                change: meta.change.clone(),
            });
        }
    }
}

/// Cache key for a single highlighted source line in a specific theme mode.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct DiffHighlightCacheKey {
    mode: ThemeMode,
    path: String,
    line: String,
}

pub(super) struct DiffSyntaxHighlighter {
    syntaxes: SyntaxSet,
    dark_theme: Theme,
    light_theme: Theme,
    highlight_cache: HashMap<DiffHighlightCacheKey, Vec<Span<'static>>>,
    highlight_cache_order: VecDeque<DiffHighlightCacheKey>,
    highlight_cache_capacity: usize,
}

impl DiffSyntaxHighlighter {
    pub(super) fn new() -> Self {
        let syntaxes = SyntaxSet::load_defaults_newlines();
        let theme_set = ThemeSet::load_defaults();
        let dark_theme = theme_set
            .themes
            .get("base16-ocean.dark")
            .cloned()
            .or_else(|| theme_set.themes.values().next().cloned())
            .unwrap_or_default();
        let light_theme = theme_set
            .themes
            .get("InspiredGitHub")
            .cloned()
            .or_else(|| theme_set.themes.values().next().cloned())
            .unwrap_or_default();

        Self {
            syntaxes,
            dark_theme,
            light_theme,
            highlight_cache: HashMap::new(),
            highlight_cache_order: VecDeque::new(),
            highlight_cache_capacity: SYNTAX_HIGHLIGHT_CACHE_CAPACITY,
        }
    }

    fn syntax_for_path(&self, path: &str) -> &SyntaxReference {
        let file_name = Path::new(path)
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or_default();
        if !file_name.is_empty() {
            if let Some(syntax) = self.syntaxes.find_syntax_by_token(file_name) {
                return syntax;
            }
            if let Some(syntax) = self.syntax_for_filename_alias(file_name) {
                return syntax;
            }
        }

        if let Some(ext) = Path::new(path).extension().and_then(|ext| ext.to_str())
            && let Some(syntax) = self.syntax_for_extension(ext)
        {
            return syntax;
        }

        self.syntaxes
            .find_syntax_for_file(path)
            .ok()
            .flatten()
            .unwrap_or_else(|| self.syntaxes.find_syntax_plain_text())
    }

    fn syntax_for_extension(&self, ext: &str) -> Option<&SyntaxReference> {
        self.syntaxes.find_syntax_by_extension(ext).or_else(|| {
            let lower = ext.to_ascii_lowercase();
            self.syntaxes
                .find_syntax_by_extension(&lower)
                .or_else(|| self.syntaxes.find_syntax_by_token(&lower))
                .or_else(|| {
                    extension_alias_tokens(&lower)
                        .and_then(|tokens| self.syntax_for_alias_tokens(tokens))
                })
        })
    }

    fn syntax_for_filename_alias(&self, file_name: &str) -> Option<&SyntaxReference> {
        let lower = file_name.to_ascii_lowercase();
        if lower == ".env" || lower.starts_with(".env.") {
            return self.syntax_for_alias_tokens(ENV_FALLBACK_TOKENS);
        }
        if matches!(
            lower.as_str(),
            ".bashrc"
                | ".bash_profile"
                | ".bash_aliases"
                | ".bash_logout"
                | ".profile"
                | ".zshrc"
                | ".zprofile"
                | ".zshenv"
                | ".zlogin"
                | ".kshrc"
                | ".tcshrc"
        ) {
            return self.syntax_for_alias_tokens(SHELL_FALLBACK_TOKENS);
        }

        let tokens = match lower.as_str() {
            "dockerfile" | "containerfile" => Some(DOCKER_FALLBACK_TOKENS),
            "makefile" | "gnumakefile" | "justfile" => Some(MAKE_FALLBACK_TOKENS),
            "cmakelists.txt" => Some(CMAKE_FALLBACK_TOKENS),
            "jenkinsfile" => Some(JENKINS_FALLBACK_TOKENS),
            "vagrantfile" | "gemfile" | "rakefile" | "podfile" | "fastfile" | "brewfile" => {
                Some(RUBY_FALLBACK_TOKENS)
            }
            ".gitignore" | ".dockerignore" | ".ignore" | ".npmignore" => {
                Some(IGNORE_FALLBACK_TOKENS)
            }
            _ => None,
        }?;
        self.syntax_for_alias_tokens(tokens)
    }

    fn syntax_for_alias_tokens(&self, tokens: &[&str]) -> Option<&SyntaxReference> {
        tokens
            .iter()
            .find_map(|token| self.syntaxes.find_syntax_by_token(token))
    }

    fn theme_for_mode(&self, mode: ThemeMode) -> &Theme {
        match mode {
            ThemeMode::Dark => &self.dark_theme,
            ThemeMode::Light => &self.light_theme,
        }
    }

    pub(super) fn highlight_single_line(
        &mut self,
        mode: ThemeMode,
        path: &str,
        line: &str,
    ) -> Vec<Span<'static>> {
        let cache_key = DiffHighlightCacheKey {
            mode,
            path: path.to_owned(),
            line: line.to_owned(),
        };
        if let Some(cached) = self.highlight_cache.get(&cache_key) {
            return cached.clone();
        }

        let syntax = self.syntax_for_path(path);
        let theme = self.theme_for_mode(mode);
        let mut highlighter = HighlightLines::new(syntax, theme);
        let highlighted = highlighter
            .highlight_line(line, &self.syntaxes)
            .unwrap_or_default();

        let highlighted: Vec<Span<'static>> = highlighted
            .into_iter()
            .map(|(style, text)| Span::styled(text.to_owned(), syntect_to_ratatui(style)))
            .collect();

        if self.highlight_cache_capacity > 0 {
            while self.highlight_cache.len() >= self.highlight_cache_capacity {
                let Some(oldest_key) = self.highlight_cache_order.pop_front() else {
                    break;
                };
                self.highlight_cache.remove(&oldest_key);
            }
            self.highlight_cache_order.push_back(cache_key.clone());
            self.highlight_cache.insert(cache_key, highlighted.clone());
        }

        highlighted
    }
}

fn syntect_to_ratatui(style: syntect::highlighting::Style) -> Style {
    Style::default().fg(Color::Rgb(
        style.foreground.r,
        style.foreground.g,
        style.foreground.b,
    ))
}

fn extension_alias_tokens(ext: &str) -> Option<&'static [&'static str]> {
    Some(match ext {
        "ts" | "tsx" | "jsx" | "mts" | "cts" => TS_FALLBACK_TOKENS,
        "mjs" | "cjs" => JS_FALLBACK_TOKENS,
        "vue" | "svelte" | "astro" => TEMPLATE_FALLBACK_TOKENS,
        "mdx" => MDX_FALLBACK_TOKENS,
        "jsonc" | "json5" => JSON_FALLBACK_TOKENS,
        "yaml" => YAML_FALLBACK_TOKENS,
        "gql" | "graphql" => GRAPHQL_FALLBACK_TOKENS,
        "tf" | "tfvars" | "hcl" => TERRAFORM_FALLBACK_TOKENS,
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::DiffSyntaxHighlighter;

    #[test]
    fn syntax_for_missing_react_path_prefers_extension_lookup() {
        let highlighter = DiffSyntaxHighlighter::new();
        let expected = highlighter
            .syntax_for_alias_tokens(super::TS_FALLBACK_TOKENS)
            .expect("ts/js fallback syntax");
        let actual = highlighter.syntax_for_path("missing/path/component.tsx");
        assert_eq!(actual.name, expected.name);
    }

    #[test]
    fn syntax_for_missing_typescript_path_prefers_extension_lookup() {
        let highlighter = DiffSyntaxHighlighter::new();
        let expected = highlighter
            .syntax_for_alias_tokens(super::TS_FALLBACK_TOKENS)
            .expect("ts/js fallback syntax");
        let actual = highlighter.syntax_for_path("missing/path/service.ts");
        assert_eq!(actual.name, expected.name);
    }

    #[test]
    fn syntax_for_missing_vue_path_uses_template_fallback() {
        let highlighter = DiffSyntaxHighlighter::new();
        let expected = highlighter
            .syntax_for_alias_tokens(super::TEMPLATE_FALLBACK_TOKENS)
            .expect("template fallback syntax");
        let actual = highlighter.syntax_for_path("missing/path/App.vue");
        assert_eq!(actual.name, expected.name);
    }

    #[test]
    fn syntax_for_missing_svelte_path_uses_template_fallback() {
        let highlighter = DiffSyntaxHighlighter::new();
        let expected = highlighter
            .syntax_for_alias_tokens(super::TEMPLATE_FALLBACK_TOKENS)
            .expect("template fallback syntax");
        let actual = highlighter.syntax_for_path("missing/path/Component.svelte");
        assert_eq!(actual.name, expected.name);
    }

    #[test]
    fn syntax_for_dockerfile_name_uses_filename_fallback() {
        let highlighter = DiffSyntaxHighlighter::new();
        let expected = highlighter
            .syntax_for_alias_tokens(super::DOCKER_FALLBACK_TOKENS)
            .expect("docker fallback syntax");
        let actual = highlighter.syntax_for_path("missing/path/Dockerfile");
        assert_eq!(actual.name, expected.name);
    }
}
