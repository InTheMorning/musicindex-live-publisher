use std::collections::HashMap;
use std::fs;
use std::path::Path;
use std::time::{Duration, Instant};

use anyhow::Result;
use musicindex_live_publisher::{
    DropEvent, DropEventKind, DropWatcher, FallbackConfig, LiveValue, LiveValueDestination,
    LiveValueModel, LiveValuePayload, PublishSchedule, WatchTarget,
};
use serde_json::json;
use tempfile::TempDir;

const DELAY: Duration = Duration::from_secs(12);

fn target(name: &str, event_guid: &str) -> WatchTarget {
    WatchTarget {
        name: name.to_owned(),
        event_guid: event_guid.to_owned(),
        fallback: FallbackConfig {
            title: "Station".to_owned(),
            image: None,
            value: LiveValue {
                model: LiveValueModel {
                    kind: "lightning".to_owned(),
                    method: "keysend".to_owned(),
                    suggested: None,
                },
                destinations: vec![LiveValueDestination {
                    kind: Some("node".to_owned()),
                    name: Some("Station".to_owned()),
                    address: Some("03station".to_owned()),
                    split: Some("100".to_owned()),
                    custom_key: None,
                    custom_value: None,
                    fee: None,
                }],
            },
        },
    }
}

fn dropfile(target: &str, title: &str, track_guid: &str) -> String {
    json!({
        "schema": "musicindex.nowplaying/1",
        "target": target,
        "artist": "Alice",
        "title": title,
        "duration_secs": 187.326,
        "image": null,
        "feed_guid": "feed-guid",
        "track_guid": track_guid,
        "value_routes": [{
            "recipient_name": "Alice",
            "route_type": "node",
            "address": "03alice",
            "split": 90.0,
            "fee": false,
            "custom_key": null,
            "custom_value": null
        }],
        "value_routes_source": "embedded-id3"
    })
    .to_string()
}

fn write(path: &Path, content: impl AsRef<[u8]>) -> Result<()> {
    fs::write(path, content)?;
    Ok(())
}

fn upsert(path: &Path) -> DropEvent {
    DropEvent {
        kind: DropEventKind::Upsert,
        path: path.to_path_buf(),
    }
}

fn remove(path: &Path) -> DropEvent {
    DropEvent {
        kind: DropEventKind::Remove,
        path: path.to_path_buf(),
    }
}

fn schedule_for(delays: &[(&str, Duration)]) -> PublishSchedule {
    PublishSchedule::new(
        delays
            .iter()
            .map(|(event_guid, delay)| ((*event_guid).to_owned(), *delay))
            .collect::<HashMap<_, _>>(),
    )
}

fn titles(payloads: &[LiveValuePayload]) -> Vec<&str> {
    payloads
        .iter()
        .map(|payload| payload.title.as_str())
        .collect()
}

fn block_guids(payloads: &[LiveValuePayload]) -> Vec<&str> {
    payloads
        .iter()
        .map(|payload| payload.block_guid.as_str())
        .collect()
}

/// Drives one drop event through the watcher and into the schedule.
fn feed(
    watcher: &mut DropWatcher,
    schedule: &mut PublishSchedule,
    event: DropEvent,
    now: Instant,
) -> Result<()> {
    for payload in watcher.process_event(event, now)? {
        schedule.schedule(payload, now);
    }
    Ok(())
}

#[test]
fn schedule_zero_delay_releases_on_the_same_tick() -> Result<()> {
    let temp = TempDir::new()?;
    let path = temp.path().join("default.json");
    let mut watcher = DropWatcher::new(target("default", "event-default"), Duration::ZERO);
    let mut schedule = schedule_for(&[("event-default", Duration::ZERO)]);
    let now = Instant::now();

    write(&path, dropfile("default", "Track One", "track-1"))?;
    feed(&mut watcher, &mut schedule, upsert(&path), now)?;

    let due = schedule.take_due(now);
    assert_eq!(titles(&due), vec!["Track One"]);
    assert!(schedule.is_empty());
    assert_eq!(schedule.next_deadline(), None);
    Ok(())
}

