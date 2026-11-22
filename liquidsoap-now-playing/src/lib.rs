//! Read Liquidsoap state and publish producer drop files.

pub mod chapters;
pub mod state;

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use now_playing_drop::merge::{has_percentage, local_split_routes, merged_routes};
use now_playing_drop::sink::{OutputFile, Presence};
use now_playing_drop::{
    InstanceSettings, LOCK_WAIT_LIMIT, PaymentRoute, ProducerLock, Version3DropFile,
};

use state::StateFile;

const FRESHNESS: Duration = Duration::from_secs(5);

/// The route service result of each chapter song, by the pair of feed GUID
/// and item GUID (ADR 0025 §The Chapter Route Lookup). An empty list is the
/// result "no routes".
pub type SongRoutes = HashMap<(String, String), Vec<PaymentRoute>>;

/// Tracks state freshness, lock ownership, and the output file.
#[derive(Debug)]
pub struct Producer {
    state_path: PathBuf,
    target: String,
    output: OutputFile,
    drop_path: PathBuf,
    lock: Option<ProducerLock>,
    last_written: Option<f64>,
    written_changed_at: Option<Instant>,
    unknown_format: Option<String>,
    bad_routes_seq: Option<i64>,
    bad_chapters_seq: Option<i64>,
    song_routes: SongRoutes,
}

impl Producer {
    /// Creates a producer for one state file and configured instance.
    pub fn new(state_path: impl Into<PathBuf>, settings: InstanceSettings) -> Self {
        let drop_path = settings.drop_dir.join(format!("{}.json", settings.target));
        Self {
            state_path: state_path.into(),
            target: settings.target,
            output: OutputFile::new(&drop_path),
            drop_path,
            lock: None,
            last_written: None,
            written_changed_at: None,
            unknown_format: None,
            bad_routes_seq: None,
            bad_chapters_seq: None,
            song_routes: SongRoutes::new(),
        }
    }

    /// Reads state and applies its output state at monotonic time `now`.
    ///
    /// # Errors
    ///
    /// Returns an error when the lock or output file cannot be changed.
    pub fn poll(&mut self, now: Instant) -> Result<()> {
        let bytes = match fs::read(&self.state_path) {
            Ok(bytes) => bytes,
            Err(_) => return self.clear(),
        };
        // ADR 0022: a state file that stays bad stops the heartbeat, so the
        // freshness rule removes the drop file after the limit.
        let header: state::StateHeader = match serde_json::from_slice(&bytes) {
            Ok(header) => header,
            Err(_) => return self.keep_while_fresh(now),
        };
        if header.format != state::STATE_FORMAT && header.format != state::STATE_FORMAT_2 {
            if self.unknown_format.as_deref() != Some(&header.format) {
                tracing::error!(format = %header.format, path = %self.state_path.display(), "unknown Liquidsoap state format");
                self.unknown_format = Some(header.format);
            }
            return self.clear();
        }
        self.unknown_format = None;
        let parsed: StateFile = match serde_json::from_slice(&bytes) {
            Ok(state) => state,
            Err(_) => return self.keep_while_fresh(now),
        };
        // At start, the state is not fresh until `written` changes one time
        // (ADR 0022). A change of `written` is the heartbeat of Liquidsoap.
        match self.last_written {
            None => self.last_written = Some(parsed.written),
            Some(previous) if previous != parsed.written => {
                self.last_written = Some(parsed.written);
                self.written_changed_at = Some(now);
            }
            Some(_) => {}
        }
        if !self.is_fresh(now) || parsed.live {
            return self.clear();
        }
        self.ensure_lock()?;
        let content = render_state(
            &parsed,
            &self.target,
            &self.song_routes,
            &mut self.bad_routes_seq,
            &mut self.bad_chapters_seq,
        )?;
        self.output
            .set(Presence::Present(serde_json::to_string_pretty(&content)?))
    }

    fn is_fresh(&self, now: Instant) -> bool {
        self.written_changed_at
            .is_some_and(|changed| now.saturating_duration_since(changed) < FRESHNESS)
    }

    /// Keeps the drop file of a state that the producer cannot parse while
    /// the last heartbeat is fresh, and removes it after that.
    fn keep_while_fresh(&mut self, now: Instant) -> Result<()> {
        if self.is_fresh(now) {
            Ok(())
        } else {
            self.clear()
        }
    }

    /// Removes the drop file and releases the producer lock.
    ///
    /// # Errors
    ///
    /// Returns an error when the drop file cannot be removed.
    pub fn shutdown(&mut self) -> Result<()> {
        self.clear()
    }

