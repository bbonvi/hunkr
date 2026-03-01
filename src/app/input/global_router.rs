use super::super::lifecycle_render::{
    PaneCycleDirection, pane_focus_cycle_direction, theme_toggle_conflicts_with_diff_pending_op,
};
use super::super::*;

/// Handles normal-mode global keys and returns whether the key was consumed.
pub(in crate::app) fn dispatch_normal_mode_key(app: &mut App, key: KeyEvent) -> bool {
    if theme_toggle_conflicts_with_diff_pending_op(
        key,
        app.ui.preferences.focused,
        app.ui.diff_ui.pending_op,
    ) {
        app.dispatch_focus_key(key);
        return true;
    }

    if let Some(direction) = pane_focus_cycle_direction(key) {
        match direction {
            PaneCycleDirection::Next => app.focus_next(),
            PaneCycleDirection::Prev => app.focus_prev(),
        }
        return true;
    }

    match key.code {
        KeyCode::Char('q') => app.runtime.should_quit = true,
        KeyCode::Right if key.modifiers == KeyModifiers::NONE => {
            app.set_focus(focus_with_l(app.ui.preferences.focused))
        }
        KeyCode::Left if key.modifiers == KeyModifiers::NONE => {
            app.set_focus(focus_with_h(app.ui.preferences.focused))
        }
        KeyCode::Char('1') => app.set_focus(FocusPane::Commits),
        KeyCode::Char('2') => app.set_focus(FocusPane::Files),
        KeyCode::Char('3') => app.set_focus(FocusPane::Diff),
        KeyCode::Char('f') if key.modifiers == KeyModifiers::NONE => {
            app.set_focus(FocusPane::Files)
        }
        KeyCode::Char('c') if key.modifiers == KeyModifiers::NONE => {
            app.set_focus(FocusPane::Commits)
        }
        KeyCode::Char('d') if key.modifiers == KeyModifiers::NONE => app.set_focus(FocusPane::Diff),
        KeyCode::Char('!')
            if key.modifiers == KeyModifiers::NONE || key.modifiers == KeyModifiers::SHIFT =>
        {
            app.open_shell_command_modal();
        }
        KeyCode::Char('w') if key.modifiers == KeyModifiers::NONE => {
            if app.ui.preferences.focused == FocusPane::Diff {
                app.dispatch_focus_key(key);
            } else {
                app.open_worktree_switcher();
            }
        }
        KeyCode::Char('w') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            app.open_worktree_switcher();
        }
        KeyCode::Char('t') => app.toggle_theme(),
        KeyCode::F(5) => app.refresh_now(),
        KeyCode::Char('r') if key.modifiers == KeyModifiers::CONTROL => app.refresh_now(),
        KeyCode::Char('?') => {
            app.runtime.show_help = !app.runtime.show_help;
            app.runtime.status = if app.runtime.show_help {
                "Help overlay opened".to_owned()
            } else {
                "Help overlay closed".to_owned()
            };
        }
        _ => return false,
    }
    true
}
