use super::super::*;

/// Scheduler input snapshot for poll timeout calculation.
#[derive(Debug, Clone, Copy)]
pub(in crate::app) struct PollTimeoutInputs {
    pub onboarding_active: bool,
    pub selection_rebuild_due: Option<Instant>,
    pub now: Instant,
    pub last_refresh_elapsed: Duration,
    pub last_relative_redraw_elapsed: Duration,
    pub shell_running: bool,
    pub shell_flash_timeout: Option<Duration>,
}

/// Scheduler input snapshot for tick-cycle planning.
#[derive(Debug, Clone, Copy)]
pub(in crate::app) struct TickPlanInputs {
    pub onboarding_active: bool,
    pub now: Instant,
    pub terminal_clear_elapsed: Duration,
    pub selection_rebuild_due: Option<Instant>,
    pub last_refresh_elapsed: Duration,
    pub last_relative_redraw_elapsed: Duration,
}

/// Tick tasks due for current runtime state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::app) enum TickTask {
    PollShellStream,
    PollShellFlash,
    RequestTerminalClear,
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
        inputs.last_refresh_elapsed,
        inputs.last_relative_redraw_elapsed,
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
    if inputs.terminal_clear_elapsed >= TERMINAL_CLEAR_EVERY {
        tasks.push(TickTask::RequestTerminalClear);
    }
    if inputs
        .selection_rebuild_due
        .is_some_and(|due| inputs.now >= due)
    {
        tasks.push(TickTask::FlushSelectionRebuild);
    }

    if inputs.last_refresh_elapsed >= AUTO_REFRESH_EVERY {
        tasks.push(TickTask::ReloadCommits);
    } else if inputs.last_relative_redraw_elapsed >= RELATIVE_TIME_REDRAW_EVERY {
        tasks.push(TickTask::RedrawRelativeTime);
    }
    tasks
}
