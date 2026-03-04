use std::{
    cmp::Ordering,
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
};

use anyhow::Context;
use chrono::{DateTime, Utc};
use fs2::FileExt;
use serde::Deserialize;

use crate::atomic_write::atomic_write_text;
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum UiSessionMergePolicy {
    PreserveDisk,
    PreferMemory,
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

    /// Merges persisted status updates from disk into the in-memory review state.
    pub fn sync_statuses_from_disk(&self, state: &mut ReviewState) -> anyhow::Result<()> {
        let persisted = self.load()?;
        state.version = state.version.max(persisted.version);
        state.statuses = merge_status_maps(&state.statuses, &persisted.statuses);
        Ok(())
    }

    pub fn load(&self) -> anyhow::Result<ReviewState> {
        if !self.state_path.exists() {
            return Ok(ReviewState::default());
        }

        let raw = fs::read_to_string(&self.state_path)
            .with_context(|| format!("failed to read {}", self.state_path.display()))?;

        if let Ok(mut parsed_json) = serde_json::from_str::<serde_json::Value>(&raw) {
            migrate_resolved_status_tokens(&mut parsed_json);
            if let Ok(parsed) = serde_json::from_value::<ReviewState>(parsed_json) {
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

    /// Persists status changes while preserving the latest UI session state on disk.
    pub fn save_statuses_merged(&self, state: &mut ReviewState) -> anyhow::Result<()> {
        self.merge_and_save_state(state, UiSessionMergePolicy::PreserveDisk)
    }

    /// Persists the full review state while merging status updates from disk.
    pub fn save_state_merged(&self, state: &mut ReviewState) -> anyhow::Result<()> {
        self.merge_and_save_state(state, UiSessionMergePolicy::PreferMemory)
    }

    fn merge_and_save_state(
        &self,
        state: &mut ReviewState,
        ui_session_policy: UiSessionMergePolicy,
    ) -> anyhow::Result<()> {
        let merged = self.with_state_write_lock(|| {
            let persisted = self.load()?;
            let merged = ReviewState {
                version: persisted.version.max(state.version),
                statuses: merge_status_maps(&persisted.statuses, &state.statuses),
                ui_session: match ui_session_policy {
                    UiSessionMergePolicy::PreserveDisk => persisted.ui_session,
                    UiSessionMergePolicy::PreferMemory => state.ui_session.clone(),
                },
            };
            self.save(&merged)?;
            Ok(merged)
        })?;
        state.version = merged.version;
        state.statuses = merged.statuses;
        Ok(())
    }

    fn with_state_write_lock<T>(
        &self,
        operation: impl FnOnce() -> anyhow::Result<T>,
    ) -> anyhow::Result<T> {
        fs::create_dir_all(&self.root)
            .with_context(|| format!("failed to create {}", self.root.display()))?;
        let lock_handle = fs::File::open(&self.root)
            .with_context(|| format!("failed to open {}", self.root.display()))?;
        lock_handle
            .lock_exclusive()
            .with_context(|| format!("failed to lock {}", self.root.display()))?;

        let operation_result = operation();
        let unlock_result = lock_handle
            .unlock()
            .with_context(|| format!("failed to unlock {}", self.root.display()));
        match (operation_result, unlock_result) {
            (Ok(value), Ok(())) => Ok(value),
            (Err(err), Ok(())) => Err(err),
            (Ok(_), Err(unlock_err)) => Err(unlock_err),
            (Err(err), Err(unlock_err)) => Err(err.context(unlock_err)),
        }
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

fn merge_status_maps(
    left: &BTreeMap<String, CommitStatusEntry>,
    right: &BTreeMap<String, CommitStatusEntry>,
) -> BTreeMap<String, CommitStatusEntry> {
    let mut merged = left.clone();
    for (commit_id, candidate) in right {
        match merged.get(commit_id) {
            Some(current) if !candidate_is_newer(candidate, current) => {}
            _ => {
                merged.insert(commit_id.clone(), candidate.clone());
            }
        }
    }
    merged
}

fn candidate_is_newer(candidate: &CommitStatusEntry, current: &CommitStatusEntry) -> bool {
    match compare_updated_at(candidate, current) {
        Ordering::Greater => true,
        Ordering::Less => false,
        Ordering::Equal => {
            (
                candidate.updated_at.as_str(),
                candidate.status.as_str(),
                candidate.branch.as_str(),
            ) > (
                current.updated_at.as_str(),
                current.status.as_str(),
                current.branch.as_str(),
            )
        }
    }
}

fn compare_updated_at(left: &CommitStatusEntry, right: &CommitStatusEntry) -> Ordering {
    match (
        parse_rfc3339_utc(&left.updated_at),
        parse_rfc3339_utc(&right.updated_at),
    ) {
        (Some(left_ts), Some(right_ts)) => left_ts.cmp(&right_ts),
        (Some(_), None) => Ordering::Greater,
        (None, Some(_)) => Ordering::Less,
        (None, None) => left.updated_at.cmp(&right.updated_at),
    }
}

fn parse_rfc3339_utc(value: &str) -> Option<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(value)
        .ok()
        .map(|parsed| parsed.with_timezone(&Utc))
}

#[derive(Debug, Deserialize)]
struct LegacyApprovalEntry {
    branch: String,
    approved_at: String,
}

#[derive(Debug, Deserialize)]
struct LegacyReviewState {
    approvals: BTreeMap<String, LegacyApprovalEntry>,
}

#[derive(Debug, Deserialize, serde::Serialize)]
struct ShellHistory {
    version: u32,
    commands: Vec<String>,
}

#[cfg(test)]
mod tests;
