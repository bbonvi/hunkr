use std::{
    path::{Path, PathBuf},
    process::Command,
    sync::Arc,
    time::Duration,
    time::Instant,
};

use super::driver::AppDriver;
use crate::app::*;
use crate::config::AppConfig;
use crate::model::UNCOMMITTED_COMMIT_ID;
use chrono::{DateTime, Utc};
use ratatui::layout::Rect;
use ratatui::{Terminal, backend::TestBackend};
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

struct TestRuntimePorts;

impl AppRuntimePorts for TestRuntimePorts {
    fn open_git_at(&self, path: &Path) -> anyhow::Result<GitService> {
        GitService::open_at(path)
    }
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

    fn clock(&self) -> Arc<dyn AppClock> {
        Arc::new(TestClock)
    }

    fn runtime_ports(&self) -> Arc<dyn AppRuntimePorts> {
        Arc::new(TestRuntimePorts)
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

fn bootstrap_driver_with_state(repo_root: &Path, state: ReviewState) -> AppDriver {
    let store = StateStore::for_project(repo_root);
    store
        .save(&state)
        .expect("seed persisted state to bypass onboarding");
    let ports = TestBootstrapPorts {
        repo_root: repo_root.to_path_buf(),
    };
    let app = App::bootstrap_with(&ports).expect("bootstrap app");
    AppDriver::new(app)
}

fn bootstrap_driver(repo_root: &Path) -> AppDriver {
    bootstrap_driver_with_state(repo_root, ReviewState::default())
}

fn bootstrap_app(repo_root: &Path) -> App {
    let ports = TestBootstrapPorts {
        repo_root: repo_root.to_path_buf(),
    };
    App::bootstrap_with(&ports).expect("bootstrap app")
}

fn draw_app(app: &mut App, width: u16, height: u16) {
    let backend = TestBackend::new(width, height);
    let mut terminal = Terminal::new(backend).expect("create test terminal");
    terminal
        .draw(|frame| app.draw(frame))
        .expect("draw test frame");
}

fn git_stdout(dir: &Path, args: &[&str]) -> String {
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
    String::from_utf8(output.stdout)
        .expect("utf8 output")
        .trim()
        .to_owned()
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
fn driver_help_overlay_blocks_background_mouse_mutation() {
    let repo = init_test_repo();
    let mut app = bootstrap_driver(repo.path()).into_app();
    app.ui.diff_ui.pane_rects = PaneRects {
        commits: Rect {
            x: 0,
            y: 2,
            width: 36,
            height: 12,
        },
        files: Rect {
            x: 0,
            y: 14,
            width: 36,
            height: 8,
        },
        diff: Rect {
            x: 36,
            y: 2,
            width: 84,
            height: 20,
        },
    };
    let mut driver = AppDriver::new(app);
    let baseline = driver.snapshot();

    driver.send_key(press(KeyCode::Char('?'), KeyModifiers::NONE));
    driver.send_event(Event::Mouse(crossterm::event::MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Left),
        column: 50,
        row: 6,
        modifiers: KeyModifiers::NONE,
    }));

    let after = driver.snapshot();
    assert!(after.show_help);
    assert_eq!(after.focused_pane, baseline.focused_pane);
}

#[test]
fn driver_search_edit_contract_is_consistent_for_diff_list_and_worktree() {
    let repo = init_test_repo();
    let mut driver = bootstrap_driver(repo.path());

    driver.send_key(press(KeyCode::Char('3'), KeyModifiers::NONE));
    driver.send_key(press(KeyCode::Char('/'), KeyModifiers::NONE));
    assert_eq!(driver.snapshot().input_mode, "diff_search");
    driver.send_key(press(KeyCode::Enter, KeyModifiers::NONE));
    let diff_enter = driver.snapshot();
    assert_eq!(diff_enter.input_mode, "normal");
    assert_eq!(diff_enter.status, "Diff search cleared");

    driver.send_key(press(KeyCode::Char('3'), KeyModifiers::NONE));
    driver.send_key(press(KeyCode::Char('/'), KeyModifiers::NONE));
    driver.send_key(press(KeyCode::Char('x'), KeyModifiers::NONE));
    driver.send_key(press(KeyCode::Backspace, KeyModifiers::NONE));
    driver.send_key(press(KeyCode::Backspace, KeyModifiers::NONE));
    let diff_backspace = driver.snapshot();
    assert_eq!(diff_backspace.input_mode, "normal");
    assert_eq!(diff_backspace.status, "Diff search cleared");

    driver.send_key(press(KeyCode::Char('1'), KeyModifiers::NONE));
    driver.send_key(press(KeyCode::Char('/'), KeyModifiers::NONE));
    assert_eq!(driver.snapshot().input_mode, "list_search_commits");
    driver.send_key(press(KeyCode::Enter, KeyModifiers::NONE));
    let commits_enter = driver.snapshot();
    assert_eq!(commits_enter.input_mode, "normal");
    assert!(commits_enter.status.starts_with("Commit filter off"));

    driver.send_key(press(KeyCode::Char('/'), KeyModifiers::NONE));
    driver.send_key(press(KeyCode::Backspace, KeyModifiers::NONE));
    let commits_backspace = driver.snapshot();
    assert_eq!(commits_backspace.input_mode, "normal");
    assert_eq!(commits_backspace.status, "Commit filter cleared");

    driver.send_key(press(KeyCode::Char('w'), KeyModifiers::CONTROL));
    assert_eq!(driver.snapshot().input_mode, "worktree_switch");
    driver.send_key(press(KeyCode::Char('/'), KeyModifiers::NONE));
    driver.send_key(press(KeyCode::Enter, KeyModifiers::NONE));
    let worktree_enter = driver.snapshot();
    assert_eq!(worktree_enter.input_mode, "worktree_switch");
    assert_eq!(worktree_enter.status, "Worktree filter off");

    driver.send_key(press(KeyCode::Char('/'), KeyModifiers::NONE));
    driver.send_key(press(KeyCode::Backspace, KeyModifiers::NONE));
    let worktree_backspace = driver.snapshot();
    assert_eq!(worktree_backspace.input_mode, "worktree_switch");
    assert_eq!(worktree_backspace.status, "Worktree filter cleared");
}

#[test]
fn driver_wheel_focus_change_updates_keyboard_target() {
    let repo = init_test_repo();
    let mut app = bootstrap_driver(repo.path()).into_app();
    app.ui.diff_ui.pane_rects = PaneRects {
        commits: Rect {
            x: 0,
            y: 2,
            width: 32,
            height: 10,
        },
        files: Rect {
            x: 0,
            y: 12,
            width: 32,
            height: 8,
        },
        diff: Rect {
            x: 32,
            y: 2,
            width: 88,
            height: 18,
        },
    };
    let mut driver = AppDriver::new(app);

    driver.send_event(Event::Mouse(crossterm::event::MouseEvent {
        kind: MouseEventKind::ScrollDown,
        column: 50,
        row: 5,
        modifiers: KeyModifiers::NONE,
    }));
    assert_eq!(driver.snapshot().focused_pane, "diff");
    driver.send_key(press(KeyCode::Char('/'), KeyModifiers::NONE));
    assert_eq!(driver.snapshot().input_mode, "diff_search");

    driver.send_key(press(KeyCode::Esc, KeyModifiers::NONE));
    driver.send_event(Event::Mouse(crossterm::event::MouseEvent {
        kind: MouseEventKind::ScrollDown,
        column: 10,
        row: 14,
        modifiers: KeyModifiers::NONE,
    }));
    assert_eq!(driver.snapshot().focused_pane, "files");
    driver.send_key(press(KeyCode::Char('/'), KeyModifiers::NONE));
    assert_eq!(driver.snapshot().input_mode, "list_search_files");
}

#[test]
fn startup_without_persisted_selection_auto_selects_one_starter_row() {
    let repo = init_test_repo();
    let driver = bootstrap_driver(repo.path());
    let snapshot = driver.snapshot();

    assert_eq!(snapshot.selected_commit_ids.len(), 1);
    assert_ne!(snapshot.selected_commit_ids[0], UNCOMMITTED_COMMIT_ID);
    assert!(snapshot.status.starts_with("Starter selection:"));
}

#[test]
fn startup_prefers_uncommitted_starter_when_changes_exist() {
    let repo = init_test_repo();
    std::fs::write(repo.path().join("work.txt"), "pending\n").expect("write uncommitted file");
    let driver = bootstrap_driver(repo.path());
    let snapshot = driver.snapshot();

    assert_eq!(
        snapshot.selected_commit_ids,
        vec![UNCOMMITTED_COMMIT_ID.to_owned()]
    );
    assert_eq!(snapshot.status, "Starter selection: Uncommitted");
}

#[test]
fn startup_keeps_persisted_selection_without_reapplying_starter() {
    let repo = init_test_repo();
    let head = git_stdout(repo.path(), &["rev-parse", "HEAD"]);
    let mut state = ReviewState::default();
    state.ui_session.selected_commit_ids.insert(head.clone());

    let driver = bootstrap_driver_with_state(repo.path(), state);
    let snapshot = driver.snapshot();
    assert_eq!(snapshot.selected_commit_ids, vec![head]);
    assert_eq!(snapshot.status, "1 commit(s) selected");
}

#[test]
fn startup_with_stale_persisted_selection_falls_back_to_starter() {
    let repo = init_test_repo();
    let mut state = ReviewState::default();
    state
        .ui_session
        .selected_commit_ids
        .insert("stale-commit-id".to_owned());

    let driver = bootstrap_driver_with_state(repo.path(), state);
    let snapshot = driver.snapshot();
    assert_eq!(snapshot.selected_commit_ids.len(), 1);
    assert_ne!(snapshot.selected_commit_ids[0], "stale-commit-id");
    assert!(snapshot.status.starts_with("Starter selection:"));
}

#[test]
fn onboarding_completion_applies_starter_selection() {
    let repo = init_test_repo();
    let mut app = bootstrap_app(repo.path());
    assert!(app.onboarding_active());

    app.handle_event(Event::Key(press(KeyCode::Char('y'), KeyModifiers::NONE)));
    app.handle_event(Event::Key(press(KeyCode::Char('n'), KeyModifiers::NONE)));

    assert!(!app.onboarding_active());
    assert_eq!(
        app.domain.commits.iter().filter(|row| row.selected).count(),
        1
    );
    assert!(app.runtime.status.contains("Starter selection:"));
}

#[test]
fn footer_helper_click_executes_same_action_as_keybinding() {
    let repo = init_test_repo();
    let mut app = bootstrap_driver(repo.path()).into_app();
    draw_app(&mut app, 140, 40);
    let target = app
        .ui
        .helper_click_hitboxes
        .iter()
        .find(|hitbox| {
            matches!(
                hitbox.action,
                HelperClickAction::Key {
                    code: KeyCode::Char('?'),
                    modifiers: KeyModifiers::NONE,
                }
            )
        })
        .copied()
        .expect("help footer helper hitbox");
    let mut driver = AppDriver::new(app);

    driver.send_event(Event::Mouse(crossterm::event::MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Left),
        column: target.rect.x,
        row: target.rect.y,
        modifiers: KeyModifiers::NONE,
    }));

    assert!(driver.snapshot().show_help);
}

