use crate::app::{App, KeyCode, KeyEvent, KeyModifiers};

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
            _ => app.apply_shell_command_editor_key(key),
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
    use std::{
        path::{Path, PathBuf},
        process::Command,
        sync::Arc,
        time::Instant,
    };

    use chrono::{DateTime, Utc};
    use tempfile::TempDir;

    use crate::{app::InputMode, config::AppConfig};

    struct TestClock;

    impl crate::app::AppClock for TestClock {
        fn now_utc(&self) -> DateTime<Utc> {
            Utc::now()
        }

        fn now_instant(&self) -> Instant {
            Instant::now()
        }
    }

    struct TestBootstrapPorts {
        repo_root: PathBuf,
    }

    impl crate::app::AppBootstrapPorts for TestBootstrapPorts {
        fn open_current_git(&self) -> anyhow::Result<crate::git_data::GitService> {
            crate::git_data::GitService::open_at(&self.repo_root)
        }

        fn load_config(&self) -> anyhow::Result<AppConfig> {
            Ok(AppConfig::default())
        }

        fn state_store_for_repo(&self, repo_root: &Path) -> crate::store::StateStore {
            crate::store::StateStore::for_project(repo_root)
        }

        fn open_comment_store(
            &self,
            store_root: &Path,
            branch: &str,
        ) -> anyhow::Result<crate::comments::CommentStore> {
            crate::comments::CommentStore::new(store_root, branch)
        }

        fn clock(&self) -> Arc<dyn crate::app::AppClock> {
            Arc::new(TestClock)
        }

        fn runtime_ports(&self) -> Arc<dyn crate::app::AppRuntimePorts> {
            Arc::new(crate::app::ports::SystemRuntimePorts)
        }
    }

    fn run_git(dir: &Path, args: &[&str]) {
        let output = Command::new("git")
            .args(args)
            .current_dir(dir)
            .output()
            .expect("spawn git");
        assert!(
            output.status.success(),
            "git {:?} failed: {}",
            args,
            String::from_utf8_lossy(&output.stderr)
        );
    }

    fn init_test_repo() -> TempDir {
        let tmp = tempfile::tempdir().expect("tempdir");
        let root = tmp.path();
        run_git(root, &["init", "-q"]);
        run_git(root, &["config", "user.name", "hunkr-test"]);
        run_git(root, &["config", "user.email", "hunkr-test@example.com"]);
        std::fs::write(root.join("README.md"), "init\n").expect("seed readme");
        run_git(root, &["add", "."]);
        run_git(root, &["commit", "-m", "init", "-q"]);
        tmp
    }

    fn bootstrap_app(repo_root: &Path) -> App {
        let store = crate::store::StateStore::for_project(repo_root);
        store
            .save(&crate::model::ReviewState::default())
            .expect("seed persisted state to bypass onboarding");

        let ports = TestBootstrapPorts {
            repo_root: repo_root.to_path_buf(),
        };
        App::bootstrap_with(&ports).expect("bootstrap app")
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

    #[test]
    fn dispatch_shell_modal_key_routes_editing_and_reverse_search_modes() {
        let repo = init_test_repo();
        let mut app = bootstrap_app(repo.path());
        app.ui.preferences.input_mode = InputMode::ShellCommand;
        app.ui.shell_command.buffer = "draft".to_owned();

        dispatch_shell_modal_key(
            &mut app,
            KeyEvent::new(KeyCode::Char('r'), KeyModifiers::CONTROL),
        );
        assert!(app.ui.shell_command.reverse_search.is_some());

        dispatch_shell_modal_key(&mut app, KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
        assert!(app.ui.shell_command.reverse_search.is_none());
        assert_eq!(app.ui.shell_command.buffer, "draft");
    }

    #[test]
    fn dispatch_shell_modal_key_routes_finished_and_running_modes() {
        let repo = init_test_repo();
        let mut app = bootstrap_app(repo.path());
        app.ui.preferences.input_mode = InputMode::ShellCommand;

        let exit_status = Command::new("git")
            .arg("--version")
            .status()
            .expect("spawn git");
        app.ui.shell_command.finished = Some(crate::app::ShellCommandResult { exit_status });
        dispatch_shell_modal_key(&mut app, KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
        assert_eq!(app.ui.preferences.input_mode, InputMode::Normal);

        app.ui.preferences.input_mode = InputMode::ShellCommand;
        app.ui.shell_command.buffer = "sleep 1".to_owned();
        app.execute_shell_command();
        assert!(app.ui.shell_command.running.is_some());
        dispatch_shell_modal_key(&mut app, KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
        assert!(app.ui.shell_command.running.is_some());
        assert!(app.runtime.status.contains("interrupted"));
        assert_eq!(app.ui.preferences.input_mode, InputMode::ShellCommand);
    }

    #[test]
    fn dispatch_shell_modal_key_prioritizes_running_over_finished_and_search() {
        let repo = init_test_repo();
        let mut app = bootstrap_app(repo.path());
        app.ui.preferences.input_mode = InputMode::ShellCommand;
        app.ui.shell_command.buffer = "sleep 1".to_owned();
        app.execute_shell_command();
        assert!(app.ui.shell_command.running.is_some());

        let exit_status = Command::new("git")
            .arg("--version")
            .status()
            .expect("spawn git");
        app.ui.shell_command.finished = Some(crate::app::ShellCommandResult { exit_status });
        app.ui.shell_command.reverse_search = Some(crate::app::ShellReverseSearchState {
            query: "fix".to_owned(),
            match_indexes: vec![0],
            match_cursor: 0,
            draft_buffer: "draft".to_owned(),
        });

        dispatch_shell_modal_key(&mut app, KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));

        assert_eq!(app.ui.preferences.input_mode, InputMode::ShellCommand);
        assert!(app.runtime.status.contains("interrupted"));
    }
}
