use super::super::state::format_uncommitted_summary;
use super::super::*;

/// Handles repository reload/rebuild orchestration independently from UI input/render modules.
pub(in crate::app) fn switch_repository_context(
    app: &mut App,
    target: &Path,
) -> anyhow::Result<()> {
    let reopened = GitService::open_at(target)
        .with_context(|| format!("failed to reopen repository at {}", target.display()))?;
    let branch = reopened.branch_name().to_owned();
    app.deps.git = reopened;
    app.deps.comments = CommentStore::new(app.deps.store.root_dir(), &branch)
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
