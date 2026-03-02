use super::super::state::format_uncommitted_summary;
use crate::app::*;

/// Handles repository reload/rebuild orchestration independently from UI input/render modules.
pub(in crate::app) fn switch_repository_context(
    app: &mut App,
    target: &Path,
) -> anyhow::Result<()> {
    let reopened = app
        .deps
        .runtime_ports
        .open_git_at(target)
        .with_context(|| format!("failed to reopen repository at {}", target.display()))?;
    let branch = reopened.branch_name().to_owned();
    app.deps.git = reopened;
    app.deps.comments = app
        .deps
        .runtime_ports
        .open_comment_store(app.deps.store.root_dir(), &branch)
        .with_context(|| format!("failed to reload comments for branch {branch}"))?;
    reload_commits(app, true).context("failed to refresh commit and diff state")?;

    let now = app.now_instant();
    app.runtime.last_refresh = now;
    app.runtime.last_relative_time_redraw = now;
    app.runtime.needs_redraw = true;
    Ok(())
}

/// Reloads commit rows and selection projections from git + persisted review state.
pub(in crate::app) fn reload_commits(
    app: &mut App,
    preserve_manual_selection: bool,
) -> anyhow::Result<()> {
    let history = app.deps.git.load_first_parent_history(HISTORY_LIMIT)?;
    let prior_cursor_idx = app.ui.commit_ui.list_state.selected();
    let prior_cursor_commit_id = app.selected_commit_id();
    let prior_visual_anchor_commit_id = app
        .ui
        .commit_ui
        .visual_anchor
        .and_then(|idx| app.domain.commits.get(idx))
        .map(|row| row.info.id.clone());

    let mut old_selected = BTreeSet::new();
    if preserve_manual_selection {
        for row in &app.domain.commits {
            if row.selected {
                old_selected.insert(row.info.id.clone());
            }
        }
    }

    let mut known = BTreeSet::new();
    for row in &app.domain.commits {
        known.insert(row.info.id.clone());
    }

    app.domain.commits = history
        .into_iter()
        .map(|info| {
            let status = app
                .deps
                .store
                .commit_status(&app.domain.review_state, &info.id);
            let selected = preserve_manual_selection && old_selected.contains(&info.id);
            CommitRow {
                info,
                selected,
                status,
                is_uncommitted: false,
            }
        })
        .collect();

    let uncommitted_file_count = app.deps.git.uncommitted_file_count()?;
    let uncommitted_selected =
        preserve_manual_selection && old_selected.contains(UNCOMMITTED_COMMIT_ID);
    app.domain.commits.insert(
        0,
        CommitRow {
            info: CommitInfo {
                short_id: UNCOMMITTED_COMMIT_SHORT.to_owned(),
                id: UNCOMMITTED_COMMIT_ID.to_owned(),
                summary: format_uncommitted_summary(uncommitted_file_count),
                author: "local".to_owned(),
                timestamp: app.now_timestamp(),
                unpushed: false,
                decorations: Vec::new(),
            },
            selected: uncommitted_selected,
            status: ReviewStatus::Unreviewed,
            is_uncommitted: true,
        },
    );

    app.sync_commit_cursor_for_filters(prior_cursor_commit_id.as_deref(), prior_cursor_idx);
    app.ui.commit_ui.visual_anchor = prior_visual_anchor_commit_id
        .as_deref()
        .and_then(|commit_id| index_of_commit(&app.domain.commits, commit_id));
    if app
        .ui
        .commit_ui
        .visual_anchor
        .is_some_and(|anchor| !app.visible_commit_indices().contains(&anchor))
    {
        app.ui.commit_ui.visual_anchor = None;
    }

    let new_commits = app
        .domain
        .commits
        .iter()
        .filter(|row| {
            !row.is_uncommitted
                && !known.contains(&row.info.id)
                && row.status == ReviewStatus::Unreviewed
        })
        .count();
    if new_commits > 0 {
        let noun = if new_commits == 1 {
            "commit"
        } else {
            "commits"
        };
        app.runtime.status = format!("{new_commits} new unreviewed {noun} detected");
    }

    rebuild_selection_dependent_views(app)?;
    app.sync_comment_report()?;
    Ok(())
}

