# Display Pairing Task 001: The Display File Version 2

Status: Implemented - 2026-10-06. Ship it in one release with task 002. Ship it in one release with task 002.

Every criterion is mechanical.

## Goal

The producer writes `display.json` with the schema `musicindex.display/2`. A
track has `song_line` and `play_id`. The publisher parses version 2 and
ignores version 1. The relay body does not change in this task.

## Files To Inspect

- `docs/adr/0012-pair-display-state-with-value-block.md` (§The Producer)
- `docs/adr/0009-hls-track-metadata.md` (§The Song Line)
- `mixxx-now-playing/src/display.rs`: `DISPLAY_SCHEMA`, `ShownTrack`,
  `render_display_json`, the tests
- `mixxx-now-playing/src/main.rs`: where `ShownTrack` is built, where
  `now-playing.txt` is written with `render_now_playing_line`
- `mixxx-now-playing/src/render.rs`: `render_now_playing_line`
- `src/display.rs`: `DISPLAY_SCHEMA`, `DisplayTrack`, `read_display_state`,
  `parse_track`
- `tests/display.rs`

## Files Likely To Change

- `mixxx-now-playing/src/display.rs`
- `mixxx-now-playing/src/main.rs`
- `src/display.rs`
- `tests/display.rs`
- `README.md`, `docs/runbooks/musicindex-live-publisher-configuration.md`

## Do Not Touch

- `DisplayState::body` in `src/display.rs`. Task 002 changes the body.
- The image rules and the image files.
- The drop file and `src/livevalue.rs`.
- `docs/adr/**`

## Constraints

- `ShownTrack` gets `song_line: &str` and `play_id: Option<i64>`.
- The producer computes `song_line` with `render_now_playing_line`, with the
  same arguments that it uses for `now-playing.txt` at the same time. It does
  not compute the line in a second way.
- `play_id` is the ID of the history row of the shown track, the same value
  as `play_id` in the drop file of this play. In the JSON it is a decimal
  string, or `null`.
- The producer writes `musicindex.display/2` only.
- The publisher `DisplayTrack` gets `song_line: String` and
  `play_id: Option<String>`. A track with no `song_line` string is ignored
  with the present warning. The publisher reads `musicindex.display/2` only.
- A `null` track does not change.

## Implementation Steps

1. Change the producer schema, `ShownTrack` and `render_display_json`.
2. Give the song line and the play ID where `ShownTrack` is built.
3. Change the publisher parse.
4. Add the tests.
5. Update the display file description in `README.md` and the runbook. Name
   ADR 0012 as the owner of version 2.

## Acceptance Criteria

Each item is a test:

- The producer writes version 2 with `song_line` equal to the first line of
  `now-playing.txt` for the same track. Include one track whose artist or
  title has a hyphen, so the format rules apply.
- The producer writes the same `play_id` in `display.json` and in the drop
  file for one play.
- The publisher parses version 2, and ignores version 1 with a warning.
- The publisher ignores a version 2 track with no `song_line`.
- The display body that the publisher sends does not change.

Also: the full gate passes.

## Test Commands

```bash
cargo fmt --all -- --check
cargo build --workspace
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
```

## Escalation Triggers

- The display state of the coordinator does not know the history row of the
  shown track.
- The producer writes `now-playing.txt` and `display.json` at different
  times for one track, so the two lines can be different.

## Prompt for lower-context coding model

You are implementing one bounded task from a larger plan.

Implement only this task. Do not redesign the architecture.

Read:
- docs/adr/0012-pair-display-state-with-value-block.md
- docs/adr/0009-hls-track-metadata.md
- docs/tasks/display-pairing-task-001-display-json-v2.md
- mixxx-now-playing/src/display.rs, main.rs, render.rs
- src/display.rs
- tests/display.rs

Goal:
- The producer writes musicindex.display/2 with song_line and play_id. The publisher parses version 2 and ignores version 1. The relay body does not change.

Constraints:
- Follow §Constraints of the packet exactly. Compute song_line only with render_now_playing_line.

Do not touch:
- DisplayState::body, the image rules, the drop file, src/livevalue.rs, docs/adr/**

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

## Review Result

Reviewed 2026-10-06. `Cargo.lock` did not change. The full gate passes.

The producer takes `play_id` from the present V4V row. A row that is not V4V
clears that row, so its display state has `play_id` `null`. The producer thus
never gives a display state the play ID of an earlier track.

The review made these changes:

- The lifecycle test
  `lifecycle_display_pairs_with_the_song_file_and_the_drop_file` runs the
  producer on a track with hyphens. It compares `song_line` with the first
  line of `now-playing.txt`, and `play_id` with the drop file of the same
  play. The tests of the first implementation compared each value with a
  literal only.
- `parse_track` uses `let ... else` in place of `unwrap`.
- The configuration runbook says that a track that is not V4V has
  `play_id` `null`.

`README.md` does not describe `display.json`, so it did not change.
