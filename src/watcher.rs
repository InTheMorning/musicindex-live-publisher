//! Drop directory event processing.

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use uuid::Uuid;

use crate::{
    DropFile, LiveValue, LiveValuePayload, fallback_payload, parse, payload_from_dropfile,
};

/// Debounce window for repeated filesystem notifications on one path.
pub const DEFAULT_DEBOUNCE_WINDOW: Duration = Duration::from_millis(75);

/// A configured publish target needed by the watcher.
#[derive(Debug, Clone, PartialEq)]
pub struct WatchTarget {
    pub name: String,
    pub event_guid: String,
    pub fallback: FallbackConfig,
}

/// Fallback live value configuration.
#[derive(Debug, Clone, PartialEq)]
pub struct FallbackConfig {
    pub title: String,
    pub image: Option<String>,
    pub value: LiveValue,
}

/// Filesystem actions that can affect the currently published payload.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DropEventKind {
    Upsert,
    Remove,
}

/// A normalized filesystem event for the drop directory.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DropEvent {
    pub kind: DropEventKind,
    pub path: PathBuf,
}

/// Stateful drop-event processor.
#[derive(Debug)]
pub struct DropWatcher {
    targets: HashMap<String, WatchTarget>,
    path_targets: HashMap<PathBuf, String>,
    blocks: HashMap<PathBuf, BlockIdentity>,
    debouncer: Debouncer,
}

/// The track a path is currently publishing, and the block GUID it publishes under.
///
/// A producer rewrites one stable path per track, so the path alone cannot
/// distinguish a rewrite of the current track from a move to the next one.
#[derive(Debug)]
struct BlockIdentity {
    track: TrackIdentity,
    block_guid: String,
}

/// What makes two drop files the same track.
///
/// Deliberately not whole-file equality. The producer writes twice for a single
/// track when a MusicIndex value-route lookup upgrades the embedded routes, and
/// that second write must stay inside the same block. A MusicIndex track GUID
/// identifies the track on its own; artist and title are the fallback for a V4V
/// file that carries no GUID.
#[derive(Debug, Clone, PartialEq, Eq)]
enum TrackIdentity {
    Guid(String),
    ArtistTitle(String, String),
}

fn track_identity(dropfile: &DropFile) -> TrackIdentity {
    match dropfile.track_guid.as_deref() {
        Some(guid) if !guid.trim().is_empty() => TrackIdentity::Guid(guid.to_owned()),
        _ => TrackIdentity::ArtistTitle(dropfile.artist.clone(), dropfile.title.clone()),
    }
}

impl DropWatcher {
    /// Creates a drop-event processor for one target.
    pub fn new(target: WatchTarget, debounce_window: Duration) -> Self {
        Self::new_targets(vec![target], debounce_window)
    }

    /// Creates a drop-event processor for configured targets.
    pub fn new_targets(targets: Vec<WatchTarget>, debounce_window: Duration) -> Self {
        Self {
            targets: targets
                .into_iter()
                .map(|target| (target.name.clone(), target))
                .collect(),
            path_targets: HashMap::new(),
            blocks: HashMap::new(),
            debouncer: Debouncer::new(debounce_window),
        }
    }

    /// Emits the startup state for an existing drop directory.
    ///
    /// If no final drop files are present, this emits the fallback payload.
    pub fn initial_payloads(&mut self, watch_dir: &Path) -> Result<Vec<LiveValuePayload>> {
        let mut paths = final_drop_files(watch_dir)?;
        paths.sort();

        if paths.is_empty() {
            return Ok(self.fallback_payloads());
        }

        paths
            .iter()
            .filter_map(|path| self.payload_for_upsert(path).transpose())
            .collect()
    }

