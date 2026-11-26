//! Tests for the publisher display path (ADR 0008, display task 004).
//!
//! The stub relay here serves each connection on its own thread, so it can
//! hold an image upload open while a payload or a keepalive goes through.

use std::collections::{HashMap, VecDeque};
use std::fs;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Condvar, Mutex};
use std::thread;
use std::time::{Duration, Instant, SystemTime};

use anyhow::{Result, anyhow};
use musicindex_live_publisher::{
    Artwork, ArtworkImage, DISPLAY_FILE_NAME, DisplayEntry, DisplayOutcome, DisplayState,
    DisplayTrack, ImageMime, LiveValue, LiveValueDestination, LiveValueModel, LiveValuePayload,
    LogFacts, MAX_IMAGE_BYTES, Pairing, ProducerState, PublisherConfig, PublisherTarget,
    RelayClient, RelayPublisher, RelayTarget, ShowLog, ShowLogEntryKind, read_display_state,
};
use serde_json::{Value, json};
use tempfile::TempDir;

const TOKEN: &str = "secret-token";
const WAIT: Duration = Duration::from_secs(3);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum Route {
    Metadata,
    Keepalive,
    Display,
    Artwork,
    Other,
}

/// Gives the route of one request. The publisher also reads the metadata
/// route with `GET` before each keepalive (ADR 0023). That read gets the
/// `404` of `Route::Other`, so it does not take a scripted publish reply,
/// and the publisher then sends the keepalive.
fn route_of(method: &str, path: &str) -> Route {
    if path.ends_with("/metadata") && method != "GET" {
        Route::Metadata
    } else if path.ends_with("/keepalive") {
        Route::Keepalive
    } else if path.ends_with("/display") {
        Route::Display
    } else if path.contains("/artwork/") {
        Route::Artwork
    } else {
        Route::Other
    }
}

#[derive(Debug, Clone)]
struct Reply {
    status: u16,
    body: String,
}

fn reply(status: u16, body: Value) -> Reply {
    Reply {
        status,
        body: body.to_string(),
    }
}

fn error_reply(status: u16, code: &str) -> Reply {
    reply(status, json!({ "error": code }))
}

fn accepted_with_interval(seq: u64, interval: u64) -> Reply {
    reply(
        200,
        json!({
            "event_id": "event-guid",
            "accepted": true,
            "seq": seq,
            "lease_secs": 90,
            "keepalive_interval_secs": interval,
        }),
    )
}

fn default_reply(route: Route) -> Reply {
    match route {
        Route::Metadata => reply(
            200,
            json!({"event_id": "event-guid", "accepted": true, "seq": 99}),
        ),
        Route::Keepalive => reply(
            200,
            json!({"event_id": "event-guid", "lease_expires_at": "2026-10-04T18:00:00Z", "keepalive_interval_secs": 60}),
        ),
        Route::Display => reply(
            200,
            json!({"event_id": "event-guid", "accepted": true, "seq": 1}),
        ),
        Route::Artwork => reply(
            200,
            json!({"event_id": "event-guid", "sha256": "x", "mime": "image/jpeg", "stored": true}),
        ),
        Route::Other => error_reply(404, "not_found"),
    }
}

#[derive(Debug, Clone)]
struct Recorded {
    method: String,
    path: String,
    route: Route,
    authorization: Option<String>,
    body: Vec<u8>,
    /// True while the stub holds this request open.
    held: bool,
}

impl Recorded {
    fn json(&self) -> Value {
        serde_json::from_slice(&self.body).unwrap_or(Value::Null)
    }
}

type Gate = Arc<(Mutex<bool>, Condvar)>;

struct Stub {
    endpoint: String,
    requests: Arc<Mutex<Vec<Recorded>>>,
    gate: Gate,
}

impl Stub {
    /// Starts a stub. `replies` gives the answers of each route in order.
    /// After them, each route gives its default answer. With `hold_artwork`,
    /// each upload stays open until [`Stub::release`].
    fn start(replies: Vec<(Route, Reply)>, hold_artwork: bool) -> Result<Self> {
        let listener = TcpListener::bind("127.0.0.1:0")?;
        let endpoint = format!("http://{}", listener.local_addr()?);
        let mut queues: HashMap<Route, VecDeque<Reply>> = HashMap::new();
        for (route, reply) in replies {
            queues.entry(route).or_default().push_back(reply);
        }
        let queues = Arc::new(Mutex::new(queues));
        let requests = Arc::new(Mutex::new(Vec::new()));
        let gate: Gate = Arc::new((Mutex::new(!hold_artwork), Condvar::new()));

        thread::spawn({
            let requests = Arc::clone(&requests);
            let gate = Arc::clone(&gate);
            move || {
                for stream in listener.incoming() {
                    let Ok(stream) = stream else {
                        break;
                    };
                    let queues = Arc::clone(&queues);
                    let requests = Arc::clone(&requests);
                    let gate = Arc::clone(&gate);
                    thread::spawn(move || {
                        let _ignored = serve(stream, &queues, &requests, &gate);
                    });
                }
            }
        });

        Ok(Self {
            endpoint,
            requests,
            gate,
        })
    }

    fn release(&self) {
        let (lock, condvar) = &*self.gate;
        if let Ok(mut open) = lock.lock() {
            *open = true;
            condvar.notify_all();
        }
    }

    fn requests(&self) -> Vec<Recorded> {
        self.requests
            .lock()
            .map(|requests| requests.clone())
            .unwrap_or_default()
    }

    fn of(&self, route: Route) -> Vec<Recorded> {
        self.requests()
            .into_iter()
            .filter(|request| request.route == route)
            .collect()
    }

    fn routes(&self) -> Vec<Route> {
        self.requests()
            .iter()
            .map(|request| request.route)
            .collect()
    }

    fn wait_until(&self, what: &str, condition: impl Fn(&[Recorded]) -> bool) -> Result<()> {
        let deadline = Instant::now() + WAIT;
        while Instant::now() < deadline {
            if condition(&self.requests()) {
                return Ok(());
            }
            thread::sleep(Duration::from_millis(5));
        }
        Err(anyhow!(
            "timed out waiting for {what}; got {:?}",
            self.routes()
        ))
    }

    fn wait_for(&self, route: Route, count: usize) -> Result<()> {
        self.wait_until(&format!("{count} {route:?} requests"), |requests| {
            requests
                .iter()
                .filter(|request| request.route == route)
                .count()
                >= count
        })
    }

    /// Asserts that the request count stays the same for `duration`.
    fn expect_quiet(&self, duration: Duration) -> Result<()> {
        let before = self.requests().len();
        thread::sleep(duration);
        let after = self.requests();
        if after.len() != before {
            return Err(anyhow!(
                "unexpected requests: {:?}",
                after[before..].iter().map(|r| r.route).collect::<Vec<_>>()
            ));
        }
        Ok(())
    }
}

fn serve(
    stream: TcpStream,
    queues: &Mutex<HashMap<Route, VecDeque<Reply>>>,
    requests: &Mutex<Vec<Recorded>>,
    gate: &Gate,
) -> Result<()> {
    let mut reader = BufReader::new(stream);
    let mut first_line = String::new();
    reader.read_line(&mut first_line)?;
    let mut parts = first_line.split_whitespace();
    let method = parts.next().unwrap_or_default().to_owned();
    let path = parts.next().unwrap_or_default().to_owned();

    let mut content_length = 0_usize;
    let mut authorization = None;
    loop {
        let mut line = String::new();
        reader.read_line(&mut line)?;
        let trimmed = line.trim_end_matches(['\r', '\n']);
        if trimmed.is_empty() {
            break;
        }
        if let Some((name, value)) = trimmed.split_once(':') {
            let value = value.trim();
            if name.eq_ignore_ascii_case("content-length") {
                content_length = value.parse()?;
            }
            if name.eq_ignore_ascii_case("authorization") {
                authorization = Some(value.to_owned());
            }
        }
    }
    let mut body = vec![0_u8; content_length];
    reader.read_exact(&mut body)?;

    let route = route_of(&method, &path);
    let index = {
        let mut requests = requests.lock().map_err(|_| anyhow!("poisoned"))?;
        requests.push(Recorded {
            method,
            path,
            route,
            authorization,
            body,
            held: route == Route::Artwork,
        });
        requests.len() - 1
    };

    if route == Route::Artwork {
        let (lock, condvar) = &**gate;
        let mut open = lock.lock().map_err(|_| anyhow!("poisoned"))?;
        while !*open {
            open = condvar.wait(open).map_err(|_| anyhow!("poisoned"))?;
        }
    }
    if let Ok(mut requests) = requests.lock() {
        requests[index].held = false;
    }

    let reply = queues
        .lock()
        .map_err(|_| anyhow!("poisoned"))?
        .get_mut(&route)
        .and_then(VecDeque::pop_front)
        .unwrap_or_else(|| default_reply(route));
    let raw = format!(
        "HTTP/1.1 {} X\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
        reply.status,
        reply.body.len(),
        reply.body
    );
    reader.get_mut().write_all(raw.as_bytes())?;
    Ok(())
}

fn jpeg(seed: &str) -> ArtworkImage {
    let mut bytes = vec![0xFF, 0xD8, 0xFF, 0xE0];
    bytes.extend_from_slice(seed.as_bytes());
    ArtworkImage::from_bytes(ImageMime::Jpeg, bytes)
}

fn track_state(title: &str, artwork: Option<Artwork>) -> DisplayState {
    DisplayState {
        track: Some(DisplayTrack {
            artist: "Artist".to_owned(),
            title: title.to_owned(),
            artwork,
            song_line: format!("Artist - {title}"),
            play_id: Some("1".to_owned()),
            album: None,
            pub_date: None,
            funding: Vec::new(),
        }),
    }
}

fn entry(state: DisplayState) -> DisplayEntry {
    DisplayEntry {
        event_id: "event-guid".to_owned(),
        state,
    }
}

fn payload(title: &str) -> LiveValuePayload {
    LiveValuePayload {
        title: title.to_owned(),
        image: None,
        description: String::new(),
        kind: "music".to_owned(),
        start_time: 0,
        duration: Some(187.326),
        event_guid: "event-guid".to_owned(),
        block_guid: format!("block-{title}"),
        feed_guid: None,
        item_guid: None,
        line: Some(vec![title.to_owned(), "Artist".to_owned()]),
        author: Some("Artist".to_owned()),
        podcast_name: None,
        value: LiveValue {
            model: LiveValueModel {
                kind: "lightning".to_owned(),
                method: "keysend".to_owned(),
                suggested: None,
            },
            destinations: Vec::new(),
        },
        play_id: None,
        log: None,
    }
}

