use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use rusqlite::{Connection, params};
use tempfile::TempDir;

pub const SYNTHETIC_MIXXX_SCHEMA: &str = "\
CREATE TABLE track_locations (
    id INTEGER PRIMARY KEY,
    location TEXT
);
CREATE TABLE library (
    id INTEGER PRIMARY KEY,
    artist TEXT,
    title TEXT,
    location INTEGER
);
CREATE TABLE Playlists (
    id INTEGER PRIMARY KEY,
    hidden INTEGER
);
CREATE TABLE PlaylistTracks (
    id INTEGER PRIMARY KEY,
    playlist_id INTEGER,
    track_id INTEGER,
    pl_datetime_added TEXT
);";

pub struct SyntheticMixxxDb {
    _temp: TempDir,
    db_path: PathBuf,
    conn: Connection,
    next_timestamp: i64,
}

impl SyntheticMixxxDb {
    pub fn new() -> Result<Self> {
        let temp = TempDir::new().context("create temporary Mixxx database directory")?;
        let db_path = temp.path().join("mixxxdb.sqlite");
        let conn = Connection::open(&db_path)
            .with_context(|| format!("create synthetic database {}", db_path.display()))?;
        conn.execute_batch(SYNTHETIC_MIXXX_SCHEMA)
            .context("create synthetic Mixxx schema")?;
        conn.execute("INSERT INTO Playlists (id, hidden) VALUES (1, 2)", [])?;

        Ok(Self {
            _temp: temp,
            db_path,
            conn,
            next_timestamp: 1,
        })
    }

    pub fn path(&self) -> &Path {
        &self.db_path
    }

    pub fn append_history_row_with_metadata(
        &mut self,
        artist: Option<&str>,
        title: Option<&str>,
        path: &Path,
    ) -> Result<i64> {
        let tx = self.conn.transaction()?;
        let location = path.display().to_string();
        tx.execute(
            "INSERT INTO track_locations (location) VALUES (?1)",
            params![location],
        )?;
        let location_id = tx.last_insert_rowid();
        tx.execute(
            "INSERT INTO library (artist, title, location) VALUES (?1, ?2, ?3)",
            params![artist, title, location_id],
        )?;
        let track_id = tx.last_insert_rowid();
        let timestamp = format!("{:020}", self.next_timestamp);
        self.next_timestamp += 1;
        tx.execute(
            "INSERT INTO PlaylistTracks (playlist_id, track_id, pl_datetime_added) VALUES (1, ?1, ?2)",
            params![track_id, timestamp],
        )?;
        let hist_id = tx.last_insert_rowid();
        tx.commit()?;
        Ok(hist_id)
    }

    /// Appends a history row whose `library.location` does not resolve.
    ///
    /// Pass `None` for a NULL location, or `Some(id)` for a dangling foreign key
    /// pointing at a `track_locations` row that does not exist. Mixxx produces
    /// both when a file is removed from disk while it stays in the library.
    // Shared across test binaries; only tests/history.rs exercises this one.
    #[allow(dead_code)]
    pub fn append_history_row_without_location(
        &mut self,
        artist: Option<&str>,
        title: Option<&str>,
        location_id: Option<i64>,
    ) -> Result<i64> {
        let tx = self.conn.transaction()?;
        tx.execute(
            "INSERT INTO library (artist, title, location) VALUES (?1, ?2, ?3)",
            params![artist, title, location_id],
        )?;
        let track_id = tx.last_insert_rowid();
        let timestamp = format!("{:020}", self.next_timestamp);
        self.next_timestamp += 1;
        tx.execute(
            "INSERT INTO PlaylistTracks (playlist_id, track_id, pl_datetime_added) VALUES (1, ?1, ?2)",
            params![track_id, timestamp],
        )?;
        let hist_id = tx.last_insert_rowid();
        tx.commit()?;
        Ok(hist_id)
    }
}
