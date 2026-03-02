use std::collections::BTreeSet;

use super::contracts::PaneViewModelBuilder;
use super::snapshot::{CommitPaneSnapshot, FilePaneSnapshot};
use crate::app::{CommitRow, CommitStatusFilter, TreeRow};

/// Owned view data for the files pane renderer.
pub(in crate::app) struct FilePaneViewModel<'a> {
    pub file_rows: &'a [TreeRow],
    pub changed_files: usize,
    pub shown_files: usize,
    pub search_display: String,
    pub search_enabled: bool,
}

/// Inputs required to build a file pane view-model.
pub(in crate::app) struct FilePaneVmInput<'a> {
    pub files_search_mode: bool,
    pub file_query: &'a str,
    pub visible_rows: &'a [TreeRow],
    pub changed_files: usize,
}

/// Builds file pane model from a pure input contract.
pub(in crate::app) fn build_file_pane_view_model(
    input: FilePaneVmInput<'_>,
) -> FilePaneViewModel<'_> {
    let query = input.file_query.trim();
    let search_display = if !query.is_empty() {
        format!("/{query}")
    } else if input.files_search_mode {
        "/".to_owned()
    } else {
        "off".to_owned()
    };
    let shown_files = input
        .visible_rows
        .iter()
        .filter(|row| row.selectable)
        .count();

    FilePaneViewModel {
        shown_files,
        changed_files: input.changed_files,
        search_enabled: input.files_search_mode || !query.is_empty(),
        search_display,
        file_rows: input.visible_rows,
    }
}

/// Owned view data for the commits pane renderer.
pub(in crate::app) struct CommitPaneViewModel<'a> {
    pub commits: &'a [CommitRow],
    pub comment_badge_commit_ids: &'a BTreeSet<String>,
    pub selected_total: usize,
    pub shown_commits: usize,
    pub total_commits: usize,
    pub status_counts: (usize, usize, usize),
    pub status_filter: CommitStatusFilter,
    pub search_display: String,
    pub search_enabled: bool,
}

/// Inputs required to build a commit pane view-model.
pub(in crate::app) struct CommitPaneVmInput<'a> {
    pub commits_search_mode: bool,
    pub commit_query: &'a str,
    pub visible_commits: &'a [CommitRow],
    pub comment_badge_commit_ids: &'a BTreeSet<String>,
    pub selected_total: usize,
    pub total_commits: usize,
    pub status_counts: (usize, usize, usize),
    pub status_filter: CommitStatusFilter,
}

/// Builds commit pane model from a pure input contract.
pub(in crate::app) fn build_commit_pane_view_model(
    input: CommitPaneVmInput<'_>,
) -> CommitPaneViewModel<'_> {
    let query = input.commit_query.trim();
    let search_display = if !query.is_empty() {
        format!("/{query}")
    } else if input.commits_search_mode {
        "/".to_owned()
    } else {
        "off".to_owned()
    };
    let shown_commits = input.visible_commits.len();

    CommitPaneViewModel {
        shown_commits,
        total_commits: input.total_commits,
        status_counts: input.status_counts,
        status_filter: input.status_filter,
        search_enabled: input.commits_search_mode || !query.is_empty(),
        search_display,
        commits: input.visible_commits,
        comment_badge_commit_ids: input.comment_badge_commit_ids,
        selected_total: input.selected_total,
    }
}

/// Commit ids that can render inline comment rows in a per-commit diff view.
pub(in crate::app) fn comment_badge_commit_ids_from_comments(
    comments: &[crate::model::ReviewComment],
) -> BTreeSet<String> {
    comments
        .iter()
        .map(|comment| comment.target.end.commit_id.clone())
        .collect()
}

pub(in crate::app) struct FilePaneVmBuilder;

impl PaneViewModelBuilder<FilePaneSnapshot> for FilePaneVmBuilder {
    type Output<'a>
        = FilePaneViewModel<'a>
    where
        FilePaneSnapshot: 'a;

    fn build<'a>(&self, snapshot: &'a FilePaneSnapshot) -> Self::Output<'a> {
        build_file_pane_view_model(FilePaneVmInput {
            files_search_mode: snapshot.files_search_mode,
            file_query: &snapshot.file_query,
            visible_rows: &snapshot.visible_rows,
            changed_files: snapshot.changed_files,
        })
    }
}

pub(in crate::app) struct CommitPaneVmBuilder;

impl PaneViewModelBuilder<CommitPaneSnapshot> for CommitPaneVmBuilder {
    type Output<'a>
        = CommitPaneViewModel<'a>
    where
        CommitPaneSnapshot: 'a;

