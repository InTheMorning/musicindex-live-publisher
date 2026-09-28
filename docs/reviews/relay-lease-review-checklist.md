# Relay Lease Review Checklist

## Scope

Use this checklist after each relay lease task packet lands, and again after
task 004.

Reviewed work:

- `docs/adr/0005-producer-liveness-and-dead-block.md`
- `docs/plans/relay-lease-keepalive.md`
- `docs/tasks/relay-lease-task-001-producer-lock-and-expiry-max.md`
- `docs/tasks/relay-lease-task-002-dead-block.md`
- `docs/tasks/relay-lease-task-003-producer-liveness.md`
- `docs/tasks/relay-lease-task-004-keepalive.md`
- The implementation diff for each packet

## Required Checks

Payment safety:

- No publish path makes a payload with zero destinations.
- The dead block pays only `no-v4v-track@example.invalid`, and no
  configuration changes it.
- The dead block has no `feedGuid` and no `itemGuid`.
- Each dead block and each new track gets a fresh `blockGuid`.
- A removed drop file, an empty route list and a missing producer each publish
  the dead block.
- A stale drop file with no producer lock never publishes a track block.
- The expiry timer never exceeds `--expiry-max` plus the slack.
- Nothing in the producer reads `TLEN` or an RSS duration.

Liveness and lease:

- Only the producer creates `.producer.lock`. The publisher probe never
  creates it.
- The publisher sends a keepalive only while the producer lock is held, and
  only after the relay gave an interval.
- A keepalive `409` publishes the last payload again. It never brings back an
  older payload.
- A new payload is never delayed by a keepalive or a keepalive retry.

Contracts and scope:

- ADR 0002 names `.producer.lock` and the source of `duration_secs`.
- `AGENTS.md` §4 names the dead block and ADR 0005.
- The drop-file schema string is still `musicindex.nowplaying/1`.
- No token appears in a log line, an error message or a `Debug` output.
- No packet changed files on its "Do Not Touch" list.
- Tests that asserted the removed fallback configuration are deleted.
- No new dependency. No new thread for each target.

## Test Commands

- `cargo fmt --all -- --check`
- `cargo build --workspace`
- `cargo test --workspace`
- `cargo clippy --workspace --all-targets -- -D warnings`
- `bash -n scripts/setup-mixxx-musicindex.sh`

## Review Result

Status: Pass - 2026-09-28. All four packets are merged, and each required
check holds on `master`.

The review changed these items:

- Task 001: ADR 0002 §Decision now states the same meaning as its changed
  invariants.
- Task 002: a runbook example no longer shows the station as the payee of a
  track.
- Task 003: the publisher does not scan the drop directory when the producer
  comes back. A scan could publish a stale track.
- Task 004: a keepalive in retry stops when the producer goes missing. Three
  tests now count a request that must not happen.

One accepted exception to "each dead block gets a fresh `blockGuid`": the dead
block for an empty route list keeps the block identity of its track. A later
route upgrade for the same track then keeps the same `blockGuid`, the same as
any route upgrade.
