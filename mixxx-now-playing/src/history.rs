use std::ffi::{CStr, CString};
use std::path::{Path, PathBuf};
use std::ptr::NonNull;
use std::thread;
use std::time::Duration;

use anyhow::{Context, Result, anyhow};
use rusqlite::ffi;

// The location lookup is deliberately a correlated subquery over the LIMIT 1
// result rather than a join in the main body. Joining track_locations inside the
// scan resolves a location for every history row before the sort discards all
// but one: on a 30k-track library that measured 10.9 ms per poll versus 2.8 ms
// here, for identical results.
const HISTORY_QUERY: &str = "\
SELECT pt_id, artist, title,
       (SELECT tl.location FROM track_locations tl WHERE tl.id = loc)
FROM (
  SELECT pt.id AS pt_id, l.artist AS artist, l.title AS title, l.location AS loc
  FROM PlaylistTracks pt
  JOIN Playlists p  ON p.id = pt.playlist_id
  JOIN library l    ON l.id = pt.track_id
  WHERE p.hidden = 2
  ORDER BY pt.pl_datetime_added DESC, pt.id DESC LIMIT 1
)";

// Cheap gate in front of the history query. `PRAGMA data_version` changes only
// when another connection commits, and costs ~0.008 ms against ~2.8 ms for the
// query itself, so an idle Mixxx makes polling almost free.
const DATA_VERSION_QUERY: &str = "PRAGMA data_version";

const BUSY_RETRY_DELAY: Duration = Duration::from_millis(50);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TrackRow {
    pub hist_id: i64,
    pub artist: String,
    pub title: String,
    pub path: PathBuf,
}

#[derive(Debug)]
pub struct HistoryWatcher {
    // Declaration order is load-bearing: fields drop in order, so every
    // statement must be finalized before the connection it belongs to is
    // closed. Do not move `connection` above these.
    statement: StatementHandle,
    data_version: StatementHandle,
    connection: ConnectionHandle,
    last_hist_id: Option<i64>,
    last_data_version: Option<i64>,
}

impl HistoryWatcher {
    pub fn open(db_path: &Path) -> Result<Self> {
        let connection = ConnectionHandle::open_read_only(db_path)?;
        connection
            .exec("PRAGMA query_only = ON")
            .context("enable sqlite query_only mode")?;
        let statement = connection
            .prepare(HISTORY_QUERY)
            .context("prepare Mixxx history query")?;
        let data_version = connection
            .prepare(DATA_VERSION_QUERY)
            .context("prepare sqlite data_version query")?;

        Ok(Self {
            statement,
            data_version,
            connection,
            last_hist_id: None,
            last_data_version: None,
        })
    }

    pub fn poll(&mut self) -> Result<Option<TrackRow>> {
        if !self.database_changed() {
            return Ok(None);
        }

        loop {
            match self.statement.step() {
                ffi::SQLITE_ROW => {
                    let row = self.read_current_row();
                    self.statement.reset().context("reset history statement")?;
                    let row = row?;
                    if Some(row.hist_id) == self.last_hist_id {
                        return Ok(None);
                    }
                    self.last_hist_id = Some(row.hist_id);
                    return Ok(Some(row));
                }
                ffi::SQLITE_DONE => {
                    self.statement.reset().context("reset history statement")?;
                    return Ok(None);
                }
                ffi::SQLITE_BUSY | ffi::SQLITE_LOCKED => {
                    self.statement
                        .reset_after_busy()
                        .context("reset busy history statement")?;
                    thread::sleep(BUSY_RETRY_DELAY);
                }
                code => {
                    let message = self.connection.error_message();
                    let _ = self.statement.reset();
                    return Err(anyhow!("query Mixxx history failed: {message} ({code})"));
                }
            }
        }
    }

