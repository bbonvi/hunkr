use std::{
    fs,
    path::{Path, PathBuf},
};

use anyhow::Context;
use chrono::Utc;

use crate::atomic_write::atomic_write_text;
use crate::model::{CommentTarget, ReviewComment, ReviewStatus};

const COMMENTS_DIR: &str = "comments";
const COMMENTS_INDEX_FILE: &str = "index.json";
const REVIEW_TASKS_SUFFIX: &str = "-review-tasks.md";

/// Stores persisted review comments and writes a single auto-updating task report.
#[derive(Debug, Clone)]
pub struct CommentStore {
    root: PathBuf,
    branch: String,
    index_path: PathBuf,
    report_path: PathBuf,
    comments: Vec<ReviewComment>,
    next_id: u64,
}

impl CommentStore {
    pub fn new(project_data_dir: &Path, branch: &str) -> anyhow::Result<Self> {
        let root = project_data_dir.join(COMMENTS_DIR);
        let index_path = root.join(COMMENTS_INDEX_FILE);
        let comments = load_index(&index_path)?;
        let next_id = comments
            .iter()
            .map(|comment| comment.id)
            .max()
            .unwrap_or(0)
            .saturating_add(1);
        let branch = sanitize(branch);
        let report_path = root.join(format!("{branch}{REVIEW_TASKS_SUFFIX}"));

        Ok(Self {
            root,
            branch,
            index_path,
            report_path,
            comments,
            next_id,
        })
    }

    pub fn comments(&self) -> &[ReviewComment] {
        &self.comments
    }

    pub fn comment_by_id(&self, id: u64) -> Option<&ReviewComment> {
        self.comments.iter().find(|comment| comment.id == id)
    }

    pub fn report_path(&self) -> &Path {
        &self.report_path
    }

    pub fn add_comment(&mut self, target: &CommentTarget, text: &str) -> anyhow::Result<u64> {
        let now = Utc::now().to_rfc3339();
        let comment = ReviewComment {
            id: self.next_id,
            target: target.clone(),
            text: text.trim().to_owned(),
            created_at: now.clone(),
            updated_at: now,
        };
        self.next_id = self.next_id.saturating_add(1);
        let id = comment.id;

        self.comments.push(comment);
        self.save_index()?;
        Ok(id)
    }

    pub fn update_comment(&mut self, id: u64, text: &str) -> anyhow::Result<bool> {
        let Some(idx) = self.comments.iter().position(|comment| comment.id == id) else {
            return Ok(false);
        };

        self.comments[idx].text = text.trim().to_owned();
        self.comments[idx].updated_at = Utc::now().to_rfc3339();
        self.save_index()?;
        Ok(true)
    }

    pub fn delete_comment(&mut self, id: u64) -> anyhow::Result<bool> {
        let Some(idx) = self.comments.iter().position(|comment| comment.id == id) else {
            return Ok(false);
        };
        self.comments.remove(idx);
        self.save_index()?;
        Ok(true)
    }

    /// Regenerates the markdown task file from persisted comments and commit statuses.
    ///
    /// Comments whose linked commits are all in non-actionable states are hidden from the task list.
    pub fn sync_review_tasks_report<F>(&self, status_for_commit: F) -> anyhow::Result<PathBuf>
    where
        F: Fn(&str) -> ReviewStatus,
    {
        fs::create_dir_all(&self.root)
            .with_context(|| format!("failed to create {}", self.root.display()))?;

        let report = render_review_tasks_report(&self.branch, &self.comments, status_for_commit);
        atomic_write_text(&self.report_path, &report)
            .with_context(|| format!("failed to write {}", self.report_path.display()))?;
        Ok(self.report_path.clone())
    }

    fn save_index(&self) -> anyhow::Result<()> {
        fs::create_dir_all(&self.root)
            .with_context(|| format!("failed to create {}", self.root.display()))?;
        let payload = serde_json::to_string_pretty(&self.comments)
            .context("failed to encode comments index")?;
        atomic_write_text(&self.index_path, &payload)
            .with_context(|| format!("failed to write {}", self.index_path.display()))?;
        Ok(())
    }
}

