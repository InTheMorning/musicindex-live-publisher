//! The show log of ADR 0017.
//!
//! This service appends one JSON Lines entry for each publish attempt and
//! for each display state that it sends. The log starts a new file at each
//! UTC day, `showlog-YYYY-MM-DD.jsonl`, in the state directory of the
//! instance. A crash can damage only the last line of the current file.
//!
//! [`ShowLog::append`] never returns an error to its caller. Payment routing
//! is the first duty of this service, and a log failure must never stop a
//! publish (ADR 0017 §The Log Never Stops A Publish). [`ShowLog::open`] and
//! [`ShowLog::remove_expired`] follow the same rule, except that `open`
//! fails when it cannot make or read the log directory, because the caller
//! must then decide whether to run with no log.
//!
//! No caller uses this module yet. A later task wires `track`, `clear` and
//! `display` entries into the publish loop.

use std::fs::{self, File, OpenOptions};
use std::io;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result};
use serde::Serialize;

use crate::LiveValue;

/// The schema string of each entry (ADR 0017).
pub const SHOW_LOG_SCHEMA: &str = "musicindex.showlog/1";

/// One millisecond, in milliseconds. Here for clarity at call sites.
const MILLIS_PER_SECOND: i64 = 1_000;
/// One day, in milliseconds.
const MILLIS_PER_DAY: i64 = 86_400_000;

/// The kind of a show log entry (ADR 0017 §Entries).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ShowLogEntryKind {
    /// A drop file is present. One entry for each publish attempt of each
    /// target.
    Track,
    /// The drop file is gone, and no track plays. One entry for each publish
    /// attempt of each target.
    Clear,
    /// This service read a new display state for a target with a display
    /// path. One entry for each display state that it sends to the relay.
    Display,
}

/// The outcome of a publish attempt (ADR 0017 §Entries, field
/// `publish_result`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PublishResult {
    /// The relay accepted the payload.
    Accepted,
    /// The relay gave a retryable failure.
    Retryable,
    /// The payload was dropped with no retry.
    Dropped,
    /// The failure was fatal to the publish target.
    Fatal,
}

/// The artwork reference of a `display` entry (ADR 0017 §Entries, field
/// `artwork`).
///
/// This holds a reference only. The log never stores an image (ADR 0017
/// §The Art Is A Reference).
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(untagged)]
pub enum ShowLogArtwork {
    /// An image that a client loads from its own host.
    Url {
        /// The public URL of the image.
        url: String,
    },
    /// An embedded image that went out live.
    Image {
        /// The SHA-256 of the image bytes, as 64 lowercase hexadecimal
        /// characters.
        sha256: String,
        /// The MIME type of the image, for example `image/jpeg`.
        mime: String,
    },
}

/// One entry of the show log (ADR 0017 §Entries).
///
/// A `track` entry and a `clear` entry share every field below `event_guid`.
/// A `display` entry uses only `play_id`, `artist`, `title`, `album`,
/// `song_line` and `artwork`. A field with no value is absent from the
/// JSON line, never `null`.
///
/// Build an entry with [`ShowLogEntry::new`], then set the fields that
/// apply to its `kind`.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ShowLogEntry {
    /// Always [`SHOW_LOG_SCHEMA`].
    pub schema: String,
    /// `track`, `clear` or `display`.
    pub kind: ShowLogEntryKind,
    /// The read time of this service, RFC 3339 in UTC with milliseconds.
    pub observed_at: String,
    /// The name of the publish target.
    pub target: String,
    /// The event of the target.
    pub event_guid: String,
    /// The block that this service published. For a non-payable track and
    /// for `clear`, the dead block. Absent for a `display` entry.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub block_guid: Option<String>,
    /// `true` when the drop file has value routes. Absent for a `display`
    /// entry.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub payable: Option<bool>,
    /// From the drop file or the display state, when present.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub play_id: Option<String>,
    /// From the drop file or the display state, when present.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub artist: Option<String>,
    /// From the drop file or the display state, when present.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    /// From the drop file or the display state, when present.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub album: Option<String>,
    /// From the display state, when present. Only a `display` entry carries
    /// this field.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub song_line: Option<String>,
    /// From the drop file, when present. Only a `track` or `clear` entry
    /// carries this field.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub duration_secs: Option<f64>,
    /// From the drop file, when present.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub feed_guid: Option<String>,
    /// From the drop file, when present.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub track_guid: Option<String>,
    /// The `image` of the drop file, when present.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub image_url: Option<String>,
    /// The `value` object of the published payload, exactly as sent. Only a
    /// `track` or `clear` entry carries this field.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub value: Option<LiveValue>,
    /// The live image reference of a `display` entry.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub artwork: Option<ShowLogArtwork>,
    /// The value sent in `Listener-Delay-Secs`. Only a `track` or `clear`
    /// entry carries this field.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub listener_delay_secs: Option<u64>,
    /// `accepted`, `retryable`, `dropped` or `fatal`. Only a `track` or
    /// `clear` entry carries this field.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub publish_result: Option<PublishResult>,
    /// The relay sequence number, when the relay accepted the payload.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub seq: Option<u64>,
}

