# The Relay Applies The Stream Delay: Phase Plan

Status: Proposed 2026-10-06. This plan does not make rules. ADR 0011 owns
them. The packets start after the operator accepts ADR 0011 and
`musicindex-live-relay` ADR 0004.

## Goal

The publisher sends each block and each display state at once. Each publish
carries the stream delay of its target in the header `Listener-Delay-Secs`.
The relay delays only Socket.IO and `GET /remoteValue`.

## Non-Goals

- No change to the payload, the drop file or the display state.
- No change to the name or the limit of `stream_delay_secs`.
- No change to the keepalive interval or the lease rules of the relay.

## Assumptions

- Relay ADR 0004 is implemented and deployed before this publisher ships.
- The worker channel of a target keeps the order of its commands. A
  `Producer(Missing)` that follows a dead block reaches the worker after that
  block.

## Affected Modules

| Module | Change |
|---|---|
| `src/relay.rs`, `RelayTarget`, `publish_value` | The delay and the header |
| `src/main.rs`, `run_watch_loop`, `update_producer_state`, `next_wakeup` | Send at once, with no schedule |
| `src/schedule.rs`, `tests/schedule.rs` | Deleted |
| `src/display.rs` | Display entries go out at once |
| `README.md`, the configuration runbook, the example configuration | The function of `stream_delay_secs` |

## Sequence

1. [Task 001](../tasks/relay-delay-task-001-delay-header.md): each publish
   carries the header. No timing change.
2. [Task 002](../tasks/relay-delay-task-002-send-at-once.md): the schedule
   retires, and each item goes out at once.

**Ship the two tasks in one release.** After task 001 alone, a relay with ADR
0004 applies the delay, and the publisher applies it too. Podcast apps then
get the delay two times.

## Schema And API Implications

- New request header on each publish: `Listener-Delay-Secs`.
- No configuration key changes.

## Risk Areas

- **The keepalive stop.** ADR 0005 stops the keepalive after the dead block
  goes out. Without the schedule, the order of the worker channel must keep
  that rule.
- **The display path.** ADR 0008 held the image bytes until the schedule
  released the state, because the producer can delete an image. At once, the
  upload follows the read with no wait, so that risk becomes smaller.
- **Deleted code.** `PublishSchedule` and its tests are deleted, not kept
  unused.

## Test Strategy

The stub relay of `tests/relay.rs` records each request. A test reads the
header from it. The watch loop tests use an injected `Instant`, as they do
today.

## Rollback Strategy

Roll back the publisher and the relay together. A publisher before task 002
with a relay with ADR 0004 gives two delays. A publisher after task 002 with
a relay before ADR 0004 gives no delay.

## Review

[Review checklist](../reviews/relay-applies-stream-delay-review-checklist.md).
At acceptance of the review, the reviewer adds the dated sentences of ADR 0011
§What This Replaces to ADR 0005 and ADR 0008.
