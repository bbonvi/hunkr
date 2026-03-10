//! Unit tests for git service data loading and commit-range aggregation behavior.
use std::{
    fs,
    path::Path,
    process::{Command, Stdio},
};

use tempfile::tempdir;

use super::*;

#[test]
fn default_unpushed_uses_upstream_when_available() {
    let repo_dir = tempdir().expect("tempdir");
    init_repo(repo_dir.path());
    commit_file(repo_dir.path(), "f.txt", "one\n", "init");

    let remote_dir = tempdir().expect("remote");
    run(Command::new("git")
        .arg("init")
        .arg("--bare")
        .arg(remote_dir.path())
        .stdout(Stdio::null())
        .stderr(Stdio::null()));

    run(Command::new("git")
        .current_dir(repo_dir.path())
        .args(["remote", "add", "origin"])
        .arg(remote_dir.path()));

    let branch = current_branch(repo_dir.path());
    run(Command::new("git")
        .current_dir(repo_dir.path())
        .args(["push", "-u", "origin", &branch]));

    commit_file(repo_dir.path(), "f.txt", "one\ntwo\n", "local");
    let local_head = git_out(
        Command::new("git")
            .current_dir(repo_dir.path())
            .args(["rev-parse", "HEAD"]),
    )
    .trim()
    .to_owned();

    let service = GitService::open_at(repo_dir.path()).expect("service");
    let unpushed = service.default_unpushed_commit_ids().expect("unpushed");

    assert!(unpushed.contains(&local_head));
    assert_eq!(unpushed.len(), 1);
}

#[test]
fn default_unpushed_without_upstream_returns_local_commits() {
    let repo_dir = tempdir().expect("tempdir");
    init_repo(repo_dir.path());
    commit_file(repo_dir.path(), "a.txt", "a\n", "a");
    commit_file(repo_dir.path(), "a.txt", "a\nb\n", "b");

    let service = GitService::open_at(repo_dir.path()).expect("service");
    let unpushed = service.default_unpushed_commit_ids().expect("unpushed");
    assert_eq!(unpushed.len(), 2);
}

#[test]
fn local_exclude_path_uses_common_git_dir_for_linked_worktrees() {
    let repo_dir = tempdir().expect("tempdir");
    init_repo(repo_dir.path());
    commit_file(repo_dir.path(), "f.txt", "one\n", "init");

    let worktree_dir = tempdir().expect("worktree");
    run(Command::new("git")
        .current_dir(repo_dir.path())
        .args(["worktree", "add", "--detach"])
        .arg(worktree_dir.path()));

    let service = GitService::open_at(worktree_dir.path()).expect("service");
    assert_eq!(
        service.local_exclude_path(),
        repo_dir.path().join(".git").join("info").join("exclude")
    );
}

#[test]
fn aggregate_for_multiple_commits_returns_only_net_changes() {
    let repo_dir = tempdir().expect("tempdir");
    init_repo(repo_dir.path());
    commit_file(repo_dir.path(), "src.txt", "let a = 1;\n", "first");
    commit_file(
        repo_dir.path(),
        "src.txt",
        "let a = 2;\nlet b = 3;\n",
        "second",
    );

    let service = GitService::open_at(repo_dir.path()).expect("service");
    let first_parent_history = service.load_first_parent_history(10).expect("history");
    assert!(first_parent_history.len() >= 2);
    let selected = vec![
        first_parent_history[1].id.clone(),
        first_parent_history[0].id.clone(),
    ];
    let aggregated = service.aggregate_for_commits(&selected).expect("aggregate");

    let patch = aggregated.files.get("src.txt").expect("src patch");
    let has_removed_first_value = patch
        .hunks
        .iter()
        .flat_map(|h| h.lines.iter())
        .any(|line| line.kind == DiffLineKind::Remove && line.text.as_ref() == "-let a = 1;");
    assert!(
        !has_removed_first_value,
        "net diff should not include intermediate churn"
    );

    let added_lines = patch
        .hunks
        .iter()
        .flat_map(|h| h.lines.iter())
        .filter(|line| line.kind == DiffLineKind::Add)
        .map(|line| line.text.as_ref())
        .collect::<Vec<_>>();
    assert!(added_lines.contains(&"+let a = 2;"));
    assert!(added_lines.contains(&"+let b = 3;"));

    let newest = &first_parent_history[0];
    assert!(
        patch
            .hunks
            .iter()
            .all(|h| h.commit_id.as_ref() == newest.id && h.commit_short.is_empty())
    );
    assert!(
        patch
            .hunks
            .iter()
            .all(|h| h.commit_summary.contains("selection net changes ("))
    );
}