impl ShowLogEntry {
    /// Builds an entry with the required fields of ADR 0017 and no optional
    /// field set.
    ///
    /// `observed_at` is the read time of this service. It is formatted with
    /// [`format_observed_at`].
    pub fn new(
        kind: ShowLogEntryKind,
        observed_at: SystemTime,
        target: impl Into<String>,
        event_guid: impl Into<String>,
    ) -> Self {
        Self {
            schema: SHOW_LOG_SCHEMA.to_owned(),
            kind,
            observed_at: format_observed_at(observed_at),
            target: target.into(),
            event_guid: event_guid.into(),
            block_guid: None,
            payable: None,
            play_id: None,
            artist: None,
            title: None,
            album: None,
            song_line: None,
            duration_secs: None,
            feed_guid: None,
            track_guid: None,
            image_url: None,
            value: None,
            artwork: None,
            listener_delay_secs: None,
            publish_result: None,
            seq: None,
        }
    }
}

/// Formats `time` as RFC 3339 in UTC with milliseconds, for example
/// `2026-10-19T01:03:23.266Z` (ADR 0017 §The Time Source).
///
/// A time before the Unix epoch gives the epoch string. This service never
/// reads a clock that is that far wrong, and the fallback avoids a panic.
pub fn format_observed_at(time: SystemTime) -> String {
    let millis = millis_since_epoch(time);
    let days = millis.div_euclid(MILLIS_PER_DAY);
    let millis_of_day = millis.rem_euclid(MILLIS_PER_DAY);
    let (year, month, day) = civil_from_days(days);

    let hours = millis_of_day / 3_600_000;
    let minutes = (millis_of_day / 60_000) % 60;
    let seconds = (millis_of_day / MILLIS_PER_SECOND) % 60;
    let millis_part = millis_of_day % MILLIS_PER_SECOND;

    format!("{year:04}-{month:02}-{day:02}T{hours:02}:{minutes:02}:{seconds:02}.{millis_part:03}Z")
}

/// Milliseconds since the Unix epoch, floored at zero for an earlier time.
fn millis_since_epoch(time: SystemTime) -> i64 {
    match time.duration_since(UNIX_EPOCH) {
        Ok(duration) => i64::try_from(duration.as_millis()).unwrap_or(i64::MAX),
        Err(_) => 0,
    }
}

/// Days since the Unix epoch, floored at the day that holds `time`.
fn epoch_days(time: SystemTime) -> i64 {
    millis_since_epoch(time).div_euclid(MILLIS_PER_DAY)
}

/// The civil calendar date of days-since-epoch `days`.
///
/// This is Howard Hinnant's `civil_from_days` algorithm for the proleptic
/// Gregorian calendar. It needs no date-time dependency.
fn civil_from_days(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097; // [0, 146096]
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365; // [0, 399]
    let year = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100); // [0, 365]
    let mp = (5 * doy + 2) / 153; // [0, 11]
    #[allow(clippy::cast_sign_loss)]
    let day = (doy - (153 * mp + 2) / 5 + 1) as u32; // [1, 31]
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = if month <= 2 { year + 1 } else { year };
    #[allow(clippy::cast_sign_loss)]
    (year, month as u32, day)
}

