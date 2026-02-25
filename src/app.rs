use std::{
    cmp::{max, min},
    collections::{BTreeMap, BTreeSet, HashMap},
    sync::Arc,
    time::{Duration, Instant},
};

use chrono::Utc;
use crossterm::event::{
    Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers, MouseButton, MouseEventKind,
};
use ratatui::{
    Frame,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, ListState, Paragraph},
};
use syntect::{
    easy::HighlightLines, highlighting::Theme, highlighting::ThemeSet, parsing::SyntaxSet,
};

use crate::{
    comments::CommentStore,
    git_data::GitService,
    model::{
        AggregatedDiff, CommentAnchor, CommentTarget, CommitInfo, DiffLineKind, FilePatch,
        HunkLine, ReviewState, ReviewStatus,
    },
    store::StateStore,
};

const HISTORY_LIMIT: usize = 400;
const AUTO_REFRESH_EVERY: Duration = Duration::from_secs(4);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FocusPane {
    Files,
    Commits,
    Diff,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum InputMode {
    Normal,
    Comment,
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
}

#[derive(Debug, Clone, Copy)]
struct DiffVisualSelection {
    anchor: usize,
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
    diff_position: DiffPosition,
    rendered_diff: Arc<Vec<RenderedDiffLine>>,
    rendered_diff_cache: HashMap<(String, ThemeMode), Arc<Vec<RenderedDiffLine>>>,
    rendered_diff_key: Option<(String, ThemeMode)>,
    highlighter: DiffSyntaxHighlighter,
    pane_rects: PaneRects,
    status: String,
    comment_buffer: String,
    last_refresh: Instant,
    should_quit: bool,
}

impl App {
    pub fn bootstrap() -> anyhow::Result<Self> {
        let git = GitService::open_current()?;
        let store = StateStore::for_project(git.root());
        let review_state = store.load()?;
        let comments = CommentStore::new(store.root_dir(), git.branch_name());

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
            diff_position: DiffPosition::default(),
            rendered_diff: Arc::new(Vec::new()),
            rendered_diff_cache: HashMap::new(),
            rendered_diff_key: None,
            highlighter: DiffSyntaxHighlighter::new(),
            pane_rects: PaneRects::default(),
            status: String::new(),
            comment_buffer: String::new(),
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
            .split(root_chunks[0]);

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

        self.render_files(frame, self.pane_rects.files, &theme);
        self.render_commits(frame, self.pane_rects.commits, &theme);
        self.render_diff(frame, self.pane_rects.diff, &theme);
        self.render_footer(frame, root_chunks[1], &theme);
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
        if self.input_mode == InputMode::Comment {
            self.handle_comment_input(key);
            return;
        }

        match key.code {
            KeyCode::Char('q') => self.should_quit = true,
            KeyCode::Tab | KeyCode::Char('l') if key.modifiers == KeyModifiers::NONE => {
                self.focus_next()
            }
            KeyCode::BackTab | KeyCode::Char('h') if key.modifiers == KeyModifiers::NONE => {
                self.focus_prev()
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
                self.status = "Footer is contextual by focused pane.".to_owned();
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

                if let Some(target) = self.comment_target_from_selection() {
                    let result = self.comments.append(&target, &self.comment_buffer);
                    match result {
                        Ok(path) => {
                            self.set_status_for_ids(&target.commits, ReviewStatus::IssueFound);
                            self.status = format!(
                                "Comment saved -> {} ({} commit(s) marked ISSUE_FOUND)",
                                path.display(),
                                target.commits.len()
                            );
                        }
                        Err(err) => {
                            self.status = format!("Failed to save comment: {err:#}");
                        }
                    }
                } else {
                    self.status = "No hunk/line anchor at cursor or selected range".to_owned();
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

    fn dispatch_focus_key(&mut self, key: KeyEvent) {
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
        match key.code {
            KeyCode::Down | KeyCode::Char('j') => self.move_diff_cursor(1),
            KeyCode::Up | KeyCode::Char('k') => self.move_diff_cursor(-1),
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
                self.input_mode = InputMode::Comment;
                self.comment_buffer.clear();
                self.status =
                    "Comment mode: type comment, Enter save, Esc cancel (supports visual range)"
                        .to_owned();
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
        let title = format!("Changed Files ({})", self.aggregate.files.len());
        let border_style = if self.focused == FocusPane::Files {
            Style::default().fg(theme.focus_border)
        } else {
            Style::default().fg(theme.border)
        };

        let width = rect.width.saturating_sub(4) as usize;
        let now_ts = Utc::now().timestamp();

        let items: Vec<ListItem<'static>> = self
            .file_rows
            .iter()
            .map(|row| {
                let style = if row.selectable {
                    Style::default().fg(theme.text)
                } else {
                    Style::default().fg(theme.dir)
                };
                let line_text = if row.selectable {
                    let right = row
                        .modified_ts
                        .map(|ts| format_relative_time(ts, now_ts))
                        .unwrap_or_default();
                    align_with_right(&row.label, &right, width)
                } else {
                    row.label.clone()
                };
                ListItem::new(Line::from(Span::styled(line_text, style)))
            })
            .collect();

        let list = List::new(items)
            .block(
                Block::default()
                    .title(title)
                    .borders(Borders::ALL)
                    .border_style(border_style),
            )
            .highlight_style(Style::default().bg(theme.highlight_bg))
            .highlight_symbol("-> ");

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
        let title = format!(
            "Commits [{} sel | U:{} R:{} I:{} Z:{}]",
            selected, unreviewed, reviewed, issue_found, resolved
        );
        let border_style = if self.focused == FocusPane::Commits {
            Style::default().fg(theme.focus_border)
        } else {
            Style::default().fg(theme.border)
        };

        let items: Vec<ListItem<'static>> = self
            .commits
            .iter()
            .map(|row| {
                let check = if row.selected { "[x]" } else { "[ ]" };
                let badge = status_badge(row.status, theme);

                let mut spans = vec![
                    Span::styled(
                        format!("{} {} ", check, row.info.short_id),
                        Style::default().fg(theme.text),
                    ),
                    Span::styled(
                        truncate(&row.info.summary, 34),
                        Style::default().fg(theme.text),
                    ),
                    Span::raw(" "),
                    badge,
                ];
                if row.info.unpushed {
                    spans.push(Span::raw(" "));
                    spans.push(Span::styled(
                        "unpushed",
                        Style::default().fg(theme.unpushed),
                    ));
                }
                ListItem::new(Line::from(spans))
            })
            .collect();

        let list = List::new(items)
            .block(
                Block::default()
                    .title(title)
                    .borders(Borders::ALL)
                    .border_style(border_style),
            )
            .highlight_style(Style::default().bg(theme.highlight_bg))
            .highlight_symbol("-> ");

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
        let title = format!("Diff: {}", file_label);

        let visual_range = self.diff_selected_range();
        let mut lines = Vec::with_capacity(self.rendered_diff.len());
        for (idx, rendered) in self.rendered_diff.iter().enumerate() {
            let mut line = rendered.line.clone();

            if let Some((start, end)) = visual_range
                && idx >= start
                && idx <= end
            {
                line = line.patch_style(Style::default().bg(theme.visual_bg));
            }

            if idx == self.diff_position.cursor && self.focused == FocusPane::Diff {
                line = line.patch_style(Style::default().bg(theme.cursor_bg));
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
                    .border_style(border_style),
            )
            .scroll((self.diff_position.scroll as u16, 0));

        frame.render_widget(paragraph, rect);
    }

    fn render_footer(&self, frame: &mut Frame<'_>, rect: ratatui::layout::Rect, theme: &UiTheme) {
        let mode = match self.input_mode {
            InputMode::Normal => "NORMAL",
            InputMode::Comment => "COMMENT",
        };
        let focus = match self.focused {
            FocusPane::Files => "files",
            FocusPane::Commits => "commits",
            FocusPane::Diff => "diff",
        };

        let pane_hints = if self.input_mode == InputMode::Comment {
            "Comment: Enter save | Esc cancel"
        } else {
            match self.focused {
                FocusPane::Files => "Files: j/k g/G Ctrl-d/u PgUp/PgDn | Enter focus diff",
                FocusPane::Commits => {
                    "Commits: <space>/v/x select | u/r/i/s current | U/R/I/S selected"
                }
                FocusPane::Diff => "Diff: j/k g/G Ctrl-d/u PgUp/PgDn | v/V range | m comment",
            }
        };

        let global_hints =
            "Global: 1/2/3 panes | Tab h/l cycle | t theme | F5/Ctrl-r refresh | q quit";

        let status = if self.input_mode == InputMode::Comment {
            format!(
                "{} | mode={} focus={} theme={} > {}",
                self.status,
                mode,
                focus,
                self.theme_mode.label(),
                self.comment_buffer
            )
        } else {
            format!(
                "{} | mode={} focus={} theme={}",
                self.status,
                mode,
                focus,
                self.theme_mode.label()
            )
        };

        let chunks = ratatui::layout::Layout::default()
            .direction(ratatui::layout::Direction::Vertical)
            .constraints([
                ratatui::layout::Constraint::Length(1),
                ratatui::layout::Constraint::Length(2),
            ])
            .split(rect);

        let status_widget = Paragraph::new(status).style(Style::default().fg(theme.text));
        let hint_widget = Paragraph::new(format!("{}\n{}", pane_hints, global_hints))
            .style(Style::default().fg(theme.dimmed))
            .block(
                Block::default()
                    .borders(Borders::TOP)
                    .border_style(Style::default().fg(theme.border)),
            );

        frame.render_widget(status_widget, chunks[0]);
        frame.render_widget(hint_widget, chunks[1]);
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
                }
            })
            .collect();

        if self.commit_list_state.selected().is_none() && !self.commits.is_empty() {
            self.commit_list_state.select(Some(0));
        }

        let new_commits = self
            .commits
            .iter()
            .filter(|row| !known.contains(&row.info.id) && row.status == ReviewStatus::Unreviewed)
            .count();
        if new_commits > 0 {
            self.status = format!("{} new unreviewed commit(s) detected", new_commits);
        }

        self.rebuild_selection_dependent_views()?;
        Ok(())
    }

    fn rebuild_selection_dependent_views(&mut self) -> anyhow::Result<()> {
        let selected_ordered = self.selected_commit_ids_oldest_first();
        self.aggregate = if selected_ordered.is_empty() {
            AggregatedDiff::default()
        } else {
            self.git.aggregate_for_commits(&selected_ordered)?
        };

        self.rendered_diff_cache.clear();
        self.rendered_diff_key = None;
        self.diff_visual = None;

        self.rebuild_file_tree();
        self.ensure_selected_file_exists();
        self.ensure_rendered_diff();
        Ok(())
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

        self.ensure_cursor_visible();
    }

    fn build_diff_lines(&self, patch: &FilePatch) -> Vec<RenderedDiffLine> {
        let mut rendered = Vec::new();
        let theme = UiTheme::from_mode(self.theme_mode);

        for hunk in &patch.hunks {
            let commit_line = format!("commit {} {}", hunk.commit_short, hunk.commit_summary);
            rendered.push(RenderedDiffLine {
                line: Line::from(vec![
                    Span::styled("commit ", Style::default().fg(theme.muted)),
                    Span::styled(
                        hunk.commit_short.clone(),
                        Style::default()
                            .fg(theme.focus_border)
                            .add_modifier(Modifier::BOLD),
                    ),
                    Span::raw(" "),
                    Span::styled(hunk.commit_summary.clone(), Style::default().fg(theme.text)),
                ]),
                raw_text: commit_line,
                anchor: None,
            });

            let hunk_label = format!("hunk {}", hunk.header);
            rendered.push(RenderedDiffLine {
                line: Line::from(vec![
                    Span::styled("hunk ", Style::default().fg(theme.muted)),
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
            });

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
                    anchor: Some(anchor),
                });
            }

            rendered.push(RenderedDiffLine {
                line: Line::from(""),
                raw_text: String::new(),
                anchor: None,
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

        let mut spans = vec![Span::styled(
            prefix.to_string(),
            Style::default().fg(accent).add_modifier(Modifier::BOLD),
        )];

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
        self.diff_visual = None;
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
        let ids = BTreeSet::from([row.info.id.clone()]);
        self.set_status_for_ids(&ids, status);
    }

    fn set_selected_commit_status(&mut self, status: ReviewStatus) {
        let ids = self
            .commits
            .iter()
            .filter(|row| row.selected)
            .map(|row| row.info.id.clone())
            .collect::<BTreeSet<_>>();
        if ids.is_empty() {
            self.status = "No selected commits".to_owned();
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

        apply_status_ids(&mut self.commits, ids, status);
        let save_result = self.store.save(&self.review_state);
        let status_message = if let Err(err) = save_result {
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

    fn ensure_cursor_visible(&mut self) {
        let visible = self.pane_rects.diff.height.saturating_sub(2).max(1) as usize;

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

        if let Some(visual) = self.diff_visual {
            Some((
                min(visual.anchor, self.diff_position.cursor),
                max(visual.anchor, self.diff_position.cursor),
            ))
        } else {
            Some((self.diff_position.cursor, self.diff_position.cursor))
        }
    }

    fn comment_target_from_selection(&self) -> Option<CommentTarget> {
        let (start_idx, end_idx) = self.diff_selected_range()?;
        let mut anchors = Vec::new();
        let mut selected_lines = Vec::new();
        let mut commits = BTreeSet::new();

        for idx in start_idx..=end_idx {
            let Some(line) = self.rendered_diff.get(idx) else {
                continue;
            };
            if let Some(anchor) = &line.anchor {
                commits.insert(anchor.commit_id.clone());
                anchors.push(anchor.clone());
                if !line.raw_text.trim().is_empty() {
                    selected_lines.push(line.raw_text.clone());
                }
            }
        }

        let start = anchors.first()?.clone();
        let end = anchors.last()?.clone();

        Some(CommentTarget {
            start,
            end,
            commits,
            selected_lines,
        })
    }

    fn status_counts(&self) -> (usize, usize, usize, usize) {
        let mut unreviewed = 0;
        let mut reviewed = 0;
        let mut issue_found = 0;
        let mut resolved = 0;
        for row in &self.commits {
            match row.status {
                ReviewStatus::Unreviewed => unreviewed += 1,
                ReviewStatus::Reviewed => reviewed += 1,
                ReviewStatus::IssueFound => issue_found += 1,
                ReviewStatus::Resolved => resolved += 1,
            }
        }
        (unreviewed, reviewed, issue_found, resolved)
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

fn align_with_right(left: &str, right: &str, width: usize) -> String {
    if right.is_empty() {
        return truncate(left, width.max(1));
    }

    let left_width = left.chars().count();
    let right_width = right.chars().count();
    if left_width + right_width + 1 >= width {
        return format!(
            "{} {}",
            truncate(left, width.saturating_sub(right_width + 1)),
            right
        );
    }

    let spaces = " ".repeat(width - left_width - right_width);
    format!("{}{}{}", left, spaces, right)
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

fn selected_ids_oldest_first(rows: &[CommitRow]) -> Vec<String> {
    rows.iter()
        .rev()
        .filter(|row| row.selected)
        .map(|row| row.info.id.clone())
        .collect()
}

fn apply_range_selection(rows: &mut [CommitRow], start: usize, end: usize) {
    let (start, end) = (min(start, end), max(start, end));
    for (idx, row) in rows.iter_mut().enumerate() {
        row.selected = idx >= start && idx <= end;
    }
}

fn apply_status_ids(rows: &mut [CommitRow], ids: &BTreeSet<String>, status: ReviewStatus) {
    for row in rows {
        if ids.contains(&row.info.id) {
            row.status = status;
        }
    }
}

fn status_badge(status: ReviewStatus, theme: &UiTheme) -> Span<'static> {
    match status {
        ReviewStatus::Unreviewed => Span::styled(
            "UNREVIEWED",
            Style::default()
                .fg(theme.unreviewed)
                .add_modifier(Modifier::BOLD),
        ),
        ReviewStatus::Reviewed => Span::styled("REVIEWED", Style::default().fg(theme.reviewed)),
        ReviewStatus::IssueFound => Span::styled(
            "ISSUE_FOUND",
            Style::default()
                .fg(theme.issue)
                .add_modifier(Modifier::BOLD),
        ),
        ReviewStatus::Resolved => Span::styled("RESOLVED", Style::default().fg(theme.resolved)),
    }
}

fn page_step(height: u16, multiplier: f32) -> isize {
    let visible = height.saturating_sub(2).max(1) as f32;
    (visible * multiplier).round() as isize
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
    fn align_with_right_keeps_right_text_visible() {
        let rendered = align_with_right("[F] file.rs", "3h ago", 24);
        assert!(rendered.ends_with("3h ago"));
    }

    #[test]
    fn relative_time_formats_expected_units() {
        assert_eq!(format_relative_time(100, 130), "30s ago");
        assert_eq!(format_relative_time(100, 220), "2m ago");
        assert_eq!(format_relative_time(100, 3700), "1h ago");
    }
}
