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
const SHELL_HISTORY_FILE: &str = "shell-history.json";

/// Project-local persistence manager for review state.
#[derive(Debug, Clone)]
pub struct StateStore {
    root: PathBuf,
    state_path: PathBuf,
    shell_history_path: PathBuf,
}

impl StateStore {
    pub fn for_project(project_root: &Path) -> Self {
        let root = project_root.join(PROJECT_DATA_DIR);
        let state_path = root.join(STATE_FILE);
        let shell_history_path = root.join(SHELL_HISTORY_FILE);
        Self {
            root,
            state_path,
            shell_history_path,
        }
    }

    pub fn root_dir(&self) -> &Path {
        &self.root
    }

    pub fn has_state_file(&self) -> bool {
        self.state_path.exists()
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

    pub fn load_shell_history(&self) -> anyhow::Result<Vec<String>> {
        if !self.shell_history_path.exists() {
            return Ok(Vec::new());
        }

        let raw = fs::read_to_string(&self.shell_history_path)
            .with_context(|| format!("failed to read {}", self.shell_history_path.display()))?;
        if let Ok(history) = serde_json::from_str::<ShellHistory>(&raw) {
            return Ok(history.commands);
        }
        if let Ok(commands) = serde_json::from_str::<Vec<String>>(&raw) {
            return Ok(commands);
        }
        Err(anyhow::anyhow!(
            "failed to parse {}",
            self.shell_history_path.display()
        ))
    }

    pub fn save_shell_history(&self, commands: &[String]) -> anyhow::Result<()> {
        fs::create_dir_all(&self.root)
            .with_context(|| format!("failed to create {}", self.root.display()))?;
        let payload = serde_json::to_string_pretty(&ShellHistory {
            version: 1,
            commands: commands.to_vec(),
        })
        .context("failed to encode shell history json")?;
        fs::write(&self.shell_history_path, payload)
            .with_context(|| format!("failed to write {}", self.shell_history_path.display()))?;
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
        let branch = branch.to_owned();
        let updated_at = Utc::now().to_rfc3339();
        for commit_id in commit_ids {
            state.statuses.insert(
                commit_id,
                CommitStatusEntry {
                    status,
                    branch: branch.clone(),
                    updated_at: updated_at.clone(),
                },
            );
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

#[derive(Debug, Deserialize, serde::Serialize)]
struct ShellHistory {
    version: u32,
    commands: Vec<String>,
}

#[cfg(test)]
mod tests;
