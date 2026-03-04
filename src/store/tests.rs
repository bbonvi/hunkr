//! Unit tests for review state persistence and legacy-upgrade behavior.
use std::collections::{BTreeMap, BTreeSet};
use std::sync::{Arc, Barrier};
use std::thread;

use tempfile::tempdir;

use super::*;
use crate::model::{
    CommitStatusEntry, UiSessionCommitStatusFilter, UiSessionDiffPosition, UiSessionFocusPane,
    UiSessionState,
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
fn save_statuses_merged_preserves_concurrent_status_updates() {
    let tmp = tempdir().expect("tempdir");
    let store_a = StateStore::for_project(tmp.path());
    let store_b = StateStore::for_project(tmp.path());
    let mut state_a = ReviewState::default();
    let mut state_b = ReviewState::default();

    state_a.statuses.insert(
        "commit-a".to_owned(),
        status_entry(ReviewStatus::Reviewed, "main", "2026-01-01T00:00:00Z"),
    );
    store_a
        .save_statuses_merged(&mut state_a)
        .expect("save state a");

    state_b.statuses.insert(
        "commit-b".to_owned(),
        status_entry(ReviewStatus::IssueFound, "main", "2026-01-01T00:00:01Z"),
    );
    store_b
        .save_statuses_merged(&mut state_b)
        .expect("save state b");

    let loaded = store_a.load().expect("load merged state");
    assert_eq!(loaded.statuses.len(), 2);
    assert_eq!(
        loaded.statuses.get("commit-a").expect("commit-a").status,
        ReviewStatus::Reviewed
    );
    assert_eq!(
        loaded.statuses.get("commit-b").expect("commit-b").status,
        ReviewStatus::IssueFound
    );
}

#[test]
fn save_statuses_merged_preserves_disk_ui_session() {
    let tmp = tempdir().expect("tempdir");
    let store = StateStore::for_project(tmp.path());
    let disk_state = ReviewState {
        version: 2,
        statuses: BTreeMap::new(),
        ui_session: UiSessionState {
            selected_commit_ids: BTreeSet::from(["disk-selection".to_owned()]),
            ..UiSessionState::default()
        },
    };
    store.save(&disk_state).expect("seed disk state");

    let mut in_memory = ReviewState {
        version: 2,
        statuses: BTreeMap::from([(
            "commit-a".to_owned(),
            status_entry(ReviewStatus::Reviewed, "main", "2026-01-01T00:00:00Z"),
        )]),
        ui_session: UiSessionState {
            selected_commit_ids: BTreeSet::from(["local-selection".to_owned()]),
            ..UiSessionState::default()
        },
    };

    store
        .save_statuses_merged(&mut in_memory)
        .expect("save statuses");

    let loaded = store.load().expect("load merged state");
    assert_eq!(
        loaded.ui_session.selected_commit_ids,
        BTreeSet::from(["disk-selection".to_owned()])
    );
}

#[test]
fn save_state_merged_preserves_local_ui_session_and_external_statuses() {
    let tmp = tempdir().expect("tempdir");
    let store = StateStore::for_project(tmp.path());
    let disk_state = ReviewState {
        version: 2,
        statuses: BTreeMap::from([(
            "commit-disk".to_owned(),
            status_entry(ReviewStatus::IssueFound, "main", "2026-01-01T00:00:01Z"),
        )]),
        ui_session: UiSessionState {
            selected_commit_ids: BTreeSet::from(["disk-selection".to_owned()]),
            ..UiSessionState::default()
        },
    };
    store.save(&disk_state).expect("seed disk state");

    let mut in_memory = ReviewState {
        version: 2,
        statuses: BTreeMap::from([(
            "commit-local".to_owned(),
            status_entry(ReviewStatus::Reviewed, "main", "2026-01-01T00:00:02Z"),
        )]),
        ui_session: UiSessionState {
            selected_commit_ids: BTreeSet::from(["local-selection".to_owned()]),
            ..UiSessionState::default()
        },
    };

    store
        .save_state_merged(&mut in_memory)
        .expect("save merged state");

    let loaded = store.load().expect("load merged state");
    assert_eq!(loaded.statuses.len(), 2);
    assert_eq!(
        loaded.ui_session.selected_commit_ids,
        BTreeSet::from(["local-selection".to_owned()])
    );
}

#[test]
fn sync_statuses_from_disk_prefers_newer_timestamp_per_commit() {
    let tmp = tempdir().expect("tempdir");
    let store = StateStore::for_project(tmp.path());
    let disk_state = ReviewState {
        version: 2,
        statuses: BTreeMap::from([(
            "commit-1".to_owned(),
            status_entry(ReviewStatus::IssueFound, "main", "2026-01-01T00:00:02Z"),
        )]),
        ui_session: UiSessionState::default(),
    };
    store.save(&disk_state).expect("seed disk state");

    let mut in_memory = ReviewState {
        version: 2,
        statuses: BTreeMap::from([(
            "commit-1".to_owned(),
            status_entry(ReviewStatus::Reviewed, "main", "2026-01-01T00:00:01Z"),
        )]),
        ui_session: UiSessionState::default(),
    };
    store
        .sync_statuses_from_disk(&mut in_memory)
        .expect("sync statuses");

    assert_eq!(
        in_memory.statuses.get("commit-1").expect("commit-1").status,
        ReviewStatus::IssueFound
    );
}

#[test]
fn concurrent_save_statuses_merged_keeps_all_commit_updates() {
    let tmp = tempdir().expect("tempdir");
    let project_root = tmp.path().to_path_buf();
    let workers = 12usize;
    let barrier = Arc::new(Barrier::new(workers));
    let mut handles = Vec::new();

    for idx in 0..workers {
        let barrier = Arc::clone(&barrier);
        let project_root = project_root.clone();
        handles.push(thread::spawn(move || {
            let store = StateStore::for_project(&project_root);
            let mut state = ReviewState::default();
            state.statuses.insert(
                format!("commit-{idx}"),
                status_entry(
                    ReviewStatus::Reviewed,
                    "main",
                    &format!("2026-01-01T00:00:{idx:02}Z"),
                ),
            );

            barrier.wait();
            store
                .save_statuses_merged(&mut state)
                .expect("save merged status");
        }));
    }

    for handle in handles {
        handle.join().expect("join writer");
    }

    let loaded = StateStore::for_project(tmp.path())
        .load()
        .expect("load merged state");
    assert_eq!(loaded.statuses.len(), workers);
}

fn status_entry(status: ReviewStatus, branch: &str, updated_at: &str) -> CommitStatusEntry {
    CommitStatusEntry {
        status,
        branch: branch.to_owned(),
        updated_at: updated_at.to_owned(),
    }
}
