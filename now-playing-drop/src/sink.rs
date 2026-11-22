use std::fs;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, anyhow};

/// Describes whether an output file has content.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Presence {
    /// Write this content when it differs from the current content.
    Present(String),
    /// Remove the output file if it exists.
    Absent,
}

/// Writes content to one file and skips unchanged writes.
#[derive(Debug)]
pub struct OutputFile {
    path: PathBuf,
    last: Option<String>,
}

impl OutputFile {
    /// Creates an output sink for `path`.
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self {
            path: path.into(),
            last: None,
        }
    }

    /// Applies the requested file presence.
    ///
    /// # Errors
    ///
    /// Returns an error when a write, rename, or removal fails.
    pub fn set(&mut self, presence: Presence) -> Result<()> {
        match presence {
            Presence::Present(content) if self.last.as_deref() != Some(&content) => {
                write_atomic(&self.path, content.as_bytes())?;
                self.last = Some(content);
                Ok(())
            }
            Presence::Present(_) => Ok(()),
            Presence::Absent => {
                remove_file_if_exists(&self.path)?;
                self.last = None;
                Ok(())
            }
        }
    }
}

/// Writes `bytes` to a temporary file in the same directory, then renames it
/// to `path` (AGENTS.md §6). After a failed rename, the temporary file is removed.
///
/// # Errors
///
/// Returns an error when the file name is not UTF-8, or when the write or the
/// rename fails.
pub fn write_atomic(path: &Path, bytes: &[u8]) -> Result<()> {
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    let name = path
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| anyhow!("output path has no UTF-8 file name: {}", path.display()))?;
    let temp_path = parent.join(format!(".{name}.{}.tmp", std::process::id()));
    fs::write(&temp_path, bytes)
        .with_context(|| format!("write temporary output {}", temp_path.display()))?;
    if let Err(error) = fs::rename(&temp_path, path) {
        let _ = fs::remove_file(&temp_path);
        return Err(error).with_context(|| {
            format!(
                "rename temporary output {} to {}",
                temp_path.display(),
                path.display()
            )
        });
    }
    Ok(())
}

/// Removes `path` and treats a missing file as success.
///
/// # Errors
///
/// Returns an error when the file cannot be removed.
pub fn remove_file_if_exists(path: &Path) -> Result<()> {
    match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error).with_context(|| format!("remove {}", path.display())),
    }
}

/// Writes `path` with no text, unless it already holds no text.
///
/// A shutdown guard uses this function. The guard holds only a path. It has
/// no live `OutputFile` cache. This function reads the file first. AGENTS.md
/// §6 forbids a rewrite when the content does not change. A missing file, or
/// any file this function cannot read, counts as not empty. This function
/// then writes it.
///
/// # Errors
///
/// Returns an error when the atomic write fails.
pub fn ensure_empty_file(path: &Path) -> Result<()> {
    let already_empty = fs::read(path)
        .map(|bytes| bytes.is_empty())
        .unwrap_or(false);
    if already_empty {
        return Ok(());
    }
    OutputFile::new(path).set(Presence::Present(String::new()))
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::time::SystemTime;

    use anyhow::Result;
    use tempfile::TempDir;

    use super::*;

    #[test]
    fn sink_writes_removes_and_ignores_redundant_content() -> Result<()> {
        let temp = TempDir::new()?;
        let path = temp.path().join("metadata.txt");
        let mut output = OutputFile::new(&path);

        output.set(Presence::Present("one".to_string()))?;
        assert_eq!(fs::read_to_string(&path)?, "one");
        let first_mtime = modified(&path)?;

        output.set(Presence::Present("one".to_string()))?;
        assert_eq!(modified(&path)?, first_mtime);

        output.set(Presence::Absent)?;
        assert!(!path.exists());
        output.set(Presence::Absent)?;
        Ok(())
    }

    fn modified(path: &Path) -> Result<SystemTime> {
        Ok(fs::metadata(path)?.modified()?)
    }

    #[test]
    fn sink_writes_empty_content_atomically_and_skips_redundant_write() -> Result<()> {
        let temp = TempDir::new()?;
        let path = temp.path().join("now-playing.txt");
        let mut output = OutputFile::new(&path);

        output.set(Presence::Present(String::new()))?;
        assert!(path.exists());
        assert_eq!(fs::read_to_string(&path)?, "");
        let first_mtime = modified(&path)?;

        output.set(Presence::Present(String::new()))?;
        assert_eq!(modified(&path)?, first_mtime);
        Ok(())
    }

    #[test]
    fn ensure_empty_file_writes_when_content_present() -> Result<()> {
        let temp = TempDir::new()?;
        let path = temp.path().join("now-playing.txt");
        fs::write(&path, "stale title")?;

        ensure_empty_file(&path)?;

        assert!(path.exists());
        assert_eq!(fs::read_to_string(&path)?, "");
        Ok(())
    }

    #[test]
    fn ensure_empty_file_skips_rewrite_when_already_empty() -> Result<()> {
        let temp = TempDir::new()?;
        let path = temp.path().join("now-playing.txt");
        fs::write(&path, "")?;
        let first_mtime = modified(&path)?;

        ensure_empty_file(&path)?;

        assert_eq!(modified(&path)?, first_mtime);
        Ok(())
    }

    #[test]
    fn ensure_empty_file_creates_missing_file() -> Result<()> {
        let temp = TempDir::new()?;
        let path = temp.path().join("now-playing.txt");
        assert!(!path.exists());

        ensure_empty_file(&path)?;

        assert!(path.exists());
        assert_eq!(fs::read_to_string(&path)?, "");
        Ok(())
    }
}
