use std::collections::HashMap;
use std::io::{Read, Write};
use std::net::TcpListener;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use musicindex_convert::{ConverterObservation, ConverterProbe, ExecutableSource, ProbeOutcome};
use musicindex_fetch::ReaderConfig;
use musicindex_tags::{read_audio_tags, read_podcast_tags};
use serde_json::Value;
use sha2::Digest;
use station_import::{Options, run, run_with};

struct TestServer {
    base_url: String,
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

impl TestServer {
    fn start() -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind test server");
        listener
            .set_nonblocking(true)
            .expect("set nonblocking listener");
        let address = listener.local_addr().expect("server address");
        let base_url = format!("http://{address}");
        let tagged_audio = include_bytes!("fixtures/musicindex-tagged.mp3").to_vec();
        let untagged_audio = include_bytes!("fixtures/untagged.flac").to_vec();
        let routes = Arc::new(routes(&base_url, tagged_audio, untagged_audio));
        let stop = Arc::new(AtomicBool::new(false));
        let thread_stop = Arc::clone(&stop);
        let thread_routes = Arc::clone(&routes);
        let worker = thread::spawn(move || {
            while !thread_stop.load(Ordering::Relaxed) {
                match listener.accept() {
                    Ok((mut stream, _)) => {
                        let mut request = [0_u8; 4096];
                        let count = stream.read(&mut request).unwrap_or(0);
                        let request = String::from_utf8_lossy(&request[..count]);
                        let path = request
                            .lines()
                            .next()
                            .and_then(|line| line.split_whitespace().nth(1))
                            .unwrap_or("/");
                        let (status, content_type, body) = thread_routes
                            .get(path)
                            .map(|(status, content_type, body)| {
                                (*status, *content_type, body.as_slice())
                            })
                            .unwrap_or((404, "text/plain", b"missing"));
                        let label = if status == 200 { "OK" } else { "Not Found" };
                        let header = format!(
                            "HTTP/1.1 {status} {label}\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                            body.len()
                        );
                        let _ = stream.write_all(header.as_bytes());
                        let _ = stream.write_all(body);
                    }
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                        thread::sleep(Duration::from_millis(5));
                    }
                    Err(_) => break,
                }
            }
        });
        Self {
            base_url,
            stop,
            thread: Some(worker),
        }
    }

    fn config(&self) -> ReaderConfig {
        ReaderConfig {
            min_interval: Duration::ZERO,
            musicindex_base_url: self.base_url.clone(),
            ..ReaderConfig::default()
        }
    }
}

impl Drop for TestServer {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(worker) = self.thread.take() {
            worker.join().expect("stop test server");
        }
    }
}

