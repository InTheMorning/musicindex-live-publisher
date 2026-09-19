# Show Log Task 001: Show Log Writer

Status: Not started - 2026-09-18. The writer needs a timestamp-source decision
before implementation. ADR 0003 defines producer time, but version 1 drop files
contain no producer timestamp. The scheduler's `Instant` values cannot supply
that wall-clock fact. Resolve the contract before changing code.

## Goal

Write the `musicindex.showlog/1` append-only log from the publish loop. One line
for each track and each clear, with both timestamps and the publish result.

## Files To Inspect

- `docs/adr/0003-show-log-contract.md`
- `docs/adr/0002-nowplaying-drop-file-contract.md`
- `src/schedule.rs` (`queued_at`, `due_at`, and the supersede rule)
- `src/main.rs` (`emit_payloads` and the watch loop)
- `src/relay.rs` (`PublishOutcome`)
- `src/config.rs`
- `src/dropfile.rs`

## Files Likely To Change

- `src/showlog.rs` (new)
- `src/lib.rs`
- `src/main.rs`
- `src/config.rs`
- `tests/showlog.rs` (new)

## Do Not Touch

- `src/livevalue.rs` and the payload shape
- `src/watcher.rs` and the drop-file contract
- `mixxx-now-playing/**`
- The relay client behavior

## Constraints

- **A log failure never stops a publish.** Catch the error, warn through
  `tracing`, and continue. Payment routing is the primary duty.
- Record `observed_at` from the producer-time source approved under ADR 0003.
  Do not substitute queue time or file-read time for an unknown producer time.
- Record `aired_at` at the actual send. Do not calculate it from the configured delay.
- `queued_at` and `due_at` are monotonic `Instant` values. They are not wall-clock timestamps.
- Append and flush one line at a time. Never rewrite a line and never rewrite
  the file.
- Never write a token, and never write a fallback destination address that the
  operator marked private.
- A `clear` event is logged with the same fields it has, which is the target,
  the event GUID, the block GUID of the fallback, and the two times.
- The log is opt-in. With no log path configured, write nothing and stay
  silent.
- Do not add a show concept. This service cannot detect a show boundary.

## Implementation Steps

1. Add `src/showlog.rs` and declare it in `src/lib.rs`.
2. Define `ShowLogEntry` with the exact field list of ADR 0003, and a `kind`
   enum with `Track` and `Clear`.
3. Serialize one entry to one line of JSON with no newline inside it.
4. Add `ShowLog` with `open(path)` and `append(entry)`. `append` writes the
   line, writes a newline, and flushes.
5. Specify the log-path and retention settings before implementation.
   The packet must explain how the operator enables logging and resolves the
   instance state path. Missing log configuration must create no file.
6. Carry the approved producer timestamp with the payload.
   Record the actual send time as `aired_at`. Preserve both facts in the entry.
7. Record the publish result after the relay answers. A retryable result is
   logged with its own value, not as a success.
8. On startup, remove a log file older than the retention period. Log the
   count.
9. Add tests:
   - a track entry holds every ADR 0003 field
   - injected producer and send times remain unchanged, including when their difference exceeds the configured delay
   - a clear entry is written when playback stops
   - a second entry for the same block GUID is appended, not merged
   - a write failure warns and the publish loop continues
   - no token appears in any line
   - with no path configured, no file is created

## Acceptance Criteria

- The log holds one valid JSON object for each line.
- Tests prove that the entry preserves the supplied producer time and actual send time.
- A route revision appears as a second line for the same block GUID.
- A failed log write does not stop publishing.
- No secret appears in the log.
- The service behaves exactly as before when the log is not configured.

## Test Commands

- `cargo fmt --all -- --check`
- `cargo check --workspace --quiet`
- `cargo test --workspace --quiet`
- `cargo clippy --workspace --quiet -- -D warnings`

## Expected Final Report Format

1. Files changed
2. Tests run
3. Behavior changed
4. Deviations from task
5. Unresolved concerns

## Escalation Triggers

- The producer timestamp source remains undefined or requires a drop-file schema change.
- The release path cannot preserve the approved timestamp without a schedule contract change.
- The contract does not define timestamps for a failed or unsent publication.
- A publish result is not available at the point the entry is written. Report
  it rather than logging an optimistic result.

## Prompt for lower-context coding model

You are implementing one bounded task from a larger plan.

Implement only this task. Do not redesign the architecture.

Read:
- `docs/adr/0003-show-log-contract.md`
- `src/schedule.rs`, `src/main.rs`, `src/relay.rs`, `src/config.rs`

Goal:
- Write the `musicindex.showlog/1` append-only JSON Lines log from the publish
  loop.

Constraints:
- A log failure never stops a publish. Warn and continue.
- Preserve the approved producer time. Record `aired_at` at the actual send.
- Append and flush one line at a time. Never rewrite.
- Never write a token. Opt-in: no configured path means no file.
- Do not add a show concept.

Do not touch:
- the payload shape, the drop-file contract, `mixxx-now-playing`, relay
  behavior

Acceptance criteria:
- Tests prove that every ADR 0003 field is present and both recorded times remain unchanged. Revisions append.
- A write failure does not stop the loop. No secret in the log.

Test commands:
- `cargo fmt --all -- --check`
- `cargo test --workspace --quiet`
- `cargo clippy --workspace --quiet -- -D warnings`

At the end, report:
1. files changed
2. tests run
3. behavior changed
4. deviations from task
5. unresolved concerns
