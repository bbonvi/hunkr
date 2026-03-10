use std::{
    cell::RefCell,
    collections::{BTreeMap, BTreeSet},
    fs,
    path::{Path, PathBuf},
    process::Command,
    sync::Arc,
    time::UNIX_EPOCH,
};

use anyhow::{Context, anyhow};
use chrono::Utc;
use git2::{BranchType, DiffOptions, ErrorCode, Oid, Repository, Sort};

use crate::config::{DEFAULT_DIFF_CONTEXT_LINES, DEFAULT_DIFF_HUNK_MERGE_DISTANCE_LINES};
use crate::model::{
    AggregatedDiff, CommitDecoration, CommitDecorationKind, CommitInfo, DiffLineKind,
    FileChangeKind, FileChangeSummary, FilePatch, Hunk, HunkLine, UNCOMMITTED_COMMIT_ID,
    UNCOMMITTED_COMMIT_SHORT, UNCOMMITTED_COMMIT_SUMMARY,
};
use crate::store::PROJECT_DATA_DIR;

/// Read-only git access tailored for multi-commit review workflows.
pub struct GitService {
    repo: Repository,
    root: PathBuf,
    main_root: PathBuf,
    branch: String,
}

/// One worktree discovered through `git worktree list --porcelain -z`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorktreeInfo {
    pub path: PathBuf,
    pub head: String,
    pub latest_commit_ts: Option<i64>,
    pub branch: Option<String>,
    pub locked_reason: Option<String>,
    pub prunable_reason: Option<String>,
}

impl GitService {
    pub fn open_current() -> anyhow::Result<Self> {
        Self::open_at(Path::new("."))
    }

