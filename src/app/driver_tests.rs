use std::{
    path::{Path, PathBuf},
    process::Command,
    sync::Arc,
    time::Duration,
    time::Instant,
};

use super::driver::AppDriver;
use super::*;
use crate::config::AppConfig;
use chrono::{DateTime, Utc};
use tempfile::TempDir;

fn press(code: KeyCode, modifiers: KeyModifiers) -> KeyEvent {
    KeyEvent {
        code,
        modifiers,
        kind: KeyEventKind::Press,
        state: crossterm::event::KeyEventState::NONE,
    }
}

struct TestClock;

impl AppClock for TestClock {
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

impl AppBootstrapPorts for TestBootstrapPorts {
    fn open_current_git(&self) -> anyhow::Result<GitService> {
        GitService::open_at(&self.repo_root)
    }

    fn load_config(&self) -> anyhow::Result<AppConfig> {
        Ok(AppConfig::default())
    }

    fn state_store_for_repo(&self, repo_root: &Path) -> StateStore {
        StateStore::for_project(repo_root)
    }

    fn open_comment_store(&self, store_root: &Path, branch: &str) -> anyhow::Result<CommentStore> {
        CommentStore::new(store_root, branch)
    }

    fn clock(&self) -> Arc<dyn AppClock> {
        Arc::new(TestClock)
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

fn bootstrap_driver(repo_root: &Path) -> AppDriver {
    let store = StateStore::for_project(repo_root);
    store
        .save(&ReviewState::default())
        .expect("seed persisted state to bypass onboarding");
    let ports = TestBootstrapPorts {
        repo_root: repo_root.to_path_buf(),
    };
    let app = App::bootstrap_with(&ports).expect("bootstrap app");
    AppDriver::new(app)
}

#[test]
fn driver_global_help_toggle_contract() {
    let repo = init_test_repo();
    let mut driver = bootstrap_driver(repo.path());

    driver.send_key(press(KeyCode::Char('?'), KeyModifiers::NONE));
    let opened = driver.snapshot();
    assert!(opened.show_help);
    let _ = (
        &opened.status,
        &opened.selected_commit_ids,
        &opened.selected_file,
    );

    driver.send_key(press(KeyCode::Char('?'), KeyModifiers::NONE));
    let closed = driver.snapshot();
    assert!(!closed.show_help);
    let _ = (
        &closed.status,
        &closed.selected_commit_ids,
        &closed.selected_file,
    );
}

#[test]
fn driver_quit_key_sets_quit_flag() {
    let repo = init_test_repo();
    let mut driver = bootstrap_driver(repo.path());
    driver.tick();

    driver.send_key(press(KeyCode::Char('q'), KeyModifiers::NONE));
    assert!(driver.snapshot().should_quit);
}

#[test]
fn driver_shell_modal_swallow_mouse_keeps_focus_context() {
    let repo = init_test_repo();
    let mut driver = bootstrap_driver(repo.path());

    let baseline = driver.snapshot();
    driver.send_key(press(KeyCode::Char('!'), KeyModifiers::NONE));
    let opened = driver.snapshot();
    assert_eq!(opened.input_mode, "shell_command");

    driver.send_event(Event::Mouse(crossterm::event::MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Left),
        column: 1,
        row: 1,
        modifiers: KeyModifiers::NONE,
    }));
    let after_click = driver.snapshot();
    assert_eq!(after_click.focused_pane, baseline.focused_pane);
}

#[test]
fn draw_perf_guardrail_counts_over_budget_frames() {
    let repo = init_test_repo();
    let driver = bootstrap_driver(repo.path());
    let mut app = driver.into_app();

    assert_eq!(app.draw_perf_over_budget_frames(), 0);
    app.record_draw_duration(Duration::from_millis(30));
    assert_eq!(app.draw_perf_over_budget_frames(), 1);
    app.record_draw_duration(Duration::from_millis(10));
    assert_eq!(app.draw_perf_over_budget_frames(), 1);
}
