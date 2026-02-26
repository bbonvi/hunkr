
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
        .any(|line| line.kind == DiffLineKind::Remove && line.text == "let a = 1;");
    assert!(
        !has_removed_first_value,
        "net diff should not include intermediate churn"
    );

    let added_lines = patch
        .hunks
        .iter()
        .flat_map(|h| h.lines.iter())
        .filter(|line| line.kind == DiffLineKind::Add)
        .map(|line| line.text.as_str())
        .collect::<Vec<_>>();
    assert!(added_lines.contains(&"let a = 2;"));
    assert!(added_lines.contains(&"let b = 3;"));

    let newest = &first_parent_history[0];
    assert!(
        patch
            .hunks
            .iter()
            .all(|h| h.commit_id == newest.id && h.commit_short.is_empty())
    );
    assert!(
        patch
            .hunks
            .iter()
            .all(|h| h.commit_summary.contains("selection net changes ("))
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
