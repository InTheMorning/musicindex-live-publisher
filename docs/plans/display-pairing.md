# Pair The Display State With Its Value Block: Phase Plan

Status: Implemented 2026-10-06. This plan does not make rules. ADR 0012 owns
them. Both packets are done, and the chain is deployed.

## Goal

Each display state carries the exact song line. The display state of a V4V
track also carries the `eventGuid` and the `blockGuid` of its payload. The
tagger of ADR 0009 then matches by equality and names the exact block.

## Non-Goals

- No change to the live value payload or to its timing.
- No change to the image rules of ADR 0008.
- No tagger code. ADR 0009 and its new repository own the tagger.

## Assumptions

- ADR 0010 task 001 is done: the drop file version 2 has `play_id`.
- ADR 0011 task 002 is done: the publisher sends each payload and each
  display state at once. The pairing then needs no schedule.
- Relay ADR 0005 is deployed before task 002 ships.

## Affected Modules

| Module | Change |
|---|---|
| `mixxx-now-playing/src/display.rs` | `ShownTrack`, `render_display_json`, schema version 2 |
| `mixxx-now-playing/src/main.rs` | Gives the song line and the play ID to `ShownTrack` |
| `src/display.rs` | Reads version 2, keeps the pairing, builds the body |
| `src/livevalue.rs` | A `play_id` that the payload keeps and does not serialize |
| `src/main.rs` | Records each payload that goes out, and sends a display state again when its payload follows it |

## Sequence

1. [Task 001](../tasks/display-pairing-task-001-display-json-v2.md):
   `display.json` version 2, from the producer to the publisher parse. The
   relay body does not change.
2. [Task 002](../tasks/display-pairing-task-002-pairing.md): the pairing, the
   two new body keys, and the second send.

Task 002 needs task 001, ADR 0010 task 001 and ADR 0011 task 002. Ship task
001 and task 002 in one release, after relay ADR 0005 is deployed.

## Schema And API Implications

- `display.json` schema `musicindex.display/2` with `song_line` and
  `play_id`.
- The display body gets `songLine` and `value` (relay ADR 0005).

## Risk Areas

- **The song line.** It must be the line of `now-playing.txt` exactly. Task
  001 uses the same function and the same setting as that file.
- **The order of the two files.** The publisher can see `display.json` before
  the drop file. Task 002 sends the display state again when its payload
  follows it.
- **A second play of one track.** `play_id` separates the two plays.

## Test Strategy

Producer tests for the version 2 file. Publisher tests for the pairing rules,
with the stub relay of `tests/relay.rs` and the display tests of
`tests/display.rs`.

## Rollback Strategy

Roll back the publisher and the producer together. A relay of ADR 0005
accepts a track without the new keys, so it needs no rollback.

## Review

[Review checklist](../reviews/display-pairing-review-checklist.md).
