use std::path::PathBuf;

use crate::app::{App, CommitRow, CommitStatusFilter, FocusPane, InputMode, TreeRow};

/// Immutable snapshot consumed by render/view-model builders.
pub(in crate::app) struct AppRenderSnapshot {
    pub header: HeaderSnapshot,
    pub files: FilePaneSnapshot,
    pub commits: CommitPaneSnapshot,
    pub footer: FooterSnapshot,
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
    pub selected_total: usize,
    pub total_commits: usize,
    pub status_counts: (usize, usize, usize),
    pub status_filter: CommitStatusFilter,
}

/// Footer immutable snapshot.
pub(in crate::app) struct FooterSnapshot {
    pub input_mode: InputMode,
    pub focused: FocusPane,
    pub status: String,
    pub commit_visual_active: bool,
    pub diff_visual_active: bool,
    pub diff_search_buffer: String,
    pub diff_search_cursor: usize,
    pub commit_query: String,
    pub commit_cursor: usize,
    pub file_query: String,
    pub file_cursor: usize,
    pub focused_commit: Option<CommitRow>,
    pub shell: FooterShellSnapshot,
    pub worktree: FooterWorktreeSnapshot,
}

/// Footer shell-mode state.
pub(in crate::app) struct FooterShellSnapshot {
    pub running: bool,
    pub finished: bool,
    pub reverse_search: bool,
    pub command_label: String,
}

/// Footer worktree search state.
pub(in crate::app) struct FooterWorktreeSnapshot {
    pub search_active: bool,
    pub query: String,
    pub visible_count: usize,
    pub total_count: usize,
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

        let visible_commit_indices = self.visible_commit_indices();
        let visible_commits = visible_commit_indices
            .iter()
            .filter_map(|idx| self.domain.commits.get(*idx).cloned())
            .collect::<Vec<_>>();
        let focused_commit = self
            .ui
            .commit_ui
            .list_state
            .selected()
            .and_then(|visible_idx| visible_commit_indices.get(visible_idx).copied())
            .and_then(|full_idx| self.domain.commits.get(full_idx).cloned());

