use super::super::*;

/// Applies status transitions to selected commits and persists review state/report side effects.
pub(in crate::app) fn set_status_for_ids(
    app: &mut App,
    ids: &BTreeSet<String>,
    status: ReviewStatus,
) {
    app.flush_pending_selection_rebuild();
    app.deps.store.set_many_status(
        &mut app.domain.review_state,
        ids.iter().cloned(),
        status,
        app.deps.git.branch_name(),
    );

    apply_status_transition(&mut app.domain.commits, ids, status);
    app.sync_commit_cursor_for_filters(None, app.ui.commit_ui.list_state.selected());

    let save_result = app.deps.store.save(&app.domain.review_state);
    let mut status_message = if let Err(err) = save_result {
        format!("failed to persist status change: {err:#}")
    } else {
        format!("{} commit(s) -> {}", ids.len(), status.as_str())
    };
    let hidden_selected =
        selected_rows_hidden_by_status_filter(&app.domain.commits, app.ui.commit_ui.status_filter);
    if hidden_selected > 0 {
        status_message.push_str(&format!(", {hidden_selected} selected hidden by filter"));
    }

    if status != ReviewStatus::Unreviewed {
        app.ui.commit_ui.visual_anchor = None;
    }
    if let Err(err) = app.sync_comment_report() {
        status_message.push_str(&format!(", review tasks sync failed: {err:#}"));
    }
    app.runtime.status = status_message;
}
