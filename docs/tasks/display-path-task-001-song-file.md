# Display Path Task 001: The Song File Is Never Deleted

Status: Implemented - 2026-10-04. See §Review Result.

Every criterion is mechanical.

## Goal

The producer writes `now-playing.txt` with no text at startup and when it
stops. It never deletes the file. `butt` then sends an empty title instead of
the last one (ADR 0008 §Context).

## Files To Inspect

- `docs/adr/0008-display-path.md` (§The Song File For `butt`)
- `mixxx-now-playing/src/main.rs` (`run`, `Runtime::new`, `MetadataCleanup`,
  the end of `run`)
- `mixxx-now-playing/src/sink.rs`
- `mixxx-now-playing/tests/lifecycle.rs`

## Files Likely To Change

- `mixxx-now-playing/src/main.rs`
- `mixxx-now-playing/tests/lifecycle.rs`

## Do Not Touch

- `mixxx-now-playing/src/connector/**`
- The metadata file (the drop file) rules
- `src/**` (the publisher)
- `docs/adr/**`

## Constraints

- `Runtime::new` writes `now-playing.txt` with no text, through `OutputFile`,
  instead of `Presence::Absent`.
- At a normal end of `run`, and on `SIGTERM` or `SIGINT`, the producer writes
  the file with no text before it exits. Use the same cleanup place as the
  drop file.
- `--once` keeps its present behavior: it writes the line of the latest row.
- Use the atomic write of `OutputFile`. Do not rewrite a file whose content
  did not change (AGENTS.md §6).
- A non-V4V row still writes its title line, as today.

## Implementation Steps

1. Change the startup write.
2. Change the exit path.
3. Change the lifecycle tests that expect a missing file.

## Acceptance Criteria

- A test: after startup with no history row, `now-playing.txt` exists with no
  text.
- A test: after the producer exits, `now-playing.txt` exists with no text.
- No code path calls `Presence::Absent` for `now-playing.txt`. A test or a
  search in the review proves it.
- The full gate passes.

## Test Commands

```bash
cargo fmt --all -- --check
cargo build --workspace
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
```

## Escalation Triggers

- A lifecycle test needs a missing song file for a reason that ADR 0008 does
  not name.

## Prompt for lower-context coding model

Implement only this task.

Read:
- docs/adr/0008-display-path.md
- this packet
- mixxx-now-playing/src/main.rs, src/sink.rs and tests/lifecycle.rs

Goal: the producer writes now-playing.txt with no text at startup and at exit.
It never deletes the file. Follow §Constraints exactly.

Write the tests in §Acceptance Criteria, and run the test commands.

Report: 1. files changed 2. tests run 3. behavior changed 4. deviations
5. unresolved concerns.

## Review Result

Reviewed 2026-10-04. The review changed no code. Both rules were broken on
purpose, and a test failed each time.

- `MetadataCleanup` became `ShutdownCleanup`. Its `drop` removes the drop file
  as before, and it writes the song file with no text. It covers an early
  return through `?`.
- `sink::ensure_empty_file` reads the file first, so the shutdown guard does
  not rewrite a file that holds no text (AGENTS.md §6).
- A `SIGKILL` or a crash runs no cleanup. The song file then keeps the last
  title until the next start, which writes it with no text.
