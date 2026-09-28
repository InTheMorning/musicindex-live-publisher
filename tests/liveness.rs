use std::fs::File;

use anyhow::{Context, Result};
use musicindex_live_publisher::{LOCK_FILE_NAME, ProducerState, probe_producer};
use tempfile::TempDir;

#[test]
fn probe_with_no_lock_file_gives_missing() -> Result<()> {
    let temp = TempDir::new()?;

    assert_eq!(probe_producer(temp.path())?, ProducerState::Missing);
    Ok(())
}

#[test]
fn probe_with_a_lock_file_with_no_holder_gives_missing() -> Result<()> {
    let temp = TempDir::new()?;
    File::create(temp.path().join(LOCK_FILE_NAME))?;

    assert_eq!(probe_producer(temp.path())?, ProducerState::Missing);
    Ok(())
}

#[test]
fn probe_with_an_exclusive_lock_from_a_second_open_file_gives_running() -> Result<()> {
    let temp = TempDir::new()?;
    let path = temp.path().join(LOCK_FILE_NAME);
    let holder = File::create(&path)?;
    holder
        .lock()
        .context("hold the exclusive lock in the test")?;

    assert_eq!(probe_producer(temp.path())?, ProducerState::Running);

    drop(holder);
    Ok(())
}

#[test]
fn probe_never_creates_the_lock_file() -> Result<()> {
    let temp = TempDir::new()?;

    probe_producer(temp.path())?;

    assert!(!temp.path().join(LOCK_FILE_NAME).exists());
    Ok(())
}
