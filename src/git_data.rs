use std::{
    cell::RefCell,
    collections::{BTreeMap, BTreeSet},
    path::{Path, PathBuf},
};

use anyhow::{Context, anyhow};
use chrono::Utc;
use git2::{BranchType, DiffOptions, Oid, Repository, Sort};

use crate::model::{
    AggregatedDiff, CommitInfo, DiffLineKind, FilePatch, Hunk, HunkLine, UNCOMMITTED_COMMIT_ID,
    UNCOMMITTED_COMMIT_SHORT, UNCOMMITTED_COMMIT_SUMMARY,
};

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
        if ordered_commit_ids.is_empty() {
            return Ok(AggregatedDiff::default());
        }

        let oldest_id = ordered_commit_ids
            .first()
            .expect("checked non-empty commit selection");
        let newest_id = ordered_commit_ids
            .last()
            .expect("checked non-empty commit selection");

        let oldest_oid =
            Oid::from_str(oldest_id).with_context(|| format!("invalid commit id {oldest_id}"))?;
        let newest_oid =
            Oid::from_str(newest_id).with_context(|| format!("invalid commit id {newest_id}"))?;

        let oldest_commit = self
            .repo
            .find_commit(oldest_oid)
            .with_context(|| format!("failed to load commit {oldest_id}"))?;
        let newest_commit = self
            .repo
            .find_commit(newest_oid)
            .with_context(|| format!("failed to load commit {newest_id}"))?;

        let base_tree = if oldest_commit.parent_count() > 0 {
            Some(
                oldest_commit
                    .parent(0)
                    .context("failed to load oldest selected commit parent")?
                    .tree()
                    .context("failed to load oldest selected commit parent tree")?,
            )
        } else {
            None
        };
        let head_tree = newest_commit
            .tree()
            .context("failed to load newest commit tree")?;

        let mut opts = DiffOptions::new();
        opts.context_lines(3);
        let diff = self
            .repo
            .diff_tree_to_tree(base_tree.as_ref(), Some(&head_tree), Some(&mut opts))
            .with_context(|| {
                format!("failed to diff selected commit span {oldest_id}..{newest_id}")
            })?;

        let source = DiffSourceMeta {
            commit_id: newest_id.clone(),
            commit_short: if ordered_commit_ids.len() > 1 {
                String::new()
            } else {
                short_id(newest_id)
            },
            commit_summary: aggregate_commit_summary(
                &oldest_commit,
                &newest_commit,
                ordered_commit_ids.len(),
            ),
            commit_timestamp: newest_commit.time().seconds(),
        };

        let mut files = BTreeMap::<String, FilePatch>::new();
        append_diff_files(&mut files, &diff, &source)
            .context("failed to iterate selected commit span diff")?;

        Ok(AggregatedDiff { files })
    }

    pub fn aggregate_uncommitted(&self) -> anyhow::Result<AggregatedDiff> {
        let head_tree = self
            .repo
            .head()
            .ok()
            .and_then(|head| head.peel_to_tree().ok());

        let mut opts = DiffOptions::new();
        opts.context_lines(3)
            .include_untracked(true)
            .show_untracked_content(true)
            .recurse_untracked_dirs(true);
        let diff = self
            .repo
            .diff_tree_to_workdir_with_index(head_tree.as_ref(), Some(&mut opts))
            .context("failed to diff uncommitted worktree/index changes")?;

        let mut files = BTreeMap::<String, FilePatch>::new();
        let source = DiffSourceMeta {
            commit_id: UNCOMMITTED_COMMIT_ID.to_owned(),
            commit_short: UNCOMMITTED_COMMIT_SHORT.to_owned(),
            commit_summary: UNCOMMITTED_COMMIT_SUMMARY.to_owned(),
            commit_timestamp: Utc::now().timestamp(),
        };
        append_diff_files(&mut files, &diff, &source)
            .context("failed to iterate uncommitted diff")?;

        Ok(AggregatedDiff { files })
    }

    pub fn commits_affecting_selection(
        &self,
        ordered_commit_ids: &[String],
        file_path: &str,
        selected_lines: &[String],
    ) -> anyhow::Result<BTreeSet<String>> {
        if ordered_commit_ids.is_empty() {
            return Ok(BTreeSet::new());
        }

        let selected_signatures = selection_line_signatures(selected_lines);
        let mut affecting = BTreeSet::new();

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

            let source = DiffSourceMeta {
                commit_id: commit_id.clone(),
                commit_short: short_id(commit_id),
                commit_summary: commit
                    .summary()
                    .map(str::to_owned)
                    .unwrap_or_else(|| "(no summary)".to_owned()),
                commit_timestamp: commit.time().seconds(),
            };
            let mut files = BTreeMap::<String, FilePatch>::new();
            append_diff_files(&mut files, &diff, &source)
                .with_context(|| format!("failed to iterate diff for commit {commit_id}"))?;
            let Some(patch) = files.remove(file_path) else {
                continue;
            };

            if selected_signatures.is_empty() {
                affecting.insert(commit_id.clone());
                continue;
            }

            let commit_signatures = patch_line_signatures(&patch);
            if selected_signatures
                .iter()
                .any(|signature| commit_signatures.contains(signature))
            {
                affecting.insert(commit_id.clone());
            }
        }

        Ok(affecting)
    }
}

