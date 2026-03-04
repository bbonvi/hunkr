use crate::app::*;

/// Scheduler input snapshot for poll timeout calculation.
#[derive(Debug, Clone, Copy)]
pub(in crate::app) struct PollTimeoutInputs {
    pub onboarding_active: bool,
    pub selection_rebuild_due: Option<Instant>,
    pub now: Instant,
    pub last_refresh_elapsed: Duration,
    pub last_relative_redraw_elapsed: Duration,
    pub last_theme_reload_elapsed: Duration,
    pub auto_refresh_every: Duration,
    pub relative_time_redraw_every: Duration,
    pub theme_reload_poll_every: Duration,
    pub shell_running: bool,
    pub shell_flash_timeout: Option<Duration>,
}

/// Scheduler input snapshot for tick-cycle planning.
#[derive(Debug, Clone, Copy)]
pub(in crate::app) struct TickPlanInputs {
    pub onboarding_active: bool,
    pub now: Instant,
    pub terminal_clear_elapsed: Duration,
    pub terminal_clear_every: Duration,
    pub selection_rebuild_due: Option<Instant>,
    pub last_refresh_elapsed: Duration,
    pub last_relative_redraw_elapsed: Duration,
    pub last_theme_reload_elapsed: Duration,
    pub auto_refresh_every: Duration,
    pub relative_time_redraw_every: Duration,
    pub theme_reload_poll_every: Duration,
}

/// Tick tasks due for current runtime state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::app) enum TickTask {
    PollShellStream,
    PollShellFlash,
    RequestTerminalClear,
    ReloadTheme,
    FlushSelectionRebuild,
    ReloadCommits,
    RedrawRelativeTime,
}

/// Computes event-loop poll timeout from scheduler inputs.
pub(in crate::app) fn compute_poll_timeout(inputs: PollTimeoutInputs) -> Duration {
    if inputs.onboarding_active {
        return Duration::from_millis(250);
    }

    let selection_rebuild_in = inputs
        .selection_rebuild_due
        .map(|due| due.saturating_duration_since(inputs.now));
    let timeout = next_poll_timeout(
        inputs.auto_refresh_every,
        inputs.relative_time_redraw_every,
        inputs.theme_reload_poll_every,
        inputs.last_refresh_elapsed,
        inputs.last_relative_redraw_elapsed,
        inputs.last_theme_reload_elapsed,
        selection_rebuild_in,
    );
    let timeout = if inputs.shell_running {
        timeout.min(SHELL_STREAM_POLL_EVERY)
    } else {
        timeout
    };
    if let Some(flash_timeout) = inputs.shell_flash_timeout {
        timeout.min(flash_timeout)
    } else {
        timeout
    }
}

