use super::*;
use crate::config::AppConfig;

/// Bootstrap-only dependencies loaded from disk/environment before UI startup.
struct BootstrapDeps {
    git: GitService,
    store: StateStore,
    instance_lock: Option<InstanceLock>,
    comments: CommentStore,
    review_state: ReviewState,
}

impl App {
    pub fn bootstrap() -> anyhow::Result<Self> {
        let git = GitService::open_current()?;
        let config = AppConfig::load()?;
        let store = StateStore::for_project(git.root());
        let instance_lock = store.try_acquire_instance_lock()?;
        let first_open = !store.has_state_file();
        let review_state = store.load()?;
        let comments = CommentStore::new(store.root_dir(), git.branch_name())?;
        let deps = BootstrapDeps {
            git,
            store,
            instance_lock,
            comments,
            review_state,
        };
        let mut app = Self::from_bootstrap_deps(deps, &config, first_open);

        if app.onboarding_active() {
            app.runtime.status.clear();
        } else {
            app.reload_commits(true)?;
            app.ensure_rendered_diff();
            let selected = app.commits.iter().filter(|row| row.selected).count();
            app.runtime.status = format!("{selected} commit(s) selected");
        }
        Ok(app)
    }

    fn from_bootstrap_deps(deps: BootstrapDeps, config: &AppConfig, first_open: bool) -> Self {
        let now = Instant::now();
        let shell_history = deps
            .store
            .load_shell_history()
            .unwrap_or_default()
            .into_iter();
        let shell_history = shell_history
            .map(ShellCommandHistoryEntry::new)
            .collect::<VecDeque<_>>();
        Self {
            git: deps.git,
            store: deps.store,
            instance_lock: deps.instance_lock,
            comments: deps.comments,
            review_state: deps.review_state,
            commits: Vec::new(),
            file_rows: Vec::new(),
            aggregate: AggregatedDiff::default(),
            deleted_file_content_visible: BTreeSet::new(),
            diff_position: DiffPosition::default(),
            rendered_diff: Arc::new(Vec::new()),
            commit_ui: CommitUiState {
                list_state: ListState::default(),
                visual_anchor: None,
                selection_anchor: None,
                mouse_anchor: None,
                mouse_dragging: false,
                mouse_drag_mode: None,
                mouse_drag_baseline: None,
                status_filter: CommitStatusFilter::All,
            },
            file_ui: FileUiState {
                list_state: ListState::default(),
            },
            preferences: UiPreferences {
                focused: FocusPane::Commits,
                input_mode: InputMode::Normal,
                theme_mode: ThemeMode::from_startup_theme(config.startup_theme),
                diff_wheel_scroll_lines: config.diff_wheel_scroll_lines,
                list_wheel_coalesce: Duration::from_millis(config.list_wheel_coalesce_ms),
                nerd_fonts: config.nerd_fonts,
                nerd_font_theme: NerdFontTheme::default(),
            },
            diff_ui: DiffUiState {
                visual_selection: None,
                block_cursor_col: 0,
                block_cursor_goal: 0,
                mouse_anchor: None,
                last_list_wheel_event: None,
                pane_rects: PaneRects::default(),
                pending_op: None,
            },
            diff_cache: DiffCacheState {
                selected_file: None,
                positions: HashMap::new(),
                file_ranges: Vec::new(),
                file_range_by_path: HashMap::new(),
                pending_view_anchor: None,
                rendered_cache: HashMap::new(),
                rendered_key: None,
                highlighter: DiffSyntaxHighlighter::new(),
            },
            comment_editor: CommentEditorState {
                buffer: String::new(),
                cursor: 0,
                selection: None,
                mouse_anchor: None,
                rect: None,
                line_ranges: Vec::new(),
                view_start: 0,
                text_offset: 0,
            },
            shell_command: ShellCommandState {
                buffer: String::new(),
                cursor: 0,
                history: shell_history,
                history_nav: None,
                history_draft: String::new(),
                reverse_search: None,
                active_command: None,
                output_lines: Vec::new(),
                output_tail: String::new(),
                output_cursor: 0,
                output_visual_selection: None,
                output_mouse_anchor: None,
                output_flash_clear_due: None,
                output_scroll: 0,
                output_viewport: 0,
                output_follow: true,
                output_rect: None,
                running: None,
                finished: None,
            },
            worktree_switch: WorktreeSwitchState {
                entries: Vec::new(),
                list_state: ListState::default(),
                query: String::new(),
                search_active: false,
                viewport_rows: 0,
            },
            search: SearchState {
                diff_buffer: String::new(),
                diff_cursor: 0,
                diff_query: None,
                commit_query: String::new(),
                commit_cursor: 0,
                file_query: String::new(),
                file_cursor: 0,
            },
            runtime: RuntimeState {
                status: String::new(),
                selection_rebuild_due: None,
                show_help: false,
                onboarding_step: first_open.then_some(OnboardingStep::ConsentProjectDataDir),
                last_refresh: now,
                last_relative_time_redraw: now,
                last_terminal_clear: now,
                terminal_clear_requested: false,
                needs_redraw: true,
                should_quit: false,
            },
        }
    }

