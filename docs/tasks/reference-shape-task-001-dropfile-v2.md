# Reference Shape Task 001: The Drop File Version 2

Status: Ready after the operator accepts ADR 0010. Ship it in one release
with task 002.

Every criterion is mechanical.

## Goal

The producer writes the drop file `musicindex.nowplaying/2` with the album
from the Mixxx library and the `play_id` of the play. The publisher parses version 2 and ignores version 1.
The payload does not change in this task.

## Files To Inspect

- `docs/adr/0010-live-value-payload-reference-shape.md` (§The Drop File,
  Version 2)
- `docs/adr/0002-nowplaying-drop-file-contract.md` (the version 1 fields)
- `mixxx-now-playing/src/history.rs`: `HISTORY_QUERY`, `TrackRow`
- `mixxx-now-playing/src/render.rs`: `TrackDisplay`,
  `render_metadata_json_with_routes`
- `mixxx-now-playing/src/main.rs` (where `TrackDisplay` is built)
- `mixxx-now-playing/tests/common/mod.rs` (the test schema)
- `mixxx-now-playing/tests/live_publisher_dropfile.rs`
- `src/dropfile.rs`: `SCHEMA_VERSION`, `DropFile`, `parse`
- `tests/watcher.rs`, `tests/config.rs` (drop file fixtures)

## Files Likely To Change

- `mixxx-now-playing/src/history.rs`
- `mixxx-now-playing/src/render.rs`
- `mixxx-now-playing/src/main.rs`
- `mixxx-now-playing/tests/common/mod.rs`
- `mixxx-now-playing/tests/live_publisher_dropfile.rs`
- `src/dropfile.rs`
- `tests/watcher.rs`, `tests/config.rs` (fixture schema strings)
- `tests/golden.rs` (its `DropFile` literal gets the two new fields as
  `None`)
- `README.md`, `docs/runbooks/musicindex-live-publisher-configuration.md`

## Do Not Touch

- `src/livevalue.rs`. Task 002 changes the payload.
- `now-playing.txt` and `render_metadata_text`. The song line does not
  change.
- The display output and `display.json`.
- `docs/adr/**`, and the older plans and tasks that name version 1 as
  history.

## Constraints

- `HISTORY_QUERY` reads `l.album` in the inner query and returns it. A
  `NULL` or an empty album gives `None` in `TrackRow`.
- `TrackDisplay` gets `album: Option<&str>` and `play_id: Option<i64>`. The
  producer sets `play_id` from the history row.
- The drop file of the producer has `schema: "musicindex.nowplaying/2"`, each
  version 1 field, `album`, and `play_id`. `play_id` is `TrackRow::hist_id`,
  the ID of the history row, written as a decimal string.
- The drop file has no `link_url` and no `link_text`. ADR 0010 removed them.
- In the publisher, `SCHEMA_VERSION` becomes `musicindex.nowplaying/2`.
  `DropFile` gets `album` and `play_id`, each `Option<String>`, each optional
  in the JSON. A version 1 file gives
  `Ok(None)` with the present warning.
- Keep the history query a single query with the `LIMIT 1` subquery. Do not
  add a join in the outer query. The comment above the query explains why.

## Implementation Steps

1. Add `album TEXT` to the `library` table of the test schema.
2. Change the history query and `TrackRow`.
3. Change `TrackDisplay` and the drop file writer.
4. Change the publisher parse.
5. Change the fixtures and the contract test to version 2.
6. Update the drop file table in `README.md` and the runbook. Name ADR 0010
   as the owner of version 2.

## Acceptance Criteria

Each item is a test:

- The history query gives the album of the newest history row, and `None`
  for a `NULL` or an empty album.
- The producer writes version 2 with `album` and `play_id`.
- Two plays of the same track give two different `play_id` values.
- The publisher parses version 2 with both new fields, and with neither of
  them.
- The publisher gives `Ok(None)` for a version 1 file.
- The contract test parses the producer output as version 2 in the
  publisher.
- The payload tests and the golden test pass with no change.

Also: the full gate passes.

## Test Commands

```bash
cargo fmt --all -- --check
cargo build --workspace
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
```

## Escalation Triggers

- The real Mixxx `library` table has no `album` column. Check one real
  Mixxx 2.5 database with
  `sqlite3 -readonly ~/.mixxx/mixxxdb.sqlite "pragma table_info(library)"`.
- A field change needs a change to `src/livevalue.rs`.

## Prompt for lower-context coding model

You are implementing one bounded task from a larger plan.

Implement only this task. Do not redesign the architecture.

Read:
- docs/adr/0010-live-value-payload-reference-shape.md
- docs/adr/0002-nowplaying-drop-file-contract.md
- docs/tasks/reference-shape-task-001-dropfile-v2.md
- mixxx-now-playing/src/history.rs, render.rs, main.rs
- mixxx-now-playing/tests/common/mod.rs, mixxx-now-playing/tests/live_publisher_dropfile.rs
- src/dropfile.rs, tests/watcher.rs, tests/config.rs
- README.md, docs/runbooks/musicindex-live-publisher-configuration.md

Goal:
- The producer writes musicindex.nowplaying/2 with the album and play_id. The publisher parses version 2 and ignores version 1. The payload does not change.

Constraints:
- Follow §Constraints of the packet exactly.

Do not touch:
- src/livevalue.rs, now-playing.txt and render_metadata_text, the display output, docs/adr/**, older plans and tasks

Acceptance criteria:
- Each item in §Acceptance Criteria of the packet is a passing test.

Test commands:
- cargo fmt --all -- --check
- cargo build --workspace
- cargo test --workspace
- cargo clippy --workspace --all-targets -- -D warnings

At the end, report:
1. files changed
2. tests run
3. behavior changed
4. deviations from task
5. unresolved concerns
