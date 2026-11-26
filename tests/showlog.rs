//! Show log task 002: the `track` and `clear` entries from the publish
//! loop (ADR 0017).
//!
//! Each test drives the public watcher and relay publisher APIs with a
//! real drop file and a stub HTTP relay, then reads the show log file the
//! relay worker wrote, the same way an operator or HitIt would.

use std::collections::VecDeque;
use std::fs;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, mpsc};
use std::thread;
use std::time::{Duration, Instant};

use anyhow::{Result, anyhow};
use musicindex_live_publisher::{
    DropEvent, DropEventKind, DropWatcher, PublisherConfig, PublisherTarget, RelayPublisher,
    ShowLog, WatchTarget,
};
use serde_json::{Value, json};
use tempfile::TempDir;

const TOKEN: &str = "secret-token";

#[derive(Debug, Clone)]
struct StubResponse {
    status: u16,
    body: String,
}

#[derive(Debug, Clone)]
struct StubRequest {
    body: Value,
}

#[derive(Debug)]
struct StubServer {
    endpoint: String,
    requests: Arc<Mutex<Vec<StubRequest>>>,
    received: mpsc::Receiver<()>,
}

impl StubServer {
    fn start(responses: Vec<StubResponse>) -> Result<Self> {
        let listener = TcpListener::bind("127.0.0.1:0")?;
        let endpoint = format!("http://{}", listener.local_addr()?);
        let responses = Arc::new(Mutex::new(VecDeque::from(responses)));
        let requests = Arc::new(Mutex::new(Vec::new()));
        let (sender, received) = mpsc::channel();

        thread::spawn({
            let responses = Arc::clone(&responses);
            let requests = Arc::clone(&requests);
            move || {
                for stream in listener.incoming() {
                    let Ok(stream) = stream else {
                        break;
                    };
                    if handle_connection(stream, &responses, &requests).is_ok() {
                        let _ignored = sender.send(());
                    }
                }
            }
        });

        Ok(Self {
            endpoint,
            requests,
            received,
        })
    }

    fn wait_for_requests(&self, count: usize) -> Result<()> {
        for _ in 0..count {
            self.received.recv_timeout(Duration::from_secs(2))?;
        }
        Ok(())
    }

    fn requests(&self) -> Result<Vec<StubRequest>> {
        self.requests
            .lock()
            .map(|requests| requests.clone())
            .map_err(|_| anyhow!("request mutex poisoned"))
    }
}

fn handle_connection(
    stream: TcpStream,
    responses: &Arc<Mutex<VecDeque<StubResponse>>>,
    requests: &Arc<Mutex<Vec<StubRequest>>>,
) -> Result<()> {
    let mut reader = BufReader::new(stream);
    let mut first_line = String::new();
    reader.read_line(&mut first_line)?;

    let mut content_length = 0_usize;
    loop {
        let mut line = String::new();
        reader.read_line(&mut line)?;
        let trimmed = line.trim_end_matches(['\r', '\n']);
        if trimmed.is_empty() {
            break;
        }
        if let Some((name, value)) = trimmed.split_once(':')
            && name.eq_ignore_ascii_case("content-length")
        {
            content_length = value.trim().parse()?;
        }
    }

    let mut body = vec![0_u8; content_length];
    reader.read_exact(&mut body)?;
    let body = if body.is_empty() {
        Value::Null
    } else {
        serde_json::from_slice(&body).unwrap_or(Value::Null)
    };
    requests
        .lock()
        .map_err(|_| anyhow!("request mutex poisoned"))?
        .push(StubRequest { body });

    let response = responses
        .lock()
        .map_err(|_| anyhow!("response mutex poisoned"))?
        .pop_front()
        .ok_or_else(|| anyhow!("stub response queue exhausted"))?;
    let reason = if response.status == 200 {
        "OK"
    } else {
        "Error"
    };
    let raw = format!(
        "HTTP/1.1 {} {}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
        response.status,
        reason,
        response.body.len(),
        response.body
    );
    reader.get_mut().write_all(raw.as_bytes())?;
    Ok(())
}

fn accepted(seq: u64) -> StubResponse {
    StubResponse {
        status: 200,
        body: json!({"event_id": "event-guid", "accepted": true, "seq": seq}).to_string(),
    }
}

fn error_response(status: u16) -> StubResponse {
    StubResponse {
        status,
        body: json!({"error": status}).to_string(),
    }
}

