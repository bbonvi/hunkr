use crate::app::*;

/// Handles comment create/edit submission side effects and status/report synchronization.
pub(in crate::app) fn submit_comment_from_editor(app: &mut App) -> bool {
    let mut close_editor = false;
    match app.ui.preferences.input_mode {
        InputMode::CommentCreate => match app.resolve_comment_create_target() {
            Ok(Some(target)) => {
                let result = app
                    .deps
                    .comments
                    .add_comment(&target, &app.ui.comment_editor.buffer);
                match result {
                    Ok(id) => {
                        app.capture_pending_diff_view_anchor();
                        app.set_status_for_ids(&target.commits, ReviewStatus::IssueFound);
                        app.invalidate_diff_cache();
                        if let Err(err) = app.sync_comment_report() {
                            app.runtime.status = format!(
                                "Comment #{} added, but review tasks sync failed: {err:#}",
                                id
                            );
                            close_editor = true;
                        } else {
                            app.runtime.status = format!(
                                "Comment #{} added -> {} ({} commit(s) marked ISSUE_FOUND)",
                                id,
                                app.deps.comments.report_path().display(),
                                target.commits.len()
                            );
                            close_editor = true;
                        }
                    }
                    Err(err) => {
                        app.runtime.status = format!("Failed to save comment: {err:#}");
                    }
                }
            }
            Ok(None) => {
                app.runtime.status = if app.diff_selection_spans_multiple_files() {
                    "Comment range must stay within a single file".to_owned()
                } else {
                    "No hunk/line anchor at cursor or selected range".to_owned()
                };
                close_editor = true;
            }
            Err(err) => {
                app.runtime.status =
                    format!("Failed to resolve affected commits for comment: {err}");
                close_editor = true;
            }
        },
        InputMode::CommentEdit(id) => {
            match app
                .deps
                .comments
                .update_comment(id, &app.ui.comment_editor.buffer)
            {
                Ok(true) => {
                    app.capture_pending_diff_view_anchor();
                    app.invalidate_diff_cache();
                    if let Err(err) = app.sync_comment_report() {
                        app.runtime.status = format!(
                            "Comment #{} updated, but review tasks sync failed: {err:#}",
                            id
                        );
                    } else {
                        app.runtime.status = format!("Comment #{} updated", id);
                    }
                    close_editor = true;
                }
                Ok(false) => {
                    app.runtime.status = format!("Comment #{} not found", id);
                    close_editor = true;
                }
                Err(err) => {
                    app.runtime.status = format!("Failed to update comment #{}: {err:#}", id);
                }
            }
        }
        InputMode::ShellCommand
        | InputMode::WorktreeSwitch
        | InputMode::DiffSearch
        | InputMode::ListSearch(_)
        | InputMode::Normal => {}
    }
    close_editor
}
