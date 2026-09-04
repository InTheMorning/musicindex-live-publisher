# Audit Fix 003 — History Query Must Not Skip Tracks Without Locations

Remediates finding 3 of the
[audit review](../reviews/nowplaying-publisher-audit-review.md).

## Goal

Restore parity with `scripts/mixxx-now-playing.sh`: the latest history row must always
be reported, even when its `library.location` is NULL or points at a missing
`track_locations` row. A track with no resolvable path is simply not a V4V
track — it is not a reason to display the previous song.

## Files To Inspect

- `mixxx-now-playing/src/history.rs:10-17` — `HISTORY_QUERY`; the offending
  inner join is line `:15`.
- `scripts/mixxx-now-playing.sh:11-20` — the parity reference. It joins only
  `Playlists` and `library`, and uses `IFNULL` for artist and title.
- `mixxx-now-playing/src/main.rs:168-176` — `process_track`, where the path is
  handed to `is_v4v_track`.
- `mixxx-now-playing/src/classify.rs:3-11` — `is_v4v_track` already returns
  `false` when `canonicalize` fails, so an empty path degrades correctly.
- `mixxx-now-playing/tests/history.rs` and `tests/common/mod.rs` — the synthetic
  database harness.

## Files Likely To Change

- `mixxx-now-playing/src/history.rs`
- `mixxx-now-playing/tests/history.rs`

## Do-Not-Touch List

- `~/build/v4vmm/` and `~/build/splitkit/` — read only, never edit.
- `musicindex-live-publisher/` — producer-side only.
- The schema assumption itself. The Mixxx 2.5.6 schema is pinned:
  `Playlists.hidden = 2` is the history playlist and `library.location` is an FK
  to `track_locations.id`. Do not add schema probing.
- The `ORDER BY pt.pl_datetime_added DESC, pt.id DESC LIMIT 1` ordering — it
  matches the bash reference exactly.
- The raw sqlite FFI layer below the query string. This packet changes SQL and
  tests, not the FFI wrappers.

## Constraints

- Change `JOIN track_locations` to `LEFT JOIN`. Nothing else in the query moves.
- A NULL location column must arrive as an empty string, which
  `column_text_or_empty` already handles at `history.rs:231-247`. Verify rather
  than assume — confirm `sqlite3_column_text` returns NULL for the missing side
  of a left join.
- An empty path must classify as not-V4V, which means the metadata drop file is
  removed and the now-playing text file still updates. Do not add a special case
  in `main.rs` if `is_v4v_track` already produces this.
- Do not change the `hist_id` de-duplication. `poll` returning `Ok(None)` for an
  unchanged `hist_id` is the whole reason the file is not rewritten every 500 ms.

## Implementation Steps

1. Add a failing test first, using the existing synthetic-database harness. The
   shape that reproduces it:

   ```text
   track_locations: (1, '/some/path.mp3')
   library:         (10, 'Test-Artist', 'Good Song',   1)
                    (11, 'Orphan',      'Orphan Song', NULL)
   PlaylistTracks:  (100, 1, 10, '...10:00:00')
                    (101, 1, 11, '...11:00:00')   <- newest
   ```

   Assert that `poll()` returns `hist_id = 101` with title `Orphan Song`.
   Before the fix it returns `hist_id = 100` and `Good Song`.
2. Change line `:15` to `LEFT JOIN track_locations tl ON tl.id = l.location`.
3. Add a second test for a *dangling* FK — `library.location = 999` with no
   matching `track_locations` row — which must behave the same as NULL.
4. Add an end-to-end assertion that with such a row the now-playing text file is
   written and the metadata file is absent.

## Acceptance Criteria

- The newest history row is returned regardless of whether its location resolves.
- `TrackRow.path` is empty for an unresolvable location, and the track is
  classified as not-V4V.
- The now-playing text file still updates for such a track, matching the bash
  script's output for the same database.
- A dangling FK behaves identically to a NULL.
- Existing history and lifecycle tests pass unmodified.
- `cargo clippy --all-targets` clean.

## Test Commands

```bash
cd mixxx-now-playing
cargo clippy --all-targets --offline
cargo test --offline
```

Manual parity check against a synthetic database, comparing both
implementations on the same file:

```bash
sqlite3 "$DB" "SELECT pt.id || '|' || IFNULL(l.artist,'') || '|' || IFNULL(l.title,'')
  FROM PlaylistTracks pt
  JOIN Playlists p ON p.id = pt.playlist_id
  JOIN library l ON l.id = pt.track_id
  WHERE p.hidden = 2
  ORDER BY pt.pl_datetime_added DESC, pt.id DESC LIMIT 1;"

./target/debug/mixxx-now-playing --db-file "$DB" \
  --txt-file ./np.txt --id3-file ./md.txt --v4v-root ./v4v --once --no-api
cat ./np.txt
```

The two must name the same track.

## Expected Final Report Format

- The one-line SQL diff.
- Both new test names and what database shape each builds.
- Confirmation that an empty path classifies as not-V4V, and where that is
  enforced.
- Output of the manual parity check: the bash query result and the `np.txt`
  contents side by side.
- Full `test result:` lines.

## Escalation Triggers

- If `column_text_or_empty` does not return an empty string for the NULL side of
  the left join — for example if it panics or returns stale bytes — stop. That is
  an FFI correctness bug, not a SQL bug, and it is more serious than this packet.
- If making the join outer changes which row is returned in the *normal* case,
  stop and report; that would mean the ordering is not as deterministic as the
  bash reference assumes.
