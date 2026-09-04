use std::collections::VecDeque;
use std::fs;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::Path;
use std::sync::{Arc, Mutex, mpsc};
use std::thread;
use std::time::{Duration, Instant};

use anyhow::{Result, anyhow};
use musicindex_live_publisher::{
    FallbackConfig, LiveValue, LiveValueDestination, LiveValueModel, LiveValuePayload,
    PublishOutcome, PublisherConfig, PublisherTarget, RelayClient, RelayPublisher, RelayTarget,
    write_token_file,
};
use serde_json::{Value, json};
use tempfile::TempDir;

#[derive(Debug, Clone)]
struct StubResponse {
    status: u16,
    body: String,
}

#[derive(Debug, Clone)]
struct StubRequest {
    path: String,
    authorization: Option<String>,
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
            let responses = responses.clone();
            let requests = requests.clone();
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
    let path = first_line
        .split_whitespace()
        .nth(1)
        .ok_or_else(|| anyhow!("request line missing path"))?
        .to_owned();

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
    let body = if body.is_empty() {
        Value::Null
    } else {
        serde_json::from_slice(&body)?
    };
    requests
        .lock()
        .map_err(|_| anyhow!("request mutex poisoned"))?
        .push(StubRequest {
            path,
            authorization,
            body,
        });

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

fn response(status: u16) -> StubResponse {
    StubResponse {
        status,
        body: json!({"error": status}).to_string(),
    }
}

fn accepted(seq: u64) -> StubResponse {
    StubResponse {
        status: 200,
        body: json!({"event_id": "event-guid", "accepted": true, "seq": seq}).to_string(),
    }
}

fn target(endpoint: &str) -> RelayTarget {
    RelayTarget {
        name: "default".to_owned(),
        endpoint: endpoint.to_owned(),
        event_id: "event-guid".to_owned(),
        token: "secret-token".to_owned(),
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
            destinations: vec![LiveValueDestination {
                kind: Some("node".to_owned()),
                name: Some("Alice".to_owned()),
                address: Some("03alice".to_owned()),
                split: Some("100".to_owned()),
                custom_key: None,
                custom_value: None,
                fee: None,
            }],
        },
    }
}

fn config(endpoint: &str) -> PublisherConfig {
    PublisherConfig {
        watch_dir: Path::new("/tmp").to_path_buf(),
        endpoint: endpoint.to_owned(),
        targets: vec![PublisherTarget {
            name: "default".to_owned(),
            event_id: "event-guid".to_owned(),
            token_file: Path::new("/tmp/default.token").to_path_buf(),
            token: "secret-token".to_owned(),
            stream_delay: Duration::ZERO,
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
        }],
    }
}

#[test]
fn relay_publish_success_posts_direct_payload_and_returns_seq() -> Result<()> {
    let server = StubServer::start(vec![accepted(7)])?;
    let client = RelayClient::new(Duration::from_secs(1))?;

    let outcome = client.publish(&target(&server.endpoint), &payload("First"))?;

    assert_eq!(outcome, PublishOutcome::Accepted { seq: 7 });
    server.wait_for_requests(1)?;
    let requests = server.requests()?;
    assert_eq!(requests[0].path, "/v1/liveitems/event-guid/metadata");
    assert_eq!(
        requests[0].authorization.as_deref(),
        Some("Bearer secret-token")
    );
    assert_eq!(requests[0].body["title"], "First");
    assert!(requests[0].body.get("metadata").is_none());
    Ok(())
}

#[test]
fn relay_target_debug_redacts_token() {
    let rendered = format!("{:?}", target("https://relay.example.test"));

    assert!(rendered.contains("<redacted>"));
    assert!(!rendered.contains("secret-token"));
}

#[test]
fn relay_status_codes_map_to_documented_outcomes() -> Result<()> {
    for status in [401_u16, 403, 404] {
        let server = StubServer::start(vec![response(status)])?;
        let client = RelayClient::new(Duration::from_secs(1))?;
        let outcome = client.publish(&target(&server.endpoint), &payload("Fatal"))?;
        assert!(matches!(outcome, PublishOutcome::Fatal { .. }));
    }

    let server = StubServer::start(vec![response(413)])?;
    let client = RelayClient::new(Duration::from_secs(1))?;
    assert!(matches!(
        client.publish(&target(&server.endpoint), &payload("Large"))?,
        PublishOutcome::Dropped { .. }
    ));

    for status in [429_u16, 500] {
        let server = StubServer::start(vec![response(status)])?;
        let client = RelayClient::new(Duration::from_secs(1))?;
        let outcome = client.publish(&target(&server.endpoint), &payload("Retry"))?;
        assert!(matches!(outcome, PublishOutcome::Retryable { .. }));
    }
    Ok(())
}

#[test]
fn relay_network_error_is_retryable() -> Result<()> {
    let listener = TcpListener::bind("127.0.0.1:0")?;
    let endpoint = format!("http://{}", listener.local_addr()?);
    drop(listener);
    let client = RelayClient::new(Duration::from_millis(200))?;

    let outcome = client.publish(&target(&endpoint), &payload("Network"))?;

    assert!(matches!(outcome, PublishOutcome::Retryable { .. }));
    Ok(())
}

#[test]
fn relay_rejects_exact_wrapped_payload_shape_before_sending() -> Result<()> {
    let server = StubServer::start(vec![accepted(1)])?;
    let client = RelayClient::new(Duration::from_secs(1))?;
    let wrapped = json!({"event_id": "event-guid", "metadata": {}});

    let error = client.publish_value(&target(&server.endpoint), &wrapped);

    assert!(error.is_err());
    assert!(server.requests()?.is_empty());
    Ok(())
}

