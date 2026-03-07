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
    run_git(root, &["init", "-q", "-b", "main"]);
    run_git(root, &["config", "user.name", "hunkr-test"]);
    run_git(root, &["config", "user.email", "hunkr-test@example.com"]);
    std::fs::write(root.join(".gitignore"), "!/.hunkr/\n!/.hunkr/**\n").expect("seed gitignore");
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

fn bootstrap_two_file_boundary_fixture() -> (TempDir, App) {
    let repo = init_test_repo();
    let alpha = repo.path().join("alpha.txt");
    let beta = repo.path().join("beta.txt");
    std::fs::write(&alpha, "one\ntwo\nthree\n").expect("write alpha baseline");
    std::fs::write(&beta, "red\ngreen\nblue\n").expect("write beta baseline");
    run_git(repo.path(), &["add", "alpha.txt", "beta.txt"]);
    run_git(repo.path(), &["commit", "-m", "seed two files", "-q"]);

    std::fs::write(&alpha, "one\ntwo changed\nthree\n").expect("write alpha update");
    std::fs::write(&beta, "red\ngreen changed\nblue\n").expect("write beta update");
    run_git(repo.path(), &["add", "alpha.txt", "beta.txt"]);
    run_git(repo.path(), &["commit", "-m", "touch both files", "-q"]);

    let mut app = bootstrap_driver(repo.path()).into_app();
    draw_app(&mut app, 120, 22);
    (repo, app)
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
fn commit_space_extends_from_last_space_anchor_and_falls_back_to_single() {
    let repo = init_test_repo();
    let readme = repo.path().join("README.md");
    std::fs::write(&readme, "init\none\n").expect("update readme");
    run_git(repo.path(), &["add", "README.md"]);
    run_git(repo.path(), &["commit", "-m", "one", "-q"]);
    std::fs::write(&readme, "init\none\ntwo\n").expect("update readme");
    run_git(repo.path(), &["add", "README.md"]);
    run_git(repo.path(), &["commit", "-m", "two", "-q"]);
    std::fs::write(&readme, "init\none\ntwo\nthree\n").expect("update readme");
    run_git(repo.path(), &["add", "README.md"]);
    run_git(repo.path(), &["commit", "-m", "three", "-q"]);

    let mut driver = bootstrap_driver(repo.path());
    driver.send_key(press(KeyCode::Down, KeyModifiers::NONE));
    driver.send_key(press(KeyCode::Down, KeyModifiers::NONE));
    driver.send_key(press(KeyCode::Enter, KeyModifiers::NONE));
    assert_eq!(driver.snapshot().selected_commit_ids.len(), 1);

    driver.send_key(press(KeyCode::Down, KeyModifiers::NONE));
    driver.send_key(press(KeyCode::Char(' '), KeyModifiers::NONE));
    assert_eq!(driver.snapshot().selected_commit_ids.len(), 2);

    driver.send_key(press(KeyCode::Up, KeyModifiers::NONE));
    driver.send_key(press(KeyCode::Up, KeyModifiers::NONE));
    driver.send_key(press(KeyCode::Up, KeyModifiers::NONE));
    driver.send_key(press(KeyCode::Char(' '), KeyModifiers::NONE));
    assert_eq!(driver.snapshot().selected_commit_ids.len(), 4);

    driver.send_key(press(KeyCode::Char('x'), KeyModifiers::NONE));
    assert!(driver.snapshot().selected_commit_ids.is_empty());

    driver.send_key(press(KeyCode::Char(' '), KeyModifiers::NONE));
    assert_eq!(driver.snapshot().selected_commit_ids.len(), 1);
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
fn draw_virtualized_diff_rows_keep_sticky_and_wrapped_mapping() {
    let repo = init_test_repo();
    let readme = repo.path().join("README.md");
    let long_line = "0123456789".repeat(40);
    std::fs::write(&readme, format!("init\n{long_line}\n")).expect("write long readme");
    run_git(repo.path(), &["add", "README.md"]);
    run_git(repo.path(), &["commit", "-m", "long diff line", "-q"]);

    let mut app = bootstrap_driver(repo.path()).into_app();
    draw_app(&mut app, 70, 18);

    let code_idx = app
        .domain
        .rendered_diff
        .iter()
        .position(|line| line.raw_text.starts_with('+'))
        .expect("code diff row");
    app.domain.diff_position = DiffPosition {
        scroll: code_idx,
        cursor: code_idx,
    };
    app.ui.search.diff_query = Some("0123".to_owned());
    app.ui.diff_ui.visual_selection = Some(DiffVisualSelection {
        anchor: code_idx,
        origin: DiffVisualOrigin::Keyboard,
    });

    draw_app(&mut app, 70, 18);

    let viewport_rows = app
        .ui
        .diff_ui
        .pane_rects
        .diff
        .height
        .saturating_sub(2)
        .max(1) as usize;
    let sticky_indexes = app.sticky_banner_indexes_for_scroll(code_idx, viewport_rows);
    let sticky_rows = sticky_indexes.len().min(viewport_rows.saturating_sub(1));
    assert!(
        sticky_indexes
            .iter()
            .any(|idx| is_hunk_header_line(&app.domain.rendered_diff[*idx])),
        "scrolling inside a hunk should include the active hunk header in sticky rows",
    );

    assert!(
        app.ui.diff_ui.visible_rows.len() <= viewport_rows,
        "diff viewport row map must stay within visible viewport",
    );
    assert!(
        app.ui.diff_ui.visible_rows.len() > sticky_rows,
        "fixture should leave room for body rows under sticky banners",
    );

    for (row, sticky_idx) in sticky_indexes.iter().take(sticky_rows).enumerate() {
        let mapped = app.ui.diff_ui.visible_rows[row];
        assert_eq!(mapped.line_index, *sticky_idx);
        assert_eq!(mapped.wrapped_row_offset, 0);
    }

    let body = &app.ui.diff_ui.visible_rows[sticky_rows..];
    assert!(
        body.windows(2).any(|pair| {
            pair[0].line_index == pair[1].line_index
                && pair[1].wrapped_row_offset == pair[0].wrapped_row_offset + 1
        }),
        "body rows should include wrapped offsets for at least one source diff line",
    );
}

#[test]
fn sticky_hunk_header_switch_keeps_hunk_sticky_row_stable() {
    let repo = init_test_repo();
    let file = repo.path().join("src.txt");
    let mut baseline = (1..=80)
        .map(|line| format!("line {line}"))
        .collect::<Vec<_>>();
    std::fs::write(&file, baseline.join("\n") + "\n").expect("write baseline file");
    run_git(repo.path(), &["add", "src.txt"]);
    run_git(repo.path(), &["commit", "-m", "seed src file", "-q"]);

    baseline[9] = "line 10 changed".to_owned();
    baseline[39] = "line 40 changed".to_owned();
    std::fs::write(&file, baseline.join("\n") + "\n").expect("write updated file");
    run_git(repo.path(), &["add", "src.txt"]);
    run_git(repo.path(), &["commit", "-m", "two separate hunks", "-q"]);

    let mut app = bootstrap_driver(repo.path()).into_app();
    draw_app(&mut app, 120, 22);

    let hunk_headers = app
        .domain
        .rendered_diff
        .iter()
        .enumerate()
        .filter_map(|(idx, line)| is_hunk_header_line(line).then_some(idx))
        .collect::<Vec<_>>();
    assert!(
        hunk_headers.len() >= 2,
        "fixture should produce at least two separate hunks",
    );
    let first_hunk = hunk_headers[0];
    let second_hunk = hunk_headers[1];

    let viewport_rows = app
        .ui
        .diff_ui
        .pane_rects
        .diff
        .height
        .saturating_sub(2)
        .max(1) as usize;
    let sticky_at_second = app.sticky_banner_indexes_for_scroll(second_hunk, viewport_rows);
    let sticky_after_second = app.sticky_banner_indexes_for_scroll(second_hunk + 1, viewport_rows);

    let sticky_hunk_at_second = sticky_at_second
        .iter()
        .copied()
        .rev()
        .find(|idx| is_hunk_header_line(&app.domain.rendered_diff[*idx]));
    let sticky_hunk_after_second = sticky_after_second
        .iter()
        .copied()
        .rev()
        .find(|idx| is_hunk_header_line(&app.domain.rendered_diff[*idx]));
    assert_eq!(
        sticky_hunk_at_second,
        Some(first_hunk),
        "previous hunk should stay sticky while the next hunk header is the top body row",
    );
    assert_eq!(
        sticky_hunk_after_second,
        Some(second_hunk),
        "latest sticky hunk should switch once the new hunk body scrolls under the top edge",
    );
}

#[test]
fn sticky_file_header_switch_keeps_file_sticky_row_stable() {
    let (_repo, app) = bootstrap_two_file_boundary_fixture();

    let file_headers = app
        .domain
        .rendered_diff
        .iter()
        .enumerate()
        .filter_map(|(idx, line)| line.raw_text.starts_with("==== file ").then_some(idx))
        .collect::<Vec<_>>();
    assert!(
        file_headers.len() >= 2,
        "fixture should produce at least two file headers",
    );
    let first_file = file_headers[0];
    let second_file = file_headers[1];

    let viewport_rows = app
        .ui
        .diff_ui
        .pane_rects
        .diff
        .height
        .saturating_sub(2)
        .max(1) as usize;
    let sticky_at_second = app.sticky_banner_indexes_for_scroll(second_file, viewport_rows);
    let sticky_after_second = app.sticky_banner_indexes_for_scroll(second_file + 1, viewport_rows);

    let sticky_file_at_second = sticky_at_second.iter().copied().find(|idx| {
        app.domain.rendered_diff[*idx]
            .raw_text
            .starts_with("==== file ")
    });
    let sticky_file_after_second = sticky_after_second.iter().copied().find(|idx| {
        app.domain.rendered_diff[*idx]
            .raw_text
            .starts_with("==== file ")
    });
    assert_eq!(
        sticky_file_at_second,
        Some(first_file),
        "previous file banner should stay sticky while the next file banner is the top body row",
    );
    assert_eq!(
        sticky_file_after_second,
        Some(second_file),
        "file sticky should switch only after scrolling past the next file banner",
    );
}

#[test]
fn sticky_file_boundary_pushes_previous_hunk_until_next_hunk_enters_stack() {
    let (_repo, app) = bootstrap_two_file_boundary_fixture();
    let second_file = app
        .domain
        .rendered_diff
        .iter()
        .enumerate()
        .find_map(|(idx, line)| line.raw_text.starts_with("==== file 2/").then_some(idx))
        .expect("second file header");
    let hunk_headers = app
        .domain
        .rendered_diff
        .iter()
        .enumerate()
        .filter_map(|(idx, line)| is_hunk_header_line(line).then_some(idx))
        .collect::<Vec<_>>();
    assert!(
        hunk_headers.len() >= 2,
        "fixture should produce at least two hunk headers",
    );
    let second_hunk = hunk_headers[1];

    let viewport_rows = app
        .ui
        .diff_ui
        .pane_rects
        .diff
        .height
        .saturating_sub(2)
        .max(1) as usize;
    let sticky_at_file_boundary = app.sticky_banner_indexes_for_scroll(second_file, viewport_rows);
    let sticky_after_second_hunk =
        app.sticky_banner_indexes_for_scroll(second_hunk.saturating_add(1), viewport_rows);

    let latest_hunk_at_boundary = sticky_at_file_boundary
        .iter()
        .copied()
        .rev()
        .find(|idx| is_hunk_header_line(&app.domain.rendered_diff[*idx]));
    let latest_hunk_after_second = sticky_after_second_hunk
        .iter()
        .copied()
        .rev()
        .find(|idx| is_hunk_header_line(&app.domain.rendered_diff[*idx]));
    assert!(
        latest_hunk_at_boundary.is_some_and(|idx| idx < second_file),
        "at a file boundary, sticky stack should keep the previous hunk until a newer one enters",
    );
    assert_eq!(
        latest_hunk_after_second,
        Some(second_hunk),
        "once the next file hunk crosses the top edge it should become the latest sticky hunk",
    );
}

#[test]
fn sticky_commit_header_pushes_previous_commit_until_next_commit_enters_stack() {
    let (_repo, app) = bootstrap_two_file_boundary_fixture();

    let commit_headers = app
        .domain
        .rendered_diff
        .iter()
        .enumerate()
        .filter_map(|(idx, line)| {
            line.anchor
                .as_ref()
                .is_some_and(is_commit_line_anchor)
                .then_some(idx)
        })
        .collect::<Vec<_>>();
    assert!(
        commit_headers.len() >= 2,
        "fixture should produce at least two commit headers",
    );
    let second_commit = commit_headers[1];

    let viewport_rows = app
        .ui
        .diff_ui
        .pane_rects
        .diff
        .height
        .saturating_sub(2)
        .max(1) as usize;
    let sticky_at_second = app.sticky_banner_indexes_for_scroll(second_commit, viewport_rows);
    let sticky_after_second =
        app.sticky_banner_indexes_for_scroll(second_commit + 1, viewport_rows);

    let sticky_commit_at_second = sticky_at_second.iter().copied().rev().find(|idx| {
        app.domain.rendered_diff[*idx]
            .anchor
            .as_ref()
            .is_some_and(is_commit_line_anchor)
    });
    let sticky_commit_after_second = sticky_after_second.iter().copied().rev().find(|idx| {
        app.domain.rendered_diff[*idx]
            .anchor
            .as_ref()
            .is_some_and(is_commit_line_anchor)
    });
    assert_eq!(
        sticky_commit_at_second,
        Some(commit_headers[0]),
        "at the incoming commit boundary, previous commit should remain until replacement reaches sticky stack",
    );
    assert_eq!(
        sticky_commit_after_second,
        Some(second_commit),
        "latest sticky commit should switch after scrolling past next commit banner",
    );
}

#[test]
fn sticky_banner_stack_is_bounded_to_three_rows() {
    let (_repo, app) = bootstrap_two_file_boundary_fixture();
    let viewport_rows = app
        .ui
        .diff_ui
        .pane_rects
        .diff
        .height
        .saturating_sub(2)
        .max(1) as usize;

    for scroll in 0..app.domain.rendered_diff.len() {
        let sticky = app.sticky_banner_indexes_for_scroll(scroll, viewport_rows);
        assert!(
            sticky.len() <= 3,
            "sticky banner stack must be capped to three rows at scroll={scroll}",
        );
        assert!(
            sticky.windows(2).all(|pair| pair[0] < pair[1]),
            "sticky indexes should remain in draw order at scroll={scroll}",
        );
        assert!(
            sticky.iter().all(|idx| {
                let line = &app.domain.rendered_diff[*idx];
                line.raw_text.starts_with("==== file ")
                    || line.anchor.as_ref().is_some_and(is_commit_line_anchor)
                    || is_hunk_header_line(line)
            }),
            "sticky rows should only include banner lines at scroll={scroll}",
        );
        if scroll > 0 {
            assert!(
                sticky.iter().all(|idx| *idx < scroll),
                "sticky rows should only include lines above the viewport top at scroll={scroll}",
            );
        }
    }
}

#[test]
fn sticky_banner_stack_moves_at_most_one_banner_per_scroll_step() {
    let (_repo, app) = bootstrap_two_file_boundary_fixture();
    let viewport_rows = app
        .ui
        .diff_ui
        .pane_rects
        .diff
        .height
        .saturating_sub(2)
        .max(1) as usize;

    let mut prior = app.sticky_banner_indexes_for_scroll(0, viewport_rows);
    for scroll in 1..app.domain.rendered_diff.len() {
        let current = app.sticky_banner_indexes_for_scroll(scroll, viewport_rows);
        let shared = prior.iter().filter(|idx| current.contains(idx)).count();
        let min_shared = prior.len().saturating_sub(1).min(current.len());
        assert!(
            shared >= min_shared,
            "adjacent scroll positions should change sticky stack by at most one row (scroll={scroll})",
        );
        prior = current;
    }
}

#[test]
fn diff_viewport_scroll_moves_exactly_requested_delta_without_scrolloff_correction() {
    let (_repo, mut app) = bootstrap_two_file_boundary_fixture();
    draw_app(&mut app, 120, 10);
    let start_scroll = app.max_diff_scroll().saturating_sub(2);
    app.set_diff_scroll(start_scroll);

    let visible = app.visible_diff_rows_for_scroll(start_scroll);
    app.domain.diff_position.cursor = start_scroll
        .saturating_add(visible.saturating_sub(1))
        .min(app.domain.rendered_diff.len().saturating_sub(1));
    app.tuning.diff_cursor_scroll_off_lines = 6;

    let expected_scroll = start_scroll.saturating_add(1).min(app.max_diff_scroll());
    app.scroll_diff_viewport(1);
    assert_eq!(
        app.domain.diff_position.scroll, expected_scroll,
        "viewport scroll should move by exactly requested delta even with non-zero scrolloff",
    );
}

#[test]
fn diff_viewport_scroll_applies_multi_line_delta_stepwise() {
    let (repo, mut app) = bootstrap_two_file_boundary_fixture();
    let mut stepwise_app = bootstrap_driver(repo.path()).into_app();
    draw_app(&mut app, 120, 10);
    draw_app(&mut stepwise_app, 120, 10);

    let second_file = app
        .domain
        .rendered_diff
        .iter()
        .enumerate()
        .find_map(|(idx, line)| line.raw_text.starts_with("==== file 2/").then_some(idx))
        .expect("second file header");
    let start_scroll = second_file.saturating_sub(1).min(app.max_diff_scroll());
    app.set_diff_scroll(start_scroll);
    app.domain.diff_position.cursor = start_scroll
        .saturating_add(
            app.visible_diff_rows_for_scroll(start_scroll)
                .saturating_sub(1),
        )
        .min(app.domain.rendered_diff.len().saturating_sub(1));
    stepwise_app.set_diff_scroll(start_scroll);
    stepwise_app.domain.diff_position.cursor = start_scroll
        .saturating_add(
            stepwise_app
                .visible_diff_rows_for_scroll(start_scroll)
                .saturating_sub(1),
        )
        .min(stepwise_app.domain.rendered_diff.len().saturating_sub(1));

    stepwise_app.scroll_diff_viewport(1);
    stepwise_app.scroll_diff_viewport(1);

    app.scroll_diff_viewport(2);
    assert_eq!(
        app.domain.diff_position.scroll, stepwise_app.domain.diff_position.scroll,
        "multi-line viewport scroll should be equivalent to two single-line steps"
    );
    assert_eq!(
        app.domain.diff_position.cursor, stepwise_app.domain.diff_position.cursor,
        "cursor projection should match stepwise scrolling semantics"
    );
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
