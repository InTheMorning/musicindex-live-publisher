use std::fs;
use std::path::Path;
use std::time::{Duration, Instant};

use anyhow::{Result, anyhow};
use musicindex_live_publisher::{
    DropEvent, DropEventKind, DropWatcher, ProducerState, WatchTarget, is_final_drop_file,
};
use serde_json::{Value, json};
use tempfile::TempDir;

fn target() -> WatchTarget {
    WatchTarget {
        name: "default".to_owned(),
        event_guid: "event-guid".to_owned(),
    }
}

fn dropfile(title: &str) -> String {
    json!({
        "schema": "musicindex.nowplaying/2",
        "target": "default",
        "artist": "Alice",
        "title": title,
        "duration_secs": 187.326,
        "image": "https://example.com/art.png",
        "feed_guid": "feed-guid",
        "track_guid": "track-guid",
        "album": null,
        "play_id": null,
        "value_routes": [{
            "recipient_name": "Alice",
            "route_type": "node",
            "address": "03alice",
            "split": 90.0,
            "fee": false,
            "custom_key": null,
            "custom_value": null
        }],
        "value_routes_source": "musicindex-api"
    })
    .to_string()
}

fn dropfile_with_routes(title: &str, routes: Value) -> String {
    let mut value: Value = serde_json::from_str(&dropfile(title)).expect("valid dropfile json");
    value["value_routes"] = routes;
    value.to_string()
}