/// The inverse of [`civil_from_days`]: days-since-epoch of a civil date.
fn days_from_civil(year: i64, month: u32, day: u32) -> i64 {
    let month = i64::from(month);
    let day = i64::from(day);
    let year = if month <= 2 { year - 1 } else { year };
    let era = if year >= 0 { year } else { year - 399 } / 400;
    let yoe = year - era * 400; // [0, 399]
    let mp = (month + 9) % 12; // [0, 11]
    let doy = (153 * mp + 2) / 5 + day - 1; // [0, 365]
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy; // [0, 146096]
    era * 146_097 + doe - 719_468
}

/// The first 10 characters of an [`format_observed_at`] string, for example
/// `2026-10-19`.
///
/// Gives `None` when `observed_at` is too short or does not have a `-` at
/// the positions a formatted date always has. The caller then cannot name a
/// daily file for the entry.
fn date_prefix(observed_at: &str) -> Option<&str> {
    let bytes = observed_at.as_bytes();
    if bytes.len() < 10 || bytes[4] != b'-' || bytes[7] != b'-' {
        return None;
    }
    Some(&observed_at[..10])
}

/// The file name of the daily log for UTC date `date`, for example
/// `2026-10-19`.
fn log_file_name(date: &str) -> String {
    format!("showlog-{date}.jsonl")
}

/// The civil date of a daily log file name, when `file_name` has the shape
/// `showlog-YYYY-MM-DD.jsonl` with a plausible month and day.
///
/// Gives `None` for any other name, so that [`ShowLog::remove_expired`]
/// leaves every other file in the directory alone.
fn parse_log_file_date(file_name: &str) -> Option<(i64, u32, u32)> {
    let date = file_name.strip_prefix("showlog-")?.strip_suffix(".jsonl")?;
    let mut parts = date.split('-');
    let year = parts.next()?.parse::<i64>().ok()?;
    let month = parts.next()?.parse::<u32>().ok()?;
    let day = parts.next()?.parse::<u32>().ok()?;
    if parts.next().is_some() {
        return None;
    }
    if !(1..=12).contains(&month) || !(1..=31).contains(&day) {
        return None;
    }
    Some((year, month, day))
}

/// The open file of a [`ShowLog`], with the UTC date it was opened for.
struct OpenLogFile {
    date: String,
    file: File,
}

/// The append-only show log writer of ADR 0017.
///
/// `ShowLog` is `Send` and `Sync`. A [`Mutex`] around the open file lets
/// several worker threads append at once, with no line split between two
/// writers. [`ShowLog::append`] opens a new file when the UTC date of an
/// entry is not the date of the open file, and closes the old one.
pub struct ShowLog {
    dir: PathBuf,
    retention_days: u32,
    open_file: Mutex<Option<OpenLogFile>>,
}

impl ShowLog {
    /// Opens the show log directory `dir`, with a retention of
    /// `retention_days`.
    ///
    /// Creates `dir` with mode `0600`'s directory counterpart, `0700`, if it
    /// is absent. Opens no file yet; [`ShowLog::append`] opens the daily
    /// file that the first entry needs.
    ///
    /// # Errors
    ///
    /// Returns an error only when `dir` cannot be made or is not a readable
    /// directory. The caller decides what to do with that error, for
    /// example running with no show log.
    pub fn open(dir: &Path, retention_days: u32) -> Result<Self> {
        create_dir_if_absent(dir)
            .with_context(|| format!("create show log directory {}", dir.display()))?;
        Ok(Self {
            dir: dir.to_path_buf(),
            retention_days,
            open_file: Mutex::new(None),
        })
    }

    /// Appends one line, `entry` and a newline, to the daily file of the
    /// UTC date of `entry.observed_at`.
    ///
    /// Opens `showlog-YYYY-MM-DD.jsonl` in append mode, and creates it with
    /// mode `0600` if it is absent. Writes the line in one `write_all`
    /// call, then flushes.
    ///
    /// Never returns an error to the caller (ADR 0017 §The Log Never Stops
    /// A Publish). On a failure, this logs one `tracing::warn!` with the
    /// path and the error, and drops the entry.
    pub fn append(&self, entry: &ShowLogEntry) {
        let Some(date) = date_prefix(&entry.observed_at) else {
            tracing::warn!(
                observed_at = %entry.observed_at,
                "show log entry has no UTC date; dropping it"
            );
            return;
        };
        let mut line = match serde_json::to_vec(entry) {
            Ok(bytes) => bytes,
            Err(error) => {
                tracing::warn!(%error, "cannot serialize show log entry; dropping it");
                return;
            }
        };
        line.push(b'\n');

        let path = self.dir.join(log_file_name(date));
        let mut guard = self
            .open_file
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);

