//! Read Liquidsoap state and publish producer drop files.

pub mod state;

use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use now_playing_drop::sink::{OutputFile, Presence};
use now_playing_drop::{
    InstanceSettings, LOCK_WAIT_LIMIT, PaymentRoute, ProducerLock, Version3DropFile,
};

use state::StateFile;

const FRESHNESS: Duration = Duration::from_secs(5);

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
        let header: state::StateHeader = match serde_json::from_slice(&bytes) {
            Ok(header) => header,
            Err(_) => return Ok(()),
        };
        if header.format != state::STATE_FORMAT {
            if self.unknown_format.as_deref() != Some(&header.format) {
                tracing::error!(format = %header.format, path = %self.state_path.display(), "unknown Liquidsoap state format");
                self.unknown_format = Some(header.format);
            }
            return self.clear();
        }
        self.unknown_format = None;
        let parsed: StateFile = match serde_json::from_slice(&bytes) {
            Ok(state) => state,
            Err(_) => return Ok(()),
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
        let fresh = self
            .written_changed_at
            .is_some_and(|changed| now.saturating_duration_since(changed) < FRESHNESS);
        if !fresh || parsed.live {
            return self.clear();
        }
        self.ensure_lock()?;
        let content = render_state(&parsed, &self.target, &mut self.bad_routes_seq);
        self.output
            .set(Presence::Present(serde_json::to_string_pretty(&content)?))
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
/// §What The Producer Writes). `bad_routes_seq` holds the `seq` of the last
/// error log for `value_routes`, so that each track gives one error line.
fn render_state<'a>(
    state: &'a StateFile,
    target: &'a str,
    bad_routes_seq: &mut Option<i64>,
) -> Version3DropFile<'a> {
    let jingle = state.jingle;
    let artist = if jingle { "" } else { state.artist.as_str() };
    let title = if jingle { "" } else { state.title.as_str() };
    // ADR 0024: routes only with the feed GUID and the track GUID.
    let guid_pair = nonempty(state.feed_guid.as_deref()).is_some()
        && nonempty(state.track_guid.as_deref()).is_some();
    let routes = match nonempty(state.value_routes.as_deref()) {
        // A jingle has no routes (ADR 0022 §Jingle Mode).
        Some(raw) if !jingle && guid_pair => match serde_json::from_str::<Vec<PaymentRoute>>(raw) {
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
    let routes_source = (!routes.is_empty()).then_some("liquidsoap-state");
    Version3DropFile {
        schema: "musicindex.nowplaying/3",
        target,
        artist,
        title,
        duration_secs: None,
        image: if jingle {
            None
        } else {
            nonempty(state.image.as_deref())
        },
        feed_guid: if jingle {
            None
        } else {
            nonempty(state.feed_guid.as_deref())
        },
        track_guid: if jingle {
            None
        } else {
            nonempty(state.track_guid.as_deref())
        },
        album: if jingle {
            None
        } else {
            nonempty(state.album.as_deref())
        },
        play_id: if jingle {
            None
        } else {
            Some(format!(
                "ls-{}-{}",
                (state.track_started * 1000.0) as i64,
                state.seq
            ))
        },
        value_routes: routes,
        value_routes_source: routes_source,
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
}
