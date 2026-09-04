mod common;

use std::fs;
use std::io::{ErrorKind, Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use anyhow::{Result, anyhow};
use mixxx_now_playing::musicindex::{
    MusicIndexClient, PaymentRoute, ResolvedRouteResult, RouteRequest, RouteRequestStatus,
    RouteResolution, ValueRouteResolver, ValueRoutesSource, apply_resolution_to_tags,
    result_matches_current,
};
use mixxx_now_playing::render::{TrackDisplay, render_metadata_text_with_routes};
use mixxx_now_playing::sink::{OutputFile, Presence};
use mixxx_now_playing::tags::{TagText, TrackTags};
use tempfile::TempDir;

use common::SyntheticMixxxDb;

static NETWORK_TEST_LOCK: Mutex<()> = Mutex::new(());

fn network_test_lock() -> MutexGuard<'static, ()> {
    NETWORK_TEST_LOCK
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name)
}

fn binary() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_mixxx-now-playing"))
}

fn tagged_track() -> TrackTags {
    TrackTags {
        musicindex: vec![
            TagText::new("Feed Guid", "feed-guid-fixture"),
            TagText::new("Track Guid", "track-guid-fixture"),
            TagText::new(
                "Value Routes",
                r#"[{"recipient_name":"Embedded Alice","route_type":"node","split":90}]"#,
            ),
        ],
        tags: Vec::new(),
        duration: None,
    }
}

fn no_embedded_routes_track() -> TrackTags {
    TrackTags {
        musicindex: vec![
            TagText::new("Feed Guid", "feed-guid-fixture"),
            TagText::new("Track Guid", "track-guid-fixture"),
        ],
        tags: Vec::new(),
        duration: None,
    }
}

fn api_body(name: &str) -> String {
    serde_json::json!({
        "data": {
            "payment_routes": [
                {
                    "recipient_name": name,
                    "route_type": "node",
                    "split": 100.0,
                    "fee": false,
                    "address": "03abcdef",
                    "custom_key": "7629169",
                    "custom_value": "podcast-guid"
                }
            ]
        }
    })
    .to_string()
}

fn api_body_without_routes() -> String {
    serde_json::json!({ "data": {} }).to_string()
}

fn api_body_with_empty_routes() -> String {
    serde_json::json!({ "data": { "payment_routes": [] } }).to_string()
}

#[test]
fn musicindex_api_track_routes_are_authoritative() -> Result<()> {
    let _network = network_test_lock();
    let server = StubServer::new(vec![(
        "/v1/tracks/track-guid-fixture?include=payment_routes",
        StubResponse::ok(api_body("API Alice")),
    )])?;
    let client = MusicIndexClient::new(server.base_url(), Duration::from_secs(1))?;
    let tags = tagged_track();

    let resolution = client.resolve_value_routes(&RouteRequest::from_tags(&tags), false);

    assert_eq!(resolution.source, ValueRoutesSource::MusicIndexApi);
    let routes_json = resolution
        .routes_json
        .as_deref()
        .expect("API routes should be present");
    assert!(routes_json.contains("API Alice"));
    assert!(!routes_json.contains("Embedded Alice"));

    let updated = apply_resolution_to_tags(&tags, &resolution);
    let rendered = render_metadata_text_with_routes(
        TrackDisplay {
            artist: "Artist",
            title: "Title",
            tags: &updated,
        },
        resolution.source,
    );
    assert!(rendered.contains("Value Routes = musicindex-api\n"));
    assert!(rendered.contains("API Alice"));
    assert_eq!(
        server.requests(),
        vec!["/v1/tracks/track-guid-fixture?include=payment_routes"]
    );
    Ok(())
}

#[test]
fn musicindex_track_404_falls_back_to_feed_lookup() -> Result<()> {
    let _network = network_test_lock();
    let server = StubServer::new(vec![
        (
            "/v1/tracks/track-guid-fixture?include=payment_routes",
            StubResponse::not_found(),
        ),
        (
            "/v1/feeds/feed-guid-fixture?include=payment_routes",
            StubResponse::ok(api_body("Feed API Alice")),
        ),
    ])?;
    let client = MusicIndexClient::new(server.base_url(), Duration::from_secs(1))?;

    let resolution = client.resolve_value_routes(&RouteRequest::from_tags(&tagged_track()), false);

    assert_eq!(resolution.source, ValueRoutesSource::MusicIndexApi);
    assert!(
        resolution
            .routes_json
            .as_deref()
            .expect("feed routes should be present")
            .contains("Feed API Alice")
    );
    assert_eq!(
        server.requests(),
        vec![
            "/v1/tracks/track-guid-fixture?include=payment_routes",
            "/v1/feeds/feed-guid-fixture?include=payment_routes"
        ]
    );
    Ok(())
}

