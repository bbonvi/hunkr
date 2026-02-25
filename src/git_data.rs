use std::{
    cell::RefCell,
    collections::{BTreeMap, BTreeSet},
    path::{Path, PathBuf},
};

use anyhow::{Context, anyhow};
use git2::{BranchType, DiffOptions, Oid, Repository, Sort};

use crate::model::{AggregatedDiff, CommitInfo, DiffLineKind, FilePatch, Hunk, HunkLine};

/// Read-only git access tailored for multi-commit review workflows.
pub struct GitService {
    repo: Repository,
    root: PathBuf,
    branch: String,
}

impl GitService {
    pub fn open_current() -> anyhow::Result<Self> {
        Self::open_at(Path::new("."))
    }

    pub fn open_at(path: &Path) -> anyhow::Result<Self> {
        let repo = Repository::discover(path).context("failed to discover git repository")?;
        let root = repo
            .workdir()
            .map(Path::to_path_buf)
            .ok_or_else(|| anyhow!("repository is bare; workdir is required"))?;
        let branch = current_branch_name(&repo)?;
        Ok(Self { repo, root, branch })
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn branch_name(&self) -> &str {
        &self.branch
    }

    pub fn load_first_parent_history(&self, max: usize) -> anyhow::Result<Vec<CommitInfo>> {
        let mut items = Vec::new();
        let mut current = self
            .repo
            .head()
            .context("failed to resolve HEAD")?
            .peel_to_commit()?;
        let unpushed = self.default_unpushed_commit_ids()?;

        loop {
            let id = current.id().to_string();
            let summary = current
                .summary()
                .map(str::to_owned)
                .unwrap_or_else(|| "(no summary)".to_owned());
            let author = current
                .author()
                .name()
                .map(str::to_owned)
                .unwrap_or_else(|| "unknown".to_owned());

            items.push(CommitInfo {
                short_id: short_id(&id),
                id: id.clone(),
                summary,
                author,
                timestamp: current.time().seconds(),
                unpushed: unpushed.contains(&id),
            });

            if items.len() >= max || current.parent_count() == 0 {
                break;
            }
            current = current.parent(0)?;
        }

        Ok(items)
    }

    pub fn default_unpushed_commit_ids(&self) -> anyhow::Result<BTreeSet<String>> {
        if let Some(oids) = self.revwalk_upstream_difference()? {
            return Ok(oids.into_iter().map(|oid| oid.to_string()).collect());
        }

        // No configured upstream: treat commits not reachable from any remote-tracking ref as local.
        let mut revwalk = self.repo.revwalk().context("failed to create revwalk")?;
        revwalk
            .set_sorting(Sort::TOPOLOGICAL | Sort::TIME)
            .context("failed to set revwalk sorting")?;
        let head = self.repo.head().context("failed to resolve HEAD")?;
        let head_oid = head.target().ok_or_else(|| anyhow!("HEAD has no target"))?;
        revwalk.push(head_oid).context("failed to push HEAD")?;

        for reference in self
            .repo
            .references_glob("refs/remotes/*")
            .context("failed to read remote references")?
        {
            let reference = reference.context("failed to read remote ref")?;
            if let Some(oid) = reference.target() {
                let _ = revwalk.hide(oid);
            }
        }

        let mut ids = BTreeSet::new();
        for oid in revwalk {
            ids.insert(oid?.to_string());
        }
        Ok(ids)
    }

    fn revwalk_upstream_difference(&self) -> anyhow::Result<Option<Vec<Oid>>> {
        let head = self.repo.head().context("failed to resolve HEAD")?;
        if !head.is_branch() {
            return Ok(None);
        }
        let branch_name = head
            .shorthand()
            .ok_or_else(|| anyhow!("failed to resolve branch shorthand"))?;
        let branch = self
            .repo
            .find_branch(branch_name, BranchType::Local)
            .with_context(|| format!("failed to resolve local branch {branch_name}"))?;

        let upstream = match branch.upstream() {
            Ok(upstream) => upstream,
            Err(_) => return Ok(None),
        };

        let head_oid = head.target().ok_or_else(|| anyhow!("HEAD has no target"))?;
        let upstream_oid = upstream
            .get()
            .target()
            .ok_or_else(|| anyhow!("upstream has no target"))?;

        let mut revwalk = self.repo.revwalk().context("failed to create revwalk")?;
        revwalk
            .set_sorting(Sort::TOPOLOGICAL | Sort::TIME)
            .context("failed to set sorting")?;
        revwalk.push(head_oid).context("failed to push HEAD")?;
        revwalk
            .hide(upstream_oid)
            .context("failed to hide upstream target")?;

        let mut ids = Vec::new();
        for oid in revwalk {
            ids.push(oid?);
        }
        Ok(Some(ids))
    }

    pub fn aggregate_for_commits(
        &self,
        ordered_commit_ids: &[String],
    ) -> anyhow::Result<AggregatedDiff> {
        let mut files = BTreeMap::<String, FilePatch>::new();

        for commit_id in ordered_commit_ids {
            let oid = Oid::from_str(commit_id)
                .with_context(|| format!("invalid commit id {commit_id}"))?;
            let commit = self
                .repo
                .find_commit(oid)
                .with_context(|| format!("failed to load commit {commit_id}"))?;
            let current_tree = commit.tree().context("failed to load commit tree")?;
            let parent_tree = if commit.parent_count() > 0 {
                Some(
                    commit
                        .parent(0)
                        .context("failed to load commit parent")?
                        .tree()
                        .context("failed to load parent tree")?,
                )
            } else {
                None
            };

            let mut opts = DiffOptions::new();
            opts.context_lines(3);
            let diff = self
                .repo
                .diff_tree_to_tree(parent_tree.as_ref(), Some(&current_tree), Some(&mut opts))
                .with_context(|| format!("failed to diff commit {commit_id}"))?;

            let summary = commit
                .summary()
                .map(str::to_owned)
                .unwrap_or_else(|| "(no summary)".to_owned());
            let short = short_id(commit_id);

            let current_path = RefCell::new(String::new());
            let current_hunk_index = RefCell::new(None::<usize>);
            let touched_paths = RefCell::new(BTreeSet::<String>::new());
            let hunked_paths = RefCell::new(BTreeSet::<String>::new());
            let commit_patches = RefCell::new(BTreeMap::<String, Vec<Hunk>>::new());

            diff.foreach(
                &mut |delta, _| {
                    let path = delta
                        .new_file()
                        .path()
                        .or_else(|| delta.old_file().path())
                        .map(path_to_string)
                        .unwrap_or_else(|| "(unknown)".to_owned());
                    *current_path.borrow_mut() = path.clone();
                    *current_hunk_index.borrow_mut() = None;
                    touched_paths.borrow_mut().insert(path.clone());
                    commit_patches.borrow_mut().entry(path).or_default();
                    true
                },
                None,
                Some(&mut |_delta, hunk| {
                    let path = current_path.borrow().clone();
                    let mut patches = commit_patches.borrow_mut();
                    let file = patches.entry(path.clone()).or_default();
                    file.push(Hunk {
                        commit_id: commit_id.clone(),
                        commit_short: short.clone(),
                        commit_summary: summary.clone(),
                        header: String::from_utf8_lossy(hunk.header())
                            .trim_end_matches('\n')
                            .to_owned(),
                        old_start: hunk.old_start(),
                        new_start: hunk.new_start(),
                        lines: Vec::new(),
                    });
                    *current_hunk_index.borrow_mut() = Some(file.len() - 1);
                    hunked_paths.borrow_mut().insert(path);
                    true
                }),
                Some(&mut |_delta, _hunk, line| {
                    let path = current_path.borrow().clone();
                    let mut patches = commit_patches.borrow_mut();
                    let file = patches.entry(path).or_default();
                    if current_hunk_index.borrow().is_none() {
                        file.push(Hunk {
                            commit_id: commit_id.clone(),
                            commit_short: short.clone(),
                            commit_summary: summary.clone(),
                            header: "@@ -0,0 +0,0 @@".to_owned(),
                            old_start: 0,
                            new_start: 0,
                            lines: Vec::new(),
                        });
                        *current_hunk_index.borrow_mut() = Some(file.len() - 1);
                    }

                    let kind = match line.origin() {
                        '+' => DiffLineKind::Add,
                        '-' => DiffLineKind::Remove,
                        ' ' => DiffLineKind::Context,
                        _ => DiffLineKind::Meta,
                    };
                    let text = String::from_utf8_lossy(line.content())
                        .trim_end_matches('\n')
                        .to_owned();
                    let hunk = file
                        .get_mut(current_hunk_index.borrow().expect("hunk index set"))
                        .expect("hunk exists");
                    hunk.lines.push(HunkLine {
                        kind,
                        text,
                        old_lineno: line.old_lineno(),
                        new_lineno: line.new_lineno(),
                    });
                    true
                }),
            )
            .with_context(|| format!("failed to iterate diff for commit {commit_id}"))?;

            let mut commit_patches = commit_patches.into_inner();
            for path in touched_paths.into_inner() {
                if hunked_paths.borrow().contains(&path) {
                    continue;
                }
                commit_patches.entry(path.clone()).or_default().push(Hunk {
                    commit_id: commit_id.clone(),
                    commit_short: short.clone(),
                    commit_summary: summary.clone(),
                    header: "@@ binary @@".to_owned(),
                    old_start: 0,
                    new_start: 0,
                    lines: vec![HunkLine {
                        kind: DiffLineKind::Meta,
                        text: "[binary or metadata-only change]".to_owned(),
                        old_lineno: None,
                        new_lineno: None,
                    }],
                });
            }

            for (path, mut hunks) in commit_patches {
                files
                    .entry(path.clone())
                    .or_insert_with(|| FilePatch {
                        path,
                        hunks: Vec::new(),
                    })
                    .hunks
                    .append(&mut hunks);
            }
        }

        Ok(AggregatedDiff { files })
    }
}

fn current_branch_name(repo: &Repository) -> anyhow::Result<String> {
    let head = repo.head().context("failed to resolve HEAD")?;
    Ok(head
        .shorthand()
        .map(str::to_owned)
        .unwrap_or_else(|| "detached-head".to_owned()))
}

fn path_to_string(path: &Path) -> String {
    path.to_string_lossy().to_string()
}

fn short_id(id: &str) -> String {
    id.chars().take(7).collect()
}

#[cfg(test)]
mod tests {
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
    fn aggregate_contains_commit_metadata_per_hunk() {
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
        assert!(
            patch
                .hunks
                .iter()
                .any(|h| h.commit_summary == "first" || h.commit_summary == "second")
        );
    }

    fn init_repo(path: &Path) {
        run(Command::new("git")
            .current_dir(path)
            .args(["init", "-b", "main"]));
        run(Command::new("git").current_dir(path).args([
            "config",
            "user.email",
            "dev@example.com",
        ]));
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
}
