use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

/// Scope used when a reviewer approves one or more commits.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum ApprovalScope {
    Commit,
    Selection,
    Branch,
}

/// Approval metadata persisted per commit id.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ApprovalEntry {
    pub scope: ApprovalScope,
    pub branch: String,
    pub approved_at: String,
}

/// Persistent review state for one project.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ReviewState {
    pub version: u32,
    pub approvals: BTreeMap<String, ApprovalEntry>,
}

impl Default for ReviewState {
    fn default() -> Self {
        Self {
            version: 1,
            approvals: BTreeMap::new(),
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