#[test]
fn musicindex_unreachable_api_falls_back_to_embedded_without_waiting_for_timeout() -> Result<()> {
    let _network = network_test_lock();
    let base_url = unused_local_base_url()?;
    let client = MusicIndexClient::new(base_url.clone(), Duration::from_secs(5))?;
    let request = RouteRequest::from_tags(&tagged_track());

    let started = Instant::now();
    let resolution = client.resolve_value_routes(&request, false);
    let elapsed = started.elapsed();

    assert_eq!(resolution.source, ValueRoutesSource::EmbeddedId3);
    assert!(
        resolution
            .routes_json
            .as_deref()
            .expect("embedded routes should be present")
            .contains("Embedded Alice")
    );
    assert!(
        elapsed < Duration::from_secs(1),
        "fallback took {elapsed:?} against {base_url}"
    );

    let mut resolver = ValueRouteResolver::new(true, base_url, Duration::from_secs(5), false)?;
    let started = Instant::now();
    let status = resolver.request(1, &tagged_track());

    assert_eq!(status, RouteRequestStatus::Spawned);
    assert!(started.elapsed() < Duration::from_millis(100));
    Ok(())
}

#[test]
fn musicindex_no_embedded_frame_and_no_api_routes_omits_value_routes_line() -> Result<()> {
    let _network = network_test_lock();
    let tags = no_embedded_routes_track();
    let server = StubServer::new(vec![(
        "/v1/tracks/track-guid-fixture?include=payment_routes",
        StubResponse::server_error(),
    )])?;
    let client = MusicIndexClient::new(server.base_url(), Duration::from_secs(1))?;

    let resolution = client.resolve_value_routes(&RouteRequest::from_tags(&tags), false);
    let updated = apply_resolution_to_tags(&tags, &resolution);
    let rendered = render_metadata_text_with_routes(
        TrackDisplay {
            artist: "Artist",
            title: "Title",
            tags: &updated,
        },
        resolution.source,
    );

    assert_eq!(resolution.source, ValueRoutesSource::EmbeddedId3);
    assert!(resolution.routes_json.is_none());
    assert!(!rendered.contains("Value Routes"));
    Ok(())
}

#[test]
fn musicindex_empty_or_missing_api_routes_are_not_rendered_as_authoritative() -> Result<()> {
    let _network = network_test_lock();
    for body in [api_body_without_routes(), api_body_with_empty_routes()] {
        let tags = no_embedded_routes_track();
        let server = StubServer::new(vec![(
            "/v1/tracks/track-guid-fixture?include=payment_routes",
            StubResponse::ok(body),
        )])?;
        let client = MusicIndexClient::new(server.base_url(), Duration::from_secs(1))?;

        let resolution = client.resolve_value_routes(&RouteRequest::from_tags(&tags), false);
        let updated = apply_resolution_to_tags(&tags, &resolution);
        let rendered = render_metadata_text_with_routes(
            TrackDisplay {
                artist: "Artist",
                title: "Title",
                tags: &updated,
            },
            resolution.source,
        );

        assert_eq!(resolution.source, ValueRoutesSource::EmbeddedId3);
        assert!(resolution.routes_json.is_none());
        assert!(!rendered.contains("Value Routes"));
    }
    Ok(())
}

