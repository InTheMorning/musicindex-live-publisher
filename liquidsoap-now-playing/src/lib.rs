//! Read Liquidsoap state and publish producer drop files.

pub mod chapters;
pub mod lookup;
pub mod state;

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use lookup::RouteLookups;
use now_playing_drop::display::{Artwork, DisplayOutput, ShownTrack, image_url};
use now_playing_drop::merge::{has_percentage, local_split_routes, merged_routes};
use now_playing_drop::picture::{MAX_PICTURE_BYTES, artwork_from, embedded_artwork};
use now_playing_drop::sink::{OutputFile, Presence};
use now_playing_drop::{
    InstanceSettings, LOCK_WAIT_LIMIT, PaymentRoute, ProducerLock, Version3DropFile,
    route_service::RouteServiceClient,
};

use state::StateFile;

const FRESHNESS: Duration = Duration::from_secs(5);
/// The longest song line that the relay accepts, in characters (ADR 0041 §The Publisher And The Relay).
const MAX_SONG_LINE_CHARS: usize = 1_024;

/// The route service result of each chapter song, by the pair of feed GUID
/// and item GUID (ADR 0041 §The Route Lookup). An empty list is the
/// result "no routes".
pub type SongRoutes = HashMap<(String, String), Vec<PaymentRoute>>;

/// Tracks state freshness, lock ownership, and the output file.
#[derive(Debug)]
pub struct Producer {
    state_path: PathBuf,
    state_dir: PathBuf,
    target: String,
    output: OutputFile,
    drop_path: PathBuf,
    lock: Option<ProducerLock>,
    last_written: Option<f64>,
    written_changed_at: Option<Instant>,
    unknown_format: Option<String>,
    bad_routes_seq: Option<i64>,
    bad_chapters_seq: Option<i64>,
    lookups: RouteLookups,
    display: Option<DisplayOutput>,
    picture_cache: Option<(PictureKey, Option<Artwork>)>,
}

/// Identifies one picture read. The `seq` of the slot starts again at 0 after a
/// restart of Liquidsoap, so the key also holds the track start and the name.
#[derive(Debug, Clone, PartialEq)]
struct PictureKey {
    seq: i64,
    track_started: u64,
    picture: String,
}

impl PictureKey {
    fn of(state: &StateFile) -> Self {
        Self {
            seq: state.seq,
            track_started: state.track_started.to_bits(),
            picture: state.picture.clone(),
        }
    }
}

impl Producer {
    /// Creates a producer for one state file and configured instance.
    pub fn new(state_path: impl Into<PathBuf>, settings: InstanceSettings) -> Self {
        let state_path = state_path.into();
        let state_dir = state_path
            .parent()
            .unwrap_or_else(|| Path::new("."))
            .to_path_buf();
        let drop_path = settings.drop_dir.join(format!("{}.json", settings.target));
        // ADR 0041 §What The Producer Writes In `display.json`: the producer writes `display.json` only with `display_dir`.
        let display = settings.display_dir.map(DisplayOutput::new);
        Self {
            state_path,
            state_dir,
            target: settings.target,
            output: OutputFile::new(&drop_path),
            drop_path,
            lock: None,
            last_written: None,
            written_changed_at: None,
            unknown_format: None,
            bad_routes_seq: None,
            bad_chapters_seq: None,
            lookups: RouteLookups::new(None),
            display,
            picture_cache: None,
        }
    }

    /// Writes the display state `null` at start (ADR 0041 §What The Producer Writes In `display.json`). Without a display directory, it does
    /// nothing. A display write error gives a warning and never stops the
    /// producer.
    ///
    /// # Errors
    ///
    /// This function does not return an error at this time. The `Result`
    /// type keeps the signature stable.
    pub fn start(&mut self) -> Result<()> {
        if let Some(display) = self.display.as_mut()
            && let Err(error) = display.start()
        {
            warn_display(display, &error);
        }
        Ok(())
    }

    /// Adds the route service client for chapter lookups.
    pub fn with_route_service(mut self, client: RouteServiceClient) -> Self {
        self.lookups = RouteLookups::new(Some(client));
        self
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
        // ADR 0041 §The State File Contract: a state file that stays bad stops the heartbeat, so the
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
        // (ADR 0041 §The State File Contract). A change of `written` is the heartbeat of Liquidsoap.
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
        let chapters = if parsed.format == state::STATE_FORMAT_2 {
            chapters::parse_chapters(&parsed, &mut self.bad_chapters_seq)
        } else {
            chapters::ChapterList::default()
        };
        let pairs = if parsed.jingle {
            Vec::new()
        } else if chapters.is_empty() {
            // ADR 0041 §The Route Lookup: a song state asks for its
            // two state GUIDs.
            song_pair(&parsed).into_iter().collect()
        } else {
            chapters
                .chapters()
                .filter_map(identified_pair)
                .map(|(feed, item)| (feed.to_owned(), item.to_owned()))
                .fold(Vec::new(), |mut pairs, pair| {
                    if !pairs.contains(&pair) {
                        pairs.push(pair);
                    }
                    pairs
                })
        };
        self.lookups.drain(now);
        self.lookups.update(parsed.seq, &pairs, now);
        let content = render_state(
            &parsed,
            &chapters,
            &self.target,
            self.lookups.results(),
            &mut self.bad_routes_seq,
        )?;
        self.output
            .set(Presence::Present(serde_json::to_string_pretty(&content)?))?;
        // ADR 0041 §Invariants: the display state follows the drop file of
        // the same read, and uses its artist, title, album and play_id.
        // ADR 0044 §The Station Producer: the segment that plays selects the
        // pair of the fresh links and tells if the state `wpay` applies. The
        // segment selection is the same as the selection of the drop file.
        let (selected, file_link) = if parsed.jingle {
            (None, false)
        } else if chapters.is_empty() {
            (song_pair(&parsed), true)
        } else {
            let identified = position_ms(&parsed)
                .and_then(|position| chapters::segment_at(&chapters, position))
                .and_then(|segment| segment.chapter)
                .and_then(identified_pair)
                .map(|(feed, item)| (feed.to_owned(), item.to_owned()));
            // An identified remote split never uses the `wpay` of the
            // episode file. Each other segment uses only that `wpay`.
            let file_link = identified.is_none();
            (identified, file_link)
        };
        let fresh = selected
            .as_ref()
            .map(|pair| self.lookups.funding(pair))
            .unwrap_or(&[]);
        let fallback = if fresh.is_empty() && file_link {
            display_keys::file_funding(parsed.wpay.as_deref().unwrap_or(""))
        } else {
            Vec::new()
        };
        let funding = if fresh.is_empty() {
            fallback
        } else {
            fresh.to_vec()
        };
        self.write_display_track(&parsed, &chapters, &content, &funding);
        Ok(())
    }

    /// Writes the display track of one fresh state (ADR 0041 §What The Producer Writes In `display.json`). A write error gives a warning.
    fn write_display_track(
        &mut self,
        state: &StateFile,
        chapters: &chapters::ChapterList,
        content: &serde_json::Value,
        funding: &[display_keys::FundingLink],
    ) {
        if self.display.is_none() {
            return;
        }
        let line = song_line(state).filter(|line| line.chars().count() <= MAX_SONG_LINE_CHARS);
        let Some(line) = line else {
            if let Some(display) = self.display.as_mut() {
                write_display(display, None);
            }
            return;
        };
        let (artist, title, play_id, album) = if state.jingle {
            // A jingle has the state artist, title and album, and no play_id.
            (
                state.artist.as_str(),
                state.title.as_str(),
                None,
                nonempty(state.album.as_deref()),
            )
        } else {
            (
                content["artist"].as_str().unwrap_or(""),
                content["title"].as_str().unwrap_or(""),
                content["play_id"].as_str(),
                nonempty(content["album"].as_str()),
            )
        };
        // ADR 0041 §The Artwork Of A Segment: the image URL of the drop file
        // (the chapter image, else the state image) wins when it is valid,
        // else the picture file of the state, else `null`. A jingle drop file
        // has no image, so a jingle uses the state image.
        let image = if state.jingle {
            nonempty(state.image.as_deref())
        } else {
            content["image"].as_str()
        };
        let artwork = artwork_from(image, None).or_else(|| self.picture_artwork(state));
        if let Some(display) = self.display.as_mut() {
            write_display(
                display,
                Some(ShownTrack {
                    artist,
                    title,
                    artwork: artwork.as_ref(),
                    song_line: &line,
                    play_id,
                    album,
                    pub_date: if !state.jingle && !chapters.is_empty() {
                        Some(state.pub_date.as_deref().and_then(display_keys::pub_day))
                    } else {
                        Some(None)
                    },
                    funding: Some(funding),
                }),
            );
        }
    }

    /// Gives the artwork of the picture file of the state. The producer
    /// prepares the picture one time for each track (ADR 0041 §The Producer Reads The Picture File
    /// Reads The Picture File).
    fn picture_artwork(&mut self, state: &StateFile) -> Option<Artwork> {
        let key = PictureKey::of(state);
        if let Some((cached_key, cached)) = &self.picture_cache
            && *cached_key == key
        {
            return cached.clone();
        }
        let artwork = self.read_picture(state);
        self.picture_cache = Some((key, artwork.clone()));
        artwork
    }

    /// Reads and prepares the picture file. A name that is not `picture-`
    /// and 1 to 20 digits, a file of more than 64 MiB, and a missing file
    /// give `None`.
    fn read_picture(&self, state: &StateFile) -> Option<Artwork> {
        let name = state.picture.as_str();
        let digits = name.strip_prefix("picture-")?;
        if digits.is_empty()
            || digits.len() > 20
            || !digits.bytes().all(|byte| byte.is_ascii_digit())
        {
            return None;
        }
        let path = self.state_dir.join(name);
        let metadata = match fs::metadata(&path) {
            Ok(metadata) => metadata,
            Err(error) => {
                tracing::debug!(path = %path.display(), %error, "cannot read picture metadata");
                return None;
            }
        };
        if metadata.len() > MAX_PICTURE_BYTES as u64 {
            return None;
        }
        match fs::read(&path) {
            Ok(bytes) => embedded_artwork(Some(&bytes)),
            Err(error) => {
                tracing::debug!(path = %path.display(), %error, "cannot read picture file");
                None
            }
        }
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
        self.picture_cache = None;
        // ADR 0041 §What The Producer Writes In `display.json`: the display state is `null` when the producer releases
        // the lock. The writer does not rewrite a file with the same content.
        if let Some(display) = self.display.as_mut() {
            write_display(display, None);
        }
        Ok(())
    }
}

