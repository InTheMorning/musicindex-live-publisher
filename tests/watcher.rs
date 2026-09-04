use std::fs;
use std::path::Path;
use std::time::{Duration, Instant};

use anyhow::{Result, anyhow};
use musicindex_live_publisher::{
    DropEvent, DropEventKind, DropWatcher, FallbackConfig, LiveValueDestination, WatchTarget,
    is_final_drop_file,
};
use serde_json::{Value, json};
use tempfile::TempDir;

fn target() -> WatchTarget {
    WatchTarget {
        name: "default".to_owned(),
        event_guid: "event-guid".to_owned(),
        fallback: FallbackConfig {
            title: "Station".to_owned(),
            image: None,
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
    }
}

fn dropfile(title: &str) -> String {
    json!({
        "schema": "musicindex.nowplaying/1",
        "target": "default",
        "artist": "Alice",
        "title": title,
        "duration_secs": 187.326,
        "image": "https://example.com/art.png",
        "feed_guid": "feed-guid",
        "track_guid": "track-guid",
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
fn watcher_create_modify_remove_emits_track_then_fallback() -> Result<()> {
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
    let fallback =
        one_payload(watcher.process_event(remove(&path), start + Duration::from_millis(200))?)?;
    assert_eq!(fallback["title"], "Station");
    assert_eq!(fallback["eventGuid"], "event-guid");
    assert_eq!(fallback["value"]["destinations"][0]["split"], "100");
    assert_ne!(fallback["blockGuid"], first_block);

    write(&path, dropfile("New Track"))?;
    let third =
        one_payload(watcher.process_event(upsert(&path), start + Duration::from_millis(300))?)?;
    assert_eq!(third["title"], "New Track");
    assert_ne!(third["blockGuid"], first_block);
    Ok(())
}

#[test]
fn watcher_malformed_file_is_skipped_without_fallback() -> Result<()> {
    let temp = TempDir::new()?;
    let path = temp.path().join("nowplaying.json");
    let mut watcher = DropWatcher::new(target(), Duration::from_millis(75));

    write(&path, br#"{"schema":"musicindex.nowplaying/1","target":"#)?;
    let payloads = watcher.process_event(upsert(&path), Instant::now())?;

    assert!(payloads.is_empty());
    Ok(())
}

#[test]
fn watcher_unknown_schema_is_skipped_without_fallback() -> Result<()> {
    let temp = TempDir::new()?;
    let path = temp.path().join("nowplaying.json");
    let mut watcher = DropWatcher::new(target(), Duration::from_millis(75));

    write(
        &path,
        dropfile("Future Track").replace("musicindex.nowplaying/1", "musicindex.nowplaying/2"),
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
fn watcher_initial_state_with_existing_file_publishes_track() -> Result<()> {
    let temp = TempDir::new()?;
    let path = temp.path().join("nowplaying.json");
    let mut watcher = DropWatcher::new(target(), Duration::from_millis(75));

    write(&path, dropfile("Already Playing"))?;
    let payload = one_payload(watcher.initial_payloads(temp.path())?)?;

    assert_eq!(payload["title"], "Already Playing");
    Ok(())
}

#[test]
fn watcher_initial_state_empty_directory_publishes_fallback() -> Result<()> {
    let temp = TempDir::new()?;
    let mut watcher = DropWatcher::new(target(), Duration::from_millis(75));

    let payload = one_payload(watcher.initial_payloads(temp.path())?)?;

    assert_eq!(payload["title"], "Station");
    assert_eq!(payload["value"]["destinations"][0]["name"], "Station");
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

    write(&path, dropfile_with_guid("Track One", "guid-one"))?;
    let second = watcher.process_event(upsert(&path), Instant::now())?;

    assert_eq!(first[0].block_guid, second[0].block_guid);
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

    assert_eq!(first[0].block_guid, same[0].block_guid);
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
