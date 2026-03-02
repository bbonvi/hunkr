use std::{collections::BTreeSet, path::PathBuf};

use super::view_models::commented_commit_ids_from_comments;
use crate::app::{App, CommitRow, CommitStatusFilter, FocusPane, InputMode, TreeRow};

/// Immutable snapshot consumed by render/view-model builders.
pub(in crate::app) struct AppRenderSnapshot {
    pub header: HeaderSnapshot,
    pub files: FilePaneSnapshot,
    pub commits: CommitPaneSnapshot,
    pub focused: FocusPane,
    pub nerd_fonts: bool,
    pub now_ts: i64,
}

/// Header-specific immutable snapshot.
pub(in crate::app) struct HeaderSnapshot {
    pub branch_name: String,
    pub repo_root: PathBuf,
}

/// Files-pane immutable snapshot.
pub(in crate::app) struct FilePaneSnapshot {
    pub files_search_mode: bool,
    pub file_query: String,
    pub visible_rows: Vec<TreeRow>,
    pub changed_files: usize,
}

/// Commits-pane immutable snapshot.
pub(in crate::app) struct CommitPaneSnapshot {
    pub commits_search_mode: bool,
    pub commit_query: String,
    pub visible_commits: Vec<CommitRow>,
    pub commented_commit_ids: BTreeSet<String>,
    pub selected_total: usize,
    pub total_commits: usize,
    pub status_counts: (usize, usize, usize, usize),
    pub status_filter: CommitStatusFilter,
}

impl App {
    /// Captures immutable render-time state for header and list-pane view-model builders.
    pub(in crate::app) fn capture_render_snapshot(&self) -> AppRenderSnapshot {
        let files = FilePaneSnapshot {
            files_search_mode: matches!(
                self.ui.preferences.input_mode,
                InputMode::ListSearch(FocusPane::Files)
            ),
            file_query: self.ui.search.file_query.clone(),
            visible_rows: self
                .visible_file_indices()
                .into_iter()
                .filter_map(|idx| self.domain.file_rows.get(idx).cloned())
                .collect::<Vec<_>>(),
            changed_files: self.domain.aggregate.files.len(),
        };

        let commits = CommitPaneSnapshot {
            commits_search_mode: matches!(
                self.ui.preferences.input_mode,
                InputMode::ListSearch(FocusPane::Commits)
            ),
            commit_query: self.ui.search.commit_query.clone(),
            visible_commits: self
                .visible_commit_indices()
                .into_iter()
                .filter_map(|idx| self.domain.commits.get(idx).cloned())
                .collect::<Vec<_>>(),
            commented_commit_ids: commented_commit_ids_from_comments(self.deps.comments.comments()),
            selected_total: self
                .domain
                .commits
                .iter()
                .filter(|row| row.selected)
                .count(),
            total_commits: self.domain.commits.len(),
            status_counts: self.status_counts(),
            status_filter: self.ui.commit_ui.status_filter,
        };

        AppRenderSnapshot {
            header: HeaderSnapshot {
                branch_name: self.deps.git.branch_name().to_owned(),
                repo_root: self.deps.git.root().to_path_buf(),
            },
            files,
            commits,
            focused: self.ui.preferences.focused,
            nerd_fonts: self.ui.preferences.nerd_fonts,
            now_ts: self.now_timestamp(),
        }
    }
}
