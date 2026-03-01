use super::super::*;

pub(in crate::app) mod commits;
pub(in crate::app) mod diff;
pub(in crate::app) mod files;

/// Routes pane-scoped key input to the controller for the active focus pane.
pub(in crate::app) fn dispatch_pane_key(app: &mut App, key: KeyEvent) {
    match app.ui.preferences.focused {
        FocusPane::Files => app.handle_files_key(key),
        FocusPane::Commits => app.handle_commits_key(key),
        FocusPane::Diff => app.handle_diff_key(key),
    }
}
