use std::{
    cmp::{max, min},
    collections::{BTreeMap, BTreeSet, HashMap, VecDeque},
    fs,
    path::Path,
    sync::Arc,
    time::{Duration, Instant},
};

use anyhow::Context;
use chrono::{DateTime, Utc};
use crossterm::event::{
    Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers, MouseButton, MouseEventKind,
};
use ratatui::{
    Frame,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, BorderType, Borders, Clear, ListState, Paragraph},
};
use syntect::{
    easy::HighlightLines,
    highlighting::Theme,
    highlighting::ThemeSet,
    parsing::{SyntaxReference, SyntaxSet},
};
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

mod lifecycle_render;
mod navigation;
mod nerd_fonts;
mod state;
mod ui;
use self::nerd_fonts::{
    NerdFontTheme, app_title_label, commit_selection_marker, format_path_with_icon,
    format_tree_dir_label, format_tree_file_label, list_highlight_symbol,
    list_highlight_symbol_width, uncommitted_badge, unpushed_marker,
};
use self::ui::diff_pane::{
    DiffPaneBody, DiffPaneRenderer, DiffPaneTitle, PendingDiffViewAnchor,
    capture_pending_diff_view_anchor, find_diff_match_from_cursor, find_index_for_line_locator,
    is_hunk_header_line,
};
use self::ui::list_panes::{CommitPaneModel, FilePaneModel, ListPaneRenderer};

use crate::{
    comments::CommentStore,
    config::StartupTheme,
    git_data::GitService,
    model::{
        AggregatedDiff, CommentAnchor, CommentTarget, CommentTargetKind, CommitInfo, DiffLineKind,
        FilePatch, HunkLine, ReviewComment, ReviewState, ReviewStatus, UNCOMMITTED_COMMIT_ID,
        UNCOMMITTED_COMMIT_SHORT, UNCOMMITTED_COMMIT_SUMMARY,
    },
    store::StateStore,
};