fn routes(
    base_url: &str,
    tagged_audio: Vec<u8>,
    untagged_audio: Vec<u8>,
) -> HashMap<String, (u16, &'static str, Vec<u8>)> {
    let song_feed = format!(
        r#"<?xml version="1.0"?><rss version="2.0" xmlns:itunes="http://www.itunes.com/dtds/podcast-1.0.dtd" xmlns:podcast="https://podcastindex.org/namespace/1.0"><channel><title>Songs</title><itunes:author>Channel Band</itunes:author><podcast:guid>song-feed</podcast:guid><item><guid>song-item</guid><title>Song</title><itunes:author>Item Band</itunes:author><enclosure url="{base_url}/audio/tagged.mp3" type="audio/mpeg" length="1"/></item><item><guid>song-item-2</guid><title>Second Song</title><enclosure url="{base_url}/audio/untagged.flac" type="audio/flac" length="1"/></item></channel></rss>"#
    );
    let failed_song_feed = format!(
        r#"<?xml version="1.0"?><rss version="2.0" xmlns:podcast="https://podcastindex.org/namespace/1.0"><channel><podcast:guid>failed-feed</podcast:guid><item><guid>failed-song</guid><title>Missing Audio</title><enclosure url="{base_url}/audio/missing.mp3" type="audio/mpeg" length="1"/></item></channel></rss>"#
    );
    let episode_feed = format!(
        r#"<?xml version="1.0"?><rss version="2.0" xmlns:itunes="http://www.itunes.com/dtds/podcast-1.0.dtd" xmlns:podcast="https://podcastindex.org/namespace/1.0"><channel><title>Episodes</title><podcast:guid>podcast-feed</podcast:guid><item><guid>episode-guid</guid><title>Episode 7</title><podcast:episode>7</podcast:episode><enclosure url="{base_url}/audio/tagged.mp3" type="audio/mpeg" length="1"/><podcast:value type="lightning" method="keysend"><podcast:valueTimeSplit startTime="0" duration="60"><podcast:remoteItem feedGuid="song-feed" itemGuid="song-item"/></podcast:valueTimeSplit><podcast:valueTimeSplit startTime="60" duration="60"><podcast:remoteItem feedGuid="missing-song-feed" itemGuid="missing-song"/></podcast:valueTimeSplit></podcast:value></item><item><guid>episode-guid-2</guid><title>Episode 8</title><podcast:episode>8</podcast:episode><enclosure url="{base_url}/audio/tagged.mp3" type="audio/mpeg" length="1"/><podcast:value type="lightning" method="keysend"><podcast:valueTimeSplit startTime="0" duration="60"><podcast:remoteItem feedGuid="song-feed" itemGuid="song-item"/></podcast:valueTimeSplit><podcast:valueTimeSplit startTime="60" duration="60"><podcast:remoteItem feedGuid="song-feed" itemGuid="song-item-2"/></podcast:valueTimeSplit></podcast:value></item></channel></rss>"#
    );
    let escape_feed = format!(
        r#"<?xml version="1.0"?><rss version="2.0" xmlns:podcast="https://podcastindex.org/namespace/1.0"><channel><podcast:guid>escape-feed</podcast:guid><item><guid>../escape</guid><title>Escape</title><enclosure url="{base_url}/audio/tagged.mp3" type="audio/mpeg" length="1"/></item></channel></rss>"#
    );
    let wav_song_feed = format!(
        r#"<?xml version="1.0"?><rss version="2.0" xmlns:podcast="https://podcastindex.org/namespace/1.0"><channel><title>Wav Songs</title><podcast:guid>wav-feed</podcast:guid><item><guid>wav-song</guid><title>Wav Song</title><enclosure url="{base_url}/audio/song.wav" type="audio/wav" length="1"/></item></channel></rss>"#
    );
    let song_lookup = format!(r#"{{"data":{{"feed_url":"{base_url}/song-feed.xml"}}}}"#);
    let mut map = HashMap::new();
    map.insert(
        "/song-feed.xml".into(),
        (200, "application/rss+xml", song_feed.into_bytes()),
    );
    map.insert(
        "/failed-song-feed.xml".into(),
        (200, "application/rss+xml", failed_song_feed.into_bytes()),
    );
    map.insert(
        "/escape-feed.xml".into(),
        (200, "application/rss+xml", escape_feed.into_bytes()),
    );
    map.insert(
        "/episode-feed.xml".into(),
        (200, "application/rss+xml", episode_feed.into_bytes()),
    );
    map.insert(
        "/v1/feeds/song-feed".into(),
        (200, "application/json", song_lookup.into_bytes()),
    );
    map.insert(
        "/audio/tagged.mp3".into(),
        (200, "audio/mpeg", tagged_audio),
    );
    map.insert(
        "/audio/untagged.flac".into(),
        (200, "audio/flac", untagged_audio),
    );
    map.insert(
        "/wav-song-feed.xml".into(),
        (200, "application/rss+xml", wav_song_feed.into_bytes()),
    );
    map.insert("/audio/song.wav".into(), (200, "audio/wav", wav_bytes()));
    map.insert(
        "/audio/missing.mp3".into(),
        (404, "text/plain", b"missing".to_vec()),
    );
    map
}

fn song_list_feed(base_url: &str) -> String {
    format!(
        r#"<?xml version="1.0"?><rss version="2.0" xmlns:podcast="https://podcastindex.org/namespace/1.0"><channel><title>Station list</title><podcast:remoteItem medium="music" feedGuid="song-feed" feedUrl="{base_url}/song-feed.xml" itemGuid="song-item"/><podcast:remoteItem medium="music" feedGuid="song-feed" itemGuid="song-item-2"/></channel></rss>"#
    )
}

fn options(folder: &tempfile::TempDir) -> Options {
    Options {
        out: folder.path().join("out"),
        ..Options::default()
    }
}

fn write_local_feed(folder: &tempfile::TempDir, name: &str, body: &str) -> String {
    let path = folder.path().join(name);
    std::fs::write(&path, body).expect("write local feed");
    path.to_string_lossy().into_owned()
}

fn manifest(folder: &tempfile::TempDir) -> Value {
    serde_json::from_slice(
        &std::fs::read(folder.path().join("out/manifest.json")).expect("manifest"),
    )
    .expect("JSON manifest")
}

#[test]
fn dry_run_writes_the_manifest_and_no_audio() {
    let server = TestServer::start();
    let folder = tempfile::tempdir().expect("temporary folder");
    let mut options = options(&folder);
    options.songs = Some(write_local_feed(
        &folder,
        "music.xml",
        &song_list_feed(&server.base_url),
    ));
    options.dry_run = true;

    let result = run(&options, server.config()).expect("run import");
    let manifest = manifest(&folder);
    assert_eq!(result.song_count(), 2);
    assert_eq!(manifest["items"][0]["feed_guid"], "song-feed");
    assert_eq!(manifest["items"][0]["item_guid"], "song-item");
    assert_eq!(
        manifest["items"][0]["source_url"],
        format!("{}/audio/tagged.mp3", server.base_url)
    );
    assert_eq!(manifest["items"][0]["path"], "songs/song-item.mp3");
    assert_eq!(manifest["items"][1]["feed_guid"], "song-feed");
    assert_eq!(manifest["items"][1]["item_guid"], "song-item-2");
    assert_eq!(
        manifest["items"][1]["source_url"],
        format!("{}/audio/untagged.flac", server.base_url)
    );
    assert_eq!(manifest["items"][1]["path"], "songs/song-item-2.flac");
    assert_eq!(manifest["items"][0]["sha256"], Value::Null);
    assert_eq!(manifest["items"][1]["sha256"], Value::Null);
    let entries: Vec<_> = std::fs::read_dir(&options.out)
        .expect("output folder")
        .map(|entry| entry.expect("folder entry").file_name())
        .collect();
    assert_eq!(entries, ["manifest.json"]);
}

#[test]
fn run_writes_songs_with_their_guids() {
    let server = TestServer::start();
    let folder = tempfile::tempdir().expect("temporary folder");
    let mut options = options(&folder);
    options.songs = Some(write_local_feed(
        &folder,
        "music.xml",
        &song_list_feed(&server.base_url),
    ));

    let result = run(&options, server.config()).expect("run import");
    assert_eq!(result.error_count(), 0);
    for (guid, expected_feed) in [("song-item", "song-feed"), ("song-item-2", "song-feed")] {
        let extension = if guid == "song-item" { "mp3" } else { "flac" };
        let path = options.out.join(format!("songs/{guid}.{extension}"));
        assert!(path.is_file());
        let tags = read_podcast_tags(&path).expect("read output tags");
        assert_eq!(tags.feed_guid.as_deref(), Some(expected_feed));
        assert_eq!(tags.item_guid.as_deref(), Some(guid));
        assert!(
            manifest(&folder)["items"]
                .as_array()
                .expect("manifest items")
                .iter()
                .any(|item| item["item_guid"] == guid && item["sha256"].is_string())
        );
    }
}

#[test]
fn run_writes_an_episode_with_its_chapters() {
    let server = TestServer::start();
    let folder = tempfile::tempdir().expect("temporary folder");
    let mut options = options(&folder);
    options.episode_feed = Some(format!("{}/episode-feed.xml", server.base_url));
    options.episodes = vec!["episode-guid".into()];

    let result = run(&options, server.config()).expect("run import");
    assert_eq!(result.error_count(), 0);
    let path = options.out.join("episodes/episode-guid.mp3");
    assert!(path.is_file());
    assert_eq!(
        read_podcast_tags(&path)
            .expect("read episode tags")
            .chapters()
            .expect("read chapters")
            .len(),
        2
    );
    let manifest = manifest(&folder);
    assert_eq!(manifest["items"][0]["chapters"], 2);
    assert_eq!(
        manifest["items"][0]["unresolved_splits"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    assert_eq!(
        manifest["items"][0]["unresolved_splits"][0]["feed_guid"],
        "missing-song-feed"
    );
}

#[test]
fn manifest_hash_and_size_are_of_the_tagged_file() {
    use sha2::{Digest, Sha256};
    let server = TestServer::start();
    let folder = tempfile::tempdir().expect("temporary folder");
    let mut options = options(&folder);
    options.songs = Some(write_local_feed(
        &folder,
        "music.xml",
        &song_list_feed(&server.base_url),
    ));
    options.episode_feed = Some(format!("{}/episode-feed.xml", server.base_url));
    options.episodes = vec!["episode-guid".into()];

    let result = run(&options, server.config()).expect("run import");
    assert_eq!(result.error_count(), 0);
    let served = [
        format!(
            "{:x}",
            Sha256::digest(include_bytes!("fixtures/musicindex-tagged.mp3"))
        ),
        format!(
            "{:x}",
            Sha256::digest(include_bytes!("fixtures/untagged.flac"))
        ),
    ];
    let manifest = manifest(&folder);
    let items = manifest["items"].as_array().expect("manifest items");
    assert_eq!(items.len(), 3);
    for item in items {
        let path = options.out.join(item["path"].as_str().expect("item path"));
        let bytes = std::fs::read(&path).expect("read output file");
        let sha256 = format!("{:x}", Sha256::digest(&bytes));
        assert_eq!(item["sha256"], sha256.as_str(), "{}", path.display());
        assert_eq!(item["bytes"], bytes.len(), "{}", path.display());
        assert!(
            !served.contains(&sha256),
            "the tag write changed {}",
            path.display()
        );
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(&path)
                .expect("file metadata")
                .permissions()
                .mode();
            assert_eq!(mode & 0o777, 0o644, "{}", path.display());
        }
    }
}

fn import_songs(server: &TestServer, folder: &tempfile::TempDir) -> Options {
    let mut options = options(folder);
    options.songs = Some(write_local_feed(
        folder,
        "music.xml",
        &song_list_feed(&server.base_url),
    ));
    let result = run(&options, server.config()).expect("run import");
    assert_eq!(result.error_count(), 0);
    options
}

#[test]
fn file_with_no_title_gets_the_item_title_artist_and_album() {
    let server = TestServer::start();
    let folder = tempfile::tempdir().expect("temporary folder");
    let options = import_songs(&server, &folder);

    // The FLAC fixture has no tag. The song has no item author.
    let tags = read_audio_tags(&options.out.join("songs/song-item-2.flac")).expect("read tags");
    assert_eq!(tags.title.as_deref(), Some("Second Song"));
    assert_eq!(tags.artist.as_deref(), Some("Channel Band"));
    assert_eq!(tags.album.as_deref(), Some("Songs"));
}

#[test]
fn file_with_a_title_keeps_it() {
    let server = TestServer::start();
    let folder = tempfile::tempdir().expect("temporary folder");
    let options = import_songs(&server, &folder);

    // The MP3 fixture has TIT2 and TPE1, and no TALB.
    let tags = read_audio_tags(&options.out.join("songs/song-item.mp3")).expect("read tags");
    assert_eq!(tags.title.as_deref(), Some("Fixture Title"));
    assert_eq!(tags.artist.as_deref(), Some("Fixture Artist"));
    assert_eq!(tags.album.as_deref(), Some("Songs"));
}

#[test]
fn chapter_artist_is_the_item_author_else_the_channel_author() {
    let server = TestServer::start();
    let folder = tempfile::tempdir().expect("temporary folder");
    let mut options = options(&folder);
    options.episode_feed = Some(format!("{}/episode-feed.xml", server.base_url));
    options.episodes = vec!["episode-guid-2".into()];

    let result = run(&options, server.config()).expect("run import");
    assert_eq!(result.error_count(), 0);
    let chapters = read_podcast_tags(&options.out.join("episodes/episode-guid-2.mp3"))
        .expect("read episode tags")
        .chapters()
        .expect("read chapters");
    assert_eq!(chapters.len(), 2);
    // A song with an item author: the chapter artist is that author.
    assert_eq!(chapters[0].artist.as_deref(), Some("Item Band"));
    assert_eq!(chapters[0].author.as_deref(), Some("Channel Band"));
    // A song with no item author: the chapter artist is the channel author.
    assert_eq!(chapters[1].artist.as_deref(), Some("Channel Band"));
    assert_eq!(chapters[1].author.as_deref(), Some("Channel Band"));
}

#[test]
fn episode_selected_by_number() {
    let server = TestServer::start();
    let folder = tempfile::tempdir().expect("temporary folder");
    let mut options = options(&folder);
    options.episode_feed = Some(format!("{}/episode-feed.xml", server.base_url));
    options.episodes = vec!["7".into()];

    let result = run(&options, server.config()).expect("run import");
    assert_eq!(result.error_count(), 0);
    assert!(options.out.join("episodes/episode-guid.mp3").is_file());
    assert_eq!(manifest(&folder)["items"][0]["selector"], "7");
}

#[test]
fn missing_episode_is_an_error_entry_and_the_run_continues() {
    let server = TestServer::start();
    let folder = tempfile::tempdir().expect("temporary folder");
    let mut options = options(&folder);
    options.episode_feed = Some(format!("{}/episode-feed.xml", server.base_url));
    options.episodes = vec!["no-such-episode".into(), "7".into()];

    let result = run(&options, server.config()).expect("run import");
    assert_eq!(result.error_count(), 1);
    assert!(manifest(&folder)["items"][0]["error"].is_string());
    assert!(options.out.join("episodes/episode-guid.mp3").is_file());
}

#[test]
fn failed_download_leaves_no_file() {
    let server = TestServer::start();
    let folder = tempfile::tempdir().expect("temporary folder");
    let mut options = options(&folder);
    let feed = format!(
        r#"<?xml version="1.0"?><rss version="2.0" xmlns:podcast="https://podcastindex.org/namespace/1.0"><channel><podcast:remoteItem medium="music" feedGuid="failed-feed" feedUrl="{}/failed-song-feed.xml" itemGuid="failed-song"/></channel></rss>"#,
        server.base_url
    );
    options.songs = Some(write_local_feed(&folder, "music.xml", &feed));

    let result = run(&options, server.config()).expect("run import");
    assert_eq!(result.error_count(), 1);
    assert!(manifest(&folder)["items"][0]["error"].is_string());
    assert!(!options.out.join("songs/failed-song.mp3").exists());
}

#[test]
fn unsafe_item_guid_gets_a_hash_file_name() {
    let server = TestServer::start();
    let folder = tempfile::tempdir().expect("temporary folder");
    let mut options = options(&folder);
    let feed = format!(
        r#"<?xml version="1.0"?><rss version="2.0" xmlns:podcast="https://podcastindex.org/namespace/1.0"><channel><podcast:remoteItem medium="music" feedGuid="escape-feed" feedUrl="{}/escape-feed.xml" itemGuid="../escape"/></channel></rss>"#,
        server.base_url
    );
    options.songs = Some(write_local_feed(&folder, "music.xml", &feed));

    let result = run(&options, server.config()).expect("run import");
    assert_eq!(result.error_count(), 0);
    // sha256("../escape") in hex.
    let name = "sha256-1ba7343c47dc442de7dec43a995deb9a7b62234ecca16d7c6f597b5155bd85b1.mp3";
    let manifest = manifest(&folder);
    assert_eq!(manifest["items"][0]["item_guid"], "../escape");
    assert_eq!(manifest["items"][0]["path"], format!("songs/{name}"));
    let path = options.out.join("songs").join(name);
    let tags = read_podcast_tags(&path).expect("read output tags");
    assert_eq!(tags.item_guid.as_deref(), Some("../escape"));
    assert!(!folder.path().join("escape.mp3").exists());
    assert_eq!(
        std::fs::read_dir(options.out.join("songs"))
            .unwrap()
            .count(),
        1
    );
}

/// A WAV file with a 44-byte header and four samples of silence.
fn wav_bytes() -> Vec<u8> {
    let data = [0_u8; 8];
    let mut bytes = Vec::new();
    bytes.extend_from_slice(b"RIFF");
    bytes.extend_from_slice(&(36 + data.len() as u32).to_le_bytes());
    bytes.extend_from_slice(b"WAVEfmt ");
    bytes.extend_from_slice(&16_u32.to_le_bytes());
    bytes.extend_from_slice(&1_u16.to_le_bytes()); // PCM
    bytes.extend_from_slice(&1_u16.to_le_bytes()); // one channel
    bytes.extend_from_slice(&44_100_u32.to_le_bytes());
    bytes.extend_from_slice(&88_200_u32.to_le_bytes());
    bytes.extend_from_slice(&2_u16.to_le_bytes());
    bytes.extend_from_slice(&16_u16.to_le_bytes());
    bytes.extend_from_slice(b"data");
    bytes.extend_from_slice(&(data.len() as u32).to_le_bytes());
    bytes.extend_from_slice(&data);
    bytes
}

fn probe(
    executable: &std::path::Path,
    source: ExecutableSource,
    outcome: ProbeOutcome,
) -> ConverterProbe {
    ConverterProbe {
        executable: executable.to_path_buf(),
        source,
        recorded_at: chrono::Utc::now(),
        outcome,
    }
}

/// A converter observation with a `flac` script that copies the FLAC fixture
/// to the output path, and no `ffmpeg`.
fn script_converter(folder: &tempfile::TempDir) -> ConverterObservation {
    use std::os::unix::fs::PermissionsExt;
    let fixture = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/untagged.flac");
    let script = folder.path().join("flac");
    std::fs::write(
        &script,
        format!(
            "#!/bin/sh\nwhile [ $# -gt 0 ]; do if [ \"$1\" = -o ]; then out=\"$2\"; fi; shift; done\ncp '{fixture}' \"$out\"\n"
        ),
    )
    .expect("write converter script");
    std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755))
        .expect("make converter script executable");
    ConverterObservation {
        flac: probe(
            &script,
            ExecutableSource::Configured,
            ProbeOutcome::Exited(Some(0)),
        ),
        ffmpeg: probe(
            std::path::Path::new("ffmpeg"),
            ExecutableSource::Path,
            ProbeOutcome::Missing,
        ),
    }
}

