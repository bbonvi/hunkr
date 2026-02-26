
use std::collections::BTreeSet;

use tempfile::tempdir;

use super::*;
use crate::model::{CommentAnchor, CommentTargetKind};

fn make_target() -> CommentTarget {
    make_target_for_commit("abc1234")
}

fn make_target_for_commit(commit_id: &str) -> CommentTarget {
    let anchor = CommentAnchor {
        commit_id: commit_id.to_string(),
        commit_summary: "add parser".to_string(),
        file_path: "src/lib.rs".to_string(),
        hunk_header: "@@ -1,3 +1,8 @@".to_string(),
        old_lineno: Some(1),
        new_lineno: Some(8),
    };
    CommentTarget {
        kind: CommentTargetKind::Hunk,
        start: anchor.clone(),
        end: anchor,
        commits: BTreeSet::from([commit_id.to_owned()]),
        selected_lines: vec!["+let x = 1;".to_owned(), "-let x = 0;".to_owned()],
    }
}

#[test]
fn add_update_delete_roundtrip() {
    let tmp = tempdir().expect("tempdir");
    let mut store = CommentStore::new(tmp.path(), "feature/test").expect("new store");

    let id = store
        .add_comment(&make_target(), "Need better naming")
        .expect("add");
    store
        .sync_review_tasks_report(|_| ReviewStatus::IssueFound)
        .expect("sync");
    assert!(store.report_path().exists());
    let report = fs::read_to_string(store.report_path()).expect("read report");
    assert!(report.contains("- Commit Context: add parser"));
    assert_eq!(store.comments().len(), 1);

    let updated = store.update_comment(id, "Renamed now").expect("update");
    assert!(updated);
    assert_eq!(store.comment_by_id(id).expect("id").text, "Renamed now");

    let deleted = store.delete_comment(id).expect("delete");
    assert!(deleted);
    assert!(store.comments().is_empty());

    let reloaded = CommentStore::new(tmp.path(), "feature/test").expect("reload");
    assert!(reloaded.comments().is_empty());
}

#[test]
fn format_anchor_lines_works() {
    assert_eq!(format_anchor_lines(Some(1), Some(2)), "old 1 / new 2");
    assert_eq!(format_anchor_lines(None, None), "n/a");
}

#[test]
fn legacy_comment_index_without_kind_defaults_to_hunk() {
    let tmp = tempdir().expect("tempdir");
    let root = tmp.path().join(COMMENTS_DIR);
    fs::create_dir_all(&root).expect("mkdir comments");
    let index = root.join(COMMENTS_INDEX_FILE);
    let legacy = r#"
[
  {
    "id": 1,
    "target": {
      "start": {
        "commit_id": "abc1234",
        "commit_summary": "summary",
        "file_path": "src/lib.rs",
        "hunk_header": "@@ -1,1 +1,1 @@",
        "old_lineno": 1,
        "new_lineno": 1
      },
      "end": {
        "commit_id": "abc1234",
        "commit_summary": "summary",
        "file_path": "src/lib.rs",
        "hunk_header": "@@ -1,1 +1,1 @@",
        "old_lineno": 1,
        "new_lineno": 1
      },
      "commits": ["abc1234"],
      "selected_lines": ["+x"]
    },
    "text": "legacy",
    "created_at": "2026-01-01T00:00:00Z",
    "updated_at": "2026-01-01T00:00:00Z"
  }
]
"#;
    fs::write(index, legacy).expect("write legacy index");

    let store = CommentStore::new(tmp.path(), "main").expect("load");
    let comment = store.comments().first().expect("comment");
    assert_eq!(comment.target.kind, CommentTargetKind::Hunk);
}

#[test]
fn sync_report_hides_reviewed_and_resolved_comments() {
    let tmp = tempdir().expect("tempdir");
    let mut store = CommentStore::new(tmp.path(), "feature/test").expect("new store");
    let first = store
        .add_comment(&make_target_for_commit("a1"), "first")
        .expect("add first");
    let second = store
        .add_comment(&make_target_for_commit("b2"), "second")
        .expect("add second");

    let report_path = store
        .sync_review_tasks_report(|commit| match commit {
            "a1" => ReviewStatus::IssueFound,
            "b2" => ReviewStatus::Reviewed,
            _ => ReviewStatus::Unreviewed,
        })
        .expect("sync");

    let report = fs::read_to_string(report_path).expect("read report");
    assert!(report.contains(&format!("TASK #{}", first)));
    assert!(!report.contains(&format!("TASK #{}", second)));
    assert!(report.contains("- Actionable tasks: 1"));
    assert!(!report.contains("## Source Task Coverage"));
}

#[test]
fn sync_report_uses_comment_ids_even_with_high_comment_ids() {
    let tmp = tempdir().expect("tempdir");
    let root = tmp.path().join(COMMENTS_DIR);
    fs::create_dir_all(&root).expect("mkdir comments");
    let index = root.join(COMMENTS_INDEX_FILE);
    let seeded = r#"
[
  {
    "id": 41,
    "target": {
      "kind": "HUNK",
      "start": {
        "commit_id": "a1",
        "commit_summary": "summary a",
        "file_path": "src/lib.rs",
        "hunk_header": "@@ -1,1 +1,1 @@",
        "old_lineno": 1,
        "new_lineno": 1
      },
      "end": {
        "commit_id": "a1",
        "commit_summary": "summary a",
        "file_path": "src/lib.rs",
        "hunk_header": "@@ -1,1 +1,1 @@",
        "old_lineno": 1,
        "new_lineno": 1
      },
      "commits": ["a1"],
      "selected_lines": ["+x"]
    },
    "text": "first",
    "created_at": "2026-01-01T00:00:00Z",
    "updated_at": "2026-01-01T00:00:00Z"
  },
  {
    "id": 88,
    "target": {
      "kind": "HUNK",
      "start": {
        "commit_id": "b2",
        "commit_summary": "summary b",
        "file_path": "src/main.rs",
        "hunk_header": "@@ -2,1 +2,1 @@",
        "old_lineno": 2,
        "new_lineno": 2
      },
      "end": {
        "commit_id": "b2",
        "commit_summary": "summary b",
        "file_path": "src/main.rs",
        "hunk_header": "@@ -2,1 +2,1 @@",
        "old_lineno": 2,
        "new_lineno": 2
      },
      "commits": ["b2"],
      "selected_lines": ["+y"]
    },
    "text": "second",
    "created_at": "2026-01-01T00:00:00Z",
    "updated_at": "2026-01-01T00:00:00Z"
  }
]
"#;
    fs::write(index, seeded).expect("write seeded index");

    let store = CommentStore::new(tmp.path(), "feature/test").expect("new store");
    let report_path = store
        .sync_review_tasks_report(|_| ReviewStatus::IssueFound)
        .expect("sync");
    let report = fs::read_to_string(report_path).expect("read report");

    assert!(report.contains("TASK #41"));
    assert!(report.contains("TASK #88"));
    assert!(!report.contains("TASK #1"));
    assert!(!report.contains("TASK #2"));
}