    pub(super) fn onboarding_active(&self) -> bool {
        self.runtime.onboarding_step.is_some()
    }

    fn complete_first_open_setup(&mut self) -> anyhow::Result<()> {
        let history = self.git.load_first_parent_history(HISTORY_LIMIT)?;
        let reviewed_ids = first_open_reviewed_commit_ids(&history);
        if !reviewed_ids.is_empty() {
            self.store.set_many_status(
                &mut self.review_state,
                reviewed_ids,
                ReviewStatus::Reviewed,
                self.git.branch_name(),
            );
        }
        self.store.save(&self.review_state)?;
        Ok(())
    }

    fn finish_onboarding(&mut self, onboarding_note: Option<String>) {
        if let Err(err) = self.complete_first_open_setup() {
            self.runtime.status = format!("setup failed: {err:#}");
            return;
        }
        if let Err(err) = self.reload_commits(true) {
            self.runtime.status = format!("reload failed after setup: {err:#}");
            return;
        }
        self.ensure_rendered_diff();
        self.runtime.onboarding_step = None;
        self.runtime.last_refresh = Instant::now();
        self.runtime.last_relative_time_redraw = Instant::now();

        let selected = self.commits.iter().filter(|row| row.selected).count();
        let ready = format!("{selected} commit(s) selected");
        self.runtime.status = if let Some(note) = onboarding_note {
            format!("{note} {ready}")
        } else {
            ready
        };
    }