fn config(endpoint: &str) -> PublisherConfig {
    PublisherConfig {
        watch_dir: PathBuf::from("/tmp"),
        endpoint: endpoint.to_owned(),
        targets: vec![PublisherTarget {
            name: "default".to_owned(),
            event_id: "event-guid".to_owned(),
            token_file: PathBuf::from("/tmp/default.token"),
            token: TOKEN.to_owned(),
            stream_delay: Duration::ZERO,
            listener_delay_secs: 0,
            dead_block: musicindex_live_publisher::DeadBlockSettings::default(),
            display_dir: None,
        }],
        show_log_retention_days: 365,
        show_log: true,
    }
}

fn watch_target() -> WatchTarget {
    WatchTarget {
        name: "default".to_owned(),
        event_guid: "event-guid".to_owned(),
        dead_block: musicindex_live_publisher::DeadBlockSettings::default(),
    }
}

/// Writes a `musicindex.nowplaying/3` drop file.
fn write_dropfile(path: &Path, artist: &str, title: &str, value_routes: Value) -> Result<()> {
    fs::write(
        path,
        json!({
            "schema": "musicindex.nowplaying/3",
            "target": "default",
            "artist": artist,
            "title": title,
            "duration_secs": 123.0,
            "image": "https://example.com/art.png",
            "feed_guid": "feed-guid",
            "track_guid": "track-guid",
            "album": "Album Name",
            "play_id": "play-1",
            "value_routes": value_routes,
            "value_routes_source": "musicindex-api"
        })
        .to_string(),
    )?;
    Ok(())
}

fn payable_routes() -> Value {
    json!([{
        "recipient_name": "Alice",
        "route_type": "node",
        "address": "03alice",
        "split": 90.0,
        "fee": false,
        "custom_key": null,
        "custom_value": null
    }])
}

/// Every show log entry across every daily file under `<state_dir>/showlog`,
/// in file name order.
fn read_log_entries(state_dir: &Path) -> Result<Vec<Value>> {
    let dir = state_dir.join("showlog");
    let mut files: Vec<PathBuf> = fs::read_dir(&dir)?
        .filter_map(std::result::Result::ok)
        .map(|entry| entry.path())
        .filter(|path| {
            path.extension()
                .is_some_and(|extension| extension == "jsonl")
        })
        .collect();
    files.sort();

    let mut entries = Vec::new();
    for file in files {
        let text = fs::read_to_string(file)?;
        for line in text.lines() {
            entries.push(serde_json::from_str(line)?);
        }
    }
    Ok(entries)
}

/// Waits until the show log holds at least `min_entries` lines, or fails
/// after two seconds.
///
/// `StubServer::wait_for_requests` only shows that the stub finished
/// writing its HTTP response; it says nothing about whether the relay
/// worker's HTTP client has read that response yet, and the worker writes
/// its show log entry only after that read returns. This polls the log
/// file instead of trusting one read right after `wait_for_requests`.
fn wait_for_log_entries(state_dir: &Path, min_entries: usize) -> Result<Vec<Value>> {
    let deadline = Instant::now() + Duration::from_secs(2);
    loop {
        let entries = read_log_entries(state_dir).unwrap_or_default();
        if entries.len() >= min_entries {
            return Ok(entries);
        }
        if Instant::now() >= deadline {
            return Err(anyhow!(
                "expected at least {min_entries} show log entries, found {}",
                entries.len()
            ));
        }
        thread::sleep(Duration::from_millis(10));
    }
}