#[test]
fn aggregate_for_single_commit_merges_nearby_hunks_by_default() {
    let repo_dir = tempdir().expect("tempdir");
    init_repo(repo_dir.path());
    commit_file(
        repo_dir.path(),
        "spread.txt",
        &numbered_lines(1, 80),
        "seed spread",
    );

    let mut lines = numbered_lines_vec(1, 80);
    lines[9] = "line-010 updated\n".to_owned();
    lines[29] = "line-030 updated\n".to_owned();
    commit_file(
        repo_dir.path(),
        "spread.txt",
        &lines.concat(),
        "touch distant lines",
    );

    let service = GitService::open_at(repo_dir.path()).expect("service");
    let history = service.load_first_parent_history(10).expect("history");
    let selected = vec![history[0].id.clone()];
    let aggregate = service.aggregate_for_commits(&selected).expect("aggregate");
    let patch = aggregate.files.get("spread.txt").expect("spread patch");

    assert_eq!(
        patch.hunks.len(),
        1,
        "default merge distance should coalesce nearby hunks into one block",
    );
}

#[test]
fn aggregate_with_options_respects_context_and_merge_distance() {
    let repo_dir = tempdir().expect("tempdir");
    init_repo(repo_dir.path());
    commit_file(
        repo_dir.path(),
        "spread.txt",
        &numbered_lines(1, 80),
        "seed spread",
    );

    let mut lines = numbered_lines_vec(1, 80);
    lines[9] = "line-010 updated\n".to_owned();
    lines[29] = "line-030 updated\n".to_owned();
    commit_file(
        repo_dir.path(),
        "spread.txt",
        &lines.concat(),
        "touch distant lines",
    );

    let service = GitService::open_at(repo_dir.path()).expect("service");
    let history = service.load_first_parent_history(10).expect("history");
    let selected = vec![history[0].id.clone()];
    let default_aggregate = service
        .aggregate_for_commits(&selected)
        .expect("default aggregate");
    let custom_aggregate = service
        .aggregate_for_commits_with_options(&selected, 1, 0)
        .expect("custom aggregate");

    let default_patch = default_aggregate
        .files
        .get("spread.txt")
        .expect("default patch");
    let custom_patch = custom_aggregate
        .files
        .get("spread.txt")
        .expect("custom patch");

    assert_eq!(
        default_patch.hunks.len(),
        1,
        "default settings should merge hunks"
    );
    assert_eq!(
        custom_patch.hunks.len(),
        2,
        "merge distance 0 should keep separated hunks distinct",
    );
    assert_eq!(
        custom_patch.hunks[0].old_start, 9,
        "context=1 should start hunk one line before the changed line",
    );
    assert_eq!(
        custom_patch.hunks[0].new_start, 9,
        "context=1 should mirror old/new starts for in-place line edits",
    );
}

#[test]
fn aggregate_sanitizes_crlf_payload_without_carriage_returns() {
    let repo_dir = tempdir().expect("tempdir");
    init_repo(repo_dir.path());
    commit_file(repo_dir.path(), "src.txt", "line 1\n", "first");
    fs::write(repo_dir.path().join("src.txt"), "line 1\r\nline 2\r\n").expect("write crlf");
    run(Command::new("git")
        .current_dir(repo_dir.path())
        .args(["add", "src.txt"]));
    run(Command::new("git")
        .current_dir(repo_dir.path())
        .args(["commit", "-m", "second"]));

    let service = GitService::open_at(repo_dir.path()).expect("service");
    let history = service.load_first_parent_history(10).expect("history");
    let selected = vec![history[0].id.clone()];
    let aggregate = service.aggregate_for_commits(&selected).expect("aggregate");
    let patch = aggregate.files.get("src.txt").expect("src patch");

    assert!(
        patch
            .hunks
            .iter()
            .flat_map(|hunk| hunk.lines.iter())
            .all(|line| !line.text.contains('\r')),
        "line payload should not contain carriage returns after sanitization",
    );
}

