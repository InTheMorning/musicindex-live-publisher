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
use std::time::{Duration, Instant};

use anyhow::{Result, anyhow};
use musicindex_live_publisher::{
    Artwork, ArtworkImage, DISPLAY_FILE_NAME, DisplayEntry, DisplayOutcome, DisplayState,
    DisplayTrack, ImageMime, LiveValue, LiveValueModel, LiveValuePayload, MAX_IMAGE_BYTES,
    ProducerState, PublishSchedule, PublisherConfig, PublisherTarget, RelayClient, RelayPublisher,
    RelayTarget, ScheduledItem, read_display_state,
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

fn route_of(path: &str) -> Route {
    if path.ends_with("/metadata") {
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

    let route = route_of(&path);
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
        value: LiveValue {
            model: LiveValueModel {
                kind: "lightning".to_owned(),
                method: "keysend".to_owned(),
                suggested: None,
            },
            destinations: Vec::new(),
        },
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
            display_dir: Some(PathBuf::from("/nonexistent/display")),
        }],
    }
}

fn start(stub: &Stub) -> Result<RelayPublisher> {
    RelayPublisher::start_with_backoff(
        &display_config(&stub.endpoint),
        Duration::from_millis(20),
        Duration::from_millis(80),
    )
}

// The relay requests.

#[test]
fn display_publish_body_holds_only_track_and_no_schema_key() -> Result<()> {
    let stub = Stub::start(Vec::new(), false)?;
    let client = RelayClient::new(Duration::from_secs(1))?;
    let image = jpeg("cover");
    let state = track_state("Title", Some(Artwork::Image(image.clone())));

    let outcome = client.publish_display(&relay_target(&stub.endpoint), &state)?;

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
                "artwork": { "sha256": image.sha256(), "mime": "image/jpeg" }
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

    client.publish_display(&target, &DisplayState::null())?;
    client.publish_display(
        &target,
        &track_state(
            "Url",
            Some(Artwork::Url("https://img.example/a.gif".to_owned())),
        ),
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
            client.publish_display(&relay_target(&stub.endpoint), &DisplayState::null())?;
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

// The stream-delay schedule.

const DELAY: Duration = Duration::from_secs(12);

fn kinds(items: &[ScheduledItem]) -> Vec<String> {
    items
        .iter()
        .map(|item| match item {
            ScheduledItem::Payload(payload) => format!("payload:{}", payload.title),
            ScheduledItem::Display(entry) => format!(
                "display:{}",
                entry
                    .state
                    .track
                    .as_ref()
                    .map_or("null", |track| track.title.as_str())
            ),
        })
        .collect()
}

fn delayed_schedule() -> PublishSchedule {
    PublishSchedule::new(HashMap::from([("event-guid".to_owned(), DELAY)]))
}

#[test]
fn a_display_entry_is_released_after_the_stream_delay_in_order_with_the_payloads() {
    let mut schedule = delayed_schedule();
    let start = Instant::now();

    schedule.schedule(payload("One"), start);
    schedule.schedule_display(entry(track_state("One", None)), start);
    let later = start + Duration::from_secs(3);
    schedule.schedule_display(entry(track_state("Two", None)), later);
    schedule.schedule(payload("Two"), later);

    assert!(
        schedule
            .take_due_items(start + DELAY - Duration::from_millis(1))
            .is_empty()
    );
    assert_eq!(
        kinds(&schedule.take_due_items(start + DELAY)),
        vec!["payload:One", "display:One"]
    );
    assert!(
        schedule
            .take_due_items(later + DELAY - Duration::from_millis(1))
            .is_empty()
    );
    assert_eq!(
        kinds(&schedule.take_due_items(later + DELAY)),
        vec!["display:Two", "payload:Two"]
    );
    assert!(schedule.is_empty());
}

#[test]
fn a_display_entry_sets_the_next_deadline() {
    let mut schedule = delayed_schedule();
    let now = Instant::now();

    schedule.schedule_display(entry(DisplayState::null()), now);

    assert_eq!(schedule.next_deadline(), Some(now + DELAY));
}

#[test]
fn a_display_entry_never_replaces_a_payload_of_the_same_target() {
    let mut schedule = delayed_schedule();
    let now = Instant::now();

    schedule.schedule(payload("One"), now);
    schedule.schedule_display(entry(DisplayState::null()), now);
    schedule.schedule(payload("One"), now);

    assert_eq!(
        kinds(&schedule.take_due_items(now + DELAY)),
        vec!["payload:One", "display:null"]
    );
}

#[test]
fn take_due_gives_only_the_payloads() {
    let mut schedule = delayed_schedule();
    let now = Instant::now();

    schedule.schedule(payload("One"), now);
    schedule.schedule_display(entry(DisplayState::null()), now);

    let due = schedule.take_due(now + DELAY);
    assert_eq!(due.len(), 1);
    assert!(schedule.is_empty());
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
        json!({ "schema": "musicindex.display/1", "track": track }).to_string(),
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
        "artwork": { "sha256": sha256, "mime": mime }
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
        json!({"artist": "A", "title": "T", "artwork": {"url": "https://img.example/a.gif"}}),
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
fn read_ignores_a_file_with_an_unknown_schema() -> Result<()> {
    let temp = TempDir::new()?;
    fs::write(
        temp.path().join(DISPLAY_FILE_NAME),
        json!({ "schema": "musicindex.display/2", "track": null }).to_string(),
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
