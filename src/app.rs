use std::{
    cmp::{max, min},
    collections::{BTreeMap, BTreeSet, HashMap},
    time::{Duration, Instant},
};

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
    easy::HighlightLines,
    highlighting::{Theme, ThemeSet},
    parsing::SyntaxSet,
};

use crate::{
    comments::CommentStore,
    git_data::GitService,
    model::{
        AggregatedDiff, ApprovalScope, CommentAnchor, CommitInfo, DiffLineKind, FilePatch,
        ReviewState,
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

#[derive(Debug, Clone)]
struct CommitRow {
    info: CommitInfo,
    selected: bool,
    approved: bool,
}

#[derive(Debug, Clone)]
struct TreeRow {
    label: String,
    path: Option<String>,
    selectable: bool,
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
    anchor: Option<CommentAnchor>,
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
    visual_anchor: Option<usize>,
    aggregate: AggregatedDiff,
    selected_file: Option<String>,
    diff_positions: HashMap<String, DiffPosition>,
    diff_position: DiffPosition,
    rendered_diff: Vec<RenderedDiffLine>,
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
            visual_anchor: None,
            aggregate: AggregatedDiff::default(),
            selected_file: None,
            diff_positions: HashMap::new(),
            diff_position: DiffPosition::default(),
            rendered_diff: Vec::new(),
            highlighter: DiffSyntaxHighlighter::new(),
            pane_rects: PaneRects::default(),
            status: String::new(),
            comment_buffer: String::new(),
            last_refresh: Instant::now(),
            should_quit: false,
        };

        app.reload_commits(true)?;
        if app.status.is_empty() {
            app.status = "Ready. Select commits with <space>; press ? for key hints.".to_owned();
        }
        Ok(app)
    }

    pub fn should_quit(&self) -> bool {
        self.should_quit
    }

    pub fn draw(&mut self, frame: &mut Frame<'_>) {
        self.ensure_rendered_diff();

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

        self.render_files(frame, self.pane_rects.files);
        self.render_commits(frame, self.pane_rects.commits);
        self.render_diff(frame, self.pane_rects.diff);
        self.render_footer(frame, root_chunks[1]);
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
            KeyCode::Char('q') => {
                self.should_quit = true;
            }
            KeyCode::Tab | KeyCode::Char('l') if key.modifiers == KeyModifiers::NONE => {
                self.focus_next();
            }
            KeyCode::BackTab | KeyCode::Char('h') if key.modifiers == KeyModifiers::NONE => {
                self.focus_prev();
            }
            KeyCode::Char('f') => self.focused = FocusPane::Files,
            KeyCode::Char('c') => self.focused = FocusPane::Commits,
            KeyCode::Char('d') => self.focused = FocusPane::Diff,
            KeyCode::Char('R') => {
                if let Err(err) = self.reload_commits(true) {
                    self.status = format!("reload failed: {err:#}");
                }
            }
            KeyCode::Char('?') => {
                self.status =
                    "Hints are in footer: pane keys + selection + approvals + comments.".to_owned();
            }
            _ => self.dispatch_focus_key(key),
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

                let anchor = self
                    .rendered_diff
                    .get(self.diff_position.cursor)
                    .and_then(|line| line.anchor.clone());
                if let Some(anchor) = anchor {
                    match self.comments.append(&anchor, &self.comment_buffer) {
                        Ok(path) => {
                            self.status = format!("Comment saved -> {}", path.display());
                        }
                        Err(err) => {
                            self.status = format!("Failed to save comment: {err:#}");
                        }
                    }
                } else {
                    self.status = "No hunk/line anchor at cursor".to_owned();
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
            KeyCode::Char('g') => self.select_first_commit(),
            KeyCode::Char('G') => self.select_last_commit(),
            KeyCode::Char('v') => {
                if self.visual_anchor.is_some() {
                    self.visual_anchor = None;
                    self.status = "Visual range selection off".to_owned();
                } else {
                    self.visual_anchor = self.commit_list_state.selected();
                    self.status = "Visual range selection on".to_owned();
                    self.apply_visual_range();
                }
            }
            KeyCode::Char('x') => {
                for row in &mut self.commits {
                    row.selected = false;
                }
                self.visual_anchor = None;
                self.on_selection_changed();
            }
            KeyCode::Char(' ') => {
                if let Some(idx) = self.commit_list_state.selected()
                    && let Some(row) = self.commits.get_mut(idx)
                    && !row.approved
                {
                    row.selected = !row.selected;
                }
                self.visual_anchor = None;
                self.on_selection_changed();
            }
            KeyCode::Char('a') => {
                self.approve_current_commit();
            }
            KeyCode::Char('A') => {
                self.approve_selected_commits();
            }
            KeyCode::Char('B') => {
                self.approve_branch_commits();
            }
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
                self.page_diff(-0.5);
            }
            KeyCode::Char('d') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.page_diff(0.5);
            }
            KeyCode::PageUp => self.page_diff(-1.0),
            KeyCode::PageDown => self.page_diff(1.0),
            KeyCode::Char('m') => {
                self.input_mode = InputMode::Comment;
                self.comment_buffer.clear();
                self.status = "Comment mode: type comment, Enter to save, Esc to cancel".to_owned();
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

    fn render_files(&mut self, frame: &mut Frame<'_>, rect: ratatui::layout::Rect) {
        let title = format!("Changed Files ({})", self.aggregate.files.len());
        let border_style = if self.focused == FocusPane::Files {
            Style::default().fg(Color::Yellow)
        } else {
            Style::default()
        };

        let items: Vec<ListItem<'static>> = self
            .file_rows
            .iter()
            .map(|row| {
                let style = if row.selectable {
                    Style::default()
                } else {
                    Style::default().fg(Color::DarkGray)
                };
                ListItem::new(Line::from(Span::styled(row.label.clone(), style)))
            })
            .collect();

        let list = List::new(items)
            .block(
                Block::default()
                    .title(title)
                    .borders(Borders::ALL)
                    .border_style(border_style),
            )
            .highlight_style(Style::default().bg(Color::DarkGray))
            .highlight_symbol("-> ");

        frame.render_stateful_widget(list, rect, &mut self.file_list_state);
    }

    fn render_commits(&mut self, frame: &mut Frame<'_>, rect: ratatui::layout::Rect) {
        let selected = self.commits.iter().filter(|row| row.selected).count();
        let unreviewed = self.commits.iter().filter(|row| !row.approved).count();
        let title = format!(
            "Commits [{} selected | {} unreviewed]",
            selected, unreviewed
        );
        let border_style = if self.focused == FocusPane::Commits {
            Style::default().fg(Color::Yellow)
        } else {
            Style::default()
        };

        let items: Vec<ListItem<'static>> = self
            .commits
            .iter()
            .map(|row| {
                let check = if row.selected { "[x]" } else { "[ ]" };
                let badge = if row.approved {
                    Span::styled("reviewed", Style::default().fg(Color::Green))
                } else {
                    Span::styled(
                        "UNREVIEWED",
                        Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
                    )
                };
                let mut spans = vec![
                    Span::raw(format!("{} {} ", check, row.info.short_id)),
                    Span::raw(truncate(&row.info.summary, 36)),
                    Span::raw(" "),
                    badge,
                ];
                if row.info.unpushed {
                    spans.push(Span::raw(" "));
                    spans.push(Span::styled("unpushed", Style::default().fg(Color::Cyan)));
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
            .highlight_style(Style::default().bg(Color::DarkGray))
            .highlight_symbol("-> ");

        frame.render_stateful_widget(list, rect, &mut self.commit_list_state);
    }

    fn render_diff(&mut self, frame: &mut Frame<'_>, rect: ratatui::layout::Rect) {
        let border_style = if self.focused == FocusPane::Diff {
            Style::default().fg(Color::Yellow)
        } else {
            Style::default()
        };

        let file_label = self
            .selected_file
            .clone()
            .unwrap_or_else(|| "(no file selected)".to_owned());
        let title = format!("Diff: {}", file_label);

        let mut lines = Vec::with_capacity(self.rendered_diff.len());
        for (idx, rendered) in self.rendered_diff.iter().enumerate() {
            if idx == self.diff_position.cursor && self.focused == FocusPane::Diff {
                lines.push(
                    rendered
                        .line
                        .clone()
                        .patch_style(Style::default().bg(Color::DarkGray)),
                );
            } else {
                lines.push(rendered.line.clone());
            }
        }

        if lines.is_empty() {
            lines.push(Line::from(
                "No selected commits or no textual diff for this file",
            ));
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

    fn render_footer(&self, frame: &mut Frame<'_>, rect: ratatui::layout::Rect) {
        let mode = match self.input_mode {
            InputMode::Normal => "NORMAL",
            InputMode::Comment => "COMMENT",
        };
        let focus = match self.focused {
            FocusPane::Files => "files",
            FocusPane::Commits => "commits",
            FocusPane::Diff => "diff",
        };

        let key_hints = if self.input_mode == InputMode::Comment {
            "Comment: type text, Enter save, Esc cancel"
        } else {
            "Pane: h/l or Tab | Nav: j/k g/G | Select commits: <space>/v | Approve: a A B | Comment: m | Refresh: R | Quit: q"
        };

        let status = if self.input_mode == InputMode::Comment {
            format!(
                "{} | mode={} focus={} > {}",
                self.status, mode, focus, self.comment_buffer
            )
        } else {
            format!("{} | mode={} focus={}", self.status, mode, focus)
        };

        let chunks = ratatui::layout::Layout::default()
            .direction(ratatui::layout::Direction::Vertical)
            .constraints([
                ratatui::layout::Constraint::Length(1),
                ratatui::layout::Constraint::Length(2),
            ])
            .split(rect);

        let status_widget = Paragraph::new(status).style(Style::default().fg(Color::White));
        let hint_widget = Paragraph::new(key_hints)
            .style(Style::default().fg(Color::DarkGray))
            .block(Block::default().borders(Borders::TOP));

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

        let mut known = BTreeSet::new();
        for row in &self.commits {
            known.insert(row.info.id.clone());
        }

        self.commits = history
            .into_iter()
            .map(|info| {
                let approved = self.review_state.approvals.contains_key(&info.id);
                let selected = if approved {
                    false
                } else if preserve_manual_selection && old_selected.contains(&info.id) {
                    true
                } else {
                    default_selected.contains(&info.id)
                };
                CommitRow {
                    info,
                    selected,
                    approved,
                }
            })
            .collect();

        if self.commit_list_state.selected().is_none() && !self.commits.is_empty() {
            self.commit_list_state.select(Some(0));
        }

        let new_commits = self
            .commits
            .iter()
            .filter(|row| !known.contains(&row.info.id) && !row.approved)
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

        self.rebuild_file_tree();
        self.ensure_selected_file_exists();
        self.ensure_rendered_diff();
        Ok(())
    }

    fn ensure_rendered_diff(&mut self) {
        self.rendered_diff = self
            .selected_file
            .as_ref()
            .and_then(|path| self.aggregate.files.get(path))
            .map(|patch| self.build_diff_lines(patch))
            .unwrap_or_default();

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

        for hunk in &patch.hunks {
            rendered.push(RenderedDiffLine {
                line: Line::from(vec![
                    Span::styled("commit ", Style::default().fg(Color::DarkGray)),
                    Span::styled(
                        hunk.commit_short.clone(),
                        Style::default()
                            .fg(Color::Yellow)
                            .add_modifier(Modifier::BOLD),
                    ),
                    Span::raw(" "),
                    Span::styled(
                        hunk.commit_summary.clone(),
                        Style::default().fg(Color::White),
                    ),
                ]),
                anchor: None,
            });

            rendered.push(RenderedDiffLine {
                line: Line::from(vec![
                    Span::styled("hunk ", Style::default().fg(Color::DarkGray)),
                    Span::styled(hunk.header.clone(), Style::default().fg(Color::Cyan)),
                ]),
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
                    line: self.render_code_line(&patch.path, line),
                    anchor: Some(anchor),
                });
            }

            rendered.push(RenderedDiffLine {
                line: Line::from(""),
                anchor: None,
            });
        }

        rendered
    }

    fn render_code_line(&self, path: &str, line: &crate::model::HunkLine) -> Line<'static> {
        let (prefix, accent, bg) = match line.kind {
            DiffLineKind::Add => ('+', Color::Green, Some(Color::Rgb(13, 41, 20))),
            DiffLineKind::Remove => ('-', Color::Red, Some(Color::Rgb(45, 16, 16))),
            DiffLineKind::Context => (' ', Color::DarkGray, None),
            DiffLineKind::Meta => ('~', Color::Yellow, None),
        };

        let mut spans = vec![Span::styled(
            prefix.to_string(),
            Style::default().fg(accent).add_modifier(Modifier::BOLD),
        )];

        let highlighted = self.highlighter.highlight(path, &line.text);
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
        for path in self.aggregate.file_paths() {
            tree.insert(&path);
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
        if idx >= self.file_rows.len() {
            return;
        }
        if !self.file_rows[idx].selectable {
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

        if self.visual_anchor.is_some() {
            self.apply_visual_range();
        }
    }

    fn select_first_commit(&mut self) {
        if self.commits.is_empty() {
            return;
        }
        self.commit_list_state.select(Some(0));
        if self.visual_anchor.is_some() {
            self.apply_visual_range();
        }
    }

    fn select_last_commit(&mut self) {
        if self.commits.is_empty() {
            return;
        }
        self.commit_list_state.select(Some(self.commits.len() - 1));
        if self.visual_anchor.is_some() {
            self.apply_visual_range();
        }
    }

    fn select_commit_row(&mut self, idx: usize, toggle: bool) {
        if idx >= self.commits.len() {
            return;
        }
        self.commit_list_state.select(Some(idx));
        if toggle
            && let Some(row) = self.commits.get_mut(idx)
            && !row.approved
        {
            row.selected = !row.selected;
            self.on_selection_changed();
        }
    }

    fn apply_visual_range(&mut self) {
        let Some(anchor) = self.visual_anchor else {
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

    fn approve_current_commit(&mut self) {
        let Some(idx) = self.commit_list_state.selected() else {
            return;
        };
        let Some(row) = self.commits.get_mut(idx) else {
            return;
        };
        self.store.mark_approved(
            &mut self.review_state,
            &row.info.id,
            ApprovalScope::Commit,
            self.git.branch_name(),
        );
        row.approved = true;
        row.selected = false;
        self.visual_anchor = None;

        let status = if let Err(err) = self.store.save(&self.review_state) {
            format!("failed to persist approval: {err:#}")
        } else {
            format!("Approved {}", row.info.short_id)
        };
        self.on_selection_changed();
        self.status = status;
    }

    fn approve_selected_commits(&mut self) {
        let ids: Vec<String> = self
            .commits
            .iter()
            .filter(|row| row.selected && !row.approved)
            .map(|row| row.info.id.clone())
            .collect();
        if ids.is_empty() {
            self.status = "No selected commits to approve".to_owned();
            return;
        }

        self.store.mark_many_approved(
            &mut self.review_state,
            ids.clone(),
            ApprovalScope::Selection,
            self.git.branch_name(),
        );

        let id_set = ids.iter().cloned().collect::<BTreeSet<_>>();
        apply_approved_ids(&mut self.commits, &id_set);

        let status = if let Err(err) = self.store.save(&self.review_state) {
            format!("failed to persist approvals: {err:#}")
        } else {
            format!("Approved {} selected commit(s)", ids.len())
        };
        self.visual_anchor = None;
        self.on_selection_changed();
        self.status = status;
    }

    fn approve_branch_commits(&mut self) {
        let ids: Vec<String> = self
            .commits
            .iter()
            .filter(|row| !row.approved)
            .map(|row| row.info.id.clone())
            .collect();

        if ids.is_empty() {
            self.status = "All commits already approved".to_owned();
            return;
        }

        self.store.mark_many_approved(
            &mut self.review_state,
            ids.clone(),
            ApprovalScope::Branch,
            self.git.branch_name(),
        );

        let id_set = ids.iter().cloned().collect::<BTreeSet<_>>();
        apply_approved_ids(&mut self.commits, &id_set);

        let status = if let Err(err) = self.store.save(&self.review_state) {
            format!("failed to persist branch approvals: {err:#}")
        } else {
            format!("Approved {} commit(s) on branch", ids.len())
        };
        self.visual_anchor = None;
        self.on_selection_changed();
        self.status = status;
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
        let visible = self.pane_rects.diff.height.saturating_sub(2).max(1) as f32;
        let step = (visible * multiplier).round() as isize;
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
}

#[derive(Default)]
struct FileTree {
    dirs: BTreeMap<String, FileTree>,
    files: BTreeSet<String>,
}

impl FileTree {
    fn insert(&mut self, path: &str) {
        let segments: Vec<&str> = path.split('/').collect();
        if segments.is_empty() {
            return;
        }

        let mut cursor = self;
        for segment in &segments[..segments.len().saturating_sub(1)] {
            cursor = cursor.dirs.entry((*segment).to_owned()).or_default();
        }

        if let Some(name) = segments.last() {
            cursor.files.insert((*name).to_owned());
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
            });
            child.flatten_into(rows, path, depth + 1);
        }

        for file in &self.files {
            let full = if prefix.is_empty() {
                file.clone()
            } else {
                format!("{prefix}/{file}")
            };
            rows.push(TreeRow {
                label: format!("{}[F] {}", "  ".repeat(depth), file),
                path: Some(full),
                selectable: true,
            });
        }
    }
}

struct DiffSyntaxHighlighter {
    syntaxes: SyntaxSet,
    theme: Theme,
}

impl DiffSyntaxHighlighter {
    fn new() -> Self {
        let syntaxes = SyntaxSet::load_defaults_newlines();
        let theme_set = ThemeSet::load_defaults();
        let theme = theme_set
            .themes
            .get("base16-ocean.dark")
            .cloned()
            .or_else(|| theme_set.themes.values().next().cloned())
            .unwrap_or_default();

        Self { syntaxes, theme }
    }

    fn highlight(&self, path: &str, line: &str) -> Vec<Span<'static>> {
        let syntax = self
            .syntaxes
            .find_syntax_for_file(path)
            .ok()
            .flatten()
            .unwrap_or_else(|| self.syntaxes.find_syntax_plain_text());

        let mut highlighter = HighlightLines::new(syntax, &self.theme);
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

fn selected_ids_oldest_first(rows: &[CommitRow]) -> Vec<String> {
    rows.iter()
        .rev()
        .filter(|row| row.selected)
        .map(|row| row.info.id.clone())
        .collect()
}

fn apply_range_selection(rows: &mut [CommitRow], start: usize, end: usize) {
    let start = min(start, end);
    let end = max(start, end);
    for (idx, row) in rows.iter_mut().enumerate() {
        row.selected = !row.approved && idx >= start && idx <= end;
    }
}

fn apply_approved_ids(rows: &mut [CommitRow], ids: &BTreeSet<String>) {
    for row in rows {
        if ids.contains(&row.info.id) {
            row.approved = true;
            row.selected = false;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn commit_row(id: &str, selected: bool, approved: bool) -> CommitRow {
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
            approved,
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
        tree.insert("src/app.rs");
        tree.insert("src/ui/view.rs");
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
            commit_row("newest", true, false),
            commit_row("middle", false, false),
            commit_row("oldest", true, false),
        ];
        assert_eq!(
            selected_ids_oldest_first(&rows),
            vec!["oldest".to_owned(), "newest".to_owned()]
        );
    }

    #[test]
    fn range_selection_skips_already_approved_commits() {
        let mut rows = vec![
            commit_row("a", false, false),
            commit_row("b", false, true),
            commit_row("c", false, false),
        ];
        apply_range_selection(&mut rows, 0, 2);

        assert!(rows[0].selected);
        assert!(!rows[1].selected);
        assert!(rows[2].selected);
    }

    #[test]
    fn apply_approved_ids_marks_rows_and_clears_selection() {
        let mut rows = vec![commit_row("a", true, false), commit_row("b", true, false)];
        let ids = BTreeSet::from(["b".to_owned()]);

        apply_approved_ids(&mut rows, &ids);

        assert!(rows[0].selected);
        assert!(!rows[0].approved);
        assert!(!rows[1].selected);
        assert!(rows[1].approved);
    }
}