/// Rebuilds aggregate diff + file/diff projections for current commit selection.
pub(in crate::app) fn rebuild_selection_dependent_views(app: &mut App) -> anyhow::Result<()> {
    let selected_ordered = app.selected_commit_ids_oldest_first();
    let mut aggregate = if selected_ordered.is_empty() {
        AggregatedDiff::default()
    } else {
        app.deps.git.aggregate_for_commits(&selected_ordered)?
    };
    if app.uncommitted_selected() {
        merge_aggregate_diff(&mut aggregate, app.deps.git.aggregate_uncommitted()?);
    }
    let changed_paths = changed_paths_between_aggregates(&app.domain.aggregate, &aggregate);
    let aggregate_changed = !changed_paths.is_empty();

    if aggregate_changed {
        app.capture_pending_diff_view_anchor();
    }

    app.domain.aggregate = aggregate;
    app.domain.deleted_file_content_visible.retain(|path| {
        app.domain
            .aggregate
            .file_changes
            .get(path)
            .is_some_and(|change| change.kind == FileChangeKind::Deleted)
    });
    app.prune_diff_positions_for_removed_files();

    if aggregate_changed {
        app.ui
            .diff_cache
            .rendered_cache
            .retain(|(path, _), _| !changed_paths.contains(path));
        app.ui.diff_cache.rendered_key = None;
        app.ui.diff_cache.file_ranges.clear();
        app.ui.diff_cache.file_range_by_path.clear();
        app.ui.diff_ui.pending_op = None;
    }

    app.rebuild_file_tree();
    app.ensure_selected_file_exists();
    app.sync_file_cursor_for_filters();
    app.ensure_rendered_diff();
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::{
        path::{Path, PathBuf},
        process::Command,
        sync::{
            Arc,
            atomic::{AtomicUsize, Ordering},
        },
        time::Instant,
    };

    use chrono::{DateTime, Utc};
    use tempfile::TempDir;

    use crate::app::*;
    use crate::config::AppConfig;

    struct TestClock;

    impl AppClock for TestClock {
        fn now_utc(&self) -> DateTime<Utc> {
            Utc::now()
        }

        fn now_instant(&self) -> Instant {
            Instant::now()
        }
    }

    #[derive(Default)]
    struct RuntimePortCalls {
        open_git_at: AtomicUsize,
        open_comment_store: AtomicUsize,
    }

    struct TestRuntimePorts {
        calls: Arc<RuntimePortCalls>,
    }

    impl AppRuntimePorts for TestRuntimePorts {
        fn open_git_at(&self, path: &Path) -> anyhow::Result<GitService> {
            self.calls.open_git_at.fetch_add(1, Ordering::Relaxed);
            GitService::open_at(path)
        }

        fn open_comment_store(
            &self,
            store_root: &Path,
            branch: &str,
        ) -> anyhow::Result<CommentStore> {
            self.calls
                .open_comment_store
                .fetch_add(1, Ordering::Relaxed);
            CommentStore::new(store_root, branch)
        }
    }

    struct TestBootstrapPorts {
        repo_root: PathBuf,
        runtime_ports: Arc<dyn AppRuntimePorts>,
    }

    impl AppBootstrapPorts for TestBootstrapPorts {
        fn open_current_git(&self) -> anyhow::Result<GitService> {
            GitService::open_at(&self.repo_root)
        }

        fn load_config(&self) -> anyhow::Result<AppConfig> {
            Ok(AppConfig::default())
        }

        fn state_store_for_repo(&self, repo_root: &Path) -> StateStore {
            StateStore::for_project(repo_root)
        }

        fn open_comment_store(
            &self,
            store_root: &Path,
            branch: &str,
        ) -> anyhow::Result<CommentStore> {
            CommentStore::new(store_root, branch)
        }

        fn clock(&self) -> Arc<dyn AppClock> {
            Arc::new(TestClock)
        }

        fn runtime_ports(&self) -> Arc<dyn AppRuntimePorts> {
            Arc::clone(&self.runtime_ports)
        }
    }

    fn run_git(dir: &Path, args: &[&str]) {
        let output = Command::new("git")
            .args(args)
            .current_dir(dir)
            .output()
            .expect("spawn git");
        assert!(
            output.status.success(),
            "git {:?} failed: {}",
            args,
            String::from_utf8_lossy(&output.stderr)
        );
    }

    fn init_test_repo() -> TempDir {
        let tmp = tempfile::tempdir().expect("tempdir");
        let root = tmp.path();
        run_git(root, &["init", "-q"]);
        run_git(root, &["config", "user.name", "hunkr-test"]);
        run_git(root, &["config", "user.email", "hunkr-test@example.com"]);
        std::fs::write(root.join("README.md"), "init\n").expect("seed readme");
        run_git(root, &["add", "."]);
        run_git(root, &["commit", "-m", "init", "-q"]);
        tmp
    }

    #[test]
    fn switch_repository_context_uses_injected_runtime_ports() {
        let repo = init_test_repo();
        let store = StateStore::for_project(repo.path());
        store
            .save(&ReviewState::default())
            .expect("seed persisted state to bypass onboarding");

        let calls = Arc::new(RuntimePortCalls::default());
        let runtime_ports = Arc::new(TestRuntimePorts {
            calls: Arc::clone(&calls),
        });
        let ports = TestBootstrapPorts {
            repo_root: repo.path().to_path_buf(),
            runtime_ports,
        };
        let mut app = App::bootstrap_with(&ports).expect("bootstrap app");

        super::switch_repository_context(&mut app, repo.path())
            .expect("switch repository context");
        assert_eq!(calls.open_git_at.load(Ordering::Relaxed), 1);
        assert_eq!(calls.open_comment_store.load(Ordering::Relaxed), 1);
    }
}