fn write(path: &Path, content: impl AsRef<[u8]>) -> Result<()> {
    fs::write(path, content).map_err(Into::into)
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

fn one_payload(payloads: Vec<musicindex_live_publisher::LiveValuePayload>) -> Result<Value> {
    let payload = payloads
        .into_iter()
        .next()
        .ok_or_else(|| anyhow!("expected one payload"))?;
    Ok(serde_json::to_value(payload)?)
}

#[test]
fn watcher_create_modify_remove_emits_track_then_dead_block() -> Result<()> {
    let temp = TempDir::new()?;
    let path = temp.path().join("nowplaying.json");
    let mut watcher = DropWatcher::new(target(), Duration::from_millis(75));
    let start = Instant::now();

    write(&path, dropfile("First Track"))?;
    let first = one_payload(watcher.process_event(upsert(&path), start)?)?;
    let first_block = first["blockGuid"]
        .as_str()
        .ok_or_else(|| anyhow!("blockGuid should be string"))?
        .to_owned();
    assert_eq!(first["title"], "First Track");
    assert_eq!(first["value"]["destinations"][0]["split"], "90");

    write(&path, dropfile("Edited Track"))?;
    let second =
        one_payload(watcher.process_event(upsert(&path), start + Duration::from_millis(100))?)?;
    assert_eq!(second["title"], "Edited Track");
    assert_eq!(second["blockGuid"], first_block);

    fs::remove_file(&path)?;
    let dead_block =
        one_payload(watcher.process_event(remove(&path), start + Duration::from_millis(200))?)?;
    assert_eq!(dead_block["title"], "No V4V track playing");
    assert_eq!(dead_block["eventGuid"], "event-guid");
    assert_eq!(dead_block["value"]["destinations"][0]["split"], "100");
    assert!(dead_block.get("feedGuid").is_none());
    assert!(dead_block.get("itemGuid").is_none());
    assert_ne!(dead_block["blockGuid"], first_block);

    write(&path, dropfile("New Track"))?;
    let third =
        one_payload(watcher.process_event(upsert(&path), start + Duration::from_millis(300))?)?;
    assert_eq!(third["title"], "New Track");
    assert_ne!(third["blockGuid"], first_block);
    Ok(())
}

#[test]
fn watcher_two_dead_blocks_have_different_block_guids() -> Result<()> {
    let temp = TempDir::new()?;
    let path_a = temp.path().join("a.json");
    let path_b = temp.path().join("b.json");
    let mut watcher = DropWatcher::new(target(), Duration::from_millis(0));

    write(&path_a, dropfile("Track A"))?;
    watcher.process_event(upsert(&path_a), Instant::now())?;
    write(&path_b, dropfile("Track B"))?;
    watcher.process_event(upsert(&path_b), Instant::now())?;

    let dead_a = one_payload(watcher.process_event(remove(&path_a), Instant::now())?)?;
    let dead_b = one_payload(watcher.process_event(remove(&path_b), Instant::now())?)?;

    assert_eq!(dead_a["title"], "No V4V track playing");
    assert_eq!(dead_b["title"], "No V4V track playing");
    assert_ne!(dead_a["blockGuid"], dead_b["blockGuid"]);
    Ok(())
}

#[test]
fn watcher_empty_value_routes_publishes_dead_block() -> Result<()> {
    let temp = TempDir::new()?;
    let path = temp.path().join("nowplaying.json");
    let mut watcher = DropWatcher::new(target(), Duration::from_millis(0));

    write(&path, dropfile_with_routes("Silence", json!([])))?;
    let payload = one_payload(watcher.process_event(upsert(&path), Instant::now())?)?;

    assert_eq!(payload["title"], "No V4V track playing");
    assert_eq!(
        payload["value"]["destinations"].as_array().map(Vec::len),
        Some(1)
    );
    assert_eq!(
        payload["value"]["destinations"][0]["address"],
        "no-v4v-track@example.invalid"
    );
    assert!(payload.get("feedGuid").is_none());
    assert!(payload.get("itemGuid").is_none());
    Ok(())
}

#[test]
fn watcher_malformed_file_is_skipped_without_dead_block() -> Result<()> {
    let temp = TempDir::new()?;
    let path = temp.path().join("nowplaying.json");
    let mut watcher = DropWatcher::new(target(), Duration::from_millis(75));

    write(&path, br#"{"schema":"musicindex.nowplaying/1","target":"#)?;
    let payloads = watcher.process_event(upsert(&path), Instant::now())?;

    assert!(payloads.is_empty());
    Ok(())
}

#[test]
fn watcher_unknown_schema_is_skipped_without_dead_block() -> Result<()> {
    let temp = TempDir::new()?;
    let path = temp.path().join("nowplaying.json");
    let mut watcher = DropWatcher::new(target(), Duration::from_millis(75));

    write(
        &path,
        dropfile("Future Track").replace("musicindex.nowplaying/2", "musicindex.nowplaying/3"),
    )?;
    let payloads = watcher.process_event(upsert(&path), Instant::now())?;

    assert!(payloads.is_empty());
    Ok(())
}

#[test]
fn watcher_ignores_temp_file_and_rename_to_final_is_one_payload() -> Result<()> {
    let temp = TempDir::new()?;
    let temp_path = temp.path().join(".nowplaying.json.123.tmp");
    let final_path = temp.path().join("nowplaying.json");
    let mut watcher = DropWatcher::new(target(), Duration::from_millis(75));
    let start = Instant::now();

    write(&temp_path, dropfile("Renamed Track"))?;
    assert!(watcher.process_event(upsert(&temp_path), start)?.is_empty());

    fs::rename(&temp_path, &final_path)?;
    let payload = one_payload(
        watcher.process_event(upsert(&final_path), start + Duration::from_millis(100))?,
    )?;
    assert_eq!(payload["title"], "Renamed Track");
    Ok(())
}

#[test]
fn watcher_startup_running_with_existing_file_publishes_track() -> Result<()> {
    let temp = TempDir::new()?;
    let path = temp.path().join("nowplaying.json");
    let mut watcher = DropWatcher::new(target(), Duration::from_millis(75));

    write(&path, dropfile("Already Playing"))?;
    let payload = one_payload(watcher.startup_payloads(temp.path(), ProducerState::Running)?)?;

    assert_eq!(payload["title"], "Already Playing");
    Ok(())
}

#[test]
fn watcher_startup_running_with_empty_directory_publishes_dead_block() -> Result<()> {
    let temp = TempDir::new()?;
    let mut watcher = DropWatcher::new(target(), Duration::from_millis(75));

    let payload = one_payload(watcher.startup_payloads(temp.path(), ProducerState::Running)?)?;

    assert_eq!(payload["title"], "No V4V track playing");
    assert_eq!(
        payload["value"]["destinations"][0]["name"],
        "No V4V payment route"
    );
    Ok(())
}

#[test]
fn watcher_startup_missing_with_file_present_publishes_dead_block() -> Result<()> {
    let temp = TempDir::new()?;
    let path = temp.path().join("nowplaying.json");
    let mut watcher = DropWatcher::new(target(), Duration::from_millis(75));

    write(&path, dropfile("Left Behind"))?;
    let payload = one_payload(watcher.startup_payloads(temp.path(), ProducerState::Missing)?)?;

    assert_eq!(payload["title"], "No V4V track playing");
    Ok(())
}

#[test]
fn watcher_startup_fills_a_target_with_no_file_with_the_dead_block() -> Result<()> {
    let temp = TempDir::new()?;
    let present_path = temp.path().join("default.json");
    let other = WatchTarget {
        name: "other".to_owned(),
        event_guid: "event-guid-other".to_owned(),
    };
    let mut watcher =
        DropWatcher::new_targets(vec![target(), other.clone()], Duration::from_millis(75));

    write(&present_path, dropfile("Only Default Plays"))?;
    let mut payloads = watcher.startup_payloads(temp.path(), ProducerState::Running)?;
    payloads.sort_by(|a, b| a.event_guid.cmp(&b.event_guid));

    assert_eq!(payloads.len(), 2);
    let default_payload = payloads
        .iter()
        .find(|payload| payload.event_guid == "event-guid")
        .ok_or_else(|| anyhow!("expected a payload for the default target"))?;
    let other_payload = payloads
        .iter()
        .find(|payload| payload.event_guid == other.event_guid)
        .ok_or_else(|| anyhow!("expected a payload for the other target"))?;
    assert_eq!(default_payload.title, "Only Default Plays");
    assert_eq!(other_payload.title, "No V4V track playing");
    Ok(())
}

#[test]
fn watcher_scan_track_payloads_gives_no_dead_block_for_an_empty_directory() -> Result<()> {
    let temp = TempDir::new()?;
    let mut watcher = DropWatcher::new(target(), Duration::from_millis(75));

    let payloads = watcher.scan_track_payloads(temp.path())?;

    assert!(payloads.is_empty());
    Ok(())
}

#[test]
fn watcher_scan_track_payloads_gives_the_track_for_a_present_file() -> Result<()> {
    let temp = TempDir::new()?;
    let path = temp.path().join("nowplaying.json");
    let mut watcher = DropWatcher::new(target(), Duration::from_millis(75));

    write(&path, dropfile("Rescanned Track"))?;
    let payload = one_payload(watcher.scan_track_payloads(temp.path())?)?;

    assert_eq!(payload["title"], "Rescanned Track");
    Ok(())
}

#[test]
fn watcher_producer_missing_payloads_gives_one_dead_block_for_each_target() -> Result<()> {
    let other = WatchTarget {
        name: "other".to_owned(),
        event_guid: "event-guid-other".to_owned(),
    };
    let mut watcher =
        DropWatcher::new_targets(vec![target(), other.clone()], Duration::from_millis(75));

    let mut payloads = watcher.producer_missing_payloads();
    payloads.sort_by(|a, b| a.event_guid.cmp(&b.event_guid));

    assert_eq!(payloads.len(), 2);
    assert!(
        payloads
            .iter()
            .all(|payload| payload.title == "No V4V track playing")
    );
    assert_eq!(payloads[0].event_guid, "event-guid");
    assert_eq!(payloads[1].event_guid, other.event_guid);
    Ok(())
}

#[test]
fn watcher_producer_missing_payloads_clears_block_state_for_a_reused_path() -> Result<()> {
    let temp = TempDir::new()?;
    let path = temp.path().join("nowplaying.json");
    let mut watcher = DropWatcher::new(target(), Duration::from_millis(0));

    write(&path, dropfile("Before Producer Left"))?;
    let before = one_payload(watcher.process_event(upsert(&path), Instant::now())?)?;
    let before_block = before["blockGuid"]
        .as_str()
        .ok_or_else(|| anyhow!("blockGuid should be a string"))?
        .to_owned();

    watcher.producer_missing_payloads();

    write(&path, dropfile("Before Producer Left"))?;
    let after = one_payload(watcher.process_event(upsert(&path), Instant::now())?)?;

    assert_ne!(after["blockGuid"], before_block);
    Ok(())
}

#[test]
fn watcher_debounces_same_action_on_same_path() -> Result<()> {
    let temp = TempDir::new()?;
    let path = temp.path().join("nowplaying.json");
    let mut watcher = DropWatcher::new(target(), Duration::from_millis(75));
    let start = Instant::now();

    write(&path, dropfile("Debounced Track"))?;
    assert_eq!(watcher.process_event(upsert(&path), start)?.len(), 1);
    assert!(
        watcher
            .process_event(upsert(&path), start + Duration::from_millis(20))?
            .is_empty()
    );
    write(&path, dropfile("Debounced Track Edited"))?;
    assert_eq!(
        watcher
            .process_event(upsert(&path), start + Duration::from_millis(80))?
            .len(),
        1
    );
    Ok(())
}

#[test]
fn watcher_final_drop_file_rule_accepts_only_visible_json_files() {
    assert!(is_final_drop_file(Path::new("nowplaying.json")));
    assert!(!is_final_drop_file(Path::new(".nowplaying.json.123.tmp")));
    assert!(!is_final_drop_file(Path::new("nowplaying.tmp")));
}

fn dropfile_with_guid(title: &str, track_guid: &str) -> String {
    let mut value: Value = serde_json::from_str(&dropfile(title)).expect("valid dropfile json");
    value["track_guid"] = json!(track_guid);
    value.to_string()
}

fn dropfile_with_api_routes(title: &str, track_guid: &str) -> String {
    let mut value: Value =
        serde_json::from_str(&dropfile_with_guid(title, track_guid)).expect("valid dropfile json");
    value["value_routes"] = json!([{
        "recipient_name": "Alice",
        "route_type": "node",
        "address": "03alice-from-api",
        "split": 95.0,
        "fee": false,
        "custom_key": null,
        "custom_value": null
    }]);
    value["value_routes_source"] = json!("musicindex-api");
    value.to_string()
}

fn dropfile_with_routes_source(title: &str, track_guid: &str, source: &str) -> String {
    let mut value = serde_json::from_str::<Value>(&dropfile_with_guid(title, track_guid))
        .expect("valid dropfile json");
    value["value_routes_source"] = json!(source);
    value.to_string()
}

#[test]
fn watcher_next_track_at_same_path_gets_a_new_block_guid() -> Result<()> {
    let temp = TempDir::new()?;
    let path = temp.path().join("default.json");
    let mut watcher = DropWatcher::new(target(), Duration::from_millis(0));

    write(&path, dropfile_with_guid("Track One", "guid-one"))?;
    let first = watcher.process_event(upsert(&path), Instant::now())?;

    write(&path, dropfile_with_guid("Track Two", "guid-two"))?;
    let second = watcher.process_event(upsert(&path), Instant::now())?;

    assert_eq!(first[0].title, "Track One");
    assert_eq!(second[0].title, "Track Two");
    assert_ne!(first[0].block_guid, second[0].block_guid);
    assert_eq!(first[0].event_guid, second[0].event_guid);
    Ok(())
}

#[test]
fn watcher_same_track_rewritten_keeps_its_block_guid() -> Result<()> {
    let temp = TempDir::new()?;
    let path = temp.path().join("default.json");
    let mut watcher = DropWatcher::new(target(), Duration::from_millis(0));

    write(&path, dropfile_with_guid("Track One", "guid-one"))?;
    let first = watcher.process_event(upsert(&path), Instant::now())?;

    write(&path, dropfile_with_api_routes("Track One", "guid-one"))?;
    let second = watcher.process_event(upsert(&path), Instant::now())?;

    assert_eq!(first[0].block_guid, second[0].block_guid);
    Ok(())
}

#[test]
fn watcher_same_track_identical_payload_rewrite_is_skipped() -> Result<()> {
    let temp = TempDir::new()?;
    let path = temp.path().join("default.json");
    let mut watcher = DropWatcher::new(target(), Duration::from_millis(0));

    write(
        &path,
        dropfile_with_routes_source("Track One", "guid-one", "embedded-id3"),
    )?;
    let first = watcher.process_event(upsert(&path), Instant::now())?;

    write(
        &path,
        dropfile_with_routes_source("Track One", "guid-one", "musicindex-api"),
    )?;
    let second = watcher.process_event(upsert(&path), Instant::now())?;

    assert_eq!(first.len(), 1);
    assert!(
        second.is_empty(),
        "a source-only rewrite transforms to the same relay payload"
    );
    Ok(())
}

#[test]
fn watcher_value_route_upgrade_for_one_track_stays_in_the_same_block() -> Result<()> {
    let temp = TempDir::new()?;
    let path = temp.path().join("default.json");
    let mut watcher = DropWatcher::new(target(), Duration::from_millis(0));

    // The producer's first write carries embedded ID3 routes.
    write(&path, dropfile_with_guid("Track One", "guid-one"))?;
    let embedded = watcher.process_event(upsert(&path), Instant::now())?;

    // Its second write replaces them with authoritative MusicIndex API routes.
    write(&path, dropfile_with_api_routes("Track One", "guid-one"))?;
    let upgraded = watcher.process_event(upsert(&path), Instant::now())?;

    assert_eq!(embedded[0].block_guid, upgraded[0].block_guid);
    assert_ne!(
        embedded[0].value.destinations[0].address,
        upgraded[0].value.destinations[0].address
    );
    Ok(())
}

#[test]
fn watcher_track_without_guid_uses_artist_and_title_for_block_identity() -> Result<()> {
    let temp = TempDir::new()?;
    let path = temp.path().join("default.json");
    let mut watcher = DropWatcher::new(target(), Duration::from_millis(0));

    let untagged = |title: &str| -> String {
        let mut value: Value = serde_json::from_str(&dropfile(title)).expect("valid dropfile json");
        value["track_guid"] = Value::Null;
        value.to_string()
    };

    write(&path, untagged("Track One"))?;
    let first = watcher.process_event(upsert(&path), Instant::now())?;
    write(&path, untagged("Track One"))?;
    let same = watcher.process_event(upsert(&path), Instant::now())?;
    write(&path, untagged("Track Two"))?;
    let next = watcher.process_event(upsert(&path), Instant::now())?;

    assert!(same.is_empty());
    assert_ne!(first[0].block_guid, next[0].block_guid);
    Ok(())
}

#[test]
fn watcher_track_returning_after_removal_gets_a_new_block_guid() -> Result<()> {
    let temp = TempDir::new()?;
    let path = temp.path().join("default.json");
    let mut watcher = DropWatcher::new(target(), Duration::from_millis(0));

    write(&path, dropfile_with_guid("Track One", "guid-one"))?;
    let first = watcher.process_event(upsert(&path), Instant::now())?;
    fs::remove_file(&path)?;
    watcher.process_event(remove(&path), Instant::now())?;

    write(&path, dropfile_with_guid("Track One", "guid-one"))?;
    let replayed = watcher.process_event(upsert(&path), Instant::now())?;

    assert_ne!(first[0].block_guid, replayed[0].block_guid);
    Ok(())
}

#[test]
fn watcher_a_remove_after_a_rewrite_inside_the_window_gives_the_dead_block() -> Result<()> {
    // Live failure, 2026-10-06: the producer removed the drop file, wrote the
    // same track again, and removed it again, in less than 75 ms. The second
    // remove must give the dead block, or the old track stays live.
    let temp = TempDir::new()?;
    let path = temp.path().join("nowplaying.json");
    let mut watcher = DropWatcher::new(target(), Duration::from_millis(75));
    let start = Instant::now();

    write(&path, dropfile("Old Track"))?;
    one_payload(watcher.process_event(upsert(&path), start)?)?;

    fs::remove_file(&path)?;
    let dead = one_payload(watcher.process_event(remove(&path), start + Duration::from_secs(60))?)?;
    assert_eq!(dead["title"], "No V4V track playing");

    write(&path, dropfile("Old Track"))?;
    let again = one_payload(watcher.process_event(
        upsert(&path),
        start + Duration::from_secs(60) + Duration::from_millis(10),
    )?)?;
    assert_eq!(again["title"], "Old Track");

    fs::remove_file(&path)?;
    let last = one_payload(watcher.process_event(
        remove(&path),
        start + Duration::from_secs(60) + Duration::from_millis(20),
    )?)?;
    assert_eq!(last["title"], "No V4V track playing");
    Ok(())
}
