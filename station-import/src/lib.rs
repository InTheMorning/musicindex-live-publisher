//! Imports songs and podcast episodes into a station folder.

use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, anyhow};
use musicindex_convert::{
    ConverterObservation, DEFAULT_MP3_BIT_RATE_KBPS, convert_to_mp3_with, convert_with,
};
use musicindex_fetch::{
    FeedDocument, FeedReader, ReaderConfig, RemoteItemRef, download_enclosure, song_list,
    track_mp3_url,
};
use musicindex_tags::format::AudioFormat;
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
    /// The format of the audio files (musicindex ADR 0095 R-MP3-1).
    pub format: OutputFormat,
}

/// The format of the audio files of one import (musicindex ADR 0095
/// R-MP3-1).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum OutputFormat {
    /// Keep the audio of the feed. A WAV song becomes a FLAC file (musicindex
    /// ADR 0094 R-CONVERT-7).
    #[default]
    Keep,
    /// Write only MP3 files. A converted file has this constant bit rate in
    /// kbit/s (R-MP3-3 item 6).
    Mp3 {
        /// The bit rate of a converted file in kbit/s.
        bit_rate_kbps: u32,
    },
}

impl OutputFormat {
    /// The format `mp3` with the default bit rate of 256 kbit/s.
    pub const MP3: Self = Self::Mp3 {
        bit_rate_kbps: DEFAULT_MP3_BIT_RATE_KBPS,
    };
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

/// Runs an import and writes its manifest last. A WAV song becomes a FLAC
/// file through `flac` or `ffmpeg` on `PATH` (musicindex ADR 0094
/// R-CONVERT-7).
///
/// # Errors
///
/// Returns an error when setup or manifest writing fails.
pub fn run(options: &Options, config: ReaderConfig) -> Result<Manifest> {
    run_with(options, config, &|| ConverterObservation::refresh(None))
}

/// [`run`] with the converter probe that `probe` gives. The run calls
/// `probe` one time, at the first file that needs a conversion.
///
/// # Errors
///
/// Returns an error when setup or manifest writing fails.
pub fn run_with(
    options: &Options,
    mut config: ReaderConfig,
    probe: &dyn Fn() -> ConverterObservation,
) -> Result<Manifest> {
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
    let mut converter = Converter {
        probe,
        observation: None,
    };
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
            manifest.items.push(import_song(
                &mut reader,
                &reference,
                options,
                &mut claimed,
                &mut converter,
            ));
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
                        &mut converter,
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
    // A failed run can leave the staging folder. Only an empty one goes.
    let _ = fs::remove_dir(options.out.join(STAGING_DIR));
    write_manifest(&options.out, &manifest)?;
    Ok(manifest)
}

/// The folder in the output folder that holds a downloaded file while it
/// becomes a FLAC or an MP3 file. The run removes it when it is empty.
const STAGING_DIR: &str = ".convert";

/// The converter of one run. It probes the programs one time, at the first
/// file that needs a conversion (musicindex ADR 0094 R-CONVERT-7, ADR 0095
/// R-MP3-3 item 4).
struct Converter<'a> {
    probe: &'a dyn Fn() -> ConverterObservation,
    observation: Option<ConverterObservation>,
}

impl Converter<'_> {
    fn observation(&mut self) -> &ConverterObservation {
        self.observation.get_or_insert_with(self.probe)
    }
}

/// Changes a downloaded WAV song into a FLAC file with the same name and the
/// extension `flac` (musicindex ADR 0094 R-CONVERT-7 item 1). Gives `None`
/// for a file that is not WAV. The WAV file does not stay in the output
/// folder, also after an error.
fn convert_wav(
    out: &Path,
    downloaded: &Path,
    converter: &mut Converter<'_>,
) -> Result<Option<PathBuf>> {
    if !matches!(
        AudioFormat::detect_from_file(downloaded),
        Ok(AudioFormat::Wav)
    ) {
        return Ok(None);
    }
    let flac = downloaded.with_extension("flac");
    let staging = out.join(STAGING_DIR);
    let stem = downloaded
        .file_stem()
        .ok_or_else(|| anyhow!("the song file has no name"))?;
    let staged = staging.join(stem).with_extension("wav");
    let moved = fs::create_dir_all(&staging)
        .and_then(|()| fs::rename(downloaded, &staged))
        .context("move the WAV song to the staging folder");
    if let Err(error) = moved {
        let _ = fs::remove_file(downloaded);
        return Err(error);
    }
    // A FLAC file of an earlier run is replaced, as a download replaces a file.
    let result = convert_with(&staged, converter.observation())
        .map_err(|error| anyhow!("cannot convert the WAV song to FLAC: {error}"))
        .and_then(|(converted, _)| {
            fs::rename(&converted, &flac)
                .inspect_err(|_| {
                    let _ = fs::remove_file(&converted);
                })
                .context("move the FLAC song to the output folder")
        });
    let _ = fs::remove_file(&staged);
    result.map(|()| Some(flac))
}