#[test]
fn load_first_parent_history_includes_ref_decorations() {
    let repo_dir = tempdir().expect("tempdir");
    init_repo(repo_dir.path());
    commit_file(repo_dir.path(), "f.txt", "one\n", "first");
    commit_file(repo_dir.path(), "f.txt", "one\ntwo\n", "second");
    run(Command::new("git")
        .current_dir(repo_dir.path())
        .args(["tag", "v0", "HEAD~1"]));

    let service = GitService::open_at(repo_dir.path()).expect("service");
    let history = service.load_first_parent_history(10).expect("history");
    let head = history.first().expect("head commit");
    let head_labels = head
        .decorations
        .iter()
        .map(|item| item.label.as_str())
        .collect::<Vec<_>>();

    assert!(head_labels.contains(&"main*"));
    assert!(head_labels.contains(&"main"));

    let first_id = git_out(
        Command::new("git")
            .current_dir(repo_dir.path())
            .args(["rev-parse", "HEAD~1"]),
    )
    .trim()
    .to_owned();
    let first = history
        .iter()
        .find(|entry| entry.id == first_id)
        .expect("first commit");
    assert!(
        first
            .decorations
            .iter()
            .any(|item| item.kind == CommitDecorationKind::Tag && item.label == "v0")
    );
}

#[test]
fn load_first_parent_history_omits_remote_head_decoration() {
    let repo_dir = tempdir().expect("tempdir");
    init_repo(repo_dir.path());
    commit_file(repo_dir.path(), "f.txt", "one\n", "first");
    run(Command::new("git").current_dir(repo_dir.path()).args([
        "update-ref",
        "refs/remotes/origin/main",
        "HEAD",
    ]));
    run(Command::new("git").current_dir(repo_dir.path()).args([
        "symbolic-ref",
        "refs/remotes/origin/HEAD",
        "refs/remotes/origin/main",
    ]));

    let service = GitService::open_at(repo_dir.path()).expect("service");
    let history = service.load_first_parent_history(10).expect("history");
    let head = history.first().expect("head commit");
    let labels = head
        .decorations
        .iter()
        .map(|item| item.label.as_str())
        .collect::<Vec<_>>();

    assert!(labels.contains(&"origin/main"));
    assert!(!labels.iter().any(|label| label.ends_with("/HEAD")));
}

#[test]
fn aggregate_for_single_commit_reports_rename_metadata() {
    let repo_dir = tempdir().expect("tempdir");
    init_repo(repo_dir.path());
    commit_file(repo_dir.path(), "old.txt", "hello\n", "first");
    run(Command::new("git")
        .current_dir(repo_dir.path())
        .args(["mv", "old.txt", "new.txt"]));
    run(Command::new("git")
        .current_dir(repo_dir.path())
        .args(["commit", "-m", "rename"]));

    let service = GitService::open_at(repo_dir.path()).expect("service");
    let history = service.load_first_parent_history(10).expect("history");
    let selected = vec![history[0].id.clone()];
    let aggregate = service.aggregate_for_commits(&selected).expect("aggregate");
    let change = aggregate
        .file_changes
        .get("new.txt")
        .expect("new path change metadata");

    assert_eq!(change.kind, FileChangeKind::Renamed);
    assert_eq!(change.old_path.as_deref(), Some("old.txt"));
}

