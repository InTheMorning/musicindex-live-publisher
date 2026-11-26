mod common;

use std::fs;
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Mutex, MutexGuard};
use std::thread;
use std::time::{Duration, Instant};

use anyhow::{Result, anyhow};
use display_keys::{FundingLink, file_funding};
use lofty::config::WriteOptions;
use lofty::prelude::*;
use lofty::tag::{ItemKey, ItemValue, TagItem};
use mixxx_now_playing::display::{ShownTrack, funding_for_track, render_display_json};
use mixxx_now_playing::musicindex::{
    ResolvedRouteResult, RouteRequest, RouteRequestStatus, RouteResolution, ValueRouteResolver,
    ValueRoutesSource, apply_resolution_to_tags, resolve_value_routes, result_matches_current,
};
use mixxx_now_playing::render::{TrackDisplay, render_metadata_text_with_routes};
use mixxx_now_playing::tags::{TagText, TrackTags};
use now_playing_drop::PaymentRoute;
use now_playing_drop::route_service::RouteServiceClient;
use now_playing_drop::sink::{OutputFile, Presence};
use now_playing_drop::test_server::{Reply, TestServer};
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

fn mp3_with_wpay(temp: &TempDir) -> Result<PathBuf> {
    mp3_with_wpay_at(&temp.path().join("non-payable-with-wpay.mp3"))
}

/// Copies the tagged fixture to `path` and adds a file frame `WPAY`.
fn mp3_with_wpay_at(path: &Path) -> Result<PathBuf> {
    let path = path.to_path_buf();
    fs::copy(fixture("musicindex-tagged.mp3"), &path)?;
    let mut tagged = lofty::read_from_path(&path)?;
    let tag = tagged
        .primary_tag_mut()
        .ok_or_else(|| anyhow!("MP3 has no tag"))?;
    tag.insert_unchecked(TagItem::new(
        ItemKey::Unknown("WPAY".to_owned()),
        ItemValue::Locator("https://file.example/pay".to_owned()),
    ));
    tagged.save_to_path(&path, WriteOptions::default())?;
    Ok(path)
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

fn funding_body(routes: bool, links: serde_json::Value) -> String {
    let mut data = serde_json::json!({ "source_links": links });
    if routes {
        data["payment_routes"] = serde_json::json!([{
            "recipient_name": "API Alice",
            "route_type": "node",
            "split": 100.0,
            "fee": false,
            "address": "03abcdef",
            "custom_key": "7629169",
            "custom_value": "podcast-guid"
        }]);
    }
    serde_json::json!({ "data": data }).to_string()
}

fn render_funding(
    play_id: &str,
    payable: bool,
    fresh: &[FundingLink],
    file: &[FundingLink],
) -> Result<serde_json::Value> {
    let funding = funding_for_track(payable, fresh, file);
    let json = render_display_json(Some(ShownTrack {
        artist: "Artist",
        title: "Title",
        artwork: None,
        song_line: "Artist - Title",
        play_id: Some(play_id),
        album: None,
        pub_date: None,
        funding,
    }))?;
    Ok(serde_json::from_str(&json)?)
}

fn test_server(responses: Vec<(&str, Reply)>) -> Result<TestServer> {
    let responses: Vec<_> = responses
        .into_iter()
        .map(|(path, reply)| (path.to_owned(), reply))
        .collect();
    TestServer::start(move |target| {
        responses
            .iter()
            .find(|(path, _)| path == target)
            .map(|(_, reply)| reply.clone())
            .unwrap_or_else(|| Reply::Answer {
                status: 404,
                body: "{}".to_owned(),
            })
    })
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
    let server = test_server(vec![(
        "/v1/feeds/feed-guid-fixture/tracks/track-guid-fixture?include=payment_routes,source_links",
        Reply::Answer {
            status: 200,
            body: api_body("API Alice"),
        },
    )])?;
    let client = RouteServiceClient::new(server.base_url(), Duration::from_secs(1))?;
    let tags = tagged_track();

    let resolution = resolve_value_routes(&client, &RouteRequest::from_tags(&tags), false);

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
            album: None,
            play_id: None,
            tags: &updated,
        },
        resolution.source,
    );
    assert!(rendered.contains("Value Routes = musicindex-api\n"));
    assert!(rendered.contains("API Alice"));
    assert_eq!(
        server.requests(),
        vec![
            "/v1/feeds/feed-guid-fixture/tracks/track-guid-fixture?include=payment_routes,source_links",
            "/v1/feeds/feed-guid-fixture?include=source_links"
        ]
    );
    Ok(())
}

