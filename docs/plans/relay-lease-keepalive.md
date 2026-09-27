# Relay Lease Keepalive Phase Plan

Date: 2026-09-27. This plan states no rule. ADR 0005 owns the rules here.
`musicindex-live-relay` ADR 0002 owns the lease, the keepalive route and the
wire fields.

## Goal

The publisher tells the relay when audio is on air. It never keeps a payee on
air that the producer did not supply. When this plan is complete:

- A non-V4V track, unreadable tags and a talk gap publish the dead block, and
  the event stays on air.
- A stopped or killed producer publishes the dead block, and the relay ends
  the lease.
- A stopped or killed publisher lets the relay end the lease.
- No configuration can make the station collect payments during a non-V4V
  track.

## Non-Goals

- The talk break block from `v4vmm`.
- The MIDI link to Mixxx for the deck play state.
- A relay stop message.
- A drop-file schema change. The schema stays `musicindex.nowplaying/1`.

## Assumptions

- No one runs the publisher or the relay in production. No old version needs
  support, so a removed option does not need a transition period.
- `mixxx-now-playing` is the only producer for each drop directory
  (AGENTS.md §6).
- The producer exits when Mixxx is not running (AGENTS.md §7). So "producer
  running" means "Mixxx running".
- Rust 1.89 or later is available for `File::lock` and `File::try_lock_shared`.
  The local toolchain is 1.93.1.

## Present Behavior

This service sends no stop message today. When a drop file is removed, the
publisher sends the target's fallback block to the metadata route. That block
pays a dead route by default, but configuration can make it pay the station.

## Affected Modules

| Module | Change | Task |
|---|---|---|
| `mixxx-now-playing/src/main.rs` | Take the producer lock | 001 |
| `mixxx-now-playing/src/cli.rs`, `expiry.rs` | `--expiry-max` replaces `--expiry-fallback` | 001 |
| `src/livevalue.rs` | The dead block constant | 002 |
| `src/config.rs` | Remove `[target.fallback]` | 002 |
| `src/watcher.rs` | Empty routes give the dead block | 002 |
| `scripts/setup-mixxx-musicindex.sh`, `packaging/arch/` | Remove the fallback options | 002 |
| `src/liveness.rs` (new), `src/main.rs` | Probe the producer lock | 003 |
| `src/relay.rs` | The keepalive and the worker commands | 004 |

## Sequence

The two repositories have separate packets. Each packet is one commit in its
own repository.

1. The operator accepts ADR 0005 here and ADR 0002 in `musicindex-live-relay`.
2. Task 001: the producer lock and the expiry maximum.
3. Task 002: the dead block.
4. Task 003: producer liveness in the publisher. It needs tasks 001 and 002.
5. `musicindex-live-relay` tasks 001 and 002: the lease and the keepalive
   route.
6. Task 004: the keepalive. It needs task 003 and relay task 002.

Tasks 001 and 002 can go in either order. Steps 2 to 4 do not need the relay.

## Schema And API Implications

- **Drop directory:** `.producer.lock` becomes part of the contract. Task 001
  amends ADR 0002. The drop-file schema does not change.
- **Configuration:** `[target.fallback]` becomes a load error that names ADR
  0005.
- **Control CLI:** `config show --json` loses `fallback_configured`. No code in
  `v4vmm/src` reads it on 2026-09-27.
- **Producer CLI:** `--expiry-max` replaces `--expiry-fallback`.
- **Setup script:** `--fallback-value-block` and `--fallback-address` are
  removed.
- **Relay:** the publisher reads `keepalive_interval_secs` from the publish
  response and calls `POST /v1/liveitems/{event_id}/keepalive`. The publisher
  sends a keepalive only after a relay gives an interval. So a relay without
  the lease never receives a keepalive.

## Risk Areas

- **A dropped first file after the producer starts.** The probe runs each
  second. A file event can arrive before the probe sees the lock. Task 003
  probes before each event batch and scans the directory when the producer
  becomes present.
- **The order of the dead block and the keepalive stop.** The dead block waits
  in `PublishSchedule` for the stream delay. The keepalive stops at once. The
  dead block publish renews the lease one time, so the order is safe while the
  stream delay is shorter than the lease.
- **A worker that is busy with backoff.** A keepalive must not delay a new
  payload. Task 004 gives a payload priority.
- **`--once` mode.** The producer writes a file and exits without a lock. The
  publisher ignores that file. This is correct, because nothing removes the
  file later.

## Test Strategy

- Unit tests for each invariant in ADR 0005, in the crate that owns it.
- Lock tests hold a real `flock` in the test process. Two open file
  descriptions of one file conflict, also in one process.
- Watcher and liveness tests use `tempfile`. They do not depend on an inotify
  race.
- Keepalive tests use the local stub server pattern in `tests/relay.rs`. No
  test calls a public relay.
- Each task runs the full gate in AGENTS.md.

## Rollback

Each task is one commit. `git revert` of that commit restores the previous
behavior. Task 002 removes configuration options. A revert restores them, and
an operator must then add `[target.fallback]` again if needed. No task
migrates stored data, so no rollback needs a data step.

## Tasks

- `docs/tasks/relay-lease-task-001-producer-lock-and-expiry-max.md`
- `docs/tasks/relay-lease-task-002-dead-block.md`
- `docs/tasks/relay-lease-task-003-producer-liveness.md`
- `docs/tasks/relay-lease-task-004-keepalive.md`
- `docs/reviews/relay-lease-review-checklist.md`

## Later Decisions

- The talk break block from `v4vmm`, while `mixxx-now-playing` owns the drop
  directory.
- The MIDI link to Mixxx for the deck play state. It supersedes the payment
  timing rule in ADR 0005.