#[test]
fn schedule_delayed_payload_is_withheld_until_its_deadline() -> Result<()> {
    let temp = TempDir::new()?;
    let path = temp.path().join("default.json");
    let mut watcher = DropWatcher::new(target("default", "event-default"), Duration::ZERO);
    let mut schedule = schedule_for(&[("event-default", DELAY)]);
    let now = Instant::now();

    write(&path, dropfile("default", "Track One", "track-1"))?;
    feed(&mut watcher, &mut schedule, upsert(&path), now)?;

    assert!(schedule.take_due(now).is_empty());
    assert!(
        schedule
            .take_due(now + DELAY - Duration::from_millis(1))
            .is_empty()
    );
    assert_eq!(schedule.next_deadline(), Some(now + DELAY));

    let due = schedule.take_due(now + DELAY);
    assert_eq!(titles(&due), vec!["Track One"]);
    assert!(schedule.is_empty());
    Ok(())
}

#[test]
fn schedule_removal_fallback_is_delayed_like_the_track() -> Result<()> {
    let temp = TempDir::new()?;
    let path = temp.path().join("default.json");
    let mut watcher = DropWatcher::new(target("default", "event-default"), Duration::ZERO);
    let mut schedule = schedule_for(&[("event-default", DELAY)]);
    let start = Instant::now();

    write(&path, dropfile("default", "Track One", "track-1"))?;
    feed(&mut watcher, &mut schedule, upsert(&path), start)?;
    assert_eq!(titles(&schedule.take_due(start + DELAY)), vec!["Track One"]);

    let removed_at = start + DELAY;
    fs::remove_file(&path)?;
    feed(&mut watcher, &mut schedule, remove(&path), removed_at)?;

    // The clear must not overtake the audio either: listeners are still hearing
    // the tail of the track for one full delay after the drop file goes away.
    assert!(
        schedule
            .take_due(removed_at + DELAY - Duration::from_millis(1))
            .is_empty()
    );
    let due = schedule.take_due(removed_at + DELAY);
    assert_eq!(titles(&due), vec!["Station"]);
    Ok(())
}

#[test]
fn schedule_two_tracks_inside_one_window_both_publish_in_order() -> Result<()> {
    let temp = TempDir::new()?;
    let path = temp.path().join("default.json");
    let mut watcher = DropWatcher::new(target("default", "event-default"), Duration::ZERO);
    let mut schedule = schedule_for(&[("event-default", DELAY)]);
    let start = Instant::now();
    let second_at = start + Duration::from_secs(4);

    write(&path, dropfile("default", "Track One", "track-1"))?;
    feed(&mut watcher, &mut schedule, upsert(&path), start)?;
    write(&path, dropfile("default", "Track Two", "track-2"))?;
    feed(&mut watcher, &mut schedule, upsert(&path), second_at)?;

    let first = schedule.take_due(start + DELAY);
    assert_eq!(titles(&first), vec!["Track One"]);
    assert!(!schedule.is_empty());

    let second = schedule.take_due(second_at + DELAY);
    assert_eq!(titles(&second), vec!["Track Two"]);
    assert_ne!(block_guids(&first), block_guids(&second));
    Ok(())
}

