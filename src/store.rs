use std::{
    fs,
    path::{Path, PathBuf},
};

use anyhow::Context;
use chrono::Utc;

use crate::model::{ApprovalEntry, ApprovalScope, ReviewState};

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
        let parsed = serde_json::from_str::<ReviewState>(&raw)
            .with_context(|| format!("failed to parse {}", self.state_path.display()))?;
        Ok(parsed)
    }

    pub fn save(&self, state: &ReviewState) -> anyhow::Result<()> {
        fs::create_dir_all(&self.root)
            .with_context(|| format!("failed to create {}", self.root.display()))?;
        let payload = serde_json::to_string_pretty(state).context("failed to encode state json")?;
        fs::write(&self.state_path, payload)
            .with_context(|| format!("failed to write {}", self.state_path.display()))?;
        Ok(())
    }

    pub fn mark_approved(
        &self,
        state: &mut ReviewState,
        commit_id: &str,
        scope: ApprovalScope,
        branch: &str,
    ) {
        let entry = ApprovalEntry {
            scope,
            branch: branch.to_owned(),
            approved_at: Utc::now().to_rfc3339(),
        };
        state.approvals.insert(commit_id.to_owned(), entry);
    }

    pub fn mark_many_approved(
        &self,
        state: &mut ReviewState,
        commit_ids: impl IntoIterator<Item = String>,
        scope: ApprovalScope,
        branch: &str,
    ) {
        for commit_id in commit_ids {
            self.mark_approved(state, &commit_id, scope, branch);
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use tempfile::tempdir;

    use super::*;

    #[test]
    fn state_roundtrip_preserves_approvals() {
        let tmp = tempdir().expect("tempdir");
        let store = StateStore::for_project(tmp.path());
        let mut state = ReviewState {
            version: 1,
            approvals: BTreeMap::new(),
        };

        store.mark_approved(&mut state, "abc123", ApprovalScope::Commit, "main");
        store.save(&state).expect("save");

        let loaded = store.load().expect("load");
        assert_eq!(loaded.version, 1);
        assert!(loaded.approvals.contains_key("abc123"));
        let entry = loaded.approvals.get("abc123").expect("approval");
        assert_eq!(entry.scope, ApprovalScope::Commit);
        assert_eq!(entry.branch, "main");
    }

    #[test]
    fn load_missing_state_returns_default() {
        let tmp = tempdir().expect("tempdir");
        let store = StateStore::for_project(tmp.path());
        let loaded = store.load().expect("load");
        assert!(loaded.approvals.is_empty());
        assert_eq!(loaded.version, 1);
    }

    #[test]
    fn mark_many_approved_writes_each_commit() {
        let tmp = tempdir().expect("tempdir");
        let store = StateStore::for_project(tmp.path());
        let mut state = ReviewState::default();

        store.mark_many_approved(
            &mut state,
            ["a1".to_string(), "b2".to_string()],
            ApprovalScope::Selection,
            "feature/x",
        );

        assert_eq!(state.approvals.len(), 2);
        assert_eq!(
            state.approvals.get("a1").expect("a1").scope,
            ApprovalScope::Selection
        );
    }
}
