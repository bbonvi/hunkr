//! Unit tests for review state persistence and legacy-upgrade behavior.
use std::collections::{BTreeMap, BTreeSet};

use tempfile::tempdir;

use super::*;
use crate::model::{
    UiSessionCommitStatusFilter, UiSessionDiffPosition, UiSessionFocusPane, UiSessionState,
};

#[test]
fn state_roundtrip_preserves_statuses() {
    let tmp = tempdir().expect("tempdir");
    let store = StateStore::for_project(tmp.path());
    let mut state = ReviewState {
        version: 2,
        statuses: BTreeMap::new(),
        ui_session: UiSessionState::default(),
    };

    store.set_status(&mut state, "abc123", ReviewStatus::IssueFound, "main");
    store.save(&state).expect("save");

    let loaded = store.load().expect("load");
    assert_eq!(loaded.version, 2);
    assert!(loaded.statuses.contains_key("abc123"));
    let entry = loaded.statuses.get("abc123").expect("status entry");
    assert_eq!(entry.status, ReviewStatus::IssueFound);
    assert_eq!(entry.branch, "main");
}

#[test]
fn load_missing_state_returns_default() {
    let tmp = tempdir().expect("tempdir");
    let store = StateStore::for_project(tmp.path());
    let loaded = store.load().expect("load");
    assert!(loaded.statuses.is_empty());
    assert_eq!(loaded.version, 2);
}

#[test]
fn set_many_status_writes_each_commit() {
    let tmp = tempdir().expect("tempdir");
    let store = StateStore::for_project(tmp.path());
    let mut state = ReviewState::default();

    store.set_many_status(
        &mut state,
        ["a1".to_string(), "b2".to_string()],
        ReviewStatus::Reviewed,
        "feature/x",
    );

    assert_eq!(state.statuses.len(), 2);
    assert_eq!(
        state.statuses.get("a1").expect("a1").status,
        ReviewStatus::Reviewed
    );
}

#[test]
fn has_state_file_tracks_persistence() {
    let tmp = tempdir().expect("tempdir");
    let store = StateStore::for_project(tmp.path());
    assert!(!store.has_state_file());

    store.save(&ReviewState::default()).expect("save");
    assert!(store.has_state_file());
}

#[test]
fn legacy_state_upgrades_to_reviewed() {
    let tmp = tempdir().expect("tempdir");
    let store = StateStore::for_project(tmp.path());
    let legacy_raw = r#"{
  "version": 1,
  "approvals": {
    "abc": {"scope": "commit", "branch": "main", "approved_at": "2024-01-01T00:00:00Z"}
  }
}"#;

    fs::create_dir_all(store.root_dir()).expect("mkdir");
    fs::write(store.state_path.clone(), legacy_raw).expect("write");

    let loaded = store.load().expect("load");
    assert_eq!(
        loaded.statuses.get("abc").expect("abc").status,
        ReviewStatus::Reviewed
    );
}

#[test]
fn load_migrates_resolved_tokens_to_reviewed() {
    let tmp = tempdir().expect("tempdir");
    let store = StateStore::for_project(tmp.path());
    let legacy_raw = r#"{
  "version": 2,
  "statuses": {
    "abc": {
      "status": "RESOLVED",
      "branch": "main",
      "updated_at": "2026-01-01T00:00:00Z"
    }
  },
  "ui_session": {
    "commit_status_filter": "REVIEWED_OR_RESOLVED"
  }
}"#;

    fs::create_dir_all(store.root_dir()).expect("mkdir");
    fs::write(store.state_path.clone(), legacy_raw).expect("write");

    let loaded = store.load().expect("load");
    assert_eq!(
        loaded.statuses.get("abc").expect("abc").status,
        ReviewStatus::Reviewed
    );
    assert_eq!(
        loaded.ui_session.commit_status_filter,
        Some(UiSessionCommitStatusFilter::Reviewed)
    );

    let persisted = fs::read_to_string(store.state_path.clone()).expect("read migrated state");
    assert!(!persisted.contains("RESOLVED"));
    assert!(!persisted.contains("REVIEWED_OR_RESOLVED"));
}

