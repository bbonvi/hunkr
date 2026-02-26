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
    widgets::{Block, BorderType, Borders, Clear, List, ListItem, ListState, Paragraph},
};
use syntect::{
    easy::HighlightLines, highlighting::Theme, highlighting::ThemeSet, parsing::SyntaxSet,
};

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
    highlight_bg: Color,
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
                highlight_bg: Color::Rgb(36, 36, 42),
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
                highlight_bg: Color::Rgb(236, 236, 236),
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
    file_diff_signatures: HashMap<String, u64>,
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

impl App {
    pub fn bootstrap() -> anyhow::Result<Self> {
        let git = GitService::open_current()?;
        let store = StateStore::for_project(git.root());
        let review_state = store.load()?;
        let comments = CommentStore::new(store.root_dir(), git.branch_name())?;

        let mut app = Self {
            git,
            store,
            comments,
            review_state,
            commits: Vec::new(),
            commit_list_state: ListState::default(),
            file_rows: Vec::new(),
            file_list_state: ListState::default(),
            focused: FocusPane::Commits,
            input_mode: InputMode::Normal,
            theme_mode: ThemeMode::Dark,
            commit_visual_anchor: None,
            diff_visual: None,
            aggregate: AggregatedDiff::default(),
            selected_file: None,
            diff_positions: HashMap::new(),
            file_diff_signatures: HashMap::new(),
            diff_position: DiffPosition::default(),
            rendered_diff: Arc::new(Vec::new()),
            rendered_diff_cache: HashMap::new(),
            rendered_diff_key: None,
            highlighter: DiffSyntaxHighlighter::new(),
            pane_rects: PaneRects::default(),
            status: String::new(),
            comment_buffer: String::new(),
            diff_search_buffer: String::new(),
            diff_search_query: None,
            diff_pending_op: None,
            show_help: false,
            last_refresh: Instant::now(),
            should_quit: false,
        };

        app.reload_commits(true)?;
        if app.status.is_empty() {
            app.status =
                "Ready. Select commit range with <space>/v, set statuses with r/i/s/u.".to_owned();
        }
        Ok(app)
    }

    pub fn should_quit(&self) -> bool {
        self.should_quit
    }

    pub fn draw(&mut self, frame: &mut Frame<'_>) {
        self.ensure_rendered_diff();
        let theme = UiTheme::from_mode(self.theme_mode);

        let root_chunks = ratatui::layout::Layout::default()
            .direction(ratatui::layout::Direction::Vertical)
            .constraints([
                ratatui::layout::Constraint::Length(2),
                ratatui::layout::Constraint::Min(1),
                ratatui::layout::Constraint::Length(3),
            ])
            .split(frame.area());

        let main_chunks = ratatui::layout::Layout::default()
            .direction(ratatui::layout::Direction::Horizontal)
            .constraints([
                ratatui::layout::Constraint::Percentage(35),
                ratatui::layout::Constraint::Percentage(65),
            ])
            .split(root_chunks[1]);

        let left_chunks = ratatui::layout::Layout::default()
            .direction(ratatui::layout::Direction::Vertical)
            .constraints([
                ratatui::layout::Constraint::Percentage(58),
                ratatui::layout::Constraint::Percentage(42),
            ])
            .split(main_chunks[0]);

        self.pane_rects = PaneRects {
            files: left_chunks[0],
            commits: left_chunks[1],
            diff: main_chunks[1],
        };

        self.render_header(frame, root_chunks[0], &theme);
        self.render_files(frame, self.pane_rects.files, &theme);
        self.render_commits(frame, self.pane_rects.commits, &theme);
        self.render_diff(frame, self.pane_rects.diff, &theme);
        self.render_footer(frame, root_chunks[2], &theme);
        if self.show_help {
            self.render_help_overlay(frame, &theme);
        }
    }

    fn render_header(&self, frame: &mut Frame<'_>, rect: ratatui::layout::Rect, theme: &UiTheme) {
        let selected = self.commits.iter().filter(|row| row.selected).count();
        let (unreviewed, reviewed, issue_found, resolved) = self.status_counts();
        let focus = match self.focused {
            FocusPane::Files => "FILES",
            FocusPane::Commits => "COMMITS",
            FocusPane::Diff => "DIFF",
        };
        let headline = Line::from(vec![
            Span::styled(
                " HUNKR ",
                Style::default()
                    .fg(theme.panel_title_fg)
                    .bg(theme.panel_title_bg)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::raw(" "),
            Span::styled(
                format!("branch:{} ", self.git.branch_name()),
                Style::default().fg(theme.text),
            ),
            Span::styled(
                format!("focus:{} ", focus),
                Style::default().fg(theme.accent),
            ),
            Span::styled(
                format!("selected:{} ", selected),
                Style::default().fg(theme.muted),
            ),
            Span::styled(
                format!(
                    "U:{} R:{} I:{} Z:{} ",
                    unreviewed, reviewed, issue_found, resolved
                ),
                Style::default().fg(theme.muted),
            ),
            Span::styled(
                format!("theme:{} ", self.theme_mode.label()),
                Style::default().fg(theme.dimmed),
            ),
        ]);

        let header = Paragraph::new(headline).block(
            Block::default()
                .borders(Borders::BOTTOM)
                .border_style(Style::default().fg(theme.border)),
        );
        frame.render_widget(header, rect);
    }

    pub fn tick(&mut self) {
        if self.last_refresh.elapsed() >= AUTO_REFRESH_EVERY {
            if let Err(err) = self.reload_commits(true) {
                self.status = format!("refresh failed: {err:#}");
            }
            self.last_refresh = Instant::now();
        }
    }

    pub fn handle_event(&mut self, event: Event) {
        match event {
            Event::Key(key) if key.kind == KeyEventKind::Press => self.handle_key(key),
            Event::Mouse(mouse) => self.handle_mouse(mouse),
            Event::Resize(_, _) => self.ensure_cursor_visible(),
            _ => {}
        }
    }

    fn handle_key(&mut self, key: KeyEvent) {
        if !matches!(self.input_mode, InputMode::Normal) {
            self.handle_non_normal_input(key);
            return;
        }

        match key.code {
            KeyCode::Char('q') => self.should_quit = true,
            KeyCode::Tab if key.modifiers == KeyModifiers::NONE => self.focus_next(),
            KeyCode::BackTab if key.modifiers == KeyModifiers::NONE => self.focus_prev(),
            KeyCode::Char('l') if key.modifiers == KeyModifiers::NONE => {
                self.focused = focus_with_l(self.focused)
            }
            KeyCode::Char('h') if key.modifiers == KeyModifiers::NONE => {
                self.focused = focus_with_h(self.focused)
            }
            KeyCode::Char('1') => self.focused = FocusPane::Files,
            KeyCode::Char('2') => self.focused = FocusPane::Commits,
            KeyCode::Char('3') => self.focused = FocusPane::Diff,
            KeyCode::Char('f') if key.modifiers == KeyModifiers::NONE => {
                self.focused = FocusPane::Files
            }
            KeyCode::Char('c') if key.modifiers == KeyModifiers::NONE => {
                self.focused = FocusPane::Commits
            }
            KeyCode::Char('d') if key.modifiers == KeyModifiers::NONE => {
                self.focused = FocusPane::Diff
            }
            KeyCode::Char('t') => self.toggle_theme(),
            KeyCode::F(5) => self.refresh_now(),
            KeyCode::Char('r') if key.modifiers == KeyModifiers::CONTROL => self.refresh_now(),
            KeyCode::Char('?') => {
                self.show_help = !self.show_help;
                self.status = if self.show_help {
                    "Help overlay opened".to_owned()
                } else {
                    "Help overlay closed".to_owned()
                };
            }
            _ => self.dispatch_focus_key(key),
        }
    }

    fn refresh_now(&mut self) {
        if let Err(err) = self.reload_commits(true) {
            self.status = format!("reload failed: {err:#}");
        }
    }

    fn toggle_theme(&mut self) {
        self.theme_mode = self.theme_mode.toggle();
        self.rendered_diff_key = None;
        self.status = format!("Theme switched to {}", self.theme_mode.label());
    }

    fn handle_non_normal_input(&mut self, key: KeyEvent) {
        match self.input_mode {
            InputMode::CommentCreate | InputMode::CommentEdit(_) => self.handle_comment_input(key),
            InputMode::DiffSearch => self.handle_diff_search_input(key),
            InputMode::Normal => {}
        }
    }

    fn handle_comment_input(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Esc => {
                self.input_mode = InputMode::Normal;
                self.comment_buffer.clear();
                self.status = "Comment canceled".to_owned();
            }
            KeyCode::Enter => {
                if self.comment_buffer.trim().is_empty() {
                    self.status = "Comment is empty".to_owned();
                    self.input_mode = InputMode::Normal;
                    self.comment_buffer.clear();
                    return;
                }

                match self.input_mode {
                    InputMode::CommentCreate => {
                        if let Some(target) = self.comment_target_from_selection() {
                            let result = self.comments.add_comment(&target, &self.comment_buffer);
                            match result {
                                Ok(id) => {
                                    self.set_status_for_ids(
                                        &target.commits,
                                        ReviewStatus::IssueFound,
                                    );
                                    self.invalidate_diff_cache();
                                    if let Err(err) = self.sync_comment_report() {
                                        self.status = format!(
                                            "Comment #{} added, but review tasks sync failed: {err:#}",
                                            id
                                        );
                                        return;
                                    }
                                    self.status = format!(
                                        "Comment #{} added -> {} ({} commit(s) marked ISSUE_FOUND)",
                                        id,
                                        self.comments.report_path().display(),
                                        target.commits.len()
                                    );
                                }
                                Err(err) => {
                                    self.status = format!("Failed to save comment: {err:#}");
                                }
                            }
                        } else {
                            self.status =
                                "No hunk/line anchor at cursor or selected range".to_owned();
                        }
                    }
                    InputMode::CommentEdit(id) => {
                        match self.comments.update_comment(id, &self.comment_buffer) {
                            Ok(true) => {
                                self.invalidate_diff_cache();
                                if let Err(err) = self.sync_comment_report() {
                                    self.status = format!(
                                        "Comment #{} updated, but review tasks sync failed: {err:#}",
                                        id
                                    );
                                    return;
                                }
                                self.status = format!("Comment #{} updated", id);
                            }
                            Ok(false) => {
                                self.status = format!("Comment #{} not found", id);
                            }
                            Err(err) => {
                                self.status = format!("Failed to update comment #{}: {err:#}", id);
                            }
                        }
                    }
                    InputMode::DiffSearch => {}
                    InputMode::Normal => {}
                }

                self.input_mode = InputMode::Normal;
                self.comment_buffer.clear();
            }
            KeyCode::Backspace => {
                self.comment_buffer.pop();
            }
            KeyCode::Char(c) => {
                if key.modifiers.contains(KeyModifiers::CONTROL) {
                    return;
                }
                self.comment_buffer.push(c);
            }
            _ => {}
        }
    }

