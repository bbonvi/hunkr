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

mod comment_helpers;
mod core_helpers;
mod lifecycle_input;
mod lifecycle_mouse;
mod lifecycle_render;
mod lifecycle_view;
mod navigation;
mod nerd_fonts;
mod selection_helpers;
mod state;
mod text_edit;
mod tree_highlight;
mod ui;
use self::comment_helpers::*;
use self::core_helpers::*;
use self::nerd_fonts::{
    NerdFontTheme, app_title_label, commit_selection_marker, format_path_with_icon,
    format_tree_dir_label, format_tree_file_label, list_highlight_symbol,
    list_highlight_symbol_width, uncommitted_badge, unpushed_marker,
};
use self::selection_helpers::*;
use self::text_edit::*;
use self::tree_highlight::*;
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

/// Commit list cursor/selection/filter UI state.
struct CommitUiState {
    list_state: ListState,
    visual_anchor: Option<usize>,
    selection_anchor: Option<usize>,
    mouse_anchor: Option<usize>,
    mouse_dragging: bool,
    mouse_drag_mode: Option<CommitMouseSelectionMode>,
    mouse_drag_baseline: Option<Vec<bool>>,
    status_filter: CommitStatusFilter,
}

/// File list cursor UI state.
struct FileUiState {
    list_state: ListState,
}

/// High-level UI preferences and active pane/input mode.
struct UiPreferences {
    focused: FocusPane,
    input_mode: InputMode,
    theme_mode: ThemeMode,
    diff_wheel_scroll_lines: isize,
    list_wheel_coalesce: Duration,
    nerd_fonts: bool,
    nerd_font_theme: NerdFontTheme,
}

/// Transient pane geometry and interaction state.
struct DiffUiState {
    visual_selection: Option<DiffVisualSelection>,
    mouse_anchor: Option<usize>,
    last_list_wheel_event: Option<(FocusPane, isize, Instant)>,
    pane_rects: PaneRects,
    pending_op: Option<DiffPendingOp>,
}

/// Cached diff rendering state and per-file viewport persistence.
struct DiffCacheState {
    selected_file: Option<String>,
    positions: HashMap<String, DiffPosition>,
    file_ranges: Vec<FileDiffRange>,
    file_range_by_path: HashMap<String, (usize, usize)>,
    pending_view_anchor: Option<PendingDiffViewAnchor>,
    rendered_cache: HashMap<(String, ThemeMode), Arc<Vec<RenderedDiffLine>>>,
    rendered_key: Option<RenderedDiffKey>,
    highlighter: DiffSyntaxHighlighter,
}

/// Comment modal/editor mutable state.
struct CommentEditorState {
    buffer: String,
    cursor: usize,
    selection: Option<(usize, usize)>,
    mouse_anchor: Option<usize>,
    rect: Option<ratatui::layout::Rect>,
    line_ranges: Vec<(usize, usize)>,
    view_start: usize,
    text_offset: u16,
}

/// Search/filter query buffers.
struct SearchState {
    diff_buffer: String,
    diff_query: Option<String>,
    commit_query: String,
    file_query: String,
}

/// Runtime control flags and status text.
struct RuntimeState {
    status: String,
    selection_rebuild_due: Option<Instant>,
    show_help: bool,
    onboarding_step: Option<OnboardingStep>,
    last_refresh: Instant,
    last_relative_time_redraw: Instant,
    needs_redraw: bool,
    should_quit: bool,
}

/// High-level app state and interaction flow for the hunkr UI.
pub struct App {
    git: GitService,
    store: StateStore,
    comments: CommentStore,
    review_state: ReviewState,
    commits: Vec<CommitRow>,
    file_rows: Vec<TreeRow>,
    aggregate: AggregatedDiff,
    diff_position: DiffPosition,
    rendered_diff: Arc<Vec<RenderedDiffLine>>,
    commit_ui: CommitUiState,
    file_ui: FileUiState,
    preferences: UiPreferences,
    diff_ui: DiffUiState,
    diff_cache: DiffCacheState,
    comment_editor: CommentEditorState,
    search: SearchState,
    runtime: RuntimeState,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct RenderedDiffKey {
    theme_mode: ThemeMode,
    visible_paths: Vec<String>,
}

#[cfg(test)]
mod tests;
