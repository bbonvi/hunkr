use crate::app::*;

/// Black-box snapshot of observable app state for flow-level tests.
#[derive(Debug, Clone)]
pub(in crate::app) struct AppSnapshot {
    pub status: String,
    pub focused_pane: &'static str,
    pub input_mode: &'static str,
    pub selected_commit_ids: Vec<String>,
    pub selected_file: Option<String>,
    pub show_help: bool,
    pub should_quit: bool,
}

/// Driver harness for event/tick based app testing.
pub(in crate::app) struct AppDriver {
    app: App,
}

impl AppDriver {
    pub(in crate::app) fn new(app: App) -> Self {
        Self { app }
    }

    pub(in crate::app) fn send_event(&mut self, event: Event) {
        self.app.handle_event(event);
    }

    pub(in crate::app) fn send_key(&mut self, key: KeyEvent) {
        self.send_event(Event::Key(key));
    }

    pub(in crate::app) fn tick(&mut self) {
        self.app.tick();
    }

    pub(in crate::app) fn snapshot(&self) -> AppSnapshot {
        AppSnapshot {
            status: self.app.runtime.status.clone(),
            focused_pane: match self.app.ui.preferences.focused {
                FocusPane::Files => "files",
                FocusPane::Commits => "commits",
                FocusPane::Diff => "diff",
            },
            input_mode: match self.app.ui.preferences.input_mode {
                InputMode::Normal => "normal",
                InputMode::ShellCommand => "shell_command",
                InputMode::WorktreeSwitch => "worktree_switch",
                InputMode::DiffSearch => "diff_search",
                InputMode::ListSearch(FocusPane::Files) => "list_search_files",
                InputMode::ListSearch(FocusPane::Commits) => "list_search_commits",
                InputMode::ListSearch(FocusPane::Diff) => "list_search_diff",
            },
            selected_commit_ids: self
                .app
                .domain
                .commits
                .iter()
                .filter(|row| row.selected)
                .map(|row| row.info.id.clone())
                .collect(),
            selected_file: self.app.ui.diff_cache.selected_file.clone(),
            show_help: self.app.runtime.show_help,
            should_quit: self.app.runtime.should_quit,
        }
    }

    pub(in crate::app) fn into_app(self) -> App {
        self.app
    }
}