fn relay_target(endpoint: &str) -> RelayTarget {
    RelayTarget {
        name: "default".to_owned(),
        endpoint: endpoint.to_owned(),
        event_id: "event-guid".to_owned(),
        token: TOKEN.to_owned(),
        listener_delay_secs: 0,
    }
}

fn display_config(endpoint: &str) -> PublisherConfig {
    PublisherConfig {
        watch_dir: PathBuf::from("/nonexistent/watch"),
        endpoint: endpoint.to_owned(),
        targets: vec![PublisherTarget {
            name: "default".to_owned(),
            event_id: "event-guid".to_owned(),
            token_file: PathBuf::from("/nonexistent/default.token"),
            token: TOKEN.to_owned(),
            stream_delay: Duration::ZERO,
            listener_delay_secs: 0,
            dead_block: musicindex_live_publisher::DeadBlockSettings::default(),
            display_dir: Some(PathBuf::from("/nonexistent/display")),
        }],
        show_log_retention_days: 365,
        show_log: true,
    }
}

/// Starts a publisher with a short wait of the first send for the picture
/// (ADR 0034 §The First Send Waits For The Picture), so that a payload with
/// no picture goes out soon.
fn start(stub: &Stub) -> Result<RelayPublisher> {
    RelayPublisher::start_with_payload_image_wait(
        &display_config(&stub.endpoint),
        Duration::from_millis(20),
        Duration::from_millis(80),
        None,
        Duration::from_millis(300),
    )
}

// The relay requests.

#[test]
fn display_publish_body_holds_only_track_and_no_schema_key() -> Result<()> {
    let stub = Stub::start(Vec::new(), false)?;
    let client = RelayClient::new(Duration::from_secs(1))?;
    let image = jpeg("cover");
    let state = track_state("Title", Some(Artwork::Image(image.clone())));

    let outcome = client.publish_display(&relay_target(&stub.endpoint), &state, None)?;

    assert_eq!(outcome, DisplayOutcome::Accepted);
    let request = &stub.of(Route::Display)[0];
    assert_eq!(request.method, "POST");
    assert_eq!(request.path, "/v1/liveitems/event-guid/display");
    assert_eq!(
        request.authorization.as_deref(),
        Some("Bearer secret-token")
    );
    assert_eq!(
        request.json(),
        json!({
            "track": {
                "artist": "Artist",
                "title": "Title",
                "artwork": { "sha256": image.sha256(), "mime": "image/jpeg" },
                "songLine": "Artist - Title",
                "playId": "1"
            }
        })
    );
    assert!(request.json().get("schema").is_none());
    Ok(())
}

#[test]
fn display_publish_body_of_a_null_state_and_of_a_url() -> Result<()> {
    let stub = Stub::start(Vec::new(), false)?;
    let client = RelayClient::new(Duration::from_secs(1))?;
    let target = relay_target(&stub.endpoint);

    client.publish_display(&target, &DisplayState::null(), None)?;
    client.publish_display(
        &target,
        &track_state(
            "Url",
            Some(Artwork::Url("https://img.example/a.gif".to_owned())),
        ),
        None,
    )?;

    let requests = stub.of(Route::Display);
    assert_eq!(requests[0].json(), json!({ "track": null }));
    assert_eq!(
        requests[1].json()["track"]["artwork"],
        json!({ "url": "https://img.example/a.gif" })
    );
    Ok(())
}

#[test]
fn artwork_upload_puts_the_raw_bytes_with_the_bearer_token() -> Result<()> {
    let stub = Stub::start(Vec::new(), false)?;
    let client = RelayClient::new(Duration::from_secs(1))?;
    let image = jpeg("cover");

    let outcome = client.upload_artwork(&relay_target(&stub.endpoint), &image)?;

    assert_eq!(outcome, DisplayOutcome::Accepted);
    let request = &stub.of(Route::Artwork)[0];
    assert_eq!(request.method, "PUT");
    assert_eq!(
        request.path,
        format!("/v1/liveitems/event-guid/artwork/{}", image.sha256())
    );
    assert_eq!(
        request.authorization.as_deref(),
        Some("Bearer secret-token")
    );
    assert_eq!(request.body, image.bytes());
    Ok(())
}

#[test]
fn display_status_codes_map_to_their_outcomes() -> Result<()> {
    let cases = [
        (error_reply(404, "event_not_found"), "disabled"),
        (error_reply(409, "event_not_reserved"), "disabled"),
        (error_reply(409, "artwork_missing"), "missing"),
        (error_reply(429, "publish_rate_limited"), "retry"),
        (error_reply(503, "unavailable"), "retry"),
        (error_reply(400, "invalid_display"), "refused"),
        (error_reply(401, "invalid_bearer_token"), "refused"),
        (error_reply(403, "invalid_token"), "refused"),
        (error_reply(413, "payload_too_large"), "refused"),
    ];
    for (answer, expected) in cases {
        let status = answer.status;
        let stub = Stub::start(vec![(Route::Display, answer)], false)?;
        let client = RelayClient::new(Duration::from_secs(1))?;
        let outcome =
            client.publish_display(&relay_target(&stub.endpoint), &DisplayState::null(), None)?;
        let kind = match outcome {
            DisplayOutcome::Accepted => "accepted",
            DisplayOutcome::ArtworkMissing => "missing",
            DisplayOutcome::Disabled { .. } => "disabled",
            DisplayOutcome::Retryable { .. } => "retry",
            DisplayOutcome::Refused { .. } => "refused",
        };
        assert_eq!(kind, expected, "HTTP {status}");
    }
    Ok(())
}

// The display worker.

#[test]
fn display_worker_uploads_the_image_before_it_publishes_the_state() -> Result<()> {
    let stub = Stub::start(Vec::new(), false)?;
    let publisher = start(&stub)?;

    publisher.publish_display(entry(track_state("One", Some(Artwork::Image(jpeg("one"))))));
    stub.wait_for(Route::Display, 1)?;

    assert_eq!(stub.routes(), vec![Route::Artwork, Route::Display]);
    Ok(())
}

#[test]
fn display_worker_sends_a_url_with_no_upload() -> Result<()> {
    let stub = Stub::start(Vec::new(), false)?;
    let publisher = start(&stub)?;

    publisher.publish_display(entry(track_state(
        "Url",
        Some(Artwork::Url("https://img.example/a.png".to_owned())),
    )));
    stub.wait_for(Route::Display, 1)?;

    assert!(stub.of(Route::Artwork).is_empty());
    Ok(())
}

#[test]
fn a_payload_is_sent_while_an_image_upload_is_held_open() -> Result<()> {
    let stub = Stub::start(Vec::new(), true)?;
    let publisher = start(&stub)?;

    publisher.publish_display(entry(track_state(
        "Held",
        Some(Artwork::Image(jpeg("held"))),
    )));
    stub.wait_for(Route::Artwork, 1)?;
    publisher.publish(payload("Track"))?;
    stub.wait_for(Route::Metadata, 1)?;

    // The upload is still open when the payload reached the relay.
    assert!(stub.of(Route::Artwork)[0].held);
    assert!(stub.of(Route::Display).is_empty());
    stub.release();
    stub.wait_for(Route::Display, 1)?;
    Ok(())
}

#[test]
fn a_keepalive_is_sent_while_an_image_upload_is_held_open() -> Result<()> {
    let stub = Stub::start(vec![(Route::Metadata, accepted_with_interval(1, 1))], true)?;
    let publisher = start(&stub)?;
    publisher.set_producer(ProducerState::Running);

    publisher.publish(payload("Track"))?;
    stub.wait_for(Route::Metadata, 1)?;
    publisher.publish_display(entry(track_state(
        "Held",
        Some(Artwork::Image(jpeg("held"))),
    )));
    stub.wait_for(Route::Artwork, 1)?;
    stub.wait_for(Route::Keepalive, 1)?;

    assert!(stub.of(Route::Artwork)[0].held);
    stub.release();
    stub.wait_for(Route::Display, 1)?;
    Ok(())
}

#[test]
fn an_image_is_uploaded_once_for_two_states_that_name_it() -> Result<()> {
    let stub = Stub::start(Vec::new(), false)?;
    let publisher = start(&stub)?;
    let image = jpeg("shared");

    publisher.publish_display(entry(track_state(
        "One",
        Some(Artwork::Image(image.clone())),
    )));
    stub.wait_for(Route::Display, 1)?;
    publisher.publish_display(entry(track_state("Two", Some(Artwork::Image(image)))));
    stub.wait_for(Route::Display, 2)?;

    assert_eq!(stub.of(Route::Artwork).len(), 1);
    Ok(())
}

#[test]
fn of_three_waiting_states_only_the_last_is_sent() -> Result<()> {
    let stub = Stub::start(Vec::new(), true)?;
    let publisher = start(&stub)?;

    publisher.publish_display(entry(track_state(
        "Busy",
        Some(Artwork::Image(jpeg("busy"))),
    )));
    stub.wait_for(Route::Artwork, 1)?;
    publisher.publish_display(entry(track_state("Two", None)));
    publisher.publish_display(entry(track_state("Three", None)));
    publisher.publish_display(entry(track_state("Four", None)));
    stub.release();
    stub.wait_for(Route::Display, 2)?;
    stub.expect_quiet(Duration::from_millis(200))?;

    let titles: Vec<Value> = stub
        .of(Route::Display)
        .iter()
        .map(|request| request.json()["track"]["title"].clone())
        .collect();
    assert_eq!(titles, vec![json!("Busy"), json!("Four")]);
    Ok(())
}

#[test]
fn artwork_missing_gives_one_new_upload_and_one_new_publish() -> Result<()> {
    let stub = Stub::start(
        vec![(Route::Display, error_reply(409, "artwork_missing"))],
        false,
    )?;
    let publisher = start(&stub)?;

    publisher.publish_display(entry(track_state("One", Some(Artwork::Image(jpeg("one"))))));
    stub.wait_for(Route::Display, 2)?;
    stub.expect_quiet(Duration::from_millis(200))?;

    assert_eq!(
        stub.routes(),
        vec![
            Route::Artwork,
            Route::Display,
            Route::Artwork,
            Route::Display
        ]
    );
    Ok(())
}

