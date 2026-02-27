use std::{
    collections::hash_map::RandomState,
    fs::{self, OpenOptions},
    hash::{BuildHasher, Hasher},
    io::Write,
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

use anyhow::Context;

/// Writes a UTF-8 file atomically by writing to a temp file then renaming into place.
pub(crate) fn atomic_write_text(path: &Path, contents: &str) -> anyhow::Result<()> {
    atomic_write_bytes(path, contents.as_bytes())
}

/// Writes bytes atomically by writing to a temp file then renaming into place.
pub(crate) fn atomic_write_bytes(path: &Path, contents: &[u8]) -> anyhow::Result<()> {
    let parent = path
        .parent()
        .context("cannot atomically write a path without a parent directory")?;
    fs::create_dir_all(parent).with_context(|| format!("failed to create {}", parent.display()))?;

    let temp_path = reserve_temp_path(path)?;
    let write_result = (|| -> anyhow::Result<()> {
        let mut file = OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&temp_path)
            .with_context(|| format!("failed to create {}", temp_path.display()))?;
        file.write_all(contents)
            .with_context(|| format!("failed to write {}", temp_path.display()))?;
        file.sync_all()
            .with_context(|| format!("failed to sync {}", temp_path.display()))?;
        Ok(())
    })();

    if let Err(err) = write_result {
        let _ = fs::remove_file(&temp_path);
        return Err(err);
    }

    fs::rename(&temp_path, path).with_context(|| {
        format!(
            "failed to move {} to {}",
            temp_path.display(),
            path.display()
        )
    })
}

fn reserve_temp_path(target: &Path) -> anyhow::Result<PathBuf> {
    let file_name = target
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .context("cannot atomically write a path without a file name")?;

    for _ in 0..32 {
        let temp_path = target.with_file_name(format!("{file_name}.{}.tmp", random_id()));
        if !temp_path.exists() {
            return Ok(temp_path);
        }
    }

    Err(anyhow::anyhow!(
        "failed to reserve temp path for {}",
        target.display()
    ))
}

fn random_id() -> String {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let thread_entropy = u128::from(RandomState::new().hash_one(std::thread::current().id()));
    let pid = u128::from(std::process::id());
    let entropy = now ^ (thread_entropy << 1) ^ (pid << 33);

    let mut hasher = RandomState::new().build_hasher();
    hasher.write_u128(entropy);
    format!("{:016x}", hasher.finish())
}

#[cfg(test)]
mod tests {
    use tempfile::tempdir;

    use super::*;

    #[test]
    fn atomic_write_text_replaces_existing_file() {
        let tmp = tempdir().expect("tempdir");
        let path = tmp.path().join("state.json");

        atomic_write_text(&path, "first").expect("first write");
        atomic_write_text(&path, "second").expect("second write");

        let actual = fs::read_to_string(path).expect("read");
        assert_eq!(actual, "second");
    }

    #[test]
    fn atomic_write_text_uses_temp_suffix_but_leaves_no_temp_files() {
        let tmp = tempdir().expect("tempdir");
        let path = tmp.path().join("shell-history.json");

        atomic_write_text(&path, "[]").expect("write");

        let entries = fs::read_dir(tmp.path())
            .expect("read_dir")
            .map(|entry| {
                entry
                    .expect("entry")
                    .file_name()
                    .to_string_lossy()
                    .into_owned()
            })
            .collect::<Vec<_>>();

        assert_eq!(entries, vec!["shell-history.json".to_owned()]);
    }
}
