use std::{
    fs,
    fs::OpenOptions,
    io::{ErrorKind, Write},
    path::{Path, PathBuf},
};

use anyhow::Context;
use chrono::Utc;
use serde::Deserialize;

use crate::atomic_write::atomic_write_text;
use crate::model::{CommitStatusEntry, ReviewState, ReviewStatus};

pub const PROJECT_DATA_DIR: &str = ".hunkr";
const STATE_FILE: &str = "state.json";
const SHELL_HISTORY_FILE: &str = "shell-history.json";
const INSTANCE_LOCK_FILE: &str = "instance.lock";
const INSTANCE_LOCK_REL_PATH: &str = ".hunkr/instance.lock";

/// Project-local persistence manager for review state.
#[derive(Debug, Clone)]
pub struct StateStore {
    root: PathBuf,
    state_path: PathBuf,
    shell_history_path: PathBuf,
}

/// Process-lifetime lock guard used to block concurrent hunkr instances in one repo.
#[derive(Debug)]
pub struct InstanceLock {
    path: PathBuf,
    file: Option<fs::File>,
}

impl Drop for InstanceLock {
    fn drop(&mut self) {
        if let Some(file) = self.file.take() {
            drop(file);
        }
        let _ = fs::remove_file(&self.path);
    }
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

    pub fn instance_lock_rel_path(&self) -> &'static str {
        INSTANCE_LOCK_REL_PATH
    }

    pub fn try_acquire_instance_lock(&self) -> anyhow::Result<Option<InstanceLock>> {
        if !self.root.exists() {
            return Ok(None);
        }
        self.acquire_instance_lock().map(Some)
    }

    pub fn acquire_instance_lock(&self) -> anyhow::Result<InstanceLock> {
        fs::create_dir_all(&self.root)
            .with_context(|| format!("failed to create {}", self.root.display()))?;
        let lock_path = self.root.join(INSTANCE_LOCK_FILE);
        let mut file = match OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&lock_path)
        {
            Ok(file) => file,
            Err(err) if err.kind() == ErrorKind::AlreadyExists => {
                return Err(anyhow::anyhow!(
                    "another hunkr instance is active ({INSTANCE_LOCK_REL_PATH} exists). If stale, remove {INSTANCE_LOCK_REL_PATH} and retry."
                ));
            }
            Err(err) => {
                return Err(err)
                    .with_context(|| format!("failed to create {}", lock_path.display()));
            }
        };

        if let Err(err) = writeln!(file, "pid={}", std::process::id()) {
            let _ = fs::remove_file(&lock_path);
            return Err(err).with_context(|| format!("failed to write {}", lock_path.display()));
        }
        if let Err(err) = file.sync_all() {
            let _ = fs::remove_file(&lock_path);
            return Err(err).with_context(|| format!("failed to sync {}", lock_path.display()));
        }
        Ok(InstanceLock {
            path: lock_path,
            file: Some(file),
        })
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

        if let Ok(mut parsed_json) = serde_json::from_str::<serde_json::Value>(&raw) {
            let migrated = migrate_resolved_status_tokens(&mut parsed_json);
            if let Ok(parsed) = serde_json::from_value::<ReviewState>(parsed_json) {
                if migrated {
                    self.save(&parsed)?;
                }
                return Ok(parsed);
            }
        }

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
        let payload = serde_json::to_string_pretty(state).context("failed to encode state json")?;
        atomic_write_text(&self.state_path, &payload)
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
        let payload = serde_json::to_string_pretty(&ShellHistory {
            version: 1,
            commands: commands.to_vec(),
        })
        .context("failed to encode shell history json")?;
        atomic_write_text(&self.shell_history_path, &payload)
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

/// Migrates legacy resolved status/filter values to reviewed in-place.
fn migrate_resolved_status_tokens(value: &mut serde_json::Value) -> bool {
    let mut migrated = false;

    if let Some(statuses) = value
        .get_mut("statuses")
        .and_then(serde_json::Value::as_object_mut)
    {
        for entry in statuses.values_mut() {
            let Some(status_obj) = entry.as_object_mut() else {
                continue;
            };
            let Some(status) = status_obj.get_mut("status") else {
                continue;
            };
            if matches!(status, serde_json::Value::String(current) if current == "RESOLVED") {
                *status = serde_json::Value::String("REVIEWED".to_owned());
                migrated = true;
            }
        }
    }

    if let Some(ui_session) = value
        .get_mut("ui_session")
        .and_then(serde_json::Value::as_object_mut)
        && let Some(filter) = ui_session.get_mut("commit_status_filter")
        && matches!(
            filter,
            serde_json::Value::String(current) if current == "REVIEWED_OR_RESOLVED"
        )
    {
        *filter = serde_json::Value::String("REVIEWED".to_owned());
        migrated = true;
    }

    migrated
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