/// The source and the planned path of the audio file of one item.
struct AudioPlan {
    /// The URL that the import downloads.
    url: String,
    /// The path in the output folder before a conversion of the format
    /// `keep`.
    path: PathBuf,
    /// The format `mp3` converts the downloaded file (musicindex ADR 0095
    /// R-MP3-2 item 4).
    convert: bool,
}

/// Selects the URL and the path of the audio file of an item.
///
/// With [`OutputFormat::Keep`], the URL is the enclosure. With
/// [`OutputFormat::Mp3`], the URL is the MP3 file that musicindex-fetch
/// selects (`mp3_url`, R-MP3-2 item 1), else the enclosure, which the import
/// converts. The path then has the extension `mp3`.
fn plan_audio(
    folder: &str,
    guid: &str,
    enclosure_url: Option<&str>,
    enclosure_type: Option<&str>,
    mp3_url: Option<&str>,
    format: OutputFormat,
) -> Result<AudioPlan> {
    if let (OutputFormat::Mp3 { .. }, Some(url)) = (format, mp3_url) {
        return Ok(AudioPlan {
            url: url.to_owned(),
            path: mp3_path(folder, guid)?,
            convert: false,
        });
    }
    let url = enclosure_url.ok_or_else(|| anyhow!("item has no enclosure URL"))?;
    match format {
        OutputFormat::Keep => Ok(AudioPlan {
            url: url.to_owned(),
            path: item_path(folder, guid, enclosure_type, url)?,
            convert: false,
        }),
        OutputFormat::Mp3 { .. } => Ok(AudioPlan {
            url: url.to_owned(),
            path: mp3_path(folder, guid)?,
            convert: true,
        }),
    }
}

/// Downloads the audio file of `plan` and gives its path in the output
/// folder, in the output format of the run.
///
/// With [`OutputFormat::Keep`], a WAV song becomes a FLAC file when
/// `wav_to_flac` is `true` (musicindex ADR 0094 R-CONVERT-7 applies to
/// songs). With [`OutputFormat::Mp3`] and a planned conversion, the file goes
/// to the staging folder and becomes an MP3 file (ADR 0095 R-MP3-2 item 4).
/// No downloaded file stays in the staging folder, also after an error.
fn fetch_audio(
    out: &Path,
    plan: &AudioPlan,
    format: OutputFormat,
    wav_to_flac: bool,
    converter: &mut Converter<'_>,
) -> Result<PathBuf> {
    let destination = out.join(&plan.path);
    let bit_rate_kbps = match format {
        OutputFormat::Mp3 { bit_rate_kbps } if plan.convert => bit_rate_kbps,
        OutputFormat::Mp3 { .. } => {
            download_enclosure(&plan.url, &destination, &user_agent())?;
            return Ok(plan.path.clone());
        }
        OutputFormat::Keep => {
            download_enclosure(&plan.url, &destination, &user_agent())?;
            if !wav_to_flac {
                return Ok(plan.path.clone());
            }
            return Ok(match convert_wav(out, &destination, converter)? {
                Some(_) => plan.path.with_extension("flac"),
                None => plan.path.clone(),
            });
        }
    };
    let staging = out.join(STAGING_DIR);
    let stem = plan
        .path
        .file_stem()
        .ok_or_else(|| anyhow!("the audio file has no name"))?;
    let staged = staging.join(stem).with_extension("download");
    fs::create_dir_all(&staging).context("create the staging folder")?;
    download_enclosure(&plan.url, &staged, &user_agent())?;
    // An MP3 file of a stopped run would block the conversion.
    let _ = fs::remove_file(staged.with_extension("mp3"));
    let result = convert_to_mp3_with(&staged, bit_rate_kbps, &converter.observation().ffmpeg)
        .map_err(|error| anyhow!("cannot convert the audio file to MP3: {error}"))
        .and_then(|converted| {
            fs::rename(&converted, &destination)
                .inspect_err(|_| {
                    let _ = fs::remove_file(&converted);
                })
                .context("move the MP3 file to the output folder")
        });
    let _ = fs::remove_file(&staged);
    result.map(|()| plan.path.clone())
}