#[test]
fn aggregate_for_single_commit_rewrite_keeps_modified_kind() {
    let repo_dir = tempdir().expect("tempdir");
    init_repo(repo_dir.path());
    let original = (1..=174)
        .map(|idx| format!("line-{idx}\n"))
        .collect::<String>();
    commit_file(repo_dir.path(), "rewrite.txt", &original, "seed");

    let rewritten = (105..=174)
        .map(|idx| format!("line-{idx}\n"))
        .collect::<String>();
    commit_file(repo_dir.path(), "rewrite.txt", &rewritten, "rewrite");

    let service = GitService::open_at(repo_dir.path()).expect("service");
    let history = service.load_first_parent_history(10).expect("history");
    let selected = vec![history[0].id.clone()];
    let aggregate = service.aggregate_for_commits(&selected).expect("aggregate");
    let change = aggregate
        .file_changes
        .get("rewrite.txt")
        .expect("rewrite metadata");

    assert_eq!(change.kind, FileChangeKind::Modified);
    assert!(change.deletions > 0);
    assert!(
        aggregate.files.contains_key("rewrite.txt"),
        "rewrite should keep the path visible in aggregated patches"
    );
}

#[test]
fn aggregate_uncommitted_includes_untracked_text_file_content() {
    let repo_dir = tempdir().expect("tempdir");
    init_repo(repo_dir.path());
    commit_file(repo_dir.path(), "tracked.txt", "base\n", "base");
    fs::write(repo_dir.path().join("new_file.rs"), "fn added() {}\n").expect("write");

    let service = GitService::open_at(repo_dir.path()).expect("service");
    let aggregate = service.aggregate_uncommitted().expect("aggregate");
    let patch = aggregate.files.get("new_file.rs").expect("untracked patch");

    assert!(
        patch
            .hunks
            .iter()
            .flat_map(|hunk| hunk.lines.iter())
            .any(|line| line.kind == DiffLineKind::Add && line.text.contains("fn added() {}"))
    );
}

#[test]
fn aggregate_uncommitted_records_file_change_kinds() {
    let repo_dir = tempdir().expect("tempdir");
    init_repo(repo_dir.path());
    commit_file(repo_dir.path(), "tracked.txt", "base\nnext\n", "base");
    fs::remove_file(repo_dir.path().join("tracked.txt")).expect("remove tracked file");
    fs::write(repo_dir.path().join("new_file.rs"), "fn added() {}\n").expect("write untracked");

    let service = GitService::open_at(repo_dir.path()).expect("service");
    let aggregate = service.aggregate_uncommitted().expect("aggregate");

    let deleted = aggregate
        .file_changes
        .get("tracked.txt")
        .expect("deleted metadata");
    assert_eq!(deleted.kind, FileChangeKind::Deleted);
    assert!(deleted.deletions > 0);

    let added = aggregate
        .file_changes
        .get("new_file.rs")
        .expect("added metadata");
    assert!(matches!(
        added.kind,
        FileChangeKind::Untracked | FileChangeKind::Added
    ));
    assert!(added.additions > 0);
}

#[test]
fn aggregate_uncommitted_includes_hunkr_metadata_when_git_exposes_it() {
    let repo_dir = tempdir().expect("tempdir");
    let excludes_dir = tempdir().expect("excludes");
    init_repo(repo_dir.path());
    let excludes_path = excludes_dir.path().join("empty-excludes");
    fs::write(&excludes_path, "").expect("write empty excludes");
    run(Command::new("git")
        .current_dir(repo_dir.path())
        .args(["config", "core.excludesFile"])
        .arg(&excludes_path));
    fs::write(
        repo_dir.path().join(".gitignore"),
        "!/.hunkr/\n!/.hunkr/**\n",
    )
    .expect("write gitignore");
    run(Command::new("git")
        .current_dir(repo_dir.path())
        .args(["add", ".gitignore"]));
    run(Command::new("git")
        .current_dir(repo_dir.path())
        .args(["commit", "-m", "track gitignore"]));
    commit_file(repo_dir.path(), "tracked.txt", "base\n", "base");
    fs::create_dir_all(repo_dir.path().join(".hunkr")).expect("create hunkr");
    fs::write(repo_dir.path().join(".hunkr/state.json"), "{}\n").expect("write state");
    assert_eq!(
        git_out(Command::new("git").current_dir(repo_dir.path()).args([
            "status",
            "--short",
            "--ignored",
            ".hunkr"
        ]),)
        .trim(),
        "?? .hunkr/"
    );

    let service = GitService::open_at(repo_dir.path()).expect("service");
    let aggregate = service.aggregate_uncommitted().expect("aggregate");
    let file_count = service
        .uncommitted_file_count()
        .expect("uncommitted file count");

    let metadata = aggregate
        .files
        .get(".hunkr/state.json")
        .expect("metadata patch should be visible");
    assert!(
        !metadata.hunks.is_empty(),
        "visible git changes should render diff hunks"
    );
    assert_eq!(
        file_count, 1,
        "visible git changes should contribute to uncommitted row count"
    );
}