#[test]
fn musicindex_once_mode_waits_for_api_routes_before_exit() -> Result<()> {
    let _network = network_test_lock();
    let server = StubServer::new(vec![(
        "/v1/tracks/track-guid-fixture?include=payment_routes",
        StubResponse::ok(api_body("Once API Alice")),
    )])?;
    let temp = TempDir::new()?;
    let v4v_root = temp.path().join("V4Vmusic");
    fs::create_dir_all(&v4v_root)?;
    let track = v4v_root.join("track.mp3");
    fs::copy(fixture("musicindex-tagged.mp3"), &track)?;

    let mut db = SyntheticMixxxDb::new()?;
    db.append_history_row_with_metadata(Some("Once Artist"), Some("Once Title"), &track)?;
    let txt_file = temp.path().join("now-playing.txt");
    let metadata_file = temp.path().join("metadata.txt");
    let xdg_config = temp.path().join("xdg-config");
    let v4vmm_config_dir = xdg_config.join("v4vmm");
    fs::create_dir_all(&v4vmm_config_dir)?;
    fs::write(
        v4vmm_config_dir.join("config.toml"),
        format!("musicindex_endpoint = {:?}\n", server.base_url()),
    )?;

    let output = Command::new(binary())
        .env("HOME", temp.path().join("home"))
        .env("XDG_CONFIG_HOME", &xdg_config)
        .arg("--once")
        .arg("--db-file")
        .arg(db.path())
        .arg("--txt-file")
        .arg(&txt_file)
        .arg("--id3-file")
        .arg(&metadata_file)
        .arg("--v4v-root")
        .arg(&v4v_root)
        .arg("--api-timeout")
        .arg("1")
        .output()?;

    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let rendered = fs::read_to_string(&metadata_file)?;
    assert!(rendered.contains("Value Routes = musicindex-api\n"));
    assert!(rendered.contains("Once API Alice"));
    Ok(())
}

#[test]
fn musicindex_repeat_track_uses_cache_without_second_api_request() -> Result<()> {
    let _network = network_test_lock();
    let server = StubServer::new(vec![(
        "/v1/tracks/track-guid-fixture?include=payment_routes",
        StubResponse::ok(api_body("Cached API Alice")),
    )])?;
    let mut resolver =
        ValueRouteResolver::new(true, server.base_url(), Duration::from_secs(1), false)?;
    let tags = tagged_track();

    assert_eq!(resolver.request(1, &tags), RouteRequestStatus::Spawned);
    let first = wait_for_result(&mut resolver, Duration::from_secs(2))?;
    assert_eq!(first.hist_id, 1);
    assert_eq!(first.resolution.source, ValueRoutesSource::MusicIndexApi);

    let RouteRequestStatus::Cached(second) = resolver.request(2, &tags) else {
        return Err(anyhow!("second play did not hit route cache"));
    };

    assert_eq!(second.hist_id, 2);
    assert_eq!(second.resolution.source, ValueRoutesSource::MusicIndexApi);
    assert_eq!(second.resolution.routes_json, first.resolution.routes_json);
    assert_eq!(
        server.requests(),
        vec!["/v1/tracks/track-guid-fixture?include=payment_routes"]
    );
    Ok(())
}

#[test]
fn musicindex_late_result_for_superseded_history_is_not_written() -> Result<()> {
    let temp = TempDir::new()?;
    let path = temp.path().join("metadata.txt");
    let mut output = OutputFile::new(&path);
    output.set(Presence::Present("current track".to_string()))?;
    let stale = ResolvedRouteResult {
        hist_id: 10,
        resolution: RouteResolution::api(api_body("Late API Alice")),
    };

    if result_matches_current(&stale, 11) {
        output.set(Presence::Present("late route".to_string()))?;
    }

    assert_eq!(fs::read_to_string(path)?, "current track");
    Ok(())
}

#[test]
fn musicindex_payment_route_deserializes_v4vmm_shape() -> Result<()> {
    let route = serde_json::from_str::<PaymentRoute>(
        r#"{
            "recipient_name": "Alice",
            "route_type": "node",
            "split": 90,
            "fee": false,
            "address": "03abcdef",
            "custom_key": "7629169",
            "custom_value": "podcast-guid"
        }"#,
    )?;

    assert_eq!(route.recipient_name.as_deref(), Some("Alice"));
    assert_eq!(route.route_type.as_deref(), Some("node"));
    assert_eq!(route.split, Some(90.0));
    assert_eq!(route.fee, Some(false));
    assert_eq!(route.address.as_deref(), Some("03abcdef"));
    assert_eq!(route.custom_key.as_deref(), Some("7629169"));
    assert_eq!(route.custom_value.as_deref(), Some("podcast-guid"));
    Ok(())
}

fn wait_for_result(
    resolver: &mut ValueRouteResolver,
    timeout: Duration,
) -> Result<ResolvedRouteResult> {
    let started = Instant::now();
    while started.elapsed() < timeout {
        if let Some(result) = resolver.drain().into_iter().next() {
            return Ok(result);
        }
        thread::sleep(Duration::from_millis(10));
    }
    Err(anyhow!("timed out waiting for route resolution"))
}