    /// Returns false when the database provably has not changed since the last
    /// poll, so the caller can skip the history query entirely.
    ///
    /// Any uncertainty — a busy database, an unexpected sqlite result — reports
    /// true, because missing a track change is far worse than one wasted query.
    fn database_changed(&mut self) -> bool {
        let version = match self.data_version.step() {
            ffi::SQLITE_ROW => {
                let version = self.data_version.column_i64(0);
                let _ = self.data_version.reset();
                Some(version)
            }
            _ => {
                let _ = self.data_version.reset();
                None
            }
        };

        let Some(version) = version else {
            self.last_data_version = None;
            return true;
        };

        let changed = self.last_data_version != Some(version);
        self.last_data_version = Some(version);
        changed
    }

    fn read_current_row(&self) -> Result<TrackRow> {
        Ok(TrackRow {
            hist_id: self.statement.column_i64(0),
            artist: self.statement.column_text_or_empty(1),
            title: self.statement.column_text_or_empty(2),
            path: PathBuf::from(self.statement.column_text_or_empty(3)),
        })
    }
}

#[derive(Debug)]
struct ConnectionHandle {
    db: NonNull<ffi::sqlite3>,
}

impl ConnectionHandle {
    fn open_read_only(db_path: &Path) -> Result<Self> {
        let uri = sqlite_uri(db_path)?;
        let uri = CString::new(uri).context("build sqlite URI")?;
        let mut db = std::ptr::null_mut();
        let flags = ffi::SQLITE_OPEN_READONLY | ffi::SQLITE_OPEN_URI;

        // SAFETY: sqlite3_open_v2 initializes `db` for the provided NUL-terminated URI.
        let code = unsafe { ffi::sqlite3_open_v2(uri.as_ptr(), &mut db, flags, std::ptr::null()) };
        let Some(db) = NonNull::new(db) else {
            return Err(anyhow!("open Mixxx database returned a null sqlite handle"));
        };

        if code != ffi::SQLITE_OK {
            let message = sqlite_error_message(db);
            // SAFETY: `db` came from sqlite3_open_v2 and must be closed on open failure.
            unsafe {
                let _ = ffi::sqlite3_close(db.as_ptr());
            }
            return Err(anyhow!("open Mixxx database read-only: {message} ({code})"));
        }

        Ok(Self { db })
    }

    fn exec(&self, sql: &str) -> Result<()> {
        let sql = CString::new(sql).context("build sqlite statement")?;
        let mut error = std::ptr::null_mut();
        // SAFETY: `self.db` is an open sqlite handle and `sql` is NUL-terminated.
        let code = unsafe {
            ffi::sqlite3_exec(
                self.db.as_ptr(),
                sql.as_ptr(),
                None,
                std::ptr::null_mut(),
                &mut error,
            )
        };
        if code == ffi::SQLITE_OK {
            return Ok(());
        }

        let message = if error.is_null() {
            self.error_message()
        } else {
            // SAFETY: sqlite returns a valid NUL-terminated error string when `error` is non-null.
            let message = unsafe { CStr::from_ptr(error) }
                .to_string_lossy()
                .into_owned();
            // SAFETY: error strings from sqlite3_exec must be released with sqlite3_free.
            unsafe {
                ffi::sqlite3_free(error.cast());
            }
            message
        };
        Err(anyhow!(
            "execute sqlite statement failed: {message} ({code})"
        ))
    }

    fn prepare(&self, sql: &str) -> Result<StatementHandle> {
        let sql = CString::new(sql).context("build sqlite query")?;
        let mut stmt = std::ptr::null_mut();
        // SAFETY: `self.db` is open and `sql` is a valid NUL-terminated query.
        let code = unsafe {
            ffi::sqlite3_prepare_v2(
                self.db.as_ptr(),
                sql.as_ptr(),
                -1,
                &mut stmt,
                std::ptr::null_mut(),
            )
        };
        if code != ffi::SQLITE_OK {
            return Err(anyhow!(
                "prepare sqlite query failed: {} ({code})",
                self.error_message()
            ));
        }
        let stmt = NonNull::new(stmt).ok_or_else(|| anyhow!("sqlite prepared a null statement"))?;
        Ok(StatementHandle { stmt })
    }