    fn ensure_lock(&mut self) -> Result<()> {
        if self.lock.is_none() {
            let parent = self.drop_path.parent().unwrap_or_else(|| Path::new("."));
            fs::create_dir_all(parent)
                .with_context(|| format!("create drop directory {}", parent.display()))?;
            self.lock = Some(ProducerLock::acquire_within(parent, LOCK_WAIT_LIMIT)?);
        }
        Ok(())
    }

    fn clear(&mut self) -> Result<()> {
        self.output.set(Presence::Absent)?;
        self.lock = None;
        Ok(())
    }
}

/// Makes the drop file for one fresh state that is not live (ADR 0022
/// §What The Producer Writes, and the chapter rows of ADR 0025). `bad_routes_seq`
/// and `bad_chapters_seq` hold the `seq` of the last error log, so that each
/// track gives one error line for each fault.
fn render_state(
    state: &StateFile,
    target: &str,
    song_routes: &SongRoutes,
    bad_routes_seq: &mut Option<i64>,
    bad_chapters_seq: &mut Option<i64>,
) -> Result<serde_json::Value> {
    let play_ms = (state.track_started * 1000.0) as i64;
    // A jingle has no identity and no routes (ADR 0022 §Jingle Mode).
    if state.jingle {
        return Ok(serde_json::to_value(Version3DropFile {
            schema: "musicindex.nowplaying/3",
            target,
            artist: "",
            title: "",
            duration_secs: None,
            image: None,
            feed_guid: None,
            track_guid: None,
            album: None,
            play_id: None,
            value_routes: Vec::new(),
            value_routes_source: None,
        })?);
    }
    let episode_feed = nonempty(state.feed_guid.as_deref());
    let episode_item = nonempty(state.track_guid.as_deref());
    // ADR 0024: routes only with the feed GUID and the track GUID.
    let guid_pair = episode_feed.is_some() && episode_item.is_some();
    let episode_routes = match nonempty(state.value_routes.as_deref()) {
        Some(raw) if guid_pair => match serde_json::from_str::<Vec<PaymentRoute>>(raw) {
            Ok(parsed) => parsed,
            Err(error) => {
                if *bad_routes_seq != Some(state.seq) {
                    tracing::error!(seq = state.seq, %error, "value_routes in the state file is not valid JSON");
                    *bad_routes_seq = Some(state.seq);
                }
                Vec::new()
            }
        },
        _ => Vec::new(),
    };
    let episode_usable = guid_pair && has_percentage(&episode_routes);
    let usable_episode: &[PaymentRoute] = if episode_usable { &episode_routes } else { &[] };

    let chapters = if state.format == state::STATE_FORMAT_2 {
        chapters::parse_chapters(state, bad_chapters_seq)
    } else {
        Vec::new()
    };
    let segment = position_ms(state).and_then(|position| chapters::segment_at(&chapters, position));
    let chapter = segment.and_then(|segment| segment.chapter);

    let mut feed_guid = episode_feed;
    let mut track_guid = episode_item;
    let routes = match chapter {
        None => usable_episode.to_vec(),
        Some(chapter) if chapter.is_local() => {
            if episode_usable {
                local_split_routes(&episode_routes, &chapter.value_routes)
            } else {
                Vec::new()
            }
        }
        Some(chapter) => match identified_pair(chapter) {
            Some((feed, item)) => {
                feed_guid = Some(feed);
                track_guid = Some(item);
                let song = select_song_routes(chapter, feed, item, song_routes);
                // ADR 0025: with no song routes, the chapter pays nobody.
                if has_percentage(song) {
                    merged_routes(usable_episode, song, chapter.remote_percentage)
                } else {
                    Vec::new()
                }
            }
            None => usable_episode.to_vec(),
        },
    };
    let title = chapter
        .and_then(|chapter| nonempty(chapter.title.as_deref()))
        .unwrap_or(&state.title);
    let artist = chapter
        .and_then(|chapter| nonempty(chapter.artist.as_deref()))
        .unwrap_or(&state.artist);
    // ADR 0025: each segment of a file with chapters gets its own play.
    let play_id = match segment {
        Some(segment) => format!("ls-{play_ms}-{}-{}", state.seq, segment.start_ms),
        None => format!("ls-{play_ms}-{}", state.seq),
    };
    let routes_source = (!routes.is_empty()).then_some("liquidsoap-state");
    Ok(serde_json::to_value(Version3DropFile {
        schema: "musicindex.nowplaying/3",
        target,
        artist,
        title,
        duration_secs: None,
        image: nonempty(state.image.as_deref()),
        feed_guid,
        track_guid,
        album: nonempty(state.album.as_deref()),
        play_id: Some(play_id),
        value_routes: routes,
        value_routes_source: routes_source,
    })?)
}