fn import_song(
    reader: &mut FeedReader<musicindex_fetch::HttpFetch>,
    reference: &RemoteItemRef,
    options: &Options,
    claimed: &mut HashSet<String>,
    converter: &mut Converter<'_>,
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
    let plan = match plan_audio(
        "songs",
        &resolved.item_guid,
        resolved.enclosure_url.as_deref(),
        resolved.enclosure_type.as_deref(),
        resolved.mp3_url(),
        options.format,
    ) {
        Ok(plan) => plan,
        Err(error) => {
            item.error = Some(error.to_string());
            return item;
        }
    };
    item.source_url = Some(plan.url.clone());
    item.path = Some(plan.path.to_string_lossy().into_owned());
    if !claimed.insert(format!("song:{}", resolved.item_guid)) {
        item.error = Some("duplicate item GUID".into());
        return item;
    }
    if options.dry_run {
        return item;
    }
    let path = match fetch_audio(&options.out, &plan, options.format, true, converter) {
        Ok(path) => path,
        Err(error) => {
            item.error = Some(format!("{error:#}"));
            return item;
        }
    };
    item.path = Some(path.to_string_lossy().into_owned());
    let destination = options.out.join(&path);
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
            image: new_image(&destination, resolved.image_url),
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
    converter: &mut Converter<'_>,
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
    let plan = match plan_audio(
        "episodes",
        item_guid,
        track.enclosure_url.as_deref(),
        track.enclosure_type.as_deref(),
        track_mp3_url(track),
        options.format,
    ) {
        Ok(plan) => plan,
        Err(error) => {
            item.error = Some(error.to_string());
            return item;
        }
    };
    item.source_url = Some(plan.url.clone());
    item.path = Some(plan.path.to_string_lossy().into_owned());
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
    let path = match fetch_audio(&options.out, &plan, options.format, false, converter) {
        Ok(path) => path,
        Err(error) => {
            item.error = Some(format!("{error:#}"));
            return item;
        }
    };
    item.path = Some(path.to_string_lossy().into_owned());
    let destination = options.out.join(&path);
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
            image: new_image(&destination, musicindex_fetch::image_url(values, track)),
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
fn mp3_path(folder: &str, guid: &str) -> Result<PathBuf> {
    if guid.is_empty() {
        return Err(anyhow!("item GUID is empty"));
    }
    Ok(PathBuf::from(folder).join(format!("{}.mp3", file_stem(guid))))
}
fn item_path(folder: &str, guid: &str, mime: Option<&str>, url: &str) -> Result<PathBuf> {
    if guid.is_empty() {
        return Err(anyhow!("item GUID is empty"));
    }
    let stem = file_stem(guid);
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
    Ok(PathBuf::from(folder).join(format!("{stem}.{extension}")))
}
/// The file name of an item without its extension: the item GUID when it is
/// safe for a file name, else "sha256-" and the SHA-256 hex of the GUID. A
/// safe GUID has only ASCII letters, digits, `-`, `_` and `.`, and does not
/// start with `.` or with "sha256-". The tags and the manifest keep the GUID.
fn file_stem(guid: &str) -> String {
    let safe = !guid.starts_with('.')
        && !guid.starts_with("sha256-")
        && guid
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.'));
    if safe {
        guid.to_owned()
    } else {
        format!("sha256-{:x}", Sha256::digest(guid.as_bytes()))
    }
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

/// The image URL to write in `TXXX:Podcast Image`.
///
/// A file that has the frame keeps its value, also when the feed has a
/// different image (musicindex ADR 0096 R-IMAGE-3 item 2 and Open Point 1).
/// The function then gives `None`, and the tag write writes no frame. An item
/// with no image URL also gives `None` (R-IMAGE-3 item 3).
fn new_image(path: &Path, image: Option<String>) -> Option<String> {
    let present = read_podcast_tags(path)
        .ok()
        .and_then(|tags| tags.image)
        .is_some_and(|value| nonempty(Some(&value)).is_some());
    if present { None } else { image }
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