    /// Processes one normalized event and returns payloads to emit.
    pub fn process_event(
        &mut self,
        event: DropEvent,
        now: Instant,
    ) -> Result<Vec<LiveValuePayload>> {
        if !is_final_drop_file(&event.path) {
            return Ok(Vec::new());
        }
        if self.debouncer.is_duplicate(&event, now) {
            return Ok(Vec::new());
        }

        match event.kind {
            DropEventKind::Upsert => Ok(self
                .payload_for_upsert(&event.path)?
                .into_iter()
                .collect::<Vec<_>>()),
            DropEventKind::Remove => {
                let identity = path_identity(&event.path);
                self.blocks.remove(&identity);
                let Some(target_name) = self.path_targets.remove(&identity) else {
                    return Ok(Vec::new());
                };
                Ok(self
                    .targets
                    .get(&target_name)
                    .map(|target| vec![fallback_payload_for_target(target)])
                    .unwrap_or_default())
            }
        }
    }

    fn payload_for_upsert(&mut self, path: &Path) -> Result<Option<LiveValuePayload>> {
        let bytes = fs::read(path).with_context(|| format!("read drop file {}", path.display()))?;
        let dropfile = match parse(&bytes) {
            Ok(Some(dropfile)) => dropfile,
            Ok(None) => return Ok(None),
            Err(error) => {
                tracing::warn!(path = %path.display(), %error, "skipping invalid drop file");
                return Ok(None);
            }
        };

        let Some(target) = self.targets.get(&dropfile.target) else {
            tracing::warn!(
                path = %path.display(),
                target = dropfile.target,
                "skipping drop file for unknown target"
            );
            return Ok(None);
        };

        let identity = path_identity(path);
        let track = track_identity(&dropfile);
        let block_guid = match self.blocks.get(&identity) {
            // Same track rewritten, or a duplicate filesystem event: keep the
            // block GUID so retries and route upgrades stay one block.
            Some(block) if block.track == track => block.block_guid.clone(),
            // Next track at a path the producer reuses: mint a fresh block.
            _ => {
                let block_guid = fresh_guid();
                self.blocks.insert(
                    identity.clone(),
                    BlockIdentity {
                        track,
                        block_guid: block_guid.clone(),
                    },
                );
                block_guid
            }
        };
        self.path_targets.insert(identity, target.name.clone());

        Ok(Some(payload_from_dropfile(
            &dropfile,
            &target.event_guid,
            &block_guid,
        )))
    }

    fn fallback_payloads(&self) -> Vec<LiveValuePayload> {
        self.targets
            .values()
            .map(fallback_payload_for_target)
            .collect()
    }
}

fn fallback_payload_for_target(target: &WatchTarget) -> LiveValuePayload {
    let image = target.fallback.image.as_deref();
    fallback_payload(
        &target.fallback.title,
        image,
        &target.event_guid,
        &fresh_guid(),
        &target.fallback.value,
    )
}

#[derive(Debug)]
struct Debouncer {
    window: Duration,
    last: HashMap<(PathBuf, DropEventKind), Instant>,
}

impl Debouncer {
    fn new(window: Duration) -> Self {
        Self {
            window,
            last: HashMap::new(),
        }
    }

    fn is_duplicate(&mut self, event: &DropEvent, now: Instant) -> bool {
        let key = (path_identity(&event.path), event.kind);
        let duplicate = self
            .last
            .get(&key)
            .is_some_and(|last| now.duration_since(*last) < self.window);
        if !duplicate {
            self.last.insert(key, now);
        }
        duplicate
    }
}

/// Returns true for final JSON drop files, false for temp/hidden files.
pub fn is_final_drop_file(path: &Path) -> bool {
    let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
        return false;
    };
    !name.starts_with('.')
        && path
            .extension()
            .is_some_and(|extension| extension == "json")
}

fn final_drop_files(watch_dir: &Path) -> Result<Vec<PathBuf>> {
    let paths = fs::read_dir(watch_dir)
        .with_context(|| format!("read watch directory {}", watch_dir.display()))?
        .map(|entry| entry.map(|entry| entry.path()))
        .collect::<std::io::Result<Vec<_>>>()
        .with_context(|| format!("read entries from {}", watch_dir.display()))?;

    Ok(paths
        .into_iter()
        .filter(|path| path.is_file() && is_final_drop_file(path))
        .collect())
}

fn path_identity(path: &Path) -> PathBuf {
    path.to_path_buf()
}

fn fresh_guid() -> String {
    Uuid::new_v4().to_string()
}
