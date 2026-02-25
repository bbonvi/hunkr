use std::{
    fs,
    path::{Path, PathBuf},
};

use anyhow::Context;
use chrono::Utc;
use serde::Deserialize;

use crate::model::{CommitStatusEntry, ReviewState, ReviewStatus};

pub const PROJECT_DATA_DIR: &str = ".hunkr";
const STATE_FILE: &str = "state.json";

/// Project-local persistence manager for review state.
#[derive(Debug, Clone)]
pub struct StateStore {
    root: PathBuf,
    state_path: PathBuf,
}

impl StateStore {
    pub fn for_project(project_root: &Path) -> Self {
        let root = project_root.join(PROJECT_DATA_DIR);
        let state_path = root.join(STATE_FILE);
        Self { root, state_path }
    }

    pub fn root_dir(&self) -> &Path {
        &self.root
    }

    pub fn load(&self) -> anyhow::Result<ReviewState> {
        if !self.state_path.exists() {
            return Ok(ReviewState::default());
        }

        let raw = fs::read_to_string(&self.state_path)
            .with_context(|| format!("failed to read {}", self.state_path.display()))?;

        if let Ok(parsed) = serde_json::from_str::<ReviewState>(&raw) {
            return Ok(parsed);
        }

        let legacy = serde_json::from_str::<LegacyReviewState>(&raw)
            .with_context(|| format!("failed to parse {}", self.state_path.display()))?;

        let mut upgraded = ReviewState::default();
        for (commit_id, approval) in legacy.approvals {
            upgraded.statuses.insert(
                commit_id,
                CommitStatusEntry {
                    status: ReviewStatus::Reviewed,
                    branch: approval.branch,
                    updated_at: approval.approved_at,
                },
            );
        }
        Ok(upgraded)
    }

    pub fn save(&self, state: &ReviewState) -> anyhow::Result<()> {
        fs::create_dir_all(&self.root)
            .with_context(|| format!("failed to create {}", self.root.display()))?;
        let payload = serde_json::to_string_pretty(state).context("failed to encode state json")?;
        fs::write(&self.state_path, payload)
            .with_context(|| format!("failed to write {}", self.state_path.display()))?;
        Ok(())
    }

    pub fn commit_status(&self, state: &ReviewState, commit_id: &str) -> ReviewStatus {
        state
            .statuses
            .get(commit_id)
            .map(|entry| entry.status)
            .unwrap_or(ReviewStatus::Unreviewed)
    }

    pub fn set_status(
        &self,
        state: &mut ReviewState,
        commit_id: &str,
        status: ReviewStatus,
        branch: &str,
    ) {
        state.statuses.insert(
            commit_id.to_owned(),
            CommitStatusEntry {
                status,
                branch: branch.to_owned(),
                updated_at: Utc::now().to_rfc3339(),
            },
        );
    }

    pub fn set_many_status(
        &self,
        state: &mut ReviewState,
        commit_ids: impl IntoIterator<Item = String>,
        status: ReviewStatus,
        branch: &str,
    ) {
        for commit_id in commit_ids {
            self.set_status(state, &commit_id, status, branch);
        }
    }
}

#[derive(Debug, Deserialize)]
struct LegacyApprovalEntry {
    branch: String,
    approved_at: String,
}

#[derive(Debug, Deserialize)]
struct LegacyReviewState {
    approvals: std::collections::BTreeMap<String, LegacyApprovalEntry>,
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use tempfile::tempdir;

    use super::*;

    #[test]
    fn state_roundtrip_preserves_statuses() {
        let tmp = tempdir().expect("tempdir");
        let store = StateStore::for_project(tmp.path());
        let mut state = ReviewState {
            version: 2,
            statuses: BTreeMap::new(),
        };

        store.set_status(&mut state, "abc123", ReviewStatus::IssueFound, "main");
        store.save(&state).expect("save");

        let loaded = store.load().expect("load");
        assert_eq!(loaded.version, 2);
        assert!(loaded.statuses.contains_key("abc123"));
        let entry = loaded.statuses.get("abc123").expect("status entry");
        assert_eq!(entry.status, ReviewStatus::IssueFound);
        assert_eq!(entry.branch, "main");
    }

    #[test]
    fn load_missing_state_returns_default() {
        let tmp = tempdir().expect("tempdir");
        let store = StateStore::for_project(tmp.path());
        let loaded = store.load().expect("load");
        assert!(loaded.statuses.is_empty());
        assert_eq!(loaded.version, 2);
    }

    #[test]
    fn set_many_status_writes_each_commit() {
        let tmp = tempdir().expect("tempdir");
        let store = StateStore::for_project(tmp.path());
        let mut state = ReviewState::default();

        store.set_many_status(
            &mut state,
            ["a1".to_string(), "b2".to_string()],
            ReviewStatus::Resolved,
            "feature/x",
        );

        assert_eq!(state.statuses.len(), 2);
        assert_eq!(
            state.statuses.get("a1").expect("a1").status,
            ReviewStatus::Resolved
        );
    }

    #[test]
    fn legacy_state_upgrades_to_reviewed() {
        let tmp = tempdir().expect("tempdir");
        let store = StateStore::for_project(tmp.path());
        let legacy_raw = r#"{
  "version": 1,
  "approvals": {
    "abc": {"scope": "commit", "branch": "main", "approved_at": "2024-01-01T00:00:00Z"}
  }
}"#;

        fs::create_dir_all(store.root_dir()).expect("mkdir");
        fs::write(store.state_path.clone(), legacy_raw).expect("write");

        let loaded = store.load().expect("load");
        assert_eq!(
            loaded.statuses.get("abc").expect("abc").status,
            ReviewStatus::Reviewed
        );
    }
}