#[test]
fn schedule_route_upgrade_in_the_same_block_publishes_once_on_time() -> Result<()> {
    let temp = TempDir::new()?;
    let path = temp.path().join("default.json");
    let mut watcher = DropWatcher::new(target("default", "event-default"), Duration::ZERO);
    let mut schedule = schedule_for(&[("event-default", DELAY)]);
    let start = Instant::now();

    write(&path, dropfile("default", "Track One", "track-1"))?;
    feed(&mut watcher, &mut schedule, upsert(&path), start)?;

    // The producer rewrites the same track when the MusicIndex lookup upgrades
    // its routes. Same block, so the pending payload is replaced rather than
    // queued behind itself, and its deadline does not move.
    let upgraded = json!({
        "schema": "musicindex.nowplaying/1",
        "target": "default",
        "artist": "Alice",
        "title": "Track One",
        "duration_secs": 187.326,
        "image": null,
        "feed_guid": "feed-guid",
        "track_guid": "track-1",
        "value_routes": [{
            "recipient_name": "Alice",
            "route_type": "node",
            "address": "03alice",
            "split": 95.0,
            "fee": false,
            "custom_key": null,
            "custom_value": null
        }],
        "value_routes_source": "musicindex-api"
    })
    .to_string();
    write(&path, upgraded)?;
    feed(
        &mut watcher,
        &mut schedule,
        upsert(&path),
        start + Duration::from_secs(1),
    )?;

    assert_eq!(schedule.next_deadline(), Some(start + DELAY));
    let due = schedule.take_due(start + DELAY);
    assert_eq!(due.len(), 1);
    assert_eq!(
        due[0].value.destinations[0].split.as_deref(),
        Some("95"),
        "the released payload should be the upgraded one"
    );
    assert!(schedule.is_empty());
    Ok(())
}

#[test]
fn schedule_zero_delay_target_is_not_blocked_by_a_delayed_one() -> Result<()> {
    let temp = TempDir::new()?;
    let slow_path = temp.path().join("slow.json");
    let fast_path = temp.path().join("fast.json");
    let mut watcher = DropWatcher::new_targets(
        vec![target("slow", "event-slow"), target("fast", "event-fast")],
        Duration::ZERO,
    );
    let mut schedule = schedule_for(&[("event-slow", DELAY), ("event-fast", Duration::ZERO)]);
    let now = Instant::now();

    write(&slow_path, dropfile("slow", "Slow Track", "track-slow"))?;
    feed(&mut watcher, &mut schedule, upsert(&slow_path), now)?;
    write(&fast_path, dropfile("fast", "Fast Track", "track-fast"))?;
    feed(&mut watcher, &mut schedule, upsert(&fast_path), now)?;

    let due = schedule.take_due(now);
    assert_eq!(titles(&due), vec!["Fast Track"]);
    assert_eq!(schedule.next_deadline(), Some(now + DELAY));

    let due = schedule.take_due(now + DELAY);
    assert_eq!(titles(&due), vec!["Slow Track"]);
    Ok(())
}

#[test]
fn schedule_unknown_event_guid_is_not_held() -> Result<()> {
    let schedule_target = target("default", "event-unconfigured");
    let temp = TempDir::new()?;
    let path = temp.path().join("default.json");
    let mut watcher = DropWatcher::new(schedule_target, Duration::ZERO);
    let mut schedule = schedule_for(&[("event-other", DELAY)]);
    let now = Instant::now();

    write(&path, dropfile("default", "Track One", "track-1"))?;
    feed(&mut watcher, &mut schedule, upsert(&path), now)?;

    let due = schedule.take_due(now);
    assert_eq!(titles(&due), vec!["Track One"]);
    Ok(())
}

#[test]
fn schedule_take_due_on_an_empty_queue_is_empty() -> Result<()> {
    let mut schedule = schedule_for(&[("event-default", DELAY)]);

    assert!(schedule.take_due(Instant::now()).is_empty());
    assert_eq!(schedule.next_deadline(), None);
    assert!(schedule.is_empty());
    Ok(())
}

#[test]
fn schedule_holds_nothing_that_the_watcher_did_not_emit() -> Result<()> {
    let temp = TempDir::new()?;
    let path = temp.path().join("default.json");
    let mut watcher = DropWatcher::new(target("default", "event-default"), Duration::ZERO);
    let mut schedule = schedule_for(&[("event-default", DELAY)]);
    let now = Instant::now();

    write(&path, "{ not json")?;
    feed(&mut watcher, &mut schedule, upsert(&path), now)?;

    assert!(schedule.is_empty());
    assert!(
        schedule.take_due(now + DELAY).is_empty(),
        "a malformed drop file must not publish anything, delayed or otherwise"
    );
    Ok(())
}