    fn handle_onboarding_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Char('q') | KeyCode::Esc => {
                self.runtime.status = match self.runtime.onboarding_step {
                    Some(OnboardingStep::ConsentProjectDataDir) => {
                        "Setup canceled. Exiting without creating .hunkr".to_owned()
                    }
                    Some(OnboardingStep::GitignoreChoice) => {
                        "Setup canceled before completion. Reopen hunkr to continue setup."
                            .to_owned()
                    }
                    None => "Setup canceled".to_owned(),
                };
                self.runtime.should_quit = true;
            }
            _ => match self.runtime.onboarding_step {
                Some(OnboardingStep::ConsentProjectDataDir) => {
                    self.handle_project_data_dir_consent(key)
                }
                Some(OnboardingStep::GitignoreChoice) => self.handle_gitignore_choice(key),
                None => {}
            },
        }
    }

    fn handle_project_data_dir_consent(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Char('y') | KeyCode::Char('Y') | KeyCode::Enter => {
                if let Err(err) = std::fs::create_dir_all(self.store.root_dir()) {
                    self.runtime.status = format!(
                        "failed to create {}: {err}",
                        self.store.root_dir().display()
                    );
                    return;
                }
                if self.instance_lock.is_none() {
                    match self.store.acquire_instance_lock() {
                        Ok(lock) => self.instance_lock = Some(lock),
                        Err(err) => {
                            self.runtime.status = format!("setup failed: {err:#}");
                            self.runtime.should_quit = true;
                            return;
                        }
                    }
                }
                self.runtime.onboarding_step = Some(OnboardingStep::GitignoreChoice);
                self.runtime.status.clear();
            }
            KeyCode::Char('n') | KeyCode::Char('N') => {
                self.runtime.status = "Setup declined. Exiting without creating .hunkr".to_owned();
                self.runtime.should_quit = true;
            }
            _ => {}
        }
    }

    fn handle_gitignore_choice(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Char('y') | KeyCode::Char('Y') | KeyCode::Enter => {
                let gitignore_path = self.git.root().join(".gitignore");
                let note = match append_gitignore_entry(&gitignore_path, ".hunkr") {
                    Ok(GitignoreUpdate::Added) => "Added .hunkr to .gitignore.".to_owned(),
                    Ok(GitignoreUpdate::AlreadyPresent) => {
                        ".hunkr is already ignored in .gitignore.".to_owned()
                    }
                    Err(err) => {
                        self.runtime.status =
                            format!("failed to update {}: {err:#}", gitignore_path.display());
                        return;
                    }
                };
                self.finish_onboarding(Some(note));
            }
            KeyCode::Char('n') | KeyCode::Char('N') => self.finish_onboarding(Some(
                "Skipped .gitignore update. You can ignore .hunkr per project or globally."
                    .to_owned(),
            )),
            _ => {}
        }
    }

    pub fn should_quit(&self) -> bool {
        self.runtime.should_quit
    }

    pub fn needs_redraw(&self) -> bool {
        self.runtime.needs_redraw
    }

    pub fn take_terminal_clear_request(&mut self) -> bool {
        std::mem::take(&mut self.runtime.terminal_clear_requested)
    }

    pub(super) fn request_terminal_clear(&mut self) {
        self.runtime.terminal_clear_requested = true;
        self.runtime.last_terminal_clear = Instant::now();
        self.runtime.needs_redraw = true;
    }

    pub fn mark_drawn(&mut self) {
        self.runtime.needs_redraw = false;
    }

    pub fn poll_timeout(&self) -> Duration {
        if self.onboarding_active() {
            return Duration::from_millis(250);
        }

        let selection_rebuild_in = self
            .runtime
            .selection_rebuild_due
            .map(|due| due.saturating_duration_since(Instant::now()));
        let timeout = next_poll_timeout(
            self.runtime.last_refresh.elapsed(),
            self.runtime.last_relative_time_redraw.elapsed(),
            selection_rebuild_in,
        );
        let timeout = if self.shell_command.running.is_some() {
            timeout.min(SHELL_STREAM_POLL_EVERY)
        } else {
            timeout
        };
        if let Some(flash_timeout) = self.shell_output_flash_timeout() {
            timeout.min(flash_timeout)
        } else {
            timeout
        }
    }

    pub fn draw(&mut self, frame: &mut Frame<'_>) {
        let theme = UiTheme::from_mode(self.preferences.theme_mode);
        if self.onboarding_active() {
            self.render_onboarding(frame, &theme);
            return;
        }

        self.ensure_rendered_diff();
        self.comment_editor.rect = None;
        self.comment_editor.line_ranges.clear();
        self.comment_editor.view_start = 0;
        self.comment_editor.text_offset = 0;
        self.shell_command.output_rect = None;
        self.shell_command.output_viewport = 0;

        let root_chunks = ratatui::layout::Layout::default()
            .direction(ratatui::layout::Direction::Vertical)
            .constraints([
                ratatui::layout::Constraint::Length(2),
                ratatui::layout::Constraint::Min(1),
                ratatui::layout::Constraint::Length(4),
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

        self.diff_ui.pane_rects = PaneRects {
            commits: left_chunks[0],
            files: left_chunks[1],
            diff: main_chunks[1],
        };

        self.render_header(frame, root_chunks[0], &theme);
        self.render_commits(frame, self.diff_ui.pane_rects.commits, &theme);
        self.render_files(frame, self.diff_ui.pane_rects.files, &theme);
        self.render_diff(frame, self.diff_ui.pane_rects.diff, &theme);
        self.render_footer(frame, root_chunks[2], &theme);
        if self.runtime.show_help {
            self.render_help_overlay(frame, &theme);
        }
        if matches!(
            self.preferences.input_mode,
            InputMode::CommentCreate | InputMode::CommentEdit(_)
        ) {
            self.render_comment_modal(frame, &theme);
        } else if matches!(self.preferences.input_mode, InputMode::ShellCommand) {
            self.render_shell_command_modal(frame, &theme);
        } else if matches!(self.preferences.input_mode, InputMode::WorktreeSwitch) {
            self.render_worktree_switcher_modal(frame, &theme);
        }
    }

    pub fn tick(&mut self) {
        if self.onboarding_active() {
            return;
        }
        self.poll_shell_command_stream();
        self.poll_shell_output_flash();
        if self.runtime.last_terminal_clear.elapsed() >= TERMINAL_CLEAR_EVERY {
            self.request_terminal_clear();
        }

        let now = Instant::now();
        if self
            .runtime
            .selection_rebuild_due
            .is_some_and(|due| now >= due)
        {
            self.flush_pending_selection_rebuild();
            self.runtime.needs_redraw = true;
        }

        let mut refreshed = false;
        if self.runtime.last_refresh.elapsed() >= AUTO_REFRESH_EVERY {
            if let Err(err) = self.reload_commits(true) {
                self.runtime.status = format!("refresh failed: {err:#}");
            }
            self.runtime.last_refresh = Instant::now();
            refreshed = true;
            self.runtime.needs_redraw = true;
        }

        if refreshed {
            self.runtime.last_relative_time_redraw = Instant::now();
        } else if self.runtime.last_relative_time_redraw.elapsed() >= RELATIVE_TIME_REDRAW_EVERY {
            self.runtime.last_relative_time_redraw = Instant::now();
            self.runtime.needs_redraw = true;
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
            self.runtime.needs_redraw = true;
        }
    }

    pub(super) fn handle_key(&mut self, key: KeyEvent) {
        if self.onboarding_active() {
            self.handle_onboarding_key(key);
            return;
        }

        if key.modifiers.contains(KeyModifiers::CONTROL)
            && matches!(key.code, KeyCode::Char('l') | KeyCode::Char('L'))
        {
            self.request_terminal_clear();
            self.runtime.status = "Terminal view refreshed".to_owned();
            return;
        }

        if self.runtime.show_help {
            if help_overlay_close_key(key) {
                self.runtime.show_help = false;
                self.runtime.status = "Help overlay closed".to_owned();
            }
            return;
        }

        if !matches!(self.preferences.input_mode, InputMode::Normal) {
            self.handle_non_normal_input(key);
            return;
        }

        if theme_toggle_conflicts_with_diff_pending_op(
            key,
            self.preferences.focused,
            self.diff_ui.pending_op,
        ) {
            self.dispatch_focus_key(key);
            return;
        }

        if let Some(direction) = pane_focus_cycle_direction(key) {
            match direction {
                PaneCycleDirection::Next => self.focus_next(),
                PaneCycleDirection::Prev => self.focus_prev(),
            }
            return;
        }

        match key.code {
            KeyCode::Char('q') => self.runtime.should_quit = true,
            KeyCode::Right if key.modifiers == KeyModifiers::NONE => {
                self.set_focus(focus_with_l(self.preferences.focused))
            }
            KeyCode::Left if key.modifiers == KeyModifiers::NONE => {
                self.set_focus(focus_with_h(self.preferences.focused))
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
            KeyCode::Char('!')
                if key.modifiers == KeyModifiers::NONE || key.modifiers == KeyModifiers::SHIFT =>
            {
                self.open_shell_command_modal();
            }
            KeyCode::Char('w') if key.modifiers == KeyModifiers::NONE => {
                if self.preferences.focused == FocusPane::Diff {
                    self.dispatch_focus_key(key);
                } else {
                    self.open_worktree_switcher();
                }
            }
            KeyCode::Char('w') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.open_worktree_switcher();
            }
            KeyCode::Char('t') => self.toggle_theme(),
            KeyCode::F(5) => self.refresh_now(),
            KeyCode::Char('r') if key.modifiers == KeyModifiers::CONTROL => self.refresh_now(),
            KeyCode::Char('?') => {
                self.runtime.show_help = !self.runtime.show_help;
                self.runtime.status = if self.runtime.show_help {
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
            self.runtime.status = format!("reload failed: {err:#}");
        }
        let now = Instant::now();
        self.runtime.last_refresh = now;
        self.runtime.last_relative_time_redraw = now;
    }

    pub(super) fn toggle_theme(&mut self) {
        self.preferences.theme_mode = self.preferences.theme_mode.toggle();
        self.diff_cache.rendered_key = None;
        self.runtime.status = format!("Theme switched to {}", self.preferences.theme_mode.label());
    }
}

pub(super) fn help_overlay_close_key(key: KeyEvent) -> bool {
    key.modifiers == KeyModifiers::NONE
        && matches!(
            key.code,
            KeyCode::Esc | KeyCode::Char('?') | KeyCode::Char('q')
        )
}

/// Maps pane-cycle keyboard shortcuts while preserving Ctrl-modified bindings.
pub(super) fn pane_focus_cycle_direction(key: KeyEvent) -> Option<PaneCycleDirection> {
    match (key.code, key.modifiers) {
        (KeyCode::Tab, KeyModifiers::NONE) => Some(PaneCycleDirection::Next),
        (KeyCode::Tab, KeyModifiers::SHIFT) => Some(PaneCycleDirection::Prev),
        (KeyCode::BackTab, KeyModifiers::NONE | KeyModifiers::SHIFT) => {
            Some(PaneCycleDirection::Prev)
        }
        _ => None,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum PaneCycleDirection {
    Prev,
    Next,
}

pub(super) fn theme_toggle_conflicts_with_diff_pending_op(
    key: KeyEvent,
    focused: FocusPane,
    pending_op: Option<DiffPendingOp>,
) -> bool {
    key.modifiers == KeyModifiers::NONE
        && key.code == KeyCode::Char('t')
        && focused == FocusPane::Diff
        && matches!(pending_op, Some(DiffPendingOp::Z))
}
