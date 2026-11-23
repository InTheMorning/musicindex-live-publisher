//! Imports songs and podcast episodes into a station folder.

use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, anyhow};
use musicindex_fetch::{
    FeedDocument, FeedReader, ReaderConfig, RemoteItemRef, download_enclosure, song_list,
};
use musicindex_tags::{
    EpisodeTags, Id3v24Edit, NoReferences, SongTags, SplitTarget, read_audio_tags,
    read_podcast_tags, write_episode_tags, write_song_tags,
};
use serde::Serialize;
use sha2::{Digest, Sha256};

/// Inputs and output folder for one import.
#[derive(Clone, Debug, Default)]
pub struct Options {
    /// A local or remote `musicL` feed.
    pub songs: Option<String>,
    /// A remote podcast feed.
    pub episode_feed: Option<String>,
    /// Episode selectors in command-line order.
    pub episodes: Vec<String>,
    /// Output folder.
    pub out: PathBuf,
    /// Skip downloads and tag writes.
    pub dry_run: bool,
}

/// One manifest and its item results.
#[derive(Clone, Debug, Serialize)]
pub struct Manifest {
    format: &'static str,
    dry_run: bool,
    items: Vec<ManifestItem>,
}

/// A summary of one selected or referenced item.
#[derive(Clone, Debug, Serialize)]
pub struct ManifestItem {
    kind: &'static str,
    feed_guid: Option<String>,
    item_guid: Option<String>,
    title: Option<String>,
    feed_url: Option<String>,
    source_url: Option<String>,
    path: Option<String>,
    sha256: Option<String>,
    bytes: Option<u64>,
    tags: Option<TagSummary>,
    chapters: Option<usize>,
    unresolved_splits: Vec<UnresolvedSplit>,
    selector: Option<String>,
    error: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
struct TagSummary {
    applied: usize,
    removed_old_frames: Vec<String>,
    removed_frames: Vec<String>,
    skipped: Vec<String>,
    bad_frames: usize,
    removed_id3v1: bool,
}

#[derive(Clone, Debug, Serialize)]
struct UnresolvedSplit {
    index: usize,
    feed_guid: String,
    item_guid: Option<String>,
    error: String,
}

impl Manifest {
    /// Counts manifest items that contain an error.
    #[must_use]
    pub fn error_count(&self) -> usize {
        self.items
            .iter()
            .filter(|item| item.error.is_some())
            .count()
    }

    /// Counts unresolved episode splits.
    #[must_use]
    pub fn unresolved_count(&self) -> usize {
        self.items
            .iter()
            .map(|item| item.unresolved_splits.len())
            .sum()
    }

    /// Counts song entries.
    #[must_use]
    pub fn song_count(&self) -> usize {
        self.items.iter().filter(|item| item.kind == "song").count()
    }

