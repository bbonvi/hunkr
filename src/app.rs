use std::{
    cmp::{max, min},
    collections::{BTreeMap, BTreeSet, HashMap},
    sync::Arc,
    time::{Duration, Instant},
};

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
    easy::HighlightLines, highlighting::Theme, highlighting::ThemeSet, parsing::SyntaxSet,
};

mod lifecycle_render;
mod navigation;
mod state;
mod ui;
use self::ui::diff_pane::{
    DiffPaneRenderer, PendingDiffViewAnchor, capture_pending_diff_view_anchor,
    find_diff_match_from_cursor, find_index_for_line_locator, is_hunk_header_line,
};
use self::ui::list_panes::ListPaneRenderer;

use crate::{
    comments::CommentStore,
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
const COMMIT_ANCHOR_HEADER: &str = "__COMMIT__";
const LIST_HIGHLIGHT_SYMBOL: &str = ">> ";
const LIST_HIGHLIGHT_SYMBOL_WIDTH: u16 = 3;

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
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum ThemeMode {
    Dark,
    Light,
}

impl ThemeMode {
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

#[derive(Debug, Clone, Copy)]
struct DiffVisualSelection {
    anchor: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DiffPendingOp {
    Z,
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
    commit_visual_anchor: Option<usize>,
    diff_visual: Option<DiffVisualSelection>,
    aggregate: AggregatedDiff,
    selected_file: Option<String>,
    diff_positions: HashMap<String, DiffPosition>,
    pending_diff_view_anchor: Option<PendingDiffViewAnchor>,
    diff_position: DiffPosition,
    rendered_diff: Arc<Vec<RenderedDiffLine>>,
    rendered_diff_cache: HashMap<(String, ThemeMode), Arc<Vec<RenderedDiffLine>>>,
    rendered_diff_key: Option<(String, ThemeMode)>,
    highlighter: DiffSyntaxHighlighter,
    pane_rects: PaneRects,
    status: String,
    comment_buffer: String,
    diff_search_buffer: String,
    diff_search_query: Option<String>,
    diff_pending_op: Option<DiffPendingOp>,
    show_help: bool,
    last_refresh: Instant,
    should_quit: bool,
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

    fn flattened_rows(&self) -> Vec<TreeRow> {
        let mut rows = Vec::new();
        self.flatten_into(&mut rows, String::new(), 0);
        rows
    }

    fn flatten_into(&self, rows: &mut Vec<TreeRow>, prefix: String, depth: usize) {
        for (dir, child) in &self.dirs {
            let path = if prefix.is_empty() {
                dir.clone()
            } else {
                format!("{prefix}/{dir}")
            };
            rows.push(TreeRow {
                label: format!("{}[D] {}", "  ".repeat(depth), dir),
                path: None,
                selectable: false,
                modified_ts: None,
            });
            child.flatten_into(rows, path, depth + 1);
        }

        for (file, modified_ts) in &self.files {
            let full = if prefix.is_empty() {
                file.clone()
            } else {
                format!("{prefix}/{file}")
            };
            rows.push(TreeRow {
                label: format!("{}[F] {}", "  ".repeat(depth), file),
                path: Some(full),
                selectable: true,
                modified_ts: Some(*modified_ts),
            });
        }
    }
}

struct DiffSyntaxHighlighter {
    syntaxes: SyntaxSet,
    dark_theme: Theme,
    light_theme: Theme,
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
        }
    }

    fn highlight(&self, mode: ThemeMode, path: &str, line: &str) -> Vec<Span<'static>> {
        let syntax = self
            .syntaxes
            .find_syntax_for_file(path)
            .ok()
            .flatten()
            .unwrap_or_else(|| self.syntaxes.find_syntax_plain_text());

        let theme = match mode {
            ThemeMode::Dark => &self.dark_theme,
            ThemeMode::Light => &self.light_theme,
        };
        let mut highlighter = HighlightLines::new(syntax, theme);
        let highlighted = highlighter
            .highlight_line(line, &self.syntaxes)
            .unwrap_or_default();

        highlighted
            .into_iter()
            .map(|(style, text)| Span::styled(text.to_owned(), syntect_to_ratatui(style)))
            .collect()
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

fn diff_index_at(
    mouse_y: u16,
    rect: ratatui::layout::Rect,
    scroll: usize,
    sticky_banner_index: Option<usize>,
) -> Option<usize> {
    if rect.height < 3 {
        return None;
    }
    if mouse_y <= rect.y || mouse_y >= rect.y + rect.height - 1 {
        return None;
    }

    let row = mouse_y.saturating_sub(rect.y + 1) as usize;
    if let Some(sticky_idx) = sticky_banner_index {
        if row == 0 {
            Some(sticky_idx)
        } else {
            Some(scroll + row - 1)
        }
    } else {
        Some(scroll + row)
    }
}

fn truncate(text: &str, max_chars: usize) -> String {
    if text.chars().count() <= max_chars {
        return text.to_owned();
    }
    let mut out = text
        .chars()
        .take(max_chars.saturating_sub(1))
        .collect::<String>();
    out.push('…');
    out
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

fn page_step(height: u16, multiplier: f32) -> isize {
    let visible = height.saturating_sub(2).max(1) as f32;
    (visible * multiplier).round() as isize
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