/// Writes the display state. A display error gives a warning and never stops
/// the drop file (ADR 0041 §What The Producer Writes In `display.json`).
fn write_display(display: &mut DisplayOutput, track: Option<ShownTrack<'_>>) {
    if let Err(error) = display.write(track) {
        warn_display(display, &error);
    }
}

fn warn_display(output: &DisplayOutput, error: &anyhow::Error) {
    let dir = output.dir().display();
    tracing::warn!(
        dir = %dir,
        error = %format!("{error:#}"),
        "display write failed"
    );
}

/// The song line of a file (ADR 0041 §The Song Line): `song`, else
/// `artist - title`, else `artist`, else `title`, else `None`. It uses the
/// strings of the state with no change.
fn song_line(state: &StateFile) -> Option<String> {
    if !state.song.is_empty() {
        return Some(state.song.clone());
    }
    match (state.artist.is_empty(), state.title.is_empty()) {
        (false, false) => Some(format!("{} - {}", state.artist, state.title)),
        (false, true) => Some(state.artist.clone()),
        (true, false) => Some(state.title.clone()),
        (true, true) => None,
    }
}

/// Makes the drop file for one fresh state that is not live (ADR 0041 §What The Producer Writes and §What The Producer Writes For An Episode File). `bad_routes_seq`
/// and `bad_chapters_seq` hold the `seq` of the last error log, so that each
/// track gives one error line for each fault.
fn render_state(
    state: &StateFile,
    chapters: &chapters::ChapterList,
    target: &str,
    song_routes: &SongRoutes,
    bad_routes_seq: &mut Option<i64>,
) -> Result<serde_json::Value> {
    let play_ms = (state.track_started * 1000.0) as i64;
    // A jingle has no identity and no routes (ADR 0041 §Jingle Mode).
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

    let segment = position_ms(state).and_then(|position| chapters::segment_at(chapters, position));
    let chapter = segment.and_then(|segment| segment.chapter);

    let mut feed_guid = episode_feed;
    let mut track_guid = episode_item;
    let routes = match chapter {
        // ADR 0041 §The Song Routes: the service result, else the state.
        None if chapters.is_empty() => match song_pair(state) {
            Some(pair) => match song_routes.get(&pair) {
                Some(routes) if has_percentage(routes) => routes.clone(),
                _ => usable_episode.to_vec(),
            },
            None => usable_episode.to_vec(),
        },
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
                // ADR 0041 §What The Producer Writes For An Episode File: with no song routes, the chapter pays nobody.
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
    // ADR 0041 §The Album Of A Segment: a chapter with a title gives the episode title as the album,
    // because the title line then shows the chapter song.
    let album = chapter
        .and_then(|chapter| nonempty(chapter.title.as_deref()))
        .and_then(|_| nonempty(Some(state.title.as_str())))
        .or_else(|| nonempty(state.album.as_deref()));
    // ADR 0041 §What The Producer Writes For An Episode File: each segment of a file with chapters gets its own play.
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
        // ADR 0041 §The Artwork Of A Segment: the chapter image first.
        image: chapter
            .and_then(|chapter| chapter.image_url.as_deref())
            .or_else(|| nonempty(state.image.as_deref()).and_then(image_url)),
        feed_guid,
        track_guid,
        album,
        play_id: Some(play_id),
        value_routes: routes,
        value_routes_source: routes_source,
    })?)
}

/// The position in the file in milliseconds (ADR 0041 §The Position Rule).
fn position_ms(state: &StateFile) -> Option<i64> {
    let elapsed = state.elapsed.filter(|value| value.is_finite())?;
    let cue_in = state.cue_in.unwrap_or(0.0);
    let position = (1000.0 * (elapsed + cue_in)).round();
    (position.is_finite() && position >= 0.0).then_some(position as i64)
}

/// The GUIDs of an identified remote split, or `None` (ADR 0041 §What The Producer Writes For An Episode File).
fn identified_pair(chapter: &chapters::Chapter) -> Option<(&str, &str)> {
    let feed = nonempty(chapter.feed_guid.as_deref())?;
    let item = nonempty(chapter.item_guid.as_deref())?;
    (chapter.remote_percentage > 0.0).then_some((feed, item))
}

/// The song routes of an identified remote split: the service result when it
/// has a percentage, else the routes of the file (ADR 0041 §The Route Lookup).
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

/// The two state GUIDs of a song state, or `None` (ADR 0041 §The Route Lookup
/// Selects The Lookup). The caller checks that the state has no chapter list.
fn song_pair(state: &StateFile) -> Option<(String, String)> {
    let feed = nonempty(state.feed_guid.as_deref())?;
    let item = nonempty(state.track_guid.as_deref())?;
    (!state.jingle && !state.live).then(|| (feed.to_owned(), item.to_owned()))
}

