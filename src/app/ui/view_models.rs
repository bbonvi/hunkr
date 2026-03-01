use std::collections::BTreeSet;

use super::super::*;
use super::contracts::PaneViewModelBuilder;

/// Owned view data for the files pane renderer.
pub(in crate::app) struct FilePaneViewModel {
    pub file_rows: Vec<TreeRow>,
    pub changed_files: usize,
    pub shown_files: usize,
    pub search_display: String,
    pub search_enabled: bool,
}

/// Inputs required to build a file pane view-model.
pub(in crate::app) struct FilePaneVmInput {
    pub files_search_mode: bool,
    pub file_query: String,
    pub visible_rows: Vec<TreeRow>,
    pub changed_files: usize,
}

/// Builds file pane model from a pure input contract.
pub(in crate::app) fn build_file_pane_view_model(input: FilePaneVmInput) -> FilePaneViewModel {
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
pub(in crate::app) struct CommitPaneViewModel {
    pub commits: Vec<CommitRow>,
    pub commented_commit_ids: BTreeSet<String>,
    pub selected_total: usize,
    pub shown_commits: usize,
    pub total_commits: usize,
    pub status_counts: (usize, usize, usize, usize),
    pub status_filter: CommitStatusFilter,
    pub search_display: String,
    pub search_enabled: bool,
}

/// Inputs required to build a commit pane view-model.
pub(in crate::app) struct CommitPaneVmInput {
    pub commits_search_mode: bool,
    pub commit_query: String,
    pub visible_commits: Vec<CommitRow>,
    pub commented_commit_ids: BTreeSet<String>,
    pub selected_total: usize,
    pub total_commits: usize,
    pub status_counts: (usize, usize, usize, usize),
    pub status_filter: CommitStatusFilter,
}

/// Builds commit pane model from a pure input contract.
pub(in crate::app) fn build_commit_pane_view_model(
    input: CommitPaneVmInput,
) -> CommitPaneViewModel {
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
        commented_commit_ids: input.commented_commit_ids,
        selected_total: input.selected_total,
    }
}

pub(in crate::app) fn commented_commit_ids_from_comments(
    comments: &[ReviewComment],
) -> BTreeSet<String> {
    comments
        .iter()
        .flat_map(|comment| comment.target.commits.iter().cloned())
        .collect()
}

pub(in crate::app) struct FilePaneVmBuilder;

impl PaneViewModelBuilder for FilePaneVmBuilder {
    type Output = FilePaneViewModel;

    fn build(&self, app: &App) -> Self::Output {
        let visible_rows = app
            .visible_file_indices()
            .into_iter()
            .filter_map(|idx| app.domain.file_rows.get(idx).cloned())
            .collect::<Vec<_>>();
        build_file_pane_view_model(FilePaneVmInput {
            files_search_mode: matches!(
                app.ui.preferences.input_mode,
                InputMode::ListSearch(FocusPane::Files)
            ),
            file_query: app.ui.search.file_query.clone(),
            visible_rows,
            changed_files: app.domain.aggregate.files.len(),
        })
    }
}

pub(in crate::app) struct CommitPaneVmBuilder;

impl PaneViewModelBuilder for CommitPaneVmBuilder {
    type Output = CommitPaneViewModel;

    fn build(&self, app: &App) -> Self::Output {
        let visible_commits = app
            .visible_commit_indices()
            .into_iter()
            .filter_map(|idx| app.domain.commits.get(idx).cloned())
            .collect::<Vec<_>>();
        build_commit_pane_view_model(CommitPaneVmInput {
            commits_search_mode: matches!(
                app.ui.preferences.input_mode,
                InputMode::ListSearch(FocusPane::Commits)
            ),
            commit_query: app.ui.search.commit_query.clone(),
            visible_commits,
            commented_commit_ids: commented_commit_ids_from_comments(app.deps.comments.comments()),
            selected_total: app.domain.commits.iter().filter(|row| row.selected).count(),
            total_commits: app.domain.commits.len(),
            status_counts: app.status_counts(),
            status_filter: app.ui.commit_ui.status_filter,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::CommitDecoration;

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
        let off = build_file_pane_view_model(FilePaneVmInput {
            files_search_mode: false,
            file_query: String::new(),
            visible_rows: vec![tree_row("a", true)],
            changed_files: 1,
        });
        assert_eq!(off.search_display, "off");
        assert!(!off.search_enabled);

        let active_empty = build_file_pane_view_model(FilePaneVmInput {
            files_search_mode: true,
            file_query: "   ".to_owned(),
            visible_rows: vec![tree_row("a", true)],
            changed_files: 1,
        });
        assert_eq!(active_empty.search_display, "/");
        assert!(active_empty.search_enabled);

        let with_query = build_file_pane_view_model(FilePaneVmInput {
            files_search_mode: false,
            file_query: " config ".to_owned(),
            visible_rows: vec![tree_row("a", true)],
            changed_files: 1,
        });
        assert_eq!(with_query.search_display, "/config");
        assert!(with_query.search_enabled);
    }

    #[test]
    fn file_vm_counts_only_selectable_rows() {
        let vm = build_file_pane_view_model(FilePaneVmInput {
            files_search_mode: false,
            file_query: String::new(),
            visible_rows: vec![
                tree_row("src", false),
                tree_row("src/main.rs", true),
                tree_row("src/lib.rs", true),
            ],
            changed_files: 3,
        });

        assert_eq!(vm.shown_files, 2);
        assert_eq!(vm.changed_files, 3);
    }

    #[test]
    fn commit_vm_selected_total_uses_all_commits() {
        let vm = build_commit_pane_view_model(CommitPaneVmInput {
            commits_search_mode: false,
            commit_query: String::new(),
            visible_commits: vec![commit_row("a", true)],
            commented_commit_ids: BTreeSet::new(),
            selected_total: 2,
            total_commits: 3,
            status_counts: (1, 0, 0, 0),
            status_filter: CommitStatusFilter::All,
        });

        assert_eq!(vm.shown_commits, 1);
        assert_eq!(vm.total_commits, 3);
        assert_eq!(vm.selected_total, 2);
        assert_eq!(vm.search_display, "off");
    }

    #[test]
    fn commit_vm_aggregates_commented_commit_ids() {
        let ids =
            commented_commit_ids_from_comments(&[comment(1, &["a", "b"]), comment(2, &["b", "c"])]);
        let vm = build_commit_pane_view_model(CommitPaneVmInput {
            commits_search_mode: true,
            commit_query: " bug ".to_owned(),
            visible_commits: vec![commit_row("a", false)],
            commented_commit_ids: ids,
            selected_total: 0,
            total_commits: 1,
            status_counts: (1, 0, 0, 0),
            status_filter: CommitStatusFilter::All,
        });

        assert_eq!(vm.search_display, "/bug");
        assert!(vm.search_enabled);
        assert_eq!(vm.commented_commit_ids.len(), 3);
        assert!(vm.commented_commit_ids.contains("a"));
        assert!(vm.commented_commit_ids.contains("b"));
        assert!(vm.commented_commit_ids.contains("c"));
    }
}