    fn error_message(&self) -> String {
        sqlite_error_message(self.db)
    }
}

impl Drop for ConnectionHandle {
    fn drop(&mut self) {
        // SAFETY: `db` is owned by this handle and closed exactly once here.
        unsafe {
            let _ = ffi::sqlite3_close(self.db.as_ptr());
        }
    }
}

#[derive(Debug)]
struct StatementHandle {
    stmt: NonNull<ffi::sqlite3_stmt>,
}

impl StatementHandle {
    fn step(&self) -> i32 {
        // SAFETY: `stmt` is a live prepared statement owned by this handle.
        unsafe { ffi::sqlite3_step(self.stmt.as_ptr()) }
    }

    fn reset(&self) -> Result<()> {
        // SAFETY: `stmt` is a live prepared statement owned by this handle.
        let code = unsafe { ffi::sqlite3_reset(self.stmt.as_ptr()) };
        match code {
            ffi::SQLITE_OK => Ok(()),
            _ => Err(anyhow!("sqlite statement reset failed ({code})")),
        }
    }

    fn reset_after_busy(&self) -> Result<()> {
        // SAFETY: `stmt` is a live prepared statement owned by this handle.
        let code = unsafe { ffi::sqlite3_reset(self.stmt.as_ptr()) };
        match code {
            ffi::SQLITE_OK | ffi::SQLITE_BUSY | ffi::SQLITE_LOCKED => Ok(()),
            _ => Err(anyhow!("sqlite statement reset after busy failed ({code})")),
        }
    }

    fn column_i64(&self, index: i32) -> i64 {
        // SAFETY: caller invokes this only while sqlite3_step has yielded SQLITE_ROW.
        unsafe { ffi::sqlite3_column_int64(self.stmt.as_ptr(), index) }
    }

    fn column_text_or_empty(&self, index: i32) -> String {
        // SAFETY: caller invokes this only while sqlite3_step has yielded SQLITE_ROW.
        let ptr = unsafe { ffi::sqlite3_column_text(self.stmt.as_ptr(), index) };
        if ptr.is_null() {
            return String::new();
        }

        // SAFETY: sqlite exposes a valid byte slice for the current row until reset/finalize.
        let len = unsafe { ffi::sqlite3_column_bytes(self.stmt.as_ptr(), index) };
        if len <= 0 {
            return String::new();
        }
        let len = len as usize;
        // SAFETY: `ptr` points to at least `len` bytes for the current sqlite row.
        let bytes = unsafe { std::slice::from_raw_parts(ptr, len) };
        String::from_utf8_lossy(bytes).into_owned()
    }
}

impl Drop for StatementHandle {
    fn drop(&mut self) {
        // SAFETY: `stmt` is owned by this handle and finalized exactly once here.
        unsafe {
            let _ = ffi::sqlite3_finalize(self.stmt.as_ptr());
        }
    }
}

fn sqlite_error_message(db: NonNull<ffi::sqlite3>) -> String {
    // SAFETY: `db` is a sqlite handle and sqlite3_errmsg returns a NUL-terminated string.
    unsafe { CStr::from_ptr(ffi::sqlite3_errmsg(db.as_ptr())) }
        .to_string_lossy()
        .into_owned()
}

fn sqlite_uri(path: &Path) -> Result<String> {
    let path = path
        .to_str()
        .ok_or_else(|| anyhow!("Mixxx database path is not valid UTF-8"))?;
    Ok(format!("file:{}?mode=ro", percent_encode_path(path)))
}

fn percent_encode_path(path: &str) -> String {
    let mut encoded = String::with_capacity(path.len());
    for byte in path.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' | b'/' => {
                encoded.push(char::from(byte));
            }
            _ => {
                encoded.push('%');
                encoded.push_str(&format!("{byte:02X}"));
            }
        }
    }
    encoded
}
