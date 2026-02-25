use std::{
    fs::{self, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
};

use anyhow::Context;
use chrono::Utc;

use crate::model::{CommentTarget, ReviewComment};

const COMMENTS_DIR: &str = "comments";
const COMMENTS_INDEX_FILE: &str = "index.json";

/// Stores persisted review comments and markdown session exports.
#[derive(Debug, Clone)]
pub struct CommentStore {
    root: PathBuf,
    branch: String,
    session_file: Option<PathBuf>,
    event_counter: u32,
    index_path: PathBuf,
    comments: Vec<ReviewComment>,
    next_id: u64,
}

impl CommentStore {
    pub fn new(project_data_dir: &Path, branch: &str) -> anyhow::Result<Self> {
        let root = project_data_dir.join(COMMENTS_DIR);
        let index_path = root.join(COMMENTS_INDEX_FILE);
        fs::create_dir_all(&root)
            .with_context(|| format!("failed to create {}", root.display()))?;

        let comments = load_index(&index_path)?;
        let next_id = comments
            .iter()
            .map(|comment| comment.id)
            .max()
            .unwrap_or(0)
            .saturating_add(1);

        Ok(Self {
            root,
            branch: sanitize(branch),
            session_file: None,
            event_counter: 0,
            index_path,
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

    pub fn add_comment(
        &mut self,
        target: &CommentTarget,
        text: &str,
    ) -> anyhow::Result<(u64, PathBuf)> {
        let now = Utc::now().to_rfc3339();
        let comment = ReviewComment {
            id: self.next_id,
            target: target.clone(),
            text: text.trim().to_owned(),
            created_at: now.clone(),
            updated_at: now,
        };
        self.next_id = self.next_id.saturating_add(1);

        self.comments.push(comment.clone());
        self.save_index()?;

        let session = self.append_markdown_event("ADD", &comment, None)?;
        Ok((comment.id, session))
    }

    pub fn update_comment(&mut self, id: u64, text: &str) -> anyhow::Result<bool> {
        let Some(idx) = self.comments.iter().position(|comment| comment.id == id) else {
            return Ok(false);
        };

        self.comments[idx].text = text.trim().to_owned();
        self.comments[idx].updated_at = Utc::now().to_rfc3339();
        let snapshot = self.comments[idx].clone();
        self.save_index()?;
        self.append_markdown_event("EDIT", &snapshot, None)?;
        Ok(true)
    }

    pub fn delete_comment(&mut self, id: u64) -> anyhow::Result<bool> {
        let Some(idx) = self.comments.iter().position(|comment| comment.id == id) else {
            return Ok(false);
        };
        let removed = self.comments.remove(idx);
        self.save_index()?;
        self.append_markdown_event("DELETE", &removed, Some("Comment removed"))?;
        Ok(true)
    }

    fn save_index(&self) -> anyhow::Result<()> {
        let payload = serde_json::to_string_pretty(&self.comments)
            .context("failed to encode comments index")?;
        fs::write(&self.index_path, payload)
            .with_context(|| format!("failed to write {}", self.index_path.display()))?;
        Ok(())
    }

    fn append_markdown_event(
        &mut self,
        action: &str,
        comment: &ReviewComment,
        override_text: Option<&str>,
    ) -> anyhow::Result<PathBuf> {
        let file_path = if let Some(path) = &self.session_file {
            path.clone()
        } else {
            let ts = Utc::now().format("%Y%m%d-%H%M%S").to_string();
            let name = format!("{}-{}-review.md", ts, self.branch);
            let path = self.root.join(name);
            self.write_session_header(&path)?;
            self.session_file = Some(path.clone());
            path
        };

        self.event_counter = self.event_counter.saturating_add(1);
        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&file_path)
            .with_context(|| format!("failed to open {}", file_path.display()))?;

        writeln!(file, "## Event {} [{}]", self.event_counter, action)
            .context("failed to write event heading")?;
        writeln!(file, "- Time: {}", Utc::now().to_rfc3339()).context("failed to write time")?;
        writeln!(file, "- Comment ID: `#{}`", comment.id).context("failed to write id")?;
        writeln!(file, "- File: `{}`", comment.target.start.file_path)
            .context("failed to write file")?;
        writeln!(
            file,
            "- Commits: {}",
            comment
                .target
                .commits
                .iter()
                .map(|id| format!("`{}`", id))
                .collect::<Vec<_>>()
                .join(", ")
        )
        .context("failed to write commits")?;
        writeln!(
            file,
            "- Start: `{}` ({})",
            comment.target.start.hunk_header,
            format_anchor_lines(
                comment.target.start.old_lineno,
                comment.target.start.new_lineno
            )
        )
        .context("failed to write start")?;
        writeln!(
            file,
            "- End: `{}` ({})",
            comment.target.end.hunk_header,
            format_anchor_lines(comment.target.end.old_lineno, comment.target.end.new_lineno)
        )
        .context("failed to write end")?;
        writeln!(file).context("failed to write spacing")?;

        let text = override_text.unwrap_or(&comment.text);
        writeln!(file, "{}", text.trim()).context("failed to write text")?;

        if !comment.target.selected_lines.is_empty() {
            writeln!(file).context("failed to write spacing")?;
            writeln!(file, "```diff").context("failed to write code fence")?;
            for line in &comment.target.selected_lines {
                writeln!(file, "{}", line).context("failed to write selected line")?;
            }
            writeln!(file, "```").context("failed to close code fence")?;
        }

        writeln!(file).context("failed to write trailing newline")?;
        Ok(file_path)
    }

    fn write_session_header(&self, path: &Path) -> anyhow::Result<()> {
        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
            .with_context(|| format!("failed to create {}", path.display()))?;
        writeln!(file, "# Hunkr Review Session").context("failed to write title")?;
        writeln!(file, "- Started: {}", Utc::now().to_rfc3339())
            .context("failed to write started line")?;
        writeln!(file, "- Branch: `{}`", self.branch).context("failed to write branch")?;
        writeln!(file).context("failed to write spacing")?;
        Ok(())
    }
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
mod tests {
    use std::collections::BTreeSet;

    use tempfile::tempdir;

    use super::*;
    use crate::model::CommentAnchor;

    fn make_target() -> CommentTarget {
        let anchor = CommentAnchor {
            commit_id: "abc1234".to_string(),
            commit_summary: "add parser".to_string(),
            file_path: "src/lib.rs".to_string(),
            hunk_header: "@@ -1,3 +1,8 @@".to_string(),
            old_lineno: Some(1),
            new_lineno: Some(8),
        };
        CommentTarget {
            start: anchor.clone(),
            end: anchor,
            commits: BTreeSet::from(["abc1234".to_owned()]),
            selected_lines: vec!["+let x = 1;".to_owned(), "-let x = 0;".to_owned()],
        }
    }

    #[test]
    fn add_update_delete_roundtrip() {
        let tmp = tempdir().expect("tempdir");
        let mut store = CommentStore::new(tmp.path(), "feature/test").expect("new store");

        let (id, path) = store
            .add_comment(&make_target(), "Need better naming")
            .expect("add");
        assert!(path.exists());
        assert_eq!(store.comments().len(), 1);

        let updated = store.update_comment(id, "Renamed now").expect("update");
        assert!(updated);
        assert_eq!(store.comment_by_id(id).expect("id").text, "Renamed now");

        let deleted = store.delete_comment(id).expect("delete");
        assert!(deleted);
        assert!(store.comments().is_empty());

        let reloaded = CommentStore::new(tmp.path(), "feature/test").expect("reload");
        assert!(reloaded.comments().is_empty());
    }

    #[test]
    fn format_anchor_lines_works() {
        assert_eq!(format_anchor_lines(Some(1), Some(2)), "old 1 / new 2");
        assert_eq!(format_anchor_lines(None, None), "n/a");
    }
}