#[test]
fn musicindex_track_404_falls_back_to_embedded_routes() -> Result<()> {
    let _network = network_test_lock();
    let server = test_server(vec![(
        "/v1/feeds/feed-guid-fixture/tracks/track-guid-fixture?include=payment_routes,source_links",
        Reply::Answer {
            status: 404,
            body: "{}".to_owned(),
        },
    )])?;
    let client = RouteServiceClient::new(server.base_url(), Duration::from_secs(1))?;

    let resolution =
        resolve_value_routes(&client, &RouteRequest::from_tags(&tagged_track()), false);

    assert_eq!(resolution.source, ValueRoutesSource::EmbeddedId3);
    assert_eq!(
        server.requests(),
        vec![
            "/v1/feeds/feed-guid-fixture/tracks/track-guid-fixture?include=payment_routes,source_links",
            "/v1/feeds/feed-guid-fixture?include=source_links"
        ]
    );
    Ok(())
}

#[test]
fn musicindex_unreachable_api_falls_back_to_embedded_without_waiting_for_timeout() -> Result<()> {
    let _network = network_test_lock();
    let base_url = unused_local_base_url()?;
    let client = RouteServiceClient::new(base_url.clone(), Duration::from_secs(5))?;
    let request = RouteRequest::from_tags(&tagged_track());

    let started = Instant::now();
    let resolution = resolve_value_routes(&client, &request, false);
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
    let server = test_server(vec![(
        "/v1/feeds/feed-guid-fixture/tracks/track-guid-fixture?include=payment_routes,source_links",
        Reply::Answer {
            status: 500,
            body: "server error".to_owned(),
        },
    )])?;
    let client = RouteServiceClient::new(server.base_url(), Duration::from_secs(1))?;

    let resolution = resolve_value_routes(&client, &RouteRequest::from_tags(&tags), false);
    let updated = apply_resolution_to_tags(&tags, &resolution);
    let rendered = render_metadata_text_with_routes(
        TrackDisplay {
            artist: "Artist",
            title: "Title",
            album: None,
            play_id: None,
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
        let server = test_server(vec![(
            "/v1/feeds/feed-guid-fixture/tracks/track-guid-fixture?include=payment_routes,source_links",
            Reply::Answer { status: 200, body },
        )])?;
        let client = RouteServiceClient::new(server.base_url(), Duration::from_secs(1))?;

        let resolution = resolve_value_routes(&client, &RouteRequest::from_tags(&tags), false);
        let updated = apply_resolution_to_tags(&tags, &resolution);
        let rendered = render_metadata_text_with_routes(
            TrackDisplay {
                artist: "Artist",
                title: "Title",
                album: None,
                play_id: None,
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
    let server = test_server(vec![(
        "/v1/feeds/feed-guid-fixture/tracks/track-guid-fixture?include=payment_routes,source_links",
        Reply::Answer {
            status: 200,
            body: api_body("Once API Alice"),
        },
    )])?;
    let temp = TempDir::new()?;
    let v4v_root = temp.path().join("V4Vmusic");
    fs::create_dir_all(&v4v_root)?;
    let track = v4v_root.join("track.mp3");
    fs::copy(fixture("musicindex-tagged.mp3"), &track)?;

    let mut db = SyntheticMixxxDb::new()?;
    db.append_history_row_with_metadata(Some("Once Artist"), Some("Once Title"), None, &track)?;
    let txt_file = temp.path().join("now-playing.txt");
    let metadata_file = temp.path().join("metadata.txt");
    let xdg_config = temp.path().join("xdg-config");
    let hitit_config_dir = xdg_config.join("hitit");
    fs::create_dir_all(&hitit_config_dir)?;
    fs::write(
        hitit_config_dir.join("config.toml"),
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
    let server = test_server(vec![(
        "/v1/feeds/feed-guid-fixture/tracks/track-guid-fixture?include=payment_routes,source_links",
        Reply::Answer {
            status: 200,
            body: api_body("Cached API Alice"),
        },
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
        vec![
            "/v1/feeds/feed-guid-fixture/tracks/track-guid-fixture?include=payment_routes,source_links",
            "/v1/feeds/feed-guid-fixture?include=source_links"
        ]
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
fn mixxx_fresh_item_links_take_priority_over_wpay() -> Result<()> {
    let _network = network_test_lock();
    let path =
        "/v1/feeds/feed-guid-fixture/tracks/track-guid-fixture?include=payment_routes,source_links";
    let server = test_server(vec![(
        path,
        Reply::Answer {
            status: 200,
            body: funding_body(
                true,
                serde_json::json!([{
                    "link_type": "funding",
                    "url": "https://item.example/support",
                    "label": "Item support",
                    "position": 1
                }]),
            ),
        },
    )])?;
    let mut resolver =
        ValueRouteResolver::new(true, server.base_url(), Duration::from_secs(1), false)?;
    let tags = tagged_track();
    assert_eq!(resolver.request(1, &tags), RouteRequestStatus::Spawned);
    let result = wait_for_result(&mut resolver, Duration::from_secs(2))?;
    let file = file_funding("https://file.example/pay");

    let display = render_funding("1", true, &result.resolution.funding, &file)?;

    assert_eq!(
        display["track"]["funding"][0]["url"],
        "https://item.example/support"
    );
    assert_eq!(display["track"]["funding"][0]["label"], "Item support");
    assert_eq!(server.requests(), vec![path]);
    Ok(())
}

#[test]
fn mixxx_uses_feed_links_when_track_links_are_empty() -> Result<()> {
    let _network = network_test_lock();
    let track_path =
        "/v1/feeds/feed-guid-fixture/tracks/track-guid-fixture?include=payment_routes,source_links";
    let feed_path = "/v1/feeds/feed-guid-fixture?include=source_links";
    let server = test_server(vec![
        (
            track_path,
            Reply::Answer {
                status: 200,
                body: funding_body(false, serde_json::json!([])),
            },
        ),
        (
            feed_path,
            Reply::Answer {
                status: 200,
                body: funding_body(
                    false,
                    serde_json::json!([{
                        "link_type": "funding",
                        "url": "https://feed.example/support",
                        "label": "Feed support",
                        "position": 1
                    }]),
                ),
            },
        ),
    ])?;
    let mut resolver =
        ValueRouteResolver::new(true, server.base_url(), Duration::from_secs(1), false)?;
    let tags = tagged_track();
    assert_eq!(resolver.request(1, &tags), RouteRequestStatus::Spawned);
    let result = wait_for_result(&mut resolver, Duration::from_secs(2))?;
    let file = file_funding("https://file.example/pay");

    let display = render_funding("1", true, &result.resolution.funding, &file)?;

    assert_eq!(
        display["track"]["funding"][0]["url"],
        "https://feed.example/support"
    );
    assert_eq!(display["track"]["funding"][0]["label"], "Feed support");
    assert_eq!(server.requests(), vec![track_path, feed_path]);
    Ok(())
}

#[test]
fn mixxx_wpay_fallback_covers_missing_and_failed_lookups() -> Result<()> {
    let _network = network_test_lock();
    let track_path =
        "/v1/feeds/feed-guid-fixture/tracks/track-guid-fixture?include=payment_routes,source_links";
    let feed_path = "/v1/feeds/feed-guid-fixture?include=source_links";
    let server = test_server(vec![
        (
            track_path,
            Reply::Answer {
                status: 404,
                body: "{}".into(),
            },
        ),
        (
            feed_path,
            Reply::Answer {
                status: 404,
                body: "{}".into(),
            },
        ),
    ])?;
    let mut resolver =
        ValueRouteResolver::new(true, server.base_url(), Duration::from_secs(1), false)?;
    let file = file_funding("https://file.example/pay");
    assert_eq!(
        resolver.request(1, &tagged_track()),
        RouteRequestStatus::Spawned
    );
    let missing = wait_for_result(&mut resolver, Duration::from_secs(2))?;
    let missing_display = render_funding("1", true, &missing.resolution.funding, &file)?;
    assert_eq!(
        missing_display["track"]["funding"][0]["url"],
        "https://file.example/pay"
    );
    assert!(
        missing_display["track"]["funding"][0]
            .get("label")
            .is_none()
    );

    let failed_base = unused_local_base_url()?;
    let mut failed = ValueRouteResolver::new(true, failed_base, Duration::from_secs(1), false)?;
    assert_eq!(
        failed.request(2, &tagged_track()),
        RouteRequestStatus::Spawned
    );
    let failed_result = wait_for_result(&mut failed, Duration::from_secs(2))?;
    let failed_display = render_funding("2", true, &failed_result.resolution.funding, &file)?;
    assert_eq!(
        failed_display["track"]["funding"][0]["url"],
        "https://file.example/pay"
    );
    assert!(failed_display["track"]["funding"][0].get("label").is_none());

    let mut disabled =
        ValueRouteResolver::new(false, server.base_url(), Duration::from_secs(1), false)?;
    assert_eq!(
        disabled.request(3, &tagged_track()),
        RouteRequestStatus::Disabled
    );
    let disabled_display = render_funding("3", true, &[], &file)?;
    assert_eq!(
        disabled_display["track"]["funding"][0]["url"],
        "https://file.example/pay"
    );
    assert!(
        disabled_display["track"]["funding"][0]
            .get("label")
            .is_none()
    );
    let one_guid = TrackTags {
        musicindex: vec![TagText::new("Track Guid", "track-guid")],
        tags: Vec::new(),
        duration: None,
    };
    let mut no_key =
        ValueRouteResolver::new(true, server.base_url(), Duration::from_secs(1), false)?;
    assert_eq!(
        no_key.request(4, &one_guid),
        RouteRequestStatus::NoLookupKey
    );
    let no_key_display = render_funding("4", true, &[], &file)?;
    assert_eq!(
        no_key_display["track"]["funding"][0]["url"],
        "https://file.example/pay"
    );
    assert!(no_key_display["track"]["funding"][0].get("label").is_none());
    assert_eq!(server.requests(), vec![track_path, feed_path]);
    Ok(())
}

#[test]
fn mixxx_pending_and_late_funding_keep_the_same_play_id_and_routes() -> Result<()> {
    let _network = network_test_lock();
    let path =
        "/v1/feeds/feed-guid-fixture/tracks/track-guid-fixture?include=payment_routes,source_links";
    let server = test_server(vec![(
        path,
        Reply::Answer {
            status: 200,
            body: funding_body(
                false,
                serde_json::json!([{
                    "link_type": "funding",
                    "url": "https://late.example/support",
                    "label": "Late support",
                    "position": 1
                }]),
            ),
        },
    )])?;
    let mut resolver =
        ValueRouteResolver::new(true, server.base_url(), Duration::from_secs(1), false)?;
    let tags = tagged_track();
    let original_routes = tags.musicindex_value("Value Routes").map(str::to_owned);
    let file = file_funding("https://file.example/pay");
    let first = render_funding("55", true, &[], &file)?;
    assert_eq!(
        first["track"]["funding"][0]["url"],
        "https://file.example/pay"
    );

    assert_eq!(resolver.request(55, &tags), RouteRequestStatus::Spawned);
    let result = wait_for_result(&mut resolver, Duration::from_secs(2))?;
    assert_eq!(result.resolution.source, ValueRoutesSource::EmbeddedId3);
    let late = render_funding("55", true, &result.resolution.funding, &file)?;
    assert_eq!(late["track"]["play_id"], first["track"]["play_id"]);
    assert_eq!(
        late["track"]["funding"][0]["url"],
        "https://late.example/support"
    );
    let unchanged_drop_tags = if result.resolution.source == ValueRoutesSource::MusicIndexApi {
        apply_resolution_to_tags(&tags, &result.resolution)
    } else {
        tags.clone()
    };
    assert_eq!(
        unchanged_drop_tags
            .musicindex_value("Value Routes")
            .map(str::to_owned),
        original_routes
    );
    Ok(())
}

#[test]
fn mixxx_wpay_trims_a_trailing_nul() -> Result<()> {
    let file = file_funding("https://file.example/pay\0");
    let display = render_funding("1", true, &[], &file)?;
    assert_eq!(
        display["track"]["funding"][0]["url"],
        "https://file.example/pay"
    );
    assert!(display["track"]["funding"][0].get("label").is_none());
    Ok(())
}

#[test]
fn mixxx_writes_null_funding_without_links_or_for_non_payable_tracks() -> Result<()> {
    let file = file_funding("https://file.example/pay");
    let no_links = render_funding("1", true, &[], &[])?;
    let non_payable = render_funding("2", false, &[], &file)?;
    // A missing key also reads as null, so the test asks for the key.
    assert_eq!(
        no_links["track"].get("funding"),
        Some(&serde_json::Value::Null)
    );
    assert_eq!(
        non_payable["track"].get("funding"),
        Some(&serde_json::Value::Null)
    );
    Ok(())
}

#[test]
fn mixxx_non_payable_track_with_wpay_has_null_and_makes_no_request() -> Result<()> {
    let server = TestServer::start(|_| Reply::Answer {
        status: 500,
        body: "unexpected request".to_owned(),
    })?;
    let temp = TempDir::new()?;
    let track = mp3_with_wpay(&temp)?;
    assert!(
        mixxx_now_playing::tags::read_tags(&track)?
            .tags
            .iter()
            .any(|tag| tag.key == "WPAY")
    );
    let mut db = SyntheticMixxxDb::new()?;
    db.append_history_row_with_metadata(Some("Guest Artist"), Some("Guest Title"), None, &track)?;
    let v4v_root = temp.path().join("V4Vmusic");
    fs::create_dir_all(&v4v_root)?;
    let display_dir = temp.path().join("display");
    let txt_file = temp.path().join("now-playing.txt");
    let metadata_file = temp.path().join("metadata.txt");
    let xdg_config = temp.path().join("xdg-config");
    let hitit_config_dir = xdg_config.join("hitit");
    fs::create_dir_all(&hitit_config_dir)?;
    fs::write(
        hitit_config_dir.join("config.toml"),
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
        .arg("--display-dir")
        .arg(&display_dir)
        .output()?;
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let display: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(display_dir.join("display.json"))?)?;
    assert_eq!(
        display["track"].get("funding"),
        Some(&serde_json::Value::Null)
    );
    assert!(server.requests().is_empty());
    Ok(())
}

#[test]
fn mixxx_caches_feed_funding_across_tracks_of_one_feed() -> Result<()> {
    let _network = network_test_lock();
    let server = TestServer::start(|target| {
        if target.starts_with("/v1/feeds/feed-guid-fixture/tracks/") {
            Reply::Answer {
                status: 200,
                body: funding_body(false, serde_json::json!([])),
            }
        } else {
            Reply::Answer {
                status: 200,
                body: funding_body(
                    false,
                    serde_json::json!([{
                        "link_type": "funding",
                        "url": "https://feed.example/support",
                        "position": 1
                    }]),
                ),
            }
        }
    })?;
    let mut resolver =
        ValueRouteResolver::new(true, server.base_url(), Duration::from_secs(1), false)?;
    assert_eq!(
        resolver.request(1, &tagged_track()),
        RouteRequestStatus::Spawned
    );
    let mut second_track = no_embedded_routes_track();
    second_track.set_musicindex_value("Track Guid", "second-track-guid");
    assert_eq!(
        resolver.request(2, &second_track),
        RouteRequestStatus::Spawned
    );
    let results = wait_for_results(&mut resolver, 2, Duration::from_secs(2))?;
    let first = &results[0];
    let second = &results[1];
    assert_eq!(first.resolution.funding, second.resolution.funding);
    assert_eq!(
        server
            .requests()
            .iter()
            .filter(|path| path.contains("?include=source_links"))
            .count(),
        1
    );
    Ok(())
}

#[test]
fn mixxx_funding_display_has_no_pub_date() -> Result<()> {
    let file = file_funding("https://file.example/pay");
    let display = render_funding("1", true, &[], &file)?;
    assert!(display["track"].get("pub_date").is_none());
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

fn wait_for_results(
    resolver: &mut ValueRouteResolver,
    count: usize,
    timeout: Duration,
) -> Result<Vec<ResolvedRouteResult>> {
    let started = Instant::now();
    let mut results = Vec::new();
    while started.elapsed() < timeout {
        results.extend(resolver.drain());
        if results.len() >= count {
            return Ok(results);
        }
        thread::sleep(Duration::from_millis(10));
    }
    Err(anyhow!("timed out waiting for {count} route resolutions"))
}

fn unused_local_base_url() -> Result<String> {
    let listener = TcpListener::bind("127.0.0.1:0")?;
    let addr = listener.local_addr()?;
    drop(listener);
    Ok(format!("http://{addr}"))
}

/// Runs the producer one time for a V4V track with a `WPAY`, and gives the
/// `display.json` that it wrote after the route service answer.
fn once_display_for_v4v_track(server: &TestServer) -> Result<serde_json::Value> {
    let temp = TempDir::new()?;
    let v4v_root = temp.path().join("V4Vmusic");
    fs::create_dir_all(&v4v_root)?;
    let track = mp3_with_wpay_at(&v4v_root.join("v4v-with-wpay.mp3"))?;
    let mut db = SyntheticMixxxDb::new()?;
    db.append_history_row_with_metadata(Some("V4V Artist"), Some("V4V Title"), None, &track)?;
    let display_dir = temp.path().join("display");
    let xdg_config = temp.path().join("xdg-config");
    let hitit_config_dir = xdg_config.join("hitit");
    fs::create_dir_all(&hitit_config_dir)?;
    fs::write(
        hitit_config_dir.join("config.toml"),
        format!("musicindex_endpoint = {:?}\n", server.base_url()),
    )?;
    let output = Command::new(binary())
        .env("HOME", temp.path().join("home"))
        .env("XDG_CONFIG_HOME", &xdg_config)
        .arg("--once")
        .arg("--db-file")
        .arg(db.path())
        .arg("--txt-file")
        .arg(temp.path().join("now-playing.txt"))
        .arg("--id3-file")
        .arg(temp.path().join("metadata.txt"))
        .arg("--v4v-root")
        .arg(&v4v_root)
        .arg("--display-dir")
        .arg(&display_dir)
        .output()?;
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    Ok(serde_json::from_str(&fs::read_to_string(
        display_dir.join("display.json"),
    )?)?)
}

#[test]
fn the_producer_writes_the_answer_links_over_the_file_wpay() -> Result<()> {
    let _network = network_test_lock();
    let server = test_server(vec![(
        "/v1/feeds/feed-guid-fixture/tracks/track-guid-fixture?include=payment_routes,source_links",
        Reply::Answer {
            status: 200,
            body: funding_body(
                false,
                serde_json::json!([{
                    "link_type": "funding",
                    "url": "https://item.example/support",
                    "label": "Item support",
                    "position": 1
                }]),
            ),
        },
    )])?;
    let display = once_display_for_v4v_track(&server)?;
    assert_eq!(
        display["track"]["funding"],
        serde_json::json!([{"url": "https://item.example/support", "label": "Item support"}])
    );
    assert!(display["track"].get("pub_date").is_none());
    Ok(())
}

#[test]
fn the_producer_writes_the_file_wpay_after_a_404() -> Result<()> {
    let _network = network_test_lock();
    let server = test_server(Vec::new())?;
    let display = once_display_for_v4v_track(&server)?;
    assert_eq!(
        display["track"]["funding"],
        serde_json::json!([{"url": "https://file.example/pay"}])
    );
    assert_eq!(
        server.requests(),
        [
            "/v1/feeds/feed-guid-fixture/tracks/track-guid-fixture?include=payment_routes,source_links",
            "/v1/feeds/feed-guid-fixture?include=source_links",
        ]
    );
    Ok(())
}
