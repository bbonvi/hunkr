//! Unit tests for review state persistence and legacy-upgrade behavior.
use std::collections::BTreeMap;

use tempfile::tempdir;

use super::*;

#[test]
fn state_roundtrip_preserves_statuses() {
    let tmp = tempdir().expect("tempdir");
    let store = StateStore::for_project(tmp.path());
    let mut state = ReviewState {
        version: 2,
        statuses: BTreeMap::new(),
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
        ReviewStatus::Resolved,
        "feature/x",
    );

    assert_eq!(state.statuses.len(), 2);
    assert_eq!(
        state.statuses.get("a1").expect("a1").status,
        ReviewStatus::Resolved
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
