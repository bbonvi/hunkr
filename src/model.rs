use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

/// Workflow status for each commit in review.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ReviewStatus {
    Unreviewed,
    Reviewed,
    IssueFound,
    Resolved,
}

impl ReviewStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Unreviewed => "UNREVIEWED",
            Self::Reviewed => "REVIEWED",
            Self::IssueFound => "ISSUE_FOUND",
            Self::Resolved => "RESOLVED",
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
}

impl Default for ReviewState {
    fn default() -> Self {
        Self {
            version: 2,
            statuses: BTreeMap::new(),
        }
    }
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

/// Diff payload used by UI.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AggregatedDiff {
    pub files: BTreeMap<String, FilePatch>,
}

impl AggregatedDiff {
    pub fn file_paths(&self) -> Vec<String> {
        self.files.keys().cloned().collect()
    }
}

/// Anchor metadata saved when adding review comments.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommentAnchor {
    pub commit_id: String,
    pub commit_summary: String,
    pub file_path: String,
    pub hunk_header: String,
    pub old_lineno: Option<u32>,
    pub new_lineno: Option<u32>,
}

/// Comment target can be a single line or a visual range.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommentTarget {
    pub start: CommentAnchor,
    pub end: CommentAnchor,
    pub commits: BTreeSet<String>,
    pub selected_lines: Vec<String>,
}
