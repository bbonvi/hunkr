use std::{
    cmp::{max, min},
    collections::{BTreeMap, BTreeSet, HashMap, VecDeque},
    fs,
    io::Read,
    path::Path,
    process::{Child, Command, ExitStatus, Stdio},
    sync::{Arc, mpsc},
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

use anyhow::Context;
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
mod flow;
mod input;
mod lifecycle_input;
mod lifecycle_mouse;
mod lifecycle_render;
mod lifecycle_view;
mod navigation;
mod nerd_fonts;
mod ports;
mod runtime;
mod selection_helpers;
mod services;
mod shell_command;
mod state;
mod text_edit;
mod tree_highlight;
mod ui;
mod worktree_switcher;
use self::comment_helpers::*;
use self::core_helpers::*;
use self::nerd_fonts::{
    CommitPushChainMarkerKind, NerdFontTheme, app_title_label, branch_label_prefix,
    commit_comment_badge, commit_push_chain_marker, commit_selection_marker, commit_status_badge,
    commit_status_filter_label_prefix, file_change_kind_symbol, format_file_change_badge,
    format_path_with_icon, format_tree_dir_label, format_tree_file_label, list_highlight_symbol,
    list_highlight_symbol_width, uncommitted_badge, worktree_label_prefix,
};
use self::ports::{AppBootstrapPorts, AppClock, AppRuntimePorts, SystemBootstrapPorts};
use self::selection_helpers::*;
use self::text_edit::*;
use self::tree_highlight::*;
use self::ui::diff_pane::{
    DiffPaneBody, DiffPaneRenderer, DiffPaneTitle, PendingDiffViewAnchor,
    capture_pending_diff_view_anchor, find_diff_match_from_cursor, find_index_for_line_locator,
    is_hunk_header_line, scrollbar_thumb,
};
use self::ui::list_panes::{CommitPaneModel, FilePaneModel, ListPaneRenderer};
use self::ui::style::{CursorSelectionPolicy, apply_row_highlight};
use self::worktree_switcher::short_path_label;

use crate::{
    comments::CommentStore,
    config::StartupTheme,
    git_data::{GitService, WorktreeInfo},
    model::{
        AggregatedDiff, CommentAnchor, CommentTarget, CommentTargetKind, CommitInfo, DiffLineKind,
        FileChangeKind, FileChangeSummary, FilePatch, HunkLine, ReviewComment, ReviewState,
        ReviewStatus, UNCOMMITTED_COMMIT_ID, UNCOMMITTED_COMMIT_SHORT, UNCOMMITTED_COMMIT_SUMMARY,
    },
    store::{InstanceLock, StateStore},
};

const HISTORY_LIMIT: usize = 400;
const AUTO_REFRESH_EVERY: Duration = Duration::from_secs(4);
const RELATIVE_TIME_REDRAW_EVERY: Duration = Duration::from_secs(30);
const SELECTION_REBUILD_DEBOUNCE: Duration = Duration::from_millis(120);
const LIST_DRAG_EDGE_MARGIN: u16 = 1;
const COMMIT_ANCHOR_HEADER: &str = "__COMMIT__";
const DELETED_FILE_TOGGLE_RAW_TEXT: &str = "__DELETED_FILE_TOGGLE__";
const SYNTAX_HIGHLIGHT_CACHE_CAPACITY: usize = 8_192;
const SHELL_HISTORY_LIMIT: usize = 1_000;
const SHELL_STREAM_POLL_EVERY: Duration = Duration::from_millis(30);
const TERMINAL_CLEAR_EVERY: Duration = Duration::from_secs(120);
const DIFF_CURSOR_SCROLL_OFF_LINES: usize = 3;
const DRAW_BUDGET_WARNING: Duration = Duration::from_millis(24);

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
    ShellCommand,
    WorktreeSwitch,
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
        if row.is_uncommitted {
            return true;
        }

        match self {
            Self::All => true,
            Self::UnreviewedOrIssueFound => {
                matches!(
                    row.status,
                    ReviewStatus::Unreviewed | ReviewStatus::IssueFound
                )
            }
            Self::ReviewedOrResolved => {
                matches!(row.status, ReviewStatus::Reviewed | ReviewStatus::Resolved)
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
    focused_cursor_bg: Color,
    block_cursor_fg: Color,
    block_cursor_bg: Color,
    visual_bg: Color,
    search_match_fg: Color,
    search_match_bg: Color,
    search_current_fg: Color,
    search_current_bg: Color,
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
                focused_cursor_bg: Color::Rgb(57, 67, 93),
                block_cursor_fg: Color::Rgb(245, 245, 245),
                block_cursor_bg: Color::Rgb(95, 128, 255),
                visual_bg: Color::Rgb(57, 67, 93),
                search_match_fg: Color::Rgb(30, 30, 30),
                search_match_bg: Color::Rgb(219, 196, 96),
                search_current_fg: Color::Rgb(12, 12, 12),
                search_current_bg: Color::Rgb(246, 205, 68),
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
                modal_bg: Color::Reset,
                modal_editor_bg: Color::Reset,
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
                cursor_bg: Color::Rgb(234, 234, 234),
                focused_cursor_bg: Color::Rgb(230, 230, 230),
                block_cursor_fg: Color::Rgb(255, 255, 255),
                block_cursor_bg: Color::Rgb(41, 94, 214),
                visual_bg: Color::Rgb(215, 225, 241),
                search_match_fg: Color::Rgb(35, 35, 35),
                search_match_bg: Color::Rgb(247, 234, 172),
                search_current_fg: Color::Rgb(28, 22, 0),
                search_current_bg: Color::Rgb(241, 197, 72),
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
                modal_bg: Color::Reset,
                modal_editor_bg: Color::Reset,
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
    change: Option<FileChangeSummary>,
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
    block_cursor_col: usize,
    block_cursor_goal: usize,
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
    create_target_cache: Option<CommentCreateTargetCache>,
}

/// Cached comment target resolution for create-mode modal rendering/submission.
#[derive(Debug, Clone, PartialEq, Eq)]
enum CommentCreateTargetCache {
    Ready(Box<Option<CommentTarget>>),
    Error(String),
}

/// Shell command modal/editor mutable state.
struct ShellCommandState {
    buffer: String,
    cursor: usize,
    history: VecDeque<ShellCommandHistoryEntry>,
    history_nav: Option<usize>,
    history_draft: String,
    reverse_search: Option<ShellReverseSearchState>,
    active_command: Option<String>,
    output_lines: Vec<String>,
    output_tail: String,
    output_cursor: usize,
    output_visual_selection: Option<ShellOutputVisualSelection>,
    output_mouse_anchor: Option<usize>,
    output_flash_clear_due: Option<Instant>,
    output_scroll: usize,
    output_viewport: usize,
    output_follow: bool,
    output_rect: Option<ratatui::layout::Rect>,
    running: Option<RunningShellCommand>,
    finished: Option<ShellCommandResult>,
}

/// Visual selection state in shell output panel.
struct ShellOutputVisualSelection {
    anchor: usize,
    origin: ShellOutputVisualOrigin,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ShellOutputVisualOrigin {
    Keyboard,
    Mouse,
}

/// Reverse-search state for shell history (`Ctrl-r`).
struct ShellReverseSearchState {
    query: String,
    match_indexes: Vec<usize>,
    match_cursor: usize,
    draft_buffer: String,
}

/// Worktree switcher modal state (entries, filter query, cursor).
struct WorktreeSwitchState {
    entries: Vec<WorktreeInfo>,
    list_state: ListState,
    query: String,
    search_active: bool,
    viewport_rows: usize,
}

/// Final shell command result displayed after process exit.
struct ShellCommandResult {
    exit_status: ExitStatus,
}

#[derive(Debug)]
struct ShellCommandHistoryEntry {
    raw: String,
    raw_lower: String,
}

impl ShellCommandHistoryEntry {
    fn new(raw: String) -> Self {
        Self {
            raw_lower: raw.to_ascii_lowercase(),
            raw,
        }
    }
}

/// Process execution state with live stdout/stderr readers.
struct RunningShellCommand {
    child: Child,
    process_group_id: Option<u32>,
    stream_rx: mpsc::Receiver<String>,
    stdout_reader: Option<JoinHandle<()>>,
    stderr_reader: Option<JoinHandle<()>>,
    exit_status: Option<ExitStatus>,
}

/// Search/filter query buffers and edit cursors.
struct SearchState {
    diff_buffer: String,
    diff_cursor: usize,
    diff_query: Option<String>,
    commit_query: String,
    commit_cursor: usize,
    file_query: String,
    file_cursor: usize,
}

/// Runtime control flags and status text.
struct RuntimeState {
    status: String,
    selection_rebuild_due: Option<Instant>,
    show_help: bool,
    onboarding_step: Option<OnboardingStep>,
    last_refresh: Instant,
    last_relative_time_redraw: Instant,
    last_terminal_clear: Instant,
    terminal_clear_requested: bool,
    needs_redraw: bool,
    should_quit: bool,
    draw_perf: DrawPerfState,
}

/// Draw-loop performance metrics used as runtime guardrails.
#[derive(Debug, Clone, Copy, Default)]
struct DrawPerfState {
    last_draw_duration: Duration,
    max_draw_duration: Duration,
    over_budget_frames: u64,
}

/// External adapters/resources used by app workflows.
struct AppDependencies {
    git: GitService,
    store: StateStore,
    instance_lock: Option<InstanceLock>,
    comments: CommentStore,
    clock: Arc<dyn AppClock>,
    runtime_ports: Arc<dyn AppRuntimePorts>,
}

/// Business/domain projections currently shown in the UI.
struct AppDomainState {
    review_state: ReviewState,
    commits: Vec<CommitRow>,
    file_rows: Vec<TreeRow>,
    aggregate: AggregatedDiff,
    deleted_file_content_visible: BTreeSet<String>,
    diff_position: DiffPosition,
    rendered_diff: Arc<Vec<RenderedDiffLine>>,
}

/// UI interaction and view/cache state.
struct AppUiState {
    commit_ui: CommitUiState,
    file_ui: FileUiState,
    preferences: UiPreferences,
    diff_ui: DiffUiState,
    diff_cache: DiffCacheState,
    comment_editor: CommentEditorState,
    shell_command: ShellCommandState,
    worktree_switch: WorktreeSwitchState,
    search: SearchState,
}

/// High-level app state and interaction flow for the hunkr UI.
pub struct App {
    deps: AppDependencies,
    domain: AppDomainState,
    ui: AppUiState,
    runtime: RuntimeState,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct RenderedDiffKey {
    theme_mode: ThemeMode,
    visible_paths: Vec<String>,
}

impl App {
    pub(super) fn now_instant(&self) -> Instant {
        self.deps.clock.now_instant()
    }

    pub(super) fn now_timestamp(&self) -> i64 {
        self.deps.clock.now_utc().timestamp()
    }

    pub fn record_draw_duration(&mut self, duration: Duration) {
        self.runtime.draw_perf.last_draw_duration = duration;
        if duration > self.runtime.draw_perf.max_draw_duration {
            self.runtime.draw_perf.max_draw_duration = duration;
        }
        if duration > DRAW_BUDGET_WARNING {
            self.runtime.draw_perf.over_budget_frames =
                self.runtime.draw_perf.over_budget_frames.saturating_add(1);
        }
    }

    #[cfg(test)]
    pub(in crate::app) fn draw_perf_over_budget_frames(&self) -> u64 {
        self.runtime.draw_perf.over_budget_frames
    }
}

#[cfg(test)]
mod driver;
#[cfg(test)]
mod driver_tests;
#[cfg(test)]
mod shell_output_policy_tests;
#[cfg(test)]
mod tests;
