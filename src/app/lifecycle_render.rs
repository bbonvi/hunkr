use super::flow::{self, AppAction};
use super::runtime::tick_scheduler::{self, PollTimeoutInputs, TickPlanInputs, TickTask};
use super::*;
use crate::config::AppConfig;

/// Bootstrap-only dependencies loaded from disk/environment before UI startup.
struct BootstrapDeps {
    git: GitService,
    store: StateStore,
    instance_lock: Option<InstanceLock>,
    comments: CommentStore,
    clock: Arc<dyn AppClock>,
    runtime_ports: Arc<dyn AppRuntimePorts>,
    review_state: ReviewState,
}

impl App {
    pub fn bootstrap() -> anyhow::Result<Self> {
        let ports = SystemBootstrapPorts;
        Self::bootstrap_with(&ports)
    }

    pub fn bootstrap_with(ports: &dyn AppBootstrapPorts) -> anyhow::Result<Self> {
        let git = ports.open_current_git()?;
        let config = ports.load_config()?;
        let store = ports.state_store_for_repo(git.root());
        let instance_lock = store.try_acquire_instance_lock()?;
        let first_open = !store.has_state_file();
        let review_state = store.load()?;
        let comments = ports.open_comment_store(store.root_dir(), git.branch_name())?;
        let deps = BootstrapDeps {
            git,
            store,
            instance_lock,
            comments,
            clock: ports.clock(),
            runtime_ports: ports.runtime_ports(),
            review_state,
        };
        let mut app = Self::from_bootstrap_deps(deps, &config, first_open);

        if app.onboarding_active() {
            app.runtime.status.clear();
        } else {
            app.reload_commits(true)?;
            app.restore_persisted_ui_session()?;
            let selected = app.domain.commits.iter().filter(|row| row.selected).count();
            app.runtime.status = format!("{selected} commit(s) selected");
        }
        Ok(app)
    }

    fn from_bootstrap_deps(deps: BootstrapDeps, config: &AppConfig, first_open: bool) -> Self {
        let now = deps.clock.now_instant();
        let shell_history = deps
            .store
            .load_shell_history()
            .unwrap_or_default()
            .into_iter();
        let shell_history = shell_history
            .map(ShellCommandHistoryEntry::new)
            .collect::<VecDeque<_>>();
        Self {
            deps: AppDependencies {
                git: deps.git,
                store: deps.store,
                instance_lock: deps.instance_lock,
                comments: deps.comments,
                clock: deps.clock,
                runtime_ports: deps.runtime_ports,
            },
            domain: AppDomainState {
                review_state: deps.review_state,
                commits: Vec::new(),
                file_rows: Vec::new(),
                aggregate: AggregatedDiff::default(),
                deleted_file_content_visible: BTreeSet::new(),
                diff_position: DiffPosition::default(),
                rendered_diff: Arc::new(Vec::new()),
            },
            ui: AppUiState {
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
                    create_target_cache: None,
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
                draw_perf: DrawPerfState::default(),
            },
        }
    }

    pub(super) fn onboarding_active(&self) -> bool {
        self.runtime.onboarding_step.is_some()
    }

