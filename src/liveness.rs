//! Producer liveness probing through the producer lock file.
//!
//! ADR 0005 names the producer lock as the source of truth for producer
//! liveness. `mixxx-now-playing` holds an exclusive lock on
//! `.producer.lock` in its drop directory for the life of its process (see
//! `mixxx-now-playing/src/lock.rs`). This module holds the publisher side of
//! that contract: a non-blocking probe of the same file.
//!
//! The probe never creates the lock file. Only the producer creates it.

use std::fmt;
use std::fs::{File, TryLockError};
use std::path::Path;

use anyhow::{Context, Result};

/// The lock file name inside a producer's drop directory.
///
/// This mirrors `mixxx-now-playing`'s `lock::LOCK_FILE_NAME`. The two crates
/// share no build dependency (AGENTS.md, Neighbors), so the publisher
/// repeats the file name instead of importing it.
pub const LOCK_FILE_NAME: &str = ".producer.lock";

/// Whether a producer process holds the drop-directory lock.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProducerState {
    /// A producer process holds the exclusive lock.
    Running,
    /// No producer process holds the exclusive lock.
    Missing,
}

impl fmt::Display for ProducerState {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let label = match self {
            Self::Running => "running",
            Self::Missing => "missing",
        };
        formatter.write_str(label)
    }
}

/// Probes whether a producer is running for `watch_dir`.
///
/// This opens `<watch_dir>/.producer.lock` read-only and tries a
/// non-blocking shared lock. When the lock attempt succeeds, no process
/// holds an exclusive lock, so the probe releases its shared lock at once
/// and reports [`ProducerState::Missing`]. When the attempt fails with
/// [`TryLockError::WouldBlock`], a producer holds the exclusive lock, and
/// the probe reports [`ProducerState::Running`]. A missing lock file also
/// reports [`ProducerState::Missing`].
///
/// This call never creates the lock file. Only the producer creates it.
///
/// # Errors
///
/// Returns an error when the lock file exists but cannot be opened, or when
/// the lock attempt fails for a reason other than the lock being held.
pub fn probe_producer(watch_dir: &Path) -> Result<ProducerState> {
    let path = watch_dir.join(LOCK_FILE_NAME);
    let file = match File::open(&path) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok(ProducerState::Missing);
        }
        Err(error) => {
            return Err(error)
                .with_context(|| format!("open producer lock file {}", path.display()));
        }
    };

    match file.try_lock_shared() {
        Ok(()) => {
            // Release the shared lock at once: this probe must not hold the
            // lock past the check. Dropping the only handle closes it, and
            // the kernel releases the lock on close.
            drop(file);
            Ok(ProducerState::Missing)
        }
        Err(TryLockError::WouldBlock) => Ok(ProducerState::Running),
        Err(TryLockError::Error(error)) => {
            Err(error).with_context(|| format!("lock producer lock file {}", path.display()))
        }
    }
}
