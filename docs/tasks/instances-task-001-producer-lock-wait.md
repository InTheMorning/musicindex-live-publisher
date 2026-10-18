# Instances Task 001: The Producer Lock Wait Limit

Status: Ready - 2026-10-18.

Every criterion is mechanical.

## Goal

`ProducerLock::acquire` waits at most 2 seconds for the exclusive lock.
After that time, it gives an error that names ADR 0016 and the drop
directory. The producer then exits.

## Files To Inspect

- `docs/adr/0016-instances-events-and-producer-lock.md` (§The Producer Lock)
- `docs/adr/0005-producer-liveness-and-dead-block.md` (the amendment of
  2026-10-18)
- `mixxx-now-playing/src/lock.rs`
- `mixxx-now-playing/src/main.rs` (the call of `ProducerLock::acquire`)
- `src/liveness.rs` (the shared lock of the publisher)

## Files Likely To Change

- `mixxx-now-playing/src/lock.rs`
- `docs/runbooks/musicindex-live-publisher-configuration.md`

## Do Not Touch

- `src/liveness.rs`. The shared lock of the publisher does not change.
- The lock file name, its mode `0600`, and the rule that the lock never
  deletes its file.
- The drop file, the payload and the display output.
- `docs/adr/**`.

## Constraints

- Add `pub const LOCK_WAIT_LIMIT: Duration = Duration::from_secs(2);` and a
  retry interval constant of 50 milliseconds.
- Add `pub fn acquire_within(drop_dir: &Path, limit: Duration) ->
  Result<Self>`. `acquire` calls it with `LOCK_WAIT_LIMIT`.
- `acquire_within` calls `File::try_lock`. On `TryLockError::WouldBlock`, it
  sleeps for the retry interval and tries again, until the time `limit` ends.
  On `TryLockError::Error`, it gives that error at once.
- The error after the limit names ADR 0016, the drop directory and the
  limit. For example: `ADR 0016: a different producer holds the lock in
  <dir> after 2s. Stop that producer, or use a different publisher
  instance.`
- The error text never says that the lock file is stale. The kernel
  releases a lock when its process ends, so the holder is alive.
- Keep the module doc comment true. Change the paragraph that says the wait
  is deliberate and has no limit.
- No new dependency.

## Implementation Steps

1. Add the constants and `acquire_within`.
2. Make `acquire` call `acquire_within` with `LOCK_WAIT_LIMIT`.
3. Change the doc comments of the module and of `acquire`.
4. Add the tests of §Acceptance Criteria.
5. In the configuration runbook, state the wait limit and the error near the
   text about one `mixxx-now-playing.service`.

## Acceptance Criteria

Each item is a test in `mixxx-now-playing/src/lock.rs`:

- A second `acquire_within` with a limit of 100 milliseconds, while a first
  `ProducerLock` lives, gives an error. The error text holds `ADR 0016` and
  the drop directory path. The call returns in less than 1 second.
- A shared lock on the file that a second thread releases after 50
  milliseconds does not stop `acquire_within` with a limit of 1 second. The
  call gives a `ProducerLock`.
- `LOCK_WAIT_LIMIT` is 2 seconds.
- The two existing tests pass with no change to their assertions.

No test waits for `LOCK_WAIT_LIMIT`.

Also: the full gate passes.

## Test Commands

```bash
cargo fmt --all -- --check
cargo build --workspace
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
```

## Escalation Triggers

- `File::try_lock` is not available in the toolchain of the workspace.
- A test of the producer lifecycle needs the old wait with no limit.
- The publisher holds its shared lock for more than one second in a test.

## Prompt for lower-context coding model

You are implementing one bounded task from a larger plan.

Implement only this task. Do not redesign the architecture.

Read:
- docs/adr/0016-instances-events-and-producer-lock.md
- docs/plans/instances-and-producer-lock.md
- docs/tasks/instances-task-001-producer-lock-wait.md
- AGENTS.md
- mixxx-now-playing/src/lock.rs, mixxx-now-playing/src/main.rs, src/liveness.rs

Goal:
- ProducerLock::acquire waits at most 2 seconds for the exclusive lock, then gives an error that names ADR 0016 and the drop directory.

Constraints:
- Follow §Constraints of the packet exactly. No new dependency. No test waits 2 seconds.

Do not touch:
- src/liveness.rs, the lock file name and mode, the rule that the lock never deletes its file, the drop file, the payload, the display output, docs/adr/**

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
