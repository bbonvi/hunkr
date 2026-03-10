use super::flow::{self, AppAction};
use super::input::global_router;
use super::runtime::tick_scheduler::{self, PollTimeoutInputs, TickPlanInputs, TickTask};
use super::theme_palette::{THEME_FILE_NAME, ThemeReloadOutcome};
use crate::app::*;
use crate::config::{AppConfig, config_path};
use anyhow::Context;

/// Bootstrap-only dependencies loaded from disk/environment before UI startup.
struct BootstrapDeps {
    git: GitService,
    store: StateStore,
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
        let first_open = !store.has_state_file();
        let review_state = store.load()?;
        let deps = BootstrapDeps {
            git,
            store,
            clock: ports.clock(),
            runtime_ports: ports.runtime_ports(),
            review_state,
        };
        let mut app = Self::from_bootstrap_deps(deps, &config);
        app.reload_theme_from_disk(true);
        let startup_note = app.initialize_project_data(first_open)?;
        let has_persisted_selection = !app
            .domain
            .review_state
            .ui_session
            .selected_commit_ids
            .is_empty();

        app.reload_commits(true)?;
        app.restore_persisted_ui_session()?;
        let has_restored_selection = app.domain.commits.iter().any(|row| row.selected);
        if !has_persisted_selection || !has_restored_selection {
            app.apply_startup_starter_selection()?;
        }
        app.finalize_startup_status(startup_note);
        Ok(app)
    }

    fn from_bootstrap_deps(deps: BootstrapDeps, config: &AppConfig) -> Self {
        let now = deps.clock.now_instant();
        let theme_path = config_path().with_file_name(THEME_FILE_NAME);
        let theme_watch_dir = theme_path
            .parent()
            .map(Path::to_path_buf)
            .unwrap_or_else(|| std::path::PathBuf::from("."));
        let tuning = RuntimeTuning::from_config(config);
        let shell_history = deps
            .store
            .load_shell_history()
            .unwrap_or_default()
            .into_iter();
        let shell_history = shell_history
            .map(ShellCommandHistoryEntry::new)
            .collect::<VecDeque<_>>();
        let review_state_sync = ReviewStateSync::new(&deps.store);
        let review_state_watch_dir = review_state_sync.watch_dir().to_path_buf();
        Self {
            deps: AppDependencies {
                git: deps.git,
                store: deps.store,
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
                    visible_rows: Vec::new(),
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
                    rendered_key: None,
                    highlighter: DiffSyntaxHighlighter::new(),
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
                helper_click_hitboxes: Vec::new(),
            },
            theme: ThemeRuntimeState::new(theme_path),
            theme_reload_driver: PathWatchDriver::new(
                &theme_watch_dir,
                tuning.theme_reload_poll_every,
                now,
            ),
            review_state_sync,
            review_state_sync_driver: PathWatchDriver::new(
                &review_state_watch_dir,
                tuning.auto_refresh_every,
                now,
            ),
            runtime: RuntimeState {
                status: String::new(),
                selection_rebuild_due: None,
                show_help: false,
                last_refresh: now,
                last_relative_time_redraw: now,
                last_terminal_clear: now,
                terminal_clear_requested: false,
                needs_redraw: true,
                should_quit: false,
                draw_perf: DrawPerfState::default(),
            },
            tuning,
        }
    }

    fn initialize_project_data(&mut self, first_open: bool) -> anyhow::Result<Option<String>> {
        std::fs::create_dir_all(self.deps.store.root_dir()).with_context(|| {
            format!("failed to create {}", self.deps.store.root_dir().display())
        })?;
        self.review_state_sync_driver = PathWatchDriver::new(
            self.review_state_sync.watch_dir(),
            self.tuning.auto_refresh_every,
            self.now_instant(),
        );

        if first_open {
            self.complete_first_open_setup()?;
        }

        let startup_note =
            match append_ignore_file_entry(&self.deps.git.local_exclude_path(), "/.hunkr/") {
                Ok(IgnoreFileUpdate::Added | IgnoreFileUpdate::AlreadyPresent) => None,
                Err(err) => Some(format!("Failed to update local git exclude ({err:#})")),
            };

        Ok(startup_note)
    }

    fn complete_first_open_setup(&mut self) -> anyhow::Result<()> {
        let history = self
            .deps
            .git
            .load_first_parent_history(self.tuning.history_limit)?;
        let reviewed_ids = first_open_reviewed_commit_ids(&history);
        if !reviewed_ids.is_empty() {
            self.deps.store.set_many_status(
                &mut self.domain.review_state,
                reviewed_ids,
                ReviewStatus::Reviewed,
                self.deps.git.branch_name(),
            );
        }
        self.deps
            .store
            .save_statuses_merged(&mut self.domain.review_state)?;
        Ok(())
    }

    fn finalize_startup_status(&mut self, startup_note: Option<String>) {
        let selected = self
            .domain
            .commits
            .iter()
            .filter(|row| row.selected)
            .count();
        let ready = if self.runtime.status.starts_with("Starter selection:") {
            self.runtime.status.clone()
        } else {
            format!("{selected} commit(s) selected")
        };
        self.runtime.status = if let Some(note) = startup_note {
            format!("{note} {ready}")
        } else {
            ready
        };
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
        let now = self.now_instant();
        tick_scheduler::compute_poll_timeout(PollTimeoutInputs {
            selection_rebuild_due: self.runtime.selection_rebuild_due,
            now,
            last_refresh_elapsed: self.runtime.last_refresh.elapsed(),
            last_relative_redraw_elapsed: self.runtime.last_relative_time_redraw.elapsed(),
            theme_reload_fallback_poll_in: self.theme_reload_driver.fallback_poll_in(now),
            auto_refresh_every: self.tuning.auto_refresh_every,
            relative_time_redraw_every: self.tuning.relative_time_redraw_every,
            shell_running: self.ui.shell_command.running.is_some(),
            shell_flash_timeout: self.shell_output_flash_timeout(),
        })
    }

    pub fn draw(&mut self, frame: &mut Frame<'_>) {
        let theme = self.active_theme().clone();
        self.ensure_rendered_diff();
        self.ui.shell_command.output_rect = None;
        self.ui.shell_command.output_viewport = 0;
        self.ui.helper_click_hitboxes.clear();

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
        let render_snapshot = self.capture_render_snapshot();

        self.render_header(frame, root_chunks[0], &theme, &render_snapshot);
        self.render_commits(
            frame,
            self.ui.diff_ui.pane_rects.commits,
            &theme,
            &render_snapshot,
        );
        self.render_files(
            frame,
            self.ui.diff_ui.pane_rects.files,
            &theme,
            &render_snapshot,
        );
        self.render_diff(frame, self.ui.diff_ui.pane_rects.diff, &theme);
        self.render_footer(frame, root_chunks[2], &theme, &render_snapshot);
        if self.runtime.show_help {
            self.render_help_overlay(frame, &theme);
        }
        if matches!(self.ui.preferences.input_mode, InputMode::ShellCommand) {
            self.render_shell_command_modal(frame, &theme);
        } else if matches!(self.ui.preferences.input_mode, InputMode::WorktreeSwitch) {
            self.render_worktree_switcher_modal(frame, &theme);
        }
    }

    pub fn tick(&mut self) {
        flow::dispatch(self, AppAction::Tick);
    }

    pub(in crate::app) fn run_tick_cycle(&mut self, now: Instant) {
        let mut tasks = tick_scheduler::plan_tick(TickPlanInputs {
            now,
            terminal_clear_elapsed: self.runtime.last_terminal_clear.elapsed(),
            terminal_clear_every: self.tuning.terminal_clear_every,
            selection_rebuild_due: self.runtime.selection_rebuild_due,
            last_refresh_elapsed: self.runtime.last_refresh.elapsed(),
            last_relative_redraw_elapsed: self.runtime.last_relative_time_redraw.elapsed(),
            auto_refresh_every: self.tuning.auto_refresh_every,
            relative_time_redraw_every: self.tuning.relative_time_redraw_every,
        });
        if self.theme_reload_driver.poll_trigger(now).is_some() {
            tasks.push(TickTask::ReloadTheme);
        }
        let review_state_trigger = self.review_state_sync_driver.poll_trigger(now);
        if let Some(trigger) = review_state_trigger
            && let Err(err) = self.sync_review_statuses_from_disk_if_changed(matches!(
                trigger,
                PathWatchTrigger::Event
            ))
        {
            self.runtime.status = format!("review-state sync failed: {err:#}");
        }
        for task in tasks {
            match task {
                TickTask::PollShellStream => self.poll_shell_command_stream(),
                TickTask::PollShellFlash => self.poll_shell_output_flash(),
                TickTask::RequestTerminalClear => self.request_terminal_clear(),
                TickTask::ReloadTheme => self.reload_theme_from_disk(false),
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

        if global_router::dispatch_normal_mode_key(self, key) {
            return;
        }
        self.dispatch_focus_key(key);
    }

    pub(super) fn refresh_now(&mut self) {
        if let Err(err) = self.reload_commits(true) {
            self.runtime.status = format!("reload failed: {err:#}");
        }
        self.reload_theme_from_disk(true);
        let now = self.now_instant();
        self.runtime.last_refresh = now;
        self.runtime.last_relative_time_redraw = now;
    }

    pub(super) fn reload_theme_from_disk(&mut self, force: bool) {
        match self.theme.reload_if_changed(force) {
            Ok(ThemeReloadOutcome::Unchanged) => {}
            Ok(ThemeReloadOutcome::LoadedFromFile) => {
                self.invalidate_diff_cache();
                self.runtime.needs_redraw = true;
                self.runtime.status =
                    format!("Theme reloaded from {}", self.theme.path().display());
            }
            Ok(ThemeReloadOutcome::ResetToDefaults) => {
                self.invalidate_diff_cache();
                self.runtime.needs_redraw = true;
                self.runtime.status =
                    "Theme file removed; reverted to built-in defaults".to_owned();
            }
            Err(err) => {
                self.runtime.status = format!("theme reload failed: {err:#}");
            }
        }
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
    struct FailingRuntimePorts;

    impl AppRuntimePorts for FailingRuntimePorts {
        fn open_git_at(&self, _path: &Path) -> anyhow::Result<GitService> {
            panic!("runtime open_git_at should not be called when bootstrap fails");
        }
    }

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

        fn clock(&self) -> Arc<dyn AppClock> {
            Arc::new(TestClock)
        }

        fn runtime_ports(&self) -> Arc<dyn AppRuntimePorts> {
            Arc::new(FailingRuntimePorts)
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
