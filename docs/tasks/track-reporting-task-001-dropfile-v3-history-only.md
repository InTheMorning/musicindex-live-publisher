# Track Reporting Task 001: The Drop File Version 3, History-Only Mode

Status: Ready - 2026-10-18. Ship it in one release with task 002.

Every criterion is mechanical.

## Goal

The two crates use the drop file `musicindex.nowplaying/3`. In the
history-only mode, the producer writes a file with an empty `value_routes`
for a row that is not payable. It does not remove the file. The publisher parses version 3
and ignores version 2. The payload does not change.

## Files To Inspect

- `docs/adr/0014-one-assembler-and-track-reporting.md` (§Each Producer
  Reports Each Track)
- `docs/adr/0005-producer-liveness-and-dead-block.md` (§Publisher Behavior,
  §Payment Timing Without The MIDI Connector)
- `mixxx-now-playing/src/connector/link.rs`: `Row`, `Coordinator::history_row`,
  `Coordinator::row`, `CurrentRow`, the expiry handling
- `mixxx-now-playing/src/main.rs`: `CurrentTrack`, `process_track`,
  `apply_row`, `write_current`, `render_metadata_content`
- `mixxx-now-playing/src/render.rs`: the schema string,
  `render_metadata_json_with_routes`
- `mixxx-now-playing/src/tags.rs`: `read_tags`, `TrackTags`
- `mixxx-now-playing/tests/live_publisher_dropfile.rs`
- `src/dropfile.rs`: `SCHEMA_VERSION`, `DropFile`, `parse`
- `tests/watcher.rs`, `tests/config.rs`, `tests/golden.rs` (drop file
  fixtures)

## Files Likely To Change

- `mixxx-now-playing/src/connector/link.rs`
- `mixxx-now-playing/src/main.rs`
- `mixxx-now-playing/src/render.rs`
- `mixxx-now-playing/tests/live_publisher_dropfile.rs`
- `src/dropfile.rs`
- `tests/watcher.rs`, `tests/config.rs`, `tests/golden.rs` (fixture schema
  strings)
- `README.md`, `docs/runbooks/musicindex-live-publisher-configuration.md`

## Do Not Touch

- The connector-mode rules of `Coordinator`. Task 002 changes them. In the
  connector mode, a non-V4V row keeps its present behavior in this task.
- `src/livevalue.rs`, the dead block and the payload.
- The display output and `display.json`.
- `docs/adr/**`, and the older plans and tasks that name version 2 as
  history.

## Constraints

- `Row::Other` gets `header_duration` and `expiry`, as `Row::V4v` has.
- For a non-V4V row, the producer reads the stream duration with `read_tags`.
  It uses `duration` only. A failed read gives no duration. The expiry then
  uses `--expiry-max`, as for a V4V row with no duration.
- **The producer never takes a route from a non-V4V file.** It ignores each
  MusicIndex tag of that file, and it starts no route lookup for it, also when
  the file holds MusicIndex tags.
- A V4V row whose tags the producer cannot read is not payable. It follows
  the same path as a non-V4V row.
- In the history-only mode, a non-payable row gives `Action::WriteFile`, and
  its expiry gives `Action::RemoveFile`, as for a V4V row. The next history
  row replaces it.
- The non-payable drop file has `schema`, `target`, `artist`, `title`,
  `duration_secs`, `album` and `play_id` from the history row, an empty
  `value_routes`, and a null `value_routes_source`. It has no `image`,
  `feed_guid` or `track_guid`.
- The schema string is `musicindex.nowplaying/3` in both crates.
  `SCHEMA_VERSION` in `src/dropfile.rs` becomes version 3. A version 2 file
  gives `Ok(None)` with the present warning.
- The doc comment of `DropFile` states the meaning of presence of ADR 0014:
  present with routes, present with no routes, and absent.

## Implementation Steps

1. Change the schema string in both crates, and the fixtures.
2. Give `Row::Other` its duration and expiry. Read the duration in
   `process_track` for a non-V4V row and for a V4V row with tags that the
   producer cannot read.
3. Give `CurrentTrack` a payable flag, or a kind. `write_current` writes the
   non-payable file for a non-payable track. Skip the route lookup for it.
4. In the history-only mode, make `Coordinator::row` write and expire a
   non-payable row as it does a V4V row.
5. Update the contract test, `README.md` and the runbook. Name ADR 0014 as the
   owner of version 3, and give its table of meanings.

## Acceptance Criteria

Each item is a test:

- In the history-only mode, a non-V4V row gives `Action::WriteFile`, and its
  expiry gives `Action::RemoveFile`.
- The producer writes a version 3 file with an empty `value_routes` for a
  non-V4V row.
- A non-V4V file that holds MusicIndex tags gives an empty `value_routes`,
  and no route lookup starts.
- A V4V row with tags that the producer cannot read gives the same
  non-payable file.
- A V4V row gives the same version 3 file as before, with its routes.
- The publisher parses version 3, and gives `Ok(None)` for version 2.
- The contract test parses a payable and a non-payable producer file as
  version 3 in the publisher.
- The payload tests and the golden test pass with no change to the payload.

Also: the full gate passes.

## Test Commands

```bash
cargo fmt --all -- --check
cargo build --workspace
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
```

## Escalation Triggers

- `read_tags` fails for a usual non-V4V format, for example MP3 or FLAC
  without MusicIndex tags. Then a different duration read is necessary.
- A change in the connector mode is necessary to pass a criterion of this
  task.
- A test of a V4V row changes its result.

## Prompt for lower-context coding model

You are implementing one bounded task from a larger plan.

Implement only this task. Do not redesign the architecture.

Read:
- docs/adr/0014-one-assembler-and-track-reporting.md
- docs/adr/0005-producer-liveness-and-dead-block.md
- docs/tasks/track-reporting-task-001-dropfile-v3-history-only.md
- mixxx-now-playing/src/connector/link.rs, main.rs, render.rs, tags.rs
- mixxx-now-playing/tests/live_publisher_dropfile.rs
- src/dropfile.rs, tests/watcher.rs, tests/config.rs, tests/golden.rs
- README.md, docs/runbooks/musicindex-live-publisher-configuration.md

Goal:
- Both crates use musicindex.nowplaying/3. In the history-only mode, the producer writes a file with an empty value_routes for a row that is not payable, and expires it as a V4V file. The publisher ignores version 2.

Constraints:
- Follow §Constraints of the packet exactly. Never take a route from a non-V4V file.

Do not touch:
- the connector-mode rules of Coordinator, src/livevalue.rs, the dead block, the display output, docs/adr/**, older plans and tasks

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