#[test]
fn shell_history_roundtrip_preserves_order() {
    let tmp = tempdir().expect("tempdir");
    let store = StateStore::for_project(tmp.path());

    let commands = vec![
        "git status".to_owned(),
        "cargo test --lib".to_owned(),
        "echo done".to_owned(),
    ];
    store
        .save_shell_history(&commands)
        .expect("save shell history");

    let loaded = store.load_shell_history().expect("load shell history");
    assert_eq!(loaded, commands);
}

#[test]
fn shell_history_missing_file_returns_empty() {
    let tmp = tempdir().expect("tempdir");
    let store = StateStore::for_project(tmp.path());

    let loaded = store.load_shell_history().expect("load shell history");
    assert!(loaded.is_empty());
}

#[test]
fn shell_history_loads_legacy_array_format() {
    let tmp = tempdir().expect("tempdir");
    let store = StateStore::for_project(tmp.path());
    fs::create_dir_all(store.root_dir()).expect("mkdir");
    fs::write(
        store.shell_history_path.clone(),
        r#"["git status", "cargo test"]"#,
    )
    .expect("write history");

    let loaded = store.load_shell_history().expect("load shell history");
    assert_eq!(
        loaded,
        vec!["git status".to_owned(), "cargo test".to_owned()]
    );
}

#[test]
fn state_roundtrip_preserves_ui_session_snapshot() {
    let tmp = tempdir().expect("tempdir");
    let store = StateStore::for_project(tmp.path());
    let state = ReviewState {
        version: 2,
        statuses: BTreeMap::new(),
        ui_session: UiSessionState {
            selected_commit_ids: BTreeSet::from(["a1".to_owned(), "b2".to_owned()]),
            commit_cursor_id: Some("b2".to_owned()),
            commit_status_filter: Some(UiSessionCommitStatusFilter::Reviewed),
            focused_pane: Some(UiSessionFocusPane::Diff),
            selected_file: Some("src/lib.rs".to_owned()),
            diff_positions: BTreeMap::from([(
                "src/lib.rs".to_owned(),
                UiSessionDiffPosition {
                    scroll: 12,
                    cursor: 18,
                },
            )]),
        },
    };

    store.save(&state).expect("save");
    let loaded = store.load().expect("load");
    assert_eq!(loaded.ui_session, state.ui_session);
}

#[test]
fn load_ignores_legacy_ui_session_theme_mode_field() {
    let tmp = tempdir().expect("tempdir");
    let store = StateStore::for_project(tmp.path());
    let legacy_raw = r#"{
  "version": 2,
  "statuses": {},
  "ui_session": {
    "selected_commit_ids": ["a1"],
    "theme_mode": "LIGHT"
  }
}"#;

    fs::create_dir_all(store.root_dir()).expect("mkdir");
    fs::write(store.state_path.clone(), legacy_raw).expect("write");

    let loaded = store.load().expect("load");
    assert_eq!(
        loaded.ui_session.selected_commit_ids,
        BTreeSet::from(["a1".to_owned()])
    );
}

#[test]
fn acquire_instance_lock_creates_and_releases_lock_file() {
    let tmp = tempdir().expect("tempdir");
    let project_root = tmp.path().join("repo");
    let store = StateStore::for_project(&project_root);
    let lock_path = project_root.join(".hunkr").join("instance.lock");

    let lock = store.acquire_instance_lock().expect("acquire lock");
    assert!(lock_path.exists());
    drop(lock);
    assert!(!lock_path.exists());
}

#[test]
fn acquire_instance_lock_hard_stops_when_lock_file_exists() {
    let tmp = tempdir().expect("tempdir");
    let project_root = tmp.path().join("repo");
    let store = StateStore::for_project(&project_root);

    let lock = store.acquire_instance_lock().expect("acquire first lock");
    let err = store
        .acquire_instance_lock()
        .expect_err("second lock should fail");
    let message = err.to_string();
    assert!(message.contains(".hunkr/instance.lock"));
    drop(lock);
}