#[test]
fn a_second_artwork_missing_gives_no_third_upload() -> Result<()> {
    let stub = Stub::start(
        vec![
            (Route::Display, error_reply(409, "artwork_missing")),
            (Route::Display, error_reply(409, "artwork_missing")),
        ],
        false,
    )?;
    let publisher = start(&stub)?;

    publisher.publish_display(entry(track_state("One", Some(Artwork::Image(jpeg("one"))))));
    stub.wait_for(Route::Display, 2)?;
    stub.expect_quiet(Duration::from_millis(300))?;

    assert_eq!(stub.of(Route::Artwork).len(), 2);
    assert_eq!(stub.of(Route::Display).len(), 2);
    Ok(())
}

fn assert_display_path_turns_off(answer: Reply) -> Result<()> {
    let stub = Stub::start(vec![(Route::Display, answer)], false)?;
    let publisher = start(&stub)?;

    publisher.publish_display(entry(track_state("One", None)));
    stub.wait_for(Route::Display, 1)?;
    publisher.publish_display(entry(track_state("Two", None)));
    publisher.publish(payload("Later"))?;
    stub.wait_for(Route::Metadata, 1)?;
    stub.expect_quiet(Duration::from_millis(200))?;

    assert_eq!(stub.of(Route::Display).len(), 1);
    assert_eq!(stub.of(Route::Metadata)[0].json()["title"], "Later");
    publisher.check_health()?;
    Ok(())
}

#[test]
fn not_found_turns_off_the_display_path_and_a_later_payload_goes_out() -> Result<()> {
    assert_display_path_turns_off(error_reply(404, "event_not_found"))
}

#[test]
fn event_not_reserved_turns_off_the_display_path_and_a_later_payload_goes_out() -> Result<()> {
    assert_display_path_turns_off(error_reply(409, "event_not_reserved"))
}

#[test]
fn not_found_on_the_upload_also_turns_off_the_display_path() -> Result<()> {
    let stub = Stub::start(
        vec![(Route::Artwork, error_reply(404, "event_not_found"))],
        false,
    )?;
    let publisher = start(&stub)?;

    publisher.publish_display(entry(track_state("One", Some(Artwork::Image(jpeg("one"))))));
    stub.wait_for(Route::Artwork, 1)?;
    publisher.publish_display(entry(track_state("Two", None)));
    stub.expect_quiet(Duration::from_millis(200))?;

    assert!(stub.of(Route::Display).is_empty());
    Ok(())
}

#[test]
fn a_rate_limited_display_retries_with_the_latest_state_only() -> Result<()> {
    let stub = Stub::start(
        vec![(Route::Display, error_reply(429, "publish_rate_limited"))],
        false,
    )?;
    let publisher = RelayPublisher::start_with_backoff(
        &display_config(&stub.endpoint),
        Duration::from_millis(300),
        Duration::from_millis(300),
    )?;

    publisher.publish_display(entry(track_state("Old", None)));
    stub.wait_for(Route::Display, 1)?;
    publisher.publish_display(entry(track_state("New", None)));
    stub.wait_for(Route::Display, 2)?;
    stub.expect_quiet(Duration::from_millis(400))?;

    let requests = stub.of(Route::Display);
    assert_eq!(requests[0].json()["track"]["title"], "Old");
    assert_eq!(requests[1].json()["track"]["title"], "New");
    Ok(())
}

#[test]
fn a_new_state_does_not_shorten_the_retry_wait() -> Result<()> {
    let stub = Stub::start(
        vec![(Route::Display, error_reply(503, "unavailable"))],
        false,
    )?;
    let publisher = RelayPublisher::start_with_backoff(
        &display_config(&stub.endpoint),
        Duration::from_millis(400),
        Duration::from_millis(400),
    )?;

    publisher.publish_display(entry(track_state("Old", None)));
    stub.wait_for(Route::Display, 1)?;
    let failed_at = Instant::now();
    publisher.publish_display(entry(track_state("New", None)));
    stub.wait_for(Route::Display, 2)?;

    assert!(failed_at.elapsed() >= Duration::from_millis(300));
    Ok(())
}

#[test]
fn a_display_retry_waits_at_most_the_max_backoff() -> Result<()> {
    let stub = Stub::start(
        vec![
            (Route::Display, error_reply(503, "unavailable")),
            (Route::Display, error_reply(503, "unavailable")),
            (Route::Display, error_reply(503, "unavailable")),
            (Route::Display, error_reply(503, "unavailable")),
        ],
        false,
    )?;
    let publisher = RelayPublisher::start_with_backoff(
        &display_config(&stub.endpoint),
        Duration::from_millis(100),
        Duration::from_millis(150),
    )?;
    let started = Instant::now();

    publisher.publish_display(entry(track_state("One", None)));
    stub.wait_for(Route::Display, 5)?;

    // Without the bound, the waits are 100 + 200 + 400 + 800 ms.
    assert!(started.elapsed() < Duration::from_millis(1200));
    Ok(())
}

#[test]
fn a_display_failure_is_never_fatal() -> Result<()> {
    let stub = Stub::start(
        vec![
            (Route::Display, error_reply(401, "invalid_bearer_token")),
            (Route::Display, error_reply(403, "invalid_token")),
            (Route::Artwork, error_reply(413, "payload_too_large")),
        ],
        false,
    )?;
    let publisher = start(&stub)?;

    publisher.publish_display(entry(track_state("One", None)));
    stub.wait_for(Route::Display, 1)?;
    publisher.publish_display(entry(track_state("Two", None)));
    stub.wait_for(Route::Display, 2)?;
    publisher.publish_display(entry(track_state(
        "Three",
        Some(Artwork::Image(jpeg("big"))),
    )));
    stub.wait_for(Route::Display, 3)?;
    publisher.publish(payload("Track"))?;
    stub.wait_for(Route::Metadata, 1)?;

    publisher.check_health()?;
    // A refused image gives the state with artwork null.
    assert_eq!(
        stub.of(Route::Display)[2].json()["track"]["artwork"],
        Value::Null
    );
    Ok(())
}

#[test]
fn after_a_keepalive_409_and_the_republish_the_display_state_is_sent_again() -> Result<()> {
    let stub = Stub::start(
        vec![
            (Route::Metadata, accepted_with_interval(1, 1)),
            (Route::Keepalive, error_reply(409, "lease_expired")),
            (Route::Metadata, accepted_with_interval(2, 60)),
        ],
        false,
    )?;
    let publisher = start(&stub)?;
    publisher.set_producer(ProducerState::Running);

    publisher.publish(payload("Track"))?;
    stub.wait_for(Route::Metadata, 1)?;
    publisher.publish_display(entry(track_state("Track", Some(Artwork::Image(jpeg("t"))))));
    stub.wait_for(Route::Display, 1)?;
    stub.wait_for(Route::Keepalive, 1)?;
    stub.wait_for(Route::Metadata, 2)?;
    stub.wait_for(Route::Display, 2)?;

    let displays = stub.of(Route::Display);
    assert_eq!(displays[1].json(), displays[0].json());
    // The lease end removed the images, so the worker uploads again.
    let uploads = stub.of(Route::Artwork);
    assert_eq!(uploads.len(), 2);
    assert_eq!(uploads[0].path, uploads[1].path);
    // The resend comes after the republish.
    let routes = stub.routes();
    let republish = routes
        .iter()
        .rposition(|route| *route == Route::Metadata)
        .ok_or_else(|| anyhow!("no republish"))?;
    let resend = routes
        .iter()
        .rposition(|route| *route == Route::Display)
        .ok_or_else(|| anyhow!("no resend"))?;
    assert!(resend > republish);
    Ok(())
}

#[test]
fn after_a_keepalive_409_a_stale_image_cache_heals_through_artwork_missing() -> Result<()> {
    // The resend clears the image cache. A cache that the worker does not
    // clear still heals through the one retry of `409 artwork_missing`. This
    // test gives that answer to the resend and expects one upload and one
    // publish more.
    let stub = Stub::start(
        vec![
            (Route::Metadata, accepted_with_interval(1, 1)),
            (Route::Keepalive, error_reply(409, "lease_expired")),
            (Route::Metadata, accepted_with_interval(2, 60)),
            (
                Route::Display,
                reply(200, json!({"accepted": true, "seq": 1})),
            ),
            (Route::Display, error_reply(409, "artwork_missing")),
        ],
        false,
    )?;
    let publisher = start(&stub)?;
    publisher.set_producer(ProducerState::Running);

    publisher.publish(payload("Track"))?;
    stub.wait_for(Route::Metadata, 1)?;
    publisher.publish_display(entry(track_state("Track", Some(Artwork::Image(jpeg("t"))))));
    stub.wait_for(Route::Display, 1)?;
    stub.wait_for(Route::Display, 3)?;
    stub.expect_quiet(Duration::from_millis(200))?;

    assert_eq!(stub.of(Route::Display).len(), 3);
    assert!(stub.of(Route::Artwork).len() >= 2);
    Ok(())
}

#[test]
fn a_keepalive_409_on_a_target_with_no_display_path_sends_no_display_request() -> Result<()> {
    let stub = Stub::start(
        vec![
            (Route::Metadata, accepted_with_interval(1, 1)),
            (Route::Keepalive, error_reply(409, "lease_expired")),
            (Route::Metadata, accepted_with_interval(2, 60)),
        ],
        false,
    )?;
    let mut config = display_config(&stub.endpoint);
    config.targets[0].display_dir = None;
    let publisher = RelayPublisher::start_with_backoff(
        &config,
        Duration::from_millis(20),
        Duration::from_millis(80),
    )?;
    publisher.set_producer(ProducerState::Running);

    publisher.publish(payload("Track"))?;
    stub.wait_for(Route::Metadata, 2)?;
    publisher.publish_display(entry(DisplayState::null()));
    stub.expect_quiet(Duration::from_millis(200))?;

    assert!(stub.of(Route::Display).is_empty());
    Ok(())
}

#[test]
fn relay_publisher_debug_with_a_display_path_has_no_token() -> Result<()> {
    let stub = Stub::start(Vec::new(), false)?;
    let publisher = start(&stub)?;

    let rendered = format!("{publisher:?}");

    assert!(!rendered.contains(TOKEN));
    Ok(())
}