    fn build<'a>(&self, snapshot: &'a CommitPaneSnapshot) -> Self::Output<'a> {
        build_commit_pane_view_model(CommitPaneVmInput {
            commits_search_mode: snapshot.commits_search_mode,
            commit_query: &snapshot.commit_query,
            visible_commits: &snapshot.visible_commits,
            comment_badge_commit_ids: &snapshot.comment_badge_commit_ids,
            selected_total: snapshot.selected_total,
            total_commits: snapshot.total_commits,
            status_counts: snapshot.status_counts,
            status_filter: snapshot.status_filter,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::CommitDecoration;
    use crate::{
        app::TreeRow,
        model::{
            CommentAnchor, CommentTarget, CommentTargetKind, CommitInfo, ReviewComment,
            ReviewStatus,
        },
    };

    fn tree_row(path: &str, selectable: bool) -> TreeRow {
        TreeRow {
            label: path.to_owned(),
            path: Some(path.to_owned()),
            depth: 0,
            selectable,
            modified_ts: None,
            change: None,
        }
    }

    fn commit_row(id: &str, selected: bool) -> CommitRow {
        CommitRow {
            info: CommitInfo {
                id: id.to_owned(),
                short_id: id.to_owned(),
                summary: "summary".to_owned(),
                author: "dev".to_owned(),
                timestamp: 0,
                unpushed: false,
                decorations: Vec::<CommitDecoration>::new(),
            },
            selected,
            status: ReviewStatus::Unreviewed,
            is_uncommitted: false,
        }
    }

    fn anchor(commit_id: &str) -> CommentAnchor {
        CommentAnchor {
            commit_id: commit_id.to_owned(),
            commit_summary: "summary".to_owned(),
            file_path: "src/lib.rs".to_owned(),
            hunk_header: "@@ -1 +1 @@".to_owned(),
            old_lineno: Some(1),
            new_lineno: Some(1),
        }
    }

    fn comment(id: u64, commit_ids: &[&str]) -> ReviewComment {
        ReviewComment {
            id,
            target: CommentTarget {
                kind: CommentTargetKind::Hunk,
                start: anchor(commit_ids[0]),
                end: anchor(commit_ids[0]),
                commits: commit_ids.iter().map(|id| (*id).to_owned()).collect(),
                selected_lines: Vec::new(),
            },
            text: "note".to_owned(),
            created_at: "2026-01-01T00:00:00Z".to_owned(),
            updated_at: "2026-01-01T00:00:00Z".to_owned(),
        }
    }

    #[test]
    fn file_vm_search_display_contract() {
        let rows = vec![tree_row("a", true)];
        let off = build_file_pane_view_model(FilePaneVmInput {
            files_search_mode: false,
            file_query: "",
            visible_rows: &rows,
            changed_files: 1,
        });
        assert_eq!(off.search_display, "off");
        assert!(!off.search_enabled);

        let rows = vec![tree_row("a", true)];
        let active_empty = build_file_pane_view_model(FilePaneVmInput {
            files_search_mode: true,
            file_query: "   ",
            visible_rows: &rows,
            changed_files: 1,
        });
        assert_eq!(active_empty.search_display, "/");
        assert!(active_empty.search_enabled);

        let rows = vec![tree_row("a", true)];
        let with_query = build_file_pane_view_model(FilePaneVmInput {
            files_search_mode: false,
            file_query: " config ",
            visible_rows: &rows,
            changed_files: 1,
        });
        assert_eq!(with_query.search_display, "/config");
        assert!(with_query.search_enabled);
    }

    #[test]
    fn file_vm_counts_only_selectable_rows() {
        let rows = vec![
            tree_row("src", false),
            tree_row("src/main.rs", true),
            tree_row("src/lib.rs", true),
        ];
        let vm = build_file_pane_view_model(FilePaneVmInput {
            files_search_mode: false,
            file_query: "",
            visible_rows: &rows,
            changed_files: 3,
        });

        assert_eq!(vm.shown_files, 2);
        assert_eq!(vm.changed_files, 3);
    }

    #[test]
    fn commit_vm_selected_total_uses_all_commits() {
        let commits = vec![commit_row("a", true)];
        let commented = BTreeSet::new();
        let vm = build_commit_pane_view_model(CommitPaneVmInput {
            commits_search_mode: false,
            commit_query: "",
            visible_commits: &commits,
            comment_badge_commit_ids: &commented,
            selected_total: 2,
            total_commits: 3,
            status_counts: (1, 0, 0),
            status_filter: CommitStatusFilter::All,
        });

        assert_eq!(vm.shown_commits, 1);
        assert_eq!(vm.total_commits, 3);
        assert_eq!(vm.selected_total, 2);
        assert_eq!(vm.search_display, "off");
    }

    #[test]
    fn commit_vm_aggregates_comment_badge_commit_ids_from_end_anchor() {
        let ids = comment_badge_commit_ids_from_comments(&[
            comment(1, &["a", "b"]),
            comment(2, &["b", "c"]),
        ]);
        let commits = vec![commit_row("a", false)];
        let vm = build_commit_pane_view_model(CommitPaneVmInput {
            commits_search_mode: true,
            commit_query: " bug ",
            visible_commits: &commits,
            comment_badge_commit_ids: &ids,
            selected_total: 0,
            total_commits: 1,
            status_counts: (1, 0, 0),
            status_filter: CommitStatusFilter::All,
        });

        assert_eq!(vm.search_display, "/bug");
        assert!(vm.search_enabled);
        assert_eq!(vm.comment_badge_commit_ids.len(), 2);
        assert!(vm.comment_badge_commit_ids.contains("a"));
        assert!(vm.comment_badge_commit_ids.contains("b"));
        assert!(!vm.comment_badge_commit_ids.contains("c"));
    }
}
