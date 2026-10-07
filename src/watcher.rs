//! Drop directory event processing.

use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use uuid::Uuid;

use crate::{
    DropFile, LiveValuePayload, ProducerState, dead_payload, parse, payload_from_dropfile,
};

/// Debounce window for repeated filesystem notifications on one path.
pub const DEFAULT_DEBOUNCE_WINDOW: Duration = Duration::from_millis(75);

/// A configured publish target needed by the watcher.
#[derive(Debug, Clone, PartialEq)]
pub struct WatchTarget {
    pub name: String,
    pub event_guid: String,
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
    last_payload: LiveValuePayload,
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

    /// Computes the startup payload for each configured target (ADR 0005).
    ///
    /// A target's block is its track block only when the producer is
    /// running and a file for that target is present. Every other target's
    /// block is the dead block. Startup always publishes one block for each
    /// target, because it replaces whatever payload a previous run may have
    /// left live on the relay.
    ///
    /// # Errors
    ///
    /// Returns an error when the watch directory cannot be read.
    pub fn startup_payloads(
        &mut self,
        watch_dir: &Path,
        producer: ProducerState,
    ) -> Result<Vec<LiveValuePayload>> {
        let track_payloads = match producer {
            ProducerState::Running => self.scan_track_payloads(watch_dir)?,
            // ADR 0005: a missing producer means every target gets the dead
            // block, so scanning the directory would be wasted work.
            ProducerState::Missing => Vec::new(),
        };

        let present: HashSet<String> = track_payloads
            .iter()
            .map(|payload| payload.event_guid.clone())
            .collect();

        let mut payloads = track_payloads;
        payloads.extend(
            self.targets
                .values()
                .filter(|target| !present.contains(&target.event_guid))
                .map(|target| dead_payload(&target.event_guid, &fresh_guid())),
        );
        Ok(payloads)
    }

    /// Scans the watch directory for present, parseable track payloads.
    ///
    /// This emits no dead block for a target with no file, and no dead
    /// block for an empty directory. It is the scan half of
    /// [`DropWatcher::startup_payloads`], and it is also what the watch loop
    /// runs when the producer lock returns after being free: ADR 0005 keeps
    /// the dead block already live for a target with no track payload, so a
    /// republish would only repeat it under a new `blockGuid`.
    ///
    /// # Errors
    ///
    /// Returns an error when the watch directory cannot be read.
    pub fn scan_track_payloads(&mut self, watch_dir: &Path) -> Result<Vec<LiveValuePayload>> {
        let mut paths = final_drop_files(watch_dir)?;
        paths.sort();

        paths
            .iter()
            .filter_map(|path| self.payload_for_upsert(path).transpose())
            .collect()
    }

    /// Emits the dead block for each configured target and clears block
    /// identity state.
    ///
    /// ADR 0005: when the producer lock frees, the publisher publishes the
    /// dead block once for each target. A path's block identity must not
    /// survive this call: the next file that lands on a path the producer
    /// reused is a new occupant, not a rewrite of the block that was live
    /// before the producer left, so it must mint a fresh `blockGuid`.
    pub fn producer_missing_payloads(&mut self) -> Vec<LiveValuePayload> {
        self.blocks.clear();
        self.dead_payloads()
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
                    .map(|target| vec![dead_payload(&target.event_guid, &fresh_guid())])
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
            _ => fresh_guid(),
        };
        // ADR 0005: an empty value_routes list is not a payable block. The
        // dead block replaces it entirely; it keeps neither the drop file's
        // title nor its GUIDs.
        let payload = if dropfile.value_routes.is_empty() {
            dead_payload(&target.event_guid, &block_guid)
        } else {
            payload_from_dropfile(&dropfile, &target.event_guid, &block_guid)
        };
        let should_emit = !self
            .blocks
            .get(&identity)
            .is_some_and(|block| block.track == track && block.last_payload == payload);
        if !should_emit {
            tracing::debug!(
                path = %path.display(),
                block_guid = %block_guid,
                title = %payload.title,
                "skipping unchanged live value payload"
            );
        }

        self.blocks.insert(
            identity.clone(),
            BlockIdentity {
                track,
                block_guid,
                last_payload: payload.clone(),
            },
        );
        self.path_targets.insert(identity, target.name.clone());

        Ok(should_emit.then_some(payload))
    }

    fn dead_payloads(&self) -> Vec<LiveValuePayload> {
        self.targets
            .values()
            .map(|target| dead_payload(&target.event_guid, &fresh_guid()))
            .collect()
    }
}

/// Drops a repeat of the last event of a path inside the window.
///
/// Only the last event of each path counts. An event of the other kind ends
/// the window, so remove, write and remove inside the window give three
/// events. The last one decides the live block.
#[derive(Debug)]
struct Debouncer {
    window: Duration,
    last: HashMap<PathBuf, (DropEventKind, Instant)>,
}

impl Debouncer {
    fn new(window: Duration) -> Self {
        Self {
            window,
            last: HashMap::new(),
        }
    }

    fn is_duplicate(&mut self, event: &DropEvent, now: Instant) -> bool {
        let key = path_identity(&event.path);
        let duplicate = self.last.get(&key).is_some_and(|(kind, last)| {
            *kind == event.kind && now.duration_since(*last) < self.window
        });
        if !duplicate {
            self.last.insert(key, (event.kind, now));
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