fn wav_song_list(base_url: &str) -> String {
    format!(
        r#"<?xml version="1.0"?><rss version="2.0" xmlns:podcast="https://podcastindex.org/namespace/1.0"><channel><title>Station list</title><podcast:remoteItem medium="music" feedGuid="wav-feed" feedUrl="{base_url}/wav-song-feed.xml" itemGuid="wav-song"/><podcast:remoteItem medium="music" feedGuid="song-feed" feedUrl="{base_url}/song-feed.xml" itemGuid="song-item"/></channel></rss>"#
    )
}

fn song_files(options: &Options) -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(options.out.join("songs"))
        .expect("songs folder")
        .map(|entry| {
            entry
                .expect("entry")
                .file_name()
                .to_string_lossy()
                .into_owned()
        })
        .collect();
    names.sort();
    names
}

// musicindex ADR 0094 §Checks, R-CONVERT-7 items 1 and 2.
#[test]
fn wav_song_becomes_a_tagged_flac_file() {
    let server = TestServer::start();
    let folder = tempfile::tempdir().expect("temporary folder");
    let mut options = options(&folder);
    options.songs = Some(write_local_feed(
        &folder,
        "music.xml",
        &wav_song_list(&server.base_url),
    ));
    let converter = script_converter(&folder);

    let result = run_with(&options, server.config(), &|| converter.clone()).expect("run import");
    assert_eq!(result.error_count(), 0);
    assert_eq!(song_files(&options), ["song-item.mp3", "wav-song.flac"]);
    assert!(!options.out.join(".convert").exists());
    let flac = options.out.join("songs/wav-song.flac");
    let tags = read_podcast_tags(&flac).expect("read FLAC tags");
    assert_eq!(tags.feed_guid.as_deref(), Some("wav-feed"));
    assert_eq!(tags.item_guid.as_deref(), Some("wav-song"));
    let item = manifest(&folder)["items"][0].clone();
    assert_eq!(item["path"], "songs/wav-song.flac");
    let hash = format!(
        "{:x}",
        sha2::Sha256::digest(std::fs::read(&flac).expect("read FLAC"))
    );
    assert_eq!(item["sha256"], hash);
}

// musicindex ADR 0094 §Checks, R-CONVERT-7 item 3.
#[test]
fn wav_song_with_no_converter_is_an_error_and_the_run_continues() {
    let server = TestServer::start();
    let folder = tempfile::tempdir().expect("temporary folder");
    let mut options = options(&folder);
    options.songs = Some(write_local_feed(
        &folder,
        "music.xml",
        &wav_song_list(&server.base_url),
    ));
    let missing = std::path::Path::new("flac");
    let none = ConverterObservation {
        flac: probe(missing, ExecutableSource::Path, ProbeOutcome::Missing),
        ffmpeg: probe(
            std::path::Path::new("ffmpeg"),
            ExecutableSource::Path,
            ProbeOutcome::Missing,
        ),
    };

    let result = run_with(&options, server.config(), &|| none.clone()).expect("run import");
    assert_eq!(result.error_count(), 1);
    let items = manifest(&folder)["items"].clone();
    let error = items[0]["error"].as_str().unwrap_or_default().to_owned();
    assert!(error.contains("no converter"), "{error}");
    assert!(items[1]["error"].is_null());
    assert_eq!(song_files(&options), ["song-item.mp3"]);
    assert!(!options.out.join(".convert").exists());
}
