//! Runtime review-state sync tracking for `.hunkr/state.json`.

use std::{
    fs,
    path::{Path, PathBuf},
    time::SystemTime,
};

use crate::app::*;
use crate::{model::ReviewState, store::StateStore};

/// Tracks `.hunkr/state.json` metadata so disk merges run only when the source actually changed.
pub(super) struct ReviewStateSync {
    state_path: PathBuf,
    watch_dir: PathBuf,
    dir_present: bool,
    last_dir_modified: Option<SystemTime>,
    file_present: bool,
    last_state_file_modified: Option<SystemTime>,
}

impl ReviewStateSync {
    /// Creates runtime tracking for the store backing the current repository.
    pub(super) fn new(store: &StateStore) -> Self {
        let state_path = store.state_path().to_path_buf();
        let watch_dir = store.root_dir().to_path_buf();
        Self {
            state_path,
            watch_dir,
            dir_present: false,
            last_dir_modified: None,
            file_present: false,
            last_state_file_modified: None,
        }
    }

    /// Returns the directory that must be watched for atomic `state.json` replacements.
    pub(super) fn watch_dir(&self) -> &Path {
        &self.watch_dir
    }

    /// Syncs disk statuses into memory when the tracked state file metadata changed.
    ///
    /// The sync path is intentionally read-only. Our own saves may trigger this, but they must
    /// never cascade into another write and loop forever.
    pub(super) fn sync_statuses_from_disk_if_changed(
        &mut self,
        store: &StateStore,
        state: &mut ReviewState,
        force: bool,
    ) -> anyhow::Result<bool> {
        let file_metadata = fs::metadata(&self.state_path).ok();
        let file_modified = file_metadata
            .as_ref()
            .and_then(|entry| entry.modified().ok());

        if !self.source_changed(force, file_metadata.as_ref(), file_modified) {
            return Ok(false);
        }

        if file_metadata.is_none() {
            self.file_present = false;
            self.last_state_file_modified = None;
            return Ok(false);
        }

        self.file_present = true;
        self.last_state_file_modified = file_modified;

        let prior_version = state.version;
        let prior_statuses = state.statuses.clone();
        store.sync_statuses_from_disk(state)?;
        Ok(prior_version != state.version || prior_statuses != state.statuses)
    }

    fn source_changed(
        &mut self,
        force: bool,
        file_metadata: Option<&fs::Metadata>,
        file_modified: Option<SystemTime>,
    ) -> bool {
        let dir_metadata = fs::metadata(&self.watch_dir).ok();
        let dir_modified = dir_metadata
            .as_ref()
            .and_then(|entry| entry.modified().ok());

        let dir_changed = if dir_metadata.is_none() {
            let changed = self.dir_present;
            self.dir_present = false;
            self.last_dir_modified = None;
            changed
        } else {
            let changed = !self.dir_present || self.last_dir_modified != dir_modified;
            self.dir_present = true;
            self.last_dir_modified = dir_modified;
            changed
        };

        let file_exists = file_metadata.is_some();
        let file_changed = self.file_present != file_exists
            || (file_exists && self.last_state_file_modified != file_modified);

        force || dir_changed || file_changed
    }
}

impl App {
    /// Merges externally updated commit statuses from `.hunkr/state.json` without reloading git.
    pub(super) fn sync_review_statuses_from_disk_if_changed(
        &mut self,
        force: bool,
    ) -> anyhow::Result<bool> {
        let changed = self.review_state_sync.sync_statuses_from_disk_if_changed(
            &self.deps.store,
            &mut self.domain.review_state,
            force,
        )?;
        if !changed {
            return Ok(false);
        }

        for row in &mut self.domain.commits {
            row.status = if row.is_uncommitted {
                ReviewStatus::Unreviewed
            } else {
                self.deps
                    .store
                    .commit_status(&self.domain.review_state, &row.info.id)
            };
        }
        self.sync_commit_cursor_for_filters(None, self.ui.commit_ui.list_state.selected());
        if self
            .ui
            .commit_ui
            .visual_anchor
            .is_some_and(|anchor| !self.visible_commit_indices().contains(&anchor))
        {
            self.ui.commit_ui.visual_anchor = None;
        }
        self.runtime.needs_redraw = true;
        Ok(true)
    }
}

