# The Live Value Payload Follows The Model Server: Phase Plan

Status: Proposed 2026-10-06. This plan does not make rules. ADR 0010 owns
them. The packets start after the operator accepts ADR 0010.

## Goal

A V4V track payload carries `line`, `author`, and when known `podcastName`,
as CurioHoster sends them. The drop file goes to version 2 and carries the
album and `play_id`.

## Non-Goals

- No change to any field that the payload sends today.
- No change to the dead block of ADR 0005.
- No change to the timing. ADR 0011 owns it.
- No `link`. The operator decided on 2026-10-06 that it stays absent until a
  source exists.
- No pairing. ADR 0012 uses `play_id`, and its own plan owns that work.

## Assumptions

- The Mixxx `library` table has an `album` column. The test schema of the
  producer does not have it yet, so task 001 adds it.
- No one runs the chain in production. Version 1 of the drop file stops at
  once, with no transition period (ADR 0010 §The Drop File, Version 2).

## Affected Modules

| Module | Change |
|---|---|
| `mixxx-now-playing/src/history.rs` | The history query and `TrackRow` read the album. `TrackRow` already has the history row ID for `play_id`. |
| `mixxx-now-playing/src/render.rs` | `TrackDisplay` and the drop file version 2 |
| `mixxx-now-playing/src/main.rs` | Gives the album to `TrackDisplay` |
| `src/dropfile.rs` | Parses version 2 and ignores version 1 |
| `src/livevalue.rs` | The three new payload fields |
| `README.md`, the configuration runbook | The drop file version 2 and the payload fields |

## Sequence

1. [Task 001](../tasks/reference-shape-task-001-dropfile-v2.md): the drop
   file version 2, from the producer to the publisher parse.
2. [Task 002](../tasks/reference-shape-task-002-payload-fields.md): the new
   payload fields.

Task 002 needs task 001. Ship both in one release: the producer and the
publisher must agree on the schema version.

## Schema And API Implications

- Drop file schema `musicindex.nowplaying/2` with `album` and `play_id`.
- New payload fields: `line`, `author`, `podcastName`. The relay passes them
  through.

## Risk Areas

- **The schema change.** A producer and a publisher of different versions
  ignore each other's drop file. The release must ship both crates.
- **The contract test.** `mixxx-now-playing/tests/live_publisher_dropfile.rs`
  checks that the producer output parses in the publisher. It must cover
  version 2.
- **The dead block.** It has no `line`. A change to the shared payload type
  must not add one to it.

## Test Strategy

Unit tests in `src/dropfile.rs` and `src/livevalue.rs`, the producer render
tests, the golden payload test, and the contract test across the two crates.

## Rollback Strategy

Revert both crates together. No stored data depends on the change.

## Review

[Review checklist](../reviews/live-value-reference-shape-review-checklist.md).
