use super::*;
use crate::config::AppConfig;

impl App {
    pub fn bootstrap() -> anyhow::Result<Self> {
        let config = AppConfig::load()?;
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
            theme_mode: ThemeMode::from_startup_theme(config.startup_theme),
            diff_wheel_scroll_lines: config.diff_wheel_scroll_lines,
            list_wheel_coalesce: Duration::from_millis(config.list_wheel_coalesce_ms),
            nerd_fonts: config.nerd_fonts,
            nerd_font_theme: NerdFontTheme::default(),
            commit_visual_anchor: None,
            commit_mouse_anchor: None,
            commit_mouse_dragging: false,
            last_list_wheel_event: None,
            diff_visual: None,
            diff_mouse_anchor: None,
            aggregate: AggregatedDiff::default(),
            selected_file: None,
            diff_positions: HashMap::new(),
            file_diff_ranges: Vec::new(),
            file_diff_range_by_path: HashMap::new(),
            pending_diff_view_anchor: None,
            diff_position: DiffPosition::default(),
            rendered_diff: Arc::new(Vec::new()),
            rendered_diff_cache: HashMap::new(),
            rendered_diff_key: None,
            highlighter: DiffSyntaxHighlighter::new(),
            pane_rects: PaneRects::default(),
            status: String::new(),
            comment_buffer: String::new(),
            comment_cursor: 0,
            comment_selection: None,
            comment_mouse_anchor: None,
            comment_editor_rect: None,
            comment_editor_line_ranges: Vec::new(),
            comment_editor_view_start: 0,
            comment_editor_text_offset: 0,
            diff_search_buffer: String::new(),
            diff_search_query: None,
            commit_search_query: String::new(),
            file_search_query: String::new(),
            commit_status_filter: CommitStatusFilter::All,
            diff_pending_op: None,
            selection_rebuild_due: None,
            show_help: false,
            last_refresh: Instant::now(),
            last_relative_time_redraw: Instant::now(),
            needs_redraw: true,
            should_quit: false,
        };