#[cfg(test)]
mod tests {
    use std::{collections::BTreeMap, thread, time::Duration};

    use tempfile::TempDir;

    use super::*;
    use crate::model::{CommitStatusEntry, ReviewStatus};

    fn state_store(tempdir: &TempDir) -> StateStore {
        StateStore::for_project(tempdir.path())
    }

    fn status_entry(status: ReviewStatus, updated_at: &str) -> CommitStatusEntry {
        CommitStatusEntry {
            status,
            branch: "main".to_owned(),
            updated_at: updated_at.to_owned(),
        }
    }

    #[test]
    fn sync_statuses_from_disk_if_changed_detects_external_state_updates() {
        let tempdir = TempDir::new().expect("tempdir");
        let store = state_store(&tempdir);
        fs::create_dir_all(store.root_dir()).expect("create state dir");

        let mut runtime = ReviewStateSync::new(&store);
        let mut state = ReviewState::default();
        assert!(
            !runtime
                .sync_statuses_from_disk_if_changed(&store, &mut state, true)
                .expect("initial sync")
        );

        let mut persisted = ReviewState::default();
        persisted.statuses.insert(
            "commit-a".to_owned(),
            status_entry(ReviewStatus::IssueFound, "2026-01-01T00:00:00Z"),
        );
        store
            .save_statuses_merged(&mut persisted)
            .expect("save statuses");

        thread::sleep(Duration::from_millis(25));

        assert!(
            runtime
                .sync_statuses_from_disk_if_changed(&store, &mut state, false)
                .expect("sync after write")
        );
        assert_eq!(
            state.statuses.get("commit-a").expect("commit-a").status,
            ReviewStatus::IssueFound
        );
    }

    #[test]
    fn sync_statuses_from_disk_if_changed_ignores_unrelated_directory_churn() {
        let tempdir = TempDir::new().expect("tempdir");
        let store = state_store(&tempdir);
        fs::create_dir_all(store.root_dir()).expect("create state dir");

        let mut runtime = ReviewStateSync::new(&store);
        let mut state = ReviewState::default();
        assert!(
            !runtime
                .sync_statuses_from_disk_if_changed(&store, &mut state, true)
                .expect("initial sync")
        );

        let scratch_path = store.root_dir().join("scratch.tmp");
        fs::write(&scratch_path, "tmp").expect("write temp");
        thread::sleep(Duration::from_millis(25));

        assert!(
            !runtime
                .sync_statuses_from_disk_if_changed(&store, &mut state, false)
                .expect("sync after unrelated change")
        );
        assert_eq!(state.statuses, BTreeMap::new());
    }

    #[test]
    fn sync_statuses_from_disk_if_changed_force_reloads_even_without_metadata_delta() {
        let tempdir = TempDir::new().expect("tempdir");
        let store = state_store(&tempdir);
        fs::create_dir_all(store.root_dir()).expect("create state dir");

        let mut runtime = ReviewStateSync::new(&store);
        let mut state = ReviewState::default();
        assert!(
            !runtime
                .sync_statuses_from_disk_if_changed(&store, &mut state, true)
                .expect("initial sync")
        );

        let mut persisted = ReviewState::default();
        persisted.statuses.insert(
            "commit-a".to_owned(),
            status_entry(ReviewStatus::Reviewed, "2026-01-01T00:00:00Z"),
        );
        store
            .save_statuses_merged(&mut persisted)
            .expect("save statuses");
        assert!(
            runtime
                .sync_statuses_from_disk_if_changed(&store, &mut state, true)
                .expect("forced sync")
        );
        assert_eq!(
            state.statuses.get("commit-a").expect("commit-a").status,
            ReviewStatus::Reviewed
        );
    }
}