        let commits = CommitPaneSnapshot {
            commits_search_mode: matches!(
                self.ui.preferences.input_mode,
                InputMode::ListSearch(FocusPane::Commits)
            ),
            commit_query: self.ui.search.commit_query.clone(),
            visible_commits,
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
        let footer = FooterSnapshot {
            input_mode: self.ui.preferences.input_mode,
            focused: self.ui.preferences.focused,
            status: self.runtime.status.clone(),
            commit_visual_active: self.ui.commit_ui.visual_anchor.is_some(),
            diff_visual_active: self.ui.diff_ui.visual_selection.is_some(),
            diff_search_buffer: self.ui.search.diff_buffer.clone(),
            diff_search_cursor: self.ui.search.diff_cursor,
            commit_query: self.ui.search.commit_query.clone(),
            commit_cursor: self.ui.search.commit_cursor,
            file_query: self.ui.search.file_query.clone(),
            file_cursor: self.ui.search.file_cursor,
            focused_commit,
            shell: FooterShellSnapshot {
                running: self.ui.shell_command.running.is_some(),
                finished: self.ui.shell_command.finished.is_some(),
                reverse_search: self.ui.shell_command.reverse_search.is_some(),
                command_label: self
                    .ui
                    .shell_command
                    .active_command
                    .clone()
                    .unwrap_or_else(|| self.ui.shell_command.buffer.clone()),
            },
            worktree: FooterWorktreeSnapshot {
                search_active: self.ui.worktree_switch.search_active,
                query: self.ui.worktree_switch.query.clone(),
                visible_count: self.visible_worktree_indices().len(),
                total_count: self.ui.worktree_switch.entries.len(),
            },
        };

        AppRenderSnapshot {
            header: HeaderSnapshot {
                branch_name: self.deps.git.branch_name().to_owned(),
                repo_root: self.deps.git.root().to_path_buf(),
            },
            files,
            commits,
            footer,
            focused: self.ui.preferences.focused,
            nerd_fonts: self.ui.preferences.nerd_fonts,
            now_ts: self.now_timestamp(),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::{
        path::{Path, PathBuf},
        process::Command,
        sync::Arc,
        time::Instant,
    };

    use chrono::{DateTime, Utc};
    use tempfile::TempDir;

    use super::*;
    use crate::{app::DiffVisualOrigin, config::AppConfig};

    struct TestClock;

    impl crate::app::AppClock for TestClock {
        fn now_utc(&self) -> DateTime<Utc> {
            Utc::now()
        }

        fn now_instant(&self) -> Instant {
            Instant::now()
        }
    }

    struct TestBootstrapPorts {
        repo_root: PathBuf,
    }

    impl crate::app::AppBootstrapPorts for TestBootstrapPorts {
        fn open_current_git(&self) -> anyhow::Result<crate::git_data::GitService> {
            crate::git_data::GitService::open_at(&self.repo_root)
        }

        fn load_config(&self) -> anyhow::Result<AppConfig> {
            Ok(AppConfig::default())
        }

        fn state_store_for_repo(&self, repo_root: &Path) -> crate::store::StateStore {
            crate::store::StateStore::for_project(repo_root)
        }

        fn clock(&self) -> Arc<dyn crate::app::AppClock> {
            Arc::new(TestClock)
        }

        fn runtime_ports(&self) -> Arc<dyn crate::app::AppRuntimePorts> {
            Arc::new(crate::app::ports::SystemRuntimePorts)
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

    fn bootstrap_app(repo_root: &Path) -> App {
        let store = crate::store::StateStore::for_project(repo_root);
        store
            .save(&crate::model::ReviewState::default())
            .expect("seed persisted state to bypass onboarding");

        let ports = TestBootstrapPorts {
            repo_root: repo_root.to_path_buf(),
        };
        App::bootstrap_with(&ports).expect("bootstrap app")
    }

    #[test]
    fn capture_render_snapshot_tracks_footer_and_list_modes() {
        let repo = init_test_repo();
        let mut app = bootstrap_app(repo.path());

        app.ui.preferences.input_mode = InputMode::ListSearch(FocusPane::Files);
        app.ui.search.file_query = "src".to_owned();
        app.ui.search.commit_query = "fix".to_owned();
        app.ui.commit_ui.visual_anchor = Some(0);
        app.ui.diff_ui.visual_selection = Some(crate::app::DiffVisualSelection {
            anchor: 0,
            origin: DiffVisualOrigin::Keyboard,
        });
        app.ui.worktree_switch.search_active = true;
        app.ui.worktree_switch.query = "feature".to_owned();
        app.ui.shell_command.buffer = "echo hello".to_owned();

        let snapshot = app.capture_render_snapshot();

        assert!(snapshot.files.files_search_mode);
        assert_eq!(snapshot.files.file_query, "src");
        assert_eq!(snapshot.commits.commit_query, "fix");
        assert!(snapshot.footer.commit_visual_active);
        assert!(snapshot.footer.diff_visual_active);
        assert!(snapshot.footer.worktree.search_active);
        assert_eq!(snapshot.footer.worktree.query, "feature");
        assert_eq!(snapshot.footer.shell.command_label, "echo hello");
        assert_eq!(
            snapshot.footer.worktree.total_count,
            app.ui.worktree_switch.entries.len()
        );
    }

    #[test]
    fn capture_render_snapshot_prefers_active_shell_command_label() {
        let repo = init_test_repo();
        let mut app = bootstrap_app(repo.path());

        app.ui.preferences.input_mode = InputMode::ShellCommand;
        app.ui.shell_command.buffer = "echo fallback".to_owned();
        app.ui.shell_command.active_command = Some("git status".to_owned());

        let snapshot = app.capture_render_snapshot();

        assert_eq!(snapshot.footer.input_mode, InputMode::ShellCommand);
        assert_eq!(snapshot.footer.shell.command_label, "git status");
    }
}