fn nonempty(value: Option<&str>) -> Option<&str> {
    value.filter(|value| !value.is_empty())
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::time::Instant;

    use anyhow::Result;
    use now_playing_drop::route_service::RouteServiceClient;
    use now_playing_drop::test_server::{Reply, TestServer};
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
        // ADR 0041 §What The Producer Writes: a file that stays bad stops the
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
        producer.lookups.insert_for_test(songs);
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

    fn chapter_patch(chapters: &serde_json::Value, seq: i64) -> serde_json::Value {
        serde_json::json!({
            "format": state::STATE_FORMAT_2,
            "seq": seq,
            "chapters": chapters.to_string(),
            "elapsed": 1.0,
            "cue_in": 0.0,
        })
    }

    fn start_service_producer(
        temp: &TempDir,
        server: &TestServer,
        patch: &serde_json::Value,
        now: Instant,
        timeout: Duration,
    ) -> Result<(Producer, PathBuf)> {
        let path = temp.path().join("state.json");
        fs::write(&path, patched(0.0, patch)?)?;
        let client = RouteServiceClient::new(server.base_url(), timeout)?;
        let mut producer =
            Producer::new(&path, settings(&temp.path().join("drop"))).with_route_service(client);
        producer.poll(now)?;
        fs::write(&path, patched(1.0, patch)?)?;
        producer.poll(now + Duration::from_millis(500))?;
        Ok((producer, path))
    }

    fn wait_for_requests(server: &TestServer, count: usize) -> bool {
        let started = Instant::now();
        while started.elapsed() < Duration::from_secs(5) {
            if server.requests().len() >= count {
                return true;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        false
    }

    fn poll_for_routes(
        producer: &mut Producer,
        now: Instant,
        expected_address: &str,
    ) -> Result<serde_json::Value> {
        let started = Instant::now();
        while started.elapsed() < Duration::from_secs(5) {
            producer.poll(now)?;
            let output: serde_json::Value =
                serde_json::from_slice(&fs::read(&producer.drop_path)?)?;
            if addresses(&output)
                .iter()
                .any(|item| item.starts_with(expected_address))
            {
                return Ok(output);
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        anyhow::bail!("timed out waiting for producer routes")
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
        // ADR 0041 §The Merged Routes item 8.
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
    fn refused_content_keeps_its_segment_and_episode_drop_data() -> Result<()> {
        let mut refused = remote(5_000, 10_000, "bad", "entry".into());
        refused["title"] = 5.into();
        let chapters = serde_json::json!([
            remote(0, 5_000, "f1", "i1".into()),
            refused,
            remote(10_000, 15_000, "f2", "i2".into()),
        ]);
        let out = chapter_drops(
            &chapters,
            &serde_json::json!({}),
            song_routes(&[("f1", "i1"), ("f2", "i2")]),
            &[(6.0, 0.0)],
        )?;
        assert_eq!(out[0]["feed_guid"], "feed");
        assert_eq!(out[0]["track_guid"], "track");
        assert_eq!(out[0]["title"], "Title");
        assert_eq!(out[0]["play_id"], "ls-12500-4-5000");
        assert_eq!(addresses(&out[0]), ["alice=95.0", "fee=5.0"]);
        Ok(())
    }

    #[test]
    fn episode_158_overlap_starts_a_segment_at_the_later_start() -> Result<()> {
        let chapters = serde_json::json!([
            remote(6_498_968, 6_729_868, "f1", "i1".into()),
            remote(6_724_607, 7_028_534, "f2", "i2".into()),
        ]);
        let out = chapter_drops(
            &chapters,
            &serde_json::json!({}),
            song_routes(&[("f1", "i1"), ("f2", "i2")]),
            &[(6_729.868, 0.0)],
        )?;
        assert_eq!(out[0]["play_id"], "ls-12500-4-6724607");
        assert_eq!(out[0]["track_guid"], "i2");
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
            // ADR 0041 §What The Producer Writes For An Episode File: a null title and a null artist give the state values.
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
                // ADR 0045 §The Segment Rule keeps the timed entry as a segment with no chapter.
                None => assert_eq!(out[0]["play_id"], "ls-12500-4-0", "{value}"),
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

    fn route_body(address: &str) -> String {
        serde_json::json!({"data":{"payment_routes":[{"address":address,"split":100}]}}).to_string()
    }

    /// A track answer with routes and one funding link. The producer sends no
    /// feed request for such an answer.
    fn route_body_with_funding(address: &str) -> String {
        serde_json::json!({"data":{
            "payment_routes":[{"address":address,"split":100}],
            "source_links":[{"link_type":"funding","url":"https://example.com/pay","position":1}]
        }})
        .to_string()
    }

    /// Polls with a fixed time until the server has `count` requests.
    fn poll_for_requests(
        producer: &mut Producer,
        server: &TestServer,
        now: Instant,
        count: usize,
    ) -> Result<()> {
        let started = Instant::now();
        while started.elapsed() < Duration::from_secs(5) {
            producer.poll(now)?;
            if server.requests().len() >= count {
                return Ok(());
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        anyhow::bail!("timed out waiting for {count} requests")
    }

    #[test]
    fn producer_requests_identified_chapters_in_start_order() -> Result<()> {
        let server = TestServer::start(|_| Reply::Answer {
            status: 200,
            body: route_body("service"),
        })?;
        let temp = TempDir::new()?;
        let chapters = serde_json::json!([
            remote(0, 10_000, "feed-b", "item-b".into()),
            remote(10_000, 20_000, "feed-a", "item-a".into()),
            remote(20_000, 30_000, "feed-c", serde_json::Value::Null),
        ]);
        let now = Instant::now();
        let (mut producer, _) = start_service_producer(
            &temp,
            &server,
            &chapter_patch(&chapters, 4),
            now,
            Duration::from_secs(1),
        )?;
        assert!(wait_for_requests(&server, 2));
        assert_eq!(
            server.requests(),
            [
                "/v1/feeds/feed-b/tracks/item-b?include=payment_routes,source_links",
                "/v1/feeds/feed-a/tracks/item-a?include=payment_routes,source_links",
            ]
        );
        producer.shutdown()?;
        Ok(())
    }

    #[test]
    fn producer_uses_service_routes_over_file_routes() -> Result<()> {
        let server = TestServer::start(|_| Reply::Answer {
            status: 200,
            body: route_body("service"),
        })?;
        let temp = TempDir::new()?;
        let mut entry = remote(0, 10_000, "feed", "item".into());
        entry["value_routes"] = serde_json::json!([{"address":"file","split":100}]);
        let chapters = serde_json::json!([entry]);
        let now = Instant::now();
        let (mut producer, _) = start_service_producer(
            &temp,
            &server,
            &chapter_patch(&chapters, 4),
            now,
            Duration::from_secs(1),
        )?;
        assert!(wait_for_requests(&server, 1));
        let output = poll_for_routes(&mut producer, now + Duration::from_secs(1), "service")?;
        assert!(
            addresses(&output)
                .iter()
                .all(|item| !item.starts_with("file="))
        );
        producer.shutdown()?;
        Ok(())
    }

    #[test]
    fn producer_applies_a_late_service_answer_without_changing_play_id() -> Result<()> {
        let server = TestServer::start(|_| {
            std::thread::sleep(Duration::from_millis(150));
            Reply::Answer {
                status: 200,
                body: route_body("service"),
            }
        })?;
        let temp = TempDir::new()?;
        let mut entry = remote(0, 10_000, "feed", "item".into());
        entry["value_routes"] = serde_json::json!([{"address":"file","split":100}]);
        let chapters = serde_json::json!([entry]);
        let now = Instant::now();
        let (mut producer, _) = start_service_producer(
            &temp,
            &server,
            &chapter_patch(&chapters, 4),
            now,
            Duration::from_secs(1),
        )?;
        assert!(wait_for_requests(&server, 1));
        let before = poll_for_routes(&mut producer, now + Duration::from_secs(1), "file")?;
        let after = poll_for_routes(&mut producer, now + Duration::from_secs(2), "service")?;
        assert_eq!(before["play_id"], after["play_id"]);
        producer.shutdown()?;
        Ok(())
    }

    #[test]
    fn producer_caches_no_routes_for_404_and_empty_results() -> Result<()> {
        for (status, body) in [(404, "{}"), (200, r#"{"data":{"payment_routes":[]}}"#)] {
            let server = TestServer::start(move |_| Reply::Answer {
                status,
                body: body.to_owned(),
            })?;
            let temp = TempDir::new()?;
            let mut entry = remote(0, 10_000, "feed", "item".into());
            entry["value_routes"] = serde_json::json!([{"address":"file","split":100}]);
            let chapters = serde_json::json!([entry]);
            let now = Instant::now();
            let (mut producer, path) = start_service_producer(
                &temp,
                &server,
                &chapter_patch(&chapters, 4),
                now,
                Duration::from_secs(1),
            )?;
            assert!(wait_for_requests(&server, 1));
            // The track answer has no funding link, so one feed request follows.
            poll_for_requests(&mut producer, &server, now + Duration::from_secs(1), 2)?;
            for step in 1..=20 {
                let elapsed = Duration::from_millis(600 * step);
                fs::write(
                    &path,
                    patched(1.0 + step as f64, &chapter_patch(&chapters, 4))?,
                )?;
                producer.poll(now + elapsed)?;
                std::thread::sleep(Duration::from_millis(5));
            }
            assert_eq!(
                server.requests(),
                [
                    "/v1/feeds/feed/tracks/item?include=payment_routes,source_links",
                    "/v1/feeds/feed?include=source_links",
                ]
            );
            let output: serde_json::Value =
                serde_json::from_slice(&fs::read(&producer.drop_path)?)?;
            assert!(
                addresses(&output)
                    .iter()
                    .any(|item| item.starts_with("file="))
            );
            producer.shutdown()?;
        }
        Ok(())
    }

    #[test]
    fn producer_retries_hang_after_thirty_seconds_and_keeps_play_id() -> Result<()> {
        let server = TestServer::start(|_| Reply::Hang(Duration::from_secs(2)))?;
        let temp = TempDir::new()?;
        let mut entry = remote(0, 10_000, "feed", "item".into());
        entry["value_routes"] = serde_json::json!([{"address":"file","split":100}]);
        let chapters = serde_json::json!([entry]);
        let now = Instant::now();
        let (mut producer, path) = start_service_producer(
            &temp,
            &server,
            &chapter_patch(&chapters, 4),
            now,
            Duration::from_millis(100),
        )?;
        assert!(wait_for_requests(&server, 1));
        let before = poll_for_routes(&mut producer, now + Duration::from_secs(1), "file")?;
        let play_id = before["play_id"].clone();

        let failed_at = Instant::now();
        while failed_at.elapsed() < Duration::from_millis(300) {
            producer.poll(now + Duration::from_secs(1))?;
            std::thread::sleep(Duration::from_millis(5));
        }
        fs::write(&path, patched(2.0, &chapter_patch(&chapters, 4))?)?;
        producer.poll(now + Duration::from_secs(29))?;
        producer.poll(now + Duration::from_secs(30))?;
        assert_eq!(server.requests().len(), 1);

        producer.poll(now + Duration::from_secs(31))?;
        assert!(wait_for_requests(&server, 2));
        let after = poll_for_routes(&mut producer, now + Duration::from_secs(32), "file")?;
        assert_eq!(after["play_id"], play_id);
        assert_eq!(server.requests().len(), 2);
        producer.shutdown()?;
        Ok(())
    }

    #[test]
    fn producer_retains_current_pair_results_and_reloads_evicted_pairs() -> Result<()> {
        let server = TestServer::start(|_| Reply::Answer {
            status: 200,
            body: route_body_with_funding("service"),
        })?;
        let temp = TempDir::new()?;
        let path = temp.path().join("state.json");
        let now = Instant::now();
        let chapters = serde_json::json!([
            remote(0, 10_000, "feed-a", "item-a".into()),
            remote(10_000, 20_000, "feed-b", "item-b".into()),
        ]);
        fs::write(&path, patched(0.0, &chapter_patch(&chapters, 1))?)?;
        let client = RouteServiceClient::new(server.base_url(), Duration::from_secs(1))?;
        let mut producer =
            Producer::new(&path, settings(&temp.path().join("drop"))).with_route_service(client);
        producer.poll(now)?;
        fs::write(&path, patched(1.0, &chapter_patch(&chapters, 1))?)?;
        producer.poll(now + Duration::from_millis(500))?;
        assert!(wait_for_requests(&server, 2));
        let settled = Instant::now();
        while producer.lookups.results().len() < 2 && settled.elapsed() < Duration::from_secs(5) {
            producer.poll(now + Duration::from_secs(1))?;
            std::thread::sleep(Duration::from_millis(5));
        }
        assert_eq!(producer.lookups.results().len(), 2);

        let only_a = serde_json::json!([remote(0, 10_000, "feed-a", "item-a".into())]);
        fs::write(&path, patched(2.0, &chapter_patch(&only_a, 2))?)?;
        producer.poll(now + Duration::from_secs(2))?;
        assert_eq!(producer.lookups.results().len(), 1);
        assert_eq!(server.requests().len(), 2);

        let only_b = serde_json::json!([remote(0, 10_000, "feed-b", "item-b".into())]);
        fs::write(&path, patched(3.0, &chapter_patch(&only_b, 3))?)?;
        producer.poll(now + Duration::from_secs(3))?;
        assert!(wait_for_requests(&server, 3));
        assert_eq!(server.requests().len(), 3);
        producer.shutdown()?;
        Ok(())
    }

    #[test]
    fn producer_skips_route_requests_for_jingles_live_one_guid_and_local_splits() -> Result<()> {
        let server = TestServer::start(|_| Reply::Answer {
            status: 200,
            body: route_body("unexpected"),
        })?;
        let chapters = serde_json::json!([remote(0, 10_000, "feed", "item".into())]);
        let now = Instant::now();
        for patch in [
            {
                let mut value = chapter_patch(&chapters, 4);
                value["jingle"] = true.into();
                value
            },
            serde_json::json!({"live": true}),
            serde_json::json!({"track_guid": ""}),
            {
                let local = serde_json::json!([{
                    "start_ms":0,"end_ms":10000,"remote_percentage":90,
                    "value_routes":[{"address":"local","split":100}]
                }]);
                chapter_patch(&local, 4)
            },
        ] {
            let temp = TempDir::new()?;
            let (mut producer, _) =
                start_service_producer(&temp, &server, &patch, now, Duration::from_secs(1))?;
            producer.shutdown()?;
        }
        assert!(server.requests().is_empty());
        Ok(())
    }

    // The song route tests of ADR 0041 §Verification.

    const FILE_ROUTES: &str = r#"[{"address":"file","split":100}]"#;

    fn song_patch(format: &str, routes: &str) -> serde_json::Value {
        let mut patch = serde_json::json!({"value_routes": routes, "seq": 4});
        if format == state::STATE_FORMAT_2 {
            patch["format"] = format.into();
            patch["chapters"] = "[]".into();
        }
        patch
    }

    #[test]
    fn a_song_state_asks_for_the_two_state_guids() -> Result<()> {
        for format in [state::STATE_FORMAT, state::STATE_FORMAT_2] {
            let server = TestServer::start(|_| Reply::Answer {
                status: 200,
                body: route_body("service"),
            })?;
            let temp = TempDir::new()?;
            let now = Instant::now();
            let (mut producer, _) = start_service_producer(
                &temp,
                &server,
                &song_patch(format, FILE_ROUTES),
                now,
                Duration::from_secs(1),
            )?;
            assert!(wait_for_requests(&server, 1), "{format}");
            assert_eq!(
                server.requests(),
                ["/v1/feeds/feed/tracks/track?include=payment_routes,source_links"],
                "{format}"
            );
            producer.shutdown()?;
        }
        Ok(())
    }

    #[test]
    fn a_song_state_pays_the_service_routes_over_the_state_routes() -> Result<()> {
        let server = TestServer::start(|_| Reply::Answer {
            status: 200,
            body: route_body("service"),
        })?;
        let temp = TempDir::new()?;
        let now = Instant::now();
        let (mut producer, _) = start_service_producer(
            &temp,
            &server,
            &song_patch(state::STATE_FORMAT, FILE_ROUTES),
            now,
            Duration::from_secs(1),
        )?;
        let output = poll_for_routes(&mut producer, now + Duration::from_secs(1), "service")?;
        assert_eq!(addresses(&output), ["service=100.0"]);
        assert_eq!(output["value_routes_source"], "liquidsoap-state");
        assert_eq!(output["feed_guid"], "feed");
        assert_eq!(output["track_guid"], "track");
        producer.shutdown()?;
        Ok(())
    }

    #[test]
    fn a_song_state_with_no_service_routes_pays_the_state_routes_or_nobody() -> Result<()> {
        for (status, body) in [(404, "{}"), (200, r#"{"data":{"payment_routes":[]}}"#)] {
            for (routes, expected) in [(FILE_ROUTES, vec!["file=100.0"]), ("", vec![])] {
                let server = TestServer::start(move |_| Reply::Answer {
                    status,
                    body: body.to_owned(),
                })?;
                let temp = TempDir::new()?;
                let now = Instant::now();
                let patch = song_patch(state::STATE_FORMAT, routes);
                let (mut producer, path) =
                    start_service_producer(&temp, &server, &patch, now, Duration::from_secs(1))?;
                assert!(wait_for_requests(&server, 1));
                // The track answer has no funding link, so one feed request follows.
                poll_for_requests(&mut producer, &server, now + Duration::from_secs(1), 2)?;
                for step in 1..=10_u32 {
                    fs::write(&path, patched(1.0 + f64::from(step), &patch)?)?;
                    producer.poll(now + Duration::from_millis(600 * u64::from(step)))?;
                    std::thread::sleep(Duration::from_millis(5));
                }
                assert_eq!(
                    server.requests(),
                    [
                        "/v1/feeds/feed/tracks/track?include=payment_routes,source_links",
                        "/v1/feeds/feed?include=source_links",
                    ]
                );
                let output: serde_json::Value =
                    serde_json::from_slice(&fs::read(&producer.drop_path)?)?;
                assert_eq!(addresses(&output), expected, "{status} {routes}");
                producer.shutdown()?;
            }
        }
        Ok(())
    }

    #[test]
    fn a_late_answer_for_a_song_keeps_the_play_id() -> Result<()> {
        let server = TestServer::start(|_| {
            std::thread::sleep(Duration::from_millis(150));
            Reply::Answer {
                status: 200,
                body: route_body("service"),
            }
        })?;
        let temp = TempDir::new()?;
        let now = Instant::now();
        let (mut producer, _) = start_service_producer(
            &temp,
            &server,
            &song_patch(state::STATE_FORMAT, FILE_ROUTES),
            now,
            Duration::from_secs(1),
        )?;
        let before = poll_for_routes(&mut producer, now + Duration::from_secs(1), "file")?;
        let after = poll_for_routes(&mut producer, now + Duration::from_secs(2), "service")?;
        assert_eq!(before["play_id"], after["play_id"]);
        producer.shutdown()?;
        Ok(())
    }

    #[test]
    fn an_episode_state_asks_only_for_its_chapter_songs() -> Result<()> {
        let server = TestServer::start(|_| Reply::Answer {
            status: 200,
            body: route_body("service"),
        })?;
        let temp = TempDir::new()?;
        let chapters = serde_json::json!([remote(0, 10_000, "song-feed", "song-item".into())]);
        let now = Instant::now();
        let (mut producer, _) = start_service_producer(
            &temp,
            &server,
            &chapter_patch(&chapters, 4),
            now,
            Duration::from_secs(1),
        )?;
        assert!(wait_for_requests(&server, 1));
        std::thread::sleep(Duration::from_millis(50));
        assert_eq!(
            server.requests(),
            ["/v1/feeds/song-feed/tracks/song-item?include=payment_routes,source_links"]
        );
        producer.shutdown()?;
        Ok(())
    }

    // The display state tests of ADR 0041 §Verification.

    fn display_settings(temp: &TempDir) -> InstanceSettings {
        InstanceSettings {
            display_dir: Some(temp.path().join("display")),
            ..settings(&temp.path().join("drop"))
        }
    }

    fn read_display(temp: &TempDir) -> Result<serde_json::Value> {
        let path = temp.path().join("display").join("display.json");
        Ok(serde_json::from_slice(&fs::read(path)?)?)
    }

    /// Gives the drop file and the display state after each `(elapsed,
    /// cue_in)` read of one fresh state with the fields of `patch`.
    fn display_reads(
        patch: &serde_json::Value,
        reads: &[(f64, f64)],
    ) -> Result<Vec<(serde_json::Value, serde_json::Value)>> {
        display_reads_with_picture(patch, reads, None, false)
    }

    fn display_reads_with_picture(
        patch: &serde_json::Value,
        reads: &[(f64, f64)],
        picture: Option<(&str, &[u8])>,
        remove_after_first: bool,
    ) -> Result<Vec<(serde_json::Value, serde_json::Value)>> {
        let temp = TempDir::new()?;
        if let Some((name, bytes)) = picture {
            fs::write(temp.path().join(name), bytes)?;
        }
        let path = temp.path().join("state.json");
        let mut producer = Producer::new(&path, display_settings(&temp));
        producer.start()?;
        let mut fields = patch.clone();
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
            let shown = read_display(&temp)?;
            if let Some(artwork) = shown["track"]["artwork"].as_object()
                && let (Some(sha), Some(mime)) = (
                    artwork.get("sha256").and_then(serde_json::Value::as_str),
                    artwork.get("mime").and_then(serde_json::Value::as_str),
                )
            {
                let extension = if mime == "image/png" { "png" } else { "jpg" };
                assert!(
                    temp.path()
                        .join("display")
                        .join(format!("{sha}.{extension}"))
                        .exists()
                );
            }
            outputs.push((
                serde_json::from_slice(&fs::read(&producer.drop_path)?)?,
                shown,
            ));
            if remove_after_first
                && step == 0
                && let Some((name, _)) = picture
            {
                fs::remove_file(temp.path().join(name))?;
            }
        }
        producer.shutdown()?;
        Ok(outputs)
    }

    fn three_chapters() -> serde_json::Value {
        serde_json::json!([
            remote(0, 10_000, "f1", "i1".into()),
            remote(10_000, 20_000, "f2", "i2".into()),
            remote(25_000, 30_000, "f3", "i3".into()),
        ])
    }

    const CHAPTER_READS: [(f64, f64); 5] = [
        (5.0, 0.0),
        (15.0, 0.0),
        (22.0, 0.0),
        (27.0, 0.0),
        (31.0, 0.0),
    ];

    fn chapter_state(extra: serde_json::Value) -> serde_json::Value {
        let mut fields = serde_json::json!({
            "format": state::STATE_FORMAT_2,
            "chapters": three_chapters().to_string(),
            "value_routes": EPISODE_ROUTES,
        });
        if let (Some(target), Some(extra)) = (fields.as_object_mut(), extra.as_object()) {
            for (key, value) in extra {
                target.insert(key.clone(), value.clone());
            }
        }
        fields
    }

    fn assert_track_of_drop(drop: &serde_json::Value, shown: &serde_json::Value, line: &str) {
        let track = &shown["track"];
        assert_eq!(shown["schema"], "musicindex.display/3");
        assert_eq!(track["artist"], drop["artist"]);
        assert_eq!(track["title"], drop["title"]);
        assert_eq!(track["album"], drop["album"]);
        assert_eq!(track["play_id"], drop["play_id"]);
        assert_eq!(track["song_line"], line);
    }

    #[test]
    fn display_chapter_segments_use_the_drop_file_and_the_file_song_line() -> Result<()> {
        let out = display_reads(&chapter_state(serde_json::json!({})), &CHAPTER_READS)?;
        let mut play_ids = Vec::new();
        for (drop, shown) in &out {
            assert_track_of_drop(drop, shown, "Artist - Title");
            play_ids.push(shown["track"]["play_id"].clone());
        }
        assert_eq!(out[0].1["track"]["artist"], "Artist 0");
        assert_eq!(out[0].1["track"]["title"], "Song 0");
        assert_eq!(out[2].1["track"]["title"], "Title");
        assert_eq!(play_ids.len(), 5);
        assert_eq!(play_ids[1], "ls-12500-4-10000");
        Ok(())
    }

    #[test]
    fn episode_date_is_written_for_each_segment_and_funding_is_null() -> Result<()> {
        let outputs = display_reads(
            &chapter_state(serde_json::json!({"pub_date": "2026-11-12"})),
            &CHAPTER_READS,
        )?;
        for (_, shown) in outputs {
            assert_eq!(shown["track"]["pub_date"], "2026-11-12");
            assert_eq!(shown["track"]["funding"], serde_json::Value::Null);
        }
        Ok(())
    }

    #[test]
    fn non_episode_states_and_invalid_dates_write_null() -> Result<()> {
        for (patch, reads) in [
            (serde_json::json!({}), vec![(1.0, 0.0)]),
            (serde_json::json!({"jingle": true}), vec![(1.0, 0.0)]),
            (
                serde_json::json!({"chapters": "[]", "pub_date": "2026-11-12"}),
                vec![(1.0, 0.0)],
            ),
        ] {
            for (_, shown) in display_reads(&patch, &reads)? {
                assert_eq!(shown["track"]["pub_date"], serde_json::Value::Null);
                assert_eq!(shown["track"]["funding"], serde_json::Value::Null);
            }
        }
        for date in ["", "2026-13-40", "2026-02-30", "12 Nov 2026"] {
            let outputs = display_reads(
                &chapter_state(serde_json::json!({"pub_date": date})),
                &CHAPTER_READS,
            )?;
            assert!(
                outputs
                    .iter()
                    .all(|(_, shown)| shown["track"]["pub_date"].is_null())
            );
        }
        let outputs = display_reads(
            &chapter_state(serde_json::json!({"pub_date": "2026-11-12T08:00:00"})),
            &CHAPTER_READS,
        )?;
        assert!(
            outputs
                .iter()
                .all(|(_, shown)| shown["track"]["pub_date"] == "2026-11-12")
        );
        Ok(())
    }

    #[test]
    fn a_chapter_with_a_title_has_the_episode_title_as_the_album() -> Result<()> {
        // ADR 0041 §The Album Of A Segment: reads 0, 1 and 3 are chapters, read 2 is a gap and read 4
        // is after the last chapter.
        let out = display_reads(&chapter_state(serde_json::json!({})), &CHAPTER_READS)?;
        let albums: Vec<_> = out.iter().map(|(drop, _)| drop["album"].clone()).collect();
        assert_eq!(albums, ["Title", "Title", "Album", "Title", "Album"]);
        for (drop, shown) in &out {
            assert_eq!(shown["track"]["album"], drop["album"]);
        }
        Ok(())
    }

    #[test]
    fn a_chapter_with_no_title_or_no_episode_title_has_the_state_album() -> Result<()> {
        // ADR 0041 §The Album Of A Segment: with no chapter title, the title line shows the episode
        // title, so the album line shows the state album.
        let mut untitled = remote(0, 10_000, "f1", "i1".into());
        untitled["title"] = "".into();
        let songs = song_routes(&[("f1", "i1")]);
        let out = chapter_drops(
            &serde_json::json!([untitled]),
            &serde_json::json!({}),
            songs,
            &[(5.0, 0.0)],
        )?;
        assert_eq!(out[0]["title"], "Title");
        assert_eq!(out[0]["album"], "Album");
        let songs = song_routes(&[("f1", "i1")]);
        let out = chapter_drops(
            &serde_json::json!([remote(0, 10_000, "f1", "i1".into())]),
            &serde_json::json!({"title": ""}),
            songs,
            &[(5.0, 0.0)],
        )?;
        assert_eq!(out[0]["title"], "Song 0");
        assert_eq!(out[0]["album"], "Album");
        Ok(())
    }

    /// The chapters of the image tests: a chapter with a valid image, the
    /// bad `image_url` values, and a local split with an image.
    fn image_chapters(bad: Option<&serde_json::Value>) -> serde_json::Value {
        let mut good = remote(0, 10_000, "f1", "i1".into());
        good["image_url"] = "https://song.example/one.jpg".into();
        good["unknown_field"] = 1.into();
        let mut refused = remote(10_000, 20_000, "f2", "i2".into());
        if let Some(bad) = bad {
            refused["image_url"] = bad.clone();
        }
        let local = serde_json::json!({
            "start_ms": 25_000, "end_ms": 30_000, "remote_percentage": 0,
            "image_url": "https://song.example/local.jpg",
            "value_routes": [{"address": "local-a", "split": 1}]
        });
        serde_json::json!([good, refused, local])
    }

    #[test]
    fn a_chapter_image_wins_over_the_state_image_and_the_picture() -> Result<()> {
        let picture = encoded_picture(image::ImageFormat::Jpeg)?;
        let state_url = "https://episode.example/cover.jpg";
        let long_url = format!("https://song.example/{}", "a".repeat(2_028));
        assert_eq!(long_url.chars().count(), 2_049);
        for bad in [
            None,
            Some(serde_json::Value::Null),
            Some(serde_json::json!(7)),
            Some(serde_json::json!("ftp://song.example/two.jpg")),
            Some(serde_json::json!(long_url)),
        ] {
            let patch = chapter_state(serde_json::json!({
                "chapters": image_chapters(bad.as_ref()).to_string(),
                "image": state_url,
                "picture": "picture-4",
            }));
            let out = display_reads_with_picture(
                &patch,
                &[(5.0, 0.0), (15.0, 0.0), (27.0, 0.0)],
                Some(("picture-4", &picture)),
                false,
            )?;
            // The chapter with a valid image_url, also with an unknown field.
            assert_eq!(out[0].0["image"], "https://song.example/one.jpg");
            assert_eq!(
                out[0].1["track"]["artwork"]["url"],
                "https://song.example/one.jpg"
            );
            assert_eq!(out[0].0["feed_guid"], "f1");
            // A bad image_url gives the state image and keeps the entry.
            assert_eq!(out[1].0["image"], state_url, "{bad:?}");
            assert_eq!(out[1].1["track"]["artwork"]["url"], state_url, "{bad:?}");
            assert_eq!(out[1].0["feed_guid"], "f2", "{bad:?}");
            // A local split does not use image_url.
            assert_eq!(out[2].0["image"], state_url);
            assert_eq!(out[2].1["track"]["artwork"]["url"], state_url);
        }
        Ok(())
    }

    #[test]
    fn a_bad_chapter_image_with_no_state_image_uses_the_picture() -> Result<()> {
        let picture = encoded_picture(image::ImageFormat::Jpeg)?;
        let chapters = image_chapters(Some(&"ftp://song.example/two.jpg".into()));
        let patch = chapter_state(serde_json::json!({
            "chapters": chapters.to_string(),
            "image": "",
            "picture": "picture-4",
        }));
        let out = display_reads_with_picture(
            &patch,
            &[(5.0, 0.0), (15.0, 0.0)],
            Some(("picture-4", &picture)),
            false,
        )?;
        assert_eq!(
            out[0].1["track"]["artwork"]["url"],
            "https://song.example/one.jpg"
        );
        assert!(out[1].0["image"].is_null());
        assert_eq!(out[1].1["track"]["artwork"]["mime"], "image/jpeg");
        Ok(())
    }

    #[test]
    fn display_song_field_is_the_song_line_for_each_segment() -> Result<()> {
        let patch = chapter_state(serde_json::json!({"song": "ICY Title"}));
        for (drop, shown) in display_reads(&patch, &CHAPTER_READS)? {
            assert_track_of_drop(&drop, &shown, "ICY Title");
        }
        Ok(())
    }

    #[test]
    fn display_song_line_falls_back_to_artist_then_title_then_null() -> Result<()> {
        let only_artist = serde_json::json!({"title": ""});
        let out = display_reads(&only_artist, &[(1.0, 0.0)])?;
        assert_eq!(out[0].1["track"]["song_line"], "Artist");
        let only_title = serde_json::json!({"artist": ""});
        let out = display_reads(&only_title, &[(1.0, 0.0)])?;
        assert_eq!(out[0].1["track"]["song_line"], "Title");
        let neither = serde_json::json!({"artist": "", "title": ""});
        let out = display_reads(&neither, &[(1.0, 0.0)])?;
        assert_eq!(out[0].1["track"], serde_json::Value::Null);
        Ok(())
    }

    #[test]
    fn display_song_file_without_chapters_gives_one_state() -> Result<()> {
        let out = display_reads(&serde_json::json!({}), &[(1.0, 0.0), (2.0, 0.0)])?;
        for (drop, shown) in &out {
            assert_track_of_drop(drop, shown, "Artist - Title");
        }
        assert_eq!(out[0].1, out[1].1);
        assert_eq!(out[0].1["track"]["play_id"], "ls-12500-4");
        assert_eq!(out[0].1["track"]["album"], "Album");
        assert_eq!(out[0].0["album"], "Album");
        Ok(())
    }

    #[test]
    fn display_jingle_has_the_state_names_and_no_play_id() -> Result<()> {
        let out = display_reads(&serde_json::json!({"jingle": true}), &[(1.0, 0.0)])?;
        let track = &out[0].1["track"];
        assert_eq!(track["artist"], "Artist");
        assert_eq!(track["title"], "Title");
        assert_eq!(track["song_line"], "Artist - Title");
        assert!(track["play_id"].is_null());
        assert_eq!(track["album"], "Album");
        Ok(())
    }

    #[test]
    fn display_live_state_gives_null() -> Result<()> {
        let temp = TempDir::new()?;
        let path = temp.path().join("state.json");
        let mut producer = Producer::new(&path, display_settings(&temp));
        producer.start()?;
        let now = Instant::now();
        fs::write(&path, state(0.0, false, ""))?;
        producer.poll(now)?;
        fs::write(&path, state(1.0, false, ""))?;
        producer.poll(now + Duration::from_millis(500))?;
        assert!(!read_display(&temp)?["track"].is_null());
        fs::write(&path, state(2.0, true, ""))?;
        producer.poll(now + Duration::from_secs(1))?;
        assert_eq!(read_display(&temp)?["track"], serde_json::Value::Null);
        producer.shutdown()?;
        Ok(())
    }

    #[test]
    fn a_target_without_display_dir_writes_no_display_file() -> Result<()> {
        let temp = TempDir::new()?;
        let path = temp.path().join("state.json");
        let mut producer = Producer::new(&path, settings(&temp.path().join("drop")));
        producer.start()?;
        let now = Instant::now();
        fs::write(&path, state(0.0, false, ""))?;
        producer.poll(now)?;
        fs::write(&path, state(1.0, false, ""))?;
        producer.poll(now + Duration::from_millis(500))?;
        producer.shutdown()?;
        assert!(producer.drop_path.parent().is_some_and(Path::exists));
        assert!(!temp.path().join("display").exists());
        assert!(!temp.path().join("drop").join("display.json").exists());
        Ok(())
    }

    #[test]
    fn a_format_2_state_without_song_parses_and_gives_artist_title() -> Result<()> {
        let mut value: serde_json::Value = serde_json::from_str(&state(0.0, false, ""))?;
        value["format"] = state::STATE_FORMAT_2.into();
        assert!(value.get("song").is_none());
        let parsed: StateFile = serde_json::from_value(value)?;
        assert_eq!(parsed.song, "");
        assert_eq!(song_line(&parsed).as_deref(), Some("Artist - Title"));
        let out = display_reads(
            &serde_json::json!({"format": state::STATE_FORMAT_2}),
            &[(1.0, 0.0)],
        )?;
        assert_eq!(out[0].1["track"]["song_line"], "Artist - Title");
        Ok(())
    }

    #[test]
    fn display_song_line_limit_is_1024_characters() -> Result<()> {
        let long = "a".repeat(1_025);
        let out = display_reads(&serde_json::json!({"song": long}), &[(1.0, 0.0)])?;
        assert_eq!(out[0].1["track"], serde_json::Value::Null);
        let limit = "é".repeat(1_024);
        let out = display_reads(&serde_json::json!({"song": limit.clone()}), &[(1.0, 0.0)])?;
        assert_eq!(out[0].1["track"]["song_line"], limit.as_str());
        Ok(())
    }

    #[test]
    fn display_artwork_is_a_http_url_or_null() -> Result<()> {
        for image in [
            "image",
            "ftp://example.test/a.png",
            "data:image/png;base64,AA",
            "",
        ] {
            let out = display_reads(&serde_json::json!({"image": image}), &[(1.0, 0.0)])?;
            assert!(out[0].1["track"]["artwork"].is_null(), "{image}");
        }
        let url = "https://example.test/a.png";
        let out = display_reads(&serde_json::json!({"image": url}), &[(1.0, 0.0)])?;
        assert_eq!(
            out[0].1["track"]["artwork"],
            serde_json::json!({"url": url})
        );
        Ok(())
    }

    fn encoded_picture(format: image::ImageFormat) -> Result<Vec<u8>> {
        let image = image::RgbImage::from_pixel(12, 9, image::Rgb([20, 40, 60]));
        let mut bytes = std::io::Cursor::new(Vec::new());
        image::DynamicImage::ImageRgb8(image).write_to(&mut bytes, format)?;
        Ok(bytes.into_inner())
    }

    #[test]
    fn valid_image_url_wins_over_picture_file() -> Result<()> {
        let picture = encoded_picture(image::ImageFormat::Jpeg)?;
        let url = "https://example.test/a.png";
        let out = display_reads_with_picture(
            &serde_json::json!({"image": url, "picture": "picture-4"}),
            &[(1.0, 0.0)],
            Some(("picture-4", &picture)),
            false,
        )?;
        assert_eq!(
            out[0].1["track"]["artwork"],
            serde_json::json!({"url": url})
        );
        Ok(())
    }

    #[test]
    fn empty_image_uses_jpeg_picture_and_writes_display_image() -> Result<()> {
        let picture = encoded_picture(image::ImageFormat::Jpeg)?;
        let out = display_reads_with_picture(
            &serde_json::json!({"image": "", "picture": "picture-4"}),
            &[(1.0, 0.0)],
            Some(("picture-4", &picture)),
            false,
        )?;
        assert_eq!(
            out[0].1["track"]["artwork"]["sha256"],
            now_playing_drop::display::sha256_hex(&picture)
        );
        assert_eq!(out[0].1["track"]["artwork"]["mime"], "image/jpeg");
        Ok(())
    }

    #[test]
    fn refused_image_url_uses_png_picture() -> Result<()> {
        let picture = encoded_picture(image::ImageFormat::Png)?;
        let out = display_reads_with_picture(
            &serde_json::json!({"image": "ftp://host/a.png", "picture": "picture-4"}),
            &[(1.0, 0.0)],
            Some(("picture-4", &picture)),
            false,
        )?;
        assert_eq!(out[0].1["track"]["artwork"]["mime"], "image/png");
        // ADR 0041 §The Artwork Of A Segment: a refused URL is not in the
        // drop file, so the publisher can use the relay copy (ADR 0034).
        assert!(out[0].0["image"].is_null());
        Ok(())
    }

    #[test]
    fn no_picture_missing_file_and_gif_give_null_artwork() -> Result<()> {
        let gif = b"GIF89a\x01\x00\x01\x00\x80\x00\x00\x00\x00\x00\xff\xff\xff!\xf9\x04\x01\x00\x00\x00\x00,\x00\x00\x00\x00\x01\x00\x01\x00\x00\x02\x02D\x01\x00;";
        for (patch, file) in [
            (serde_json::json!({}), None),
            (serde_json::json!({"picture": "picture-4"}), None),
            (
                serde_json::json!({"picture": "picture-4"}),
                Some(("picture-4", gif.as_slice())),
            ),
        ] {
            let out = display_reads_with_picture(&patch, &[(1.0, 0.0)], file, false)?;
            assert!(out[0].1["track"]["artwork"].is_null());
        }
        Ok(())
    }

    #[test]
    fn invalid_picture_names_give_null_without_reading() -> Result<()> {
        for name in ["../state.json", "picture-", "picture-12a"] {
            let out = display_reads_with_picture(
                &serde_json::json!({"picture": name}),
                &[(1.0, 0.0)],
                None,
                false,
            )?;
            assert!(out[0].1["track"]["artwork"].is_null(), "{name}");
        }
        Ok(())
    }

    #[test]
    fn picture_over_64_mib_gives_null_artwork() -> Result<()> {
        use std::fs::File;

        let temp = TempDir::new()?;
        let state_path = temp.path().join("state.json");
        File::create(temp.path().join("picture-4"))?.set_len(MAX_PICTURE_BYTES as u64 + 1)?;
        let mut producer = Producer::new(&state_path, display_settings(&temp));
        producer.start()?;
        let parsed: StateFile =
            serde_json::from_str(&patched(1.0, &serde_json::json!({"picture": "picture-4"}))?)?;
        producer.write_display_track(
            &parsed,
            &chapters::ChapterList::default(),
            &serde_json::json!({}),
            &[],
        );
        assert!(read_display(&temp)?["track"]["artwork"].is_null());
        producer.shutdown()?;
        Ok(())
    }

    #[test]
    fn chapter_segments_keep_picture_after_slot_removes_file() -> Result<()> {
        let picture = encoded_picture(image::ImageFormat::Jpeg)?;
        let patch = chapter_state(serde_json::json!({"picture": "picture-4"}));
        let out = display_reads_with_picture(
            &patch,
            &CHAPTER_READS,
            Some(("picture-4", &picture)),
            true,
        )?;
        let first = out[0].1["track"]["artwork"].clone();
        assert!(first.is_object());
        for (_, shown) in &out[1..] {
            assert_eq!(shown["track"]["artwork"], first);
        }
        Ok(())
    }

    #[test]
    fn jingle_uses_the_picture_of_the_last_song() -> Result<()> {
        let picture = encoded_picture(image::ImageFormat::Jpeg)?;
        let out = display_reads_with_picture(
            &serde_json::json!({"jingle": true, "picture": "picture-4"}),
            &[(1.0, 0.0)],
            Some(("picture-4", &picture)),
            false,
        )?;
        assert_eq!(
            out[0].1["track"]["artwork"]["sha256"],
            now_playing_drop::display::sha256_hex(&picture)
        );
        Ok(())
    }

    #[test]
    fn restart_with_same_seq_reads_the_new_picture() -> Result<()> {
        let temp = TempDir::new()?;
        let state_path = temp.path().join("state.json");
        let mut producer = Producer::new(&state_path, display_settings(&temp));
        producer.start()?;
        let mut shown = Vec::new();
        for (started, format) in [
            (100.0, image::ImageFormat::Jpeg),
            (200.0, image::ImageFormat::Png),
        ] {
            let picture = encoded_picture(format)?;
            fs::write(temp.path().join("picture-4"), &picture)?;
            let fields = serde_json::json!({"picture": "picture-4", "track_started": started});
            let parsed: StateFile = serde_json::from_str(&patched(1.0, &fields)?)?;
            producer.write_display_track(
                &parsed,
                &chapters::ChapterList::default(),
                &serde_json::json!({}),
                &[],
            );
            shown.push((
                read_display(&temp)?["track"]["artwork"]["sha256"].clone(),
                now_playing_drop::display::sha256_hex(&picture),
            ));
        }
        for (artwork, expected) in shown {
            assert_eq!(artwork, expected);
        }
        producer.shutdown()?;
        Ok(())
    }

    #[test]
    fn format_2_state_without_picture_parses() -> Result<()> {
        let mut value: serde_json::Value = serde_json::from_str(&state(0.0, false, ""))?;
        value["format"] = state::STATE_FORMAT_2.into();
        value
            .as_object_mut()
            .context("state object")?
            .remove("picture");
        let parsed: StateFile = serde_json::from_value(value)?;
        assert_eq!(parsed.picture, "");
        assert_eq!(parsed.pub_date, None);
        Ok(())
    }

    #[test]
    fn display_start_and_shutdown_write_null() -> Result<()> {
        let temp = TempDir::new()?;
        let path = temp.path().join("state.json");
        let mut producer = Producer::new(&path, display_settings(&temp));
        assert!(!temp.path().join("display").exists());
        producer.start()?;
        assert_eq!(read_display(&temp)?["track"], serde_json::Value::Null);
        let now = Instant::now();
        fs::write(&path, state(0.0, false, ""))?;
        producer.poll(now)?;
        fs::write(&path, state(1.0, false, ""))?;
        producer.poll(now + Duration::from_millis(500))?;
        assert!(!read_display(&temp)?["track"].is_null());
        producer.shutdown()?;
        assert_eq!(read_display(&temp)?["track"], serde_json::Value::Null);
        Ok(())
    }

    #[test]
    fn display_keeps_the_track_when_a_fresh_state_does_not_parse() -> Result<()> {
        let temp = TempDir::new()?;
        let path = temp.path().join("state.json");
        let mut producer = Producer::new(&path, display_settings(&temp));
        producer.start()?;
        let now = Instant::now();
        fs::write(&path, state(0.0, false, ""))?;
        producer.poll(now)?;
        fs::write(&path, state(1.0, false, ""))?;
        producer.poll(now + Duration::from_millis(500))?;
        let before = read_display(&temp)?;
        assert!(!before["track"].is_null());
        fs::write(&path, b"{")?;
        producer.poll(now + Duration::from_secs(2))?;
        assert_eq!(read_display(&temp)?, before);
        producer.shutdown()?;
        Ok(())
    }

    // The funding tests of ADR 0044 §Verification.

    const WPAY: &str = "https://file.example/pay";
    const TRACK_REQUEST: &str = "/v1/feeds/feed/tracks/track?include=payment_routes,source_links";
    const FEED_REQUEST: &str = "/v1/feeds/feed?include=source_links";

    fn funding_entry(url: &str, position: i64, label: Option<&str>) -> serde_json::Value {
        let mut entry =
            serde_json::json!({"link_type": "funding", "url": url, "position": position});
        if let Some(label) = label {
            entry["label"] = label.into();
        }
        entry
    }

    fn answer(status: u16, body: &serde_json::Value) -> Reply {
        Reply::Answer {
            status,
            body: body.to_string(),
        }
    }

    /// An answer that has `source_links` and no routes.
    fn links_reply(links: &serde_json::Value) -> Reply {
        answer(200, &serde_json::json!({"data": {"source_links": links}}))
    }

    /// An answer that has no funding link.
    fn no_links_reply() -> Reply {
        answer(200, &serde_json::json!({"data": {}}))
    }

    /// A server that gives `track` for a track request and `feed` for a feed
    /// request.
    fn funding_server(track: Reply, feed: Reply) -> Result<TestServer> {
        TestServer::start(move |target| {
            if target.contains("/tracks/") {
                track.clone()
            } else {
                feed.clone()
            }
        })
    }

    /// Starts a producer with a display folder and a route service. The two
    /// polls make the state fresh.
    fn funding_producer(
        temp: &TempDir,
        server: &TestServer,
        patch: &serde_json::Value,
        now: Instant,
    ) -> Result<(Producer, PathBuf)> {
        let path = temp.path().join("state.json");
        fs::write(&path, patched(0.0, patch)?)?;
        let client = RouteServiceClient::new(server.base_url(), Duration::from_secs(1))?;
        let mut producer = Producer::new(&path, display_settings(temp)).with_route_service(client);
        producer.start()?;
        producer.poll(now)?;
        fs::write(&path, patched(1.0, patch)?)?;
        producer.poll(now + Duration::from_millis(500))?;
        Ok((producer, path))
    }

    /// Reads the state at `elapsed` seconds, with the fields of `patch`.
    fn read_at(
        producer: &mut Producer,
        path: &Path,
        patch: &serde_json::Value,
        written: f64,
        elapsed: f64,
        now: Instant,
    ) -> Result<()> {
        let mut fields = patch.clone();
        fields["elapsed"] = elapsed.into();
        fields["cue_in"] = 0.0.into();
        fs::write(path, patched(written, &fields)?)?;
        producer.poll(now)
    }

    fn shown_funding(temp: &TempDir) -> Result<serde_json::Value> {
        Ok(read_display(temp)?["track"]["funding"].clone())
    }

    /// Polls until `display.json` has `expected` as `funding`. It fails after
    /// five seconds.
    fn poll_for_funding(
        producer: &mut Producer,
        temp: &TempDir,
        now: Instant,
        expected: &serde_json::Value,
    ) -> Result<()> {
        let started = Instant::now();
        loop {
            producer.poll(now)?;
            let shown = shown_funding(temp)?;
            if shown == *expected {
                return Ok(());
            }
            if started.elapsed() >= Duration::from_secs(5) {
                anyhow::bail!("funding is {shown} and not {expected}");
            }
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    /// Polls for a short time. The `funding` value must not change.
    fn assert_funding_stays(
        producer: &mut Producer,
        temp: &TempDir,
        now: Instant,
        expected: &serde_json::Value,
    ) -> Result<()> {
        for _ in 0..40 {
            producer.poll(now)?;
            assert_eq!(shown_funding(temp)?, *expected);
            std::thread::sleep(Duration::from_millis(5));
        }
        Ok(())
    }

    fn wpay_json() -> serde_json::Value {
        serde_json::json!([{"url": WPAY}])
    }

    #[test]
    fn item_funding_links_come_in_position_order_and_beat_the_channel_links() -> Result<()> {
        let server = funding_server(
            links_reply(&serde_json::json!([
                funding_entry("https://two.example", 2, Some("Two")),
                funding_entry("https://one.example", 1, None),
            ])),
            links_reply(&serde_json::json!([funding_entry(
                "https://channel.example",
                1,
                None
            )])),
        )?;
        let temp = TempDir::new()?;
        let now = Instant::now();
        let (mut producer, _) =
            funding_producer(&temp, &server, &serde_json::json!({"seq": 4}), now)?;
        let expected = serde_json::json!([
            {"url": "https://one.example"},
            {"url": "https://two.example", "label": "Two"},
        ]);
        poll_for_funding(
            &mut producer,
            &temp,
            now + Duration::from_secs(1),
            &expected,
        )?;
        assert_funding_stays(
            &mut producer,
            &temp,
            now + Duration::from_secs(1),
            &expected,
        )?;
        // The item links are usable, so the producer sends no feed request.
        assert_eq!(server.requests(), [TRACK_REQUEST]);
        producer.shutdown()?;
        Ok(())
    }

    #[test]
    fn a_track_answer_with_no_funding_link_causes_the_feed_request() -> Result<()> {
        let server = funding_server(
            no_links_reply(),
            links_reply(&serde_json::json!([funding_entry(
                "https://channel.example",
                1,
                Some("Channel")
            )])),
        )?;
        let temp = TempDir::new()?;
        let now = Instant::now();
        let (mut producer, _) =
            funding_producer(&temp, &server, &serde_json::json!({"seq": 4}), now)?;
        let expected = serde_json::json!([{"url": "https://channel.example", "label": "Channel"}]);
        poll_for_funding(
            &mut producer,
            &temp,
            now + Duration::from_secs(1),
            &expected,
        )?;
        assert_eq!(server.requests(), [TRACK_REQUEST, FEED_REQUEST]);
        producer.shutdown()?;
        Ok(())
    }

    #[test]
    fn funding_links_follow_the_url_label_duplicate_and_count_rules() -> Result<()> {
        let long_url = format!("https://{}", "a".repeat(505));
        assert_eq!(long_url.chars().count(), 513);
        let server = funding_server(
            links_reply(&serde_json::json!([
                funding_entry("ftp://x", 1, None),
                funding_entry(&long_url, 2, None),
                funding_entry("https://example.com/a b", 3, None),
                funding_entry("https://example.com/a\u{1}b", 4, None),
                funding_entry("https:///support", 5, None),
                funding_entry("HTTPS://example.com", 6, Some(&"x".repeat(200))),
                funding_entry("HTTPS://example.com", 7, Some("Duplicate")),
                funding_entry("https://b.example", 8, Some("")),
                funding_entry("https://c.example", 9, Some("C")),
                funding_entry("https://d.example", 10, None),
            ])),
            no_links_reply(),
        )?;
        let temp = TempDir::new()?;
        let now = Instant::now();
        let (mut producer, _) =
            funding_producer(&temp, &server, &serde_json::json!({"seq": 4}), now)?;
        let expected = serde_json::json!([
            {"url": "HTTPS://example.com", "label": "x".repeat(128)},
            {"url": "https://b.example"},
            {"url": "https://c.example", "label": "C"},
        ]);
        poll_for_funding(
            &mut producer,
            &temp,
            now + Duration::from_secs(1),
            &expected,
        )?;
        // An empty label gives no key.
        assert!(shown_funding(&temp)?[1].get("label").is_none());
        producer.shutdown()?;
        Ok(())
    }

    #[test]
    fn a_song_state_with_fresh_links_and_a_wpay_gives_the_fresh_links() -> Result<()> {
        let server = funding_server(
            links_reply(&serde_json::json!([funding_entry(
                "https://fresh.example",
                1,
                None
            )])),
            no_links_reply(),
        )?;
        let temp = TempDir::new()?;
        let now = Instant::now();
        let patch = serde_json::json!({"seq": 4, "wpay": WPAY});
        let (mut producer, _) = funding_producer(&temp, &server, &patch, now)?;
        let expected = serde_json::json!([{"url": "https://fresh.example"}]);
        poll_for_funding(
            &mut producer,
            &temp,
            now + Duration::from_secs(1),
            &expected,
        )?;
        assert_funding_stays(
            &mut producer,
            &temp,
            now + Duration::from_secs(1),
            &expected,
        )?;
        producer.shutdown()?;
        Ok(())
    }

    #[test]
    fn a_song_state_uses_the_wpay_with_no_label_when_no_fresh_link_exists() -> Result<()> {
        let not_found = || answer(404, &serde_json::json!({}));
        let failed = || answer(500, &serde_json::json!({}));
        // The NUL character at the end is part of the value of the state.
        let wpay = format!("{WPAY}\0");
        for (name, track, feed, patch, requests) in [
            (
                "404",
                not_found(),
                not_found(),
                serde_json::json!({"seq": 4, "wpay": wpay}),
                vec![TRACK_REQUEST, FEED_REQUEST],
            ),
            (
                "failed request",
                failed(),
                failed(),
                serde_json::json!({"seq": 4, "wpay": wpay}),
                vec![TRACK_REQUEST],
            ),
            (
                "missing GUID",
                no_links_reply(),
                no_links_reply(),
                serde_json::json!({"seq": 4, "wpay": wpay, "feed_guid": ""}),
                vec![],
            ),
        ] {
            let server = funding_server(track, feed)?;
            let temp = TempDir::new()?;
            let now = Instant::now();
            let (mut producer, _) = funding_producer(&temp, &server, &patch, now)?;
            let later = now + Duration::from_secs(1);
            if !requests.is_empty() {
                poll_for_requests(&mut producer, &server, later, requests.len())?;
            }
            assert_funding_stays(&mut producer, &temp, later, &wpay_json())?;
            assert_eq!(server.requests(), requests, "{name}");
            producer.shutdown()?;
        }
        Ok(())
    }

    #[test]
    fn timed_entries_with_invalid_content_keep_song_lookup_and_wpay_behavior() -> Result<()> {
        let not_found = || answer(404, &serde_json::json!({}));
        let server = funding_server(not_found(), not_found())?;
        let temp = TempDir::new()?;
        let now = Instant::now();
        let patch = serde_json::json!({
            "format": state::STATE_FORMAT_2,
            "seq": 4,
            "chapters": "[{\"start_ms\":0,\"end_ms\":10000,\"title\":5}]",
            "elapsed": 1.0,
            "cue_in": 0.0,
            "wpay": WPAY,
        });
        let (mut producer, _) = funding_producer(&temp, &server, &patch, now)?;
        poll_for_requests(&mut producer, &server, now + Duration::from_secs(1), 2)?;
        assert_eq!(server.requests(), [TRACK_REQUEST, FEED_REQUEST]);
        let drop: serde_json::Value = serde_json::from_slice(&fs::read(&producer.drop_path)?)?;
        assert_eq!(drop["play_id"], "ls-12500-4-0");
        assert_eq!(shown_funding(&temp)?, wpay_json());
        producer.shutdown()?;
        Ok(())
    }

    #[test]
    fn a_late_answer_replaces_the_wpay_and_keeps_the_play_id_and_the_drop_file() -> Result<()> {
        let server = TestServer::start(|_| {
            std::thread::sleep(Duration::from_millis(200));
            links_reply(&serde_json::json!([funding_entry(
                "https://fresh.example",
                1,
                None
            )]))
        })?;
        let temp = TempDir::new()?;
        let now = Instant::now();
        let patch = serde_json::json!({"seq": 4, "wpay": WPAY});
        let (mut producer, _) = funding_producer(&temp, &server, &patch, now)?;
        // No answer has come yet.
        assert_eq!(shown_funding(&temp)?, wpay_json());
        let play_id = read_display(&temp)?["track"]["play_id"].clone();
        assert!(play_id.is_string());
        let drop_before = fs::read(&producer.drop_path)?;
        let expected = serde_json::json!([{"url": "https://fresh.example"}]);
        poll_for_funding(
            &mut producer,
            &temp,
            now + Duration::from_secs(1),
            &expected,
        )?;
        assert_eq!(read_display(&temp)?["track"]["play_id"], play_id);
        assert_eq!(fs::read(&producer.drop_path)?, drop_before);
        producer.shutdown()?;
        Ok(())
    }

    #[test]
    fn a_wpay_loses_the_nul_at_the_end_and_a_refused_wpay_gives_null() -> Result<()> {
        for (wpay, expected) in [
            (format!("{WPAY}\0"), wpay_json()),
            ("ftp://x".to_owned(), serde_json::Value::Null),
            ("\0".to_owned(), serde_json::Value::Null),
        ] {
            let out = display_reads(&serde_json::json!({"wpay": wpay}), &[(1.0, 0.0)])?;
            assert_eq!(out[0].1["track"]["funding"], expected, "{wpay:?}");
        }
        Ok(())
    }

    fn episode_chapters() -> serde_json::Value {
        serde_json::json!([
            {
                "start_ms": 0, "end_ms": 10_000, "remote_percentage": 90,
                "value_routes": [{"address": "local", "split": 100}]
            },
            remote(15_000, 20_000, "feed-u", serde_json::Value::Null),
            remote(25_000, 30_000, "f3", "i3".into()),
        ])
    }

    #[test]
    fn the_segments_that_are_not_identified_remote_splits_give_the_state_wpay() -> Result<()> {
        let server = funding_server(
            links_reply(&serde_json::json!([funding_entry(
                "https://fresh.example",
                1,
                None
            )])),
            no_links_reply(),
        )?;
        let temp = TempDir::new()?;
        let now = Instant::now();
        let later = now + Duration::from_secs(1);
        let mut patch = chapter_patch(&episode_chapters(), 4);
        patch["wpay"] = WPAY.into();
        let (mut producer, path) = funding_producer(&temp, &server, &patch, now)?;
        // The identified remote split shows its fresh links. This also shows
        // that an answer is in the cache for the other reads.
        read_at(&mut producer, &path, &patch, 2.0, 27.0, later)?;
        let fresh = serde_json::json!([{"url": "https://fresh.example"}]);
        poll_for_funding(&mut producer, &temp, later, &fresh)?;
        // A local split, a gap, a remote split that is not identified, and
        // the part after the last chapter.
        for (step, elapsed) in [5.0, 12.0, 17.0, 35.0].into_iter().enumerate() {
            read_at(
                &mut producer,
                &path,
                &patch,
                3.0 + step as f64,
                elapsed,
                later,
            )?;
            assert_eq!(shown_funding(&temp)?, wpay_json(), "elapsed {elapsed}");
        }
        // The producer asks only for the identified chapter. It sends no
        // request for the state pair.
        let requests = server.requests();
        assert!(!requests.is_empty());
        assert!(
            requests
                .iter()
                .all(|request| request.starts_with("/v1/feeds/f3/tracks/i3")),
            "{requests:?}"
        );
        producer.shutdown()?;
        Ok(())
    }

    #[test]
    fn an_identified_remote_split_never_uses_the_wpay() -> Result<()> {
        let chapters = serde_json::json!([remote(0, 10_000, "feed", "track".into())]);
        let mut patch = chapter_patch(&chapters, 4);
        patch["wpay"] = WPAY.into();

        // No fresh link: `funding` is `null`, also with a `wpay`.
        let not_found = || answer(404, &serde_json::json!({}));
        let server = funding_server(not_found(), not_found())?;
        let temp = TempDir::new()?;
        let now = Instant::now();
        let later = now + Duration::from_secs(1);
        let (mut producer, _) = funding_producer(&temp, &server, &patch, now)?;
        assert_eq!(shown_funding(&temp)?, serde_json::Value::Null);
        poll_for_requests(&mut producer, &server, later, 2)?;
        assert_funding_stays(&mut producer, &temp, later, &serde_json::Value::Null)?;
        producer.shutdown()?;

        // Fresh links: `funding` has the fresh links.
        let server = funding_server(
            links_reply(&serde_json::json!([funding_entry(
                "https://fresh.example",
                1,
                None
            )])),
            no_links_reply(),
        )?;
        let temp = TempDir::new()?;
        let (mut producer, _) = funding_producer(&temp, &server, &patch, now)?;
        let fresh = serde_json::json!([{"url": "https://fresh.example"}]);
        poll_for_funding(&mut producer, &temp, later, &fresh)?;
        producer.shutdown()?;
        Ok(())
    }

    #[test]
    fn a_jingle_with_a_wpay_gives_null_funding_and_sends_no_request() -> Result<()> {
        let server = funding_server(
            links_reply(&serde_json::json!([funding_entry(
                "https://fresh.example",
                1,
                None
            )])),
            no_links_reply(),
        )?;
        let temp = TempDir::new()?;
        let now = Instant::now();
        let patch = serde_json::json!({"seq": 4, "jingle": true, "wpay": WPAY});
        let (mut producer, _) = funding_producer(&temp, &server, &patch, now)?;
        assert_funding_stays(
            &mut producer,
            &temp,
            now + Duration::from_secs(1),
            &serde_json::Value::Null,
        )?;
        assert!(server.requests().is_empty());
        producer.shutdown()?;
        Ok(())
    }

    #[test]
    fn two_segments_with_the_same_feed_guid_and_no_item_link_cause_one_feed_request() -> Result<()>
    {
        let server = funding_server(
            no_links_reply(),
            links_reply(&serde_json::json!([funding_entry(
                "https://channel.example",
                1,
                None
            )])),
        )?;
        let chapters = serde_json::json!([
            remote(0, 10_000, "shared", "item-1".into()),
            remote(10_000, 20_000, "shared", "item-2".into()),
        ]);
        let patch = chapter_patch(&chapters, 4);
        let temp = TempDir::new()?;
        let now = Instant::now();
        let later = now + Duration::from_secs(1);
        let (mut producer, path) = funding_producer(&temp, &server, &patch, now)?;
        let expected = serde_json::json!([{"url": "https://channel.example"}]);
        poll_for_funding(&mut producer, &temp, later, &expected)?;
        read_at(&mut producer, &path, &patch, 2.0, 15.0, later)?;
        poll_for_funding(&mut producer, &temp, later, &expected)?;
        assert_funding_stays(&mut producer, &temp, later, &expected)?;
        let feed_requests: Vec<_> = server
            .requests()
            .into_iter()
            .filter(|request| !request.contains("/tracks/"))
            .collect();
        assert_eq!(feed_requests, ["/v1/feeds/shared?include=source_links"]);
        assert_eq!(server.requests().len(), 3);
        producer.shutdown()?;
        Ok(())
    }
}
