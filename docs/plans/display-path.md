# Display Path Phase Plan

Date: 2026-10-04. This plan states no rule. ADR 0008 owns the rules here.
`musicindex-live-relay` ADR 0003 owns the relay routes.

## Status

Tasks 001 to 004 are implemented - 2026-10-04. The review of task 004 and
the visual check of ADR 0008 are open.

## Goal

When this plan is complete:

- `now-playing.txt` is never deleted. It has no text when nothing plays.
- In the connector mode, a pause of any track clears the Icecast title.
- With `--display-dir`, the producer writes the display state and the
  embedded image of each track.
- With `display_dir` on a target, the publisher sends the display state and
  the image to the relay, through the stream delay of that target.

## Non-Goals

- A change to the drop file, the live value or a payment rule.
- A display path in `v4vmm`.
- A read token on the relay.

## Assumptions

- Relay ADR 0001 and ADR 0003 are implemented before a live test of task 004.
  Tasks 001 to 003 need no relay change. Task 004 tests against a stub relay.
- The crate `image`, with only its JPEG and PNG features, is acceptable as a
  new producer dependency (ADR 0008 §Consequences).

## Affected Modules

| Module | Change | Task |
|---|---|---|
| `mixxx-now-playing/src/main.rs` | The song file is written with no text, never deleted | 001 |
| `mixxx-now-playing/src/connector/link.rs` | The display link and the display state | 002 |
| `mixxx-now-playing/src/main.rs` | The song file follows the display state | 002 |
| `mixxx-now-playing/src/display.rs` (new) | `display.json`, the artwork source, the image rules, the retention | 003 |
| `mixxx-now-playing/src/cli.rs`, `Cargo.toml` | `--display-dir`, the `image` crate | 003 |
| `src/config.rs` | `display_dir` for a target | 004 |
| `src/display.rs` (new), `src/schedule.rs`, `src/main.rs` | The display watch, the delay and the display worker | 004 |
| `src/relay.rs` | The display and artwork requests | 004 |

## Sequence

1. Task 001: the song file. It needs nothing else, and it fixes the stale
   title now.
2. Task 002: the display link and the display state.
3. Task 003: the producer display output. It needs task 002.
4. Task 004: the publisher display path. It needs task 003 for the file
   format.

## Schema And API Implications

- New producer option `--display-dir DIR`.
- New file format `musicindex.display/1` in `DIR` (ADR 0008).
- New target field `display_dir` in the publisher configuration. A target
  without it has no display path.
- New relay requests: the artwork upload and the display publish (relay
  ADR 0003).

## Risk Areas

- **Payment isolation.** The display link must not change the payment link.
  Task 002 tests both links side by side.
- **A payload must never wait.** Display requests go through one display
  worker, separate from the payload workers. An image upload never runs in a
  payload worker.
- **Memory.** `DIR` is on a tmpfs, so the producer keeps two images at most.
  The publisher reads an image at once and holds it only inside the stream
  delay.
- **Image decode cost.** The producer reads the pixel size from the header
  first, and it rejects an image over 4,000 pixels before a decode.
- **A missing drive.** A file that cannot be read gives `artwork: null`. It is
  not an error.

## Test Strategy

- Pure `Coordinator` tests for the display link, the same as for the payment
  link.
- Image tests use small generated images in a `tempfile` directory. A test
  image can be made with the `image` crate in the test.
- Publisher tests use the stub relay pattern of `tests/relay.rs`.
- Each task runs the full gate in `AGENTS.md`.

## Rollback

Each task is one commit. A revert of task 001 brings back the deletion of the
song file. Tasks 002 to 004 are off unless the operator sets the new options,
except the song file rule of task 002.

## Tasks

- `docs/tasks/display-path-task-001-song-file.md`
- `docs/tasks/display-path-task-002-display-link.md`
- `docs/tasks/display-path-task-003-producer-output.md`
- `docs/tasks/display-path-task-004-publisher.md`
- `docs/reviews/display-path-review-checklist.md`
