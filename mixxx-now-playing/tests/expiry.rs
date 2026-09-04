use std::time::{Duration, Instant};

use anyhow::Result;
use mixxx_now_playing::expiry::{Expiry, expire_metadata_at};
use mixxx_now_playing::sink::{OutputFile, Presence};
use tempfile::TempDir;

#[test]
fn expiry_adds_slack_to_track_duration() {
    let started_at = Instant::now();
    let expiry = Expiry::duration(
        started_at,
        Some(Duration::from_secs(180)),
        Duration::from_secs(5),
        Duration::from_secs(600),
    );

    assert_eq!(
        expiry.deadline(),
        started_at.checked_add(Duration::from_secs(185))
    );
    assert!(!expiry.expired_at(started_at + Duration::from_secs(184)));
    assert!(expiry.expired_at(started_at + Duration::from_secs(185)));
}

#[test]
fn expiry_none_never_expires() {
    let started_at = Instant::now();
    let expiry = Expiry::none();

    assert_eq!(expiry.deadline(), None);
    assert!(!expiry.expired_at(started_at + Duration::from_secs(60 * 60)));
}

#[test]
fn expiry_uses_fallback_without_duration() {
    let started_at = Instant::now();
    let expiry = Expiry::duration(
        started_at,
        None,
        Duration::from_secs(5),
        Duration::from_secs(600),
    );

    assert_eq!(
        expiry.deadline(),
        started_at.checked_add(Duration::from_secs(600))
    );
}

#[test]
fn expiry_rearm_replaces_old_deadline() {
    let started_at = Instant::now();
    let old = Expiry::duration(
        started_at,
        Some(Duration::from_secs(180)),
        Duration::from_secs(5),
        Duration::from_secs(600),
    );
    let new_started_at = started_at + Duration::from_secs(30);
    let new = Expiry::duration(
        new_started_at,
        Some(Duration::from_secs(240)),
        Duration::from_secs(5),
        Duration::from_secs(600),
    );

    assert!(old.expired_at(started_at + Duration::from_secs(185)));
    assert!(!new.expired_at(started_at + Duration::from_secs(185)));
    assert_eq!(
        new.deadline(),
        started_at.checked_add(Duration::from_secs(275))
    );
}

#[test]
fn expiry_removes_metadata_after_deadline_without_new_track() -> Result<()> {
    let temp = TempDir::new()?;
    let metadata_path = temp.path().join("metadata.txt");
    let mut metadata = OutputFile::new(&metadata_path);
    metadata.set(Presence::Present("metadata".to_string()))?;

    let started_at = Instant::now();
    let mut expiry = Expiry::duration(
        started_at,
        Some(Duration::from_millis(50)),
        Duration::from_millis(10),
        Duration::from_secs(600),
    );

    expire_metadata_at(
        &mut expiry,
        &mut metadata,
        started_at + Duration::from_millis(59),
    )?;
    assert!(metadata_path.exists());

    expire_metadata_at(
        &mut expiry,
        &mut metadata,
        started_at + Duration::from_millis(60),
    )?;
    assert!(!metadata_path.exists());
    assert_eq!(expiry.deadline(), None);
    Ok(())
}
