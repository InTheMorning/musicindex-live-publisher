# Relay Lease Task 001: Producer Lock And Expiry Maximum

Status: Implemented - 2026-09-27 (`32d2ce5`).

Every criterion is mechanical. This packet has no visual criteria, because it
adds no user interface.

## Goal

`mixxx-now-playing` holds an exclusive lock on `.producer.lock` in its drop
directory while it runs. Its expiry timer never exceeds a maximum, and
`--expiry-max` replaces `--expiry-fallback`.

## Files To Inspect

- `docs/adr/0005-producer-liveness-and-dead-block.md`
- `docs/adr/0002-nowplaying-drop-file-contract.md`
- `mixxx-now-playing/src/main.rs` (`run`, `Runtime::new`, `process_track`,
  `MetadataCleanup`)
- `mixxx-now-playing/src/cli.rs`
- `mixxx-now-playing/src/expiry.rs`
- `mixxx-now-playing/src/sink.rs`
- `mixxx-now-playing/tests/expiry.rs`
- `mixxx-now-playing/tests/lifecycle.rs`

## Files Likely To Change

- `mixxx-now-playing/src/main.rs`
- `mixxx-now-playing/src/lock.rs` (new)
- `mixxx-now-playing/src/lib.rs`
- `mixxx-now-playing/src/cli.rs`
- `mixxx-now-playing/src/expiry.rs`
- `mixxx-now-playing/tests/expiry.rs`
- `mixxx-now-playing/tests/lifecycle.rs`
- `docs/adr/0002-nowplaying-drop-file-contract.md`
- `docs/runbooks/musicindex-live-publisher-configuration.md`

## Do Not Touch

- `src/**` (the publisher crate)
- `mixxx-now-playing/src/tags.rs`. The duration source stays
  `properties().duration()`.
- The drop-file JSON fields and the schema string
- `docs/adr/0005-producer-liveness-and-dead-block.md`

## Constraints

- The lock path is `<drop directory>/.producer.lock`. The drop directory is the
  parent directory of `config.id3_file`.
- Use `std::fs::File::lock`. Add no dependency.
- Take the lock in `run` before `Runtime::new`, because `Runtime::new` writes
  to the drop directory. Keep the `File` alive until `run` returns.
- `File::lock` waits. Do not use a try call here. The publisher holds a shared
  lock for a very short time, and the producer must wait for it.
- Do not delete `.producer.lock` on exit. A delete can race with a new
  producer. The kernel releases the lock.
- `--once` does not take the lock.
- The expiry length is `min(stream duration, max) + slack`. With no stream
  duration, it is `max`. The default `max` is 600 seconds.
- Remove `--expiry-fallback`. An operator who passes it gets an error that
  names `--expiry-max`.

## Implementation Steps

1. Add `lock.rs` with `ProducerLock::acquire(drop_dir: &Path) -> Result<Self>`.
   It creates the file with mode `0600` if it is absent, then calls
   `lock()`. Use `.context(...)` with the path.
2. In `run`, derive the drop directory from `config.id3_file` and call
   `ProducerLock::acquire` before `Runtime::new`. Skip this for `--once`.
3. In `cli.rs`, rename the field to `expiry_max` and the flag to
   `--expiry-max`. Give `--expiry-fallback` the error in the constraints.
4. In `expiry.rs`, change `Expiry::duration` so that it applies the maximum
   as the constraints state.
5. Add tests:
   - `ProducerLock::acquire` makes a lock that a second open file cannot take
     with `try_lock_shared`.
   - After the `ProducerLock` drops, `try_lock_shared` succeeds.
   - A duration longer than the maximum gives `max + slack`.
   - No duration gives `max`.
   - A duration shorter than the maximum gives `duration + slack`.
   - `--expiry-fallback` fails and the message names `--expiry-max`.
6. Amend ADR 0002 in place with a dated sentence. Record these facts:
   - A producer holds `.producer.lock` while it runs.
   - `duration_secs` comes from the audio stream headers, and nothing verifies
     it.
   - "A present file means playing" becomes "A present file means a block
     plays". "Removing a file means stopped" becomes "Removing a file means no
     payable block".
7. Change `--expiry-fallback` to `--expiry-max` in the configuration runbook.

## Acceptance Criteria

- A test proves that a running producer lock blocks `try_lock_shared` from a
  second open file.
- A test proves that the lock is free after the lock value drops.
- The three expiry tests pass as step 5 states.
- The `--expiry-fallback` test passes.
- ADR 0002 names `.producer.lock` and the source of `duration_secs`.
- `grep -rn "expiry-fallback" mixxx-now-playing/src docs/runbooks` finds
  nothing.

## Test Commands

```bash
cargo fmt --all -- --check
cargo build --workspace
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
```

## Escalation Triggers

Stop and report if one of these occurs:

- `File::lock` is not available with the toolchain.
- `config.id3_file` can have no parent directory.
- An existing test depends on `--expiry-fallback` in a way that step 3 cannot
  fix.
- You think the publisher must also change in this task.

## Prompt for lower-context coding model

You are implementing one bounded task from a larger plan.

Implement only this task. Do not redesign the architecture.

Read:
- docs/adr/0005-producer-liveness-and-dead-block.md
- docs/tasks/relay-lease-task-001-producer-lock-and-expiry-max.md
- docs/adr/0002-nowplaying-drop-file-contract.md
- mixxx-now-playing/src/main.rs
- mixxx-now-playing/src/cli.rs
- mixxx-now-playing/src/expiry.rs
- mixxx-now-playing/tests/expiry.rs
- mixxx-now-playing/tests/lifecycle.rs
- ~/.agents/skills/asd-ste100/SKILL.md (for the ADR 0002 and runbook prose)

Goal:
- The producer holds an exclusive lock on `<drop dir>/.producer.lock` while it runs.
- The expiry timer uses `min(duration, max) + slack`, or `max` with no duration. `--expiry-max` (default 600 seconds) replaces `--expiry-fallback`.

Constraints:
- Use `std::fs::File::lock`. Add no dependency.
- Take the lock in `run` before `Runtime::new`. Keep it alive until `run` returns. Skip it for `--once`.
- Use the blocking `lock()`, not a try call.
- Do not delete the lock file on exit.
- `--expiry-fallback` must fail with a message that names `--expiry-max`.
- Follow AGENTS.md conventions: `anyhow::Result` with `.context(...)`, doc comments on public items.

Do not touch:
- src/** (the publisher crate)
- mixxx-now-playing/src/tags.rs
- The drop-file JSON fields and schema string
- docs/adr/0005-producer-liveness-and-dead-block.md

Acceptance criteria:
- A test proves that the producer lock blocks `try_lock_shared` from a second open file.
- A test proves that the lock is free after the lock value drops.
- Expiry tests: longer than max gives max + slack. No duration gives max. Shorter than max gives duration + slack.
- A test proves that `--expiry-fallback` fails and names `--expiry-max`.
- ADR 0002 has a dated amendment that names `.producer.lock` and the source of `duration_secs`, and changes the two invariants as the task file states.
- `grep -rn "expiry-fallback" mixxx-now-playing/src docs/runbooks` finds nothing.

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
