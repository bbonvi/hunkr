use crate::app::{
    App, KeyCode, KeyEvent, KeyModifiers, SingleLineEditOutcome, apply_single_line_edit_key,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ShellInputMode {
    Running,
    Finished,
    ReverseSearch,
    Editing,
}

trait ShellModeController {
    fn handle_key(&self, app: &mut App, key: KeyEvent);
}

struct RunningShellModeController;
struct FinishedShellModeController;
struct ReverseSearchShellModeController;
struct EditingShellModeController;

static RUNNING_CONTROLLER: RunningShellModeController = RunningShellModeController;
static FINISHED_CONTROLLER: FinishedShellModeController = FinishedShellModeController;
static REVERSE_SEARCH_CONTROLLER: ReverseSearchShellModeController = ReverseSearchShellModeController;
static EDITING_CONTROLLER: EditingShellModeController = EditingShellModeController;

impl ShellModeController for RunningShellModeController {
    fn handle_key(&self, app: &mut App, key: KeyEvent) {
        app.handle_running_shell_command_input(key);
    }
}

impl ShellModeController for FinishedShellModeController {
    fn handle_key(&self, app: &mut App, key: KeyEvent) {
        app.handle_finished_shell_command_input(key);
    }
}

impl ShellModeController for ReverseSearchShellModeController {
    fn handle_key(&self, app: &mut App, key: KeyEvent) {
        app.handle_shell_reverse_search_input(key);
    }
}

impl ShellModeController for EditingShellModeController {
    fn handle_key(&self, app: &mut App, key: KeyEvent) {
        if is_ctrl_char(key, 'r') {
            app.start_or_advance_shell_reverse_search();
            return;
        }
        if is_ctrl_char(key, 'p') {
            app.navigate_shell_history_previous();
            return;
        }
        if is_ctrl_char(key, 'n') {
            app.navigate_shell_history_next();
            return;
        }

        match key.code {
            KeyCode::Esc => app.close_shell_command_modal(),
            KeyCode::Enter => app.execute_shell_command(),
            KeyCode::Up => app.navigate_shell_history_previous(),
            KeyCode::Down => app.navigate_shell_history_next(),
            KeyCode::PageUp => {
                app.scroll_shell_output_lines(-(app.ui.shell_command.output_viewport as isize))
            }
            KeyCode::PageDown => {
                app.scroll_shell_output_lines(app.ui.shell_command.output_viewport as isize)
            }
            _ => {
                let edit = apply_single_line_edit_key(
                    &mut app.ui.shell_command.buffer,
                    &mut app.ui.shell_command.cursor,
                    key,
                );
                if !matches!(edit, SingleLineEditOutcome::NotHandled) {
                    app.ui.shell_command.history_nav = None;
                }
            }
        }
    }
}

fn is_ctrl_char(key: KeyEvent, c: char) -> bool {
    key.modifiers.contains(KeyModifiers::CONTROL)
        && matches!(
            key.code,
            KeyCode::Char(ch) if ch.eq_ignore_ascii_case(&c)
        )
}

fn shell_input_mode(app: &App) -> ShellInputMode {
    if app.ui.shell_command.running.is_some() {
        return ShellInputMode::Running;
    }
    if app.ui.shell_command.finished.is_some() {
        return ShellInputMode::Finished;
    }
    if app.ui.shell_command.reverse_search.is_some() {
        return ShellInputMode::ReverseSearch;
    }
    ShellInputMode::Editing
}

/// Routes shell modal key events into explicit shell-mode controllers.
pub(in crate::app) fn dispatch_shell_modal_key(app: &mut App, key: KeyEvent) {
    match shell_input_mode(app) {
        ShellInputMode::Running => RUNNING_CONTROLLER.handle_key(app, key),
        ShellInputMode::Finished => FINISHED_CONTROLLER.handle_key(app, key),
        ShellInputMode::ReverseSearch => REVERSE_SEARCH_CONTROLLER.handle_key(app, key),
        ShellInputMode::Editing => EDITING_CONTROLLER.handle_key(app, key),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn state_mode(running: bool, finished: bool, reverse_search: bool) -> ShellInputMode {
        if running {
            return ShellInputMode::Running;
        }
        if finished {
            return ShellInputMode::Finished;
        }
        if reverse_search {
            return ShellInputMode::ReverseSearch;
        }
        ShellInputMode::Editing
    }

    #[test]
    fn shell_mode_priority_prefers_running_then_finished_then_search() {
        assert_eq!(state_mode(true, true, true), ShellInputMode::Running);
        assert_eq!(state_mode(false, true, true), ShellInputMode::Finished);
        assert_eq!(state_mode(false, false, true), ShellInputMode::ReverseSearch);
        assert_eq!(state_mode(false, false, false), ShellInputMode::Editing);
    }

    #[test]
    fn ctrl_char_detection_accepts_case_variants() {
        assert!(is_ctrl_char(
            KeyEvent::new(KeyCode::Char('r'), KeyModifiers::CONTROL),
            'r'
        ));
        assert!(is_ctrl_char(
            KeyEvent::new(KeyCode::Char('R'), KeyModifiers::CONTROL),
            'r'
        ));
        assert!(!is_ctrl_char(
            KeyEvent::new(KeyCode::Char('r'), KeyModifiers::NONE),
            'r'
        ));
    }
}