        let needs_new_file = !matches!(guard.as_ref(), Some(open) if open.date == date);
        if needs_new_file {
            match open_log_file(&path) {
                Ok(file) => {
                    *guard = Some(OpenLogFile {
                        date: date.to_owned(),
                        file,
                    });
                }
                Err(error) => {
                    tracing::warn!(
                        path = %path.display(),
                        %error,
                        "cannot open show log file; dropping entry"
                    );
                    return;
                }
            }
        }

        let Some(open) = guard.as_mut() else {
            // Unreachable: the branch above always fills the guard or
            // returns. Kept for safety against a future change above.
            return;
        };
        if let Err(error) = write_line(&mut open.file, &line) {
            tracing::warn!(
                path = %path.display(),
                %error,
                "cannot write show log entry; dropping it"
            );
            // Drop the handle so that the next append retries opening it.
            *guard = None;
        }
    }

    /// Removes each daily log file whose date is more than
    /// `self.retention_days` before the UTC date of `now`.
    ///
    /// Removes no file whose name is not `showlog-YYYY-MM-DD.jsonl`. Warns
    /// on an error for one file or for the directory listing, and
    /// continues with the rest.
    pub fn remove_expired(&self, now: SystemTime) {
        let today = epoch_days(now);
        let entries = match fs::read_dir(&self.dir) {
            Ok(entries) => entries,
            Err(error) => {
                tracing::warn!(
                    dir = %self.dir.display(),
                    %error,
                    "cannot list show log directory; skipping retention"
                );
                return;
            }
        };
        for entry in entries {
            let entry = match entry {
                Ok(entry) => entry,
                Err(error) => {
                    tracing::warn!(
                        dir = %self.dir.display(),
                        %error,
                        "cannot read a show log directory entry; skipping it"
                    );
                    continue;
                }
            };
            let Some(file_name) = entry.file_name().to_str().map(str::to_owned) else {
                continue;
            };
            let Some((year, month, day)) = parse_log_file_date(&file_name) else {
                continue;
            };
            let age_days = today - days_from_civil(year, month, day);
            if age_days > i64::from(self.retention_days) {
                let path = entry.path();
                if let Err(error) = fs::remove_file(&path) {
                    tracing::warn!(
                        path = %path.display(),
                        %error,
                        "cannot remove an expired show log file"
                    );
                }
            }
        }
    }
}

/// Writes `line` whole, with one `write_all` call, then flushes.
fn write_line(file: &mut File, line: &[u8]) -> io::Result<()> {
    use std::io::Write;

    file.write_all(line)?;
    file.flush()
}

/// Opens or creates the daily log file at `path`, in append mode with mode
/// `0600`.
fn open_log_file(path: &Path) -> io::Result<File> {
    let mut options = OpenOptions::new();
    options.create(true).append(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let file = options.open(path)?;
    // `OpenOptionsExt::mode` is reduced by the process umask, so this also
    // sets the mode directly to guarantee `0600` whatever the umask is.
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        file.set_permissions(fs::Permissions::from_mode(0o600))?;
    }
    Ok(file)
}