#[test]
fn a_payable_drop_file_gives_an_accepted_track_entry_with_the_sent_value() -> Result<()> {
    let server = StubServer::start(vec![accepted(7)])?;
    let watch_dir = TempDir::new()?;
    let state_dir = TempDir::new()?;
    let drop_path = watch_dir.path().join("default.json");
    write_dropfile(&drop_path, "Alice", "Track One", payable_routes())?;

    let log = Arc::new(ShowLog::open(&state_dir.path().join("showlog"), 365)?);
    let publisher = RelayPublisher::start_with_show_log(
        &config(&server.endpoint),
        Duration::from_millis(50),
        Duration::from_millis(200),
        Some(log),
    )?;
    let mut watcher = DropWatcher::new_targets(vec![watch_target()], Duration::ZERO);

    let payloads = watcher.process_event(
        DropEvent {
            kind: DropEventKind::Upsert,
            path: drop_path,
        },
        Instant::now(),
    )?;
    assert_eq!(payloads.len(), 1);
    let payload = payloads.into_iter().next().expect("one payload");
    publisher.publish(payload)?;
    server.wait_for_requests(1)?;

    let requests = server.requests()?;
    let entries = wait_for_log_entries(state_dir.path(), 1)?;
    assert_eq!(entries.len(), 1);
    let entry = &entries[0];
    assert_eq!(entry["schema"], "musicindex.showlog/1");
    assert_eq!(entry["kind"], "track");
    assert_eq!(entry["payable"], true);
    assert_eq!(entry["publish_result"], "accepted");
    assert_eq!(entry["seq"], 7);
    assert_eq!(entry["value"], requests[0].body["value"]);
    Ok(())
}

#[test]
fn a_non_payable_drop_file_gives_a_track_entry_with_the_dead_value() -> Result<()> {
    let server = StubServer::start(vec![accepted(1)])?;
    let watch_dir = TempDir::new()?;
    let state_dir = TempDir::new()?;
    let drop_path = watch_dir.path().join("default.json");
    write_dropfile(&drop_path, "Bob", "Talk Segment", json!([]))?;

    let log = Arc::new(ShowLog::open(&state_dir.path().join("showlog"), 365)?);
    let publisher = RelayPublisher::start_with_show_log(
        &config(&server.endpoint),
        Duration::from_millis(50),
        Duration::from_millis(200),
        Some(log),
    )?;
    let mut watcher = DropWatcher::new_targets(vec![watch_target()], Duration::ZERO);

    let payloads = watcher.process_event(
        DropEvent {
            kind: DropEventKind::Upsert,
            path: drop_path,
        },
        Instant::now(),
    )?;
    let payload = payloads.into_iter().next().expect("one payload");
    // The payload is the dead block: ADR 0029 keeps neither the drop
    // file's title nor its GUIDs for a non-payable track.
    assert_eq!(payload.title, "No V4V track playing");
    let expected_value = serde_json::to_value(payload.value.clone())?;
    publisher.publish(payload)?;
    server.wait_for_requests(1)?;

    let entries = wait_for_log_entries(state_dir.path(), 1)?;
    assert_eq!(entries.len(), 1);
    let entry = &entries[0];
    assert_eq!(entry["kind"], "track");
    assert_eq!(entry["payable"], false);
    assert_eq!(entry["artist"], "Bob");
    assert_eq!(entry["title"], "Talk Segment");
    assert_eq!(entry["value"], expected_value);
    Ok(())
}

#[test]
fn a_removed_drop_file_gives_a_clear_entry() -> Result<()> {
    let server = StubServer::start(vec![accepted(1), accepted(2)])?;
    let watch_dir = TempDir::new()?;
    let state_dir = TempDir::new()?;
    let drop_path = watch_dir.path().join("default.json");
    write_dropfile(&drop_path, "Alice", "Track One", payable_routes())?;

    let log = Arc::new(ShowLog::open(&state_dir.path().join("showlog"), 365)?);
    let publisher = RelayPublisher::start_with_show_log(
        &config(&server.endpoint),
        Duration::from_millis(50),
        Duration::from_millis(200),
        Some(log),
    )?;
    let mut watcher = DropWatcher::new_targets(vec![watch_target()], Duration::ZERO);

    let upserted = watcher.process_event(
        DropEvent {
            kind: DropEventKind::Upsert,
            path: drop_path.clone(),
        },
        Instant::now(),
    )?;
    publisher.publish(upserted.into_iter().next().expect("one payload"))?;
    server.wait_for_requests(1)?;
    wait_for_log_entries(state_dir.path(), 1)?;

    fs::remove_file(&drop_path)?;
    let removed = watcher.process_event(
        DropEvent {
            kind: DropEventKind::Remove,
            path: drop_path,
        },
        Instant::now(),
    )?;
    publisher.publish(removed.into_iter().next().expect("one payload"))?;
    // `wait_for_requests` counts from zero on each call: the first request
    // was already awaited above, so this call awaits only the one more
    // request this second publish sends.
    server.wait_for_requests(1)?;

    let entries = wait_for_log_entries(state_dir.path(), 2)?;
    assert_eq!(entries.len(), 2);
    assert_eq!(entries[0]["kind"], "track");
    assert_eq!(entries[1]["kind"], "clear");
    Ok(())
}