/// The position in the file in milliseconds (ADR 0025 §The Position Rule).
fn position_ms(state: &StateFile) -> Option<i64> {
    let elapsed = state.elapsed.filter(|value| value.is_finite())?;
    let cue_in = state.cue_in.unwrap_or(0.0);
    let position = (1000.0 * (elapsed + cue_in)).round();
    (position.is_finite() && position >= 0.0).then_some(position as i64)
}

/// The GUIDs of an identified remote split, or `None` (ADR 0025 §What The
/// Producer Writes).
fn identified_pair(chapter: &chapters::Chapter) -> Option<(&str, &str)> {
    let feed = nonempty(chapter.feed_guid.as_deref())?;
    let item = nonempty(chapter.item_guid.as_deref())?;
    (chapter.remote_percentage > 0.0).then_some((feed, item))
}

/// The song routes of an identified remote split: the service result when it
/// has a percentage, else the routes of the file (ADR 0025 §The Chapter Route
/// Lookup).
fn select_song_routes<'a>(
    chapter: &'a chapters::Chapter,
    feed: &str,
    item: &str,
    song_routes: &'a SongRoutes,
) -> &'a [PaymentRoute] {
    match song_routes.get(&(feed.to_owned(), item.to_owned())) {
        Some(routes) if has_percentage(routes) => routes,
        _ => &chapter.value_routes,
    }
}