#[test]
fn display_types_debug_has_no_image_bytes() {
    let image = ArtworkImage::from_bytes(ImageMime::Jpeg, vec![0xFF, 0xD8, 0xFF, 0x42, 0x42]);
    let rendered = format!("{:?}", entry(track_state("T", Some(Artwork::Image(image)))));

    assert!(rendered.contains("len: 5"));
    assert!(!rendered.contains("255, 216"));
}

// The read of the display directory.

fn sha256_of(bytes: &[u8]) -> String {
    ArtworkImage::from_bytes(ImageMime::Jpeg, bytes.to_vec())
        .sha256()
        .to_owned()
}

fn write_display_json(dir: &Path, track: Value) -> Result<()> {
    fs::write(
        dir.join(DISPLAY_FILE_NAME),
        json!({ "schema": "musicindex.display/3", "track": track }).to_string(),
    )?;
    Ok(())
}

fn write_image(dir: &Path, bytes: &[u8], extension: &str) -> Result<String> {
    let sha256 = sha256_of(bytes);
    fs::write(dir.join(format!("{sha256}.{extension}")), bytes)?;
    Ok(sha256)
}

fn image_track(sha256: &str, mime: &str) -> Value {
    json!({
        "artist": "Artist",
        "title": "Title",
        "artwork": { "sha256": sha256, "mime": mime },
        "song_line": "Artist - Title",
        "play_id": "1"
    })
}

fn jpeg_bytes(len: usize) -> Vec<u8> {
    let mut bytes = vec![0xFF, 0xD8, 0xFF, 0xE0];
    bytes.resize(len, 0x42);
    bytes
}

#[test]
fn read_gives_the_track_and_the_image_bytes() -> Result<()> {
    let temp = TempDir::new()?;
    let bytes = jpeg_bytes(64);
    let sha256 = write_image(temp.path(), &bytes, "jpg")?;
    write_display_json(temp.path(), image_track(&sha256, "image/jpeg"))?;

    let state = read_display_state(temp.path()).ok_or_else(|| anyhow!("no state"))?;

    let image = state.image().ok_or_else(|| anyhow!("no image"))?;
    assert_eq!(image.sha256(), sha256);
    assert_eq!(image.bytes(), bytes.as_slice());
    assert_eq!(state.body()["track"]["artwork"]["mime"], "image/jpeg");
    Ok(())
}

#[test]
fn read_gives_a_png_image() -> Result<()> {
    let temp = TempDir::new()?;
    let mut bytes = vec![0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A];
    bytes.extend_from_slice(b"png data");
    let sha256 = write_image(temp.path(), &bytes, "png")?;
    write_display_json(temp.path(), image_track(&sha256, "image/png"))?;

    let state = read_display_state(temp.path()).ok_or_else(|| anyhow!("no state"))?;

    assert!(state.image().is_some());
    Ok(())
}

#[test]
fn read_of_a_missing_image_gives_artwork_null() -> Result<()> {
    let temp = TempDir::new()?;
    let sha256 = sha256_of(&jpeg_bytes(64));
    write_display_json(temp.path(), image_track(&sha256, "image/jpeg"))?;

    let state = read_display_state(temp.path()).ok_or_else(|| anyhow!("no state"))?;

    let track = state.track.ok_or_else(|| anyhow!("no track"))?;
    assert_eq!(track.title, "Title");
    assert_eq!(track.artwork, None);
    Ok(())
}

#[test]
fn read_of_an_image_with_a_wrong_sha256_gives_artwork_null() -> Result<()> {
    let temp = TempDir::new()?;
    let named = sha256_of(&jpeg_bytes(64));
    fs::write(temp.path().join(format!("{named}.jpg")), jpeg_bytes(65))?;
    write_display_json(temp.path(), image_track(&named, "image/jpeg"))?;

    let state = read_display_state(temp.path()).ok_or_else(|| anyhow!("no state"))?;

    assert!(state.track.is_some());
    assert!(state.image().is_none());
    Ok(())
}

#[test]
fn read_of_an_image_at_the_limit_gives_the_image() -> Result<()> {
    let temp = TempDir::new()?;
    let limit = usize::try_from(MAX_IMAGE_BYTES)?;
    let sha256 = write_image(temp.path(), &jpeg_bytes(limit), "jpg")?;
    write_display_json(temp.path(), image_track(&sha256, "image/jpeg"))?;

    let state = read_display_state(temp.path()).ok_or_else(|| anyhow!("no state"))?;

    assert_eq!(state.image().map(|image| image.bytes().len()), Some(limit));
    Ok(())
}

#[test]
fn read_of_an_image_over_the_limit_gives_artwork_null() -> Result<()> {
    let temp = TempDir::new()?;
    let limit = usize::try_from(MAX_IMAGE_BYTES)?;
    let sha256 = write_image(temp.path(), &jpeg_bytes(limit + 1), "jpg")?;
    write_display_json(temp.path(), image_track(&sha256, "image/jpeg"))?;

    let state = read_display_state(temp.path()).ok_or_else(|| anyhow!("no state"))?;

    assert!(state.track.is_some());
    assert!(state.image().is_none());
    Ok(())
}

#[test]
fn read_of_an_image_that_is_not_of_its_type_gives_artwork_null() -> Result<()> {
    let temp = TempDir::new()?;
    let sha256 = write_image(temp.path(), b"GIF89a not a jpeg", "jpg")?;
    write_display_json(temp.path(), image_track(&sha256, "image/jpeg"))?;

    let state = read_display_state(temp.path()).ok_or_else(|| anyhow!("no state"))?;

    assert!(state.image().is_none());
    Ok(())
}

#[test]
fn read_gives_a_url_artwork() -> Result<()> {
    let temp = TempDir::new()?;
    write_display_json(
        temp.path(),
        json!({
            "artist": "A",
            "title": "T",
            "artwork": {"url": "https://img.example/a.gif"},
            "song_line": "A - T",
            "play_id": "1"
        }),
    )?;

    let state = read_display_state(temp.path()).ok_or_else(|| anyhow!("no state"))?;

    assert_eq!(
        state.track.and_then(|track| track.artwork),
        Some(Artwork::Url("https://img.example/a.gif".to_owned()))
    );
    Ok(())
}

#[test]
fn read_gives_the_null_state() -> Result<()> {
    let temp = TempDir::new()?;
    write_display_json(temp.path(), Value::Null)?;

    assert_eq!(read_display_state(temp.path()), Some(DisplayState::null()));
    Ok(())
}

#[test]
fn read_parses_version_2_with_song_line_and_play_id() -> Result<()> {
    let temp = TempDir::new()?;
    write_display_json(
        temp.path(),
        json!({
            "artist": "Artist",
            "title": "Title",
            "artwork": null,
            "song_line": "Artist - Title",
            "play_id": "42"
        }),
    )?;

    let state = read_display_state(temp.path()).ok_or_else(|| anyhow!("no state"))?;

    assert_eq!(
        state.track,
        Some(DisplayTrack {
            artist: "Artist".to_owned(),
            title: "Title".to_owned(),
            artwork: None,
            song_line: "Artist - Title".to_owned(),
            play_id: Some("42".to_owned()),
            album: None,
            pub_date: None,
            funding: Vec::new(),
        })
    );
    Ok(())
}

#[test]
fn read_parses_version_2_with_null_play_id() -> Result<()> {
    let temp = TempDir::new()?;
    write_display_json(
        temp.path(),
        json!({
            "artist": "Artist",
            "title": "Title",
            "artwork": null,
            "song_line": "Artist - Title",
            "play_id": null
        }),
    )?;

    let state = read_display_state(temp.path()).ok_or_else(|| anyhow!("no state"))?;

    assert_eq!(state.track.as_ref().map(|t| &t.play_id), Some(&None));
    Ok(())
}

#[test]
fn display_body_has_play_id_of_display_json() -> Result<()> {
    let temp = tempfile::tempdir()?;
    write_display_json(
        temp.path(),
        json!({"artist":"Artist","title":"Title","song_line":"Artist - Title","play_id":"ls-12500-4-30665"}),
    )?;
    let state = read_display_state(temp.path()).ok_or_else(|| anyhow!("no state"))?;
    assert_eq!(state.body()["track"]["playId"], "ls-12500-4-30665");
    Ok(())
}

#[test]
fn display_body_has_null_play_id_for_missing_empty_and_null_values() -> Result<()> {
    let temp = tempfile::tempdir()?;
    for track in [
        json!({"artist":"Artist","title":"Title","song_line":"Artist - Title","play_id":null}),
        json!({"artist":"Artist","title":"Title","song_line":"Artist - Title"}),
        json!({"artist":"Artist","title":"Title","song_line":"Artist - Title","play_id":""}),
    ] {
        write_display_json(temp.path(), track)?;
        let state = read_display_state(temp.path()).ok_or_else(|| anyhow!("no state"))?;
        assert_eq!(state.body()["track"]["playId"], Value::Null);
    }
    Ok(())
}

#[test]
fn display_body_null_play_id_when_too_long() -> Result<()> {
    let temp = tempfile::tempdir()?;
    for (length, expected) in [(128, true), (129, false)] {
        write_display_json(
            temp.path(),
            json!({"artist":"Artist","title":"Title","song_line":"Artist - Title","play_id":"x".repeat(length)}),
        )?;
        let state = read_display_state(temp.path()).ok_or_else(|| anyhow!("no state"))?;
        assert_eq!(state.body()["track"]["playId"].is_string(), expected);
    }
    Ok(())
}

#[test]
fn read_ignores_version_2_track_without_song_line() -> Result<()> {
    let temp = TempDir::new()?;
    write_display_json(
        temp.path(),
        json!({
            "artist": "Artist",
            "title": "Title",
            "artwork": null
        }),
    )?;

    assert_eq!(read_display_state(temp.path()), None);
    Ok(())
}

#[test]
fn read_ignores_version_1_with_a_warning() -> Result<()> {
    let temp = TempDir::new()?;
    fs::write(
        temp.path().join(DISPLAY_FILE_NAME),
        json!({ "schema": "musicindex.display/1", "track": null }).to_string(),
    )?;

    assert_eq!(read_display_state(temp.path()), None);
    Ok(())
}

#[test]
fn read_ignores_a_file_with_an_unknown_schema() -> Result<()> {
    let temp = TempDir::new()?;
    fs::write(
        temp.path().join(DISPLAY_FILE_NAME),
        json!({ "schema": "musicindex.display/4", "track": null }).to_string(),
    )?;

    assert_eq!(read_display_state(temp.path()), None);
    Ok(())
}

