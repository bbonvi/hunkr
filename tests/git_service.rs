use std::{fs, path::Path, process::Command};

use hunkr::git_data::GitService;
use tempfile::tempdir;

#[test]
fn aggregate_for_multiple_commits_returns_net_change_only() {
    let repo_dir = tempdir().expect("tempdir");
    init_repo(repo_dir.path());
    commit_file(repo_dir.path(), "src.txt", "let a = 1;\n", "first");
    commit_file(
        repo_dir.path(),
        "src.txt",
        "let a = 2;\nlet b = 2;\n",
        "second",
    );

    let service = GitService::open_at(repo_dir.path()).expect("service");
    let history = service.load_first_parent_history(10).expect("history");
    let selected = vec![history[1].id.clone(), history[0].id.clone()];

    let aggregated = service.aggregate_for_commits(&selected).expect("aggregate");
    let patch = aggregated.files.get("src.txt").expect("src patch");

    let has_removed_intermediate = patch
        .hunks
        .iter()
        .flat_map(|h| h.lines.iter())
        .any(|line| line.text == "let a = 1;");
    assert!(
        !has_removed_intermediate,
        "net diff should not include intermediate line state"
    );

    let added_lines = patch
        .hunks
        .iter()
        .flat_map(|h| h.lines.iter())
        .map(|line| line.text.as_str())
        .collect::<Vec<_>>();
    assert!(added_lines.contains(&"let a = 2;"));
    assert!(added_lines.contains(&"let b = 2;"));

    assert!(
        patch
            .hunks
            .iter()
            .all(|h| h.commit_summary.contains("selection net changes ("))
    );
}

#[test]
fn aggregate_rename_uses_new_file_path() {
    let repo_dir = tempdir().expect("tempdir");
    init_repo(repo_dir.path());
    commit_file(repo_dir.path(), "old.txt", "hello\n", "add old");

    run(Command::new("git")
        .current_dir(repo_dir.path())
        .args(["mv", "old.txt", "new.txt"]));
    run(Command::new("git")
        .current_dir(repo_dir.path())
        .args(["commit", "-m", "rename"]));

    let service = GitService::open_at(repo_dir.path()).expect("service");
    let history = service.load_first_parent_history(10).expect("history");
    let selected = vec![history[0].id.clone()];

    let aggregated = service.aggregate_for_commits(&selected).expect("aggregate");
    let patch = aggregated
        .files
        .get("new.txt")
        .expect("renamed path present");

    assert!(patch.hunks.iter().any(|h| h.commit_summary == "rename"));
}

#[test]
fn aggregate_binary_change_emits_placeholder_hunk() {
    let repo_dir = tempdir().expect("tempdir");
    init_repo(repo_dir.path());

    fs::write(repo_dir.path().join("asset.bin"), [0u8, 159, 146, 150, 0]).expect("write asset");
    run(Command::new("git")
        .current_dir(repo_dir.path())
        .args(["add", "asset.bin"]));
    run(Command::new("git")
        .current_dir(repo_dir.path())
        .args(["commit", "-m", "add binary"]));

    fs::write(
        repo_dir.path().join("asset.bin"),
        [0u8, 159, 146, 150, 1, 0],
    )
    .expect("write asset");
    run(Command::new("git")
        .current_dir(repo_dir.path())
        .args(["add", "asset.bin"]));
    run(Command::new("git")
        .current_dir(repo_dir.path())
        .args(["commit", "-m", "update binary"]));

    let service = GitService::open_at(repo_dir.path()).expect("service");
    let history = service.load_first_parent_history(10).expect("history");
    let selected = vec![history[0].id.clone()];

    let aggregated = service.aggregate_for_commits(&selected).expect("aggregate");
    let patch = aggregated
        .files
        .get("asset.bin")
        .expect("binary path present");

    assert!(
        patch
            .hunks
            .iter()
            .flat_map(|h| h.lines.iter())
            .any(|line| { line.text.contains("binary or metadata-only change") })
    );
}

#[test]
fn aggregate_uncommitted_includes_staged_and_unstaged_changes() {
    let repo_dir = tempdir().expect("tempdir");
    init_repo(repo_dir.path());
    commit_file(repo_dir.path(), "src.txt", "line1\n", "init");

    fs::write(repo_dir.path().join("src.txt"), "line1\nline2\n").expect("update tracked");
    run(Command::new("git")
        .current_dir(repo_dir.path())
        .args(["add", "src.txt"]));

    fs::write(repo_dir.path().join("src.txt"), "line1\nline2\nline3\n")
        .expect("update tracked unstaged");
    fs::write(repo_dir.path().join("new.txt"), "new file\n").expect("write untracked");

    let service = GitService::open_at(repo_dir.path()).expect("service");
    let aggregated = service
        .aggregate_uncommitted()
        .expect("aggregate uncommitted");

    assert!(aggregated.files.contains_key("src.txt"));
    assert!(aggregated.files.contains_key("new.txt"));
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

fn run(cmd: &mut Command) {
    let output = cmd.output().expect("spawn command");
    assert!(
        output.status.success(),
        "command failed: status={:?}, stderr={} ",
        output.status.code(),
        String::from_utf8_lossy(&output.stderr)
    );
}
