//! The lock that limits Mixxx to one producer per user session.

use std::error::Error;
use std::fmt;
use std::fs::File;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::{Context, Result};
use now_playing_drop::{LOCK_WAIT_LIMIT, open_lock_file, wait_for_lock};

/// The directory name of the Mixxx lock inside the runtime directory.
pub const MIXXX_LOCK_DIR_NAME: &str = "mixxx-now-playing";

/// The file name of the Mixxx lock.
pub const MIXXX_LOCK_FILE_NAME: &str = "mixxx.lock";

/// The error of a Mixxx lock that a different producer holds (ADR 0021).
///
/// The process maps this error to exit status 3.
#[derive(Debug)]
pub struct MixxxLockHeld {
    /// The holder name that the holder wrote into the lock file.
    pub holder: String,
    /// The path of the lock file.
    pub path: PathBuf,
}

impl fmt::Display for MixxxLockHeld {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // A producer without `--instance` writes its drop directory, which
        // is an absolute path, as the holder name.
        if self.holder.starts_with('/') {
            write!(
                f,
                "ADR 0021: the Mixxx producer with the drop directory {} holds the Mixxx lock in {}. \
                 Stop that producer first, then start this instance.",
                self.holder,
                self.path.display()
            )
        } else {
            write!(
                f,
                "ADR 0021: the Mixxx producer of instance {} holds the Mixxx lock in {}. \
                 Stop mixxx-now-playing@{}.service first, then start this instance.",
                self.holder,
                self.path.display(),
                self.holder
            )
        }
    }
}

impl Error for MixxxLockHeld {}

/// An exclusive hold on the Mixxx lock file (ADR 0021 §One Mixxx Producer
/// At A Time).
///
/// The lock lasts as long as this value stays alive. The value never
/// deletes the lock file.
#[derive(Debug)]
pub struct MixxxLock {
    // Never read after construction. The OS keeps the lock as long as this
    // value stays alive.
    _file: File,
}

impl MixxxLock {
    /// Takes the Mixxx lock under `runtime_dir`, within [`LOCK_WAIT_LIMIT`].
    ///
    /// # Errors
    ///
    /// See [`MixxxLock::acquire_within`].
    pub fn acquire(runtime_dir: &Path, holder: &str) -> Result<Self> {
        Self::acquire_within(runtime_dir, holder, LOCK_WAIT_LIMIT)
    }

    /// Takes the Mixxx lock under `runtime_dir`, within `limit`.
    ///
    /// This call creates `<runtime_dir>/mixxx-now-playing` with mode `0700`
    /// and `mixxx.lock` with mode `0600` when they are absent. After it
    /// gets the lock, it truncates the file and writes `holder` into it.
    ///
    /// # Errors
    ///
    /// Returns a [`MixxxLockHeld`] error, with the holder name read from
    /// the file, when a different handle holds the lock after `limit`.
    /// Returns a different error when the directory or file cannot be
    /// created or opened, or when the lock attempt fails.
    pub fn acquire_within(runtime_dir: &Path, holder: &str, limit: Duration) -> Result<Self> {
        let dir = runtime_dir.join(MIXXX_LOCK_DIR_NAME);
        create_private_dir(&dir)?;
        let path = dir.join(MIXXX_LOCK_FILE_NAME);
        let mut file = open_lock_file(&path, "Mixxx lock file")?;

        if !wait_for_lock(&file, &path, limit)? {
            // Reading is safe: the lock is advisory, and the holder wrote
            // the name after it got the lock.
            let mut text = String::new();
            let read = file.read_to_string(&mut text);
            let holder = match read {
                Ok(_) if !text.trim().is_empty() => text.trim().to_owned(),
                _ => "unknown".to_owned(),
            };
            return Err(MixxxLockHeld { holder, path }.into());
        }

        file.set_len(0)
            .with_context(|| format!("truncate Mixxx lock file {}", path.display()))?;
        file.write_all(holder.as_bytes())
            .with_context(|| format!("write holder name to {}", path.display()))?;
        Ok(Self { _file: file })
    }
}

/// Creates `dir` with mode `0700` when it is absent.
fn create_private_dir(dir: &Path) -> Result<()> {
    let mut builder = std::fs::DirBuilder::new();
    builder.recursive(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        builder.mode(0o700);
    }
    builder
        .create(dir)
        .with_context(|| format!("create Mixxx lock directory {}", dir.display()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use anyhow::Result;
    use std::time::{Duration, Instant};
    use tempfile::TempDir;

    /// ADR 0021 §One Mixxx Producer At A Time: a second Mixxx lock fails
    /// after the limit and names the holder that the first lock wrote.
    #[test]
    fn mixxx_lock_second_taker_fails_and_names_the_holder() -> Result<()> {
        let temp = TempDir::new()?;
        let first = MixxxLock::acquire(temp.path(), "show")?;

        let started = Instant::now();
        let error = MixxxLock::acquire_within(temp.path(), "other", Duration::from_millis(100))
            .expect_err("a second Mixxx lock must not succeed while the first holds it");
        assert!(started.elapsed() < Duration::from_secs(1));

        let held = error
            .downcast_ref::<MixxxLockHeld>()
            .expect("the error must be a MixxxLockHeld");
        assert_eq!(held.holder, "show");
        let message = format!("{error:#}");
        assert!(message.contains("ADR 0021"), "error is: {message}");
        assert!(message.contains("show"), "error is: {message}");

        drop(first);
        // The file stays and the lock is free again.
        MixxxLock::acquire(temp.path(), "other")?;
        Ok(())
    }

    #[test]
    fn mixxx_lock_truncates_the_old_holder_and_uses_private_modes() -> Result<()> {
        use std::os::unix::fs::PermissionsExt;

        let temp = TempDir::new()?;
        drop(MixxxLock::acquire(temp.path(), "a-long-holder-name")?);
        let _second = MixxxLock::acquire(temp.path(), "b")?;

        let dir = temp.path().join(MIXXX_LOCK_DIR_NAME);
        let file = dir.join(MIXXX_LOCK_FILE_NAME);
        assert_eq!(std::fs::read_to_string(&file)?, "b");
        assert_eq!(
            std::fs::metadata(&file)?.permissions().mode() & 0o777,
            0o600
        );
        assert_eq!(std::fs::metadata(&dir)?.permissions().mode() & 0o777, 0o700);
        Ok(())
    }
}