    fn handle_diff_search_input(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Esc => {
                self.input_mode = InputMode::Normal;
                self.diff_search_buffer.clear();
                self.status = "Diff search canceled".to_owned();
            }
            KeyCode::Enter => {
                let query = self.diff_search_buffer.trim().to_owned();
                self.input_mode = InputMode::Normal;
                self.diff_search_buffer.clear();
                if query.is_empty() {
                    self.status = "Diff search canceled".to_owned();
                    return;
                }
                self.execute_diff_search(&query, true);
            }
            KeyCode::Backspace => {
                self.diff_search_buffer.pop();
                self.status = format!("/{}", self.diff_search_buffer);
            }
            KeyCode::Char(c) => {
                if key.modifiers.contains(KeyModifiers::CONTROL) {
                    return;
                }
                self.diff_search_buffer.push(c);
                self.status = format!("/{}", self.diff_search_buffer);
            }
            _ => {}
        }
    }

    fn execute_diff_search(&mut self, query: &str, forward: bool) {
        let normalized = query.trim();
        if normalized.is_empty() {
            self.status = "Empty diff search query".to_owned();
            return;
        }

        self.diff_search_query = Some(normalized.to_owned());
        if let Some(idx) = find_diff_match_from_cursor(
            &self.rendered_diff,
            normalized,
            forward,
            self.diff_position.cursor,
        ) {
            self.set_diff_cursor(idx);
            self.status = format!("/{normalized} -> line {}", idx.saturating_add(1));
        } else {
            self.status = format!("/{normalized} -> no match");
        }
    }

    fn repeat_diff_search(&mut self, forward: bool) {
        let Some(query) = self.diff_search_query.clone() else {
            self.status = "No previous diff search".to_owned();
            return;
        };
        self.execute_diff_search(&query, forward);
    }

    fn start_comment_edit_mode(&mut self) {
        let Some(id) = self.current_comment_id() else {
            self.status = "No comment under cursor to edit".to_owned();
            return;
        };
        let Some(comment) = self.comments.comment_by_id(id) else {
            self.status = format!("Comment #{} missing", id);
            return;
        };
        self.input_mode = InputMode::CommentEdit(id);
        self.comment_buffer = comment.text.clone();
        self.status = format!("Editing comment #{}: Enter save, Esc cancel", id);
    }

    fn delete_current_comment(&mut self) {
        let Some(id) = self.current_comment_id() else {
            self.status = "No comment under cursor to delete".to_owned();
            return;
        };
        match self.comments.delete_comment(id) {
            Ok(true) => {
                self.invalidate_diff_cache();
                if let Err(err) = self.sync_comment_report() {
                    self.status = format!(
                        "Comment #{} deleted, but review tasks sync failed: {err:#}",
                        id
                    );
                    return;
                }
                self.status = format!("Comment #{} deleted", id);
            }
            Ok(false) => {
                self.status = format!("Comment #{} not found", id);
            }
            Err(err) => {
                self.status = format!("Failed to delete comment #{}: {err:#}", id);
            }
        }
    }

    fn dispatch_focus_key(&mut self, key: KeyEvent) {
        if self.focused != FocusPane::Diff {
            self.diff_pending_op = None;
        }
        match self.focused {
            FocusPane::Files => self.handle_files_key(key),
            FocusPane::Commits => self.handle_commits_key(key),
            FocusPane::Diff => self.handle_diff_key(key),
        }
    }

    fn handle_files_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Down | KeyCode::Char('j') => self.move_file_cursor(1),
            KeyCode::Up | KeyCode::Char('k') => self.move_file_cursor(-1),
            KeyCode::Char('d') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.page_files(0.5)
            }
            KeyCode::Char('u') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.page_files(-0.5)
            }
            KeyCode::PageDown => self.page_files(1.0),
            KeyCode::PageUp => self.page_files(-1.0),
            KeyCode::Char('g') => self.select_first_file(),
            KeyCode::Char('G') => self.select_last_file(),
            KeyCode::Enter | KeyCode::Char(' ') => self.focused = FocusPane::Diff,
            _ => {}
        }
    }

    fn handle_commits_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Down | KeyCode::Char('j') => self.move_commit_cursor(1),
            KeyCode::Up | KeyCode::Char('k') => self.move_commit_cursor(-1),
            KeyCode::Char('d') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.page_commits(0.5)
            }
            KeyCode::Char('u') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.page_commits(-0.5)
            }
            KeyCode::PageDown => self.page_commits(1.0),
            KeyCode::PageUp => self.page_commits(-1.0),
            KeyCode::Char('g') => self.select_first_commit(),
            KeyCode::Char('G') => self.select_last_commit(),
            KeyCode::Char('v') => {
                if self.commit_visual_anchor.is_some() {
                    self.commit_visual_anchor = None;
                    self.status = "Commit visual range off".to_owned();
                } else {
                    self.commit_visual_anchor = self.commit_list_state.selected();
                    self.status = "Commit visual range on".to_owned();
                    self.apply_commit_visual_range();
                }
            }
            KeyCode::Char('x') => {
                for row in &mut self.commits {
                    row.selected = false;
                }
                self.commit_visual_anchor = None;
                self.on_selection_changed();
            }
            KeyCode::Char(' ') => {
                if let Some(idx) = self.commit_list_state.selected()
                    && let Some(row) = self.commits.get_mut(idx)
                {
                    row.selected = !row.selected;
                }
                self.commit_visual_anchor = None;
                self.on_selection_changed();
            }
            KeyCode::Enter => {
                if let Some(idx) = self.commit_list_state.selected() {
                    select_only_index(&mut self.commits, idx);
                    self.commit_visual_anchor = None;
                    self.on_selection_changed();
                }
            }
            KeyCode::Char('u') => self.set_current_commit_status(ReviewStatus::Unreviewed),
            KeyCode::Char('r') => self.set_current_commit_status(ReviewStatus::Reviewed),
            KeyCode::Char('i') => self.set_current_commit_status(ReviewStatus::IssueFound),
            KeyCode::Char('s') => self.set_current_commit_status(ReviewStatus::Resolved),
            KeyCode::Char('U') => self.set_selected_commit_status(ReviewStatus::Unreviewed),
            KeyCode::Char('R') => self.set_selected_commit_status(ReviewStatus::Reviewed),
            KeyCode::Char('I') => self.set_selected_commit_status(ReviewStatus::IssueFound),
            KeyCode::Char('S') => self.set_selected_commit_status(ReviewStatus::Resolved),
            _ => {}
        }
    }

    fn handle_diff_key(&mut self, key: KeyEvent) {
        if let Some(op) = self.diff_pending_op {
            if key.modifiers == KeyModifiers::NONE {
                match (op, key.code) {
                    (DiffPendingOp::Z, KeyCode::Char('z')) => {
                        self.diff_pending_op = None;
                        self.align_diff_cursor_middle();
                        return;
                    }
                    (DiffPendingOp::Z, KeyCode::Char('t')) => {
                        self.diff_pending_op = None;
                        self.align_diff_cursor_top();
                        return;
                    }
                    (DiffPendingOp::Z, KeyCode::Char('b')) => {
                        self.diff_pending_op = None;
                        self.align_diff_cursor_bottom();
                        return;
                    }
                    _ => {}
                }
            }
            self.diff_pending_op = None;
        }

        match key.code {
            KeyCode::Down | KeyCode::Char('j') => self.move_diff_cursor(1),
            KeyCode::Up | KeyCode::Char('k') => self.move_diff_cursor(-1),
            KeyCode::Esc => {
                if self.diff_visual.is_some() {
                    self.diff_visual = None;
                    self.status = "Diff visual range off".to_owned();
                }
            }
            KeyCode::Char('g') => {
                self.diff_position.cursor = 0;
                self.ensure_cursor_visible();
            }
            KeyCode::Char('G') => {
                if !self.rendered_diff.is_empty() {
                    self.diff_position.cursor = self.rendered_diff.len() - 1;
                    self.ensure_cursor_visible();
                }
            }
            KeyCode::Char('u') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.page_diff(-0.5)
            }
            KeyCode::Char('d') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.page_diff(0.5)
            }
            KeyCode::PageUp => self.page_diff(-1.0),
            KeyCode::PageDown => self.page_diff(1.0),
            KeyCode::Char('z') if key.modifiers == KeyModifiers::NONE => {
                self.diff_pending_op = Some(DiffPendingOp::Z);
            }
            KeyCode::Char('[') if key.modifiers == KeyModifiers::NONE => self.move_prev_hunk(),
            KeyCode::Char(']') if key.modifiers == KeyModifiers::NONE => self.move_next_hunk(),
            KeyCode::Char('/') if key.modifiers == KeyModifiers::NONE => {
                self.input_mode = InputMode::DiffSearch;
                self.diff_search_buffer.clear();
                self.diff_pending_op = None;
                self.status = "/".to_owned();
            }
            KeyCode::Char('n') if key.modifiers == KeyModifiers::NONE => {
                self.repeat_diff_search(true);
            }
            KeyCode::Char('N') if key.modifiers == KeyModifiers::NONE => {
                self.repeat_diff_search(false);
            }
            KeyCode::Char('v') | KeyCode::Char('V') => {
                if self.rendered_diff.is_empty() {
                    return;
                }
                if self.diff_visual.is_some() {
                    self.diff_visual = None;
                    self.status = "Diff visual range off".to_owned();
                } else {
                    self.diff_visual = Some(DiffVisualSelection {
                        anchor: self.diff_position.cursor,
                    });
                    self.status = "Diff visual range on".to_owned();
                }
            }
            KeyCode::Char('m') => {
                if self.uncommitted_selected() {
                    self.status = "Comments are disabled for uncommitted changes".to_owned();
                    return;
                }
                self.input_mode = InputMode::CommentCreate;
                self.comment_buffer.clear();
                self.diff_pending_op = None;
                self.status =
                    "Comment mode: type comment, Enter save, Esc cancel (commit/hunk/range)"
                        .to_owned();
            }
            KeyCode::Char('e') => {
                if self.uncommitted_selected() {
                    self.status = "Comments are disabled for uncommitted changes".to_owned();
                    return;
                }
                self.start_comment_edit_mode();
            }
            KeyCode::Char('D') => {
                if self.uncommitted_selected() {
                    self.status = "Comments are disabled for uncommitted changes".to_owned();
                    return;
                }
                self.delete_current_comment();
            }
            _ => {}
        }
    }

    fn handle_mouse(&mut self, mouse: crossterm::event::MouseEvent) {
        let x = mouse.column;
        let y = mouse.row;

        let in_files = contains(self.pane_rects.files, x, y);
        let in_commits = contains(self.pane_rects.commits, x, y);
        let in_diff = contains(self.pane_rects.diff, x, y);

        match mouse.kind {
            MouseEventKind::ScrollDown => {
                if in_diff {
                    self.move_diff_cursor(3);
                } else if in_files {
                    self.move_file_cursor(1);
                } else if in_commits {
                    self.move_commit_cursor(1);
                }
            }
            MouseEventKind::ScrollUp => {
                if in_diff {
                    self.move_diff_cursor(-3);
                } else if in_files {
                    self.move_file_cursor(-1);
                } else if in_commits {
                    self.move_commit_cursor(-1);
                }
            }
            MouseEventKind::Down(MouseButton::Left) => {
                if in_files {
                    self.focused = FocusPane::Files;
                    if let Some(idx) =
                        list_index_at(y, self.pane_rects.files, self.file_list_state.offset())
                    {
                        self.select_file_row(idx);
                    }
                } else if in_commits {
                    self.focused = FocusPane::Commits;
                    if let Some(idx) =
                        list_index_at(y, self.pane_rects.commits, self.commit_list_state.offset())
                    {
                        self.select_commit_row(idx, true);
                    }
                } else if in_diff {
                    self.focused = FocusPane::Diff;
                    if let Some(row) =
                        list_index_at(y, self.pane_rects.diff, self.diff_position.scroll)
                    {
                        self.set_diff_cursor(row);
                    }
                }
            }
            _ => {}
        }
    }

    fn render_files(
        &mut self,
        frame: &mut Frame<'_>,
        rect: ratatui::layout::Rect,
        theme: &UiTheme,
    ) {
        let title = Line::from(vec![
            Span::styled(
                " 1 FILES ",
                Style::default()
                    .fg(theme.panel_title_fg)
                    .bg(theme.panel_title_bg)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::raw(" "),
            Span::styled(
                format!("{} changed", self.aggregate.files.len()),
                Style::default().fg(theme.muted),
            ),
        ]);
        let border_style = if self.focused == FocusPane::Files {
            Style::default().fg(theme.focus_border)
        } else {
            Style::default().fg(theme.border)
        };

        let width = list_content_width(rect.width);
        let now_ts = Utc::now().timestamp();

        let items: Vec<ListItem<'static>> = self
            .file_rows
            .iter()
            .map(|row| {
                if row.selectable {
                    let right = row
                        .modified_ts
                        .map(|ts| format_relative_time(ts, now_ts))
                        .unwrap_or_default();
                    ListItem::new(line_with_right(
                        row.label.clone(),
                        Style::default().fg(theme.text),
                        right,
                        Style::default().fg(theme.dimmed),
                        width,
                    ))
                } else {
                    ListItem::new(Line::from(Span::styled(
                        row.label.clone(),
                        Style::default().fg(theme.dir).add_modifier(Modifier::BOLD),
                    )))
                }
            })
            .collect();

        let list = List::new(items)
            .block(
                Block::default()
                    .title(title)
                    .borders(Borders::ALL)
                    .border_type(BorderType::Rounded)
                    .border_style(border_style),
            )
            .highlight_style(Style::default().bg(theme.highlight_bg))
            .highlight_symbol(LIST_HIGHLIGHT_SYMBOL);

        frame.render_stateful_widget(list, rect, &mut self.file_list_state);
    }

    fn render_commits(
        &mut self,
        frame: &mut Frame<'_>,
        rect: ratatui::layout::Rect,
        theme: &UiTheme,
    ) {
        let selected = self.commits.iter().filter(|row| row.selected).count();
        let (unreviewed, reviewed, issue_found, resolved) = self.status_counts();
        let title = Line::from(vec![
            Span::styled(
                " 2 COMMITS ",
                Style::default()
                    .fg(theme.panel_title_fg)
                    .bg(theme.panel_title_bg)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::raw(" "),
            Span::styled(
                format!(
                    "sel:{}  U:{} R:{} I:{} Z:{}",
                    selected, unreviewed, reviewed, issue_found, resolved
                ),
                Style::default().fg(theme.muted),
            ),
        ]);
        let border_style = if self.focused == FocusPane::Commits {
            Style::default().fg(theme.focus_border)
        } else {
            Style::default().fg(theme.border)
        };

        let width = list_content_width(rect.width);
        let now_ts = Utc::now().timestamp();
        let items: Vec<ListItem<'static>> = self
            .commits
            .iter()
            .map(|row| ListItem::new(compose_commit_line(row, width, now_ts, theme)))
            .collect();

        let list = List::new(items)
            .block(
                Block::default()
                    .title(title)
                    .borders(Borders::ALL)
                    .border_type(BorderType::Rounded)
                    .border_style(border_style),
            )
            .highlight_style(Style::default().bg(theme.highlight_bg))
            .highlight_symbol(LIST_HIGHLIGHT_SYMBOL);

        frame.render_stateful_widget(list, rect, &mut self.commit_list_state);
    }

    fn render_diff(&mut self, frame: &mut Frame<'_>, rect: ratatui::layout::Rect, theme: &UiTheme) {
        let border_style = if self.focused == FocusPane::Diff {
            Style::default().fg(theme.focus_border)
        } else {
            Style::default().fg(theme.border)
        };

        let file_label = self
            .selected_file
            .clone()
            .unwrap_or_else(|| "(no file selected)".to_owned());
        let selected = self
            .diff_selected_range()
            .map(|(start, end)| end.saturating_sub(start) + 1)
            .unwrap_or(0);
        let title = Line::from(vec![
            Span::styled(
                " 3 DIFF ",
                Style::default()
                    .fg(theme.panel_title_fg)
                    .bg(theme.panel_title_bg)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::raw(" "),
            Span::styled(file_label, Style::default().fg(theme.text)),
            Span::raw(" "),
            Span::styled(
                format!("{} line(s) selected", selected),
                Style::default().fg(theme.muted),
            ),
        ]);

        let visual_range = self.diff_selected_range();
        let mut lines = Vec::with_capacity(self.rendered_diff.len());
        for (idx, rendered) in self.rendered_diff.iter().enumerate() {
            let mut line = rendered.line.clone();

            if let Some((start, end)) = visual_range
                && idx >= start
                && idx <= end
            {
                line = tint_line_background(&line, theme.visual_bg, false);
            }

            if idx == self.diff_position.cursor && self.focused == FocusPane::Diff {
                line = tint_line_background(&line, theme.cursor_bg, true);
            }
            lines.push(line);
        }

        if lines.is_empty() {
            lines.push(Line::from(Span::styled(
                "No selected commits or no textual diff for this file",
                Style::default().fg(theme.muted),
            )));
        }

        let paragraph = Paragraph::new(lines)
            .block(
                Block::default()
                    .title(title)
                    .borders(Borders::ALL)
                    .border_type(BorderType::Rounded)
                    .border_style(border_style),
            )
            .scroll((self.diff_position.scroll as u16, 0));

        frame.render_widget(paragraph, rect);
        self.render_diff_scrollbar(frame, rect, theme);
    }

    fn render_diff_scrollbar(
        &self,
        frame: &mut Frame<'_>,
        rect: ratatui::layout::Rect,
        theme: &UiTheme,
    ) {
        if rect.width < 3 || rect.height < 3 {
            return;
        }

        let inner_height = rect.height.saturating_sub(2) as usize;
        if inner_height == 0 {
            return;
        }

        let total = self.rendered_diff.len().max(1);
        let (thumb_start, thumb_len) =
            scrollbar_thumb(total, inner_height, self.diff_position.scroll);

        let x = rect.x.saturating_add(rect.width.saturating_sub(2));
        let y = rect.y.saturating_add(1);
        let track_style = Style::default().fg(theme.dimmed);
        let thumb_style = Style::default()
            .fg(theme.muted)
            .add_modifier(Modifier::BOLD);

        let buffer = frame.buffer_mut();
        for row in 0..inner_height {
            buffer.set_string(x, y + row as u16, "│", track_style);
        }
        for row in thumb_start..thumb_start.saturating_add(thumb_len) {
            if row < inner_height {
                buffer.set_string(x, y + row as u16, "█", thumb_style);
            }
        }
    }

    fn render_footer(&self, frame: &mut Frame<'_>, rect: ratatui::layout::Rect, theme: &UiTheme) {
        let mode = match self.input_mode {
            InputMode::Normal => "NORMAL",
            InputMode::CommentCreate => "COMMENT+",
            InputMode::CommentEdit(_) => "COMMENT*",
            InputMode::DiffSearch => "SEARCH/",
        };
        let focus = match self.focused {
            FocusPane::Files => "files",
            FocusPane::Commits => "commits",
            FocusPane::Diff => "diff",
        };

        let pane_line = match self.input_mode {
            InputMode::CommentCreate | InputMode::CommentEdit(_) => Line::from(vec![
                key_chip("Enter", theme),
                Span::styled(" save ", Style::default().fg(theme.muted)),
                key_chip("Esc", theme),
                Span::styled(" cancel comment", Style::default().fg(theme.muted)),
            ]),
            InputMode::DiffSearch => Line::from(vec![
                key_chip("Enter", theme),
                Span::styled(" search ", Style::default().fg(theme.muted)),
                key_chip("Esc", theme),
                Span::styled(" cancel search", Style::default().fg(theme.muted)),
            ]),
            InputMode::Normal => match self.focused {
                FocusPane::Files => Line::from(vec![
                    key_chip("j/k", theme),
                    Span::styled(" move ", Style::default().fg(theme.muted)),
                    key_chip("Ctrl-d/u", theme),
                    Span::styled(" jump ", Style::default().fg(theme.muted)),
                    key_chip("Enter", theme),
                    Span::styled(" focus diff", Style::default().fg(theme.muted)),
                ]),
                FocusPane::Commits => Line::from(vec![
                    key_chip("space", theme),
                    Span::styled(" select ", Style::default().fg(theme.muted)),
                    key_chip("u/r/i/s", theme),
                    Span::styled(" current ", Style::default().fg(theme.muted)),
                    key_chip("U/R/I/S", theme),
                    Span::styled(" selected", Style::default().fg(theme.muted)),
                ]),
                FocusPane::Diff => Line::from(vec![
                    key_chip("v", theme),
                    Span::styled(" range ", Style::default().fg(theme.muted)),
                    key_chip("m", theme),
                    Span::styled(" comment ", Style::default().fg(theme.muted)),
                    key_chip("/", theme),
                    Span::styled(" search ", Style::default().fg(theme.muted)),
                    key_chip("[/]", theme),
                    Span::styled(" hunks ", Style::default().fg(theme.muted)),
                    key_chip("zz/zt/zb", theme),
                    Span::styled(" scroll ", Style::default().fg(theme.muted)),
                    key_chip("e/D", theme),
                    Span::styled(" edit/delete ", Style::default().fg(theme.muted)),
                    key_chip("Ctrl-d/u", theme),
                    Span::styled(" jump", Style::default().fg(theme.muted)),
                ]),
            },
        };

        let global_line = Line::from(vec![
            key_chip("1/2/3", theme),
            Span::styled(" panes ", Style::default().fg(theme.dimmed)),
            key_chip("Tab", theme),
            Span::styled(" cycle all ", Style::default().fg(theme.dimmed)),
            key_chip("h/l", theme),
            Span::styled(" prev/next pane ", Style::default().fg(theme.dimmed)),
            key_chip("t", theme),
            Span::styled(" theme ", Style::default().fg(theme.dimmed)),
            key_chip("?", theme),
            Span::styled(" help ", Style::default().fg(theme.dimmed)),
            key_chip("q", theme),
            Span::styled(" quit", Style::default().fg(theme.dimmed)),
        ]);

        let status = match self.input_mode {
            InputMode::CommentCreate | InputMode::CommentEdit(_) => format!(
                "{} | mode={} focus={} theme={} > {}",
                self.status,
                mode,
                focus,
                self.theme_mode.label(),
                self.comment_buffer
            ),
            InputMode::DiffSearch => format!(
                "{} | mode={} focus={} theme={} /{}",
                self.status,
                mode,
                focus,
                self.theme_mode.label(),
                self.diff_search_buffer
            ),
            InputMode::Normal => format!(
                "{} | mode={} focus={} theme={}",
                self.status,
                mode,
                focus,
                self.theme_mode.label()
            ),
        };

        let chunks = ratatui::layout::Layout::default()
            .direction(ratatui::layout::Direction::Vertical)
            .constraints([
                ratatui::layout::Constraint::Length(1),
                ratatui::layout::Constraint::Length(2),
            ])
            .split(rect);

        let status_widget = Paragraph::new(status).style(Style::default().fg(theme.text));
        let hint_widget = Paragraph::new(vec![pane_line, global_line])
            .style(Style::default().fg(theme.dimmed))
            .block(
                Block::default()
                    .borders(Borders::TOP)
                    .border_style(Style::default().fg(theme.border)),
            );

        frame.render_widget(status_widget, chunks[0]);
        frame.render_widget(hint_widget, chunks[1]);
    }

    fn render_help_overlay(&self, frame: &mut Frame<'_>, theme: &UiTheme) {
        let area = centered_rect(70, 62, frame.area());
        frame.render_widget(Clear, area);

        let help_lines = vec![
            Line::from(vec![Span::styled(
                "HUNKR QUICK GUIDE",
                Style::default()
                    .fg(theme.panel_title_fg)
                    .bg(theme.panel_title_bg)
                    .add_modifier(Modifier::BOLD),
            )]),
            Line::from(""),
            Line::from(vec![
                key_chip("1/2/3", theme),
                Span::raw(" focus files/commits/diff"),
            ]),
            Line::from(vec![
                key_chip("h/l", theme),
                Span::raw(" cycle panes prev/next"),
            ]),
            Line::from(vec![key_chip("space", theme), Span::raw(" select commits")]),
            Line::from(vec![
                key_chip("v", theme),
                Span::raw(" visual select (commits or diff)"),
            ]),
            Line::from(vec![
                key_chip("u/r/i/s", theme),
                Span::raw(" set commit status"),
            ]),
            Line::from(vec![
                key_chip("m", theme),
                Span::raw(" add comment to commit/hunk/range"),
            ]),
            Line::from(vec![
                key_chip("/", theme),
                Span::raw(" diff search (Esc cancel, Enter run)"),
            ]),
            Line::from(vec![
                key_chip("n/N", theme),
                Span::raw(" repeat diff search next/prev"),
            ]),
            Line::from(vec![
                key_chip("[/]", theme),
                Span::raw(" previous/next diff hunk"),
            ]),
            Line::from(vec![
                key_chip("zz/zt/zb", theme),
                Span::raw(" center/top/bottom cursor"),
            ]),
            Line::from(vec![
                key_chip("e", theme),
                Span::raw(" edit comment under cursor"),
            ]),
            Line::from(vec![
                key_chip("D", theme),
                Span::raw(" delete comment under cursor"),
            ]),
            Line::from(vec![key_chip("t", theme), Span::raw(" toggle theme")]),
            Line::from(vec![
                key_chip("Ctrl-d/u", theme),
                Span::raw(" big jump in focused pane"),
            ]),
            Line::from(""),
            Line::from(vec![
                Span::styled("UNREVIEWED", Style::default().fg(theme.unreviewed)),
                Span::raw("  "),
                Span::styled("REVIEWED", Style::default().fg(theme.reviewed)),
                Span::raw("  "),
                Span::styled("ISSUE_FOUND", Style::default().fg(theme.issue)),
                Span::raw("  "),
                Span::styled("RESOLVED", Style::default().fg(theme.resolved)),
            ]),
            Line::from(""),
            Line::from(vec![
                Span::styled("Press ", Style::default().fg(theme.muted)),
                key_chip("?", theme),
                Span::styled(" to close", Style::default().fg(theme.muted)),
            ]),
        ];

        let widget = Paragraph::new(help_lines).block(
            Block::default()
                .title(" Help ")
                .borders(Borders::ALL)
                .border_type(BorderType::Rounded)
                .border_style(Style::default().fg(theme.focus_border)),
        );
        frame.render_widget(widget, area);
    }

    fn reload_commits(&mut self, preserve_manual_selection: bool) -> anyhow::Result<()> {
        let history = self.git.load_first_parent_history(HISTORY_LIMIT)?;
        let default_selected = self.git.default_unpushed_commit_ids()?;

        let mut old_selected = BTreeSet::new();
        if preserve_manual_selection {
            for row in &self.commits {
                if row.selected {
                    old_selected.insert(row.info.id.clone());
                }
            }
        }

        let had_existing_rows = !self.commits.is_empty();
        let mut known = BTreeSet::new();
        for row in &self.commits {
            known.insert(row.info.id.clone());
        }

        self.commits = history
            .into_iter()
            .map(|info| {
                let status = self.store.commit_status(&self.review_state, &info.id);
                let selected = if preserve_manual_selection && old_selected.contains(&info.id) {
                    true
                } else if !had_existing_rows {
                    default_selected.contains(&info.id) && status == ReviewStatus::Unreviewed
                } else {
                    false
                };
                CommitRow {
                    info,
                    selected,
                    status,
                    is_uncommitted: false,
                }
            })
            .collect();

        let uncommitted_selected =
            preserve_manual_selection && old_selected.contains(UNCOMMITTED_COMMIT_ID);
        self.commits.insert(
            0,
            CommitRow {
                info: CommitInfo {
                    short_id: UNCOMMITTED_COMMIT_SHORT.to_owned(),
                    id: UNCOMMITTED_COMMIT_ID.to_owned(),
                    summary: UNCOMMITTED_COMMIT_SUMMARY.to_owned(),
                    author: "local".to_owned(),
                    timestamp: Utc::now().timestamp(),
                    unpushed: false,
                },
                selected: uncommitted_selected,
                status: ReviewStatus::Unreviewed,
                is_uncommitted: true,
            },
        );

        if self.commit_list_state.selected().is_none() && !self.commits.is_empty() {
            self.commit_list_state.select(Some(0));
        }

        let new_commits = self
            .commits
            .iter()
            .filter(|row| {
                !row.is_uncommitted
                    && !known.contains(&row.info.id)
                    && row.status == ReviewStatus::Unreviewed
            })
            .count();
        if new_commits > 0 {
            self.status = format!("{} new unreviewed commit(s) detected", new_commits);
        }

        self.rebuild_selection_dependent_views()?;
        self.sync_comment_report()?;
        Ok(())
    }

    fn rebuild_selection_dependent_views(&mut self) -> anyhow::Result<()> {
        let selected_ordered = self.selected_commit_ids_oldest_first();
        let mut aggregate = if selected_ordered.is_empty() {
            AggregatedDiff::default()
        } else {
            self.git.aggregate_for_commits(&selected_ordered)?
        };
        if self.uncommitted_selected() {
            merge_aggregate_diff(&mut aggregate, self.git.aggregate_uncommitted()?);
        }
        self.aggregate = aggregate;
        self.reset_diff_positions_for_changed_files();

        self.rendered_diff_cache.clear();
        self.rendered_diff_key = None;
        self.diff_pending_op = None;

        self.rebuild_file_tree();
        self.ensure_selected_file_exists();
        self.ensure_rendered_diff();
        Ok(())
    }

    fn reset_diff_positions_for_changed_files(&mut self) {
        let mut next_signatures = HashMap::new();
        for (path, patch) in &self.aggregate.files {
            next_signatures.insert(path.clone(), file_patch_signature(patch));
        }

        let changed_or_removed =
            changed_or_removed_paths(&self.file_diff_signatures, &next_signatures);
        for path in changed_or_removed {
            self.diff_positions.remove(&path);
            if self.selected_file.as_ref() == Some(&path) {
                self.diff_position = DiffPosition::default();
            }
        }

        self.file_diff_signatures = next_signatures;
    }

    fn ensure_rendered_diff(&mut self) {
        let Some(path) = self.selected_file.clone() else {
            self.rendered_diff = Arc::new(Vec::new());
            self.rendered_diff_key = None;
            self.diff_position = DiffPosition::default();
            return;
        };

        let key = (path.clone(), self.theme_mode);
        if self.rendered_diff_key.as_ref() == Some(&key) {
            return;
        }

        if let Some(cached) = self.rendered_diff_cache.get(&key) {
            self.rendered_diff = cached.clone();
            self.rendered_diff_key = Some(key);
            self.sync_diff_cursor_to_content_bounds();
            return;
        }

        let rendered = self
            .aggregate
            .files
            .get(&path)
            .map(|patch| Arc::new(self.build_diff_lines(patch)))
            .unwrap_or_else(|| Arc::new(Vec::new()));

        self.rendered_diff_cache
            .insert(key.clone(), rendered.clone());
        self.rendered_diff = rendered;
        self.rendered_diff_key = Some(key);
        self.sync_diff_cursor_to_content_bounds();
    }

    fn sync_diff_cursor_to_content_bounds(&mut self) {
        if self.rendered_diff.is_empty() {
            self.diff_position = DiffPosition::default();
            return;
        }

        if self.diff_position.cursor >= self.rendered_diff.len() {
            self.diff_position.cursor = self.rendered_diff.len() - 1;
        }
        self.sync_diff_visual_bounds();

        self.ensure_cursor_visible();
    }

    fn invalidate_diff_cache(&mut self) {
        self.rendered_diff_cache.clear();
        self.rendered_diff_key = None;
        self.ensure_rendered_diff();
    }

    fn current_comment_id(&self) -> Option<u64> {
        self.rendered_diff
            .get(self.diff_position.cursor)
            .and_then(|line| line.comment_id)
    }

    fn build_diff_lines(&self, patch: &FilePatch) -> Vec<RenderedDiffLine> {
        let mut rendered = Vec::new();
        let theme = UiTheme::from_mode(self.theme_mode);
        let now_ts = Utc::now().timestamp();
        let file_comments: Vec<&ReviewComment> = self
            .comments
            .comments()
            .iter()
            .filter(|comment| comment.target.end.file_path == patch.path)
            .collect();
        let mut last_commit_banner: Option<String> = None;
        let mut inserted_commit_comments = BTreeSet::new();

        for hunk in &patch.hunks {
            let commit_anchor = CommentAnchor {
                commit_id: hunk.commit_id.clone(),
                commit_summary: hunk.commit_summary.clone(),
                file_path: patch.path.clone(),
                hunk_header: COMMIT_ANCHOR_HEADER.to_owned(),
                old_lineno: None,
                new_lineno: None,
            };
            if should_render_commit_banner(last_commit_banner.as_deref(), &hunk.commit_id) {
                let age = format_relative_time(hunk.commit_timestamp, now_ts);
                let commit_line = format!(
                    "---- commit {} {} ({})",
                    hunk.commit_short, hunk.commit_summary, age
                );
                rendered.push(RenderedDiffLine {
                    line: Line::from(vec![
                        Span::styled("---- ", Style::default().fg(theme.dimmed)),
                        Span::styled("commit ", Style::default().fg(theme.muted)),
                        Span::styled(
                            hunk.commit_short.clone(),
                            Style::default()
                                .fg(theme.focus_border)
                                .add_modifier(Modifier::BOLD),
                        ),
                        Span::raw(" "),
                        Span::styled(hunk.commit_summary.clone(), Style::default().fg(theme.text)),
                        Span::raw(" "),
                        Span::styled(format!("({})", age), Style::default().fg(theme.dimmed)),
                    ]),
                    raw_text: commit_line,
                    anchor: Some(commit_anchor.clone()),
                    comment_id: None,
                });

                let mut commit_comments: Vec<&ReviewComment> = file_comments
                    .iter()
                    .copied()
                    .filter(|comment| {
                        comment_targets_commit_end(comment, &patch.path, &hunk.commit_id)
                    })
                    .collect();
                commit_comments.sort_by_key(|comment| comment.id);
                for comment in commit_comments {
                    if inserted_commit_comments.insert(comment.id) {
                        push_comment_lines(&mut rendered, comment, &theme, now_ts);
                    }
                }
            }
            last_commit_banner = Some(hunk.commit_id.clone());

            let mut hunk_comments: Vec<&ReviewComment> = file_comments
                .iter()
                .copied()
                .filter(|comment| {
                    comment_targets_hunk_end(comment, &patch.path, &hunk.commit_id, &hunk.header)
                })
                .collect();
            hunk_comments.sort_by_key(|comment| comment.id);
            let mut injected_comment_ids = BTreeSet::new();

            let hunk_anchor = CommentAnchor {
                commit_id: hunk.commit_id.clone(),
                commit_summary: hunk.commit_summary.clone(),
                file_path: patch.path.clone(),
                hunk_header: hunk.header.clone(),
                old_lineno: Some(hunk.old_start),
                new_lineno: Some(hunk.new_start),
            };
            let hunk_label = format!("@@ {}", hunk.header);
            rendered.push(RenderedDiffLine {
                line: Line::from(vec![
                    Span::styled("@@ ", Style::default().fg(theme.muted)),
                    Span::styled(hunk.header.clone(), Style::default().fg(theme.diff_header)),
                ]),
                raw_text: hunk_label,
                anchor: Some(CommentAnchor {
                    commit_id: hunk.commit_id.clone(),
                    commit_summary: hunk.commit_summary.clone(),
                    file_path: patch.path.clone(),
                    hunk_header: hunk.header.clone(),
                    old_lineno: Some(hunk.old_start),
                    new_lineno: Some(hunk.new_start),
                }),
                comment_id: None,
            });
            push_comment_lines_for_anchor(
                &mut rendered,
                &hunk_comments,
                &mut injected_comment_ids,
                &hunk_anchor,
                &theme,
                now_ts,
            );

            for line in &hunk.lines {
                let anchor = CommentAnchor {
                    commit_id: hunk.commit_id.clone(),
                    commit_summary: hunk.commit_summary.clone(),
                    file_path: patch.path.clone(),
                    hunk_header: hunk.header.clone(),
                    old_lineno: line.old_lineno,
                    new_lineno: line.new_lineno,
                };
                rendered.push(RenderedDiffLine {
                    line: self.render_code_line(&patch.path, line, &theme),
                    raw_text: raw_diff_text(line),
                    anchor: Some(anchor.clone()),
                    comment_id: None,
                });
                push_comment_lines_for_anchor(
                    &mut rendered,
                    &hunk_comments,
                    &mut injected_comment_ids,
                    &anchor,
                    &theme,
                    now_ts,
                );
            }

            for comment in hunk_comments {
                if injected_comment_ids.insert(comment.id) {
                    push_comment_lines(&mut rendered, comment, &theme, now_ts);
                }
            }

            rendered.push(RenderedDiffLine {
                line: Line::from(""),
                raw_text: String::new(),
                anchor: None,
                comment_id: None,
            });
        }

        rendered
    }

    fn render_code_line(&self, path: &str, line: &HunkLine, theme: &UiTheme) -> Line<'static> {
        let (prefix, accent, bg) = match line.kind {
            DiffLineKind::Add => ('+', theme.diff_add, Some(theme.diff_add_bg)),
            DiffLineKind::Remove => ('-', theme.diff_remove, Some(theme.diff_remove_bg)),
            DiffLineKind::Context => (' ', theme.dimmed, None),
            DiffLineKind::Meta => ('~', theme.diff_meta, None),
        };

        let old_col = line
            .old_lineno
            .map(|n| format!("{:>4}", n))
            .unwrap_or_else(|| "    ".to_owned());
        let new_col = line
            .new_lineno
            .map(|n| format!("{:>4}", n))
            .unwrap_or_else(|| "    ".to_owned());

        let mut spans = vec![
            Span::styled(
                format!("{} {} ", old_col, new_col),
                Style::default().fg(theme.dimmed),
            ),
            Span::styled(
                prefix.to_string(),
                Style::default().fg(accent).add_modifier(Modifier::BOLD),
            ),
            Span::raw(" "),
        ];

        let highlighted = self
            .highlighter
            .highlight(self.theme_mode, path, &line.text);
        for mut span in highlighted {
            if let Some(bg_color) = bg {
                span.style = span.style.bg(bg_color);
            }
            spans.push(span);
        }

        Line::from(spans)
    }

    fn rebuild_file_tree(&mut self) {
        let mut tree = FileTree::default();
        for (path, patch) in &self.aggregate.files {
            let modified_ts = patch
                .hunks
                .iter()
                .map(|h| h.commit_timestamp)
                .max()
                .unwrap_or(0);
            tree.insert(path, modified_ts);
        }

        self.file_rows = tree.flattened_rows();
        if self.file_rows.is_empty() {
            self.file_list_state.select(None);
            self.selected_file = None;
            return;
        }

        if self.file_list_state.selected().is_none() {
            self.select_first_file();
        }
    }

    fn ensure_selected_file_exists(&mut self) {
        if self.file_rows.is_empty() {
            self.selected_file = None;
            self.file_list_state.select(None);
            return;
        }

        if let Some(path) = self.selected_file.clone()
            && let Some(idx) = self
                .file_rows
                .iter()
                .position(|row| row.selectable && row.path.as_ref() == Some(&path))
        {
            self.file_list_state.select(Some(idx));
            self.restore_diff_position(&path);
            return;
        }

        self.select_first_file();
    }

    fn on_selection_changed(&mut self) {
        if let Err(err) = self.rebuild_selection_dependent_views() {
            self.status = format!("failed to rebuild diff: {err:#}");
        } else {
            let selected = self.commits.iter().filter(|row| row.selected).count();
            self.status = format!("{} commit(s) selected", selected);
        }
    }

    fn selected_commit_ids_oldest_first(&self) -> Vec<String> {
        selected_ids_oldest_first(&self.commits)
    }

    fn move_file_cursor(&mut self, delta: isize) {
        if self.file_rows.is_empty() {
            return;
        }

        let mut idx = self.file_list_state.selected().unwrap_or(0) as isize;
        let len = self.file_rows.len() as isize;
        loop {
            idx = (idx + delta).clamp(0, len - 1);
            if self.file_rows[idx as usize].selectable || idx == 0 || idx == len - 1 {
                break;
            }
            if (delta > 0 && idx == len - 1) || (delta < 0 && idx == 0) {
                break;
            }
        }

        self.select_file_row(idx as usize);
    }

    fn page_files(&mut self, multiplier: f32) {
        let step = page_step(self.pane_rects.files.height, multiplier);
        self.move_file_cursor(step);
    }

    fn select_first_file(&mut self) {
        if let Some(idx) = self.file_rows.iter().position(|row| row.selectable) {
            self.select_file_row(idx);
        }
    }

    fn select_last_file(&mut self) {
        if let Some(idx) = self.file_rows.iter().rposition(|row| row.selectable) {
            self.select_file_row(idx);
        }
    }

    fn select_file_row(&mut self, idx: usize) {
        if idx >= self.file_rows.len() || !self.file_rows[idx].selectable {
            return;
        }

        if let Some(prev) = &self.selected_file {
            self.diff_positions.insert(prev.clone(), self.diff_position);
        }

        self.file_list_state.select(Some(idx));
        let path = self.file_rows[idx]
            .path
            .clone()
            .expect("selectable rows always contain path");
        self.selected_file = Some(path.clone());
        self.restore_diff_position(&path);
        self.ensure_rendered_diff();
    }

    fn move_commit_cursor(&mut self, delta: isize) {
        if self.commits.is_empty() {
            return;
        }
        let len = self.commits.len() as isize;
        let current = self.commit_list_state.selected().unwrap_or(0) as isize;
        let next = (current + delta).clamp(0, len - 1) as usize;
        self.commit_list_state.select(Some(next));

        if self.commit_visual_anchor.is_some() {
            self.apply_commit_visual_range();
        }
    }

    fn page_commits(&mut self, multiplier: f32) {
        let step = page_step(self.pane_rects.commits.height, multiplier);
        self.move_commit_cursor(step);
    }

    fn select_first_commit(&mut self) {
        if self.commits.is_empty() {
            return;
        }
        self.commit_list_state.select(Some(0));
        if self.commit_visual_anchor.is_some() {
            self.apply_commit_visual_range();
        }
    }

    fn select_last_commit(&mut self) {
        if self.commits.is_empty() {
            return;
        }
        self.commit_list_state.select(Some(self.commits.len() - 1));
        if self.commit_visual_anchor.is_some() {
            self.apply_commit_visual_range();
        }
    }

    fn select_commit_row(&mut self, idx: usize, toggle: bool) {
        if idx >= self.commits.len() {
            return;
        }
        self.commit_list_state.select(Some(idx));
        if toggle && let Some(row) = self.commits.get_mut(idx) {
            row.selected = !row.selected;
            self.on_selection_changed();
        }
    }

    fn apply_commit_visual_range(&mut self) {
        let Some(anchor) = self.commit_visual_anchor else {
            return;
        };
        let Some(cursor) = self.commit_list_state.selected() else {
            return;
        };

        let start = min(anchor, cursor);
        let end = max(anchor, cursor);
        apply_range_selection(&mut self.commits, start, end);
        self.on_selection_changed();
    }

    fn set_current_commit_status(&mut self, status: ReviewStatus) {
        let Some(idx) = self.commit_list_state.selected() else {
            return;
        };
        let Some(row) = self.commits.get(idx) else {
            return;
        };
        if row.is_uncommitted {
            self.status = "Cannot set review status for uncommitted changes".to_owned();
            return;
        }
        let ids = BTreeSet::from([row.info.id.clone()]);
        self.set_status_for_ids(&ids, status);
    }

    fn set_selected_commit_status(&mut self, status: ReviewStatus) {
        let ids = self
            .commits
            .iter()
            .filter(|row| row.selected && !row.is_uncommitted)
            .map(|row| row.info.id.clone())
            .collect::<BTreeSet<_>>();
        if ids.is_empty() {
            self.status = "No selected committed revisions".to_owned();
            return;
        }
        self.set_status_for_ids(&ids, status);
    }

    fn set_status_for_ids(&mut self, ids: &BTreeSet<String>, status: ReviewStatus) {
        self.store.set_many_status(
            &mut self.review_state,
            ids.iter().cloned(),
            status,
            self.git.branch_name(),
        );

        apply_status_transition(&mut self.commits, ids, status);

        let save_result = self.store.save(&self.review_state);
        let mut status_message = if let Err(err) = save_result {
            format!("failed to persist status change: {err:#}")
        } else {
            format!("{} commit(s) -> {}", ids.len(), status.as_str())
        };

        if status != ReviewStatus::Unreviewed {
            self.commit_visual_anchor = None;
        }
        if let Err(err) = self.rebuild_selection_dependent_views() {
            self.status = format!("failed to rebuild diff: {err:#}");
            return;
        }
        if let Err(err) = self.sync_comment_report() {
            status_message.push_str(&format!(", review tasks sync failed: {err:#}"));
        }
        self.status = status_message;
    }

    fn move_diff_cursor(&mut self, delta: isize) {
        if self.rendered_diff.is_empty() {
            return;
        }
        let len = self.rendered_diff.len() as isize;
        let next = (self.diff_position.cursor as isize + delta).clamp(0, len - 1) as usize;
        self.diff_position.cursor = next;
        self.ensure_cursor_visible();
    }

    fn set_diff_cursor(&mut self, absolute_row: usize) {
        if self.rendered_diff.is_empty() {
            self.diff_position = DiffPosition::default();
            return;
        }
        self.diff_position.cursor = absolute_row.min(self.rendered_diff.len() - 1);
        self.ensure_cursor_visible();
    }

    fn page_diff(&mut self, multiplier: f32) {
        let step = page_step(self.pane_rects.diff.height, multiplier);
        self.move_diff_cursor(step);
    }

    fn align_diff_cursor_top(&mut self) {
        if self.rendered_diff.is_empty() {
            return;
        }
        self.set_diff_scroll(self.diff_position.cursor);
        self.status = "zt".to_owned();
    }

    fn align_diff_cursor_middle(&mut self) {
        if self.rendered_diff.is_empty() {
            return;
        }
        let visible = self.visible_diff_rows();
        let scroll = self.diff_position.cursor.saturating_sub(visible / 2);
        self.set_diff_scroll(scroll);
        self.status = "zz".to_owned();
    }

    fn align_diff_cursor_bottom(&mut self) {
        if self.rendered_diff.is_empty() {
            return;
        }
        let visible = self.visible_diff_rows();
        let scroll = self
            .diff_position
            .cursor
            .saturating_sub(visible.saturating_sub(1));
        self.set_diff_scroll(scroll);
        self.status = "zb".to_owned();
    }

    fn move_prev_hunk(&mut self) {
        if self.rendered_diff.is_empty() {
            return;
        }
        for idx in (0..self.diff_position.cursor).rev() {
            if is_hunk_header_line(&self.rendered_diff[idx]) {
                self.set_diff_cursor(idx);
                self.status = format!("hunk {}", idx.saturating_add(1));
                return;
            }
        }
        self.status = "No previous hunk".to_owned();
    }

    fn move_next_hunk(&mut self) {
        if self.rendered_diff.is_empty() {
            return;
        }
        for idx in self.diff_position.cursor.saturating_add(1)..self.rendered_diff.len() {
            if is_hunk_header_line(&self.rendered_diff[idx]) {
                self.set_diff_cursor(idx);
                self.status = format!("hunk {}", idx.saturating_add(1));
                return;
            }
        }
        self.status = "No next hunk".to_owned();
    }

    fn visible_diff_rows(&self) -> usize {
        self.pane_rects.diff.height.saturating_sub(2).max(1) as usize
    }

    fn max_diff_scroll(&self) -> usize {
        self.rendered_diff
            .len()
            .saturating_sub(self.visible_diff_rows().min(self.rendered_diff.len()))
    }

    fn set_diff_scroll(&mut self, scroll: usize) {
        self.diff_position.scroll = scroll.min(self.max_diff_scroll());
        if let Some(file) = &self.selected_file {
            self.diff_positions.insert(file.clone(), self.diff_position);
        }
    }

    fn ensure_cursor_visible(&mut self) {
        let visible = self.visible_diff_rows();

        if self.diff_position.cursor < self.diff_position.scroll {
            self.diff_position.scroll = self.diff_position.cursor;
        } else if self.diff_position.cursor >= self.diff_position.scroll + visible {
            self.diff_position.scroll = self.diff_position.cursor + 1 - visible;
        }

        if let Some(file) = &self.selected_file {
            self.diff_positions.insert(file.clone(), self.diff_position);
        }
    }

    fn restore_diff_position(&mut self, path: &str) {
        self.diff_position = self.diff_positions.get(path).copied().unwrap_or_default();
    }

    fn sync_diff_visual_bounds(&mut self) {
        let Some(visual) = self.diff_visual else {
            return;
        };
        if self.rendered_diff.is_empty() {
            return;
        }
        let max_idx = self.rendered_diff.len() - 1;
        let clamped_anchor = visual.anchor.min(max_idx);
        if clamped_anchor != visual.anchor {
            self.diff_visual = Some(DiffVisualSelection {
                anchor: clamped_anchor,
            });
        }
    }

    fn focus_next(&mut self) {
        self.focused = match self.focused {
            FocusPane::Files => FocusPane::Commits,
            FocusPane::Commits => FocusPane::Diff,
            FocusPane::Diff => FocusPane::Files,
        }
    }

    fn focus_prev(&mut self) {
        self.focused = match self.focused {
            FocusPane::Files => FocusPane::Diff,
            FocusPane::Commits => FocusPane::Files,
            FocusPane::Diff => FocusPane::Commits,
        }
    }

    fn diff_selected_range(&self) -> Option<(usize, usize)> {
        if self.rendered_diff.is_empty() {
            return None;
        }
        let max_idx = self.rendered_diff.len() - 1;
        let cursor = self.diff_position.cursor.min(max_idx);

        if let Some(visual) = self.diff_visual {
            let anchor = visual.anchor.min(max_idx);
            Some((min(anchor, cursor), max(anchor, cursor)))
        } else {
            Some((cursor, cursor))
        }
    }

    fn comment_target_from_selection(&self) -> Option<CommentTarget> {
        let (start_idx, end_idx) = self.diff_selected_range()?;
        let mut commit_anchors = Vec::new();
        let mut hunk_anchors = Vec::new();
        let mut commit_lines = Vec::new();
        let mut hunk_lines = Vec::new();

        for idx in start_idx..=end_idx {
            let Some(line) = self.rendered_diff.get(idx) else {
                continue;
            };
            if let Some(anchor) = &line.anchor {
                if is_commit_anchor(anchor) {
                    commit_anchors.push(anchor.clone());
                    if !line.raw_text.trim().is_empty() {
                        commit_lines.push(line.raw_text.clone());
                    }
                } else {
                    hunk_anchors.push(anchor.clone());
                    if !line.raw_text.trim().is_empty() {
                        hunk_lines.push(line.raw_text.clone());
                    }
                }
            }
        }

        if hunk_anchors.is_empty() && commit_anchors.is_empty() {
            return None;
        }

        if hunk_anchors.is_empty() {
            let anchor = commit_anchors.last()?.clone();
            return Some(CommentTarget {
                kind: CommentTargetKind::Commit,
                start: anchor.clone(),
                end: anchor.clone(),
                commits: BTreeSet::from([anchor.commit_id.clone()]),
                selected_lines: if commit_lines.is_empty() {
                    Vec::new()
                } else {
                    vec![commit_lines.last()?.clone()]
                },
            });
        }

        let start = hunk_anchors.first()?.clone();
        let end = hunk_anchors.last()?.clone();
        let commits = hunk_anchors
            .iter()
            .map(|anchor| anchor.commit_id.clone())
            .collect::<BTreeSet<_>>();

        Some(CommentTarget {
            kind: CommentTargetKind::Hunk,
            start,
            end,
            commits,
            selected_lines: hunk_lines,
        })
    }

    fn status_counts(&self) -> (usize, usize, usize, usize) {
        let mut unreviewed = 0;
        let mut reviewed = 0;
        let mut issue_found = 0;
        let mut resolved = 0;
        for row in &self.commits {
            if row.is_uncommitted {
                continue;
            }
            match row.status {
                ReviewStatus::Unreviewed => unreviewed += 1,
                ReviewStatus::Reviewed => reviewed += 1,
                ReviewStatus::IssueFound => issue_found += 1,
                ReviewStatus::Resolved => resolved += 1,
            }
        }
        (unreviewed, reviewed, issue_found, resolved)
    }

    fn uncommitted_selected(&self) -> bool {
        self.commits
            .iter()
            .any(|row| row.is_uncommitted && row.selected)
    }

    fn sync_comment_report(&self) -> anyhow::Result<()> {
        self.comments.sync_review_tasks_report(|commit_id| {
            self.store.commit_status(&self.review_state, commit_id)
        })?;
        Ok(())
    }
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

fn tint_line_background(line: &Line<'static>, tint: Color, blend_existing: bool) -> Line<'static> {
    let mut patched = line.clone();
    for span in &mut patched.spans {
        let bg = if blend_existing {
            span.style
                .bg
                .map(|existing| blend_colors(existing, tint, 170))
                .unwrap_or(tint)
        } else {
            tint
        };
        span.style = span.style.patch(Style::default().bg(bg));
    }
    patched
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

fn is_hunk_header_line(line: &RenderedDiffLine) -> bool {
    line.raw_text.starts_with("@@ ")
        && line
            .anchor
            .as_ref()
            .is_some_and(|anchor| !is_commit_anchor(anchor))
}

fn find_diff_match_from_cursor(
    lines: &[RenderedDiffLine],
    query: &str,
    forward: bool,
    cursor: usize,
) -> Option<usize> {
    if lines.is_empty() {
        return None;
    }
    let query = query.to_ascii_lowercase();
    if query.is_empty() {
        return None;
    }

    let current = cursor.min(lines.len().saturating_sub(1));
    if forward {
        for idx in current.saturating_add(1)..lines.len() {
            if lines[idx].raw_text.to_ascii_lowercase().contains(&query) {
                return Some(idx);
            }
        }
        for idx in 0..=current {
            if lines[idx].raw_text.to_ascii_lowercase().contains(&query) {
                return Some(idx);
            }
        }
    } else {
        for idx in (0..current).rev() {
            if lines[idx].raw_text.to_ascii_lowercase().contains(&query) {
                return Some(idx);
            }
        }
        for idx in (current..lines.len()).rev() {
            if lines[idx].raw_text.to_ascii_lowercase().contains(&query) {
                return Some(idx);
            }
        }
    }
    None
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

fn line_with_right(
    left: String,
    left_style: Style,
    right: String,
    right_style: Style,
    width: usize,
) -> Line<'static> {
    if right.is_empty() {
        return Line::from(Span::styled(truncate(&left, width.max(1)), left_style));
    }
    let right_width = right.chars().count();
    if right_width + 1 >= width {
        return Line::from(Span::styled(truncate(&right, width.max(1)), right_style));
    }

    let max_left = width - right_width - 1;
    let left_render = truncate(&left, max_left.max(1));
    let left_width = left_render.chars().count();
    let spaces = if left_width + right_width + 1 >= width {
        " ".to_owned()
    } else {
        " ".repeat(width - left_width - right_width)
    };

    Line::from(vec![
        Span::styled(left_render, left_style),
        Span::raw(spaces),
        Span::styled(right, right_style),
    ])
}

fn list_content_width(rect_width: u16) -> usize {
    rect_width
        .saturating_sub(2 + LIST_HIGHLIGHT_SYMBOL_WIDTH)
        .max(1) as usize
}

fn scrollbar_thumb(total: usize, viewport: usize, scroll: usize) -> (usize, usize) {
    if viewport == 0 {
        return (0, 0);
    }
    if total <= viewport {
        return (0, viewport);
    }

    let max_scroll = total - viewport;
    let clamped_scroll = scroll.min(max_scroll);
    let thumb_len = ((viewport * viewport) / total).clamp(1, viewport);
    let track_len = viewport - thumb_len;
    let thumb_start = if max_scroll == 0 {
        0
    } else {
        (clamped_scroll * track_len) / max_scroll
    };
    (thumb_start, thumb_len)
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

fn status_style(status: ReviewStatus, theme: &UiTheme) -> Style {
    match status {
        ReviewStatus::Unreviewed => Style::default()
            .fg(theme.unreviewed)
            .add_modifier(Modifier::BOLD),
        ReviewStatus::Reviewed => Style::default().fg(theme.reviewed),
        ReviewStatus::IssueFound => Style::default()
            .fg(theme.issue)
            .add_modifier(Modifier::BOLD),
        ReviewStatus::Resolved => Style::default().fg(theme.resolved),
    }
}

fn compose_commit_line(
    row: &CommitRow,
    width: usize,
    now_ts: i64,
    theme: &UiTheme,
) -> Line<'static> {
    if row.is_uncommitted {
        let marker = if row.selected { "[x]" } else { "[ ]" };
        let left = format!("{marker} {} {}", row.info.short_id, row.info.summary);
        let badge = "[UNCOMMITTED]";
        let right = "draft";
        let reserved = 1 + badge.chars().count() + 1 + right.chars().count();
        let max_left = width.saturating_sub(reserved).max(1);
        let left_render = truncate(&left, max_left);
        let static_used =
            left_render.chars().count() + badge.chars().count() + right.chars().count() + 1;
        let spaces = if static_used >= width {
            " ".to_owned()
        } else {
            " ".repeat(width - static_used)
        };

        return Line::from(vec![
            Span::styled(left_render, Style::default().fg(theme.text)),
            Span::raw(" "),
            Span::styled(
                badge,
                Style::default()
                    .fg(theme.accent)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::raw(spaces),
            Span::styled(right, Style::default().fg(theme.dimmed)),
        ]);
    }

    let marker = if row.selected { "[x]" } else { "[ ]" };
    let left = format!("{} {} {}", marker, row.info.short_id, row.info.summary);
    let status_label = format!("[{}]", status_short_label(row.status));
    let unpushed = if row.info.unpushed { " [^]" } else { "" };
    let right = format_relative_time(row.info.timestamp, now_ts);
    let reserved =
        1 + status_label.chars().count() + unpushed.chars().count() + 1 + right.chars().count();
    let max_left = width.saturating_sub(reserved).max(1);
    let left_render = truncate(&left, max_left);
    let static_used = left_render.chars().count()
        + status_label.chars().count()
        + unpushed.chars().count()
        + right.chars().count()
        + 1;
    let spaces = if static_used >= width {
        " ".to_owned()
    } else {
        " ".repeat(width - static_used)
    };

    Line::from(vec![
        Span::styled(left_render, Style::default().fg(theme.text)),
        Span::raw(" "),
        Span::styled(status_label, status_style(row.status, theme)),
        Span::styled(unpushed.to_owned(), Style::default().fg(theme.unpushed)),
        Span::raw(spaces),
        Span::styled(right, Style::default().fg(theme.dimmed)),
    ])
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

fn changed_or_removed_paths(
    previous: &HashMap<String, u64>,
    next: &HashMap<String, u64>,
) -> BTreeSet<String> {
    let mut changed = BTreeSet::new();
    for (path, prev_sig) in previous {
        if next.get(path).copied() != Some(*prev_sig) {
            changed.insert(path.clone());
        }
    }
    changed
}

fn file_patch_signature(patch: &FilePatch) -> u64 {
    let mut h = 1_469_598_103_934_665_603_u64;
    h = hash_str(h, &patch.path);
    for hunk in &patch.hunks {
        h = hash_str(h, &hunk.commit_id);
        h = hash_str(h, &hunk.commit_short);
        h = hash_str(h, &hunk.commit_summary);
        h = hash_i64(h, hunk.commit_timestamp);
        h = hash_str(h, &hunk.header);
        h = hash_u32(h, hunk.old_start);
        h = hash_u32(h, hunk.new_start);
        for line in &hunk.lines {
            h = hash_u8(h, diff_line_kind_code(line.kind));
            h = hash_str(h, &line.text);
            h = hash_option_u32(h, line.old_lineno);
            h = hash_option_u32(h, line.new_lineno);
        }
    }
    h
}

fn diff_line_kind_code(kind: DiffLineKind) -> u8 {
    match kind {
        DiffLineKind::Context => 0,
        DiffLineKind::Add => 1,
        DiffLineKind::Remove => 2,
        DiffLineKind::Meta => 3,
    }
}

fn hash_u8(seed: u64, value: u8) -> u64 {
    let next = seed ^ u64::from(value);
    next.wrapping_mul(1_099_511_628_211)
}

fn hash_u32(seed: u64, value: u32) -> u64 {
    hash_bytes(seed, &value.to_le_bytes())
}

fn hash_i64(seed: u64, value: i64) -> u64 {
    hash_bytes(seed, &value.to_le_bytes())
}

fn hash_option_u32(seed: u64, value: Option<u32>) -> u64 {
    match value {
        Some(v) => hash_u32(hash_u8(seed, 1), v),
        None => hash_u8(seed, 0),
    }
}

fn hash_str(seed: u64, value: &str) -> u64 {
    hash_bytes(seed, value.as_bytes())
}

fn hash_bytes(mut seed: u64, bytes: &[u8]) -> u64 {
    for byte in bytes {
        seed = hash_u8(seed, *byte);
    }
    seed
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

fn focus_with_h(current: FocusPane) -> FocusPane {
    match current {
        FocusPane::Files => FocusPane::Diff,
        FocusPane::Commits => FocusPane::Files,
        FocusPane::Diff => FocusPane::Commits,
    }
}

fn focus_with_l(current: FocusPane) -> FocusPane {
    match current {
        FocusPane::Files => FocusPane::Commits,
        FocusPane::Commits => FocusPane::Diff,
        FocusPane::Diff => FocusPane::Files,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn commit_row(id: &str, selected: bool, status: ReviewStatus) -> CommitRow {
        CommitRow {
            info: CommitInfo {
                id: id.to_owned(),
                short_id: id.chars().take(7).collect(),
                summary: format!("summary-{id}"),
                author: "dev".to_owned(),
                timestamp: 0,
                unpushed: true,
            },
            selected,
            status,
            is_uncommitted: false,
        }
    }

    fn sample_comment(start: CommentAnchor, end: CommentAnchor, text: &str) -> ReviewComment {
        ReviewComment {
            id: 7,
            target: CommentTarget {
                kind: CommentTargetKind::Hunk,
                start,
                end,
                commits: BTreeSet::from(["abc".to_owned()]),
                selected_lines: vec!["+x".to_owned()],
            },
            text: text.to_owned(),
            created_at: "2026-01-01T00:00:00Z".to_owned(),
            updated_at: "2026-01-01T00:00:00Z".to_owned(),
        }
    }

    fn sample_commit_comment(anchor: CommentAnchor, text: &str) -> ReviewComment {
        ReviewComment {
            id: 9,
            target: CommentTarget {
                kind: CommentTargetKind::Commit,
                start: anchor.clone(),
                end: anchor.clone(),
                commits: BTreeSet::from([anchor.commit_id.clone()]),
                selected_lines: vec!["---- commit abc1234 add parser (1m ago)".to_owned()],
            },
            text: text.to_owned(),
            created_at: "2026-01-01T00:00:00Z".to_owned(),
            updated_at: "2026-01-01T00:00:00Z".to_owned(),
        }
    }

    fn sample_file_patch(line_text: &str) -> FilePatch {
        FilePatch {
            path: "src/lib.rs".to_owned(),
            hunks: vec![crate::model::Hunk {
                commit_id: "abc1234".to_owned(),
                commit_short: "abc1234".to_owned(),
                commit_summary: "summary".to_owned(),
                commit_timestamp: 123,
                header: "@@ -1,1 +1,1 @@".to_owned(),
                old_start: 1,
                new_start: 1,
                lines: vec![HunkLine {
                    kind: DiffLineKind::Add,
                    text: line_text.to_owned(),
                    old_lineno: None,
                    new_lineno: Some(1),
                }],
            }],
        }
    }

    #[test]
    fn truncate_short_strings_unchanged() {
        assert_eq!(truncate("abc", 4), "abc");
    }

    #[test]
    fn truncate_long_strings_adds_ellipsis() {
        assert_eq!(truncate("abcdef", 4), "abc…");
    }

    #[test]
    fn file_tree_builds_directories_and_files() {
        let mut tree = FileTree::default();
        tree.insert("src/app.rs", 100);
        tree.insert("src/ui/view.rs", 200);
        let rows = tree.flattened_rows();

        assert!(rows.iter().any(|r| r.label.contains("[D] src")));
        assert!(rows.iter().any(|r| r.label.contains("[F] app.rs")));
        assert!(rows.iter().any(|r| r.label.contains("[D] ui")));
        assert!(rows.iter().any(|r| r.label.contains("[F] view.rs")));
    }

    #[test]
    fn list_index_skips_border_rows() {
        let rect = ratatui::layout::Rect::new(0, 0, 10, 6);
        assert_eq!(list_index_at(0, rect, 3), None);
        assert_eq!(list_index_at(5, rect, 3), None);
        assert_eq!(list_index_at(1, rect, 3), Some(3));
    }

    #[test]
    fn list_content_width_accounts_for_border_and_highlight_symbol() {
        assert_eq!(list_content_width(20), 15);
        assert_eq!(list_content_width(4), 1);
    }

    #[test]
    fn scrollbar_thumb_fills_viewport_when_content_fits() {
        assert_eq!(scrollbar_thumb(10, 20, 0), (0, 20));
    }

    #[test]
    fn scrollbar_thumb_moves_from_top_to_bottom() {
        let (start_top, len) = scrollbar_thumb(100, 20, 0);
        let (start_bottom, len_bottom) = scrollbar_thumb(100, 20, 80);
        assert_eq!(len, 4);
        assert_eq!(len_bottom, 4);
        assert_eq!(start_top, 0);
        assert_eq!(start_bottom, 16);
    }

    #[test]
    fn file_patch_signature_changes_when_patch_content_changes() {
        let sig_a = file_patch_signature(&sample_file_patch("+let a = 1;"));
        let sig_b = file_patch_signature(&sample_file_patch("+let a = 2;"));
        assert_ne!(sig_a, sig_b);
    }

    #[test]
    fn changed_or_removed_paths_detects_updates_and_removals() {
        let previous = HashMap::from([
            ("a.rs".to_owned(), 10_u64),
            ("b.rs".to_owned(), 20_u64),
            ("c.rs".to_owned(), 30_u64),
        ]);
        let next = HashMap::from([("a.rs".to_owned(), 10_u64), ("b.rs".to_owned(), 99_u64)]);

        let changed = changed_or_removed_paths(&previous, &next);

        assert_eq!(
            changed,
            BTreeSet::from(["b.rs".to_owned(), "c.rs".to_owned()])
        );
    }

    #[test]
    fn contains_checks_bounds() {
        let rect = ratatui::layout::Rect::new(5, 5, 4, 3);
        assert!(contains(rect, 5, 5));
        assert!(contains(rect, 8, 7));
        assert!(!contains(rect, 9, 7));
        assert!(!contains(rect, 4, 5));
    }

    #[test]
    fn selected_ids_are_reported_oldest_first() {
        let rows = vec![
            commit_row("newest", true, ReviewStatus::Unreviewed),
            commit_row("middle", false, ReviewStatus::Reviewed),
            commit_row("oldest", true, ReviewStatus::Unreviewed),
        ];
        assert_eq!(
            selected_ids_oldest_first(&rows),
            vec!["oldest".to_owned(), "newest".to_owned()]
        );
    }

    #[test]
    fn selected_ids_skip_uncommitted_entry() {
        let mut rows = vec![
            commit_row("newest", true, ReviewStatus::Unreviewed),
            commit_row("oldest", true, ReviewStatus::Unreviewed),
        ];
        rows.insert(
            0,
            CommitRow {
                info: CommitInfo {
                    id: UNCOMMITTED_COMMIT_ID.to_owned(),
                    short_id: UNCOMMITTED_COMMIT_SHORT.to_owned(),
                    summary: UNCOMMITTED_COMMIT_SUMMARY.to_owned(),
                    author: "local".to_owned(),
                    timestamp: 0,
                    unpushed: false,
                },
                selected: true,
                status: ReviewStatus::Unreviewed,
                is_uncommitted: true,
            },
        );

        assert_eq!(
            selected_ids_oldest_first(&rows),
            vec!["oldest".to_owned(), "newest".to_owned()]
        );
    }

    #[test]
    fn range_selection_handles_reverse_bounds() {
        let mut rows = vec![
            commit_row("a", false, ReviewStatus::Unreviewed),
            commit_row("b", false, ReviewStatus::Reviewed),
            commit_row("c", false, ReviewStatus::Unreviewed),
        ];
        apply_range_selection(&mut rows, 2, 0);
        assert!(rows.iter().all(|row| row.selected));
    }

    #[test]
    fn select_only_index_keeps_only_target_selected() {
        let mut rows = vec![
            commit_row("a", true, ReviewStatus::Unreviewed),
            commit_row("b", true, ReviewStatus::Reviewed),
            commit_row("c", false, ReviewStatus::Unreviewed),
        ];

        select_only_index(&mut rows, 1);

        assert!(!rows[0].selected);
        assert!(rows[1].selected);
        assert!(!rows[2].selected);
    }

    #[test]
    fn apply_status_ids_changes_only_targeted_commits() {
        let mut rows = vec![
            commit_row("a", true, ReviewStatus::Unreviewed),
            commit_row("b", true, ReviewStatus::Reviewed),
        ];
        let ids = BTreeSet::from(["b".to_owned()]);

        apply_status_ids(&mut rows, &ids, ReviewStatus::IssueFound);

        assert_eq!(rows[0].status, ReviewStatus::Unreviewed);
        assert_eq!(rows[1].status, ReviewStatus::IssueFound);
    }

    #[test]
    fn reviewed_status_auto_deselects_targeted_commits() {
        let mut rows = vec![
            commit_row("a", true, ReviewStatus::Unreviewed),
            commit_row("b", true, ReviewStatus::IssueFound),
            commit_row("c", false, ReviewStatus::Unreviewed),
        ];
        let ids = BTreeSet::from(["b".to_owned(), "c".to_owned()]);

        apply_status_transition(&mut rows, &ids, ReviewStatus::Reviewed);

        assert!(rows[0].selected);
        assert!(!rows[1].selected);
        assert!(!rows[2].selected);
        assert_eq!(rows[1].status, ReviewStatus::Reviewed);
        assert_eq!(rows[2].status, ReviewStatus::Reviewed);
    }

    #[test]
    fn issue_found_status_keeps_selection_intact() {
        let mut rows = vec![
            commit_row("a", true, ReviewStatus::Unreviewed),
            commit_row("b", false, ReviewStatus::Reviewed),
        ];
        let ids = BTreeSet::from(["a".to_owned()]);

        apply_status_transition(&mut rows, &ids, ReviewStatus::IssueFound);

        assert!(rows[0].selected);
        assert_eq!(rows[0].status, ReviewStatus::IssueFound);
    }

    #[test]
    fn line_with_right_keeps_right_text_visible() {
        let rendered = line_with_right(
            "[F] file.rs".to_owned(),
            Style::default(),
            "3h ago".to_owned(),
            Style::default(),
            24,
        );
        let flattened = rendered
            .spans
            .iter()
            .map(|s| s.content.to_string())
            .collect::<String>();
        assert!(flattened.ends_with("3h ago"));
    }

    #[test]
    fn compose_commit_line_preserves_age_column_on_narrow_width() {
        let row = commit_row("abc1234", false, ReviewStatus::IssueFound);
        let theme = UiTheme::from_mode(ThemeMode::Dark);
        let rendered = compose_commit_line(&row, 24, 3_600, &theme);
        let flattened = rendered
            .spans
            .iter()
            .map(|s| s.content.to_string())
            .collect::<String>();
        assert!(flattened.ends_with("1h ago"));
    }

    #[test]
    fn status_badges_use_exact_workflow_labels() {
        assert_eq!(status_short_label(ReviewStatus::Unreviewed), "UNREVIEWED");
        assert_eq!(status_short_label(ReviewStatus::Reviewed), "REVIEWED");
        assert_eq!(status_short_label(ReviewStatus::IssueFound), "ISSUE_FOUND");
        assert_eq!(status_short_label(ReviewStatus::Resolved), "RESOLVED");
    }

    #[test]
    fn relative_time_formats_expected_units() {
        assert_eq!(format_relative_time(100, 130), "30s ago");
        assert_eq!(format_relative_time(100, 220), "2m ago");
        assert_eq!(format_relative_time(100, 3700), "1h ago");
    }

    #[test]
    fn h_and_l_cycle_all_panes() {
        assert_eq!(focus_with_h(FocusPane::Files), FocusPane::Diff);
        assert_eq!(focus_with_h(FocusPane::Commits), FocusPane::Files);
        assert_eq!(focus_with_h(FocusPane::Diff), FocusPane::Commits);
        assert_eq!(focus_with_l(FocusPane::Files), FocusPane::Commits);
        assert_eq!(focus_with_l(FocusPane::Commits), FocusPane::Diff);
        assert_eq!(focus_with_l(FocusPane::Diff), FocusPane::Files);
    }

    #[test]
    fn commit_banner_renders_only_when_commit_changes() {
        let commits = ["a", "a", "b", "b", "a"];
        let mut previous: Option<&str> = None;
        let rendered = commits
            .iter()
            .map(|current| {
                let show = should_render_commit_banner(previous, current);
                previous = Some(current);
                show
            })
            .collect::<Vec<_>>();

        assert_eq!(rendered, vec![true, false, true, false, true]);
    }

    #[test]
    fn commit_anchor_marker_is_detected() {
        let commit_anchor = CommentAnchor {
            commit_id: "abc1234".to_owned(),
            commit_summary: "summary".to_owned(),
            file_path: "src/lib.rs".to_owned(),
            hunk_header: COMMIT_ANCHOR_HEADER.to_owned(),
            old_lineno: None,
            new_lineno: None,
        };
        let hunk_anchor = CommentAnchor {
            hunk_header: "@@ -1,1 +1,1 @@".to_owned(),
            old_lineno: Some(1),
            new_lineno: Some(1),
            ..commit_anchor.clone()
        };

        assert!(is_commit_anchor(&commit_anchor));
        assert!(!is_commit_anchor(&hunk_anchor));
    }

    #[test]
    fn comment_anchor_match_requires_exact_line_mapping() {
        let base = CommentAnchor {
            commit_id: "abc".to_owned(),
            commit_summary: "summary".to_owned(),
            file_path: "src/lib.rs".to_owned(),
            hunk_header: "@@ -1,1 +1,1 @@".to_owned(),
            old_lineno: Some(1),
            new_lineno: Some(1),
        };
        let same = base.clone();
        let mut different = base.clone();
        different.new_lineno = Some(2);

        assert!(comment_anchor_matches(&base, &same));
        assert!(!comment_anchor_matches(&base, &different));
    }

    #[test]
    fn comment_location_formats_range_when_bounds_differ() {
        let start = CommentAnchor {
            commit_id: "abc".to_owned(),
            commit_summary: "summary".to_owned(),
            file_path: "src/lib.rs".to_owned(),
            hunk_header: "@@ -1,1 +1,1 @@".to_owned(),
            old_lineno: Some(1),
            new_lineno: Some(1),
        };
        let end = CommentAnchor {
            old_lineno: Some(3),
            new_lineno: Some(4),
            ..start.clone()
        };
        let comment = sample_comment(start, end, "check this");

        assert_eq!(
            comment_location_label(&comment),
            "range old 1/new 1 -> old 3/new 4"
        );
    }

    #[test]
    fn comment_location_formats_commit_targets() {
        let anchor = CommentAnchor {
            commit_id: "abc1234deadbeef".to_owned(),
            commit_summary: "summary".to_owned(),
            file_path: "src/lib.rs".to_owned(),
            hunk_header: COMMIT_ANCHOR_HEADER.to_owned(),
            old_lineno: None,
            new_lineno: None,
        };
        let comment = sample_commit_comment(anchor, "commit-level note");

        assert_eq!(comment_location_label(&comment), "commit abc1234");
    }

    #[test]
    fn comment_commit_membership_uses_commit_anchor() {
        let anchor = CommentAnchor {
            commit_id: "abc1234deadbeef".to_owned(),
            commit_summary: "summary".to_owned(),
            file_path: "src/lib.rs".to_owned(),
            hunk_header: COMMIT_ANCHOR_HEADER.to_owned(),
            old_lineno: None,
            new_lineno: None,
        };
        let comment = sample_commit_comment(anchor, "commit-level note");

        assert!(comment_targets_commit_end(
            &comment,
            "src/lib.rs",
            "abc1234deadbeef"
        ));
        assert!(!comment_targets_commit_end(
            &comment,
            "src/lib.rs",
            "fffffff"
        ));
    }

    #[test]
    fn comment_hunk_membership_uses_end_anchor() {
        let start = CommentAnchor {
            commit_id: "start".to_owned(),
            commit_summary: "summary".to_owned(),
            file_path: "src/lib.rs".to_owned(),
            hunk_header: "@@ -1,1 +1,1 @@".to_owned(),
            old_lineno: Some(1),
            new_lineno: Some(1),
        };
        let end = CommentAnchor {
            commit_id: "end".to_owned(),
            commit_summary: "summary".to_owned(),
            file_path: "src/lib.rs".to_owned(),
            hunk_header: "@@ -10,1 +10,1 @@".to_owned(),
            old_lineno: Some(10),
            new_lineno: Some(10),
        };
        let mut comment = sample_comment(start, end.clone(), "multi hunk");
        comment.id = 8;
        comment.target.commits = BTreeSet::from(["start".to_owned(), "end".to_owned()]);

        assert!(comment_targets_hunk_end(
            &comment,
            "src/lib.rs",
            "end",
            "@@ -10,1 +10,1 @@"
        ));
        assert!(!comment_targets_hunk_end(
            &comment,
            "src/lib.rs",
            "start",
            "@@ -1,1 +1,1 @@"
        ));
        assert!(!comment_targets_hunk_end(
            &comment,
            "src/other.rs",
            "end",
            "@@ -10,1 +10,1 @@"
        ));
    }

    #[test]
    fn push_comment_lines_sets_comment_id_on_each_rendered_row() {
        let start = CommentAnchor {
            commit_id: "abc".to_owned(),
            commit_summary: "summary".to_owned(),
            file_path: "src/lib.rs".to_owned(),
            hunk_header: "@@ -1,1 +1,1 @@".to_owned(),
            old_lineno: Some(1),
            new_lineno: Some(1),
        };
        let end = CommentAnchor {
            old_lineno: Some(2),
            new_lineno: Some(2),
            ..start.clone()
        };
        let comment = sample_comment(start, end, "line one\nline two");
        let theme = UiTheme::from_mode(ThemeMode::Dark);
        let mut rendered = Vec::new();

        push_comment_lines(&mut rendered, &comment, &theme, 0);

        assert_eq!(rendered.len(), 3);
        assert!(
            rendered
                .iter()
                .all(|line| line.comment_id == Some(comment.id))
        );
    }

    #[test]
    fn push_comment_lines_for_anchor_injects_once_on_matching_end_anchor() {
        let start = CommentAnchor {
            commit_id: "abc".to_owned(),
            commit_summary: "summary".to_owned(),
            file_path: "src/lib.rs".to_owned(),
            hunk_header: "@@ -1,1 +1,1 @@".to_owned(),
            old_lineno: Some(1),
            new_lineno: Some(1),
        };
        let end = CommentAnchor {
            old_lineno: Some(2),
            new_lineno: Some(2),
            ..start.clone()
        };
        let comment = sample_comment(start.clone(), end.clone(), "line one");
        let theme = UiTheme::from_mode(ThemeMode::Dark);
        let mut rendered = Vec::new();
        let comments = vec![&comment];
        let mut injected = BTreeSet::new();

        push_comment_lines_for_anchor(&mut rendered, &comments, &mut injected, &start, &theme, 0);
        assert!(rendered.is_empty());

        push_comment_lines_for_anchor(&mut rendered, &comments, &mut injected, &end, &theme, 0);
        let inserted_rows = rendered.len();
        assert!(inserted_rows > 0);

        push_comment_lines_for_anchor(&mut rendered, &comments, &mut injected, &end, &theme, 0);
        assert_eq!(rendered.len(), inserted_rows);
    }

    #[test]
    fn diff_search_wraps_forward() {
        let lines = vec![
            RenderedDiffLine {
                line: Line::from("alpha"),
                raw_text: "alpha".to_owned(),
                anchor: None,
                comment_id: None,
            },
            RenderedDiffLine {
                line: Line::from("beta"),
                raw_text: "beta".to_owned(),
                anchor: None,
                comment_id: None,
            },
            RenderedDiffLine {
                line: Line::from("gamma"),
                raw_text: "gamma".to_owned(),
                anchor: None,
                comment_id: None,
            },
        ];

        let found = find_diff_match_from_cursor(&lines, "alp", true, 2);
        assert_eq!(found, Some(0));
    }

    #[test]
    fn diff_search_wraps_backward() {
        let lines = vec![
            RenderedDiffLine {
                line: Line::from("alpha"),
                raw_text: "alpha".to_owned(),
                anchor: None,
                comment_id: None,
            },
            RenderedDiffLine {
                line: Line::from("beta"),
                raw_text: "beta".to_owned(),
                anchor: None,
                comment_id: None,
            },
            RenderedDiffLine {
                line: Line::from("gamma"),
                raw_text: "gamma".to_owned(),
                anchor: None,
                comment_id: None,
            },
        ];

        let found = find_diff_match_from_cursor(&lines, "gam", false, 0);
        assert_eq!(found, Some(2));
    }

    #[test]
    fn hunk_header_detection_ignores_commit_banner() {
        let commit_anchor = CommentAnchor {
            commit_id: "abc1234".to_owned(),
            commit_summary: "summary".to_owned(),
            file_path: "src/lib.rs".to_owned(),
            hunk_header: COMMIT_ANCHOR_HEADER.to_owned(),
            old_lineno: None,
            new_lineno: None,
        };
        let hunk_anchor = CommentAnchor {
            hunk_header: "@@ -1,1 +1,1 @@".to_owned(),
            old_lineno: Some(1),
            new_lineno: Some(1),
            ..commit_anchor.clone()
        };

        let commit_line = RenderedDiffLine {
            line: Line::from("---- commit abc1234 summary"),
            raw_text: "---- commit abc1234 summary".to_owned(),
            anchor: Some(commit_anchor),
            comment_id: None,
        };
        let hunk_line = RenderedDiffLine {
            line: Line::from("@@ -1,1 +1,1 @@"),
            raw_text: "@@ -1,1 +1,1 @@".to_owned(),
            anchor: Some(hunk_anchor),
            comment_id: None,
        };

        assert!(!is_hunk_header_line(&commit_line));
        assert!(is_hunk_header_line(&hunk_line));
    }

    #[test]
    fn cursor_tint_blends_existing_diff_background() {
        let line = Line::from(vec![Span::styled(
            "+ let x = 1",
            Style::default().bg(Color::Rgb(19, 51, 30)),
        )]);
        let tinted = tint_line_background(&line, Color::Rgb(52, 52, 62), true);
        let bg = tinted.spans[0].style.bg.expect("bg");

        assert_ne!(bg, Color::Rgb(19, 51, 30));
        assert_ne!(bg, Color::Rgb(52, 52, 62));
    }
}