const HISTORY_LIMIT: usize = 400;
const AUTO_REFRESH_EVERY: Duration = Duration::from_secs(4);
const RELATIVE_TIME_REDRAW_EVERY: Duration = Duration::from_secs(30);
const SELECTION_REBUILD_DEBOUNCE: Duration = Duration::from_millis(120);
const LIST_DRAG_EDGE_MARGIN: u16 = 1;
const COMMIT_ANCHOR_HEADER: &str = "__COMMIT__";
const SYNTAX_HIGHLIGHT_CACHE_CAPACITY: usize = 8_192;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FocusPane {
    Files,
    Commits,
    Diff,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum InputMode {
    Normal,
    CommentCreate,
    CommentEdit(u64),
    DiffSearch,
    ListSearch(FocusPane),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum OnboardingStep {
    ConsentProjectDataDir,
    GitignoreChoice,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum ThemeMode {
    Dark,
    Light,
}

impl ThemeMode {
    fn from_startup_theme(theme: StartupTheme) -> Self {
        match theme {
            StartupTheme::Dark => Self::Dark,
            StartupTheme::Light => Self::Light,
        }
    }

    fn toggle(self) -> Self {
        match self {
            Self::Dark => Self::Light,
            Self::Light => Self::Dark,
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::Dark => "dark",
            Self::Light => "light",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CommitStatusFilter {
    All,
    UnreviewedOrIssueFound,
    ReviewedOrResolved,
}

impl CommitStatusFilter {
    fn next(self) -> Self {
        match self {
            Self::All => Self::UnreviewedOrIssueFound,
            Self::UnreviewedOrIssueFound => Self::ReviewedOrResolved,
            Self::ReviewedOrResolved => Self::All,
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::All => "all",
            Self::UnreviewedOrIssueFound => "unreviewed|issue_found",
            Self::ReviewedOrResolved => "reviewed|resolved",
        }
    }

    fn matches_row(self, row: &CommitRow) -> bool {
        match self {
            Self::All => true,
            Self::UnreviewedOrIssueFound => {
                row.is_uncommitted
                    || matches!(
                        row.status,
                        ReviewStatus::Unreviewed | ReviewStatus::IssueFound
                    )
            }
            Self::ReviewedOrResolved => {
                !row.is_uncommitted
                    && matches!(row.status, ReviewStatus::Reviewed | ReviewStatus::Resolved)
            }
        }
    }
}

#[derive(Debug, Clone)]
struct UiTheme {
    border: Color,
    focus_border: Color,
    accent: Color,
    panel_title_bg: Color,
    panel_title_fg: Color,
    text: Color,
    muted: Color,
    dimmed: Color,
    cursor_bg: Color,
    visual_bg: Color,
    reviewed: Color,
    unreviewed: Color,
    issue: Color,
    resolved: Color,
    unpushed: Color,
    diff_add: Color,
    diff_add_bg: Color,
    diff_remove: Color,
    diff_remove_bg: Color,
    diff_meta: Color,
    diff_header: Color,
    dir: Color,
    modal_bg: Color,
    modal_editor_bg: Color,
    modal_cursor_fg: Color,
    modal_cursor_bg: Color,
}

impl UiTheme {
    fn from_mode(mode: ThemeMode) -> Self {
        match mode {
            ThemeMode::Dark => Self {
                border: Color::Rgb(68, 68, 68),
                focus_border: Color::Rgb(221, 189, 40),
                accent: Color::Rgb(120, 196, 255),
                panel_title_bg: Color::Rgb(32, 32, 38),
                panel_title_fg: Color::Rgb(219, 219, 219),
                text: Color::Rgb(228, 228, 228),
                muted: Color::Rgb(170, 170, 170),
                dimmed: Color::Rgb(115, 115, 115),
                cursor_bg: Color::Rgb(52, 52, 62),
                visual_bg: Color::Rgb(57, 67, 93),
                reviewed: Color::Rgb(85, 190, 120),
                unreviewed: Color::Rgb(236, 92, 92),
                issue: Color::Rgb(238, 184, 64),
                resolved: Color::Rgb(84, 178, 209),
                unpushed: Color::Rgb(87, 181, 227),
                diff_add: Color::Rgb(123, 214, 144),
                diff_add_bg: Color::Rgb(19, 51, 30),
                diff_remove: Color::Rgb(240, 124, 124),
                diff_remove_bg: Color::Rgb(59, 23, 23),
                diff_meta: Color::Rgb(235, 199, 86),
                diff_header: Color::Rgb(101, 188, 227),
                dir: Color::Rgb(150, 170, 230),
                modal_bg: Color::Rgb(18, 19, 26),
                modal_editor_bg: Color::Rgb(28, 30, 40),
                modal_cursor_fg: Color::Rgb(245, 245, 245),
                modal_cursor_bg: Color::Rgb(95, 128, 255),
            },
            ThemeMode::Light => Self {
                border: Color::Rgb(195, 195, 195),
                focus_border: Color::Rgb(169, 120, 0),
                accent: Color::Rgb(0, 123, 184),
                panel_title_bg: Color::Rgb(241, 241, 241),
                panel_title_fg: Color::Rgb(52, 52, 52),
                text: Color::Rgb(40, 40, 40),
                muted: Color::Rgb(90, 90, 90),
                dimmed: Color::Rgb(140, 140, 140),
                cursor_bg: Color::Rgb(226, 226, 226),
                visual_bg: Color::Rgb(215, 225, 241),
                reviewed: Color::Rgb(36, 141, 74),
                unreviewed: Color::Rgb(194, 48, 48),
                issue: Color::Rgb(170, 113, 0),
                resolved: Color::Rgb(0, 122, 151),
                unpushed: Color::Rgb(10, 131, 163),
                diff_add: Color::Rgb(16, 127, 33),
                diff_add_bg: Color::Rgb(230, 248, 233),
                diff_remove: Color::Rgb(168, 42, 42),
                diff_remove_bg: Color::Rgb(253, 235, 235),
                diff_meta: Color::Rgb(145, 94, 0),
                diff_header: Color::Rgb(0, 111, 151),
                dir: Color::Rgb(80, 99, 172),
                modal_bg: Color::Rgb(248, 249, 253),
                modal_editor_bg: Color::Rgb(238, 241, 248),
                modal_cursor_fg: Color::Rgb(255, 255, 255),
                modal_cursor_bg: Color::Rgb(41, 94, 214),
            },
        }
    }
}

#[derive(Debug, Clone)]
struct CommitRow {
    info: CommitInfo,
    selected: bool,
    status: ReviewStatus,
    is_uncommitted: bool,
}

#[derive(Debug, Clone)]
struct TreeRow {
    label: String,
    path: Option<String>,
    depth: usize,
    selectable: bool,
    modified_ts: Option<i64>,
}

#[derive(Debug, Clone, Copy, Default)]
struct DiffPosition {
    scroll: usize,
    cursor: usize,
}

#[derive(Debug, Clone, Default)]
struct PaneRects {
    files: ratatui::layout::Rect,
    commits: ratatui::layout::Rect,
    diff: ratatui::layout::Rect,
}

#[derive(Debug, Clone)]
struct RenderedDiffLine {
    line: Line<'static>,
    raw_text: String,
    anchor: Option<CommentAnchor>,
    comment_id: Option<u64>,
}

#[derive(Debug, Clone)]
struct FileDiffRange {
    path: String,
    start: usize,
    end: usize,
}

#[derive(Debug, Clone, Copy)]
struct DiffVisualSelection {
    anchor: usize,
    origin: DiffVisualOrigin,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DiffVisualOrigin {
    Keyboard,
    Mouse,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DiffPendingOp {
    Z,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CommitMouseSelectionMode {
    Replace,
    Toggle,
    Range,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum GitignoreUpdate {
    Added,
    AlreadyPresent,
}

/// High-level app state and interaction flow for the hunkr UI.
pub struct App {
    git: GitService,
    store: StateStore,
    comments: CommentStore,
    review_state: ReviewState,
    commits: Vec<CommitRow>,
    commit_list_state: ListState,
    file_rows: Vec<TreeRow>,
    file_list_state: ListState,
    focused: FocusPane,
    input_mode: InputMode,
    theme_mode: ThemeMode,
    diff_wheel_scroll_lines: isize,
    list_wheel_coalesce: Duration,
    nerd_fonts: bool,
    nerd_font_theme: NerdFontTheme,
    commit_visual_anchor: Option<usize>,
    commit_selection_anchor: Option<usize>,
    commit_mouse_anchor: Option<usize>,
    commit_mouse_dragging: bool,
    commit_mouse_drag_mode: Option<CommitMouseSelectionMode>,
    commit_mouse_drag_baseline: Option<Vec<bool>>,
    last_list_wheel_event: Option<(FocusPane, isize, Instant)>,
    diff_visual: Option<DiffVisualSelection>,
    diff_mouse_anchor: Option<usize>,
    aggregate: AggregatedDiff,
    selected_file: Option<String>,
    diff_positions: HashMap<String, DiffPosition>,
    file_diff_ranges: Vec<FileDiffRange>,
    file_diff_range_by_path: HashMap<String, (usize, usize)>,
    pending_diff_view_anchor: Option<PendingDiffViewAnchor>,
    diff_position: DiffPosition,
    rendered_diff: Arc<Vec<RenderedDiffLine>>,
    rendered_diff_cache: HashMap<(String, ThemeMode), Arc<Vec<RenderedDiffLine>>>,
    rendered_diff_key: Option<RenderedDiffKey>,
    highlighter: DiffSyntaxHighlighter,
    pane_rects: PaneRects,
    status: String,
    comment_buffer: String,
    comment_cursor: usize,
    comment_selection: Option<(usize, usize)>,
    comment_mouse_anchor: Option<usize>,
    comment_editor_rect: Option<ratatui::layout::Rect>,
    comment_editor_line_ranges: Vec<(usize, usize)>,
    comment_editor_view_start: usize,
    comment_editor_text_offset: u16,
    diff_search_buffer: String,
    diff_search_query: Option<String>,
    commit_search_query: String,
    file_search_query: String,
    commit_status_filter: CommitStatusFilter,
    diff_pending_op: Option<DiffPendingOp>,
    selection_rebuild_due: Option<Instant>,
    show_help: bool,
    onboarding_step: Option<OnboardingStep>,
    last_refresh: Instant,
    last_relative_time_redraw: Instant,
    needs_redraw: bool,
    should_quit: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct RenderedDiffKey {
    theme_mode: ThemeMode,
    visible_paths: Vec<String>,
}

#[derive(Default)]
struct FileTree {
    dirs: BTreeMap<String, FileTree>,
    files: BTreeMap<String, i64>,
}

impl FileTree {
    fn insert(&mut self, path: &str, modified_ts: i64) {
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
                .or_insert(modified_ts);
            *entry = max(*entry, modified_ts);
        }
    }

    fn flattened_rows(&self, nerd_fonts: bool, nerd_font_theme: &NerdFontTheme) -> Vec<TreeRow> {
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
            });
            child.flatten_into(rows, path, depth + 1, nerd_fonts, nerd_font_theme);
        }

        for (file, modified_ts) in &self.files {
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
                modified_ts: Some(*modified_ts),
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

struct DiffSyntaxHighlighter {
    syntaxes: SyntaxSet,
    dark_theme: Theme,
    light_theme: Theme,
    highlight_cache: HashMap<DiffHighlightCacheKey, Vec<Span<'static>>>,
    highlight_cache_order: VecDeque<DiffHighlightCacheKey>,
    highlight_cache_capacity: usize,
}

impl DiffSyntaxHighlighter {
    fn new() -> Self {
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
        self.syntaxes
            .find_syntax_for_file(path)
            .ok()
            .flatten()
            .unwrap_or_else(|| self.syntaxes.find_syntax_plain_text())
    }

    fn theme_for_mode(&self, mode: ThemeMode) -> &Theme {
        match mode {
            ThemeMode::Dark => &self.dark_theme,
            ThemeMode::Light => &self.light_theme,
        }
    }

    fn highlight_single_line(
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

fn blend_colors(base: Color, overlay: Color, overlay_weight: u8) -> Color {
    match (base, overlay) {
        (Color::Rgb(br, bg, bb), Color::Rgb(or, og, ob)) => {
            let keep_weight = u16::from(255_u8.saturating_sub(overlay_weight));
            let overlay_weight = u16::from(overlay_weight);
            let mix = |base: u8, over: u8| -> u8 {
                (((u16::from(base) * keep_weight) + (u16::from(over) * overlay_weight)) / 255) as u8
            };
            Color::Rgb(mix(br, or), mix(bg, og), mix(bb, ob))
        }
        (_, over) => over,
    }
}

fn contains(rect: ratatui::layout::Rect, x: u16, y: u16) -> bool {
    x >= rect.x
        && x < rect.x.saturating_add(rect.width)
        && y >= rect.y
        && y < rect.y.saturating_add(rect.height)
}

fn list_index_at(mouse_y: u16, rect: ratatui::layout::Rect, offset: usize) -> Option<usize> {
    if rect.height < 3 {
        return None;
    }
    if mouse_y <= rect.y || mouse_y >= rect.y + rect.height - 1 {
        return None;
    }
    let row = mouse_y.saturating_sub(rect.y + 1) as usize;
    Some(offset + row)
}

fn list_drag_scroll_delta(mouse_y: u16, rect: ratatui::layout::Rect, edge_margin: u16) -> isize {
    if rect.height < 3 {
        return 0;
    }
    let content_top = rect.y.saturating_add(1);
    let content_bottom = rect.y.saturating_add(rect.height.saturating_sub(2));
    if mouse_y <= content_top.saturating_add(edge_margin) {
        return -1;
    }
    if mouse_y >= content_bottom.saturating_sub(edge_margin) {
        return 1;
    }
    0
}

fn list_wheel_event_is_duplicate(
    last_event: Option<(FocusPane, isize, Instant)>,
    pane: FocusPane,
    delta: isize,
    now: Instant,
    min_interval: Duration,
) -> bool {
    if let Some((last_pane, last_delta, last_time)) = last_event {
        return last_pane == pane
            && last_delta == delta
            && now.duration_since(last_time) < min_interval;
    }
    false
}

fn diff_index_at(
    mouse_y: u16,
    rect: ratatui::layout::Rect,
    scroll: usize,
    sticky_banner_indexes: &[usize],
) -> Option<usize> {
    if rect.height < 3 {
        return None;
    }
    if mouse_y <= rect.y || mouse_y >= rect.y + rect.height - 1 {
        return None;
    }

    let row = mouse_y.saturating_sub(rect.y + 1) as usize;
    if row < sticky_banner_indexes.len() {
        sticky_banner_indexes.get(row).copied()
    } else {
        Some(scroll + row.saturating_sub(sticky_banner_indexes.len()))
    }
}

fn diff_visual_from_drag_anchor(
    anchor: Option<usize>,
    cursor: usize,
) -> Option<DiffVisualSelection> {
    let anchor = anchor?;
    (anchor != cursor).then_some(DiffVisualSelection {
        anchor,
        origin: DiffVisualOrigin::Mouse,
    })
}

fn should_clear_diff_visual_on_wheel(visual: Option<DiffVisualSelection>) -> bool {
    visual.is_some_and(|selection| selection.origin == DiffVisualOrigin::Keyboard)
}

fn commit_mouse_selection_mode(modifiers: KeyModifiers) -> CommitMouseSelectionMode {
    if modifiers.contains(KeyModifiers::SHIFT) {
        return CommitMouseSelectionMode::Range;
    }
    if modifiers.intersects(KeyModifiers::CONTROL | KeyModifiers::SUPER) {
        return CommitMouseSelectionMode::Toggle;
    }
    CommitMouseSelectionMode::Replace
}

fn apply_toggle_range_from_baseline(
    rows: &mut [CommitRow],
    baseline: &[bool],
    start: usize,
    end: usize,
) {
    if rows.len() != baseline.len() {
        return;
    }
    let (start, end) = (min(start, end), max(start, end));
    for (idx, row) in rows.iter_mut().enumerate() {
        row.selected = if idx >= start && idx <= end {
            !baseline[idx]
        } else {
            baseline[idx]
        };
    }
}

fn compose_sticky_banner_indexes(
    sticky_file_idx: Option<usize>,
    sticky_commit_idx: Option<usize>,
    viewport_rows: usize,
) -> Vec<usize> {
    let max_sticky = viewport_rows.saturating_sub(1);
    if max_sticky == 0 {
        return Vec::new();
    }

    let mut sticky = Vec::with_capacity(2);
    if let Some(file_idx) = sticky_file_idx {
        sticky.push(file_idx);
    }
    if sticky.len() < max_sticky
        && let Some(commit_idx) = sticky_commit_idx
    {
        sticky.push(commit_idx);
    }
    sticky
}

fn truncate(text: &str, max_chars: usize) -> String {
    if max_chars == 0 {
        return String::new();
    }
    if display_width(text) <= max_chars {
        return text.to_owned();
    }
    if max_chars == 1 {
        return "…".to_owned();
    }

    let target_width = max_chars.saturating_sub(1);
    let mut out = String::new();
    let mut used_width = 0usize;
    for ch in text.chars() {
        let width = UnicodeWidthChar::width(ch).unwrap_or(0);
        if used_width + width > target_width {
            break;
        }
        out.push(ch);
        used_width += width;
    }
    out.push('…');
    out
}

fn display_width(text: &str) -> usize {
    UnicodeWidthStr::width(text)
}

fn contains_case_insensitive(haystack: &str, needle: &str) -> bool {
    haystack.to_lowercase().contains(&needle.to_lowercase())
}

fn commit_row_matches_query(row: &CommitRow, query: &str) -> bool {
    let status = status_short_label(row.status);
    contains_case_insensitive(&row.info.short_id, query)
        || contains_case_insensitive(&row.info.id, query)
        || contains_case_insensitive(&row.info.summary, query)
        || contains_case_insensitive(&row.info.author, query)
        || contains_case_insensitive(status, query)
}

fn commit_row_matches_filter_query(row: &CommitRow, query: &str) -> bool {
    row.is_uncommitted || query.is_empty() || commit_row_matches_query(row, query)
}

/// Returns first-parent history commit IDs that should be baseline-reviewed on first open.
///
/// All visible commits that are already pushed are marked `REVIEWED`; unpushed commits remain
/// `UNREVIEWED` so users can focus on local outgoing work.
fn first_open_reviewed_commit_ids(commits: &[CommitInfo]) -> Vec<String> {
    commits
        .iter()
        .filter(|commit| !commit.unpushed)
        .map(|commit| commit.id.clone())
        .collect()
}

fn canonical_gitignore_entry(entry: &str) -> String {
    let trimmed = entry.trim();
    let trimmed = trimmed.strip_prefix("./").unwrap_or(trimmed);
    let trimmed = trimmed.trim_start_matches('/');
    trimmed.trim_end_matches('/').to_owned()
}

fn gitignore_contains_entry(contents: &str, entry: &str) -> bool {
    let needle = canonical_gitignore_entry(entry);
    if needle.is_empty() {
        return false;
    }

    contents.lines().any(|line| {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            return false;
        }
        canonical_gitignore_entry(trimmed) == needle
    })
}

fn append_gitignore_entry(path: &Path, entry: &str) -> anyhow::Result<GitignoreUpdate> {
    let canonical = canonical_gitignore_entry(entry);
    if canonical.is_empty() {
        return Ok(GitignoreUpdate::AlreadyPresent);
    }

    let existing = if path.exists() {
        fs::read_to_string(path).with_context(|| format!("failed to read {}", path.display()))?
    } else {
        String::new()
    };
    if gitignore_contains_entry(&existing, &canonical) {
        return Ok(GitignoreUpdate::AlreadyPresent);
    }

    let mut next = existing;
    if !next.is_empty() && !next.ends_with('\n') {
        next.push('\n');
    }
    next.push_str(&canonical);
    next.push('\n');
    fs::write(path, next).with_context(|| format!("failed to write {}", path.display()))?;
    Ok(GitignoreUpdate::Added)
}

fn matching_file_indices_with_parent_dirs(rows: &[TreeRow], query: &str) -> Vec<usize> {
    if query.is_empty() {
        return rows.iter().enumerate().map(|(idx, _)| idx).collect();
    }

    let mut include = BTreeSet::new();
    let mut ancestor_dirs: Vec<(usize, usize)> = Vec::new();
    for (idx, row) in rows.iter().enumerate() {
        if row.selectable {
            while ancestor_dirs
                .last()
                .is_some_and(|(depth, _)| *depth >= row.depth)
            {
                ancestor_dirs.pop();
            }
            if row
                .path
                .as_ref()
                .is_some_and(|path| contains_case_insensitive(path, query))
            {
                include.insert(idx);
                for (_, ancestor_idx) in &ancestor_dirs {
                    include.insert(*ancestor_idx);
                }
            }
            continue;
        }

        while ancestor_dirs
            .last()
            .is_some_and(|(depth, _)| *depth >= row.depth)
        {
            ancestor_dirs.pop();
        }
        ancestor_dirs.push((row.depth, idx));
    }

    include.into_iter().collect()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum WordClass {
    Whitespace,
    Word,
    Symbol,
}

fn classify_char(ch: char) -> WordClass {
    if ch.is_whitespace() {
        WordClass::Whitespace
    } else if ch.is_alphanumeric() || ch == '_' {
        WordClass::Word
    } else {
        WordClass::Symbol
    }
}

fn clamp_char_boundary(text: &str, cursor: usize) -> usize {
    let mut idx = cursor.min(text.len());
    while idx > 0 && !text.is_char_boundary(idx) {
        idx -= 1;
    }
    idx
}

fn prev_char_boundary(text: &str, cursor: usize) -> usize {
    if cursor == 0 {
        return 0;
    }
    text[..cursor]
        .char_indices()
        .next_back()
        .map(|(idx, _)| idx)
        .unwrap_or(0)
}

fn next_char_boundary(text: &str, cursor: usize) -> usize {
    if cursor >= text.len() {
        return text.len();
    }
    let Some(ch) = text[cursor..].chars().next() else {
        return text.len();
    };
    cursor + ch.len_utf8()
}

fn prev_word_boundary(text: &str, cursor: usize) -> usize {
    let mut idx = clamp_char_boundary(text, cursor);
    while idx > 0 {
        let prev = prev_char_boundary(text, idx);
        let ch = text[prev..idx].chars().next().expect("char at boundary");
        if classify_char(ch) == WordClass::Whitespace {
            idx = prev;
        } else {
            break;
        }
    }
    if idx == 0 {
        return 0;
    }
    let prev = prev_char_boundary(text, idx);
    let cls = classify_char(text[prev..idx].chars().next().expect("char at boundary"));
    while idx > 0 {
        let next = prev_char_boundary(text, idx);
        let ch_cls = classify_char(text[next..idx].chars().next().expect("char at boundary"));
        if ch_cls == cls {
            idx = next;
        } else {
            break;
        }
    }
    idx
}

fn next_word_boundary(text: &str, cursor: usize) -> usize {
    let mut idx = clamp_char_boundary(text, cursor);
    while idx < text.len() {
        let next = next_char_boundary(text, idx);
        let ch = text[idx..next].chars().next().expect("char at boundary");
        if classify_char(ch) == WordClass::Whitespace {
            idx = next;
        } else {
            break;
        }
    }
    if idx >= text.len() {
        return text.len();
    }
    let next = next_char_boundary(text, idx);
    let cls = classify_char(text[idx..next].chars().next().expect("char at boundary"));
    while idx < text.len() {
        let tail = next_char_boundary(text, idx);
        let ch_cls = classify_char(text[idx..tail].chars().next().expect("char at boundary"));
        if ch_cls == cls {
            idx = tail;
        } else {
            break;
        }
    }
    idx
}

fn insert_char_at_cursor(text: &mut String, cursor: &mut usize, ch: char) {
    let idx = clamp_char_boundary(text, *cursor);
    text.insert(idx, ch);
    *cursor = idx + ch.len_utf8();
}

fn delete_prev_char(text: &mut String, cursor: &mut usize) {
    let idx = clamp_char_boundary(text, *cursor);
    if idx == 0 {
        *cursor = 0;
        return;
    }
    let start = prev_char_boundary(text, idx);
    text.replace_range(start..idx, "");
    *cursor = start;
}

fn delete_next_char(text: &mut String, cursor: &mut usize) {
    let idx = clamp_char_boundary(text, *cursor);
    if idx >= text.len() {
        *cursor = text.len();
        return;
    }
    let end = next_char_boundary(text, idx);
    text.replace_range(idx..end, "");
    *cursor = idx;
}

fn delete_prev_word(text: &mut String, cursor: &mut usize) {
    let idx = clamp_char_boundary(text, *cursor);
    let start = prev_word_boundary(text, idx);
    if start == idx {
        *cursor = idx;
        return;
    }
    text.replace_range(start..idx, "");
    *cursor = start;
}

fn delete_next_word(text: &mut String, cursor: &mut usize) {
    let idx = clamp_char_boundary(text, *cursor);
    let end = next_word_boundary(text, idx);
    if end == idx {
        *cursor = idx;
        return;
    }
    text.replace_range(idx..end, "");
    *cursor = idx;
}

fn line_start_boundary(text: &str, cursor: usize) -> usize {
    let idx = clamp_char_boundary(text, cursor);
    text[..idx].rfind('\n').map(|pos| pos + 1).unwrap_or(0)
}

fn line_end_boundary(text: &str, cursor: usize) -> usize {
    let idx = clamp_char_boundary(text, cursor);
    text[idx..]
        .find('\n')
        .map(|pos| idx + pos)
        .unwrap_or(text.len())
}

fn line_char_count(text: &str, line_start: usize, line_end: usize) -> usize {
    text[line_start..line_end].chars().count()
}

fn line_cursor_with_column(text: &str, line_start: usize, line_end: usize, column: usize) -> usize {
    let mut idx = line_start;
    let mut col = 0usize;
    while idx < line_end && col < column {
        let next = next_char_boundary(text, idx);
        if next <= idx || next > line_end {
            break;
        }
        idx = next;
        col += 1;
    }
    idx
}

fn move_cursor_up(text: &str, cursor: usize) -> usize {
    let idx = clamp_char_boundary(text, cursor);
    let current_start = line_start_boundary(text, idx);
    if current_start == 0 {
        return idx;
    }
    let current_col = line_char_count(text, current_start, idx);
    let prev_end = current_start.saturating_sub(1);
    let prev_start = line_start_boundary(text, prev_end);
    let prev_len = line_char_count(text, prev_start, prev_end);
    line_cursor_with_column(text, prev_start, prev_end, current_col.min(prev_len))
}

fn move_cursor_down(text: &str, cursor: usize) -> usize {
    let idx = clamp_char_boundary(text, cursor);
    let current_start = line_start_boundary(text, idx);
    let current_end = line_end_boundary(text, idx);
    if current_end >= text.len() {
        return idx;
    }
    let current_col = line_char_count(text, current_start, idx);
    let next_start = current_end + 1;
    let next_end = line_end_boundary(text, next_start);
    let next_len = line_char_count(text, next_start, next_end);
    line_cursor_with_column(text, next_start, next_end, current_col.min(next_len))
}

fn delete_to_line_start(text: &mut String, cursor: &mut usize) {
    let idx = clamp_char_boundary(text, *cursor);
    let start = line_start_boundary(text, idx);
    if start == idx {
        *cursor = idx;
        return;
    }
    text.replace_range(start..idx, "");
    *cursor = start;
}

fn delete_to_line_end(text: &mut String, cursor: &mut usize) {
    let idx = clamp_char_boundary(text, *cursor);
    let end = line_end_boundary(text, idx);
    if idx >= end {
        *cursor = idx;
        return;
    }
    text.replace_range(idx..end, "");
    *cursor = idx;
}

fn normalize_selection_range(
    text: &str,
    selection: Option<(usize, usize)>,
) -> Option<(usize, usize)> {
    let (raw_start, raw_end) = selection?;
    let start = clamp_char_boundary(text, raw_start);
    let end = clamp_char_boundary(text, raw_end);
    let (lo, hi) = if start <= end {
        (start, end)
    } else {
        (end, start)
    };
    (lo < hi).then_some((lo, hi))
}

fn delete_selection_range(
    text: &mut String,
    cursor: &mut usize,
    selection: &mut Option<(usize, usize)>,
) -> bool {
    let Some((start, end)) = normalize_selection_range(text, *selection) else {
        *selection = None;
        return false;
    };
    text.replace_range(start..end, "");
    *cursor = start;
    *selection = None;
    true
}

fn comment_line_ranges(text: &str) -> Vec<(usize, usize)> {
    let mut ranges = Vec::<(usize, usize)>::new();
    let mut start = 0usize;
    for (idx, ch) in text.char_indices() {
        if ch == '\n' {
            ranges.push((start, idx));
            start = idx + ch.len_utf8();
        }
    }
    ranges.push((start, text.len()));
    ranges
}

fn comment_cursor_line_col(text: &str, cursor: usize) -> (usize, usize) {
    let idx = clamp_char_boundary(text, cursor);
    let line = text[..idx].chars().filter(|ch| *ch == '\n').count() + 1;
    let line_start = text[..idx].rfind('\n').map(|pos| pos + 1).unwrap_or(0);
    let col = text[line_start..idx].chars().count() + 1;
    (line, col)
}

struct CommentModalView {
    lines: Vec<Line<'static>>,
    line_ranges: Vec<(usize, usize)>,
    view_start: usize,
    text_offset: u16,
}

fn comment_gutter_digits(total_lines: usize) -> usize {
    total_lines.max(1).to_string().len().max(2)
}

fn comment_modal_lines(
    text: &str,
    cursor: usize,
    selection: Option<(usize, usize)>,
    viewport_rows: usize,
    theme: &UiTheme,
) -> CommentModalView {
    let ranges = comment_line_ranges(text);
    let clamped_cursor = clamp_char_boundary(text, cursor);
    let cursor_line_idx = text[..clamped_cursor]
        .chars()
        .filter(|ch| *ch == '\n')
        .count();
    let selected = normalize_selection_range(text, selection);
    let rows = viewport_rows.max(1);
    let max_start = ranges.len().saturating_sub(rows);
    let mut view_start = cursor_line_idx.saturating_sub(rows / 2);
    view_start = view_start.min(max_start);
    let view_end = (view_start + rows).min(ranges.len());
    let gutter_digits = comment_gutter_digits(ranges.len());
    let text_offset = (gutter_digits + 3) as u16;

    let mut lines = Vec::new();
    for (line_idx, (line_start, line_end)) in ranges
        .iter()
        .enumerate()
        .skip(view_start)
        .take(view_end.saturating_sub(view_start))
    {
        let gutter = Span::styled(
            format!("{:>width$} ", line_idx + 1, width = gutter_digits),
            Style::default().fg(theme.dimmed),
        );
        let mut spans = vec![
            gutter,
            Span::styled("│ ", Style::default().fg(theme.border)),
        ];

        let cursor_on_line = line_idx == cursor_line_idx;
        let mut idx = *line_start;
        while idx < *line_end {
            let next = next_char_boundary(text, idx);
            let fragment = &text[idx..next];
            let is_selected = selected.is_some_and(|(start, end)| idx >= start && idx < end);
            let is_cursor = cursor_on_line && idx == clamped_cursor;
            if is_cursor {
                spans.push(Span::styled(
                    fragment.to_owned(),
                    Style::default()
                        .fg(theme.modal_cursor_fg)
                        .bg(theme.modal_cursor_bg),
                ));
            } else if is_selected {
                spans.push(Span::styled(
                    fragment.to_owned(),
                    Style::default().bg(theme.visual_bg),
                ));
            } else {
                spans.push(Span::raw(fragment.to_owned()));
            }
            idx = next;
        }
        if cursor_on_line && clamped_cursor == *line_end {
            spans.push(Span::styled(
                " ",
                Style::default()
                    .fg(theme.modal_cursor_fg)
                    .bg(theme.modal_cursor_bg),
            ));
        }
        lines.push(Line::from(spans));
    }

    if lines.is_empty() {
        lines.push(Line::from(vec![Span::styled(
            " ",
            Style::default()
                .fg(theme.modal_cursor_fg)
                .bg(theme.modal_cursor_bg),
        )]));
    }

    CommentModalView {
        lines,
        line_ranges: ranges,
        view_start,
        text_offset,
    }
}

fn key_chip(label: &'static str, theme: &UiTheme) -> Span<'static> {
    Span::styled(
        format!(" {} ", label),
        Style::default()
            .fg(theme.panel_title_fg)
            .bg(theme.panel_title_bg)
            .add_modifier(Modifier::BOLD),
    )
}

fn status_short_label(status: ReviewStatus) -> &'static str {
    match status {
        ReviewStatus::Unreviewed => "UNREVIEWED",
        ReviewStatus::Reviewed => "REVIEWED",
        ReviewStatus::IssueFound => "ISSUE_FOUND",
        ReviewStatus::Resolved => "RESOLVED",
    }
}

fn centered_rect(
    width_percent: u16,
    height_percent: u16,
    area: ratatui::layout::Rect,
) -> ratatui::layout::Rect {
    let vertical = ratatui::layout::Layout::default()
        .direction(ratatui::layout::Direction::Vertical)
        .constraints([
            ratatui::layout::Constraint::Percentage((100 - height_percent) / 2),
            ratatui::layout::Constraint::Percentage(height_percent),
            ratatui::layout::Constraint::Percentage((100 - height_percent) / 2),
        ])
        .split(area);

    ratatui::layout::Layout::default()
        .direction(ratatui::layout::Direction::Horizontal)
        .constraints([
            ratatui::layout::Constraint::Percentage((100 - width_percent) / 2),
            ratatui::layout::Constraint::Percentage(width_percent),
            ratatui::layout::Constraint::Percentage((100 - width_percent) / 2),
        ])
        .split(vertical[1])[1]
}

fn format_relative_time(timestamp: i64, now: i64) -> String {
    let delta = now.saturating_sub(timestamp).max(0);
    if delta < 60 {
        format!("{}s ago", delta)
    } else if delta < 3_600 {
        format!("{}m ago", delta / 60)
    } else if delta < 86_400 {
        format!("{}h ago", delta / 3_600)
    } else if delta < 2_592_000 {
        format!("{}d ago", delta / 86_400)
    } else if delta < 31_536_000 {
        format!("{}mo ago", delta / 2_592_000)
    } else {
        format!("{}y ago", delta / 31_536_000)
    }
}

fn raw_diff_text(line: &HunkLine) -> String {
    let prefix = match line.kind {
        DiffLineKind::Add => '+',
        DiffLineKind::Remove => '-',
        DiffLineKind::Context => ' ',
        DiffLineKind::Meta => '~',
    };
    format!("{}{}", prefix, line.text)
}

fn prune_diff_positions_for_missing_paths(
    diff_positions: &mut HashMap<String, DiffPosition>,
    existing_paths: &BTreeSet<String>,
) {
    diff_positions.retain(|path, _| existing_paths.contains(path));
}

fn changed_paths_between_aggregates(
    current: &AggregatedDiff,
    next: &AggregatedDiff,
) -> BTreeSet<String> {
    let mut changed = BTreeSet::new();
    let all_paths = current
        .files
        .keys()
        .chain(next.files.keys())
        .cloned()
        .collect::<BTreeSet<_>>();

    for path in all_paths {
        if current.files.get(&path) != next.files.get(&path) {
            changed.insert(path);
        }
    }

    changed
}

fn should_render_commit_banner(previous_commit_id: Option<&str>, current_commit_id: &str) -> bool {
    previous_commit_id != Some(current_commit_id)
}

fn push_comment_lines_for_anchor(
    rendered: &mut Vec<RenderedDiffLine>,
    comments: &[&ReviewComment],
    injected_ids: &mut BTreeSet<u64>,
    anchor: &CommentAnchor,
    theme: &UiTheme,
    now_ts: i64,
) {
    for comment in comments {
        if injected_ids.contains(&comment.id) {
            continue;
        }
        if comment_anchor_matches(anchor, &comment.target.end) {
            injected_ids.insert(comment.id);
            push_comment_lines(rendered, comment, theme, now_ts);
        }
    }
}

fn push_comment_lines(
    rendered: &mut Vec<RenderedDiffLine>,
    comment: &ReviewComment,
    theme: &UiTheme,
    now_ts: i64,
) {
    let age = comment_age(comment, now_ts);
    let location = comment_location_label(comment);
    rendered.push(RenderedDiffLine {
        line: Line::from(vec![
            Span::styled(
                format!("  [#{}] ", comment.id),
                Style::default()
                    .fg(theme.focus_border)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                format!("[{}] ", comment.target.kind.as_str()),
                Style::default().fg(theme.accent),
            ),
            Span::styled(location, Style::default().fg(theme.muted)),
            Span::raw(" "),
            Span::styled(format!("({})", age), Style::default().fg(theme.dimmed)),
            Span::raw(" "),
            Span::styled("[e edit | D delete]", Style::default().fg(theme.dimmed)),
        ]),
        raw_text: format!("#{} {}", comment.id, comment.text),
        anchor: None,
        comment_id: Some(comment.id),
    });

    for text in comment.text.lines() {
        rendered.push(RenderedDiffLine {
            line: Line::from(vec![
                Span::styled("       ", Style::default().fg(theme.dimmed)),
                Span::styled(text.to_owned(), Style::default().fg(theme.text)),
            ]),
            raw_text: text.to_owned(),
            anchor: None,
            comment_id: Some(comment.id),
        });
    }
}

fn comment_anchor_matches(actual: &CommentAnchor, expected: &CommentAnchor) -> bool {
    actual.commit_id == expected.commit_id
        && actual.file_path == expected.file_path
        && actual.hunk_header == expected.hunk_header
        && actual.old_lineno == expected.old_lineno
        && actual.new_lineno == expected.new_lineno
}

fn is_commit_anchor(anchor: &CommentAnchor) -> bool {
    anchor.hunk_header == COMMIT_ANCHOR_HEADER
        && anchor.old_lineno.is_none()
        && anchor.new_lineno.is_none()
}

fn comment_targets_commit_end(comment: &ReviewComment, path: &str, commit_id: &str) -> bool {
    comment.target.kind == CommentTargetKind::Commit
        && comment.target.end.file_path == path
        && comment.target.end.commit_id == commit_id
}

fn comment_targets_hunk_end(
    comment: &ReviewComment,
    path: &str,
    commit_id: &str,
    hunk_header: &str,
) -> bool {
    comment.target.kind == CommentTargetKind::Hunk
        && comment.target.end.file_path == path
        && comment.target.end.commit_id == commit_id
        && comment.target.end.hunk_header == hunk_header
}

fn format_anchor_lines(old_lineno: Option<u32>, new_lineno: Option<u32>) -> String {
    match (old_lineno, new_lineno) {
        (Some(old), Some(new)) => format!("old {old}/new {new}"),
        (Some(old), None) => format!("old {old}"),
        (None, Some(new)) => format!("new {new}"),
        (None, None) => "n/a".to_owned(),
    }
}

fn comment_location_label(comment: &ReviewComment) -> String {
    if comment.target.kind == CommentTargetKind::Commit {
        let short = comment
            .target
            .end
            .commit_id
            .chars()
            .take(7)
            .collect::<String>();
        return format!("commit {short}");
    }

    let start = format_anchor_lines(
        comment.target.start.old_lineno,
        comment.target.start.new_lineno,
    );
    let end = format_anchor_lines(comment.target.end.old_lineno, comment.target.end.new_lineno);
    if start == end {
        format!("line {start}")
    } else {
        format!("range {start} -> {end}")
    }
}

fn comment_age(comment: &ReviewComment, now_ts: i64) -> String {
    let ts = DateTime::parse_from_rfc3339(&comment.updated_at)
        .map(|dt| dt.timestamp())
        .unwrap_or(now_ts);
    format_relative_time(ts, now_ts)
}

fn selected_ids_oldest_first(rows: &[CommitRow]) -> Vec<String> {
    rows.iter()
        .rev()
        .filter(|row| row.selected && !row.is_uncommitted)
        .map(|row| row.info.id.clone())
        .collect()
}

fn index_of_commit(rows: &[CommitRow], commit_id: &str) -> Option<usize> {
    rows.iter().position(|row| row.info.id == commit_id)
}

#[cfg(test)]
fn restore_list_index_by_commit_id(
    rows: &[CommitRow],
    previous_commit_id: Option<&str>,
    fallback_index: Option<usize>,
) -> Option<usize> {
    if rows.is_empty() {
        return None;
    }
    if let Some(commit_id) = previous_commit_id
        && let Some(idx) = index_of_commit(rows, commit_id)
    {
        return Some(idx);
    }
    fallback_index
        .map(|idx| idx.min(rows.len() - 1))
        .or(Some(0))
}

fn merge_aggregate_diff(base: &mut AggregatedDiff, next: AggregatedDiff) {
    for (path, mut patch) in next.files {
        base.files
            .entry(path.clone())
            .or_insert_with(|| FilePatch {
                path,
                hunks: Vec::new(),
            })
            .hunks
            .append(&mut patch.hunks);
    }
}

fn apply_range_selection(rows: &mut [CommitRow], start: usize, end: usize) {
    let (start, end) = (min(start, end), max(start, end));
    for (idx, row) in rows.iter_mut().enumerate() {
        row.selected = idx >= start && idx <= end;
    }
}

fn select_only_index(rows: &mut [CommitRow], selected_idx: usize) {
    for (idx, row) in rows.iter_mut().enumerate() {
        row.selected = idx == selected_idx;
    }
}

fn apply_status_ids(rows: &mut [CommitRow], ids: &BTreeSet<String>, status: ReviewStatus) {
    for row in rows {
        if ids.contains(&row.info.id) {
            row.status = status;
        }
    }
}

fn auto_deselect_status(status: ReviewStatus) -> bool {
    matches!(status, ReviewStatus::Reviewed | ReviewStatus::Resolved)
}

fn deselect_ids(rows: &mut [CommitRow], ids: &BTreeSet<String>) {
    for row in rows {
        if ids.contains(&row.info.id) {
            row.selected = false;
        }
    }
}

fn apply_status_transition(rows: &mut [CommitRow], ids: &BTreeSet<String>, status: ReviewStatus) {
    apply_status_ids(rows, ids, status);
    if auto_deselect_status(status) {
        deselect_ids(rows, ids);
    }
}

fn selected_ids_will_change_for_status_update(
    rows: &[CommitRow],
    ids: &BTreeSet<String>,
    status: ReviewStatus,
) -> bool {
    if !auto_deselect_status(status) {
        return false;
    }
    rows.iter()
        .any(|row| row.selected && ids.contains(&row.info.id))
}

fn deselect_rows_outside_status_filter(
    rows: &mut [CommitRow],
    status_filter: CommitStatusFilter,
) -> usize {
    let mut deselected = 0usize;
    for row in rows {
        if row.selected && !status_filter.matches_row(row) {
            row.selected = false;
            deselected += 1;
        }
    }
    deselected
}

fn page_step(height: u16, multiplier: f32) -> isize {
    let visible = height.saturating_sub(2).max(1) as f32;
    (visible * multiplier).round() as isize
}

fn diff_empty_state_message(
    has_rendered_diff: bool,
    changed_files: usize,
    rendered_files: usize,
    file_search_query: &str,
) -> Option<String> {
    if has_rendered_diff || changed_files == 0 || rendered_files > 0 {
        return None;
    }

    let query = file_search_query.trim();
    if query.is_empty() {
        return None;
    }

    Some(format!(
        "Diff hidden: file tree filter /{query} hides all {changed_files} changed file(s)"
    ))
}

fn next_poll_timeout(
    refresh_elapsed: Duration,
    relative_elapsed: Duration,
    selection_rebuild_in: Option<Duration>,
) -> Duration {
    // Sleep until the earliest maintenance deadline: git auto-refresh or coarse age-label repaint.
    let timeout = AUTO_REFRESH_EVERY
        .saturating_sub(refresh_elapsed)
        .min(RELATIVE_TIME_REDRAW_EVERY.saturating_sub(relative_elapsed));
    if let Some(selection_timeout) = selection_rebuild_in {
        timeout.min(selection_timeout)
    } else {
        timeout
    }
}

fn scrolled_diff_position_preserving_offset(
    current: DiffPosition,
    delta: isize,
    max_scroll: usize,
    max_index: usize,
) -> DiffPosition {
    let offset = current.cursor.saturating_sub(current.scroll);
    let delta_abs = delta.saturating_abs() as usize;
    let next_scroll = if delta >= 0 {
        current.scroll.saturating_add(delta_abs)
    } else {
        current.scroll.saturating_sub(delta_abs)
    }
    .min(max_scroll);

    DiffPosition {
        scroll: next_scroll,
        cursor: next_scroll.saturating_add(offset).min(max_index),
    }
}

fn focus_with_h(current: FocusPane) -> FocusPane {
    match current {
        FocusPane::Commits => FocusPane::Diff,
        FocusPane::Files => FocusPane::Commits,
        FocusPane::Diff => FocusPane::Files,
    }
}

fn focus_with_l(current: FocusPane) -> FocusPane {
    match current {
        FocusPane::Commits => FocusPane::Files,
        FocusPane::Files => FocusPane::Diff,
        FocusPane::Diff => FocusPane::Commits,
    }
}

#[cfg(test)]
mod tests;
