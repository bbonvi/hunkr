use super::*;

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
            pending_diff_view_anchor: None,
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
            commits: left_chunks[0],
            files: left_chunks[1],
            diff: main_chunks[1],
        };

        self.render_header(frame, root_chunks[0], &theme);
        self.render_commits(frame, self.pane_rects.commits, &theme);
        self.render_files(frame, self.pane_rects.files, &theme);
        self.render_diff(frame, self.pane_rects.diff, &theme);
        self.render_footer(frame, root_chunks[2], &theme);
        if self.show_help {
            self.render_help_overlay(frame, &theme);
        }
    }

    pub(super) fn render_header(
        &self,
        frame: &mut Frame<'_>,
        rect: ratatui::layout::Rect,
        theme: &UiTheme,
    ) {
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

    pub(super) fn handle_key(&mut self, key: KeyEvent) {
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
            KeyCode::Char('1') => self.focused = FocusPane::Commits,
            KeyCode::Char('2') => self.focused = FocusPane::Files,
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

    pub(super) fn refresh_now(&mut self) {
        if let Err(err) = self.reload_commits(true) {
            self.status = format!("reload failed: {err:#}");
        }
    }

    pub(super) fn toggle_theme(&mut self) {
        self.theme_mode = self.theme_mode.toggle();
        self.rendered_diff_key = None;
        self.status = format!("Theme switched to {}", self.theme_mode.label());
    }

    pub(super) fn handle_non_normal_input(&mut self, key: KeyEvent) {
        match self.input_mode {
            InputMode::CommentCreate | InputMode::CommentEdit(_) => self.handle_comment_input(key),
            InputMode::DiffSearch => self.handle_diff_search_input(key),
            InputMode::Normal => {}
        }
    }

    pub(super) fn handle_comment_input(&mut self, key: KeyEvent) {
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

    pub(super) fn handle_diff_search_input(&mut self, key: KeyEvent) {
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

    pub(super) fn execute_diff_search(&mut self, query: &str, forward: bool) {
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

    pub(super) fn repeat_diff_search(&mut self, forward: bool) {
        let Some(query) = self.diff_search_query.clone() else {
            self.status = "No previous diff search".to_owned();
            return;
        };
        self.execute_diff_search(&query, forward);
    }

    pub(super) fn start_comment_edit_mode(&mut self) {
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

    pub(super) fn delete_current_comment(&mut self) {
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

    pub(super) fn dispatch_focus_key(&mut self, key: KeyEvent) {
        if self.focused != FocusPane::Diff {
            self.diff_pending_op = None;
        }
        match self.focused {
            FocusPane::Files => self.handle_files_key(key),
            FocusPane::Commits => self.handle_commits_key(key),
            FocusPane::Diff => self.handle_diff_key(key),
        }
    }

    pub(super) fn handle_files_key(&mut self, key: KeyEvent) {
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

    pub(super) fn handle_commits_key(&mut self, key: KeyEvent) {
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

    pub(super) fn handle_diff_key(&mut self, key: KeyEvent) {
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

    pub(super) fn handle_mouse(&mut self, mouse: crossterm::event::MouseEvent) {
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
                    if let Some(row) = diff_index_at(
                        y,
                        self.pane_rects.diff,
                        self.diff_position.scroll,
                        self.sticky_commit_banner_index_for_scroll(self.diff_position.scroll),
                    ) {
                        self.set_diff_cursor(row);
                    }
                }
            }
            _ => {}
        }
    }

    pub(super) fn render_files(
        &mut self,
        frame: &mut Frame<'_>,
        rect: ratatui::layout::Rect,
        theme: &UiTheme,
    ) {
        ListPaneRenderer::new(theme, self.focused).render_files(
            frame,
            rect,
            &self.file_rows,
            self.aggregate.files.len(),
            &mut self.file_list_state,
        );
    }

    pub(super) fn render_commits(
        &mut self,
        frame: &mut Frame<'_>,
        rect: ratatui::layout::Rect,
        theme: &UiTheme,
    ) {
        ListPaneRenderer::new(theme, self.focused).render_commits(
            frame,
            rect,
            &self.commits,
            self.status_counts(),
            &mut self.commit_list_state,
        );
    }

    pub(super) fn render_diff(
        &mut self,
        frame: &mut Frame<'_>,
        rect: ratatui::layout::Rect,
        theme: &UiTheme,
    ) {
        let selected_lines = self
            .diff_selected_range()
            .map(|(start, end)| end.saturating_sub(start) + 1)
            .unwrap_or(0);
        let sticky_commit_idx =
            self.sticky_commit_banner_index_for_scroll(self.diff_position.scroll);
        DiffPaneRenderer::new(theme, self.focused).render(
            frame,
            rect,
            self.selected_file.as_deref(),
            selected_lines,
            &self.rendered_diff,
            self.diff_position,
            self.diff_selected_range(),
            sticky_commit_idx,
        );
    }

    pub(super) fn render_footer(
        &self,
        frame: &mut Frame<'_>,
        rect: ratatui::layout::Rect,
        theme: &UiTheme,
    ) {
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

    pub(super) fn render_help_overlay(&self, frame: &mut Frame<'_>, theme: &UiTheme) {
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
                Span::raw(" focus commits/files/diff"),
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
}