#[test]
fn a_retried_publish_gives_a_retryable_entry_then_an_accepted_entry_with_the_same_facts()
-> Result<()> {
    let server = StubServer::start(vec![error_response(503), accepted(2)])?;
    let watch_dir = TempDir::new()?;
    let state_dir = TempDir::new()?;
    let drop_path = watch_dir.path().join("default.json");
    write_dropfile(&drop_path, "Alice", "Track One", payable_routes())?;

    let log = Arc::new(ShowLog::open(&state_dir.path().join("showlog"), 365)?);
    let publisher = RelayPublisher::start_with_show_log(
        &config(&server.endpoint),
        Duration::from_millis(50),
        Duration::from_millis(200),
        Some(log),
    )?;
    let mut watcher = DropWatcher::new_targets(vec![watch_target()], Duration::ZERO);

    let payloads = watcher.process_event(
        DropEvent {
            kind: DropEventKind::Upsert,
            path: drop_path,
        },
        Instant::now(),
    )?;
    publisher.publish(payloads.into_iter().next().expect("one payload"))?;
    server.wait_for_requests(2)?;

    let entries = wait_for_log_entries(state_dir.path(), 2)?;
    assert_eq!(entries.len(), 2);
    assert_eq!(entries[0]["publish_result"], "retryable");
    assert_eq!(entries[1]["publish_result"], "accepted");
    assert_eq!(entries[0]["observed_at"], entries[1]["observed_at"]);
    assert_eq!(entries[0]["block_guid"], entries[1]["block_guid"]);
    Ok(())
}

#[test]
fn an_oversized_payload_gives_a_dropped_entry() -> Result<()> {
    let server = StubServer::start(vec![error_response(413)])?;
    let watch_dir = TempDir::new()?;
    let state_dir = TempDir::new()?;
    let drop_path = watch_dir.path().join("default.json");
    write_dropfile(&drop_path, "Alice", "Track One", payable_routes())?;

    let log = Arc::new(ShowLog::open(&state_dir.path().join("showlog"), 365)?);
    let publisher = RelayPublisher::start_with_show_log(
        &config(&server.endpoint),
        Duration::from_millis(50),
        Duration::from_millis(200),
        Some(log),
    )?;
    let mut watcher = DropWatcher::new_targets(vec![watch_target()], Duration::ZERO);

    let payloads = watcher.process_event(
        DropEvent {
            kind: DropEventKind::Upsert,
            path: drop_path,
        },
        Instant::now(),
    )?;
    publisher.publish(payloads.into_iter().next().expect("one payload"))?;
    server.wait_for_requests(1)?;

    let entries = wait_for_log_entries(state_dir.path(), 1)?;
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0]["publish_result"], "dropped");
    Ok(())
}

#[test]
fn rewriting_the_same_drop_file_unchanged_gives_no_second_entry() -> Result<()> {
    let server = StubServer::start(vec![accepted(1)])?;
    let watch_dir = TempDir::new()?;
    let state_dir = TempDir::new()?;
    let drop_path = watch_dir.path().join("default.json");
    write_dropfile(&drop_path, "Alice", "Track One", payable_routes())?;

    let log = Arc::new(ShowLog::open(&state_dir.path().join("showlog"), 365)?);
    let publisher = RelayPublisher::start_with_show_log(
        &config(&server.endpoint),
        Duration::from_millis(50),
        Duration::from_millis(200),
        Some(log),
    )?;
    let mut watcher = DropWatcher::new_targets(vec![watch_target()], Duration::ZERO);

    let payloads = watcher.process_event(
        DropEvent {
            kind: DropEventKind::Upsert,
            path: drop_path.clone(),
        },
        Instant::now(),
    )?;
    publisher.publish(payloads.into_iter().next().expect("one payload"))?;
    server.wait_for_requests(1)?;
    let entries = wait_for_log_entries(state_dir.path(), 1)?;
    assert_eq!(entries.len(), 1);

    // The exact same bytes, written again: the watcher must not emit a
    // second payload, so the relay worker never gets a second outcome to
    // log.
    write_dropfile(&drop_path, "Alice", "Track One", payable_routes())?;
    let unchanged = watcher.process_event(
        DropEvent {
            kind: DropEventKind::Upsert,
            path: drop_path,
        },
        Instant::now(),
    )?;
    assert!(unchanged.is_empty());

    let entries_after = read_log_entries(state_dir.path())?;
    assert_eq!(entries_after.len(), 1);
    Ok(())
}