fn unused_local_base_url() -> Result<String> {
    let listener = TcpListener::bind("127.0.0.1:0")?;
    let addr = listener.local_addr()?;
    drop(listener);
    Ok(format!("http://{addr}"))
}

#[derive(Debug, Clone)]
struct StubResponse {
    status: u16,
    body: String,
}

impl StubResponse {
    fn ok(body: String) -> Self {
        Self { status: 200, body }
    }

    fn not_found() -> Self {
        Self {
            status: 404,
            body: "{}".to_string(),
        }
    }

    fn server_error() -> Self {
        Self {
            status: 500,
            body: "server error".to_string(),
        }
    }
}

#[derive(Debug)]
struct StubServer {
    addr: SocketAddr,
    requests: Arc<Mutex<Vec<String>>>,
    shutdown: Arc<AtomicBool>,
    handle: Option<JoinHandle<()>>,
}

impl StubServer {
    fn new(routes: Vec<(&str, StubResponse)>) -> Result<Self> {
        let listener = TcpListener::bind("127.0.0.1:0")?;
        listener.set_nonblocking(true)?;
        let addr = listener.local_addr()?;
        let routes = Arc::new(
            routes
                .into_iter()
                .map(|(path, response)| (path.to_string(), response))
                .collect::<Vec<_>>(),
        );
        let requests = Arc::new(Mutex::new(Vec::new()));
        let shutdown = Arc::new(AtomicBool::new(false));

        let thread_routes = Arc::clone(&routes);
        let thread_requests = Arc::clone(&requests);
        let thread_shutdown = Arc::clone(&shutdown);
        let handle = thread::spawn(move || {
            while !thread_shutdown.load(Ordering::Relaxed) {
                match listener.accept() {
                    Ok((mut stream, _peer)) => {
                        let target = read_request_target(&mut stream)
                            .unwrap_or_else(|_| "/__invalid__".to_string());
                        if let Ok(mut requests) = thread_requests.lock() {
                            requests.push(target.clone());
                        }
                        let response = thread_routes
                            .iter()
                            .find(|(path, _response)| path == &target)
                            .map(|(_path, response)| response.clone())
                            .unwrap_or_else(StubResponse::not_found);
                        let _ = write_response(&mut stream, &response);
                    }
                    Err(error) if error.kind() == ErrorKind::WouldBlock => {
                        thread::sleep(Duration::from_millis(5));
                    }
                    Err(_) => break,
                }
            }
        });

        Ok(Self {
            addr,
            requests,
            shutdown,
            handle: Some(handle),
        })
    }

    fn base_url(&self) -> String {
        format!("http://{}", self.addr)
    }

    fn requests(&self) -> Vec<String> {
        self.requests
            .lock()
            .map(|requests| requests.clone())
            .unwrap_or_default()
    }
}

impl Drop for StubServer {
    fn drop(&mut self) {
        self.shutdown.store(true, Ordering::Relaxed);
        let _ = TcpStream::connect(self.addr);
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
    }
}

fn read_request_target(stream: &mut TcpStream) -> Result<String> {
    stream.set_read_timeout(Some(Duration::from_millis(100)))?;
    let mut bytes = Vec::new();
    let mut buffer = [0_u8; 1024];
    loop {
        match stream.read(&mut buffer) {
            Ok(0) => break,
            Ok(count) => {
                bytes.extend_from_slice(&buffer[..count]);
                if bytes.windows(4).any(|window| window == b"\r\n\r\n") {
                    break;
                }
            }
            Err(error)
                if error.kind() == ErrorKind::WouldBlock || error.kind() == ErrorKind::TimedOut =>
            {
                break;
            }
            Err(error) => return Err(error.into()),
        }
    }

    let request = String::from_utf8_lossy(&bytes);
    let first_line = request
        .lines()
        .next()
        .ok_or_else(|| anyhow!("empty HTTP request"))?;
    let mut parts = first_line.split_whitespace();
    let _method = parts.next();
    parts
        .next()
        .map(str::to_string)
        .ok_or_else(|| anyhow!("HTTP request has no target"))
}

fn write_response(stream: &mut TcpStream, response: &StubResponse) -> Result<()> {
    let reason = match response.status {
        200 => "OK",
        404 => "Not Found",
        500 => "Internal Server Error",
        _ => "OK",
    };
    write!(
        stream,
        "HTTP/1.1 {} {}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
        response.status,
        reason,
        response.body.len(),
        response.body
    )?;
    stream.flush()?;
    Ok(())
}