    pub fn open_at(path: &Path) -> anyhow::Result<Self> {
        let repo = match Repository::discover(path) {
            Ok(repo) => repo,
            Err(err) if err.code() == ErrorCode::NotFound => {
                return Err(anyhow!(
                    "no git repository found at or above {}",
                    path.display()
                ));
            }
            Err(err) => return Err(anyhow!(err).context("failed to discover git repository")),
        };
        let root = repo
            .workdir()
            .map(Path::to_path_buf)
            .ok_or_else(|| anyhow!("repository is bare; workdir is required"))?;
        let main_root = repo
            .commondir()
            .parent()
            .map(Path::to_path_buf)
            .unwrap_or_else(|| root.clone());
        let branch = current_branch_name(&repo)?;
        Ok(Self {
            repo,
            root,
            main_root,
            branch,
        })
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Returns the repository-local ignore file shared by the current worktree family.
    pub fn local_exclude_path(&self) -> PathBuf {
        self.repo.commondir().join("info").join("exclude")
    }

    /// Reports whether Git currently treats the given worktree-relative path as ignored.
    pub fn path_is_ignored(&self, path: &Path) -> anyhow::Result<bool> {
        let relative = path.strip_prefix(&self.root).unwrap_or(path);
        self.repo
            .status_should_ignore(relative)
            .map_err(|err| anyhow!(err).context("failed to evaluate git ignore rules"))
    }

    pub fn branch_name(&self) -> &str {
        &self.branch
    }

    pub fn list_worktrees(&self) -> anyhow::Result<Vec<WorktreeInfo>> {
        let output = Command::new("git")
            .arg("-C")
            .arg(&self.root)
            .args(["worktree", "list", "--porcelain", "-z"])
            .output()
            .context("failed to run `git worktree list --porcelain -z`")?;
        if !output.status.success() {
            return Err(anyhow!(
                "`git worktree list --porcelain -z` failed: {}",
                String::from_utf8_lossy(&output.stderr).trim()
            ));
        }
        let mut items = parse_worktree_list_porcelain(&output.stdout)?;
        for entry in &mut items {
            entry.latest_commit_ts = self.commit_timestamp_for_head(&entry.head);
        }
        sort_worktrees(&mut items, &self.main_root);
        Ok(items)
    }

    fn commit_timestamp_for_head(&self, head: &str) -> Option<i64> {
        let oid = Oid::from_str(head).ok()?;
        let commit = self.repo.find_commit(oid).ok()?;
        Some(commit.time().seconds())
    }

    pub fn load_first_parent_history(&self, max: usize) -> anyhow::Result<Vec<CommitInfo>> {
        let mut items = Vec::new();
        let mut current = self
            .repo
            .head()
            .context("failed to resolve HEAD")?
            .peel_to_commit()?;
        let unpushed = self.default_unpushed_commit_ids()?;
        let decorations = self.commit_decorations()?;

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
                decorations: decorations.get(&id).cloned().unwrap_or_default(),
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
        self.aggregate_for_commits_with_options(
            ordered_commit_ids,
            DEFAULT_DIFF_CONTEXT_LINES,
            DEFAULT_DIFF_HUNK_MERGE_DISTANCE_LINES,
        )
    }

    pub fn aggregate_for_commits_with_options(
        &self,
        ordered_commit_ids: &[String],
        context_lines: u32,
        merge_distance_lines: u32,
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

        let mut opts = diff_options(context_lines, merge_distance_lines);
        let mut diff = self
            .repo
            .diff_tree_to_tree(base_tree.as_ref(), Some(&head_tree), Some(&mut opts))
            .with_context(|| {
                format!("failed to diff selected commit span {oldest_id}..{newest_id}")
            })?;
        detect_renames_and_copies(&mut diff)?;

        let source = DiffSourceMeta {
            commit_id: Arc::from(newest_id.as_str()),
            commit_short: if ordered_commit_ids.len() > 1 {
                Arc::from("")
            } else {
                Arc::from(short_id(newest_id))
            },
            commit_summary: Arc::from(aggregate_commit_summary(
                &oldest_commit,
                &newest_commit,
                ordered_commit_ids.len(),
            )),
            commit_timestamp: newest_commit.time().seconds(),
        };

        let mut files = BTreeMap::<String, FilePatch>::new();
        let mut file_changes = BTreeMap::<String, FileChangeSummary>::new();
        append_diff_files(&mut files, &mut file_changes, &diff, &source)
            .context("failed to iterate selected commit span diff")?;

        Ok(AggregatedDiff {
            files,
            file_changes,
        })
    }

    pub fn aggregate_uncommitted(&self) -> anyhow::Result<AggregatedDiff> {
        self.aggregate_uncommitted_with_options(
            DEFAULT_DIFF_CONTEXT_LINES,
            DEFAULT_DIFF_HUNK_MERGE_DISTANCE_LINES,
        )
    }

    pub fn aggregate_uncommitted_with_options(
        &self,
        context_lines: u32,
        merge_distance_lines: u32,
    ) -> anyhow::Result<AggregatedDiff> {
        let mut diff = self.uncommitted_diff_with_options(context_lines, merge_distance_lines)?;
        detect_renames_and_copies(&mut diff)?;

        let mut files = BTreeMap::<String, FilePatch>::new();
        let mut file_changes = BTreeMap::<String, FileChangeSummary>::new();
        let source = DiffSourceMeta {
            commit_id: Arc::from(UNCOMMITTED_COMMIT_ID),
            commit_short: Arc::from(UNCOMMITTED_COMMIT_SHORT),
            commit_summary: Arc::from(UNCOMMITTED_COMMIT_SUMMARY),
            commit_timestamp: Utc::now().timestamp(),
        };
        append_diff_files(&mut files, &mut file_changes, &diff, &source)
            .context("failed to iterate uncommitted diff")?;

        Ok(AggregatedDiff {
            files,
            file_changes,
        })
    }

    /// Returns the number of changed files in the synthetic uncommitted draft.
    pub fn uncommitted_file_count(&self) -> anyhow::Result<usize> {
        let diff = self.uncommitted_diff()?;
        Ok(diff
            .deltas()
            .filter(|delta| {
                !is_internal_project_data_delta(delta.new_file().path(), delta.old_file().path())
            })
            .count())
    }

    pub fn commits_affecting_selection(
        &self,
        ordered_commit_ids: &[String],
        file_path: &str,
        selected_lines: &[String],
    ) -> anyhow::Result<BTreeSet<String>> {
        self.commits_affecting_selection_with_options(
            ordered_commit_ids,
            file_path,
            selected_lines,
            DEFAULT_DIFF_CONTEXT_LINES,
            DEFAULT_DIFF_HUNK_MERGE_DISTANCE_LINES,
        )
    }

    pub fn commits_affecting_selection_with_options(
        &self,
        ordered_commit_ids: &[String],
        file_path: &str,
        selected_lines: &[String],
        context_lines: u32,
        merge_distance_lines: u32,
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

            let mut opts = diff_options(context_lines, merge_distance_lines);
            let mut diff = self
                .repo
                .diff_tree_to_tree(parent_tree.as_ref(), Some(&current_tree), Some(&mut opts))
                .with_context(|| format!("failed to diff commit {commit_id}"))?;
            detect_renames_and_copies(&mut diff)?;

            let source = DiffSourceMeta {
                commit_id: Arc::from(commit_id.as_str()),
                commit_short: Arc::from(short_id(commit_id)),
                commit_summary: Arc::from(
                    commit
                        .summary()
                        .map(str::to_owned)
                        .unwrap_or_else(|| "(no summary)".to_owned()),
                ),
                commit_timestamp: commit.time().seconds(),
            };
            let mut files = BTreeMap::<String, FilePatch>::new();
            let mut file_changes = BTreeMap::<String, FileChangeSummary>::new();
            append_diff_files(&mut files, &mut file_changes, &diff, &source)
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

impl GitService {
    /// Builds `git log --decorate`-like labels for commits visible via references.
    fn commit_decorations(&self) -> anyhow::Result<BTreeMap<String, Vec<CommitDecoration>>> {
        let mut decorations = BTreeMap::<String, Vec<CommitDecoration>>::new();

        let head = self.repo.head().context("failed to resolve HEAD")?;
        if let Some(head_oid) = head.target() {
            let head_label = if head.is_branch() {
                let branch = head.shorthand().unwrap_or("HEAD");
                format!("{branch}*")
            } else {
                "HEAD".to_owned()
            };
            push_decoration(
                &mut decorations,
                head_oid,
                CommitDecoration {
                    kind: CommitDecorationKind::Head,
                    label: head_label,
                },
            );
        }

        for reference in self
            .repo
            .references()
            .context("failed to iterate references")?
        {
            let reference = reference.context("failed to read git reference")?;
            let Some(name) = reference.name() else {
                continue;
            };
            let Some((kind, label)) = decoration_from_ref_name(name) else {
                continue;
            };
            let Ok(commit) = reference.peel_to_commit() else {
                continue;
            };
            push_decoration(
                &mut decorations,
                commit.id(),
                CommitDecoration { kind, label },
            );
        }

        for refs in decorations.values_mut() {
            refs.sort_by(|left, right| {
                left.kind
                    .cmp(&right.kind)
                    .then_with(|| left.label.cmp(&right.label))
            });
        }
        Ok(decorations)
    }

    fn uncommitted_diff(&self) -> anyhow::Result<git2::Diff<'_>> {
        self.uncommitted_diff_with_options(
            DEFAULT_DIFF_CONTEXT_LINES,
            DEFAULT_DIFF_HUNK_MERGE_DISTANCE_LINES,
        )
    }

    fn uncommitted_diff_with_options(
        &self,
        context_lines: u32,
        merge_distance_lines: u32,
    ) -> anyhow::Result<git2::Diff<'_>> {
        let head_tree = self
            .repo
            .head()
            .ok()
            .and_then(|head| head.peel_to_tree().ok());

        let mut opts = diff_options(context_lines, merge_distance_lines);
        opts.include_untracked(true)
            .show_untracked_content(true)
            .recurse_untracked_dirs(true);
        self.repo
            .diff_tree_to_workdir_with_index(head_tree.as_ref(), Some(&mut opts))
            .context("failed to diff uncommitted worktree/index changes")
    }
}

fn diff_options(context_lines: u32, merge_distance_lines: u32) -> DiffOptions {
    let mut opts = DiffOptions::new();
    opts.context_lines(context_lines)
        .interhunk_lines(merge_distance_lines);
    opts
}

struct DiffSourceMeta {
    commit_id: Arc<str>,
    commit_short: Arc<str>,
    commit_summary: Arc<str>,
    commit_timestamp: i64,
}

fn append_diff_files(
    files: &mut BTreeMap<String, FilePatch>,
    file_changes: &mut BTreeMap<String, FileChangeSummary>,
    diff: &git2::Diff<'_>,
    source: &DiffSourceMeta,
) -> anyhow::Result<()> {
    let current_path = RefCell::new(None::<String>);
    let current_hunk_index = RefCell::new(None::<usize>);
    let touched_paths = RefCell::new(BTreeSet::<String>::new());
    let hunked_paths = RefCell::new(BTreeSet::<String>::new());
    let commit_patches = RefCell::new(BTreeMap::<String, Vec<Hunk>>::new());
    let path_changes = RefCell::new(BTreeMap::<String, FileChangeSummary>::new());

    diff.foreach(
        &mut |delta, _| {
            let new_path = delta.new_file().path().map(path_to_string);
            let old_path = delta.old_file().path().map(path_to_string);
            if is_internal_project_data_delta(delta.new_file().path(), delta.old_file().path()) {
                *current_path.borrow_mut() = None;
                *current_hunk_index.borrow_mut() = None;
                return true;
            }
            let path = new_path
                .as_ref()
                .or(old_path.as_ref())
                .cloned()
                .unwrap_or_else(|| "(unknown)".to_owned());
            *current_path.borrow_mut() = Some(path.clone());
            *current_hunk_index.borrow_mut() = None;
            touched_paths.borrow_mut().insert(path.clone());
            commit_patches.borrow_mut().entry(path.clone()).or_default();
            let summary = file_change_summary_from_delta(delta, old_path.clone());
            path_changes
                .borrow_mut()
                .entry(path)
                .and_modify(|current| merge_delta_file_change_summary(current, &summary))
                .or_insert(summary);
            true
        },
        None,
        Some(&mut |_delta, hunk| {
            let Some(path) = current_path.borrow().clone() else {
                return true;
            };
            let mut patches = commit_patches.borrow_mut();
            let file = patches.entry(path.clone()).or_default();
            file.push(Hunk {
                commit_id: source.commit_id.clone(),
                commit_short: source.commit_short.clone(),
                commit_summary: source.commit_summary.clone(),
                commit_timestamp: source.commit_timestamp,
                header: Arc::from(
                    String::from_utf8_lossy(hunk.header())
                        .trim_end_matches('\n')
                        .to_owned(),
                ),
                old_start: hunk.old_start(),
                new_start: hunk.new_start(),
                lines: Vec::new(),
            });
            *current_hunk_index.borrow_mut() = Some(file.len() - 1);
            hunked_paths.borrow_mut().insert(path);
            true
        }),
        Some(&mut |_delta, _hunk, line| {
            let Some(path) = current_path.borrow().clone() else {
                return true;
            };
            let mut patches = commit_patches.borrow_mut();
            let file = patches.entry(path.clone()).or_default();
            if current_hunk_index.borrow().is_none() {
                file.push(Hunk {
                    commit_id: source.commit_id.clone(),
                    commit_short: source.commit_short.clone(),
                    commit_summary: source.commit_summary.clone(),
                    commit_timestamp: source.commit_timestamp,
                    header: Arc::from("@@ -0,0 +0,0 @@"),
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
            if let Some(change) = path_changes.borrow_mut().get_mut(&path) {
                match kind {
                    DiffLineKind::Add => change.additions = change.additions.saturating_add(1),
                    DiffLineKind::Remove => change.deletions = change.deletions.saturating_add(1),
                    DiffLineKind::Context | DiffLineKind::Meta => {}
                }
            }
            let payload = String::from_utf8_lossy(line.content())
                .trim_end_matches('\n')
                .to_owned();
            let mut text = String::with_capacity(payload.len().saturating_add(1));
            text.push(match kind {
                DiffLineKind::Add => '+',
                DiffLineKind::Remove => '-',
                DiffLineKind::Context => ' ',
                DiffLineKind::Meta => '~',
            });
            text.push_str(&crate::text_sanitize::sanitize_terminal_text(&payload));
            let hunk = file
                .get_mut(current_hunk_index.borrow().expect("hunk index set"))
                .expect("hunk exists");
            hunk.lines.push(HunkLine {
                kind,
                text: Arc::from(text),
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
            header: Arc::from("@@ binary @@"),
            old_start: 0,
            new_start: 0,
            lines: vec![HunkLine {
                kind: DiffLineKind::Meta,
                text: Arc::from("~[binary or metadata-only change]"),
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
    for (path, change) in path_changes.into_inner() {
        file_changes.insert(path, change);
    }

    Ok(())
}

/// Enables rename/copy detection so file metadata and badges can show canonical git status types.
fn detect_renames_and_copies(diff: &mut git2::Diff<'_>) -> anyhow::Result<()> {
    let mut opts = git2::DiffFindOptions::new();
    opts.renames(true).copies(true).all(true);
    diff.find_similar(Some(&mut opts))
        .context("failed to detect renames/copies")
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

fn is_internal_project_data_path(path: &Path) -> bool {
    path.starts_with(PROJECT_DATA_DIR)
}

fn is_internal_project_data_delta(new_path: Option<&Path>, old_path: Option<&Path>) -> bool {
    new_path.is_some_and(is_internal_project_data_path)
        || old_path.is_some_and(is_internal_project_data_path)
}

fn short_id(id: &str) -> String {
    id.chars().take(7).collect()
}

fn push_decoration(
    decorations: &mut BTreeMap<String, Vec<CommitDecoration>>,
    oid: Oid,
    decoration: CommitDecoration,
) {
    let items = decorations.entry(oid.to_string()).or_default();
    if items
        .iter()
        .any(|existing| existing.label == decoration.label)
    {
        return;
    }
    items.push(decoration);
}

fn decoration_from_ref_name(name: &str) -> Option<(CommitDecorationKind, String)> {
    if let Some(branch) = name.strip_prefix("refs/heads/") {
        return Some((CommitDecorationKind::LocalBranch, branch.to_owned()));
    }
    if let Some(remote_branch) = name.strip_prefix("refs/remotes/") {
        if remote_branch.ends_with("/HEAD") {
            return None;
        }
        return Some((CommitDecorationKind::RemoteBranch, remote_branch.to_owned()));
    }
    if let Some(tag) = name.strip_prefix("refs/tags/") {
        return Some((CommitDecorationKind::Tag, tag.to_owned()));
    }
    None
}

fn file_change_summary_from_delta(
    delta: git2::DiffDelta<'_>,
    rename_from: Option<String>,
) -> FileChangeSummary {
    let kind = match delta.status() {
        git2::Delta::Added => FileChangeKind::Added,
        git2::Delta::Deleted => FileChangeKind::Deleted,
        git2::Delta::Modified => FileChangeKind::Modified,
        git2::Delta::Renamed => FileChangeKind::Renamed,
        git2::Delta::Copied => FileChangeKind::Copied,
        git2::Delta::Typechange => FileChangeKind::TypeChanged,
        git2::Delta::Untracked => FileChangeKind::Untracked,
        git2::Delta::Conflicted => FileChangeKind::Unmerged,
        _ => FileChangeKind::Unknown,
    };

    let old_path = if matches!(kind, FileChangeKind::Renamed | FileChangeKind::Copied) {
        rename_from
    } else {
        None
    };

    FileChangeSummary {
        kind,
        old_path,
        additions: 0,
        deletions: 0,
    }
}

fn merge_delta_file_change_summary(current: &mut FileChangeSummary, next: &FileChangeSummary) {
    if current.old_path.is_none() {
        current.old_path = next.old_path.clone();
    }
    current.kind = merge_delta_change_kind(current.kind, next.kind);
}

fn merge_delta_change_kind(current: FileChangeKind, next: FileChangeKind) -> FileChangeKind {
    use FileChangeKind::*;
    match (current, next) {
        (_, Unknown) => current,
        (Unknown, _) => next,
        (Added, Deleted) | (Deleted, Added) => Modified,
        // Preserve richer classifications when later steps emit a plain modified marker.
        (Renamed | Copied | TypeChanged, Modified) => current,
        (_, next) => next,
    }
}

#[derive(Debug, Default)]
struct WorktreeInfoBuilder {
    path: Option<PathBuf>,
    head: Option<String>,
    branch: Option<String>,
    locked_reason: Option<String>,
    prunable_reason: Option<String>,
}

impl WorktreeInfoBuilder {
    fn finish(self) -> anyhow::Result<WorktreeInfo> {
        let path = self
            .path
            .ok_or_else(|| anyhow!("malformed `git worktree` output: missing worktree path"))?;
        Ok(WorktreeInfo {
            path,
            head: self.head.unwrap_or_default(),
            latest_commit_ts: None,
            branch: self.branch,
            locked_reason: self.locked_reason,
            prunable_reason: self.prunable_reason,
        })
    }
}

fn parse_worktree_list_porcelain(payload: &[u8]) -> anyhow::Result<Vec<WorktreeInfo>> {
    let mut current = WorktreeInfoBuilder::default();
    let mut items = Vec::<WorktreeInfo>::new();

    for raw in payload.split(|byte| *byte == 0) {
        if raw.is_empty() {
            if current.path.is_some() {
                items.push(current.finish()?);
                current = WorktreeInfoBuilder::default();
            }
            continue;
        }

        let line = String::from_utf8_lossy(raw);
        if let Some(path) = line.strip_prefix("worktree ") {
            if current.path.is_some() {
                items.push(current.finish()?);
                current = WorktreeInfoBuilder::default();
            }
            current.path = Some(PathBuf::from(path));
            continue;
        }

        if current.path.is_none() {
            return Err(anyhow!(
                "malformed `git worktree` output: field before worktree path"
            ));
        }

        if let Some(head) = line.strip_prefix("HEAD ") {
            current.head = Some(head.to_owned());
            continue;
        }
        if let Some(branch) = line.strip_prefix("branch ") {
            let branch = branch.strip_prefix("refs/heads/").unwrap_or(branch);
            current.branch = Some(branch.to_owned());
            continue;
        }
        if line == "detached" {
            current.branch = None;
            continue;
        }
        if let Some(value) = line.strip_prefix("locked") {
            current.locked_reason = parse_optional_worktree_message(value);
            continue;
        }
        if let Some(value) = line.strip_prefix("prunable") {
            current.prunable_reason = parse_optional_worktree_message(value);
            continue;
        }
    }

    if current.path.is_some() {
        items.push(current.finish()?);
    }

    Ok(items)
}

fn parse_optional_worktree_message(field_suffix: &str) -> Option<String> {
    let message = field_suffix.trim_start();
    (!message.is_empty()).then(|| message.to_owned())
}

fn sort_worktrees(items: &mut [WorktreeInfo], main_root: &Path) {
    sort_worktrees_with(items, main_root, worktree_timestamp);
}

fn sort_worktrees_with<F>(items: &mut [WorktreeInfo], main_root: &Path, mut timestamp_of: F)
where
    F: FnMut(&Path) -> Option<i64>,
{
    items.sort_by(|left, right| {
        let left_main = left.path == main_root;
        let right_main = right.path == main_root;
        if left_main != right_main {
            return right_main.cmp(&left_main);
        }

        let left_ts = left.latest_commit_ts.or_else(|| timestamp_of(&left.path));
        let right_ts = right.latest_commit_ts.or_else(|| timestamp_of(&right.path));
        right_ts
            .cmp(&left_ts)
            .then_with(|| left.path.cmp(&right.path))
    });
}

fn worktree_timestamp(path: &Path) -> Option<i64> {
    let git_path = path.join(".git");
    let metadata = fs::metadata(&git_path)
        .or_else(|_| fs::metadata(path))
        .ok()?;
    let modified = metadata.modified().ok()?;
    modified
        .duration_since(UNIX_EPOCH)
        .ok()
        .and_then(|delta| i64::try_from(delta.as_secs()).ok())
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
        .filter_map(|line| match line.kind {
            DiffLineKind::Add | DiffLineKind::Remove | DiffLineKind::Meta => {
                Some(line.text.as_ref().to_owned())
            }
            DiffLineKind::Context => None,
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
mod tests;
