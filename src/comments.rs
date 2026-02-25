use std::{
    fs::{self, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
};

use anyhow::Context;
use chrono::Utc;

use crate::model::CommentAnchor;

const COMMENTS_DIR: &str = "comments";

/// Appends review comments into one markdown file per app session.
#[derive(Debug, Clone)]
pub struct CommentStore {
    root: PathBuf,
    branch: String,
    session_file: Option<PathBuf>,
    counter: u32,
}

impl CommentStore {
    pub fn new(project_data_dir: &Path, branch: &str) -> Self {
        Self {
            root: project_data_dir.join(COMMENTS_DIR),
            branch: sanitize(branch),
            session_file: None,
            counter: 0,
        }
    }

    pub fn append(&mut self, anchor: &CommentAnchor, text: &str) -> anyhow::Result<PathBuf> {
        fs::create_dir_all(&self.root)
            .with_context(|| format!("failed to create {}", self.root.display()))?;

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

        self.counter = self.counter.saturating_add(1);
        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&file_path)
            .with_context(|| format!("failed to open {}", file_path.display()))?;

        let now = Utc::now().to_rfc3339();
        let line_anchor = match (anchor.old_lineno, anchor.new_lineno) {
            (Some(old), Some(new)) => format!("old {} / new {}", old, new),
            (Some(old), None) => format!("old {}", old),
            (None, Some(new)) => format!("new {}", new),
            (None, None) => "n/a".to_owned(),
        };

        writeln!(file, "## Comment {}", self.counter).context("failed to write comment heading")?;
        writeln!(file, "- Time: {}", now).context("failed to write time")?;
        writeln!(
            file,
            "- Commit: `{}` - {}",
            anchor.commit_id, anchor.commit_summary
        )
        .context("failed to write commit")?;
        writeln!(file, "- File: `{}`", anchor.file_path).context("failed to write file path")?;
        writeln!(file, "- Hunk: `{}`", anchor.hunk_header).context("failed to write hunk")?;
        writeln!(file, "- Line: `{}`", line_anchor).context("failed to write line anchor")?;
        writeln!(file).context("failed to write spacing")?;
        writeln!(file, "{}", text.trim()).context("failed to write comment text")?;
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
    use tempfile::tempdir;

    use super::*;

    #[test]
    fn append_creates_one_session_file() {
        let tmp = tempdir().expect("tempdir");
        let mut store = CommentStore::new(tmp.path(), "feature/test");
        let anchor = CommentAnchor {
            commit_id: "abc1234".to_string(),
            commit_summary: "add parser".to_string(),
            file_path: "src/lib.rs".to_string(),
            hunk_header: "@@ -1,3 +1,8 @@".to_string(),
            old_lineno: Some(1),
            new_lineno: Some(8),
        };

        let first = store.append(&anchor, "Need better naming").expect("append");
        let second = store
            .append(&anchor, "Also split this function")
            .expect("append");

        assert_eq!(first, second);
        let content = fs::read_to_string(first).expect("read");
        assert!(content.contains("# Hunkr Review Session"));
        assert!(content.contains("## Comment 1"));
        assert!(content.contains("## Comment 2"));
        assert!(content.contains("Need better naming"));
    }
}
