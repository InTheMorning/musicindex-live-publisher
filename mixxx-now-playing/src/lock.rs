//! The producer lock that marks one `mixxx-now-playing` process as running.
//!
//! ADR 0005 names the lock as the source of truth for producer liveness.
//! The producer holds an exclusive lock on `.producer.lock` in its drop
//! directory. It holds the lock for the life of the process.
//!
//! The publisher tests the same file with a non-blocking shared lock at each
//! health check. A missing lock file, or a free lock, means a missing
//! producer.

use std::fs::{File, OpenOptions};
use std::path::Path;

use anyhow::{Context, Result};

/// The lock file name inside a producer's drop directory.
pub const LOCK_FILE_NAME: &str = ".producer.lock";

/// An exclusive hold on a producer's drop-directory lock file.
///
/// The lock lasts as long as this value stays alive. The kernel releases the
/// lock when the last file handle closes.
///
/// The value never deletes the lock file on drop. A delete could race with a
/// new producer that opens the same path. The file stays in place for the
/// life of the drop directory.
#[derive(Debug)]
pub struct ProducerLock {
    // Never read after construction. It exists only to outlive `acquire`.
    // The OS keeps the lock as long as this value stays alive.
    _file: File,
}

impl ProducerLock {
    /// Takes the exclusive producer lock for `drop_dir`
    ///
    /// This call creates `<drop_dir>/.producer.lock` when the file is
    /// absent. It sets file mode `0600` on creation. It then waits for an
    /// exclusive lock on the file.
    ///
    /// The wait is deliberate. The publisher holds a shared lock on this
    /// file for a short time during each health check. A producer must wait
    /// out that short hold. It must not fail.
    ///
    /// # Errors
    ///
    /// This function returns an error when it cannot create, open, or lock
    /// the file.
    pub fn acquire(drop_dir: &Path) -> Result<Self> {
        let path = drop_dir.join(LOCK_FILE_NAME);
        let mut options = OpenOptions::new();
        options.read(true).write(true).create(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }

        let file = options
            .open(&path)
            .with_context(|| format!("open producer lock file {}", path.display()))?;
        file.lock()
            .with_context(|| format!("lock producer lock file {}", path.display()))?;

        Ok(Self { _file: file })
    }
}

#[cfg(test)]
mod tests {
    use std::fs::{OpenOptions, TryLockError};

    use tempfile::TempDir;

    use super::*;

    #[test]
    fn acquire_blocks_a_second_open_file_with_try_lock_shared() -> Result<()> {
        let temp = TempDir::new()?;
        let lock = ProducerLock::acquire(temp.path())?;

        let second = OpenOptions::new()
            .read(true)
            .write(true)
            .open(temp.path().join(LOCK_FILE_NAME))
            .context("open second handle to lock file")?;

        match second.try_lock_shared() {
            Err(TryLockError::WouldBlock) => {}
            Err(TryLockError::Error(error)) => {
                panic!("unexpected lock error: {error}");
            }
            Ok(()) => panic!("second handle should not take the lock while the first holds it"),
        }

        drop(lock);
        Ok(())
    }

    #[test]
    fn lock_is_free_after_the_producer_lock_drops() -> Result<()> {
        let temp = TempDir::new()?;
        let lock = ProducerLock::acquire(temp.path())?;
        drop(lock);

        let second = OpenOptions::new()
            .read(true)
            .write(true)
            .open(temp.path().join(LOCK_FILE_NAME))
            .context("open second handle to lock file")?;

        second
            .try_lock_shared()
            .context("second handle should take the lock once the first is gone")?;
        Ok(())
    }
}