#[test]
fn uncommitted_file_count_includes_tracked_and_untracked_changes() {
    let repo_dir = tempdir().expect("tempdir");
    init_repo(repo_dir.path());
    commit_file(repo_dir.path(), "tracked.txt", "base\n", "base");
    fs::write(repo_dir.path().join("new_file.rs"), "fn added() {}\n").expect("write untracked");
    fs::write(repo_dir.path().join("tracked.txt"), "base\nnext\n").expect("write tracked");

    let service = GitService::open_at(repo_dir.path()).expect("service");
    let file_count = service
        .uncommitted_file_count()
        .expect("uncommitted file count");

    assert_eq!(file_count, 2);
}

#[test]
fn commits_affecting_selection_matches_only_commits_touching_selected_lines() {
    let repo_dir = tempdir().expect("tempdir");
    init_repo(repo_dir.path());
    commit_file(repo_dir.path(), "src.txt", "let a = 1;\n", "first");
    commit_file(
        repo_dir.path(),
        "src.txt",
        "let a = 2;\nlet b = 3;\n",
        "second",
    );
    commit_file(repo_dir.path(), "other.txt", "x\n", "third");

    let service = GitService::open_at(repo_dir.path()).expect("service");
    let history = service.load_first_parent_history(10).expect("history");
    let selected = vec![
        history[2].id.clone(),
        history[1].id.clone(),
        history[0].id.clone(),
    ];

    let affected = service
        .commits_affecting_selection(
            &selected,
            "src.txt",
            &[String::from("+let a = 2;"), String::from("+let b = 3;")],
        )
        .expect("affected");

    assert_eq!(affected, BTreeSet::from([history[1].id.clone()]));
}

#[test]
fn commits_affecting_selection_without_changed_lines_uses_file_scope() {
    let repo_dir = tempdir().expect("tempdir");
    init_repo(repo_dir.path());
    commit_file(repo_dir.path(), "src.txt", "let a = 1;\n", "first");
    commit_file(
        repo_dir.path(),
        "src.txt",
        "let a = 2;\nlet b = 3;\n",
        "second",
    );
    commit_file(repo_dir.path(), "other.txt", "x\n", "third");

    let service = GitService::open_at(repo_dir.path()).expect("service");
    let history = service.load_first_parent_history(10).expect("history");
    let selected = vec![
        history[2].id.clone(),
        history[1].id.clone(),
        history[0].id.clone(),
    ];

    let affected = service
        .commits_affecting_selection(&selected, "src.txt", &[])
        .expect("affected");

    assert_eq!(
        affected,
        BTreeSet::from([history[2].id.clone(), history[1].id.clone()])
    );
}

#[test]
fn open_at_without_repository_returns_clear_error() {
    let non_repo = tempdir().expect("tempdir");
    let err = match GitService::open_at(non_repo.path()) {
        Ok(_) => panic!("missing repo should fail"),
        Err(err) => err,
    };
    let rendered = format!("{err:#}");

    assert!(
        rendered.contains("no git repository found at or above"),
        "expected friendly missing-repo error, got: {rendered}"
    );
}