        app.reload_commits(true)?;
        app.ensure_rendered_diff();
        if app.status.is_empty() {
            app.status =
                "Ready. Select commit range with <space>/v, set statuses with r/i/s/u.".to_owned();
        }
        Ok(app)
    }

    pub fn should_quit(&self) -> bool {
        self.should_quit
    }

    pub fn needs_redraw(&self) -> bool {
        self.needs_redraw
    }

    pub fn mark_drawn(&mut self) {
        self.needs_redraw = false;
    }

    pub fn poll_timeout(&self) -> Duration {
        let selection_rebuild_in = self
            .selection_rebuild_due
            .map(|due| due.saturating_duration_since(Instant::now()));
        next_poll_timeout(
            self.last_refresh.elapsed(),
            self.last_relative_time_redraw.elapsed(),
            selection_rebuild_in,
        )
    }

    pub fn draw(&mut self, frame: &mut Frame<'_>) {
        self.ensure_rendered_diff();
        let theme = UiTheme::from_mode(self.theme_mode);
        self.comment_editor_rect = None;
        self.comment_editor_line_ranges.clear();
        self.comment_editor_view_start = 0;
        self.comment_editor_text_offset = 0;

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
        if matches!(
            self.input_mode,
            InputMode::CommentCreate | InputMode::CommentEdit(_)
        ) {
            self.render_comment_modal(frame, &theme);
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
                app_title_label(self.nerd_fonts),
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
            Span::styled(
                format!("nf:{} ", if self.nerd_fonts { "on" } else { "off" }),
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
        let now = Instant::now();
        if self.selection_rebuild_due.is_some_and(|due| now >= due) {
            self.flush_pending_selection_rebuild();
            self.needs_redraw = true;
        }

        let mut refreshed = false;
        if self.last_refresh.elapsed() >= AUTO_REFRESH_EVERY {
            if let Err(err) = self.reload_commits(true) {
                self.status = format!("refresh failed: {err:#}");
            }
            self.last_refresh = Instant::now();
            refreshed = true;
            self.needs_redraw = true;
        }

        if refreshed {
            self.last_relative_time_redraw = Instant::now();
        } else if self.last_relative_time_redraw.elapsed() >= RELATIVE_TIME_REDRAW_EVERY {
            self.last_relative_time_redraw = Instant::now();
            self.needs_redraw = true;
        }
    }

    pub fn handle_event(&mut self, event: Event) {
        let mut should_redraw = false;
        match event {
            Event::Key(key) if key.kind == KeyEventKind::Press => {
                self.handle_key(key);
                should_redraw = true;
            }
            Event::Mouse(mouse) => {
                self.handle_mouse(mouse);
                should_redraw = true;
            }
            Event::Resize(_, _) => {
                self.ensure_cursor_visible();
                should_redraw = true;
            }
            _ => {}
        }
        if should_redraw {
            self.needs_redraw = true;
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
                self.set_focus(focus_with_l(self.focused))
            }
            KeyCode::Char('h') if key.modifiers == KeyModifiers::NONE => {
                self.set_focus(focus_with_h(self.focused))
            }
            KeyCode::Char('1') => self.set_focus(FocusPane::Commits),
            KeyCode::Char('2') => self.set_focus(FocusPane::Files),
            KeyCode::Char('3') => self.set_focus(FocusPane::Diff),
            KeyCode::Char('f') if key.modifiers == KeyModifiers::NONE => {
                self.set_focus(FocusPane::Files)
            }
            KeyCode::Char('c') if key.modifiers == KeyModifiers::NONE => {
                self.set_focus(FocusPane::Commits)
            }
            KeyCode::Char('d') if key.modifiers == KeyModifiers::NONE => {
                self.set_focus(FocusPane::Diff)
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
        let now = Instant::now();
        self.last_refresh = now;
        self.last_relative_time_redraw = now;
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
            InputMode::ListSearch(pane) => self.handle_list_search_input(pane, key),
            InputMode::Normal => {}
        }
    }

    pub(super) fn handle_comment_input(&mut self, key: KeyEvent) {
        if key.modifiers.contains(KeyModifiers::CONTROL)
            && matches!(key.code, KeyCode::Char('s') | KeyCode::Char('S'))
        {
            self.submit_comment_input();
            return;
        }

        match key.code {
            KeyCode::Esc => self.cancel_comment_input(),
            KeyCode::Enter
                if key
                    .modifiers
                    .intersects(KeyModifiers::SHIFT | KeyModifiers::ALT) =>
            {
                delete_selection_range(
                    &mut self.comment_buffer,
                    &mut self.comment_cursor,
                    &mut self.comment_selection,
                );
                insert_char_at_cursor(&mut self.comment_buffer, &mut self.comment_cursor, '\n');
                self.comment_mouse_anchor = None;
            }
            KeyCode::Enter => self.submit_comment_input(),
            KeyCode::Backspace
                if key
                    .modifiers
                    .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT) =>
            {
                if !delete_selection_range(
                    &mut self.comment_buffer,
                    &mut self.comment_cursor,
                    &mut self.comment_selection,
                ) {
                    delete_prev_word(&mut self.comment_buffer, &mut self.comment_cursor);
                }
                self.comment_mouse_anchor = None;
            }
            KeyCode::Backspace => {
                if !delete_selection_range(
                    &mut self.comment_buffer,
                    &mut self.comment_cursor,
                    &mut self.comment_selection,
                ) {
                    delete_prev_char(&mut self.comment_buffer, &mut self.comment_cursor);
                }
                self.comment_mouse_anchor = None;
            }
            KeyCode::Delete
                if key
                    .modifiers
                    .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT) =>
            {
                if !delete_selection_range(
                    &mut self.comment_buffer,
                    &mut self.comment_cursor,
                    &mut self.comment_selection,
                ) {
                    delete_next_word(&mut self.comment_buffer, &mut self.comment_cursor);
                }
                self.comment_mouse_anchor = None;
            }
            KeyCode::Delete => {
                if !delete_selection_range(
                    &mut self.comment_buffer,
                    &mut self.comment_cursor,
                    &mut self.comment_selection,
                ) {
                    delete_next_char(&mut self.comment_buffer, &mut self.comment_cursor);
                }
                self.comment_mouse_anchor = None;
            }
            KeyCode::Left
                if key
                    .modifiers
                    .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT) =>
            {
                self.comment_cursor = prev_word_boundary(&self.comment_buffer, self.comment_cursor);
                self.comment_selection = None;
                self.comment_mouse_anchor = None;
            }
            KeyCode::Left => {
                self.comment_cursor = prev_char_boundary(
                    &self.comment_buffer,
                    clamp_char_boundary(&self.comment_buffer, self.comment_cursor),
                );
                self.comment_selection = None;
                self.comment_mouse_anchor = None;
            }
            KeyCode::Right
                if key
                    .modifiers
                    .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT) =>
            {
                self.comment_cursor = next_word_boundary(&self.comment_buffer, self.comment_cursor);
                self.comment_selection = None;
                self.comment_mouse_anchor = None;
            }
            KeyCode::Right => {
                self.comment_cursor = next_char_boundary(
                    &self.comment_buffer,
                    clamp_char_boundary(&self.comment_buffer, self.comment_cursor),
                );
                self.comment_selection = None;
                self.comment_mouse_anchor = None;
            }
            KeyCode::Up => {
                self.comment_cursor = move_cursor_up(&self.comment_buffer, self.comment_cursor);
                self.comment_selection = None;
                self.comment_mouse_anchor = None;
            }
            KeyCode::Down => {
                self.comment_cursor = move_cursor_down(&self.comment_buffer, self.comment_cursor);
                self.comment_selection = None;
                self.comment_mouse_anchor = None;
            }
            KeyCode::Home => {
                self.comment_cursor = 0;
                self.comment_selection = None;
                self.comment_mouse_anchor = None;
            }
            KeyCode::End => {
                self.comment_cursor = self.comment_buffer.len();
                self.comment_selection = None;
                self.comment_mouse_anchor = None;
            }
            KeyCode::Char('a') | KeyCode::Char('A')
                if key.modifiers.contains(KeyModifiers::CONTROL) =>
            {
                self.comment_cursor = 0;
                self.comment_selection = None;
                self.comment_mouse_anchor = None;
            }
            KeyCode::Char('e') | KeyCode::Char('E')
                if key.modifiers.contains(KeyModifiers::CONTROL) =>
            {
                self.comment_cursor = self.comment_buffer.len();
                self.comment_selection = None;
                self.comment_mouse_anchor = None;
            }
            KeyCode::Char('w') | KeyCode::Char('W')
                if key.modifiers.contains(KeyModifiers::CONTROL) =>
            {
                if !delete_selection_range(
                    &mut self.comment_buffer,
                    &mut self.comment_cursor,
                    &mut self.comment_selection,
                ) {
                    delete_prev_word(&mut self.comment_buffer, &mut self.comment_cursor);
                }
                self.comment_mouse_anchor = None;
            }
            KeyCode::Char('u') | KeyCode::Char('U')
                if key.modifiers.contains(KeyModifiers::CONTROL) =>
            {
                if !delete_selection_range(
                    &mut self.comment_buffer,
                    &mut self.comment_cursor,
                    &mut self.comment_selection,
                ) {
                    delete_to_line_start(&mut self.comment_buffer, &mut self.comment_cursor);
                }
                self.comment_mouse_anchor = None;
            }
            KeyCode::Char('k') | KeyCode::Char('K')
                if key.modifiers.contains(KeyModifiers::CONTROL) =>
            {
                if !delete_selection_range(
                    &mut self.comment_buffer,
                    &mut self.comment_cursor,
                    &mut self.comment_selection,
                ) {
                    delete_to_line_end(&mut self.comment_buffer, &mut self.comment_cursor);
                }
                self.comment_mouse_anchor = None;
            }
            KeyCode::Char('b') | KeyCode::Char('B')
                if key.modifiers.contains(KeyModifiers::ALT) =>
            {
                self.comment_cursor = prev_word_boundary(&self.comment_buffer, self.comment_cursor);
                self.comment_selection = None;
                self.comment_mouse_anchor = None;
            }
            KeyCode::Char('f') | KeyCode::Char('F')
                if key.modifiers.contains(KeyModifiers::ALT) =>
            {
                self.comment_cursor = next_word_boundary(&self.comment_buffer, self.comment_cursor);
                self.comment_selection = None;
                self.comment_mouse_anchor = None;
            }
            KeyCode::Char('d') | KeyCode::Char('D')
                if key.modifiers.contains(KeyModifiers::ALT) =>
            {
                if !delete_selection_range(
                    &mut self.comment_buffer,
                    &mut self.comment_cursor,
                    &mut self.comment_selection,
                ) {
                    delete_next_word(&mut self.comment_buffer, &mut self.comment_cursor);
                }
                self.comment_mouse_anchor = None;
            }
            KeyCode::Char(c)
                if key.modifiers == KeyModifiers::NONE || key.modifiers == KeyModifiers::SHIFT =>
            {
                delete_selection_range(
                    &mut self.comment_buffer,
                    &mut self.comment_cursor,
                    &mut self.comment_selection,
                );
                insert_char_at_cursor(&mut self.comment_buffer, &mut self.comment_cursor, c);
                self.comment_mouse_anchor = None;
            }
            _ => {}
        }
    }

    fn cancel_comment_input(&mut self) {
        self.input_mode = InputMode::Normal;
        self.clear_diff_visual_selection();
        self.comment_buffer.clear();
        self.comment_cursor = 0;
        self.comment_selection = None;
        self.comment_mouse_anchor = None;
        self.comment_editor_rect = None;
        self.comment_editor_line_ranges.clear();
        self.comment_editor_view_start = 0;
        self.comment_editor_text_offset = 0;
        self.status = "Comment canceled".to_owned();
    }

    fn submit_comment_input(&mut self) {
        if self.comment_buffer.trim().is_empty() {
            self.status = "Comment is empty".to_owned();
            return;
        }

        let mut close_editor = false;
        match self.input_mode {
            InputMode::CommentCreate => match self.comment_target_from_selection() {
                Ok(Some(target)) => {
                    let result = self.comments.add_comment(&target, &self.comment_buffer);
                    match result {
                        Ok(id) => {
                            self.set_status_for_ids(&target.commits, ReviewStatus::IssueFound);
                            self.invalidate_diff_cache();
                            if let Err(err) = self.sync_comment_report() {
                                self.status = format!(
                                    "Comment #{} added, but review tasks sync failed: {err:#}",
                                    id
                                );
                                close_editor = true;
                            } else {
                                self.status = format!(
                                    "Comment #{} added -> {} ({} commit(s) marked ISSUE_FOUND)",
                                    id,
                                    self.comments.report_path().display(),
                                    target.commits.len()
                                );
                                close_editor = true;
                            }
                        }
                        Err(err) => {
                            self.status = format!("Failed to save comment: {err:#}");
                        }
                    }
                }
                Ok(None) => {
                    self.status = if self.diff_selection_spans_multiple_files() {
                        "Comment range must stay within a single file".to_owned()
                    } else {
                        "No hunk/line anchor at cursor or selected range".to_owned()
                    };
                    close_editor = true;
                }
                Err(err) => {
                    self.status =
                        format!("Failed to resolve affected commits for comment: {err:#}");
                    close_editor = true;
                }
            },
            InputMode::CommentEdit(id) => {
                match self.comments.update_comment(id, &self.comment_buffer) {
                    Ok(true) => {
                        self.invalidate_diff_cache();
                        if let Err(err) = self.sync_comment_report() {
                            self.status = format!(
                                "Comment #{} updated, but review tasks sync failed: {err:#}",
                                id
                            );
                        } else {
                            self.status = format!("Comment #{} updated", id);
                        }
                        close_editor = true;
                    }
                    Ok(false) => {
                        self.status = format!("Comment #{} not found", id);
                        close_editor = true;
                    }
                    Err(err) => {
                        self.status = format!("Failed to update comment #{}: {err:#}", id);
                    }
                }
            }
            InputMode::DiffSearch | InputMode::ListSearch(_) | InputMode::Normal => {}
        }

        if close_editor {
            self.input_mode = InputMode::Normal;
            self.clear_diff_visual_selection();
            self.comment_buffer.clear();
            self.comment_cursor = 0;
            self.comment_selection = None;
            self.comment_mouse_anchor = None;
            self.comment_editor_rect = None;
            self.comment_editor_line_ranges.clear();
            self.comment_editor_view_start = 0;
            self.comment_editor_text_offset = 0;
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

    pub(super) fn handle_list_search_input(&mut self, pane: FocusPane, key: KeyEvent) {
        let preferred_commit_id = (pane == FocusPane::Commits)
            .then(|| self.selected_commit_id())
            .flatten();
        let fallback_visible_idx = self.commit_list_state.selected();

        match key.code {
            KeyCode::Esc => {
                self.input_mode = InputMode::Normal;
                match pane {
                    FocusPane::Commits => {
                        self.commit_search_query.clear();
                        self.sync_commit_cursor_for_filters(
                            preferred_commit_id.as_deref(),
                            fallback_visible_idx,
                        );
                        self.status = "Commit search cleared".to_owned();
                    }
                    FocusPane::Files => {
                        self.file_search_query.clear();
                        self.sync_file_cursor_for_filters();
                        self.status = "File search cleared".to_owned();
                    }
                    FocusPane::Diff => {}
                }
            }
            KeyCode::Enter => {
                self.input_mode = InputMode::Normal;
                let query = match pane {
                    FocusPane::Commits => self.commit_search_query.trim(),
                    FocusPane::Files => self.file_search_query.trim(),
                    FocusPane::Diff => "",
                };
                self.status = if query.is_empty() {
                    match pane {
                        FocusPane::Commits => {
                            format!("Commit search off ({})", self.commit_status_filter.label())
                        }
                        FocusPane::Files => "File search off".to_owned(),
                        FocusPane::Diff => "Search off".to_owned(),
                    }
                } else {
                    match pane {
                        FocusPane::Commits => format!(
                            "Commit filter: /{} ({})",
                            query,
                            self.commit_status_filter.label()
                        ),
                        FocusPane::Files => format!("File filter: /{query}"),
                        FocusPane::Diff => format!("/{query}"),
                    }
                };
            }
            KeyCode::Backspace => match pane {
                FocusPane::Commits => {
                    self.commit_search_query.pop();
                    self.sync_commit_cursor_for_filters(
                        preferred_commit_id.as_deref(),
                        fallback_visible_idx,
                    );
                    self.status = format!("/{}", self.commit_search_query);
                }
                FocusPane::Files => {
                    self.file_search_query.pop();
                    self.sync_file_cursor_for_filters();
                    self.status = format!("/{}", self.file_search_query);
                }
                FocusPane::Diff => {}
            },
            KeyCode::Char(c) => {
                if key.modifiers.contains(KeyModifiers::CONTROL) {
                    return;
                }
                match pane {
                    FocusPane::Commits => {
                        self.commit_search_query.push(c);
                        self.sync_commit_cursor_for_filters(
                            preferred_commit_id.as_deref(),
                            fallback_visible_idx,
                        );
                        self.status = format!("/{}", self.commit_search_query);
                    }
                    FocusPane::Files => {
                        self.file_search_query.push(c);
                        self.sync_file_cursor_for_filters();
                        self.status = format!("/{}", self.file_search_query);
                    }
                    FocusPane::Diff => {}
                }
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
        self.comment_cursor = self.comment_buffer.len();
        self.comment_selection = None;
        self.comment_mouse_anchor = None;
        self.status = format!(
            "Editing comment #{}: Enter save, Ctrl-s save, Esc cancel",
            id
        );
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
            KeyCode::Char('/') if key.modifiers == KeyModifiers::NONE => {
                self.input_mode = InputMode::ListSearch(FocusPane::Files);
                self.status = format!("/{}", self.file_search_query);
            }
            KeyCode::Enter | KeyCode::Char(' ') => self.set_focus(FocusPane::Diff),
            _ => {}
        }
    }

    pub(super) fn handle_commits_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Esc => {
                if self.commit_visual_anchor.is_some() {
                    self.commit_visual_anchor = None;
                    self.status = "Commit visual range off".to_owned();
                }
            }
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
            KeyCode::Char('/') if key.modifiers == KeyModifiers::NONE => {
                self.input_mode = InputMode::ListSearch(FocusPane::Commits);
                self.status = format!("/{}", self.commit_search_query);
            }
            KeyCode::Char('v') => {
                if self.commit_visual_anchor.is_some() {
                    self.commit_visual_anchor = None;
                    self.status = "Commit visual range off".to_owned();
                } else {
                    self.commit_visual_anchor = self.selected_commit_full_index();
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
                if let Some(idx) = self.selected_commit_full_index()
                    && let Some(row) = self.commits.get_mut(idx)
                {
                    row.selected = !row.selected;
                }
                self.commit_visual_anchor = None;
                self.on_selection_changed();
            }
            KeyCode::Enter => {
                if let Some(idx) = self.selected_commit_full_index() {
                    select_only_index(&mut self.commits, idx);
                    self.commit_visual_anchor = None;
                    self.on_selection_changed();
                }
            }
            KeyCode::Char('e') if key.modifiers == KeyModifiers::NONE => {
                self.cycle_commit_status_filter();
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
                    self.clear_diff_visual_selection();
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
                    self.clear_diff_visual_selection();
                    self.status = "Diff visual range off".to_owned();
                } else {
                    self.diff_mouse_anchor = None;
                    self.diff_visual = Some(DiffVisualSelection {
                        anchor: self.diff_position.cursor,
                        origin: DiffVisualOrigin::Keyboard,
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
                self.comment_cursor = 0;
                self.comment_selection = None;
                self.comment_mouse_anchor = None;
                self.diff_pending_op = None;
                self.status = "Comment mode: Enter save, Alt+Enter newline, Esc cancel".to_owned();
            }
            KeyCode::Char('e') => {
                if self.uncommitted_selected() {
                    self.status = "Comments are disabled for uncommitted changes".to_owned();
                    return;
                }
                self.start_comment_edit_mode();
            }
            KeyCode::Char('y') if key.modifiers == KeyModifiers::NONE => {
                self.copy_review_tasks_path();
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
        if matches!(
            self.input_mode,
            InputMode::CommentCreate | InputMode::CommentEdit(_)
        ) {
            self.handle_comment_mouse(mouse);
            return;
        }
        let x = mouse.column;
        let y = mouse.row;

        let in_files = contains(self.pane_rects.files, x, y);
        let in_commits = contains(self.pane_rects.commits, x, y);
        let in_diff = contains(self.pane_rects.diff, x, y);
        let resolve_diff_row = |app: &Self, mouse_y: u16| -> Option<usize> {
            let viewport_rows = app.pane_rects.diff.height.saturating_sub(2).max(1) as usize;
            let sticky_banner_indexes =
                app.sticky_banner_indexes_for_scroll(app.diff_position.scroll, viewport_rows);
            diff_index_at(
                mouse_y,
                app.pane_rects.diff,
                app.diff_position.scroll,
                &sticky_banner_indexes,
            )
        };
        let resolve_commit_visible_idx = |app: &Self, mouse_y: u16| -> Option<usize> {
            list_index_at(
                mouse_y,
                app.pane_rects.commits,
                app.commit_list_state.offset(),
            )
        };

        match mouse.kind {
            MouseEventKind::ScrollDown => {
                if in_diff {
                    self.clear_keyboard_diff_visual_selection();
                    self.scroll_diff_viewport(self.diff_wheel_scroll_lines);
                } else if in_files && self.should_scroll_list_wheel(FocusPane::Files, 1) {
                    self.scroll_file_list_lines(1);
                } else if in_commits && self.should_scroll_list_wheel(FocusPane::Commits, 1) {
                    self.commit_visual_anchor = None;
                    self.scroll_commit_list_lines(1);
                }
            }
            MouseEventKind::ScrollUp => {
                if in_diff {
                    self.clear_keyboard_diff_visual_selection();
                    self.scroll_diff_viewport(-self.diff_wheel_scroll_lines);
                } else if in_files && self.should_scroll_list_wheel(FocusPane::Files, -1) {
                    self.scroll_file_list_lines(-1);
                } else if in_commits && self.should_scroll_list_wheel(FocusPane::Commits, -1) {
                    self.commit_visual_anchor = None;
                    self.scroll_commit_list_lines(-1);
                }
            }
            MouseEventKind::Down(MouseButton::Left) => {
                self.commit_mouse_anchor = None;
                self.commit_mouse_dragging = false;
                if in_files {
                    self.set_focus(FocusPane::Files);
                    self.diff_mouse_anchor = None;
                    if let Some(idx) =
                        list_index_at(y, self.pane_rects.files, self.file_list_state.offset())
                    {
                        self.select_file_row(idx);
                    }
                } else if in_commits {
                    self.set_focus(FocusPane::Commits);
                    self.diff_mouse_anchor = None;
                    self.commit_visual_anchor = None;
                    if let Some(idx) = resolve_commit_visible_idx(self, y)
                        && let Some(full_idx) = self.visible_commit_indices().get(idx).copied()
                    {
                        self.commit_mouse_anchor = Some(full_idx);
                        self.select_commit_row(idx, true);
                    }
                } else if in_diff {
                    self.set_focus(FocusPane::Diff);
                    self.diff_visual = None;
                    if let Some(row) = resolve_diff_row(self, y) {
                        self.set_diff_cursor(row);
                        self.diff_mouse_anchor = Some(self.diff_position.cursor);
                    } else {
                        self.diff_mouse_anchor = None;
                    }
                } else {
                    self.diff_mouse_anchor = None;
                }
            }
            MouseEventKind::Drag(MouseButton::Left) if self.commit_mouse_anchor.is_some() => {
                let edge_delta =
                    list_drag_scroll_delta(y, self.pane_rects.commits, LIST_DRAG_EDGE_MARGIN);
                if edge_delta != 0 {
                    self.scroll_commit_list_lines(edge_delta);
                }

                let target_visible_idx =
                    resolve_commit_visible_idx(self, y).or(self.commit_list_state.selected());
                if let Some(visible_idx) = target_visible_idx {
                    let visible_indices = self.visible_commit_indices();
                    if let Some(full_idx) = visible_indices.get(visible_idx).copied() {
                        self.commit_list_state.select(Some(visible_idx));
                        let anchor = self.commit_mouse_anchor.expect("checked above");
                        apply_range_selection(&mut self.commits, anchor, full_idx);
                        if anchor != full_idx {
                            self.commit_mouse_dragging = true;
                        }
                        if self.commit_mouse_dragging {
                            self.on_selection_changed_debounced();
                        }
                    }
                }
            }
            MouseEventKind::Drag(MouseButton::Left) if in_diff => {
                if let Some(row) = resolve_diff_row(self, y) {
                    self.set_diff_cursor(row);
                    self.diff_visual = diff_visual_from_drag_anchor(
                        self.diff_mouse_anchor,
                        self.diff_position.cursor,
                    );
                }
            }
            MouseEventKind::Up(MouseButton::Left) => {
                let commit_dragging = self.commit_mouse_dragging;
                self.commit_mouse_anchor = None;
                self.commit_mouse_dragging = false;

                if in_diff {
                    if let Some(row) = resolve_diff_row(self, y) {
                        self.set_diff_cursor(row);
                    }
                    self.diff_visual = diff_visual_from_drag_anchor(
                        self.diff_mouse_anchor,
                        self.diff_position.cursor,
                    );
                }
                self.diff_mouse_anchor = None;

                if commit_dragging {
                    self.on_selection_changed();
                }
            }
            _ => {}
        }
    }

    fn should_scroll_list_wheel(&mut self, pane: FocusPane, delta: isize) -> bool {
        let now = Instant::now();
        if list_wheel_event_is_duplicate(
            self.last_list_wheel_event,
            pane,
            delta,
            now,
            self.list_wheel_coalesce,
        ) {
            return false;
        }
        self.last_list_wheel_event = Some((pane, delta, now));
        true
    }

    fn clear_keyboard_diff_visual_selection(&mut self) {
        if should_clear_diff_visual_on_wheel(self.diff_visual) {
            self.clear_diff_visual_selection();
        }
    }

    fn handle_comment_mouse(&mut self, mouse: crossterm::event::MouseEvent) {
        let Some(editor_rect) = self.comment_editor_rect else {
            return;
        };
        if self.comment_editor_line_ranges.is_empty() {
            return;
        }
        let inside_editor = contains(editor_rect, mouse.column, mouse.row);
        let resolve_cursor = |app: &Self, x: u16, y: u16| -> usize {
            let row = y.saturating_sub(editor_rect.y) as usize;
            let line_idx =
                (app.comment_editor_view_start + row).min(app.comment_editor_line_ranges.len() - 1);
            let (line_start, line_end) = app.comment_editor_line_ranges[line_idx];
            let col = x
                .saturating_sub(editor_rect.x)
                .saturating_sub(app.comment_editor_text_offset)
                .min(editor_rect.width.saturating_sub(1)) as usize;
            line_cursor_with_column(&app.comment_buffer, line_start, line_end, col)
        };

        match mouse.kind {
            MouseEventKind::Down(MouseButton::Left) if inside_editor => {
                let idx = resolve_cursor(self, mouse.column, mouse.row);
                self.comment_cursor = idx;
                self.comment_selection = None;
                self.comment_mouse_anchor = Some(idx);
            }
            MouseEventKind::Drag(MouseButton::Left) if inside_editor => {
                let idx = resolve_cursor(self, mouse.column, mouse.row);
                self.comment_cursor = idx;
                if let Some(anchor) = self.comment_mouse_anchor {
                    self.comment_selection = (anchor != idx).then_some((anchor, idx));
                }
            }
            MouseEventKind::Up(MouseButton::Left) if inside_editor => {
                let idx = resolve_cursor(self, mouse.column, mouse.row);
                self.comment_cursor = idx;
                if let Some(anchor) = self.comment_mouse_anchor.take() {
                    self.comment_selection = (anchor != idx).then_some((anchor, idx));
                }
            }
            MouseEventKind::Down(MouseButton::Left) => {
                self.comment_mouse_anchor = None;
            }
            MouseEventKind::Up(MouseButton::Left) => {
                self.comment_mouse_anchor = None;
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
        let files_search_mode = matches!(self.input_mode, InputMode::ListSearch(FocusPane::Files));
        let file_query = self.file_search_query.trim();
        let files_search_display = if !file_query.is_empty() {
            format!("/{file_query}")
        } else if files_search_mode {
            "/".to_owned()
        } else {
            "off".to_owned()
        };
        let visible_indices = self.visible_file_indices();
        let visible_rows: Vec<TreeRow> = visible_indices
            .iter()
            .filter_map(|idx| self.file_rows.get(*idx).cloned())
            .collect();
        ListPaneRenderer::new(theme, self.focused, self.nerd_fonts).render_files(
            frame,
            rect,
            FilePaneModel {
                file_rows: &visible_rows,
                changed_files: self.aggregate.files.len(),
                shown_files: visible_rows.iter().filter(|row| row.selectable).count(),
                search_display: &files_search_display,
                search_enabled: files_search_mode || !file_query.is_empty(),
                file_list_state: &mut self.file_list_state,
            },
        );
    }

    pub(super) fn render_commits(
        &mut self,
        frame: &mut Frame<'_>,
        rect: ratatui::layout::Rect,
        theme: &UiTheme,
    ) {
        let commits_search_mode =
            matches!(self.input_mode, InputMode::ListSearch(FocusPane::Commits));
        let commit_query = self.commit_search_query.trim();
        let commits_search_display = if !commit_query.is_empty() {
            format!("/{commit_query}")
        } else if commits_search_mode {
            "/".to_owned()
        } else {
            "off".to_owned()
        };
        let visible_indices = self.visible_commit_indices();
        let visible_rows: Vec<CommitRow> = visible_indices
            .iter()
            .filter_map(|idx| self.commits.get(*idx).cloned())
            .collect();
        let selected_total = self.commits.iter().filter(|row| row.selected).count();
        ListPaneRenderer::new(theme, self.focused, self.nerd_fonts).render_commits(
            frame,
            rect,
            CommitPaneModel {
                commits: &visible_rows,
                status_counts: self.status_counts(),
                selected_total,
                shown_commits: visible_rows.len(),
                total_commits: self.commits.len(),
                status_filter: self.commit_status_filter.label(),
                search_display: &commits_search_display,
                search_enabled: commits_search_mode || !commit_query.is_empty(),
                commit_list_state: &mut self.commit_list_state,
            },
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
        let viewport_rows = rect.height.saturating_sub(2).max(1) as usize;
        let sticky_banner_indexes =
            self.sticky_banner_indexes_for_scroll(self.diff_position.scroll, viewport_rows);
        let sticky_rows = sticky_banner_indexes
            .len()
            .min(viewport_rows.saturating_sub(1));
        let body_rows = viewport_rows.saturating_sub(sticky_rows);
        let mut visible_indexes = BTreeSet::new();
        for idx in sticky_banner_indexes.iter().take(sticky_rows) {
            visible_indexes.insert(*idx);
        }
        for row in 0..body_rows {
            let idx = self.diff_position.scroll.saturating_add(row);
            if idx >= self.rendered_diff.len() {
                break;
            }
            visible_indexes.insert(idx);
        }
        let mut line_overrides = HashMap::new();
        for idx in visible_indexes {
            if let Some(line) = self.highlight_visible_diff_line(idx, theme) {
                line_overrides.insert(idx, line);
            }
        }
        let title = DiffPaneTitle {
            selected_file: self.selected_file.as_deref(),
            selected_file_progress: self.selected_file_progress(),
            nerd_fonts: self.nerd_fonts,
            nerd_font_theme: &self.nerd_font_theme,
            selected_lines,
        };
        let body = DiffPaneBody {
            rendered_diff: &self.rendered_diff,
            diff_position: self.diff_position,
            visual_range: self.diff_selected_range(),
            sticky_banner_indexes: &sticky_banner_indexes,
            empty_state_message: None,
            line_overrides: &line_overrides,
        };
        DiffPaneRenderer::new(theme, self.focused).render(frame, rect, title, body);
    }

    fn highlight_visible_diff_line(&self, idx: usize, theme: &UiTheme) -> Option<Line<'static>> {
        let rendered = self.rendered_diff.get(idx)?;
        let anchor = rendered.anchor.as_ref()?;
        if is_commit_anchor(anchor) {
            return None;
        }

        let mut chars = rendered.raw_text.chars();
        let prefix = chars.next()?;
        if !matches!(prefix, '+' | '-' | ' ') {
            return None;
        }
        if rendered.line.spans.len() < 4 {
            return None;
        }

        let code_text = chars.as_str();
        let mut spans = vec![
            rendered.line.spans[0].clone(),
            rendered.line.spans[1].clone(),
            rendered.line.spans[2].clone(),
        ];
        let mut highlighted =
            self.highlighter
                .highlight_single_line(self.theme_mode, &anchor.file_path, code_text);
        if highlighted.is_empty() {
            highlighted.push(Span::raw(code_text.to_owned()));
        }

        let bg = match prefix {
            '+' => Some(theme.diff_add_bg),
            '-' => Some(theme.diff_remove_bg),
            _ => None,
        };
        if let Some(bg_color) = bg {
            for span in &mut highlighted {
                span.style = span.style.bg(bg_color);
            }
        }
        spans.extend(highlighted);
        Some(Line::from(spans))
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
            InputMode::DiffSearch | InputMode::ListSearch(_) => "SEARCH/",
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
                key_chip("Alt+Enter", theme),
                Span::styled(" newline ", Style::default().fg(theme.muted)),
                key_chip("mouse", theme),
                Span::styled(" cursor/select ", Style::default().fg(theme.muted)),
                key_chip("Esc", theme),
                Span::styled(" cancel comment", Style::default().fg(theme.muted)),
            ]),
            InputMode::DiffSearch => Line::from(vec![
                key_chip("Enter", theme),
                Span::styled(" search ", Style::default().fg(theme.muted)),
                key_chip("Esc", theme),
                Span::styled(" cancel search", Style::default().fg(theme.muted)),
            ]),
            InputMode::ListSearch(_) => Line::from(vec![
                key_chip("Enter", theme),
                Span::styled(" defocus ", Style::default().fg(theme.muted)),
                key_chip("Esc", theme),
                Span::styled(" clear ", Style::default().fg(theme.muted)),
                key_chip("Backspace", theme),
                Span::styled(" edit", Style::default().fg(theme.muted)),
            ]),
            InputMode::Normal => match self.focused {
                FocusPane::Files => Line::from(vec![
                    key_chip("j/k", theme),
                    Span::styled(" move ", Style::default().fg(theme.muted)),
                    key_chip("Ctrl-d/u", theme),
                    Span::styled(" jump ", Style::default().fg(theme.muted)),
                    key_chip("/", theme),
                    Span::styled(" filter ", Style::default().fg(theme.muted)),
                    key_chip("Enter", theme),
                    Span::styled(" focus diff", Style::default().fg(theme.muted)),
                ]),
                FocusPane::Commits => Line::from(vec![
                    key_chip("space", theme),
                    Span::styled(" select ", Style::default().fg(theme.muted)),
                    key_chip("u/r/i/s", theme),
                    Span::styled(" current ", Style::default().fg(theme.muted)),
                    key_chip("U/R/I/S", theme),
                    Span::styled(" selected ", Style::default().fg(theme.muted)),
                    key_chip("e", theme),
                    Span::styled(" status filter ", Style::default().fg(theme.muted)),
                    key_chip("/", theme),
                    Span::styled(" search", Style::default().fg(theme.muted)),
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
                    key_chip("y", theme),
                    Span::styled(" copy task path ", Style::default().fg(theme.muted)),
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
            InputMode::CommentCreate | InputMode::CommentEdit(_) => {
                let line_count = self.comment_buffer.matches('\n').count() + 1;
                let (line, col) =
                    comment_cursor_line_col(&self.comment_buffer, self.comment_cursor);
                format!(
                    "{} | mode={} focus={} theme={} comment:{} chars line:{} col:{} ({line_count} lines)",
                    self.status,
                    mode,
                    focus,
                    self.theme_mode.label(),
                    self.comment_buffer.chars().count(),
                    line,
                    col
                )
            }
            InputMode::DiffSearch => format!(
                "{} | mode={} focus={} theme={} /{}",
                self.status,
                mode,
                focus,
                self.theme_mode.label(),
                self.diff_search_buffer
            ),
            InputMode::ListSearch(pane) => {
                let query = match pane {
                    FocusPane::Commits => &self.commit_search_query,
                    FocusPane::Files => &self.file_search_query,
                    FocusPane::Diff => "",
                };
                format!(
                    "{} | mode={} focus={} theme={} /{}",
                    self.status,
                    mode,
                    focus,
                    self.theme_mode.label(),
                    query
                )
            }
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

    pub(super) fn render_comment_modal(&mut self, frame: &mut Frame<'_>, theme: &UiTheme) {
        let area = centered_rect(56, 48, frame.area());
        frame.render_widget(Clear, area);

        let title = match self.input_mode {
            InputMode::CommentCreate => " NEW COMMENT ",
            InputMode::CommentEdit(_) => " EDIT COMMENT ",
            InputMode::Normal | InputMode::DiffSearch | InputMode::ListSearch(_) => " COMMENT ",
        };
        let shell = Block::default()
            .title(Span::styled(
                title,
                Style::default()
                    .fg(theme.panel_title_fg)
                    .bg(theme.panel_title_bg)
                    .add_modifier(Modifier::BOLD),
            ))
            .title_alignment(ratatui::layout::Alignment::Center)
            .borders(Borders::ALL)
            .border_type(BorderType::Double)
            .style(Style::default().bg(theme.modal_bg))
            .border_style(Style::default().fg(theme.focus_border));
        frame.render_widget(shell.clone(), area);
        let inner = shell.inner(area);

        let sections = ratatui::layout::Layout::default()
            .direction(ratatui::layout::Direction::Vertical)
            .constraints([
                ratatui::layout::Constraint::Length(2),
                ratatui::layout::Constraint::Length(6),
                ratatui::layout::Constraint::Min(5),
                ratatui::layout::Constraint::Length(1),
            ])
            .split(inner);

        let mode_badge = match self.input_mode {
            InputMode::CommentCreate => "create",
            InputMode::CommentEdit(_) => "edit",
            InputMode::Normal | InputMode::DiffSearch | InputMode::ListSearch(_) => "idle",
        };
        let status_style = if self.status.contains("Failed")
            || self.status.contains("failed")
            || self.status.contains("empty")
            || self.status.contains("No ")
        {
            Style::default().fg(theme.issue)
        } else {
            Style::default().fg(theme.muted)
        };
        let header = Paragraph::new(vec![
            Line::from(vec![
                Span::styled("mode:", Style::default().fg(theme.dimmed)),
                Span::styled(
                    format!(" {mode_badge} "),
                    Style::default()
                        .fg(theme.panel_title_fg)
                        .bg(theme.panel_title_bg)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::raw("  "),
                Span::styled(
                    format!("{} chars", self.comment_buffer.chars().count()),
                    Style::default().fg(theme.dimmed),
                ),
            ]),
            Line::from(vec![
                Span::styled("status: ", Style::default().fg(theme.dimmed)),
                Span::styled(self.status.clone(), status_style),
            ]),
        ])
        .style(Style::default().bg(theme.modal_bg));
        frame.render_widget(header, sections[0]);

        let context_block = Block::default()
            .title(" Context ")
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .style(Style::default().bg(theme.modal_bg))
            .border_style(Style::default().fg(theme.border));
        let context_inner = context_block.inner(sections[1]);
        let context_lines =
            self.comment_context_preview_lines(context_inner.height as usize, theme);
        frame.render_widget(context_block, sections[1]);
        frame.render_widget(
            Paragraph::new(context_lines).style(Style::default().fg(theme.text)),
            context_inner,
        );

        let editor_block = Block::default()
            .title(" Comment ")
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .style(Style::default().bg(theme.modal_editor_bg))
            .border_style(Style::default().fg(theme.border));
        let editor_inner = editor_block.inner(sections[2]);
        let modal_view = comment_modal_lines(
            &self.comment_buffer,
            self.comment_cursor,
            self.comment_selection,
            editor_inner.height.saturating_sub(1) as usize,
            theme,
        );
        let CommentModalView {
            lines,
            line_ranges,
            view_start,
            text_offset,
        } = modal_view;
        self.comment_editor_rect = Some(editor_inner);
        self.comment_editor_line_ranges = line_ranges;
        self.comment_editor_view_start = view_start;
        self.comment_editor_text_offset = text_offset;
        frame.render_widget(editor_block, sections[2]);
        frame.render_widget(
            Paragraph::new(lines).style(Style::default().fg(theme.text)),
            editor_inner,
        );

        let footer = Paragraph::new(Line::from(vec![
            key_chip("Enter", theme),
            Span::styled(" save  ", Style::default().fg(theme.muted)),
            key_chip("Esc", theme),
            Span::styled(" cancel  ", Style::default().fg(theme.muted)),
            key_chip("Alt+Enter", theme),
            Span::styled(" newline  ", Style::default().fg(theme.muted)),
            key_chip("Mouse", theme),
            Span::styled(" cursor/select", Style::default().fg(theme.muted)),
        ]))
        .style(Style::default().bg(theme.modal_bg));
        frame.render_widget(footer, sections[3]);
    }

    fn comment_context_preview_lines(
        &self,
        max_rows: usize,
        theme: &UiTheme,
    ) -> Vec<Line<'static>> {
        let rows = max_rows.max(1);
        let mut lines = Vec::<Line<'static>>::new();
        let mut has_primary_context = false;

        match self.input_mode {
            InputMode::CommentEdit(id) => {
                if let Some(comment) = self.comments.comment_by_id(id) {
                    lines.push(Line::from(vec![
                        Span::styled("target ", Style::default().fg(theme.dimmed)),
                        Span::styled(
                            format!(
                                "{} {} ({} selected lines)",
                                comment.target.kind.as_str(),
                                comment_location_label(comment),
                                comment.target.selected_lines.len()
                            ),
                            Style::default().fg(theme.muted),
                        ),
                    ]));
                    has_primary_context = !comment.target.selected_lines.is_empty();
                    self.push_compact_selection_preview(
                        &mut lines,
                        &comment.target.selected_lines,
                        rows.saturating_sub(1),
                        theme,
                    );
                }
            }
            InputMode::CommentCreate => match self.comment_target_from_selection() {
                Ok(Some(target)) => {
                    let start =
                        format_anchor_lines(target.start.old_lineno, target.start.new_lineno);
                    let end = format_anchor_lines(target.end.old_lineno, target.end.new_lineno);
                    let span = if start == end {
                        start
                    } else {
                        format!("{start} -> {end}")
                    };
                    lines.push(Line::from(vec![
                        Span::styled("target ", Style::default().fg(theme.dimmed)),
                        Span::styled(
                            format!(
                                "{} {} ({span}; {} selected lines)",
                                target.kind.as_str(),
                                target.start.file_path,
                                target.selected_lines.len()
                            ),
                            Style::default().fg(theme.muted),
                        ),
                    ]));
                    has_primary_context = !target.selected_lines.is_empty();
                    self.push_compact_selection_preview(
                        &mut lines,
                        &target.selected_lines,
                        rows.saturating_sub(1),
                        theme,
                    );
                }
                Ok(None) => {
                    lines.push(Line::from(vec![
                        Span::styled("target ", Style::default().fg(theme.dimmed)),
                        Span::styled(
                            "no anchor at cursor; showing local diff snippet",
                            Style::default().fg(theme.muted),
                        ),
                    ]));
                }
                Err(err) => {
                    lines.push(Line::from(vec![
                        Span::styled("target ", Style::default().fg(theme.dimmed)),
                        Span::styled(
                            format!("failed to resolve target: {err:#}"),
                            Style::default().fg(theme.issue),
                        ),
                    ]));
                }
            },
            InputMode::Normal | InputMode::DiffSearch | InputMode::ListSearch(_) => {}
        }

        if !has_primary_context && !self.rendered_diff.is_empty() {
            let cursor = self.diff_position.cursor;
            let start = cursor.saturating_sub(1);
            let end = (cursor + 1).min(self.rendered_diff.len().saturating_sub(1));
            for idx in start..=end {
                if lines.len() >= rows {
                    break;
                }
                let focused = idx == cursor;
                lines.push(Line::from(vec![
                    Span::styled(
                        if focused { "> " } else { "  " },
                        if focused {
                            Style::default().fg(theme.accent)
                        } else {
                            Style::default().fg(theme.dimmed)
                        },
                    ),
                    Span::raw(truncate(&self.rendered_diff[idx].raw_text, 120)),
                ]));
            }
        }

        if lines.is_empty() {
            lines.push(Line::from(Span::styled(
                "No diff context available at cursor.",
                Style::default().fg(theme.muted),
            )));
        }
        lines.truncate(rows);
        lines
    }

    fn push_compact_selection_preview(
        &self,
        out: &mut Vec<Line<'static>>,
        snippets: &[String],
        max_rows: usize,
        theme: &UiTheme,
    ) {
        if max_rows == 0 || snippets.is_empty() {
            return;
        }
        if snippets.len() <= max_rows {
            for snippet in snippets.iter().take(max_rows) {
                out.push(Line::from(vec![
                    Span::styled("  ", Style::default().fg(theme.dimmed)),
                    Span::raw(truncate(snippet, 120)),
                ]));
            }
            return;
        }

        if max_rows == 1 {
            out.push(Line::from(vec![
                Span::styled("  ", Style::default().fg(theme.dimmed)),
                Span::styled(
                    format!("… {} lines selected …", snippets.len()),
                    Style::default().fg(theme.muted),
                ),
            ]));
            return;
        }

        let preview_rows = max_rows.saturating_sub(1);
        let head = preview_rows / 2;
        let tail = preview_rows.saturating_sub(head);
        for snippet in snippets.iter().take(head) {
            out.push(Line::from(vec![
                Span::styled("  ", Style::default().fg(theme.dimmed)),
                Span::raw(truncate(snippet, 120)),
            ]));
        }

        let omitted = snippets.len().saturating_sub(head + tail);
        out.push(Line::from(vec![
            Span::styled("  ", Style::default().fg(theme.dimmed)),
            Span::styled(
                format!("… {omitted} lines omitted …"),
                Style::default().fg(theme.muted),
            ),
        ]));

        for snippet in snippets.iter().skip(snippets.len().saturating_sub(tail)) {
            out.push(Line::from(vec![
                Span::styled("  ", Style::default().fg(theme.dimmed)),
                Span::raw(truncate(snippet, 120)),
            ]));
        }
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
                Span::raw(" diff search or live list filter"),
            ]),
            Line::from(vec![
                key_chip("Esc/Enter", theme),
                Span::raw(" search: clear / defocus"),
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
                Span::raw(" commits pane: cycle status filter"),
            ]),
            Line::from(vec![
                key_chip("e", theme),
                Span::raw(" diff pane: edit comment under cursor"),
            ]),
            Line::from(vec![
                key_chip("D", theme),
                Span::raw(" delete comment under cursor"),
            ]),
            Line::from(vec![
                key_chip("y", theme),
                Span::raw(" copy review-task file path"),
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
