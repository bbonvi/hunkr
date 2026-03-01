use std::{fs, path::Path, process::Command};

use hunkr::git_data::GitService;
use tempfile::tempdir;

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

fn run(cmd: &mut Command) {
    let output = cmd.output().expect("spawn command");
    assert!(
        output.status.success(),
        "command failed: status={:?}, stderr={} ",
        output.status.code(),
        String::from_utf8_lossy(&output.stderr)
    );
}