#[test]
fn parse_worktree_list_parses_branches_and_flags() {
    let payload = concat!(
        "worktree /repo/main\0",
        "HEAD abc123\0",
        "branch refs/heads/main\0",
        "\0",
        "worktree /tmp/wt-1\0",
        "HEAD def456\0",
        "detached\0",
        "locked by admin\0",
        "prunable stale\0",
        "\0",
    );

    let parsed = parse_worktree_list_porcelain(payload.as_bytes()).expect("parse");
    assert_eq!(parsed.len(), 2);
    assert_eq!(parsed[0].path, Path::new("/repo/main"));
    assert_eq!(parsed[0].head, "abc123");
    assert_eq!(parsed[0].latest_commit_ts, None);
    assert_eq!(parsed[0].branch.as_deref(), Some("main"));
    assert_eq!(parsed[0].locked_reason, None);
    assert_eq!(parsed[0].prunable_reason, None);

    assert_eq!(parsed[1].path, Path::new("/tmp/wt-1"));
    assert_eq!(parsed[1].head, "def456");
    assert_eq!(parsed[1].latest_commit_ts, None);
    assert_eq!(parsed[1].branch, None);
    assert_eq!(parsed[1].locked_reason.as_deref(), Some("by admin"));
    assert_eq!(parsed[1].prunable_reason.as_deref(), Some("stale"));
}

#[test]
fn parse_worktree_list_rejects_field_before_worktree() {
    let err = match parse_worktree_list_porcelain(b"HEAD abc123\0\0") {
        Ok(_) => panic!("parser should reject malformed payload"),
        Err(err) => err,
    };

    assert!(
        format!("{err:#}").contains("field before worktree path"),
        "unexpected parse error: {err:#}"
    );
}

#[test]
fn sort_worktrees_keeps_main_first_then_newest_linked_entries() {
    let main = Path::new("/repo/main").to_path_buf();
    let older = Path::new("/tmp/wt-old").to_path_buf();
    let newer = Path::new("/tmp/wt-new").to_path_buf();
    let mut worktrees = vec![
        WorktreeInfo {
            path: older.clone(),
            head: "a".to_owned(),
            latest_commit_ts: Some(10),
            branch: Some("old".to_owned()),
            locked_reason: None,
            prunable_reason: None,
        },
        WorktreeInfo {
            path: main.clone(),
            head: "b".to_owned(),
            latest_commit_ts: Some(5),
            branch: Some("main".to_owned()),
            locked_reason: None,
            prunable_reason: None,
        },
        WorktreeInfo {
            path: newer.clone(),
            head: "c".to_owned(),
            latest_commit_ts: Some(20),
            branch: Some("new".to_owned()),
            locked_reason: None,
            prunable_reason: None,
        },
    ];
    sort_worktrees(&mut worktrees, &main);

    assert_eq!(worktrees[0].path, main);
    assert_eq!(worktrees[1].path, newer);
    assert_eq!(worktrees[2].path, older);
}

fn numbered_lines(start: usize, end: usize) -> String {
    numbered_lines_vec(start, end).concat()
}

fn numbered_lines_vec(start: usize, end: usize) -> Vec<String> {
    (start..=end)
        .map(|idx| format!("line-{idx:03}\n"))
        .collect()
}

fn init_repo(path: &Path) {
    run(Command::new("git")
        .current_dir(path)
        .args(["init", "-b", "main"]));
    run(Command::new("git")
        .current_dir(path)
        .args(["config", "user.email", "dev@example.com"]));
    run(Command::new("git")
        .current_dir(path)
        .args(["config", "user.name", "Dev"]));
}

fn commit_file(path: &Path, file: &str, contents: &str, message: &str) {
    fs::write(path.join(file), contents).expect("write file");
    run(Command::new("git").current_dir(path).args(["add", file]));
    run(Command::new("git")
        .current_dir(path)
        .args(["commit", "-m", message]));
}

fn current_branch(path: &Path) -> String {
    git_out(
        Command::new("git")
            .current_dir(path)
            .args(["rev-parse", "--abbrev-ref", "HEAD"]),
    )
    .trim()
    .to_owned()
}

fn run(cmd: &mut Command) {
    let output = cmd.output().expect("spawn command");
    assert!(
        output.status.success(),
        "command failed: status={:?}, stderr={} ",
        output.status.code(),
        String::from_utf8_lossy(&output.stderr)
    );
}

fn git_out(cmd: &mut Command) -> String {
    let output = cmd.output().expect("spawn command");
    assert!(output.status.success(), "git command failed");
    String::from_utf8_lossy(&output.stdout).to_string()
}