#[test]
fn relay_worker_retries_429_and_new_payload_supersedes_pending_retry() -> Result<()> {
    let server = StubServer::start(vec![response(429), accepted(2)])?;
    let publisher = RelayPublisher::start_with_backoff(
        &config(&server.endpoint),
        Duration::from_millis(200),
        Duration::from_secs(1),
    )?;

    publisher.publish(payload("Old"))?;
    server.wait_for_requests(1)?;
    publisher.publish(payload("New"))?;
    server.wait_for_requests(1)?;

    let requests = server.requests()?;
    assert_eq!(requests.len(), 2);
    assert_eq!(requests[0].body["title"], "Old");
    assert_eq!(requests[1].body["title"], "New");
    Ok(())
}

#[test]
fn relay_worker_retries_429_until_payload_is_accepted() -> Result<()> {
    let server = StubServer::start(vec![response(429), accepted(2)])?;
    let publisher = RelayPublisher::start_with_backoff(
        &config(&server.endpoint),
        Duration::from_millis(5),
        Duration::from_millis(20),
    )?;

    publisher.publish(payload("Retry Same"))?;
    server.wait_for_requests(2)?;

    let requests = server.requests()?;
    assert_eq!(requests.len(), 2);
    assert_eq!(requests[0].body["title"], "Retry Same");
    assert_eq!(requests[1].body["title"], "Retry Same");
    Ok(())
}

#[test]
fn provision_writes_token_file_with_private_permissions() -> Result<()> {
    let body = json!({
        "event_id": "created-event",
        "broadcaster_token": "created-secret",
        "metadata_url": "/v1/liveitems/created-event/metadata",
        "remote_value_url": "/v1/liveitems/created-event/remoteValue",
        "events_url": "/v1/liveitems/created-event/events",
        "socket_io_url": "/event?event_id=created-event"
    });
    let server = StubServer::start(vec![StubResponse {
        status: 200,
        body: body.to_string(),
    }])?;
    let temp = TempDir::new()?;
    let token_file = temp.path().join("default.token");
    let client = RelayClient::new(Duration::from_secs(1))?;

    let item = client.provision(&server.endpoint)?;
    write_token_file(&token_file, &item.broadcaster_token)?;

    assert_eq!(item.event_id, "created-event");
    assert_eq!(fs::read_to_string(&token_file)?, "created-secret\n");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = fs::metadata(&token_file)?.permissions().mode() & 0o777;
        assert_eq!(mode, 0o600);
    }
    Ok(())
}

#[test]
#[ignore = "requires a running local musicindex-live-relay; set LOCAL_RELAY_ENDPOINT"]
fn relay_integration_round_trips_remote_value_against_local_relay() -> Result<()> {
    let endpoint = std::env::var("LOCAL_RELAY_ENDPOINT")
        .map_err(|_| anyhow!("LOCAL_RELAY_ENDPOINT must point at a local relay"))?;
    if endpoint.contains("api.musicindex.org") {
        return Err(anyhow!(
            "refusing to run integration test against api.musicindex.org"
        ));
    }

    let client = RelayClient::new(Duration::from_secs(2))?;
    let item = client.provision(&endpoint)?;
    let relay_target = RelayTarget {
        name: "default".to_owned(),
        endpoint: endpoint.clone(),
        event_id: item.event_id,
        token: item.broadcaster_token,
    };
    let sent = payload("Integration");
    let sent_value = serde_json::to_value(&sent)?;

    assert!(matches!(
        client.publish(&relay_target, &sent)?,
        PublishOutcome::Accepted { .. }
    ));

    let remote_value: Value = reqwest::blocking::get(format!(
        "{}/v1/liveitems/{}/remoteValue",
        endpoint.trim_end_matches('/'),
        relay_target.event_id
    ))?
    .json()?;
    assert_eq!(remote_value, sent_value);
    Ok(())
}

#[test]
fn relay_worker_stops_after_fatal_so_the_process_cannot_publish_silently() -> Result<()> {
    // 401 is fatal: the token is bad and no amount of retrying fixes it.
    let server = StubServer::start(vec![response(401)])?;
    let publisher = RelayPublisher::start_with_backoff(
        &config(&server.endpoint),
        Duration::from_millis(5),
        Duration::from_millis(20),
    )?;

    publisher.publish(payload("Fatal"))?;

    // The worker must stop, which closes its channel. The next publish then
    // fails instead of being accepted into a queue nobody drains.
    let deadline = Instant::now() + Duration::from_secs(5);
    let mut last = Ok(());
    while Instant::now() < deadline {
        last = publisher.publish(payload("After Fatal"));
        if last.is_err() {
            break;
        }
        thread::sleep(Duration::from_millis(20));
    }

    let error = last.expect_err("publishing after a fatal outcome must fail");
    assert!(
        error.to_string().contains("stopped"),
        "unexpected error: {error}"
    );
    Ok(())
}

#[test]
fn relay_worker_survives_a_dropped_payload() -> Result<()> {
    // 413 drops one oversized payload but the target is still healthy.
    let server = StubServer::start(vec![response(413), accepted(1)])?;
    let publisher = RelayPublisher::start_with_backoff(
        &config(&server.endpoint),
        Duration::from_millis(5),
        Duration::from_millis(20),
    )?;

    publisher.publish(payload("Too Large"))?;
    server.wait_for_requests(1)?;
    publisher.publish(payload("Next Track"))?;
    server.wait_for_requests(1)?;

    let requests = server.requests()?;
    assert_eq!(
        requests.len(),
        2,
        "worker should still accept work after 413"
    );
    assert_eq!(requests[1].body["title"], "Next Track");
    Ok(())
}
