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

mod core_helpers;
mod flow;
mod input;
mod lifecycle_input;
mod lifecycle_mouse;
mod lifecycle_render;
mod lifecycle_view;
mod navigation;
mod nerd_fonts;
mod path_watch_driver;
mod ports;
mod review_state_sync;
mod runtime;
mod selection_helpers;
mod services;
mod shell_command;
mod state;
mod system_theme;
mod text_edit;
mod theme_palette;
mod tree_highlight;
mod ui;
mod worktree_switcher;
use self::core_helpers::*;
use self::nerd_fonts::{
    CommitPushChainMarkerKind, NerdFontTheme, app_title_label, branch_label_prefix,
    commit_push_chain_marker, commit_selection_marker, commit_status_badge,
    commit_status_filter_label_prefix, format_file_change_badge, format_path_with_icon,
    format_tree_dir_label, format_tree_file_label, list_highlight_symbol,
    list_highlight_symbol_width, uncommitted_badge, worktree_label_prefix,
};
use self::path_watch_driver::{PathWatchDriver, PathWatchTrigger};
use self::ports::{AppBootstrapPorts, AppClock, AppRuntimePorts, SystemBootstrapPorts};
use self::review_state_sync::ReviewStateSync;
use self::selection_helpers::*;
use self::text_edit::*;
use self::theme_palette::ThemeRuntimeState;
use self::tree_highlight::*;
use self::ui::diff_pane::{
    DiffPaneBody, DiffPaneRenderer, DiffPaneTitle, DiffViewportBuildInput, PendingDiffViewAnchor,
    build_diff_viewport_rows, capture_pending_diff_view_anchor, find_diff_match_from_cursor,
    find_index_for_line_locator, is_hunk_header_line, scrollbar_thumb,
};
use self::ui::list_panes::{CommitPaneModel, FilePaneModel, ListPaneRenderer};
use self::ui::style::{CursorSelectionPolicy, apply_row_highlight};
use self::worktree_switcher::short_path_label;

use crate::{
    config::{AppConfig, ThemePreference},
    git_data::{GitService, WorktreeInfo},
    model::{
        AggregatedDiff, CommitInfo, DiffLineAnchor, DiffLineAnchorMeta, DiffLineKind,
        FileChangeKind, FileChangeSummary, FilePatch, ReviewState, ReviewStatus,
        UNCOMMITTED_COMMIT_ID, UNCOMMITTED_COMMIT_SHORT, UNCOMMITTED_COMMIT_SUMMARY,
    },
    store::StateStore,
};