/// Plans due tasks for the current tick.
pub(in crate::app) fn plan_tick(inputs: TickPlanInputs) -> Vec<TickTask> {
    if inputs.onboarding_active {
        return Vec::new();
    }

    let mut tasks = vec![TickTask::PollShellStream, TickTask::PollShellFlash];
    if inputs.terminal_clear_elapsed >= inputs.terminal_clear_every {
        tasks.push(TickTask::RequestTerminalClear);
    }
    if inputs
        .selection_rebuild_due
        .is_some_and(|due| inputs.now >= due)
    {
        tasks.push(TickTask::FlushSelectionRebuild);
    }
    if inputs.last_theme_reload_elapsed >= inputs.theme_reload_poll_every {
        tasks.push(TickTask::ReloadTheme);
    }

    if inputs.last_refresh_elapsed >= inputs.auto_refresh_every {
        tasks.push(TickTask::ReloadCommits);
    } else if inputs.last_relative_redraw_elapsed >= inputs.relative_time_redraw_every {
        tasks.push(TickTask::RedrawRelativeTime);
    }
    tasks
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compute_poll_timeout_short_circuits_onboarding() {
        let auto_refresh_every = Duration::from_secs(4);
        let relative_time_redraw_every = Duration::from_secs(30);
        let theme_reload_poll_every = Duration::from_millis(250);
        let timeout = compute_poll_timeout(PollTimeoutInputs {
            onboarding_active: true,
            selection_rebuild_due: None,
            now: Instant::now(),
            last_refresh_elapsed: Duration::from_secs(10),
            last_relative_redraw_elapsed: Duration::from_secs(10),
            last_theme_reload_elapsed: Duration::from_secs(10),
            auto_refresh_every,
            relative_time_redraw_every,
            theme_reload_poll_every,
            shell_running: true,
            shell_flash_timeout: Some(Duration::from_millis(1)),
        });
        assert_eq!(timeout, Duration::from_millis(250));
    }

    #[test]
    fn compute_poll_timeout_clamps_to_shell_stream_polling() {
        let auto_refresh_every = Duration::from_secs(4);
        let relative_time_redraw_every = Duration::from_secs(30);
        let theme_reload_poll_every = Duration::from_millis(250);
        let timeout = compute_poll_timeout(PollTimeoutInputs {
            onboarding_active: false,
            selection_rebuild_due: None,
            now: Instant::now(),
            last_refresh_elapsed: Duration::from_secs(0),
            last_relative_redraw_elapsed: Duration::from_secs(0),
            last_theme_reload_elapsed: Duration::from_secs(0),
            auto_refresh_every,
            relative_time_redraw_every,
            theme_reload_poll_every,
            shell_running: true,
            shell_flash_timeout: None,
        });
        assert_eq!(timeout, SHELL_STREAM_POLL_EVERY);
    }

    #[test]
    fn compute_poll_timeout_prefers_flash_deadline_when_earlier() {
        let auto_refresh_every = Duration::from_secs(4);
        let relative_time_redraw_every = Duration::from_secs(30);
        let theme_reload_poll_every = Duration::from_millis(250);
        let timeout = compute_poll_timeout(PollTimeoutInputs {
            onboarding_active: false,
            selection_rebuild_due: None,
            now: Instant::now(),
            last_refresh_elapsed: Duration::from_secs(0),
            last_relative_redraw_elapsed: Duration::from_secs(0),
            last_theme_reload_elapsed: Duration::from_secs(0),
            auto_refresh_every,
            relative_time_redraw_every,
            theme_reload_poll_every,
            shell_running: true,
            shell_flash_timeout: Some(Duration::from_millis(5)),
        });
        assert_eq!(timeout, Duration::from_millis(5));
    }

    #[test]
    fn plan_tick_prefers_refresh_over_relative_redraw() {
        let auto_refresh_every = Duration::from_secs(4);
        let relative_time_redraw_every = Duration::from_secs(30);
        let theme_reload_poll_every = Duration::from_millis(250);
        let terminal_clear_every = Duration::from_secs(120);
        let tasks = plan_tick(TickPlanInputs {
            onboarding_active: false,
            now: Instant::now(),
            terminal_clear_elapsed: Duration::from_secs(0),
            terminal_clear_every,
            selection_rebuild_due: None,
            last_refresh_elapsed: auto_refresh_every,
            last_relative_redraw_elapsed: relative_time_redraw_every,
            last_theme_reload_elapsed: Duration::from_secs(0),
            auto_refresh_every,
            relative_time_redraw_every,
            theme_reload_poll_every,
        });
        assert!(tasks.contains(&TickTask::ReloadCommits));
        assert!(!tasks.contains(&TickTask::RedrawRelativeTime));
    }

    #[test]
    fn plan_tick_skips_all_tasks_when_onboarding_is_active() {
        let auto_refresh_every = Duration::from_secs(4);
        let relative_time_redraw_every = Duration::from_secs(30);
        let theme_reload_poll_every = Duration::from_millis(250);
        let terminal_clear_every = Duration::from_secs(120);
        let tasks = plan_tick(TickPlanInputs {
            onboarding_active: true,
            now: Instant::now(),
            terminal_clear_elapsed: terminal_clear_every,
            terminal_clear_every,
            selection_rebuild_due: Some(Instant::now()),
            last_refresh_elapsed: auto_refresh_every,
            last_relative_redraw_elapsed: relative_time_redraw_every,
            last_theme_reload_elapsed: theme_reload_poll_every,
            auto_refresh_every,
            relative_time_redraw_every,
            theme_reload_poll_every,
        });
        assert!(tasks.is_empty());
    }

    #[test]
    fn plan_tick_includes_selection_rebuild_when_due() {
        let now = Instant::now();
        let auto_refresh_every = Duration::from_secs(4);
        let relative_time_redraw_every = Duration::from_secs(30);
        let theme_reload_poll_every = Duration::from_millis(250);
        let terminal_clear_every = Duration::from_secs(120);
        let tasks = plan_tick(TickPlanInputs {
            onboarding_active: false,
            now,
            terminal_clear_elapsed: Duration::from_secs(0),
            terminal_clear_every,
            selection_rebuild_due: Some(now),
            last_refresh_elapsed: Duration::from_secs(0),
            last_relative_redraw_elapsed: Duration::from_secs(0),
            last_theme_reload_elapsed: Duration::from_secs(0),
            auto_refresh_every,
            relative_time_redraw_every,
            theme_reload_poll_every,
        });
        assert!(tasks.contains(&TickTask::FlushSelectionRebuild));
    }

    #[test]
    fn compute_poll_timeout_respects_selection_rebuild_deadline() {
        let now = Instant::now();
        let auto_refresh_every = Duration::from_secs(4);
        let relative_time_redraw_every = Duration::from_secs(30);
        let theme_reload_poll_every = Duration::from_millis(250);
        let timeout = compute_poll_timeout(PollTimeoutInputs {
            onboarding_active: false,
            selection_rebuild_due: Some(now + Duration::from_millis(2)),
            now,
            last_refresh_elapsed: Duration::from_secs(0),
            last_relative_redraw_elapsed: Duration::from_secs(0),
            last_theme_reload_elapsed: Duration::from_secs(0),
            auto_refresh_every,
            relative_time_redraw_every,
            theme_reload_poll_every,
            shell_running: false,
            shell_flash_timeout: None,
        });
        assert!(timeout <= Duration::from_millis(2));
    }

    #[test]
    fn plan_tick_includes_theme_reload_when_due() {
        let auto_refresh_every = Duration::from_secs(4);
        let relative_time_redraw_every = Duration::from_secs(30);
        let theme_reload_poll_every = Duration::from_millis(250);
        let terminal_clear_every = Duration::from_secs(120);
        let tasks = plan_tick(TickPlanInputs {
            onboarding_active: false,
            now: Instant::now(),
            terminal_clear_elapsed: Duration::from_secs(0),
            terminal_clear_every,
            selection_rebuild_due: None,
            last_refresh_elapsed: Duration::from_secs(0),
            last_relative_redraw_elapsed: Duration::from_secs(0),
            last_theme_reload_elapsed: theme_reload_poll_every,
            auto_refresh_every,
            relative_time_redraw_every,
            theme_reload_poll_every,
        });
        assert!(tasks.contains(&TickTask::ReloadTheme));
    }
}