/// Creates `dir` with mode `0700` if it is absent. Leaves an existing
/// directory's mode as it is.
fn create_dir_if_absent(dir: &Path) -> io::Result<()> {
    if dir.is_dir() {
        return Ok(());
    }
    fs::create_dir_all(dir)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(dir, fs::Permissions::from_mode(0o700))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::thread;
    use std::time::Duration;

    use anyhow::Result;
    use serde_json::Value;

    use super::*;

    /// 2026-10-19T01:03:23.266Z, used across several tests as a fixed time.
    const SAMPLE_TIME_SECS: u64 = 1_792_371_803;
    const SAMPLE_TIME_MILLIS: u32 = 266_000_000;

    fn sample_time() -> SystemTime {
        UNIX_EPOCH + Duration::new(SAMPLE_TIME_SECS, SAMPLE_TIME_MILLIS)
    }

    fn read_lines(path: &Path) -> Result<Vec<String>> {
        let text = fs::read_to_string(path)?;
        Ok(text.lines().map(str::to_owned).collect())
    }

    fn jsonl_files(dir: &Path) -> Result<Vec<String>> {
        let mut names = Vec::new();
        for entry in fs::read_dir(dir)? {
            let entry = entry?;
            if let Some(name) = entry.file_name().to_str() {
                names.push(name.to_owned());
            }
        }
        names.sort();
        Ok(names)
    }

    #[test]
    fn format_observed_at_gives_the_epoch_string() {
        assert_eq!(format_observed_at(UNIX_EPOCH), "1970-01-01T00:00:00.000Z");
    }

    #[test]
    fn format_observed_at_gives_the_leap_day_string() {
        let time = UNIX_EPOCH + Duration::new(951_868_799, 999_000_000);
        assert_eq!(format_observed_at(time), "2000-02-29T23:59:59.999Z");
    }

    #[test]
    fn format_observed_at_gives_the_sample_time_string() {
        assert_eq!(
            format_observed_at(sample_time()),
            "2026-10-19T01:03:23.266Z"
        );
    }

    #[test]
    fn a_track_entry_serializes_with_schema_kind_and_no_absent_key() -> Result<()> {
        let entry = ShowLogEntry::new(ShowLogEntryKind::Track, sample_time(), "default", "event");

        let value = serde_json::to_value(&entry)?;

        assert_eq!(value["schema"], SHOW_LOG_SCHEMA);
        assert_eq!(value["kind"], "track");
        assert_eq!(value["target"], "default");
        assert_eq!(value["event_guid"], "event");
        assert!(value.get("block_guid").is_none());
        assert!(value.get("payable").is_none());
        assert!(value.get("play_id").is_none());
        assert!(value.get("artist").is_none());
        assert!(value.get("album").is_none());
        assert!(value.get("value").is_none());
        assert!(value.get("publish_result").is_none());
        assert!(value.get("seq").is_none());
        Ok(())
    }

    #[test]
    fn a_display_entry_with_no_track_has_no_artist_title_or_artwork_key() -> Result<()> {
        let entry = ShowLogEntry::new(ShowLogEntryKind::Display, sample_time(), "default", "event");

        let value = serde_json::to_value(&entry)?;

        assert_eq!(value["kind"], "display");
        assert!(value.get("artist").is_none());
        assert!(value.get("title").is_none());
        assert!(value.get("artwork").is_none());
        Ok(())
    }

    #[test]
    fn two_entries_of_one_day_go_into_one_file_as_two_parsing_lines() -> Result<()> {
        let dir = tempfile::TempDir::new()?;
        let log = ShowLog::open(dir.path(), 365)?;
        let first = ShowLogEntry::new(ShowLogEntryKind::Track, sample_time(), "default", "event");
        let second = ShowLogEntry::new(ShowLogEntryKind::Clear, sample_time(), "default", "event");

        log.append(&first);
        log.append(&second);

        let files = jsonl_files(dir.path())?;
        assert_eq!(files, vec!["showlog-2026-10-19.jsonl"]);
        let lines = read_lines(&dir.path().join(&files[0]))?;
        assert_eq!(lines.len(), 2);
        for line in &lines {
            let _value: Value = serde_json::from_str(line)?;
        }
        Ok(())
    }

    #[test]
    fn an_entry_of_the_next_day_goes_into_a_second_file() -> Result<()> {
        let dir = tempfile::TempDir::new()?;
        let log = ShowLog::open(dir.path(), 365)?;
        let first = ShowLogEntry::new(ShowLogEntryKind::Track, sample_time(), "default", "event");
        let next_day = sample_time() + Duration::from_secs(24 * 60 * 60);
        let second = ShowLogEntry::new(ShowLogEntryKind::Track, next_day, "default", "event");

        log.append(&first);
        log.append(&second);

        let files = jsonl_files(dir.path())?;
        assert_eq!(
            files,
            vec!["showlog-2026-10-19.jsonl", "showlog-2026-10-20.jsonl"]
        );
        assert_eq!(read_lines(&dir.path().join(&files[0]))?.len(), 1);
        assert_eq!(read_lines(&dir.path().join(&files[1]))?.len(), 1);
        Ok(())
    }

    #[cfg(unix)]
    #[test]
    fn the_file_mode_is_0600_and_the_directory_mode_is_0700() -> Result<()> {
        use std::os::unix::fs::PermissionsExt;

        let parent = tempfile::TempDir::new()?;
        // `ShowLog::open` only sets the mode of a directory that it creates
        // itself, so the log directory must not already exist.
        let dir = parent.path().join("showlog-state");
        let log = ShowLog::open(&dir, 365)?;
        let entry = ShowLogEntry::new(ShowLogEntryKind::Track, sample_time(), "default", "event");
        log.append(&entry);

        let dir_mode = fs::metadata(&dir)?.permissions().mode() & 0o777;
        assert_eq!(dir_mode, 0o700);

        let files = jsonl_files(&dir)?;
        let file_mode = fs::metadata(dir.join(&files[0]))?.permissions().mode() & 0o777;
        assert_eq!(file_mode, 0o600);
        Ok(())
    }

    #[test]
    fn two_threads_appending_100_entries_each_give_200_parsing_lines() -> Result<()> {
        let dir = tempfile::TempDir::new()?;
        let log = Arc::new(ShowLog::open(dir.path(), 365)?);

        let spawn_writer = |log: Arc<ShowLog>, target: &'static str| {
            thread::spawn(move || {
                for seq in 0..100u64 {
                    let mut entry =
                        ShowLogEntry::new(ShowLogEntryKind::Track, sample_time(), target, "event");
                    entry.seq = Some(seq);
                    log.append(&entry);
                }
            })
        };
        let first = spawn_writer(Arc::clone(&log), "one");
        let second = spawn_writer(Arc::clone(&log), "two");
        first.join().expect("writer thread one panicked");
        second.join().expect("writer thread two panicked");

        let files = jsonl_files(dir.path())?;
        assert_eq!(files, vec!["showlog-2026-10-19.jsonl"]);
        let lines = read_lines(&dir.path().join(&files[0]))?;
        assert_eq!(lines.len(), 200);
        for line in &lines {
            let _value: Value = serde_json::from_str(line)?;
        }
        Ok(())
    }

    #[test]
    fn remove_expired_removes_only_the_file_older_than_the_retention() -> Result<()> {
        let dir = tempfile::TempDir::new()?;
        let log = ShowLog::open(dir.path(), 2)?;
        let now = sample_time();
        let today = epoch_days(now);

        let name_for_offset = |days_before: i64| {
            let (year, month, day) = civil_from_days(today - days_before);
            log_file_name(&format!("{year:04}-{month:02}-{day:02}"))
        };
        let three_days_before = name_for_offset(3);
        let two_days_before = name_for_offset(2);
        let today_file = name_for_offset(0);
        let other_file = "other.txt";

        for name in [&three_days_before, &two_days_before, &today_file] {
            fs::write(dir.path().join(name), "{}\n")?;
        }
        fs::write(dir.path().join(other_file), "not a show log file")?;

        log.remove_expired(now);

        let remaining = jsonl_files(dir.path())?;
        assert!(!remaining.contains(&three_days_before));
        assert!(remaining.contains(&two_days_before));
        assert!(remaining.contains(&today_file));
        assert!(remaining.contains(&other_file.to_owned()));
        Ok(())
    }

    #[test]
    fn append_into_a_read_only_directory_returns_with_no_panic() -> Result<()> {
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;

            let dir = tempfile::TempDir::new()?;
            let log = ShowLog::open(dir.path(), 365)?;
            fs::set_permissions(dir.path(), fs::Permissions::from_mode(0o500))?;

            let entry =
                ShowLogEntry::new(ShowLogEntryKind::Track, sample_time(), "default", "event");
            log.append(&entry);

            // Restore write access so the temporary directory can be removed.
            fs::set_permissions(dir.path(), fs::Permissions::from_mode(0o700))?;
            let files = jsonl_files(dir.path())?;
            assert!(files.is_empty());
        }
        Ok(())
    }
}