fn nonempty(value: Option<&str>) -> Option<&str> {
    value.filter(|value| !value.is_empty())
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::time::Instant;

    use anyhow::Result;
    use tempfile::TempDir;

    use super::*;

    fn settings(dir: &Path) -> InstanceSettings {
        InstanceSettings {
            drop_dir: dir.to_path_buf(),
            target: "test".to_owned(),
            display_dir: None,
        }
    }

    fn state(written: f64, live: bool, extra: &str) -> String {
        let mut value = serde_json::json!({
            "format": state::STATE_FORMAT, "seq": 4, "written": written, "live": live,
            "jingle": false, "track_started": 12.5, "artist": "Artist", "title": "Title",
            "album": "Album", "track_guid": "track", "feed_guid": "feed", "image": "image",
            "value_routes": "[{\"address\":\"lnbc\"}]"
        });
        if extra.contains("\"jingle\":true") {
            value["jingle"] = true.into();
        }
        if extra.contains("\"feed_guid\":\"\"") {
            value["feed_guid"] = "".into();
        }
        if extra.contains("\"value_routes\":\"bad\"") {
            value["value_routes"] = "bad".into();
        }
        value.to_string()
    }

    fn poll_pair(producer: &mut Producer, now: Instant) -> Result<()> {
        producer.poll(now)?;
        producer.poll(now + Duration::from_millis(500))
    }

    #[test]
    fn initial_observation_is_not_fresh_then_routes_are_written() -> Result<()> {
        let temp = TempDir::new()?;
        let state_path = temp.path().join("state.json");
        fs::write(&state_path, state(0.0, false, ""))?;
        let mut producer = Producer::new(&state_path, settings(&temp.path().join("drop")));
        let now = Instant::now();
        producer.poll(now)?;
        fs::write(&state_path, state(1.0, false, ""))?;
        producer.poll(now + Duration::from_millis(500))?;
        producer.poll(now + Duration::from_secs(1))?;
        let output: serde_json::Value = serde_json::from_slice(&fs::read(&producer.drop_path)?)?;
        assert_eq!(output["value_routes_source"], "liquidsoap-state");
        assert_eq!(output["play_id"], "ls-12500-4");
        assert!(producer.lock.is_some());
        Ok(())
    }

    #[test]
    fn a_state_that_stays_bad_clears_after_the_freshness_limit() -> Result<()> {
        // ADR 0022 §What The Producer Writes: a file that stays bad stops the
        // heartbeat, and the row "not fresh" then applies.
        let bad_texts: [&[u8]; 2] = [b"{", br#"{"format":"musicindex-liquidsoap-state/1"}"#];
        for bad in bad_texts {
            let temp = TempDir::new()?;
            let state_path = temp.path().join("state.json");
            fs::write(&state_path, state(0.0, false, ""))?;
            let mut producer = Producer::new(&state_path, settings(&temp.path().join("drop")));
            let now = Instant::now();
            producer.poll(now)?;
            fs::write(&state_path, state(1.0, false, ""))?;
            producer.poll(now + Duration::from_millis(500))?;
            assert!(producer.drop_path.exists());

            fs::write(&state_path, bad)?;
            producer.poll(now + Duration::from_secs(3))?;
            assert!(producer.drop_path.exists(), "fresh: {bad:?}");
            producer.poll(now + Duration::from_secs(6))?;
            assert!(!producer.drop_path.exists(), "stale: {bad:?}");
            assert!(producer.lock.is_none());
        }
        Ok(())
    }

    #[test]
    fn stale_live_jingle_and_missing_guid_states_clear_or_write_as_required() -> Result<()> {
        let temp = TempDir::new()?;
        let state_path = temp.path().join("state.json");
        let drop_dir = temp.path().join("drop");
        fs::write(&state_path, state(0.0, false, ""))?;
        let mut producer = Producer::new(&state_path, settings(&drop_dir));
        let now = Instant::now();
        producer.poll(now)?;
        fs::write(&state_path, state(1.0, false, ""))?;
        poll_pair(&mut producer, now + Duration::from_millis(500))?;
        let malformed = b"{";
        fs::write(&state_path, malformed)?;
        producer.poll(now + Duration::from_secs(1))?;
        assert!(producer.drop_path.exists());
        fs::write(&state_path, state(1.0, false, ""))?;
        producer.poll(now + Duration::from_secs(6))?;
        assert!(!producer.drop_path.exists());
        assert!(producer.lock.is_none());

        fs::write(&state_path, state(2.0, true, ""))?;
        poll_pair(&mut producer, now + Duration::from_secs(7))?;
        assert!(!producer.drop_path.exists());
        fs::write(&state_path, state(3.0, false, ",\"jingle\":true"))?;
        poll_pair(&mut producer, now + Duration::from_secs(8))?;
        let output: serde_json::Value = serde_json::from_slice(&fs::read(&producer.drop_path)?)?;
        assert_eq!(output["artist"], "");
        assert_eq!(output["title"], "");
        assert!(output["album"].is_null());
        producer.shutdown()?;
        Ok(())
    }

    #[test]
    fn unknown_format_and_invalid_routes_have_no_routes() -> Result<()> {
        let temp = TempDir::new()?;
        let path = temp.path().join("state.json");
        let drop_dir = temp.path().join("drop");
        fs::write(&path, state(0.0, false, ""))?;
        let mut producer = Producer::new(&path, settings(&drop_dir));
        let now = Instant::now();
        producer.poll(now)?;
        fs::write(&path, state(1.0, false, ""))?;
        poll_pair(&mut producer, now + Duration::from_millis(500))?;
        fs::write(
            &path,
            state(2.0, false, ",\"feed_guid\":\"\",\"value_routes\":\"bad\""),
        )?;
        poll_pair(&mut producer, now + Duration::from_secs(1))?;
        let output: serde_json::Value = serde_json::from_slice(&fs::read(&producer.drop_path)?)?;
        assert_eq!(output["value_routes"], serde_json::json!([]));
        assert!(output["value_routes_source"].is_null());
        fs::write(
            &path,
            state(3.0, false, "").replace(state::STATE_FORMAT, "other/9"),
        )?;
        poll_pair(&mut producer, now + Duration::from_secs(2))?;
        assert!(!producer.drop_path.exists());
        assert!(producer.lock.is_none());
        Ok(())
    }

    #[test]
    fn live_to_idle_takes_the_lock_without_a_heartbeat_change() -> Result<()> {
        let temp = TempDir::new()?;
        let path = temp.path().join("state.json");
        let drop_dir = temp.path().join("drop");
        fs::write(&path, state(1.0, true, ""))?;
        let mut producer = Producer::new(&path, settings(&drop_dir));
        let now = Instant::now();
        producer.poll(now)?;
        fs::write(&path, state(2.0, true, ""))?;
        producer.poll(now + Duration::from_millis(500))?;
        producer.poll(now + Duration::from_secs(1))?;
        assert!(!producer.drop_path.exists());
        fs::write(&path, state(2.0, false, ""))?;
        producer.poll(now + Duration::from_millis(1500))?;
        assert!(producer.drop_path.exists());
        assert!(producer.lock.is_some());
        producer.shutdown()?;
        Ok(())
    }

    /// Gives a producer that has a fresh state with `patch` applied. Each
    /// later `written` value goes through `write_patched`.
    fn patched(written: f64, patch: &serde_json::Value) -> Result<String> {
        let mut value: serde_json::Value = serde_json::from_str(&state(written, false, ""))?;
        if let (Some(target), Some(fields)) = (value.as_object_mut(), patch.as_object()) {
            for (key, field) in fields {
                target.insert(key.clone(), field.clone());
            }
        }
        Ok(value.to_string())
    }

    fn drop_for(patch: &serde_json::Value) -> Result<serde_json::Value> {
        let temp = TempDir::new()?;
        let path = temp.path().join("state.json");
        fs::write(&path, patched(0.0, patch)?)?;
        let mut producer = Producer::new(&path, settings(&temp.path().join("drop")));
        let now = Instant::now();
        producer.poll(now)?;
        fs::write(&path, patched(1.0, patch)?)?;
        producer.poll(now + Duration::from_millis(500))?;
        let output = serde_json::from_slice(&fs::read(&producer.drop_path)?)?;
        producer.shutdown()?;
        Ok(output)
    }

    #[test]
    fn heartbeat_change_keeps_the_drop_file_and_the_lock() -> Result<()> {
        let temp = TempDir::new()?;
        let path = temp.path().join("state.json");
        fs::write(&path, state(0.0, false, ""))?;
        let mut producer = Producer::new(&path, settings(&temp.path().join("drop")));
        let now = Instant::now();
        producer.poll(now)?;
        for step in 1..=6_u32 {
            fs::write(&path, state(f64::from(step), false, ""))?;
            producer.poll(now + Duration::from_secs(u64::from(step)))?;
            assert!(
                producer.drop_path.exists(),
                "drop file missing at step {step}"
            );
            assert!(producer.lock.is_some(), "lock released at step {step}");
        }
        producer.shutdown()?;
        assert!(!producer.drop_path.exists());
        assert!(producer.lock.is_none());
        Ok(())
    }

    #[test]
    fn jingle_has_no_routes_and_no_other_fields() -> Result<()> {
        let output = drop_for(&serde_json::json!({"jingle": true}))?;
        assert_eq!(output["value_routes"], serde_json::json!([]));
        assert!(output["value_routes_source"].is_null());
        for field in ["feed_guid", "track_guid", "image", "album", "play_id"] {
            assert!(output[field].is_null(), "{field} is not null");
        }
        Ok(())
    }

    #[test]
    fn routes_without_feed_guid_are_empty() -> Result<()> {
        for patch in [
            serde_json::json!({"feed_guid": null}),
            serde_json::json!({"feed_guid": ""}),
            serde_json::json!({"track_guid": ""}),
        ] {
            let output = drop_for(&patch)?;
            assert_eq!(output["value_routes"], serde_json::json!([]), "{patch}");
            assert!(output["value_routes_source"].is_null(), "{patch}");
        }
        Ok(())
    }

    #[test]
    fn invalid_or_empty_routes_text_gives_empty_routes() -> Result<()> {
        for raw in ["bad", ""] {
            let output = drop_for(&serde_json::json!({"value_routes": raw}))?;
            assert_eq!(output["value_routes"], serde_json::json!([]), "{raw}");
            assert_eq!(output["feed_guid"], "feed");
        }
        Ok(())
    }

    #[test]
    fn routes_with_the_two_guids_are_written() -> Result<()> {
        let output = drop_for(&serde_json::json!({}))?;
        assert_eq!(output["value_routes"][0]["address"], "lnbc");
        assert_eq!(output["value_routes_source"], "liquidsoap-state");
        assert_eq!(output["artist"], "Artist");
        Ok(())
    }

    const EPISODE_ROUTES: &str =
        r#"[{"address":"alice","split":95},{"address":"fee","split":5,"fee":true}]"#;

    fn remote(start: i64, end: i64, feed: &str, item: serde_json::Value) -> serde_json::Value {
        serde_json::json!({
            "start_ms": start, "end_ms": end, "title": format!("Song {start}"),
            "artist": format!("Artist {start}"), "feed_guid": feed, "item_guid": item,
            "remote_percentage": 90, "value_routes": null
        })
    }

    fn song_routes(pairs: &[(&str, &str)]) -> SongRoutes {
        pairs
            .iter()
            .map(|(feed, item)| {
                let routes = vec![
                    PaymentRoute {
                        address: Some(format!("{item}-a")),
                        split: Some(50.0),
                        ..PaymentRoute::default()
                    },
                    PaymentRoute {
                        address: Some(format!("{item}-b")),
                        split: Some(50.0),
                        ..PaymentRoute::default()
                    },
                ];
                (((*feed).to_owned(), (*item).to_owned()), routes)
            })
            .collect()
    }

    /// Gives the drop file for each `(elapsed, cue_in)` read of one format 2
    /// state with `chapters` and the fields of `patch`.
    fn chapter_drops(
        chapters: &serde_json::Value,
        patch: &serde_json::Value,
        songs: SongRoutes,
        reads: &[(f64, f64)],
    ) -> Result<Vec<serde_json::Value>> {
        let temp = TempDir::new()?;
        let path = temp.path().join("state.json");
        let mut producer = Producer::new(&path, settings(&temp.path().join("drop")));
        producer.song_routes = songs;
        let mut fields = serde_json::json!({
            "format": state::STATE_FORMAT_2,
            "chapters": chapters.to_string(),
            "value_routes": EPISODE_ROUTES,
        });
        if let (Some(target), Some(extra)) = (fields.as_object_mut(), patch.as_object()) {
            for (key, value) in extra {
                target.insert(key.clone(), value.clone());
            }
        }
        let now = Instant::now();
        fs::write(&path, patched(0.0, &fields)?)?;
        producer.poll(now)?;
        let mut outputs = Vec::new();
        for (step, (elapsed, cue_in)) in reads.iter().enumerate() {
            fields["elapsed"] = (*elapsed).into();
            fields["cue_in"] = (*cue_in).into();
            let written = f64::from(u32::try_from(step)?) + 1.0;
            fs::write(&path, patched(written, &fields)?)?;
            producer.poll(now + Duration::from_millis(100 * (u64::try_from(step)? + 1)))?;
            outputs.push(serde_json::from_slice(&fs::read(&producer.drop_path)?)?);
        }
        producer.shutdown()?;
        Ok(outputs)
    }

    fn addresses(output: &serde_json::Value) -> Vec<String> {
        output["value_routes"]
            .as_array()
            .map(|routes| {
                routes
                    .iter()
                    .map(|route| {
                        format!(
                            "{}={}",
                            route["address"].as_str().unwrap_or(""),
                            route["split"]
                        )
                    })
                    .collect()
            })
            .unwrap_or_default()
    }

    #[test]
    fn an_episode_plays_through_three_chapters_a_gap_and_the_end() -> Result<()> {
        let chapters = serde_json::json!([
            remote(0, 10_000, "f1", "i1".into()),
            remote(10_000, 20_000, "f2", "i2".into()),
            remote(25_000, 30_000, "f3", "i3".into()),
        ]);
        let songs = song_routes(&[("f1", "i1"), ("f2", "i2"), ("f3", "i3")]);
        let reads = [
            (5.0, 0.0),
            (15.0, 0.0),
            (22.0, 0.0),
            (27.0, 0.0),
            (31.0, 0.0),
        ];
        let out = chapter_drops(&chapters, &serde_json::json!({}), songs, &reads)?;
        let identity: Vec<_> = out
            .iter()
            .map(|o| (o["feed_guid"].clone(), o["track_guid"].clone()))
            .collect();
        assert_eq!(identity[0], ("f1".into(), "i1".into()));
        assert_eq!(identity[1], ("f2".into(), "i2".into()));
        assert_eq!(identity[2], ("feed".into(), "track".into()));
        assert_eq!(identity[3], ("f3".into(), "i3".into()));
        assert_eq!(identity[4], ("feed".into(), "track".into()));
        let play_ids: Vec<_> = out.iter().map(|o| o["play_id"].clone()).collect();
        assert_eq!(
            play_ids,
            [
                "ls-12500-4-0",
                "ls-12500-4-10000",
                "ls-12500-4-20000",
                "ls-12500-4-25000",
                "ls-12500-4-30000"
            ]
        );
        assert_eq!(out[0]["title"], "Song 0");
        assert_eq!(out[0]["artist"], "Artist 0");
        assert_eq!(out[2]["title"], "Title");
        // ADR 0025 §The Merged Routes item 8.
        assert_eq!(
            addresses(&out[0]),
            ["i1-a=42.75", "i1-b=42.75", "alice=9.5", "fee=5.0"]
        );
        assert_eq!(addresses(&out[2]), ["alice=95.0", "fee=5.0"]);
        assert_eq!(out[0]["value_routes_source"], "liquidsoap-state");
        Ok(())
    }

    #[test]
    fn the_chapter_limits_use_the_cue_in_point() -> Result<()> {
        let chapters = serde_json::json!([remote(20_000, 30_000, "f1", "i1".into())]);
        let songs = song_routes(&[("f1", "i1")]);
        // The position is elapsed + cue_in: 12.5 s + 7.499 s = 19 999 ms.
        let reads = [
            (7.499, 12.5),
            (7.5, 12.5),
            (7.501, 12.5),
            (17.499, 12.5),
            (17.5, 12.5),
            (17.501, 12.5),
        ];
        let out = chapter_drops(&chapters, &serde_json::json!({}), songs, &reads)?;
        let items: Vec<_> = out.iter().map(|o| o["track_guid"].clone()).collect();
        assert_eq!(items, ["track", "i1", "i1", "i1", "track", "track"]);
        Ok(())
    }

    #[test]
    fn overlapping_and_equal_starts() -> Result<()> {
        let chapters = serde_json::json!([
            remote(0, 10_000, "f1", "i1".into()),
            remote(5_000, 15_000, "f2", "i2".into()),
            remote(5_000, 15_000, "f3", "i3".into()),
        ]);
        let songs = song_routes(&[("f1", "i1"), ("f2", "i2"), ("f3", "i3")]);
        let out = chapter_drops(
            &chapters,
            &serde_json::json!({}),
            songs,
            &[(4.999, 0.0), (5.0, 0.0), (12.0, 0.0)],
        )?;
        let items: Vec<_> = out.iter().map(|o| o["track_guid"].clone()).collect();
        assert_eq!(items, ["i1", "i2", "i2"]);
        Ok(())
    }

    #[test]
    fn a_remote_chapter_with_a_missing_guid_pays_the_episode() -> Result<()> {
        for item in [serde_json::Value::Null, "".into(), "   ".into()] {
            let chapters = serde_json::json!([remote(0, 10_000, "f1", item.clone())]);
            let out = chapter_drops(
                &chapters,
                &serde_json::json!({}),
                song_routes(&[("f1", "")]),
                &[(1.0, 0.0)],
            )?;
            assert_eq!(out[0]["feed_guid"], "feed", "{item}");
            assert_eq!(out[0]["track_guid"], "track", "{item}");
            assert_eq!(addresses(&out[0]), ["alice=95.0", "fee=5.0"], "{item}");
        }
        Ok(())
    }

    #[test]
    fn the_file_routes_apply_when_the_service_gives_no_routes() -> Result<()> {
        let mut entry = remote(0, 10_000, "f1", "i1".into());
        entry["value_routes"] = serde_json::json!([{"address": "file", "split": 1}]);
        let chapters = serde_json::json!([entry]);
        let mut empty = SongRoutes::new();
        empty.insert(("f1".to_owned(), "i1".to_owned()), Vec::new());
        for songs in [SongRoutes::new(), song_routes(&[("f2", "i2")]), empty] {
            let out = chapter_drops(&chapters, &serde_json::json!({}), songs, &[(1.0, 0.0)])?;
            assert_eq!(out[0]["track_guid"], "i1");
            assert_eq!(addresses(&out[0]), ["file=85.5", "alice=9.5", "fee=5.0"]);
        }
        Ok(())
    }

    #[test]
    fn the_service_routes_win_over_the_file_routes() -> Result<()> {
        let mut entry = remote(0, 10_000, "f1", "i1".into());
        entry["value_routes"] = serde_json::json!([{"address": "file", "split": 1}]);
        let out = chapter_drops(
            &serde_json::json!([entry]),
            &serde_json::json!({}),
            song_routes(&[("f1", "i1")]),
            &[(1.0, 0.0)],
        )?;
        assert_eq!(
            addresses(&out[0]),
            ["i1-a=42.75", "i1-b=42.75", "alice=9.5", "fee=5.0"]
        );
        Ok(())
    }

    #[test]
    fn a_remote_chapter_with_no_song_routes_pays_nobody() -> Result<()> {
        let chapters = serde_json::json!([remote(0, 10_000, "f1", "i1".into())]);
        let out = chapter_drops(
            &chapters,
            &serde_json::json!({}),
            SongRoutes::new(),
            &[(1.0, 0.0)],
        )?;
        assert_eq!(out[0]["feed_guid"], "f1");
        assert_eq!(out[0]["track_guid"], "i1");
        assert_eq!(out[0]["value_routes"], serde_json::json!([]));
        assert!(out[0]["value_routes_source"].is_null());
        Ok(())
    }

    #[test]
    fn an_episode_with_one_guid_gives_the_song_routes_at_100() -> Result<()> {
        let chapters = serde_json::json!([remote(0, 10_000, "f1", "i1".into())]);
        let out = chapter_drops(
            &chapters,
            &serde_json::json!({"track_guid": ""}),
            song_routes(&[("f1", "i1")]),
            &[(1.0, 0.0), (11.0, 0.0)],
        )?;
        assert_eq!(out[0]["track_guid"], "i1");
        assert_eq!(addresses(&out[0]), ["i1-a=50.0", "i1-b=50.0"]);
        assert_eq!(out[1]["value_routes"], serde_json::json!([]));
        Ok(())
    }

    #[test]
    fn a_local_split_ignores_remote_percentage() -> Result<()> {
        for percentage in [0, 50, 100] {
            let chapters = serde_json::json!([{
                "start_ms": 0, "end_ms": 10_000, "title": null, "artist": null,
                "remote_percentage": percentage,
                "value_routes": [{"address": "local-a", "split": 1}, {"address": "local-b", "split": 3}]
            }]);
            let out = chapter_drops(
                &chapters,
                &serde_json::json!({}),
                SongRoutes::new(),
                &[(1.0, 0.0)],
            )?;
            assert_eq!(out[0]["track_guid"], "track");
            assert_eq!(
                addresses(&out[0]),
                ["fee=5.0", "local-a=23.75", "local-b=71.25"],
                "{percentage}"
            );
            // ADR 0025: a null title and a null artist give the state values.
            assert_eq!(out[0]["title"], "Title");
            assert_eq!(out[0]["artist"], "Artist");
        }
        Ok(())
    }

    #[test]
    fn remote_percentage_is_clamped_or_makes_the_entry_not_valid() -> Result<()> {
        let songs = || song_routes(&[("f1", "i1")]);
        let cases = [
            (serde_json::json!(-5), Some("track")),
            (serde_json::json!(105), Some("i1")),
            (serde_json::Value::Null, Some("i1")),
            (serde_json::json!("x"), None),
        ];
        for (value, item) in cases {
            let mut entry = remote(0, 10_000, "f1", "i1".into());
            entry["remote_percentage"] = value.clone();
            let out = chapter_drops(
                &serde_json::json!([entry]),
                &serde_json::json!({}),
                songs(),
                &[(1.0, 0.0)],
            )?;
            match item {
                // A clamped 0 makes the song group not usable.
                Some(item) => assert_eq!(out[0]["track_guid"], item, "{value}"),
                // A not valid entry leaves no chapter, so the file is one track.
                None => assert_eq!(out[0]["play_id"], "ls-12500-4", "{value}"),
            }
        }
        let mut full = remote(0, 10_000, "f1", "i1".into());
        full["remote_percentage"] = serde_json::json!(105);
        let out = chapter_drops(
            &serde_json::json!([full]),
            &serde_json::json!({}),
            songs(),
            &[(1.0, 0.0)],
        )?;
        assert_eq!(addresses(&out[0]), ["i1-a=47.5", "i1-b=47.5", "fee=5.0"]);
        Ok(())
    }

    #[test]
    fn bad_chapter_text_format_1_and_a_missing_position_give_one_track() -> Result<()> {
        let chapters = serde_json::json!([remote(0, 10_000, "f1", "i1".into())]);
        let songs = || song_routes(&[("f1", "i1")]);
        let bad = chapter_drops(
            &serde_json::json!("not json"),
            &serde_json::json!({}),
            songs(),
            &[(1.0, 0.0)],
        )?;
        assert_eq!(bad[0]["play_id"], "ls-12500-4");
        let format_1 = chapter_drops(
            &chapters,
            &serde_json::json!({"format": state::STATE_FORMAT}),
            songs(),
            &[(1.0, 0.0)],
        )?;
        assert_eq!(format_1[0]["track_guid"], "track");
        assert_eq!(format_1[0]["play_id"], "ls-12500-4");
        let negative = chapter_drops(&chapters, &serde_json::json!({}), songs(), &[(-2.0, 0.0)])?;
        assert_eq!(negative[0]["play_id"], "ls-12500-4");
        Ok(())
    }

    #[test]
    fn a_jingle_with_chapters_gives_the_jingle_drop_file() -> Result<()> {
        let chapters = serde_json::json!([remote(0, 10_000, "f1", "i1".into())]);
        let out = chapter_drops(
            &chapters,
            &serde_json::json!({"jingle": true}),
            song_routes(&[("f1", "i1")]),
            &[(1.0, 0.0)],
        )?;
        assert_eq!(out[0]["title"], "");
        assert_eq!(out[0]["value_routes"], serde_json::json!([]));
        for field in ["feed_guid", "track_guid", "image", "album", "play_id"] {
            assert!(out[0][field].is_null(), "{field}");
        }
        Ok(())
    }
}