#[test]
fn the_log_file_holds_no_target_token() -> Result<()> {
    let server = StubServer::start(vec![accepted(1)])?;
    let watch_dir = TempDir::new()?;
    let state_dir = TempDir::new()?;
    let drop_path = watch_dir.path().join("default.json");
    write_dropfile(&drop_path, "Alice", "Track One", payable_routes())?;

    let log_dir = state_dir.path().join("showlog");
    let log = Arc::new(ShowLog::open(&log_dir, 365)?);
    let publisher = RelayPublisher::start_with_show_log(
        &config(&server.endpoint),
        Duration::from_millis(50),
        Duration::from_millis(200),
        Some(log),
    )?;
    let mut watcher = DropWatcher::new_targets(vec![watch_target()], Duration::ZERO);

    let payloads = watcher.process_event(
        DropEvent {
            kind: DropEventKind::Upsert,
            path: drop_path,
        },
        Instant::now(),
    )?;
    publisher.publish(payloads.into_iter().next().expect("one payload"))?;
    server.wait_for_requests(1)?;
    wait_for_log_entries(state_dir.path(), 1)?;

    for entry in fs::read_dir(&log_dir)? {
        let text = fs::read_to_string(entry?.path())?;
        assert!(!text.contains(TOKEN));
    }
    Ok(())
}

#[cfg(unix)]
#[test]
fn a_read_only_log_directory_does_not_stop_the_publish() -> Result<()> {
    use std::os::unix::fs::PermissionsExt;

    let server = StubServer::start(vec![accepted(1)])?;
    let watch_dir = TempDir::new()?;
    let state_dir = TempDir::new()?;
    let drop_path = watch_dir.path().join("default.json");
    write_dropfile(&drop_path, "Alice", "Track One", payable_routes())?;

    let log_dir = state_dir.path().join("showlog");
    let log = Arc::new(ShowLog::open(&log_dir, 365)?);
    fs::set_permissions(&log_dir, fs::Permissions::from_mode(0o500))?;

    let publisher = RelayPublisher::start_with_show_log(
        &config(&server.endpoint),
        Duration::from_millis(50),
        Duration::from_millis(200),
        Some(log),
    )?;
    let mut watcher = DropWatcher::new_targets(vec![watch_target()], Duration::ZERO);

    let payloads = watcher.process_event(
        DropEvent {
            kind: DropEventKind::Upsert,
            path: drop_path,
        },
        Instant::now(),
    )?;
    publisher.publish(payloads.into_iter().next().expect("one payload"))?;
    // The relay still gets the payload: a log failure never stops a
    // publish (ADR 0017 §The Log Never Stops A Publish).
    server.wait_for_requests(1)?;
    let requests = server.requests()?;
    assert_eq!(requests.len(), 1);

    // Restore write access so the temporary directory can be removed.
    fs::set_permissions(&log_dir, fs::Permissions::from_mode(0o700))?;
    Ok(())
}

#[test]
fn with_no_show_log_configured_a_publish_creates_no_showlog_directory() -> Result<()> {
    let server = StubServer::start(vec![accepted(1)])?;
    let watch_dir = TempDir::new()?;
    // Stands in for a state directory the service never opened a show log
    // under, the same as a service started with no `--state-dir`.
    let state_dir = TempDir::new()?;
    let drop_path = watch_dir.path().join("default.json");
    write_dropfile(&drop_path, "Alice", "Track One", payable_routes())?;

    let publisher = RelayPublisher::start_with_backoff(
        &config(&server.endpoint),
        Duration::from_millis(50),
        Duration::from_millis(200),
    )?;
    let mut watcher = DropWatcher::new_targets(vec![watch_target()], Duration::ZERO);

    let payloads = watcher.process_event(
        DropEvent {
            kind: DropEventKind::Upsert,
            path: drop_path,
        },
        Instant::now(),
    )?;
    publisher.publish(payloads.into_iter().next().expect("one payload"))?;
    server.wait_for_requests(1)?;

    assert!(!state_dir.path().join("showlog").exists());
    Ok(())
}
