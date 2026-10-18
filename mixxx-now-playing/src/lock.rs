//! The producer lock that marks one `mixxx-now-playing` process as running.
//!
//! ADR 0005 names the lock as the source of truth for producer liveness.
//! The producer holds an exclusive lock on `.producer.lock` in its drop
//! directory. It holds the lock for the life of the process.
//!
//! The publisher tests the same file with a non-blocking shared lock at each
//! health check. A missing lock file, or a free lock, means a missing
//! producer.
//!
//! ADR 0016 limits the wait for the exclusive lock to
//! [`LOCK_WAIT_LIMIT`]. A different producer can hold the lock past that
//! limit only when it is a second instance on the same drop directory. The
//! producer must stop and name the problem, not wait without a limit.

use std::fs::{File, OpenOptions, TryLockError};
use std::path::Path;
use std::thread;
use std::time::{Duration, Instant};

use anyhow::{Context, Result, anyhow};

/// The lock file name inside a producer's drop directory.
pub const LOCK_FILE_NAME: &str = ".producer.lock";

/// The longest time `acquire` waits for the exclusive lock (ADR 0016).
pub const LOCK_WAIT_LIMIT: Duration = Duration::from_secs(2);

/// The time between two tries of the exclusive lock.
const LOCK_RETRY_INTERVAL: Duration = Duration::from_millis(50);

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
    /// Takes the exclusive producer lock for `drop_dir`, within
    /// [`LOCK_WAIT_LIMIT`].
    ///
    /// This call creates `<drop_dir>/.producer.lock` when the file is
    /// absent. It sets file mode `0600` on creation. It then waits for an
    /// exclusive lock on the file, as `acquire_within` describes.
    ///
    /// # Errors
    ///
    /// This function returns an error when it cannot create or open the
    /// file, or when the lock does not come free within the limit (ADR
    /// 0016).
    pub fn acquire(drop_dir: &Path) -> Result<Self> {
        Self::acquire_within(drop_dir, LOCK_WAIT_LIMIT)
    }

    /// Takes the exclusive producer lock for `drop_dir`, within `limit`.
    ///
    /// This call creates `<drop_dir>/.producer.lock` when the file is
    /// absent. It sets file mode `0600` on creation. It then tries an
    /// exclusive lock on the file with [`File::try_lock`]. While a different
    /// handle holds the lock, this call sleeps for a short retry interval
    /// and tries again, until `limit` passes.
    ///
    /// The wait is deliberate, within its limit. The publisher holds a
    /// shared lock on this file for a short time during each health check.
    /// This call waits out that short hold. It does not wait without a
    /// limit, because a different producer can hold the lock (ADR 0016).
    ///
    /// # Errors
    ///
    /// This function returns an error when it cannot create or open the
    /// file, when the lock attempt fails for a reason other than the lock
    /// being held, or when the limit passes while a different handle still
    /// holds the lock. That last error names ADR 0016, `drop_dir` and the
    /// limit. It never says that the lock file is stale: the kernel
    /// releases a lock when its holder's process ends, so a held lock means
    /// a live holder.
    pub fn acquire_within(drop_dir: &Path, limit: Duration) -> Result<Self> {
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

        let deadline = Instant::now() + limit;
        loop {
            match file.try_lock() {
                Ok(()) => return Ok(Self { _file: file }),
                Err(TryLockError::Error(error)) => {
                    return Err(error)
                        .with_context(|| format!("lock producer lock file {}", path.display()));
                }
                Err(TryLockError::WouldBlock) => {
                    if Instant::now() >= deadline {
                        return Err(anyhow!(
                            "ADR 0016: a different producer holds the lock in {} after {:?}. \
                             Stop that producer, or use a different publisher instance.",
                            drop_dir.display(),
                            limit
                        ));
                    }
                    thread::sleep(LOCK_RETRY_INTERVAL);
                }
            }
        }
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

    /// ADR 0016: a producer that cannot get the lock stops with an error
    /// that names the ADR and the drop directory, within its limit.
    #[test]
    fn acquire_within_names_adr_0016_and_the_drop_directory_after_its_limit() -> Result<()> {
        let temp = TempDir::new()?;
        let holder = ProducerLock::acquire(temp.path())?;

        let started = Instant::now();
        let error = ProducerLock::acquire_within(temp.path(), Duration::from_millis(100))
            .expect_err("a second producer must not take a lock the first still holds");
        let elapsed = started.elapsed();

        let message = format!("{error:#}");
        assert!(message.contains("ADR 0016"), "error is: {message}");
        assert!(
            message.contains(&temp.path().display().to_string()),
            "error is: {message}"
        );
        assert!(elapsed < Duration::from_secs(1), "waited {elapsed:?}");

        drop(holder);
        Ok(())
    }

    /// ADR 0016 §The Producer Lock, rule 4: a limit must wait out the short
    /// shared lock of a publisher health check.
    #[test]
    fn acquire_within_waits_out_a_short_shared_lock() -> Result<()> {
        let temp = TempDir::new()?;
        let path = temp.path().join(LOCK_FILE_NAME);
        let shared = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(&path)
            .context("open lock file for the shared lock")?;
        shared.lock_shared().context("take the shared lock")?;

        let released = thread::spawn(move || {
            thread::sleep(Duration::from_millis(50));
            drop(shared);
        });

        let lock = ProducerLock::acquire_within(temp.path(), Duration::from_secs(1))?;

        released
            .join()
            .expect("the thread that releases the shared lock must not panic");
        drop(lock);
        Ok(())
    }

    #[test]
    fn lock_wait_limit_is_two_seconds() {
        assert_eq!(LOCK_WAIT_LIMIT, Duration::from_secs(2));
    }
}
