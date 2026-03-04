use std::{
    collections::{BTreeMap, BTreeSet},
    sync::Arc,
};

use serde::{Deserialize, Serialize};

/// Synthetic commit id representing worktree + index changes.
pub const UNCOMMITTED_COMMIT_ID: &str = "__UNCOMMITTED__";
/// Label shown for the synthetic uncommitted entry.
pub const UNCOMMITTED_COMMIT_SHORT: &str = "WORKDIR";
/// Summary shown for the synthetic uncommitted entry.
pub const UNCOMMITTED_COMMIT_SUMMARY: &str = "Uncommitted changes";

/// Workflow status for each commit in review.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ReviewStatus {
    Unreviewed,
    Reviewed,
    IssueFound,
}

impl ReviewStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Unreviewed => "UNREVIEWED",
            Self::Reviewed => "REVIEWED",
            Self::IssueFound => "ISSUE_FOUND",
        }
    }
}

/// Persisted status metadata for one commit.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CommitStatusEntry {
    pub status: ReviewStatus,
    pub branch: String,
    pub updated_at: String,
}

/// Persistent review state for one project.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ReviewState {
    pub version: u32,
    pub statuses: BTreeMap<String, CommitStatusEntry>,
    #[serde(default)]
    pub ui_session: UiSessionState,
}

impl Default for ReviewState {
    fn default() -> Self {
        Self {
            version: 2,
            statuses: BTreeMap::new(),
            ui_session: UiSessionState::default(),
        }
    }
}

/// Persisted UI/session context restored on next launch when still applicable.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct UiSessionState {
    #[serde(default)]
    pub selected_commit_ids: BTreeSet<String>,
    #[serde(default)]
    pub commit_cursor_id: Option<String>,
    #[serde(default)]
    pub commit_status_filter: Option<UiSessionCommitStatusFilter>,
    #[serde(default)]
    pub focused_pane: Option<UiSessionFocusPane>,
    #[serde(default)]
    pub selected_file: Option<String>,
    #[serde(default)]
    pub diff_positions: BTreeMap<String, UiSessionDiffPosition>,
}

/// Serializable focus-pane variant for restart persistence.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum UiSessionFocusPane {
    Commits,
    Files,
    Diff,
}

/// Serializable commit filter variant for restart persistence.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum UiSessionCommitStatusFilter {
    All,
    UnreviewedOrIssueFound,
    Reviewed,
}

/// Serializable per-file local diff viewport snapshot for restart persistence.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct UiSessionDiffPosition {
    pub scroll: usize,
    pub cursor: usize,
}

/// Lightweight commit entry shown in commit history pane.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommitInfo {
    pub id: String,
    pub short_id: String,
    pub summary: String,
    pub author: String,
    pub timestamp: i64,
    pub unpushed: bool,
    pub decorations: Vec<CommitDecoration>,
}

/// One ref decoration shown in commit metadata (e.g. `main*`, `origin/main`, `v1.2.3`).
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct CommitDecoration {
    pub kind: CommitDecorationKind,
    pub label: String,
}

/// Sorted decoration groups roughly matching `git log --decorate` precedence.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum CommitDecorationKind {
    Head,
    LocalBranch,
    RemoteBranch,
    Tag,
}

/// Type of a line inside a unified diff hunk.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiffLineKind {
    Context,
    Add,
    Remove,
    Meta,
}

/// One line of a hunk with optional line-number anchors.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HunkLine {
    pub kind: DiffLineKind,
    pub text: String,
    pub old_lineno: Option<u32>,
    pub new_lineno: Option<u32>,
}

/// One hunk associated with exactly one commit.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Hunk {
    pub commit_id: String,
    pub commit_short: String,
    pub commit_summary: String,
    pub commit_timestamp: i64,
    pub header: String,
    pub old_start: u32,
    pub new_start: u32,
    pub lines: Vec<HunkLine>,
}

/// Aggregated hunks for one file across selected commits.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FilePatch {
    pub path: String,
    pub hunks: Vec<Hunk>,
}

/// Canonical git delta classification for a path in the rendered aggregate.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Default)]
pub enum FileChangeKind {
    Added,
    #[default]
    Modified,
    Deleted,
    Renamed,
    Copied,
    TypeChanged,
    Unmerged,
    Untracked,
    Unknown,
}

/// Compact per-file metadata shown across the UI (badges, line stats, rename source).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct FileChangeSummary {
    pub kind: FileChangeKind,
    pub old_path: Option<String>,
    pub additions: usize,
    pub deletions: usize,
}

/// Diff payload used by UI.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AggregatedDiff {
    pub files: BTreeMap<String, FilePatch>,
    pub file_changes: BTreeMap<String, FileChangeSummary>,
}

impl AggregatedDiff {
    pub fn file_paths(&self) -> Vec<String> {
        self.files.keys().cloned().collect()
    }
}

/// Stable metadata for one rendered diff position.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiffLineAnchor {
    pub commit_id: Arc<str>,
    pub commit_summary: Arc<str>,
    pub file_path: Arc<str>,
    pub hunk_header: Arc<str>,
    pub old_lineno: Option<u32>,
    pub new_lineno: Option<u32>,
}