struct DiffSourceMeta {
    commit_id: String,
    commit_short: String,
    commit_summary: String,
    commit_timestamp: i64,
}

fn append_diff_files(
    files: &mut BTreeMap<String, FilePatch>,
    diff: &git2::Diff<'_>,
    source: &DiffSourceMeta,
) -> anyhow::Result<()> {
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
                commit_id: source.commit_id.clone(),
                commit_short: source.commit_short.clone(),
                commit_summary: source.commit_summary.clone(),
                commit_timestamp: source.commit_timestamp,
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
                    commit_id: source.commit_id.clone(),
                    commit_short: source.commit_short.clone(),
                    commit_summary: source.commit_summary.clone(),
                    commit_timestamp: source.commit_timestamp,
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
    )?;

    let mut commit_patches = commit_patches.into_inner();
    for path in touched_paths.into_inner() {
        if hunked_paths.borrow().contains(&path) {
            continue;
        }
        commit_patches.entry(path.clone()).or_default().push(Hunk {
            commit_id: source.commit_id.clone(),
            commit_short: source.commit_short.clone(),
            commit_summary: source.commit_summary.clone(),
            commit_timestamp: source.commit_timestamp,
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

    Ok(())
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

fn selection_line_signatures(lines: &[String]) -> BTreeSet<String> {
    lines
        .iter()
        .filter_map(|line| match line.chars().next() {
            Some('+') | Some('-') | Some('~') => Some(line.clone()),
            _ => None,
        })
        .collect()
}

fn patch_line_signatures(patch: &FilePatch) -> BTreeSet<String> {
    patch
        .hunks
        .iter()
        .flat_map(|hunk| hunk.lines.iter())
        .filter_map(|line| {
            let prefix = match line.kind {
                DiffLineKind::Add => '+',
                DiffLineKind::Remove => '-',
                DiffLineKind::Meta => '~',
                DiffLineKind::Context => return None,
            };
            Some(format!("{prefix}{}", line.text))
        })
        .collect()
}

fn aggregate_commit_summary(
    oldest_commit: &git2::Commit<'_>,
    newest_commit: &git2::Commit<'_>,
    commit_count: usize,
) -> String {
    if commit_count <= 1 {
        return newest_commit
            .summary()
            .map(str::to_owned)
            .unwrap_or_else(|| "(no summary)".to_owned());
    }

    let oldest = short_id(&oldest_commit.id().to_string());
    let newest = short_id(&newest_commit.id().to_string());
    format!("selection net changes ({oldest}..{newest})")
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
