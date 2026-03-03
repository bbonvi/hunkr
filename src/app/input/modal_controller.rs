use crate::app::*;

/// Shared contract for modal key/mouse handlers.
pub(in crate::app) trait ModalInputController {
    fn handle_key(&self, app: &mut App, key: KeyEvent);
    fn handle_mouse(&self, _app: &mut App, _mouse: crossterm::event::MouseEvent) {}
}

struct ShellModalController;
struct WorktreeModalController;

impl ModalInputController for ShellModalController {
    fn handle_key(&self, app: &mut App, key: KeyEvent) {
        app.handle_shell_command_input(key);
    }

    fn handle_mouse(&self, app: &mut App, mouse: crossterm::event::MouseEvent) {
        app.handle_shell_command_mouse(mouse);
    }
}

impl ModalInputController for WorktreeModalController {
    fn handle_key(&self, app: &mut App, key: KeyEvent) {
        app.handle_worktree_switch_input(key);
    }
}

static SHELL_MODAL: ShellModalController = ShellModalController;
static WORKTREE_MODAL: WorktreeModalController = WorktreeModalController;

/// Dispatches key input to the currently active modal controller.
pub(in crate::app) fn dispatch_modal_key(app: &mut App, key: KeyEvent) -> bool {
    match app.ui.preferences.input_mode {
        InputMode::ShellCommand => {
            SHELL_MODAL.handle_key(app, key);
            true
        }
        InputMode::WorktreeSwitch => {
            WORKTREE_MODAL.handle_key(app, key);
            true
        }
        InputMode::DiffSearch => {
            app.handle_diff_search_input(key);
            true
        }
        InputMode::ListSearch(pane) => {
            app.handle_list_search_input(pane, key);
            true
        }
        InputMode::Normal => false,
    }
}

/// Dispatches mouse input to the active modal controller when applicable.
pub(in crate::app) fn dispatch_modal_mouse(
    app: &mut App,
    mouse: crossterm::event::MouseEvent,
) -> bool {
    match app.ui.preferences.input_mode {
        InputMode::ShellCommand => {
            SHELL_MODAL.handle_mouse(app, mouse);
            true
        }
        InputMode::WorktreeSwitch => {
            WORKTREE_MODAL.handle_mouse(app, mouse);
            true
        }
        InputMode::DiffSearch | InputMode::ListSearch(_) | InputMode::Normal => false,
    }
}