#[test]
fn help_overlay_blocks_footer_helper_click_actions() {
    let repo = init_test_repo();
    let mut app = bootstrap_driver(repo.path()).into_app();
    draw_app(&mut app, 140, 40);
    let footer_quit = app
        .ui
        .helper_click_hitboxes
        .iter()
        .find(|hitbox| {
            matches!(
                hitbox.action,
                HelperClickAction::Key {
                    code: KeyCode::Char('q'),
                    modifiers: KeyModifiers::NONE,
                }
            )
        })
        .copied()
        .expect("footer q helper hitbox");

    app.handle_event(Event::Key(press(KeyCode::Char('?'), KeyModifiers::NONE)));
    assert!(app.runtime.show_help);
    draw_app(&mut app, 140, 40);

    app.handle_event(Event::Mouse(crossterm::event::MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Left),
        column: footer_quit.rect.x,
        row: footer_quit.rect.y,
        modifiers: KeyModifiers::NONE,
    }));

    assert!(app.runtime.show_help);
    assert!(!app.runtime.should_quit);
}

#[test]
fn help_overlay_question_helper_click_closes_overlay() {
    let repo = init_test_repo();
    let mut app = bootstrap_driver(repo.path()).into_app();
    app.handle_event(Event::Key(press(KeyCode::Char('?'), KeyModifiers::NONE)));
    assert!(app.runtime.show_help);
    draw_app(&mut app, 140, 40);

    let help_close = app
        .ui
        .helper_click_hitboxes
        .iter()
        .find(|hitbox| {
            hitbox.rect.y < 36
                && matches!(
                    hitbox.action,
                    HelperClickAction::Key {
                        code: KeyCode::Char('?'),
                        modifiers: KeyModifiers::NONE,
                    }
                )
        })
        .copied()
        .expect("help close helper hitbox");

    app.handle_event(Event::Mouse(crossterm::event::MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Left),
        column: help_close.rect.x,
        row: help_close.rect.y,
        modifiers: KeyModifiers::NONE,
    }));

    assert!(!app.runtime.show_help);
}

#[test]
fn help_overlay_non_close_helper_click_keeps_overlay_open() {
    let repo = init_test_repo();
    let mut app = bootstrap_driver(repo.path()).into_app();
    app.handle_event(Event::Key(press(KeyCode::Char('?'), KeyModifiers::NONE)));
    assert!(app.runtime.show_help);
    let baseline_focus = app.ui.preferences.focused;
    draw_app(&mut app, 140, 40);

    let help_nav = app
        .ui
        .helper_click_hitboxes
        .iter()
        .find(|hitbox| {
            hitbox.rect.y < 36
                && matches!(
                    hitbox.action,
                    HelperClickAction::Key {
                        code: KeyCode::Char('1'),
                        modifiers: KeyModifiers::NONE,
                    }
                )
        })
        .copied()
        .expect("help nav helper hitbox");

    app.handle_event(Event::Mouse(crossterm::event::MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Left),
        column: help_nav.rect.x,
        row: help_nav.rect.y,
        modifiers: KeyModifiers::NONE,
    }));

    assert!(app.runtime.show_help);
    assert_eq!(app.ui.preferences.focused, baseline_focus);
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