    /// Counts episode entries.
    #[must_use]
    pub fn episode_count(&self) -> usize {
        self.items
            .iter()
            .filter(|item| item.kind == "episode")
            .count()
    }
}

/// Runs an import and writes its manifest last.
///
/// # Errors
///
/// Returns an error when setup or manifest writing fails.
pub fn run(options: &Options, mut config: ReaderConfig) -> Result<Manifest> {
    if options.songs.is_none() && options.episode_feed.is_none() {
        return Err(anyhow!("at least one input is required"));
    }
    if options.episode_feed.is_none() && !options.episodes.is_empty() {
        return Err(anyhow!("--episode requires --episode-feed"));
    }
    // A dry run writes only the manifest (musicindex ADR 0092 R-STATION-4 item 4).
    if options.dry_run {
        fs::create_dir_all(&options.out).context("create output folder")?;
    } else {
        fs::create_dir_all(options.out.join("songs")).context("create songs folder")?;
        fs::create_dir_all(options.out.join("episodes")).context("create episodes folder")?;
    }
    let mut manifest = Manifest {
        format: "station-import-manifest/1",
        dry_run: options.dry_run,
        items: Vec::new(),
    };
    config.user_agent = format!("station-import/{}", env!("CARGO_PKG_VERSION"));
    let mut reader = FeedReader::new(config).context("make feed reader")?;
    let mut claimed = HashSet::new();
    if let Some(source) = &options.songs {
        let document = if source.starts_with("http://") || source.starts_with("https://") {
            reader
                .read(source)
                .map_err(|e| anyhow!(e.to_string()))?
                .as_ref()
                .clone()
        } else {
            let bytes = fs::read(source).with_context(|| format!("read feed {source}"))?;
            FeedDocument::parse(source, bytes).map_err(|e| anyhow!(e.to_string()))?
        };
        for reference in song_list(&document.values) {
            manifest
                .items
                .push(import_song(&mut reader, &reference, options, &mut claimed));
        }
    }
    if let Some(url) = &options.episode_feed {
        match reader.read(url) {
            Ok(document) => {
                for selector in &options.episodes {
                    manifest.items.push(import_episode(
                        &mut reader,
                        &document.values,
                        url,
                        selector,
                        options,
                        &mut claimed,
                    ));
                }
            }
            Err(error) => {
                for selector in &options.episodes {
                    manifest.items.push(failed(
                        "episode",
                        Some(selector.clone()),
                        Some(url.clone()),
                        error.to_string(),
                    ));
                }
            }
        }
    }
    write_manifest(&options.out, &manifest)?;
    Ok(manifest)
}

fn import_song(
    reader: &mut FeedReader<musicindex_fetch::HttpFetch>,
    reference: &RemoteItemRef,
    options: &Options,
    claimed: &mut HashSet<String>,
) -> ManifestItem {
    let resolved = match reader.resolve(reference) {
        Ok(item) => item,
        Err(error) => {
            return ManifestItem {
                kind: "song",
                feed_guid: Some(reference.feed_guid.clone()),
                item_guid: reference.item_guid.clone(),
                title: reference.title.clone(),
                feed_url: reference.feed_url.clone(),
                source_url: None,
                path: None,
                sha256: None,
                bytes: None,
                tags: None,
                chapters: None,
                unresolved_splits: vec![],
                selector: None,
                error: Some(error.to_string()),
            };
        }
    };
    let mut item = base_item(
        "song",
        &resolved.feed_guid,
        &resolved.item_guid,
        resolved.title.clone(),
        &resolved.feed_url,
        None,
    );
    let Some(url) = resolved.enclosure_url.as_deref() else {
        item.error = Some("item has no enclosure URL".into());
        return item;
    };
    item.source_url = Some(url.to_owned());
    let path = match item_path(
        "songs",
        &resolved.item_guid,
        resolved.enclosure_type.as_deref(),
        url,
    ) {
        Ok(path) => path,
        Err(error) => {
            item.error = Some(error.to_string());
            return item;
        }
    };
    item.path = Some(path.to_string_lossy().into_owned());
    if !claimed.insert(format!("song:{}", resolved.item_guid)) {
        item.error = Some("duplicate item GUID".into());
        return item;
    }
    if options.dry_run {
        return item;
    }
    let destination = options.out.join(&path);
    if let Err(error) = download_enclosure(url, &destination, &user_agent()) {
        item.error = Some(error.to_string());
        return item;
    }
    // The reader keeps the song feed from the resolve, so this read sends no request.
    let album = reader
        .read(&resolved.feed_url)
        .ok()
        .and_then(|document| document.values.title.clone());
    let edits = missing_field_edits(
        &destination,
        &DisplayFields {
            title: resolved.title.as_deref(),
            artist: display_artist(resolved.artist.as_deref(), resolved.author.as_deref()),
            album: album.as_deref(),
        },
    );
    match write_song_tags(
        &destination,
        &SongTags {
            feed_guid: resolved.feed_guid,
            item_guid: resolved.item_guid,
        },
        &edits,
        &NoReferences,
    ) {
        Ok(tags) => match finish_file(&destination) {
            Ok((sha256, bytes)) => {
                item.sha256 = Some(sha256);
                item.bytes = Some(bytes);
                item.tags = Some(tag_summary(tags));
            }
            Err(error) => {
                let _ = fs::remove_file(&destination);
                item.error = Some(format!("{error:#}"));
            }
        },
        Err(error) => {
            let _ = fs::remove_file(&destination);
            item.error = Some(error.to_string());
        }
    }
    item
}

fn import_episode(
    reader: &mut FeedReader<musicindex_fetch::HttpFetch>,
    values: &stophammer_values::FeedValues,
    url: &str,
    selector: &str,
    options: &Options,
    claimed: &mut HashSet<String>,
) -> ManifestItem {
    let number_selector = !selector.is_empty() && selector.bytes().all(|b| b.is_ascii_digit());
    let track = values.tracks.iter().find(|track| {
        if number_selector {
            selector
                .parse::<i64>()
                .ok()
                .is_some_and(|number| track.track_number == Some(number))
        } else {
            track.track_guid.as_deref().map(str::trim) == Some(selector.trim())
        }
    });
    let Some(track) = track else {
        return failed(
            "episode",
            Some(selector.into()),
            Some(url.into()),
            format!("no episode matches selector {selector}"),
        );
    };
    let Some(guid) = values.feed_guid.as_deref().map(str::trim) else {
        return failed(
            "episode",
            Some(selector.into()),
            Some(url.into()),
            "podcast feed has no feed GUID".into(),
        );
    };
    if guid.is_empty() {
        return failed(
            "episode",
            Some(selector.into()),
            Some(url.into()),
            "podcast feed has no feed GUID".into(),
        );
    }
    let Some(item_guid) = track.track_guid.as_deref().map(str::trim) else {
        return failed(
            "episode",
            Some(selector.into()),
            Some(url.into()),
            format!("episode {selector} has no item GUID"),
        );
    };
    let mut item = base_item(
        "episode",
        guid,
        item_guid,
        track.title.clone(),
        url,
        Some(selector.into()),
    );
    let Some(enclosure) = track.enclosure_url.as_deref() else {
        item.error = Some("item has no enclosure URL".into());
        return item;
    };
    item.source_url = Some(enclosure.into());
    let path = match item_path(
        "episodes",
        item_guid,
        track.enclosure_type.as_deref(),
        enclosure,
    ) {
        Ok(path) => path,
        Err(error) => {
            item.error = Some(error.to_string());
            return item;
        }
    };
    item.path = Some(path.to_string_lossy().into_owned());
    if !claimed.insert(format!("episode:{item_guid}")) {
        item.error = Some("duplicate item GUID".into());
        return item;
    }
    let mut episode_splits = match reader.episode_splits(track) {
        Ok(value) => value,
        Err(error) => {
            item.error = Some(error.to_string());
            return item;
        }
    };
    // musicindex ADR 0093 R-TAG-4 item 4: the caller gives the chapter artist.
    // station-import gives the same artist as for a song file. `author` stays.
    for split in &mut episode_splits.splits {
        split.artist =
            display_artist(split.artist.as_deref(), split.author.as_deref()).map(str::to_owned);
    }
    item.chapters = Some(episode_splits.splits.len());
    item.unresolved_splits = episode_splits
        .unresolved
        .iter()
        .filter_map(|(index, error)| {
            episode_splits
                .splits
                .get(*index)
                .and_then(|split| match &split.target {
                    SplitTarget::Remote {
                        feed_guid,
                        item_guid,
                        ..
                    } => Some(UnresolvedSplit {
                        index: *index,
                        feed_guid: feed_guid.clone(),
                        item_guid: item_guid.clone(),
                        error: error.to_string(),
                    }),
                    _ => None,
                })
        })
        .collect();
    if options.dry_run {
        return item;
    }
    let destination = options.out.join(&path);
    if let Err(error) = download_enclosure(enclosure, &destination, &user_agent()) {
        item.error = Some(error.to_string());
        return item;
    }
    let edits = missing_field_edits(
        &destination,
        &DisplayFields {
            title: track.title.as_deref(),
            artist: display_artist(track.author_name.as_deref(), values.author_name.as_deref()),
            album: values.title.as_deref(),
        },
    );
    let routes = musicindex_fetch::value_routes(&track.value_blocks, &track.payment_routes)
        .or_else(|| {
            musicindex_fetch::value_routes(&values.feed_value_blocks, &values.feed_payment_routes)
        })
        .map(|value| value.to_string());
    match write_episode_tags(
        &destination,
        &EpisodeTags {
            feed_guid: guid.into(),
            item_guid: item_guid.into(),
            value_routes: routes,
            splits: episode_splits.splits,
        },
        &edits,
        &NoReferences,
    ) {
        Ok(tags) => match finish_file(&destination) {
            Ok((sha256, bytes)) => {
                item.sha256 = Some(sha256);
                item.bytes = Some(bytes);
                item.tags = Some(tag_summary(tags));
                item.chapters = read_podcast_tags(&destination)
                    .ok()
                    .and_then(|tags| tags.chapters().ok())
                    .map(|chapters| chapters.len());
            }
            Err(error) => {
                let _ = fs::remove_file(&destination);
                item.error = Some(format!("{error:#}"));
            }
        },
        Err(error) => {
            let _ = fs::remove_file(&destination);
            item.error = Some(error.to_string());
        }
    }
    item
}

// This alias keeps the parser type reachable through musicindex-fetch only.
mod stophammer_values {
    pub use musicindex_fetch::stophammer_parser::FeedValues;
}

fn base_item(
    kind: &'static str,
    feed: &str,
    guid: &str,
    title: Option<String>,
    feed_url: &str,
    selector: Option<String>,
) -> ManifestItem {
    ManifestItem {
        kind,
        feed_guid: Some(feed.into()),
        item_guid: Some(guid.into()),
        title,
        feed_url: Some(feed_url.into()),
        source_url: None,
        path: None,
        sha256: None,
        bytes: None,
        tags: None,
        chapters: None,
        unresolved_splits: vec![],
        selector,
        error: None,
    }
}
fn failed(
    kind: &'static str,
    selector: Option<String>,
    feed_url: Option<String>,
    error: String,
) -> ManifestItem {
    ManifestItem {
        kind,
        feed_guid: None,
        item_guid: None,
        title: None,
        feed_url,
        source_url: None,
        path: None,
        sha256: None,
        bytes: None,
        tags: None,
        chapters: None,
        unresolved_splits: vec![],
        selector,
        error: Some(error),
    }
}
fn item_path(folder: &str, guid: &str, mime: Option<&str>, url: &str) -> Result<PathBuf> {
    if guid.is_empty()
        || guid.starts_with('.')
        || !guid
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.'))
    {
        return Err(anyhow!("item GUID is not safe for a file name"));
    }
    let extension = mime
        .and_then(mime_extension)
        .map(str::to_owned)
        .or_else(|| {
            Path::new(
                url.split('?')
                    .next()
                    .unwrap_or(url)
                    .rsplit('/')
                    .next()
                    .unwrap_or(""),
            )
            .extension()
            .and_then(|ext| ext.to_str())
            .map(str::to_ascii_lowercase)
            .filter(|ext| {
                matches!(
                    ext.as_str(),
                    "mp3" | "flac" | "ogg" | "opus" | "m4a" | "wav"
                )
            })
        })
        .ok_or_else(|| anyhow!("enclosure has no supported audio extension"))?;
    Ok(PathBuf::from(folder).join(format!("{guid}.{extension}")))
}
fn mime_extension(mime: &str) -> Option<&'static str> {
    match mime.trim().to_ascii_lowercase().as_str() {
        "audio/mpeg" | "audio/mp3" => Some("mp3"),
        "audio/flac" | "audio/x-flac" => Some("flac"),
        "audio/ogg" => Some("ogg"),
        "audio/opus" => Some("opus"),
        "audio/mp4" | "audio/x-m4a" | "audio/aac" => Some("m4a"),
        "audio/wav" | "audio/x-wav" => Some("wav"),
        _ => None,
    }
}
fn user_agent() -> String {
    format!("station-import/{}", env!("CARGO_PKG_VERSION"))
}
fn tag_summary(value: musicindex_tags::TagWriteResult) -> TagSummary {
    TagSummary {
        applied: value.applied,
        removed_old_frames: value.removed_old_frames,
        removed_frames: value.removed_frames,
        skipped: value.skipped,
        bad_frames: value.bad_frames.len(),
        removed_id3v1: value.removed_id3v1,
    }
}
/// The artist of a song or an episode: the item `itunes:author`, else the
/// channel `itunes:author` (orchestrator choice of 2026-11-23, task 001). An
/// empty value counts as no value.
fn display_artist<'a>(
    item_author: Option<&'a str>,
    channel_author: Option<&'a str>,
) -> Option<&'a str> {
    let present = |value: &&str| !value.trim().is_empty();
    item_author
        .filter(present)
        .or(channel_author.filter(present))
}

