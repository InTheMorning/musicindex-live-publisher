# Display Album Task 001: The Display File Version 3

Status: Implemented - 2026-10-06. Ship it after relay ADR 0006 is deployed.

Every criterion is mechanical.

## Goal

The producer writes `display.json` with the schema `musicindex.display/3`. A
track has `album`. The publisher reads version 3, ignores version 2, and
sends `album` in the display body.

## Files To Inspect

- `docs/adr/0013-display-album.md`
- `docs/adr/0012-pair-display-state-with-value-block.md`
- `mixxx-now-playing/src/connector/link.rs`: `DisplayTrack`, `DisplayState`,
  `Coordinator::display_state`, the test helper `track`
- `mixxx-now-playing/src/main.rs`: `apply_row` (where `DisplayTrack` is
  built from `TrackRow`), `apply_display_output`
- `mixxx-now-playing/src/display.rs`: `DISPLAY_SCHEMA`, `ShownTrack`,
  `TrackJson`, `render_display_json`
- `mixxx-now-playing/src/history.rs`: `TrackRow::album`
- `mixxx-now-playing/tests/lifecycle.rs`:
  `lifecycle_display_pairs_with_the_song_file_and_the_drop_file`
- `src/display.rs`: `DISPLAY_SCHEMA`, `DisplayTrack`, `parse_track`,
  `DisplayState::body_with_pairing`
- `tests/display.rs`

## Files Likely To Change

- `mixxx-now-playing/src/connector/link.rs`
- `mixxx-now-playing/src/main.rs`
- `mixxx-now-playing/src/display.rs`
- `mixxx-now-playing/tests/lifecycle.rs`
- `src/display.rs`
- `tests/display.rs`
- `docs/runbooks/musicindex-live-publisher-configuration.md`

## Do Not Touch

- The live value payload, the drop file and `src/livevalue.rs`
- The pairing rules: `Pairing`, `pairs_with`, the resend in `src/relay.rs`
- The image rules
- `docs/adr/**`

## Constraints

- `DisplayTrack` in `link.rs` gets `album: Option<String>`. `apply_row` sets
  it from `TrackRow::album` of the same row. `DisplayState::Track` gets
  `album: Option<String>` from it.
- Never take the album from `state.current`. The producer clears that state
  for a track that is not V4V.
- `ShownTrack` gets `album: Option<&str>`. `display.json` has `album` as a
  string, or `null` when the album is `None`. `TrackRow` already gives
  `None` for a `NULL` or an empty album.
- The producer writes `musicindex.display/3` only.
- The publisher `DisplayTrack` gets `album: Option<String>`. It reads version
  3 only, and ignores version 2 with a warning, as ADR 0012 did for
  version 1. A `null`, absent or empty `album` gives `None`.
- `body_with_pairing` adds `"album"` only for `Some` album. It adds no
  `null`.

## Implementation Steps

1. Add the album to the `Coordinator` display track and display state.
2. Write it in `display.json` version 3.
3. Read it in the publisher and send it.
4. Add the tests.
5. Give version 3 in the configuration runbook, with ADR 0013 as its owner.

## Acceptance Criteria

Each item is a test:

- A lifecycle test runs the producer on a V4V track with an album, and
  `display.json` version 3 has that album.
- A lifecycle test runs the producer on a track that is not V4V with an
  album, and `display.json` has that album.
- A lifecycle test with a `NULL` album gives `"album": null`.
- The publisher parses version 3 with an album, and with `null`.
- The publisher ignores version 2 with a warning.
- A display state with an album, sent through the display worker to the stub
  relay, has `album`. A display state with no album has no `album` key.
- The pairing tests and the golden test pass with no change.

Also: the full gate passes.

## Test Commands

```bash
cargo fmt --all -- --check
cargo build --workspace
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
```

## Escalation Triggers

- `apply_row` does not have the `TrackRow` of the display state, so the album
  must come from a different place.
- A change to `DisplayState::Track` changes when the producer sees a display
  change in a way that a test shows.

## Prompt for lower-context coding model

You are implementing one bounded task from a larger plan.

Implement only this task. Do not redesign the architecture.

Read:
- docs/adr/0013-display-album.md
- docs/tasks/display-album-task-001-display-json-v3.md
- mixxx-now-playing/src/connector/link.rs, main.rs, display.rs, history.rs
- mixxx-now-playing/tests/lifecycle.rs
- src/display.rs
- tests/display.rs

Goal:
- The producer writes musicindex.display/3 with album. The publisher reads version 3 and sends album in the display body.

Constraints:
- Follow §Constraints of the packet exactly. The album comes from the history row of the display state, never from state.current.
- Each acceptance criterion is a test that drives the real path: the producer binary for the producer, the display worker and the stub relay for the publisher.

Do not touch:
- The live value payload, the drop file, src/livevalue.rs, the pairing rules, the image rules, docs/adr/**

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

Reviewed 2026-10-06. `Cargo.lock` did not change. The full gate passes with
491 tests.

The album comes from `TrackRow` in `apply_row`, through `DisplayTrack` and
`DisplayState::Track`. It never comes from `state.current`. The lifecycle
test `lifecycle_display_non_v4v_track_with_album` runs the producer on a
track outside the V4V root, and `display.json` has its album.

The review made these changes:

- The two body tests call the body helper only. The review added
  `a_display_state_with_no_album_goes_out_with_no_album_key`, which sends
  through the display worker to the stub relay.
- The configuration runbook did not give version 3. The review added
  `album`, the body example and ADR 0013 as the owner.
- The implementation reverted the uncommitted acceptance of ADR 0013 in
  `AGENTS.md`, `docs/README.md` and the ADR. The review wrote it again.

The report of the implementation gave 514 tests. The real count was 490
before the review.