const LIST_DRAG_EDGE_MARGIN: u16 = 1;
const COMMIT_ANCHOR_HEADER: &str = "__COMMIT__";
const DELETED_FILE_TOGGLE_RAW_TEXT: &str = "__DELETED_FILE_TOGGLE__";
const SYNTAX_HIGHLIGHT_CACHE_CAPACITY: usize = 8_192;
const SHELL_HISTORY_LIMIT: usize = 1_000;
const SHELL_STREAM_POLL_EVERY: Duration = Duration::from_millis(30);
const DRAW_BUDGET_WARNING: Duration = Duration::from_millis(24);
const AUTO_THEME_SYNC_EVERY: Duration = Duration::from_secs(5);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FocusPane {
    Files,
    Commits,
    Diff,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum InputMode {
    Normal,
    ShellCommand,
    WorktreeSwitch,
    DiffSearch,
    ListSearch(FocusPane),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(in crate::app) enum ThemeMode {
    Dark,
    Light,
}

impl ThemeMode {
    fn label(self) -> &'static str {
        match self {
            Self::Dark => "dark",
            Self::Light => "light",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct ThemeSelectionState {
    preference: ThemePreference,
    resolved_mode: ThemeMode,
    system_mode: Option<ThemeMode>,
}

impl ThemeSelectionState {
    fn new(preference: ThemePreference, system_mode: Option<ThemeMode>) -> Self {
        Self {
            preference,
            resolved_mode: resolve_theme_mode(preference, system_mode),
            system_mode,
        }
    }

    fn cycle_preference(&mut self) {
        self.preference = match self.preference {
            ThemePreference::Auto => ThemePreference::Dark,
            ThemePreference::Dark => ThemePreference::Light,
            ThemePreference::Light => ThemePreference::Auto,
        };
        self.resolved_mode = resolve_theme_mode(self.preference, self.system_mode);
    }

    fn sync_system_mode(&mut self, system_mode: Option<ThemeMode>) -> bool {
        let Some(system_mode) = system_mode else {
            return false;
        };
        let previous_mode = self.resolved_mode;
        self.system_mode = Some(system_mode);
        self.resolved_mode = resolve_theme_mode(self.preference, self.system_mode);
        previous_mode != self.resolved_mode
    }

    fn resolved_mode(self) -> ThemeMode {
        self.resolved_mode
    }

    fn preference_label(self) -> &'static str {
        match self.preference {
            ThemePreference::Dark => "dark",
            ThemePreference::Light => "light",
            ThemePreference::Auto => "auto",
        }
    }
}

fn resolve_theme_mode(preference: ThemePreference, system_mode: Option<ThemeMode>) -> ThemeMode {
    match preference {
        ThemePreference::Dark => ThemeMode::Dark,
        ThemePreference::Light => ThemeMode::Light,
        ThemePreference::Auto => system_mode.unwrap_or(ThemeMode::Dark),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CommitStatusFilter {
    All,
    UnreviewedOrIssueFound,
    Reviewed,
}

impl CommitStatusFilter {
    fn next(self) -> Self {
        match self {
            Self::All => Self::UnreviewedOrIssueFound,
            Self::UnreviewedOrIssueFound => Self::Reviewed,
            Self::Reviewed => Self::All,
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::All => "All",
            Self::UnreviewedOrIssueFound => "Unreviewed + Issue Found",
            Self::Reviewed => "Reviewed",
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
            Self::Reviewed => matches!(row.status, ReviewStatus::Reviewed),
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
    footer_chip_bg: Color,
    text: Color,
    muted: Color,
    dimmed: Color,
    cursor_bg: Color,
    focused_cursor_bg: Color,
    cursor_visual_overlap_weight: u8,
    block_cursor_fg: Color,
    block_cursor_bg: Color,
    visual_bg: Color,
    commit_selected_bg: Color,
    commit_selected_text: Color,
    search_match_fg: Color,
    search_match_bg: Color,
    search_current_fg: Color,
    search_current_bg: Color,
    reviewed: Color,
    unreviewed: Color,
    issue: Color,
    pushed: Color,
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
                footer_chip_bg: Color::Reset,
                text: Color::Rgb(228, 228, 228),
                muted: Color::Rgb(170, 170, 170),
                dimmed: Color::Rgb(115, 115, 115),
                cursor_bg: Color::Rgb(52, 52, 62),
                focused_cursor_bg: Color::Rgb(50, 56, 70),
                cursor_visual_overlap_weight: 150,
                block_cursor_fg: Color::Rgb(245, 245, 245),
                block_cursor_bg: Color::Rgb(95, 128, 255),
                visual_bg: Color::Rgb(62, 78, 108),
                commit_selected_bg: Color::Reset,
                commit_selected_text: Color::Rgb(120, 196, 255),
                search_match_fg: Color::Rgb(30, 30, 30),
                search_match_bg: Color::Rgb(219, 196, 96),
                search_current_fg: Color::Rgb(12, 12, 12),
                search_current_bg: Color::Rgb(246, 205, 68),
                reviewed: Color::Rgb(85, 190, 120),
                unreviewed: Color::Rgb(236, 92, 92),
                issue: Color::Rgb(238, 184, 64),
                pushed: Color::Rgb(122, 176, 202),
                unpushed: Color::Rgb(115, 115, 115),
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
                footer_chip_bg: Color::Reset,
                text: Color::Rgb(57, 57, 57),
                muted: Color::Rgb(106, 106, 106),
                dimmed: Color::Rgb(165, 165, 165),
                cursor_bg: Color::Rgb(226, 226, 226),
                focused_cursor_bg: Color::Rgb(224, 224, 224),
                cursor_visual_overlap_weight: 255,
                block_cursor_fg: Color::Rgb(255, 255, 255),
                block_cursor_bg: Color::Rgb(41, 94, 214),
                visual_bg: Color::Rgb(198, 198, 240),
                commit_selected_bg: Color::Reset,
                commit_selected_text: Color::Rgb(0, 123, 184),
                search_match_fg: Color::Rgb(35, 35, 35),
                search_match_bg: Color::Rgb(247, 234, 172),
                search_current_fg: Color::Rgb(28, 22, 0),
                search_current_bg: Color::Rgb(241, 197, 72),
                reviewed: Color::Rgb(36, 141, 74),
                unreviewed: Color::Rgb(194, 48, 48),
                issue: Color::Rgb(170, 113, 0),
                pushed: Color::Rgb(44, 113, 131),
                unpushed: Color::Rgb(165, 165, 165),
                diff_add: Color::Rgb(106, 106, 106),
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
    #[cfg(test)]
    /// Test-only escape hatch for unit tests that inject prebuilt display rows.
    line: Line<'static>,
    raw_text: Arc<str>,
    anchor: Option<DiffLineAnchor>,
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
    Range,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum IgnoreFileUpdate {
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
    theme: ThemeSelectionState,
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
    visible_rows: Vec<DiffVisibleRow>,
    last_list_wheel_event: Option<(FocusPane, isize, Instant)>,
    pane_rects: PaneRects,
    pending_op: Option<DiffPendingOp>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct DiffVisibleRow {
    line_index: usize,
    wrapped_row_offset: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum HelperClickAction {
    Key {
        code: KeyCode,
        modifiers: KeyModifiers,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct HelperClickHitbox {
    rect: ratatui::layout::Rect,
    action: HelperClickAction,
}

/// Cached diff rendering state and per-file viewport persistence.
struct DiffCacheState {
    selected_file: Option<String>,
    positions: HashMap<String, DiffPosition>,
    file_ranges: Vec<FileDiffRange>,
    file_range_by_path: HashMap<String, (usize, usize)>,
    pending_view_anchor: Option<PendingDiffViewAnchor>,
    rendered_key: Option<RenderedDiffKey>,
    highlighter: DiffSyntaxHighlighter,
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
    last_auto_theme_sync: Instant,
    last_refresh: Instant,
    last_relative_time_redraw: Instant,
    last_terminal_clear: Instant,
    terminal_clear_requested: bool,
    needs_redraw: bool,
    should_quit: bool,
    draw_perf: DrawPerfState,
}

/// Tunable runtime limits and cadences loaded from the app config file.
#[derive(Debug, Clone, Copy)]
struct RuntimeTuning {
    history_limit: usize,
    auto_theme_sync_every: Duration,
    auto_refresh_every: Duration,
    relative_time_redraw_every: Duration,
    theme_reload_poll_every: Duration,
    selection_rebuild_debounce: Duration,
    terminal_clear_every: Duration,
    diff_cursor_scroll_off_lines: usize,
    diff_context_lines: u32,
    diff_hunk_merge_distance_lines: u32,
}

impl RuntimeTuning {
    fn from_config(config: &AppConfig) -> Self {
        Self {
            history_limit: config.history_limit,
            auto_theme_sync_every: AUTO_THEME_SYNC_EVERY,
            auto_refresh_every: Duration::from_secs(config.auto_refresh_every_secs),
            relative_time_redraw_every: Duration::from_secs(config.relative_time_redraw_every_secs),
            theme_reload_poll_every: Duration::from_millis(config.theme_reload_poll_every_ms),
            selection_rebuild_debounce: Duration::from_millis(config.selection_rebuild_debounce_ms),
            terminal_clear_every: Duration::from_secs(config.terminal_clear_every_secs),
            diff_cursor_scroll_off_lines: config.diff_cursor_scroll_off_lines,
            diff_context_lines: config.diff_context_lines,
            diff_hunk_merge_distance_lines: config.diff_hunk_merge_distance_lines,
        }
    }
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
    shell_command: ShellCommandState,
    worktree_switch: WorktreeSwitchState,
    search: SearchState,
    helper_click_hitboxes: Vec<HelperClickHitbox>,
}

/// High-level app state and interaction flow for the hunkr UI.
pub struct App {
    deps: AppDependencies,
    domain: AppDomainState,
    ui: AppUiState,
    theme: ThemeRuntimeState,
    theme_reload_driver: PathWatchDriver,
    review_state_sync: ReviewStateSync,
    review_state_sync_driver: PathWatchDriver,
    runtime: RuntimeState,
    tuning: RuntimeTuning,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct RenderedDiffKey {
    theme_mode: ThemeMode,
    visible_paths: Vec<String>,
}

impl App {
    fn active_theme(&self) -> &UiTheme {
        self.theme
            .for_mode(self.ui.preferences.theme.resolved_mode())
    }

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
