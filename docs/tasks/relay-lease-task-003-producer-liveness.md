# Relay Lease Task 003: Producer Liveness In The Publisher

Status: Ready - 2026-09-27. It needs tasks 001 and 002. It does not need the
relay.

Every criterion is mechanical. This packet has no visual criteria, because it
adds no user interface.

## Goal

The publisher tests the producer lock. When the producer becomes missing, the
publisher publishes the dead block for each target and ignores the drop files.
When the producer becomes present, the publisher scans the drop directory
again. At startup, the publisher publishes one block for each target.

## Files To Inspect

- `docs/adr/0005-producer-liveness-and-dead-block.md` (§The Producer Lock and
  §Publisher Behavior)
- `docs/tasks/relay-lease-task-001-producer-lock-and-expiry-max.md` (the lock
  path and the lock kind)
- `src/main.rs` (`run_watch_loop`, `next_wakeup`, the startup call to
  `initial_payloads`, `HEALTH_CHECK_INTERVAL`)
- `src/watcher.rs` (`DropWatcher`, `initial_payloads`, `process_event`)
- `src/lib.rs`
- `tests/watcher.rs`

## Files Likely To Change

- `src/liveness.rs` (new)
- `src/lib.rs`
- `src/watcher.rs`
- `src/main.rs`
- `tests/watcher.rs`
- `tests/liveness.rs` (new)

## Do Not Touch

- `mixxx-now-playing/**`
- `src/relay.rs`, `src/schedule.rs`, `src/livevalue.rs`, `src/config.rs`
- The drop-file contract and ADR 0002

## Constraints

- The lock path is `<watch_dir>/.producer.lock`.
- The probe opens the file read-only and calls `try_lock_shared`. When the
  call succeeds, the probe releases the lock at once and reports `Missing`.
  When the call fails with `WouldBlock`, it reports `Running`. A missing file
  reports `Missing`. Any other error goes to the caller with `.context(...)`.
- The probe never creates the lock file. Only the producer creates it.
- The probe runs before each batch of filesystem events and at each timeout of
  the watch loop.
- On a change from `Running` to `Missing`:
  - publish one dead block for each target through `PublishSchedule`,
  - clear the block state in `DropWatcher`.
- While `Missing`, discard the payloads that file events produce.
- On a change from `Missing` to `Running`, scan the directory with the same
  code as `initial_payloads`, but do not publish the dead block when no file
  is present. The dead block is already live.
- At startup, publish one block for each target. If the producer is
  `Running` and a file for that target is present, it is the track block. In
  all other conditions, it is the dead block.
- Log each change with `tracing::info!` and a `producer` field.
- Do not add a thread. The watch loop owns the probe.

## Implementation Steps

1. Add `liveness.rs` with `enum ProducerState { Running, Missing }` and
   `fn probe_producer(watch_dir: &Path) -> Result<ProducerState>`.
2. Add a `DropWatcher` method that returns one dead block for each target and
   clears the block state. Add a method that scans the directory and returns
   only the track payloads.
3. Change the startup code in `main.rs` so that it follows the startup rule.
   A target that has no track block gets a dead block.
4. In `run_watch_loop`, keep the last `ProducerState`. Probe as the
   constraints state. Act on each change as the constraints state.
5. Add tests in `tests/liveness.rs`:
   - no lock file gives `Missing`,
   - a lock file with no holder gives `Missing`,
   - a lock file with an exclusive `File::lock` from a second open file gives
     `Running`,
   - the probe does not create the lock file.
6. Add tests in `tests/watcher.rs` for the `DropWatcher` methods:
   - the dead-block method gives one dead block for each target, and a later
     file for the same path gets a new `blockGuid`,
   - the scan method gives no dead block for an empty directory.
7. Add a test for the startup rule that uses a function you extract from
   `main.rs`. Cover: producer missing with a file present gives the dead
   block, and producer running with a file present gives the track block.

## Acceptance Criteria

- Each test in steps 5, 6 and 7 exists and passes.
- `probe_producer` never creates `.producer.lock`. A test proves this.
- A stale drop file with no producer lock publishes the dead block at
  startup. A test proves this.
- No new thread and no new dependency.

## Test Commands

```bash
cargo fmt --all -- --check
cargo build --workspace
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
```

## Escalation Triggers

Stop and report if one of these occurs:

- The watch loop cannot probe before an event batch without a large change to
  `run_watch_loop`.
- `try_lock_shared` reports `Running` for a lock that the same test process
  holds through a different open file, or the reverse.
- The startup rule conflicts with the stream delay in a way that this packet
  does not name.
- You think a target needs its own lock file.

## Prompt for lower-context coding model

You are implementing one bounded task from a larger plan.

Implement only this task. Do not redesign the architecture.

Read:
- docs/adr/0005-producer-liveness-and-dead-block.md
- docs/tasks/relay-lease-task-003-producer-liveness.md
- src/main.rs
- src/watcher.rs
- src/lib.rs
- tests/watcher.rs

Goal:
- The publisher probes `<watch_dir>/.producer.lock`. When the producer goes missing, it publishes one dead block for each target and ignores drop files. When the producer returns, it scans the directory again. At startup, it publishes one block for each target.

Constraints:
- `probe_producer` opens the file read-only and calls `try_lock_shared`. Success means Missing, and the probe releases the lock at once. WouldBlock means Running. No file means Missing. Other errors return with `.context(...)`.
- The probe never creates the lock file.
- Probe before each batch of filesystem events and at each watch-loop timeout.
- Running to Missing: one dead block for each target through PublishSchedule, then clear the DropWatcher block state.
- While Missing: discard payloads from file events.
- Missing to Running: scan the directory for track payloads. Do not publish a dead block for an empty directory.
- Startup: the track block if Running and a file for the target is present, otherwise the dead block.
- Log each change with tracing::info! and a `producer` field.
- No new thread. No new dependency.

Do not touch:
- mixxx-now-playing/**
- src/relay.rs, src/schedule.rs, src/livevalue.rs, src/config.rs
- The drop-file contract and ADR 0002

Acceptance criteria:
- tests/liveness.rs: no file gives Missing; unheld file gives Missing; a file with an exclusive lock from a second open file gives Running; the probe does not create the file.
- tests/watcher.rs: the dead-block method gives one dead block per target and a later file gets a new blockGuid; the scan method gives no dead block for an empty directory.
- A startup-rule test: producer missing with a file present gives the dead block; producer running with a file present gives the track block.
- No new thread and no new dependency.

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
