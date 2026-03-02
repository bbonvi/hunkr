use crate::app::*;

/// Runtime-level actions entering the application state machine.
pub(in crate::app) enum AppAction {
    Tick,
    KeyPress(KeyEvent),
    Mouse(crossterm::event::MouseEvent),
    Resize,
}

/// Side effects produced by the reducer and executed against `App`.
pub(in crate::app) enum AppEffect {
    RunTickCycle(Instant),
    HandleKey(KeyEvent),
    HandleMouse(crossterm::event::MouseEvent),
    EnsureCursorVisible,
    MarkNeedsRedraw,
}

/// Central action dispatcher for unidirectional runtime flow.
pub(in crate::app) fn dispatch(app: &mut App, action: AppAction) {
    let now = app.now_instant();
    let effects = reduce(app, action, now);
    for effect in effects {
        apply_effect(app, effect);
    }
}

fn reduce(_app: &App, action: AppAction, now: Instant) -> Vec<AppEffect> {
    match action {
        AppAction::Tick => vec![AppEffect::RunTickCycle(now)],
        AppAction::KeyPress(key) => vec![AppEffect::HandleKey(key), AppEffect::MarkNeedsRedraw],
        AppAction::Mouse(mouse) => vec![AppEffect::HandleMouse(mouse), AppEffect::MarkNeedsRedraw],
        AppAction::Resize => vec![AppEffect::EnsureCursorVisible, AppEffect::MarkNeedsRedraw],
    }
}

fn apply_effect(app: &mut App, effect: AppEffect) {
    match effect {
        AppEffect::RunTickCycle(now) => app.run_tick_cycle(now),
        AppEffect::HandleKey(key) => app.handle_key(key),
        AppEffect::HandleMouse(mouse) => app.handle_mouse(mouse),
        AppEffect::EnsureCursorVisible => app.ensure_cursor_visible(),
        AppEffect::MarkNeedsRedraw => app.runtime.needs_redraw = true,
    }
}