fn render_review_tasks_report<F>(
    branch: &str,
    comments: &[ReviewComment],
    status_for_commit: F,
) -> String
where
    F: Fn(&str) -> ReviewStatus,
{
    #[derive(Clone)]
    struct VisibleComment {
        comment: ReviewComment,
        statuses: Vec<(String, ReviewStatus)>,
    }

    let mut visible = Vec::<VisibleComment>::new();
    let mut hidden_count = 0usize;

    for comment in comments {
        let mut statuses = comment
            .target
            .commits
            .iter()
            .map(|commit| (commit.clone(), status_for_commit(commit)))
            .collect::<Vec<_>>();
        statuses.sort_by(|left, right| left.0.cmp(&right.0));

        let hidden = !statuses.is_empty()
            && statuses
                .iter()
                .all(|(_, status)| hidden_from_task_file(*status));
        if hidden {
            hidden_count += 1;
            continue;
        }
        visible.push(VisibleComment {
            comment: comment.clone(),
            statuses,
        });
    }
    visible.sort_by_key(|entry| entry.comment.id);

    let mut report = String::new();
    report.push_str("# Action Tasks\n\n");
    report.push_str("> This file is auto-generated and is read-only for agents.\n");
    report.push_str("> Do not edit this file manually.\n");
    report.push_str("> Agent instruction: address every task listed in `Open Tasks` and include an addressed report in your output.\n\n");

    report.push_str("## Working Agreement\n\n");
    report.push_str("- Purpose: actionable work derived from persisted annotations.\n");
    report.push_str("- Scope: only actionable tasks are listed in `Open Tasks`.\n");
    report
        .push_str("- Visibility rule: tasks linked only to non-actionable states are excluded.\n");
    report.push_str("- Contract: treat each listed task as required work.\n\n");

    report.push_str("## Addressed Report\n\n");
    report.push_str("After handling tasks, report:\n");
    report.push_str("- Which task IDs were addressed.\n");
    report.push_str("- Which task IDs remain open and why.\n");
    report.push_str("- Any blockers that prevented completion.\n\n");

    report.push_str("## Snapshot\n\n");
    report.push_str(&format!("- Updated: {}\n", Utc::now().to_rfc3339()));
    report.push_str(&format!("- Branch: `{branch}`\n"));
    report.push_str(&format!("- Actionable tasks: {}\n", visible.len()));
    report.push('\n');

    report.push_str("## Open Tasks\n\n");
    if visible.is_empty() {
        if hidden_count > 0 {
            report.push_str("No open tasks. Existing annotations are currently non-actionable.\n");
        } else {
            report.push_str("No open tasks.\n");
        }
        return report;
    }

    for entry in &visible {
        let comment = &entry.comment;
        report.push_str(&format!("### TASK #{}\n\n", comment.id));
        report.push_str("- Status: `ACTION_REQUIRED`\n");
        report.push_str(&format!(
            "- Target Type: `{}`\n",
            comment.target.kind.as_str()
        ));
        report.push_str(&format!("- File: `{}`\n", comment.target.start.file_path));
        report.push_str(&format!(
            "- Sources: {}\n",
            format_source_ids(&entry.statuses)
        ));
        report.push_str(&format!(
            "- Start: `{}` ({})\n",
            comment.target.start.hunk_header,
            format_anchor_lines(
                comment.target.start.old_lineno,
                comment.target.start.new_lineno
            )
        ));
        report.push_str(&format!(
            "- End: `{}` ({})\n",
            comment.target.end.hunk_header,
            format_anchor_lines(comment.target.end.old_lineno, comment.target.end.new_lineno)
        ));
        if comment.target.start.commit_summary == comment.target.end.commit_summary {
            report.push_str(&format!(
                "- Commit Context: {}\n",
                comment.target.start.commit_summary
            ));
        } else {
            report.push_str(&format!(
                "- Commit Context: {} -> {}\n",
                comment.target.start.commit_summary, comment.target.end.commit_summary
            ));
        }
        report.push_str(&format!("- Updated: {}\n", comment.updated_at));
        report.push_str("\nComment:\n\n");
        report.push_str(comment.text.trim());
        report.push('\n');

        if !comment.target.selected_lines.is_empty() {
            report.push_str("\n```diff\n");
            for line in &comment.target.selected_lines {
                report.push_str(line);
                report.push('\n');
            }
            report.push_str("```\n");
        }
        report.push('\n');
    }

    report
}

fn hidden_from_task_file(status: ReviewStatus) -> bool {
    matches!(status, ReviewStatus::Reviewed)
}

/// Formats source identifiers for agent-facing task context.
fn format_source_ids(statuses: &[(String, ReviewStatus)]) -> String {
    if statuses.is_empty() {
        return "n/a".to_owned();
    }
    statuses
        .iter()
        .map(|(commit, _)| format!("`{}`", short_id(commit)))
        .collect::<Vec<_>>()
        .join(", ")
}

fn short_id(commit: &str) -> String {
    commit.chars().take(12).collect()
}

fn load_index(path: &Path) -> anyhow::Result<Vec<ReviewComment>> {
    if !path.exists() {
        return Ok(Vec::new());
    }

    let raw =
        fs::read_to_string(path).with_context(|| format!("failed to read {}", path.display()))?;
    let comments = serde_json::from_str::<Vec<ReviewComment>>(&raw)
        .with_context(|| format!("failed to parse {}", path.display()))?;
    Ok(comments)
}

fn format_anchor_lines(old_lineno: Option<u32>, new_lineno: Option<u32>) -> String {
    match (old_lineno, new_lineno) {
        (Some(old), Some(new)) => format!("old {} / new {}", old, new),
        (Some(old), None) => format!("old {}", old),
        (None, Some(new)) => format!("new {}", new),
        (None, None) => "n/a".to_owned(),
    }
}

fn sanitize(input: &str) -> String {
    input
        .chars()
        .map(|ch| match ch {
            '/' | '\\' | ' ' => '-',
            c if c.is_ascii_alphanumeric() || c == '-' || c == '_' => c,
            _ => '_',
        })
        .collect()
}

#[cfg(test)]
mod tests;