/// The display values of an item: the title, the artist and the album.
struct DisplayFields<'a> {
    title: Option<&'a str>,
    artist: Option<&'a str>,
    album: Option<&'a str>,
}

/// The edits of the title, artist and album frames that the downloaded file
/// does not have.
///
/// The title is the item title. The artist is the value of [`display_artist`].
/// The album is the channel title. A value that the file has stays
/// (orchestrator choice of 2026-11-23, task 001). When the tag read fails, the
/// function gives no edit, so that it replaces no value.
fn missing_field_edits(path: &Path, fields: &DisplayFields<'_>) -> Vec<Id3v24Edit> {
    let Ok(tags) = read_audio_tags(path) else {
        return Vec::new();
    };
    [
        ("TIT2", tags.title, fields.title),
        ("TPE1", tags.artist, fields.artist),
        ("TALB", tags.album, fields.album),
    ]
    .into_iter()
    .filter(|(_, present, _)| nonempty(present.as_deref()).is_none())
    .filter_map(|(frame_label, _, value)| {
        nonempty(value).map(|value| Id3v24Edit {
            frame_label: frame_label.to_owned(),
            value: value.to_owned(),
        })
    })
    .collect()
}

fn nonempty(value: Option<&str>) -> Option<&str> {
    value.map(str::trim).filter(|value| !value.is_empty())
}

