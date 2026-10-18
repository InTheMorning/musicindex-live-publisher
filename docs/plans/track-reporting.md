# Each Producer Reports Each Track: Phase Plan

Status: Packets done 2026-10-18. The manual checks of the review checklist
are open. This plan does not make rules. ADR 0014 owns them.
The operator accepted ADR 0014 on 2026-10-18.

## Goal

The Mixxx producer writes the drop file `musicindex.nowplaying/3`. For a track
that plays and is not payable, it writes a file with an empty `value_routes`.
It removes the file only when no track plays. The publisher reads version 3
only.

## Non-Goals

- No live segment. ADR 0015 owns it, and it is not accepted.
- No change to what the publisher publishes. ADR 0005 gives the dead block
  for a file with no routes, and for an absent file.
- No new field in the drop file. Version 3 has the fields of version 2.
- No change to the display output. A non-V4V row gives a display track
  today.
- No instance or lock work. ADR 0016 owns it, and it is not accepted.

## Assumptions

- No one runs the chain in production. Version 2 stops at once, with no
  transition period (ADR 0014).
- `read_tags` (`mixxx-now-playing/src/tags.rs`) can read the stream duration
  of a non-V4V file. A failed read gives no duration, and the expiry then
  uses its maximum (ADR 0005).

## Affected Modules

| Module | Change |
|---|---|
| `mixxx-now-playing/src/connector/link.rs` | `Row::Other` gets the presence rules of `Row::V4v`: a write while its track plays, a removal when it stops. Task 001 changes the history-only mode. Task 002 changes the connector mode. |
| `mixxx-now-playing/src/main.rs` | `CurrentTrack` holds a non-payable track. `write_current` writes a file with no routes for it. The route lookup does not run for it. |
| `mixxx-now-playing/src/render.rs` | The schema string `musicindex.nowplaying/3` |
| `src/dropfile.rs` | `SCHEMA_VERSION` is version 3. Version 2 gives `Ok(None)`. The doc comment states the meaning of presence of ADR 0014. |
| `README.md`, the configuration runbook | The drop file version 3 and its meaning |

## Sequence

1. [Task 001](../tasks/track-reporting-task-001-dropfile-v3-history-only.md):
   the drop file version 3 in both crates, and the non-payable file in the
   history-only mode.
2. [Task 002](../tasks/track-reporting-task-002-connector-mode.md): the
   non-payable file in the connector mode.

Task 002 needs task 001. Ship both in one release. A producer that writes
version 3 for one mode only removes the file for a non-V4V row in the other
mode, and ADR 0014 then does not hold.

## Schema And API Implications

- Drop file schema `musicindex.nowplaying/3`. The fields do not change.
- The payload, the dead block and the relay calls do not change.

## Risk Areas

- **Payment from a non-V4V file.** A file outside the V4V root can hold
  MusicIndex tags. The producer must never take a route from such a file,
  and must never start a route lookup for it. A test proves it.
- **The coordinator.** `connector/link.rs` holds the payment timing of
  ADR 0006: the link, the resume settle, and the new load. Task 002 gives a
  non-V4V row the same rules. Each existing test of a V4V row keeps its
  result.
- **The expiry in the history-only mode.** A non-payable file expires as a
  V4V file does. After its expiry, the publisher sees "no track plays" while
  a long non-V4V track can still play. That is the limit of ADR 0005 for a
  mode with no stop signal. The connector mode does not have this limit.
- **The schema change.** A producer and a publisher of different versions
  ignore each other's drop file. The release must ship both crates.

## Test Strategy

Unit tests in `connector/link.rs` and `src/dropfile.rs`, the lifecycle tests
of the producer, and the contract test
`mixxx-now-playing/tests/live_publisher_dropfile.rs` across the two crates.

## Rollback Strategy

Revert both crates together. No stored data depends on the change.

## Review

[Review checklist](../reviews/track-reporting-review-checklist.md).