    fn complete_first_open_setup(&mut self) -> anyhow::Result<()> {
        let history = self.deps.git.load_first_parent_history(HISTORY_LIMIT)?;
        let reviewed_ids = first_open_reviewed_commit_ids(&history);
        if !reviewed_ids.is_empty() {
            self.deps.store.set_many_status(
                &mut self.domain.review_state,
                reviewed_ids,
                ReviewStatus::Reviewed,
                self.deps.git.branch_name(),
            );
        }
        self.deps.store.save(&self.domain.review_state)?;
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
        let now = self.deps.clock.now_instant();
        self.runtime.last_refresh = now;
        self.runtime.last_relative_time_redraw = now;

        let selected = self
            .domain
            .commits
            .iter()
            .filter(|row| row.selected)
            .count();
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
                if let Err(err) = std::fs::create_dir_all(self.deps.store.root_dir()) {
                    self.runtime.status = format!(
                        "failed to create {}: {err}",
                        self.deps.store.root_dir().display()
                    );
                    return;
                }
                if self.deps.instance_lock.is_none() {
                    match self.deps.store.acquire_instance_lock() {
                        Ok(lock) => self.deps.instance_lock = Some(lock),
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
                let gitignore_path = self.deps.git.root().join(".gitignore");
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
        self.runtime.last_terminal_clear = self.now_instant();
        self.runtime.needs_redraw = true;
    }

    pub fn mark_drawn(&mut self) {
        self.runtime.needs_redraw = false;
    }

    pub fn poll_timeout(&self) -> Duration {
        tick_scheduler::compute_poll_timeout(PollTimeoutInputs {
            onboarding_active: self.onboarding_active(),
            selection_rebuild_due: self.runtime.selection_rebuild_due,
            now: self.now_instant(),
            last_refresh_elapsed: self.runtime.last_refresh.elapsed(),
            last_relative_redraw_elapsed: self.runtime.last_relative_time_redraw.elapsed(),
            shell_running: self.ui.shell_command.running.is_some(),
            shell_flash_timeout: self.shell_output_flash_timeout(),
        })
    }

    pub fn draw(&mut self, frame: &mut Frame<'_>) {
        let theme = UiTheme::from_mode(self.ui.preferences.theme_mode);
        if self.onboarding_active() {
            self.render_onboarding(frame, &theme);
            return;
        }

        self.ensure_rendered_diff();
        self.ui.comment_editor.rect = None;
        self.ui.comment_editor.line_ranges.clear();
        self.ui.comment_editor.view_start = 0;
        self.ui.comment_editor.text_offset = 0;
        self.ui.shell_command.output_rect = None;
        self.ui.shell_command.output_viewport = 0;

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

        self.ui.diff_ui.pane_rects = PaneRects {
            commits: left_chunks[0],
            files: left_chunks[1],
            diff: main_chunks[1],
        };
        self.sync_diff_cursor_to_content_bounds();

        self.render_header(frame, root_chunks[0], &theme);
        self.render_commits(frame, self.ui.diff_ui.pane_rects.commits, &theme);
        self.render_files(frame, self.ui.diff_ui.pane_rects.files, &theme);
        self.render_diff(frame, self.ui.diff_ui.pane_rects.diff, &theme);
        self.render_footer(frame, root_chunks[2], &theme);
        if self.runtime.show_help {
            self.render_help_overlay(frame, &theme);
        }
        if matches!(
            self.ui.preferences.input_mode,
            InputMode::CommentCreate | InputMode::CommentEdit(_)
        ) {
            self.render_comment_modal(frame, &theme);
        } else if matches!(self.ui.preferences.input_mode, InputMode::ShellCommand) {
            self.render_shell_command_modal(frame, &theme);
        } else if matches!(self.ui.preferences.input_mode, InputMode::WorktreeSwitch) {
            self.render_worktree_switcher_modal(frame, &theme);
        }
    }

    pub fn tick(&mut self) {
        flow::dispatch(self, AppAction::Tick);
    }

    pub(in crate::app) fn run_tick_cycle(&mut self, now: Instant) {
        let tasks = tick_scheduler::plan_tick(TickPlanInputs {
            onboarding_active: self.onboarding_active(),
            now,
            terminal_clear_elapsed: self.runtime.last_terminal_clear.elapsed(),
            selection_rebuild_due: self.runtime.selection_rebuild_due,
            last_refresh_elapsed: self.runtime.last_refresh.elapsed(),
            last_relative_redraw_elapsed: self.runtime.last_relative_time_redraw.elapsed(),
        });
        for task in tasks {
            match task {
                TickTask::PollShellStream => self.poll_shell_command_stream(),
                TickTask::PollShellFlash => self.poll_shell_output_flash(),
                TickTask::RequestTerminalClear => self.request_terminal_clear(),
                TickTask::FlushSelectionRebuild => {
                    self.flush_pending_selection_rebuild();
                    self.runtime.needs_redraw = true;
                }
                TickTask::ReloadCommits => {
                    if let Err(err) = self.reload_commits(true) {
                        self.runtime.status = format!("refresh failed: {err:#}");
                    }
                    let refreshed_at = self.now_instant();
                    self.runtime.last_refresh = refreshed_at;
                    self.runtime.last_relative_time_redraw = refreshed_at;
                    self.runtime.needs_redraw = true;
                }
                TickTask::RedrawRelativeTime => {
                    self.runtime.last_relative_time_redraw = self.now_instant();
                    self.runtime.needs_redraw = true;
                }
            }
        }
    }

    pub fn handle_event(&mut self, event: Event) {
        match event {
            Event::Key(key) if key.kind == KeyEventKind::Press => {
                flow::dispatch(self, AppAction::KeyPress(key));
            }
            Event::Mouse(mouse) => flow::dispatch(self, AppAction::Mouse(mouse)),
            Event::Resize(_, _) => flow::dispatch(self, AppAction::Resize),
            _ => {}
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

        if !matches!(self.ui.preferences.input_mode, InputMode::Normal) {
            self.handle_non_normal_input(key);
            return;
        }

        if theme_toggle_conflicts_with_diff_pending_op(
            key,
            self.ui.preferences.focused,
            self.ui.diff_ui.pending_op,
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
                self.set_focus(focus_with_l(self.ui.preferences.focused))
            }
            KeyCode::Left if key.modifiers == KeyModifiers::NONE => {
                self.set_focus(focus_with_h(self.ui.preferences.focused))
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
                if self.ui.preferences.focused == FocusPane::Diff {
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
        let now = self.now_instant();
        self.runtime.last_refresh = now;
        self.runtime.last_relative_time_redraw = now;
    }

    pub(super) fn toggle_theme(&mut self) {
        self.ui.preferences.theme_mode = self.ui.preferences.theme_mode.toggle();
        self.ui.diff_cache.rendered_key = None;
        self.runtime.status = format!(
            "Theme switched to {}",
            self.ui.preferences.theme_mode.label()
        );
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

#[cfg(test)]
mod tests {
    use super::*;
    use anyhow::anyhow;
    use chrono::{DateTime, Utc};

    struct TestClock;

    impl AppClock for TestClock {
        fn now_utc(&self) -> DateTime<Utc> {
            Utc::now()
        }

        fn now_instant(&self) -> Instant {
            Instant::now()
        }
    }

    struct FailingGitBootstrapPorts;

    impl AppBootstrapPorts for FailingGitBootstrapPorts {
        fn open_current_git(&self) -> anyhow::Result<GitService> {
            Err(anyhow!("git open failed"))
        }

        fn load_config(&self) -> anyhow::Result<AppConfig> {
            panic!("load_config should not be called when git open fails");
        }

        fn state_store_for_repo(&self, _repo_root: &Path) -> StateStore {
            panic!("state_store_for_repo should not be called when git open fails");
        }

        fn open_comment_store(
            &self,
            _store_root: &Path,
            _branch: &str,
        ) -> anyhow::Result<CommentStore> {
            panic!("open_comment_store should not be called when git open fails");
        }

        fn clock(&self) -> Arc<dyn AppClock> {
            Arc::new(TestClock)
        }
    }

    #[test]
    fn bootstrap_with_propagates_git_open_errors() {
        match App::bootstrap_with(&FailingGitBootstrapPorts) {
            Ok(_) => panic!("bootstrap should fail"),
            Err(err) => assert!(format!("{err:#}").contains("git open failed")),
        }
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