/// Gives the file the mode 0644 and returns its SHA-256 hash and size.
///
/// Call it after the tag write. The manifest hash is the hash of the file at
/// `path`, so the operator can examine the copy on the station
/// (musicindex ADR 0092 R-STATION-4 item 3). The download makes a file with
/// the mode 0600. The mode 0644 lets the AzuraCast container read a copy that
/// keeps the mode.
fn finish_file(path: &Path) -> Result<(String, u64)> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o644))
            .context("set the file mode")?;
    }
    let mut file = fs::File::open(path).context("open the tagged file")?;
    let mut hasher = Sha256::new();
    let bytes = std::io::copy(&mut file, &mut hasher).context("hash the tagged file")?;
    Ok((format!("{:x}", hasher.finalize()), bytes))
}

fn write_manifest(folder: &Path, manifest: &Manifest) -> Result<()> {
    let bytes = serde_json::to_vec_pretty(manifest)?;
    let temp = folder.join(".manifest.json.tmp");
    fs::write(&temp, bytes).context("write temporary manifest")?;
    fs::rename(temp, folder.join("manifest.json")).context("replace manifest")?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::display_artist;

    #[test]
    fn display_artist_is_the_item_author_first() {
        assert_eq!(display_artist(Some("Item"), Some("Channel")), Some("Item"));
    }

    #[test]
    fn display_artist_with_no_item_author_is_the_channel_author() {
        assert_eq!(display_artist(None, Some("Channel")), Some("Channel"));
        assert_eq!(display_artist(Some("  "), Some("Channel")), Some("Channel"));
        assert_eq!(display_artist(None, Some("")), None);
    }

    #[test]
    fn display_artist_keeps_a_present_artist_as_it_is() {
        assert_eq!(
            display_artist(Some(" Artist "), Some("Channel")),
            Some(" Artist ")
        );
    }
}