#[test]
fn read_ignores_a_missing_or_broken_file() -> Result<()> {
    let temp = TempDir::new()?;
    assert_eq!(read_display_state(temp.path()), None);

    fs::write(temp.path().join(DISPLAY_FILE_NAME), "{ not json")?;
    assert_eq!(read_display_state(temp.path()), None);
    Ok(())
}

// The token never reaches a log line.

#[derive(Clone)]
struct Capture(Arc<Mutex<Vec<u8>>>);

impl Write for Capture {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        if let Ok(mut buffer) = self.0.lock() {
            buffer.extend_from_slice(bytes);
        }
        Ok(bytes.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

fn captured_logs() -> Arc<Mutex<Vec<u8>>> {
    static LOGS: std::sync::OnceLock<Arc<Mutex<Vec<u8>>>> = std::sync::OnceLock::new();
    Arc::clone(LOGS.get_or_init(|| {
        let buffer = Arc::new(Mutex::new(Vec::new()));
        let writer = Capture(Arc::clone(&buffer));
        let subscriber = tracing_subscriber::fmt()
            .with_max_level(tracing::Level::TRACE)
            .with_ansi(false)
            .with_writer(move || writer.clone())
            .finish();
        let _ignored = tracing::subscriber::set_global_default(subscriber);
        buffer
    }))
}

#[test]
fn no_display_log_line_holds_the_token() -> Result<()> {
    let logs = captured_logs();
    let stub = Stub::start(
        vec![
            (Route::Display, error_reply(503, "unavailable")),
            (Route::Display, error_reply(409, "artwork_missing")),
            (Route::Display, error_reply(409, "artwork_missing")),
            (Route::Display, error_reply(401, "invalid_bearer_token")),
            (Route::Artwork, error_reply(413, "payload_too_large")),
            (Route::Display, error_reply(404, "event_not_found")),
        ],
        false,
    )?;
    let publisher = start(&stub)?;

    publisher.publish_display(entry(track_state("One", Some(Artwork::Image(jpeg("one"))))));
    stub.wait_for(Route::Display, 3)?;
    publisher.publish_display(entry(track_state("Two", None)));
    stub.wait_for(Route::Display, 4)?;
    publisher.publish_display(entry(track_state("Three", Some(Artwork::Image(jpeg("x"))))));
    stub.wait_for(Route::Display, 5)?;
    publisher.publish_display(entry(track_state("Four", None)));
    publisher.publish(payload("Track"))?;
    stub.wait_for(Route::Metadata, 1)?;

    // A relay that does not answer gives a network error.
    let closed = TcpListener::bind("127.0.0.1:0")?;
    let endpoint = format!("http://{}", closed.local_addr()?);
    drop(closed);
    let offline = RelayPublisher::start_with_backoff(
        &display_config(&endpoint),
        Duration::from_millis(20),
        Duration::from_millis(40),
    )?;
    offline.publish_display(entry(DisplayState::null()));
    thread::sleep(Duration::from_millis(300));

    let text = String::from_utf8_lossy(&logs.lock().map_err(|_| anyhow!("poisoned"))?).into_owned();
    assert!(
        text.contains("display request failed; backing off"),
        "{text}"
    );
    assert!(text.contains("relay refused the display state"), "{text}");
    assert!(
        text.contains("display path off until the next start"),
        "{text}"
    );
    assert!(text.contains("network error"), "{text}");
    assert!(!text.contains(TOKEN), "a log line holds the token");
    Ok(())
}

// Pairing tests (ADR 0039)

#[test]
fn display_state_with_matching_play_id_includes_value() -> Result<()> {
    let state = DisplayState {
        track: Some(DisplayTrack {
            artist: "Artist".to_owned(),
            title: "Title".to_owned(),
            artwork: None,
            song_line: "Artist - Title".to_owned(),
            play_id: Some("play-1".to_owned()),
            album: None,
            pub_date: None,
            funding: Vec::new(),
        }),
    };

    let pairing = Pairing {
        play_id: "play-1".to_owned(),
        event_guid: "event-guid".to_owned(),
        block_guid: "block-guid".to_owned(),
    };
    let body = state.body_with_pairing(Some(&pairing));

    assert_eq!(
        body,
        json!({
            "track": {
                "artist": "Artist",
                "title": "Title",
                "artwork": Value::Null,
                "songLine": "Artist - Title",
                "playId": "play-1",
                "value": {
                    "eventGuid": "event-guid",
                    "blockGuid": "block-guid"
                }
            }
        })
    );
    Ok(())
}

#[test]
fn display_state_with_mismatched_play_id_omits_value() -> Result<()> {
    let state = DisplayState {
        track: Some(DisplayTrack {
            artist: "Artist".to_owned(),
            title: "Title".to_owned(),
            artwork: None,
            song_line: "Artist - Title".to_owned(),
            play_id: Some("play-1".to_owned()),
            album: None,
            pub_date: None,
            funding: Vec::new(),
        }),
    };

    let pairing = Pairing {
        play_id: "play-2".to_owned(),
        event_guid: "event-guid".to_owned(),
        block_guid: "block-guid".to_owned(),
    };
    let body = state.body_with_pairing(Some(&pairing));

    assert_eq!(
        body.get("track").and_then(|t| t.get("value")),
        None,
        "value should not be present for mismatched play_id"
    );
    assert_eq!(
        body["track"]["songLine"], "Artist - Title",
        "songLine should always be present"
    );
    Ok(())
}

#[test]
fn display_state_without_play_id_has_no_value() -> Result<()> {
    let state = DisplayState {
        track: Some(DisplayTrack {
            artist: "Artist".to_owned(),
            title: "Title".to_owned(),
            artwork: None,
            song_line: "Artist - Title".to_owned(),
            play_id: None,
            album: None,
            pub_date: None,
            funding: Vec::new(),
        }),
    };

    let pairing = Pairing {
        play_id: "play-1".to_owned(),
        event_guid: "event-guid".to_owned(),
        block_guid: "block-guid".to_owned(),
    };
    let body = state.body_with_pairing(Some(&pairing));

    assert_eq!(
        body.get("track").and_then(|t| t.get("value")),
        None,
        "value should not be present when track has no play_id"
    );
    Ok(())
}

#[test]
fn null_state_always_has_no_value() -> Result<()> {
    let state = DisplayState::null();

    let pairing = Pairing {
        play_id: "play-1".to_owned(),
        event_guid: "event-guid".to_owned(),
        block_guid: "block-guid".to_owned(),
    };
    let body = state.body_with_pairing(Some(&pairing));

    assert_eq!(body, json!({ "track": null }));
    Ok(())
}

// Worker integration tests (ADR 0012 task 002)
#[test]
fn pairing_track_without_drop_file_has_no_value() -> Result<()> {
    // Criterion c: no drop file means no value
    let stub = Stub::start(Vec::new(), false)?;
    let publisher = start(&stub)?;

    let state = DisplayState {
        track: Some(DisplayTrack {
            artist: "Artist".to_owned(),
            title: "Track".to_owned(),
            artwork: None,
            song_line: "Artist - Track".to_owned(),
            play_id: None,
            album: None,
            pub_date: None,
            funding: Vec::new(),
        }),
    };

    publisher.publish_display(DisplayEntry {
        event_id: "event-guid".to_owned(),
        state,
    });

    stub.wait_for(Route::Display, 1)?;
    let display_requests = stub.of(Route::Display);
    assert_eq!(
        display_requests[0].json()["track"]["songLine"],
        "Artist - Track"
    );
    assert!(display_requests[0].json()["track"].get("value").is_none());
    Ok(())
}

#[test]
fn pairing_after_dead_block_has_no_value() -> Result<()> {
    // Criterion d: display state always has songLine; after dead block no value
    let stub = Stub::start(Vec::new(), false)?;
    let publisher = start(&stub)?;

    let state = DisplayState {
        track: Some(DisplayTrack {
            artist: "Artist".to_owned(),
            title: "Track".to_owned(),
            artwork: None,
            song_line: "Artist - Track".to_owned(),
            play_id: Some("play-1".to_owned()),
            album: None,
            pub_date: None,
            funding: Vec::new(),
        }),
    };

    // Send after dead block (no payload): should have songLine, no value
    let dead = LiveValuePayload {
        title: "No V4V".to_owned(),
        image: None,
        description: String::new(),
        kind: "music".to_owned(),
        start_time: 0,
        duration: None,
        event_guid: "event-guid".to_owned(),
        block_guid: "block-dead".to_owned(),
        feed_guid: None,
        item_guid: None,
        line: None,
        author: None,
        podcast_name: None,
        value: LiveValue {
            model: LiveValueModel {
                kind: "lightning".to_owned(),
                method: "lnaddress".to_owned(),
                suggested: None,
            },
            destinations: vec![],
        },
        play_id: None,
        log: None,
    };
    publisher.publish(dead)?;

    publisher.publish_display(DisplayEntry {
        event_id: "event-guid".to_owned(),
        state,
    });

    stub.wait_for(Route::Display, 1)?;
    let display = stub.of(Route::Display)[0].json();
    assert_eq!(display["track"]["songLine"], "Artist - Track");
    assert!(display["track"].get("value").is_none());
    Ok(())
}

fn paired_track(song_line: &str, play_id: &str, artwork: Option<Artwork>) -> DisplayState {
    DisplayState {
        track: Some(DisplayTrack {
            artist: "Artist".to_owned(),
            title: "Track".to_owned(),
            artwork,
            song_line: song_line.to_owned(),
            play_id: Some(play_id.to_owned()),
            album: None,
            pub_date: None,
            funding: Vec::new(),
        }),
    }
}

fn paired_payload(play_id: &str, block_guid: &str) -> LiveValuePayload {
    let mut payload = payload("Track");
    payload.play_id = Some(play_id.to_owned());
    payload.block_guid = block_guid.to_owned();
    payload
}

/// Waits for a display request whose `track.value.blockGuid` is `block_guid`.
fn wait_for_paired_display(stub: &Stub, block_guid: &str) -> Result<()> {
    stub.wait_until(&format!("a display paired with {block_guid}"), |requests| {
        requests.iter().any(|request| {
            request.route == Route::Display
                && request.json()["track"]["value"]["blockGuid"] == block_guid
        })
    })
}

#[test]
fn pairing_display_after_payload_includes_value() -> Result<()> {
    // Criterion a: a display state after its payload names that payload.
    let stub = Stub::start(Vec::new(), false)?;
    let publisher = start(&stub)?;

    publisher.publish(paired_payload("play-1", "block-1"))?;
    stub.wait_for(Route::Metadata, 1)?;
    publisher.publish_display(DisplayEntry {
        event_id: "event-guid".to_owned(),
        state: paired_track("Artist - Track", "play-1", None),
    });

    wait_for_paired_display(&stub, "block-1")?;
    let displays = stub.of(Route::Display);
    let body = displays[displays.len() - 1].json();
    assert_eq!(body["track"]["songLine"], "Artist - Track");
    assert_eq!(
        body["track"]["value"],
        json!({ "eventGuid": "event-guid", "blockGuid": "block-1" })
    );
    Ok(())
}

#[test]
fn pairing_display_before_payload_resends_with_value() -> Result<()> {
    // Criterion b: a display state before its payload goes out without
    // `value`, then one more time with `value`. The image goes up one time.
    let stub = Stub::start(Vec::new(), false)?;
    let publisher = start(&stub)?;
    let state = paired_track(
        "Artist - Track",
        "play-1",
        Some(Artwork::Image(jpeg("cover"))),
    );

    publisher.publish_display(DisplayEntry {
        event_id: "event-guid".to_owned(),
        state,
    });
    stub.wait_for(Route::Display, 1)?;
    let first = stub.of(Route::Display)[0].json();
    assert_eq!(first["track"]["songLine"], "Artist - Track");
    assert!(first["track"].get("value").is_none(), "{first}");

    publisher.publish(paired_payload("play-1", "block-1"))?;
    wait_for_paired_display(&stub, "block-1")?;
    stub.expect_quiet(Duration::from_millis(300))?;

    let displays = stub.of(Route::Display);
    assert_eq!(displays.len(), 2, "one resend only");
    let second = displays[1].json();
    assert_eq!(second["track"]["songLine"], "Artist - Track");
    assert_eq!(
        second["track"]["value"],
        json!({ "eventGuid": "event-guid", "blockGuid": "block-1" })
    );
    assert_eq!(stub.of(Route::Artwork).len(), 1, "no second upload");
    Ok(())
}

#[test]
fn pairing_two_plays_of_one_track_name_the_second_block() -> Result<()> {
    // Criterion e: two plays of the same track. The second display state
    // names the second block, never the first.
    let stub = Stub::start(Vec::new(), false)?;
    let publisher = start(&stub)?;

    publisher.publish(paired_payload("play-1", "block-1"))?;
    stub.wait_for(Route::Metadata, 1)?;
    publisher.publish_display(DisplayEntry {
        event_id: "event-guid".to_owned(),
        state: paired_track("Artist - Track", "play-1", None),
    });
    wait_for_paired_display(&stub, "block-1")?;

    publisher.publish(paired_payload("play-2", "block-2"))?;
    stub.wait_for(Route::Metadata, 2)?;
    publisher.publish_display(DisplayEntry {
        event_id: "event-guid".to_owned(),
        state: paired_track("Artist - Track", "play-2", None),
    });
    wait_for_paired_display(&stub, "block-2")?;
    stub.expect_quiet(Duration::from_millis(300))?;

    let displays = stub.of(Route::Display);
    let last = displays[displays.len() - 1].json();
    assert_eq!(last["track"]["value"]["blockGuid"], "block-2");
    // No request after the first block-2 display names block-1.
    let first_block_2 = displays
        .iter()
        .position(|request| request.json()["track"]["value"]["blockGuid"] == "block-2")
        .ok_or_else(|| anyhow!("no block-2 display"))?;
    assert!(
        displays[first_block_2..]
            .iter()
            .all(|request| request.json()["track"]["value"]["blockGuid"] == "block-2")
    );
    Ok(())
}

#[test]
fn pairing_second_play_before_its_payload_resends_with_the_second_block() -> Result<()> {
    // The pairing of the first play exists when the display state of the
    // second play goes out. That state has no `value`, so it goes out again
    // when the second payload arrives.
    let stub = Stub::start(Vec::new(), false)?;
    let publisher = start(&stub)?;

    publisher.publish(paired_payload("play-1", "block-1"))?;
    stub.wait_for(Route::Metadata, 1)?;
    publisher.publish_display(DisplayEntry {
        event_id: "event-guid".to_owned(),
        state: paired_track("Artist - Track", "play-1", None),
    });
    wait_for_paired_display(&stub, "block-1")?;
    stub.expect_quiet(Duration::from_millis(300))?;
    let before = stub.of(Route::Display).len();

    publisher.publish_display(DisplayEntry {
        event_id: "event-guid".to_owned(),
        state: paired_track("Artist - Track", "play-2", None),
    });
    stub.wait_for(Route::Display, before + 1)?;
    let unpaired = stub.of(Route::Display)[before].json();
    assert!(unpaired["track"].get("value").is_none(), "{unpaired}");

    publisher.publish(paired_payload("play-2", "block-2"))?;
    wait_for_paired_display(&stub, "block-2")?;
    stub.expect_quiet(Duration::from_millis(300))?;
    assert_eq!(stub.of(Route::Display).len(), before + 2, "one resend only");
    Ok(())
}

#[test]
fn read_parses_version_3_with_album() -> Result<()> {
    let temp = TempDir::new()?;
    write_display_json(
        temp.path(),
        json!({
            "artist": "Artist",
            "title": "Title",
            "artwork": null,
            "song_line": "Artist - Title",
            "play_id": "42",
            "album": "Test Album"
        }),
    )?;

    let state = read_display_state(temp.path()).ok_or_else(|| anyhow!("no state"))?;

    assert_eq!(
        state.track,
        Some(DisplayTrack {
            artist: "Artist".to_owned(),
            title: "Title".to_owned(),
            artwork: None,
            song_line: "Artist - Title".to_owned(),
            play_id: Some("42".to_owned()),
            album: Some("Test Album".to_owned()),
            pub_date: None,
            funding: Vec::new(),
        })
    );
    Ok(())
}

#[test]
fn read_parses_version_3_with_null_album() -> Result<()> {
    let temp = TempDir::new()?;
    write_display_json(
        temp.path(),
        json!({
            "artist": "Artist",
            "title": "Title",
            "artwork": null,
            "song_line": "Artist - Title",
            "play_id": "42",
            "album": null
        }),
    )?;

    let state = read_display_state(temp.path()).ok_or_else(|| anyhow!("no state"))?;

    assert_eq!(
        state.track,
        Some(DisplayTrack {
            artist: "Artist".to_owned(),
            title: "Title".to_owned(),
            artwork: None,
            song_line: "Artist - Title".to_owned(),
            play_id: Some("42".to_owned()),
            album: None,
            pub_date: None,
            funding: Vec::new(),
        })
    );
    Ok(())
}

#[test]
fn read_ignores_version_2_with_warning() -> Result<()> {
    let temp = TempDir::new()?;
    fs::write(
        temp.path().join(DISPLAY_FILE_NAME),
        json!({ "schema": "musicindex.display/2", "track": null }).to_string(),
    )?;

    assert_eq!(read_display_state(temp.path()), None);
    Ok(())
}

#[test]
fn display_body_with_album_includes_album_key() -> Result<()> {
    let state = DisplayState {
        track: Some(DisplayTrack {
            artist: "Artist".to_owned(),
            title: "Title".to_owned(),
            artwork: None,
            song_line: "Artist - Title".to_owned(),
            play_id: Some("1".to_owned()),
            album: Some("Test Album".to_owned()),
            pub_date: None,
            funding: Vec::new(),
        }),
    };

    let body = state.body();
    assert_eq!(body["track"]["album"], "Test Album");
    Ok(())
}

#[test]
fn display_body_without_album_omits_album_key() -> Result<()> {
    let state = DisplayState {
        track: Some(DisplayTrack {
            artist: "Artist".to_owned(),
            title: "Title".to_owned(),
            artwork: None,
            song_line: "Artist - Title".to_owned(),
            play_id: Some("1".to_owned()),
            album: None,
            pub_date: None,
            funding: Vec::new(),
        }),
    };

    let body = state.body();
    assert!(body["track"].get("album").is_none());
    Ok(())
}

#[test]
fn display_keys_are_filtered_and_added_to_the_wire_body() -> Result<()> {
    let temp = TempDir::new()?;
    write_display_json(
        temp.path(),
        json!({
            "artist": "Artist",
            "title": "Title",
            "song_line": "Artist - Title",
            "pub_date": "2026-11-12",
            "funding": [
                {"url": "https://one.example/support", "label": "Support"},
                {"url": "https://two.example/support"},
                {"url": "https:///invalid"}
            ]
        }),
    )?;
    let state = read_display_state(temp.path()).ok_or_else(|| anyhow!("no state"))?;
    let body = state.body();
    assert_eq!(body["track"]["pubDate"], "2026-11-12");
    assert_eq!(body["track"]["funding"].as_array().unwrap().len(), 2);
    assert_eq!(body["track"]["funding"][0]["label"], "Support");
    assert!(body["track"]["funding"][1].get("label").is_none());
    Ok(())
}

#[test]
fn invalid_or_empty_display_keys_are_omitted() -> Result<()> {
    let temp = TempDir::new()?;
    for optional in [
        json!({"pub_date": null, "funding": null}),
        json!({"pub_date": "", "funding": []}),
        json!({"funding": [{"url": "https:///support"}]}),
        json!({}),
    ] {
        let mut track =
            json!({"artist": "Artist", "title": "Title", "song_line": "Artist - Title"});
        for (key, value) in optional.as_object().unwrap() {
            track[key] = value.clone();
        }
        write_display_json(temp.path(), track)?;
        let state = read_display_state(temp.path()).ok_or_else(|| anyhow!("no state"))?;
        assert!(state.body()["track"].get("pubDate").is_none());
        assert!(state.body()["track"].get("funding").is_none());
    }
    Ok(())
}

#[test]
fn oversized_funding_body_is_sent_without_funding() -> Result<()> {
    let temp = TempDir::new()?;
    write_display_json(
        temp.path(),
        json!({
            "artist": "Artist",
            "title": "x".repeat(9_000),
            "song_line": "Artist - Title",
            "play_id": "oversized-play",
            "album": "Album",
            "pub_date": "2026-11-12",
            "funding": [{"url": "https://example.com/support"}]
        }),
    )?;
    let state = read_display_state(temp.path()).ok_or_else(|| anyhow!("no state"))?;
    let body = state.body();
    assert!(body["track"].get("funding").is_none());
    assert_eq!(body["track"]["title"].as_str().unwrap().len(), 9_000);
    assert_eq!(body["track"]["pubDate"], "2026-11-12");
    assert_eq!(body["track"]["album"], "Album");
    Ok(())
}

#[test]
fn pairing_display_with_album_includes_album_in_value() -> Result<()> {
    // Verify that album is included in the display body when sent with value.
    let stub = Stub::start(Vec::new(), false)?;
    let publisher = start(&stub)?;

    publisher.publish(paired_payload("play-1", "block-1"))?;
    stub.wait_for(Route::Metadata, 1)?;
    publisher.publish_display(DisplayEntry {
        event_id: "event-guid".to_owned(),
        state: DisplayState {
            track: Some(DisplayTrack {
                artist: "Artist".to_owned(),
                title: "Track".to_owned(),
                artwork: None,
                song_line: "Artist - Track".to_owned(),
                play_id: Some("play-1".to_owned()),
                album: Some("Album".to_owned()),
                pub_date: None,
                funding: Vec::new(),
            }),
        },
    });

    wait_for_paired_display(&stub, "block-1")?;
    let displays = stub.of(Route::Display);
    let body = displays[displays.len() - 1].json();
    assert_eq!(body["track"]["album"], "Album");
    assert_eq!(
        body["track"]["value"],
        json!({ "eventGuid": "event-guid", "blockGuid": "block-1" })
    );
    Ok(())
}

#[test]
fn a_display_state_with_no_album_goes_out_with_no_album_key() -> Result<()> {
    // ADR 0013: the publisher never sends an empty or a null `album`.
    let stub = Stub::start(Vec::new(), false)?;
    let publisher = start(&stub)?;
    publisher.publish_display(DisplayEntry {
        event_id: "event-guid".to_owned(),
        state: DisplayState {
            track: Some(DisplayTrack {
                artist: "Artist".to_owned(),
                title: "Track".to_owned(),
                artwork: None,
                song_line: "Artist - Track".to_owned(),
                play_id: None,
                album: None,
                pub_date: None,
                funding: Vec::new(),
            }),
        },
    });

    stub.wait_for(Route::Display, 1)?;
    let body = stub.of(Route::Display)[0].json();
    assert_eq!(body["track"]["songLine"], "Artist - Track");
    assert!(body["track"].get("album").is_none(), "{body}");
    Ok(())
}

// The payload image (ADR 0034 §Verification After Implementation).

/// The longest wait of the first send in the tests of [`waiting_start`].
/// It is longer than [`WAIT`], so a payload that falls back to the timeout
/// makes the test fail.
const LONG_IMAGE_WAIT: Duration = Duration::from_secs(5);

/// Starts a publisher whose first send waits [`LONG_IMAGE_WAIT`] for the
/// picture of its play.
fn waiting_start(stub: &Stub) -> Result<RelayPublisher> {
    RelayPublisher::start_with_payload_image_wait(
        &display_config(&stub.endpoint),
        Duration::from_millis(20),
        Duration::from_millis(80),
        None,
        LONG_IMAGE_WAIT,
    )
}

/// A payable payload of one play. A payload with no destination is a dead
/// block, and a dead block never waits and never gets a second send.
fn payable_payload(play_id: &str, block_guid: &str) -> LiveValuePayload {
    let mut payload = paired_payload(play_id, block_guid);
    payload.value.destinations = vec![LiveValueDestination {
        kind: Some("node".to_owned()),
        name: Some("Alice".to_owned()),
        address: Some("03alice".to_owned()),
        split: Some("100".to_owned()),
        custom_key: None,
        custom_value: None,
        fee: None,
    }];
    payload
}

fn metadata_bodies(stub: &Stub) -> Vec<Value> {
    stub.of(Route::Metadata)
        .iter()
        .map(Recorded::json)
        .collect()
}

/// Publishes the payload of `play-1` and its display state at the same
/// time, as the watcher does, and waits until the stub is quiet. Gives the
/// metadata bodies.
fn payload_with_display(
    stub: &Stub,
    publisher: &RelayPublisher,
    payload: LiveValuePayload,
    artwork: Option<Artwork>,
) -> Result<Vec<Value>> {
    let block_guid = payload.block_guid.clone();
    publisher.publish(payload)?;
    publisher.publish_display(entry(paired_track("Artist - Track", "play-1", artwork)));
    stub.wait_for(Route::Metadata, 1)?;
    wait_for_paired_display(stub, &block_guid)?;
    stub.expect_quiet(Duration::from_millis(300))?;
    Ok(metadata_bodies(stub))
}

fn relay_image_url(endpoint: &str, image: &ArtworkImage) -> String {
    format!(
        "{endpoint}/v1/liveitems/event-guid/artwork/{}",
        image.sha256()
    )
}

#[test]
fn the_first_send_waits_for_the_upload_and_names_the_relay_url() -> Result<()> {
    let stub = Stub::start(Vec::new(), false)?;
    let publisher = waiting_start(&stub)?;
    let image = jpeg("cover");

    let sent = payload_with_display(
        &stub,
        &publisher,
        payable_payload("play-1", "block-1"),
        Some(Artwork::Image(image.clone())),
    )?;

    assert_eq!(sent.len(), 1, "no second send: {sent:?}");
    assert_eq!(sent[0]["image"], relay_image_url(&stub.endpoint, &image));
    let routes = stub.routes();
    assert_eq!(
        routes[..3],
        [Route::Artwork, Route::Display, Route::Metadata],
        "the payload goes after the display state: {routes:?}"
    );
    Ok(())
}

#[test]
fn the_relay_url_is_the_top_level_image_in_the_form_of_relay_adr_0011() -> Result<()> {
    // Relay ADR 0011 keeps an image only when the top-level `image` ends
    // with this path. If `image` moves, no relay test finds it.
    let stub = Stub::start(Vec::new(), false)?;
    let publisher = waiting_start(&stub)?;
    let image = jpeg("cover");

    let sent = payload_with_display(
        &stub,
        &publisher,
        payable_payload("play-1", "block-1"),
        Some(Artwork::Image(image.clone())),
    )?;

    let url = sent[0]["image"]
        .as_str()
        .ok_or_else(|| anyhow!("no top-level image: {}", sent[0]))?;
    let suffix = format!("/v1/liveitems/event-guid/artwork/{}", image.sha256());
    assert!(url.ends_with(&suffix), "{url}");
    let hash = &url[url.len() - 64..];
    assert!(
        hash.chars()
            .all(|ch| ch.is_ascii_digit() || ('a'..='f').contains(&ch)),
        "{hash}"
    );
    assert!(url.starts_with("http://") || url.starts_with("https://"));
    Ok(())
}

#[test]
fn an_upload_with_201_gives_the_relay_url() -> Result<()> {
    let stub = Stub::start(
        vec![(Route::Artwork, reply(201, json!({ "stored": true })))],
        false,
    )?;
    let publisher = waiting_start(&stub)?;
    let image = jpeg("cover");

    let sent = payload_with_display(
        &stub,
        &publisher,
        payable_payload("play-1", "block-1"),
        Some(Artwork::Image(image.clone())),
    )?;

    assert_eq!(sent.len(), 1, "{sent:?}");
    assert_eq!(sent[0]["image"], relay_image_url(&stub.endpoint, &image));
    Ok(())
}

#[test]
fn a_display_url_goes_into_the_first_send() -> Result<()> {
    let stub = Stub::start(Vec::new(), false)?;
    let publisher = waiting_start(&stub)?;

    let sent = payload_with_display(
        &stub,
        &publisher,
        payable_payload("play-1", "block-1"),
        Some(Artwork::Url("https://img.example/a.jpg".to_owned())),
    )?;

    assert_eq!(sent.len(), 1, "{sent:?}");
    assert_eq!(sent[0]["image"], "https://img.example/a.jpg");
    assert_eq!(stub.of(Route::Artwork).len(), 0);
    Ok(())
}

#[test]
fn a_drop_file_image_wins_and_does_not_wait() -> Result<()> {
    let stub = Stub::start(Vec::new(), false)?;
    let publisher = waiting_start(&stub)?;
    let mut payload = payable_payload("play-1", "block-1");
    payload.image = Some("https://feed.example/art.png".to_owned());

    publisher.publish(payload)?;
    stub.wait_for(Route::Metadata, 1)?;
    assert_eq!(stub.of(Route::Display).len(), 0, "it did not wait");
    publisher.publish_display(entry(paired_track(
        "Artist - Track",
        "play-1",
        Some(Artwork::Image(jpeg("cover"))),
    )));
    wait_for_paired_display(&stub, "block-1")?;
    stub.expect_quiet(Duration::from_millis(300))?;

    let sent = metadata_bodies(&stub);
    assert_eq!(sent.len(), 1, "{sent:?}");
    assert_eq!(sent[0]["image"], "https://feed.example/art.png");
    Ok(())
}

#[test]
fn a_dead_block_does_not_wait() -> Result<()> {
    let stub = Stub::start(Vec::new(), false)?;
    let publisher = waiting_start(&stub)?;

    publisher.publish(paired_payload("play-1", "block-1"))?;
    stub.wait_for(Route::Metadata, 1)?;

    assert_eq!(stub.of(Route::Display).len(), 0, "it did not wait");
    Ok(())
}

#[test]
fn no_artwork_releases_the_first_send_at_once() -> Result<()> {
    let stub = Stub::start(Vec::new(), false)?;
    let publisher = waiting_start(&stub)?;
    let started = Instant::now();

    let sent = payload_with_display(
        &stub,
        &publisher,
        payable_payload("play-1", "block-1"),
        None,
    )?;

    assert!(
        started.elapsed() < LONG_IMAGE_WAIT,
        "the payload waited for the timeout"
    );
    assert_eq!(sent.len(), 1, "{sent:?}");
    assert!(sent[0].get("image").is_none());
    Ok(())
}

#[test]
fn a_refused_upload_releases_the_first_send_with_no_image() -> Result<()> {
    // The relay refuses the same image each time. The paired display state
    // sends the upload again.
    let stub = Stub::start(
        vec![(Route::Artwork, error_reply(400, "invalid_artwork")); 3],
        false,
    )?;
    let publisher = waiting_start(&stub)?;
    let started = Instant::now();

    let sent = payload_with_display(
        &stub,
        &publisher,
        payable_payload("play-1", "block-1"),
        Some(Artwork::Image(jpeg("cover"))),
    )?;

    assert!(
        started.elapsed() < LONG_IMAGE_WAIT,
        "the payload waited for the timeout"
    );
    assert_eq!(sent.len(), 1, "{sent:?}");
    assert!(sent[0].get("image").is_none());
    let display = stub.of(Route::Display)[0].json();
    assert_eq!(display["track"]["artwork"], Value::Null);
    Ok(())
}

#[test]
fn a_target_without_a_display_path_does_not_wait() -> Result<()> {
    let stub = Stub::start(Vec::new(), false)?;
    let mut config = display_config(&stub.endpoint);
    config.targets[0].display_dir = None;
    let publisher = RelayPublisher::start_with_payload_image_wait(
        &config,
        Duration::from_millis(20),
        Duration::from_millis(80),
        None,
        LONG_IMAGE_WAIT,
    )?;

    publisher.publish(payable_payload("play-1", "block-1"))?;
    stub.wait_for(Route::Metadata, 1)?;
    publisher.publish_display(entry(paired_track(
        "Artist - Track",
        "play-1",
        Some(Artwork::Image(jpeg("cover"))),
    )));
    stub.expect_quiet(Duration::from_millis(300))?;

    assert_eq!(stub.of(Route::Metadata).len(), 1);
    assert_eq!(stub.of(Route::Artwork).len(), 0);
    Ok(())
}

#[test]
fn a_display_state_before_the_payload_gives_one_send_with_the_image() -> Result<()> {
    // The display state goes out before the payload, then one more time
    // with `value`. Each accepted display state names the image, but the
    // play gets one send only.
    let stub = Stub::start(Vec::new(), false)?;
    let publisher = waiting_start(&stub)?;
    let image = jpeg("cover");
    let state = paired_track(
        "Artist - Track",
        "play-1",
        Some(Artwork::Image(image.clone())),
    );

    publisher.publish_display(entry(state.clone()));
    stub.wait_for(Route::Display, 1)?;
    // The stub records a request before it replies. This gives the display
    // worker time to complete and release the play before the payload.
    thread::sleep(Duration::from_millis(100));
    let started = Instant::now();
    publisher.publish(payable_payload("play-1", "block-1"))?;
    stub.wait_for(Route::Metadata, 1)?;
    assert!(
        started.elapsed() < Duration::from_secs(1),
        "the payload waited after the release"
    );
    wait_for_paired_display(&stub, "block-1")?;
    publisher.publish_display(entry(state));
    stub.wait_for(Route::Display, 3)?;
    stub.expect_quiet(Duration::from_millis(300))?;

    let sent = metadata_bodies(&stub);
    assert_eq!(sent.len(), 1, "{sent:?}");
    assert_eq!(sent[0]["image"], relay_image_url(&stub.endpoint, &image));
    assert_eq!(sent[0]["blockGuid"], "block-1");
    Ok(())
}

#[test]
fn a_newer_payload_replaces_a_waiting_payload() -> Result<()> {
    let stub = Stub::start(Vec::new(), false)?;
    let publisher = waiting_start(&stub)?;

    publisher.publish(payable_payload("play-1", "block-1"))?;
    publisher.publish(payable_payload("play-2", "block-2"))?;
    publisher.publish_display(entry(paired_track(
        "Artist - Track",
        "play-2",
        Some(Artwork::Url("https://img.example/b.jpg".to_owned())),
    )));
    stub.wait_for(Route::Metadata, 1)?;
    wait_for_paired_display(&stub, "block-2")?;
    stub.expect_quiet(Duration::from_millis(300))?;

    let sent = metadata_bodies(&stub);
    assert_eq!(sent.len(), 1, "{sent:?}");
    assert_eq!(sent[0]["blockGuid"], "block-2");
    assert_eq!(sent[0]["image"], "https://img.example/b.jpg");
    Ok(())
}

#[test]
fn a_slow_upload_gives_a_first_send_with_no_image_and_a_second_send() -> Result<()> {
    let stub = Stub::start(Vec::new(), true)?;
    let publisher = start(&stub)?;
    let image = jpeg("cover");

    let started = Instant::now();
    publisher.publish(payable_payload("play-1", "block-1"))?;
    publisher.publish_display(entry(paired_track(
        "Artist - Track",
        "play-1",
        Some(Artwork::Image(image.clone())),
    )));
    stub.wait_for(Route::Metadata, 1)?;
    let waited = started.elapsed();
    assert!(
        waited >= Duration::from_millis(250) && waited < Duration::from_millis(1500),
        "the first send goes at the end of the 300 ms wait: {waited:?}"
    );
    assert!(stub.of(Route::Metadata)[0].json().get("image").is_none());
    stub.release();
    stub.wait_for(Route::Metadata, 2)?;
    wait_for_paired_display(&stub, "block-1")?;
    stub.expect_quiet(Duration::from_millis(300))?;

    let sent = metadata_bodies(&stub);
    assert_eq!(sent.len(), 2, "one second send: {sent:?}");
    assert_eq!(sent[1]["image"], relay_image_url(&stub.endpoint, &image));
    for key in ["eventGuid", "blockGuid", "value", "title"] {
        assert_eq!(sent[0][key], sent[1][key], "{key}");
    }
    Ok(())
}

#[test]
fn a_newer_payload_stops_the_second_send() -> Result<()> {
    let stub = Stub::start(Vec::new(), false)?;
    let publisher = start(&stub)?;

    publisher.publish(payable_payload("play-1", "block-1"))?;
    stub.wait_for(Route::Metadata, 1)?;
    publisher.publish(payable_payload("play-2", "block-2"))?;
    stub.wait_for(Route::Metadata, 2)?;
    publisher.publish_display(entry(paired_track(
        "Artist - Track",
        "play-1",
        Some(Artwork::Url("https://img.example/old.jpg".to_owned())),
    )));
    stub.wait_for(Route::Display, 1)?;
    stub.expect_quiet(Duration::from_millis(300))?;

    let sent = stub.of(Route::Metadata);
    assert_eq!(sent.len(), 2);
    assert!(
        sent.iter()
            .all(|request| request.json().get("image").is_none())
    );
    Ok(())
}

#[test]
fn the_second_send_adds_no_show_log_line() -> Result<()> {
    let stub = Stub::start(Vec::new(), false)?;
    let state_dir = TempDir::new()?;
    let log_dir = state_dir.path().join("showlog");
    let log = Arc::new(ShowLog::open(&log_dir, 365)?);
    let publisher = RelayPublisher::start_with_payload_image_wait(
        &display_config(&stub.endpoint),
        Duration::from_millis(20),
        Duration::from_millis(80),
        Some(log),
        Duration::from_millis(100),
    )?;
    let mut payload = payable_payload("play-1", "block-1");
    let mut facts = LogFacts::clear(SystemTime::now());
    facts.kind = ShowLogEntryKind::Track;
    facts.payable = Some(true);
    facts.play_id = Some("play-1".to_owned());
    payload.log = Some(facts);

    publisher.publish(payload)?;
    stub.wait_for(Route::Metadata, 1)?;
    publisher.publish_display(entry(paired_track(
        "Artist - Track",
        "play-1",
        Some(Artwork::Url("https://img.example/a.jpg".to_owned())),
    )));
    stub.wait_for(Route::Metadata, 2)?;
    stub.expect_quiet(Duration::from_millis(300))?;

    let mut lines = 0;
    for file in fs::read_dir(&log_dir)? {
        lines += fs::read_to_string(file?.path())?.lines().count();
    }
    assert_eq!(lines, 1, "one line for the first send only");
    Ok(())
}

#[test]
fn after_a_keepalive_409_the_republish_has_no_relay_url_and_a_second_send_follows() -> Result<()> {
    // The lease end removed the images in the relay. The republish must not
    // name the deleted image. After the new upload, the play gets the relay
    // URL again.
    let stub = Stub::start(
        vec![
            (Route::Metadata, accepted_with_interval(1, 1)),
            (Route::Keepalive, error_reply(409, "lease_expired")),
            (Route::Metadata, accepted_with_interval(2, 60)),
        ],
        false,
    )?;
    let publisher = waiting_start(&stub)?;
    publisher.set_producer(ProducerState::Running);
    let image = jpeg("cover");

    publisher.publish(payable_payload("play-1", "block-1"))?;
    publisher.publish_display(entry(paired_track(
        "Artist - Track",
        "play-1",
        Some(Artwork::Image(image.clone())),
    )));
    stub.wait_for(Route::Keepalive, 1)?;
    stub.wait_for(Route::Metadata, 3)?;
    stub.expect_quiet(Duration::from_millis(300))?;

    let sent = metadata_bodies(&stub);
    let url = relay_image_url(&stub.endpoint, &image);
    assert_eq!(sent.len(), 3, "{sent:?}");
    assert_eq!(sent[0]["image"], url, "the first send");
    assert!(sent[1].get("image").is_none(), "the republish: {}", sent[1]);
    assert_eq!(sent[2]["image"], url, "the new second send");
    assert!(sent.iter().all(|body| body["blockGuid"] == "block-1"));
    assert_eq!(stub.of(Route::Artwork).len(), 2);
    Ok(())
}

#[test]
fn an_image_during_a_keepalive_retry_gives_the_second_send_after_it() -> Result<()> {
    let mut replies = vec![(Route::Metadata, accepted_with_interval(1, 1))];
    for _ in 0..8 {
        replies.push((Route::Keepalive, error_reply(503, "unavailable")));
    }
    let stub = Stub::start(replies, false)?;
    let publisher = start(&stub)?;
    publisher.set_producer(ProducerState::Running);

    publisher.publish(payable_payload("play-1", "block-1"))?;
    stub.wait_for(Route::Keepalive, 1)?;
    publisher.publish_display(entry(paired_track(
        "Artist - Track",
        "play-1",
        Some(Artwork::Url("https://img.example/a.jpg".to_owned())),
    )));
    stub.wait_for(Route::Metadata, 2)?;

    let routes = stub.routes();
    let second = routes
        .iter()
        .rposition(|route| *route == Route::Metadata)
        .ok_or_else(|| anyhow!("no second send"))?;
    assert_eq!(
        routes[..second]
            .iter()
            .filter(|route| **route == Route::Keepalive)
            .count(),
        9,
        "the second send comes after the keepalive retry: {routes:?}"
    );
    let sent = stub.of(Route::Metadata);
    assert_eq!(sent[1].json()["image"], "https://img.example/a.jpg");
    Ok(())
}
