# Task 002 — Mixxx history source and test harness

Part of [Rust now-playing utility plan](../plans/rust-now-playing-utility-plan.md), Phase 1.

## Goal

Read the current track from the Mixxx set log and emit a change event only when
the track actually changes. Build the synthetic-database test harness that every
later task depends on.

## Files To Inspect

- `docs/plans/rust-now-playing-utility-plan.md` — Concern 1, Test Strategy
- `scripts/mixxx-now-playing.sh` — the working query and its polling behaviour
- Task 001 output: `mixxx-now-playing/src/config.rs`

## Files Likely To Change

- `mixxx-now-playing/src/history.rs` (new)
- `mixxx-now-playing/src/main.rs`
- `mixxx-now-playing/tests/common/mod.rs` (new — synthetic DB builder)

## Do Not Touch

- The shell and Python scripts at repo root
- `docs/plans/`
- Task 001's config precedence logic

## Constraints

- Open the database **read-only**: `file:...?mode=ro` URI plus `PRAGMA query_only`.
- Open the connection **once** and keep it. Do not reconnect per poll — the Python
  version reconnects every 500ms and that is one of the defects being fixed.
- Prepare the statement once and reuse it.
- On `SQLITE_BUSY`, retry rather than returning an error or exiting. Mixxx holds a
  write lock during set-log inserts.
- Do not probe the schema. It is pinned to Mixxx 2.5.6. Write the query literally.

## Implementation Steps

1. Define `TrackRow { hist_id: i64, artist: String, title: String, path: PathBuf }`.
2. Implement `HistoryWatcher` holding the connection, the prepared statement, and
   `last_hist_id: Option<i64>`.
3. Use exactly this query:

   ```sql
   SELECT pt.id, l.artist, l.title, tl.location
   FROM PlaylistTracks pt
   JOIN Playlists p  ON p.id = pt.playlist_id
   JOIN library l    ON l.id = pt.track_id
   JOIN track_locations tl ON tl.id = l.location
   WHERE p.hidden = 2
   ORDER BY pt.pl_datetime_added DESC, pt.id DESC LIMIT 1
   ```

4. `poll()` returns `Option<TrackRow>` — `Some` only when `hist_id` differs from
   `last_hist_id`. Update `last_hist_id` on emission.
5. Treat NULL `artist` and `title` as empty strings.
6. Build `tests/common/mod.rs` with a helper that creates a synthetic database in
   a `tempfile::TempDir`: tables `track_locations`, `library`, `Playlists`
   (`hidden = 2`), `PlaylistTracks`, plus a function to append a history row for a
   given file path.

## Acceptance Criteria

- Repeated `poll()` calls with no new history row return `None` after the first.
- Appending a history row makes the next `poll()` return `Some` with the correct
  path.
- An empty history table returns `None` rather than erroring.
- Only one sqlite connection is opened for the lifetime of the watcher — assert
  by construction, not by counting file descriptors.
- The harness in `tests/common/mod.rs` is usable by later tasks without change.

## Test Commands

```bash
cd mixxx-now-playing
cargo clippy -- -D warnings
cargo test history
```

## Expected Final Report

- Files created or changed
- Test names and results
- The exact schema DDL used in the harness
- Whether `SQLITE_BUSY` retry was exercised, and how

## Escalation Triggers

Stop and ask before proceeding if:

- The query returns no rows against a real `~/.mixxx/mixxxdb.sqlite` that has
  history — the join may need the `library.location` indirection re-checked.
- `track_locations.id` does not match `library.location` in the real database.
- Any step appears to require reading tags or writing output files.
