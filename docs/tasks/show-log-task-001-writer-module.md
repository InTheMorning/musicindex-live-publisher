# Show Log Task 001: The Writer Module

Status: Implemented - 2026-10-19.

Every criterion is mechanical.

## Goal

A module `src/showlog.rs` that writes the entries of ADR 0017 to one JSON
Lines file for each UTC day. No caller uses it in this task.

## Files To Inspect

- `docs/adr/0017-show-log.md` (all of it, also the amendment)
- `src/livevalue.rs` (`LiveValue`, the `value` object of a payload)
- `src/display.rs` (`Artwork`, `ImageMime`)
- `src/lib.rs`
- `Cargo.toml`

## Files Likely To Change

- `src/showlog.rs` (new)
- `src/lib.rs`

## Do Not Touch

- Every other source file.
- `Cargo.toml`. Add no dependency.
- `docs/adr/**`.

## Constraints

- **The types.** One public `ShowLogEntry` with `kind` `track`, `clear` or
  `display`, and the fields of ADR 0017 §Entries. A field with no value is
  not serialized: use `skip_serializing_if`. The JSON keys are the snake case
  names of the ADR, for example `observed_at` and `event_guid`.
- **`value`.** The `track` and `clear` entries hold the `value` object of the
  published payload, as a `serde_json::Value` or as `LiveValue`. It must
  serialize exactly as the payload serializes it.
- **`artwork`.** `{"url": "…"}` or `{"sha256": "…", "mime": "image/jpeg"}`.
- **`publish_result`.** A public enum with the strings `accepted`,
  `retryable`, `dropped` and `fatal`.
- **The time.** A public function `format_observed_at(SystemTime) -> String`
  gives RFC 3339 in UTC with milliseconds, for example
  `2026-10-19T01:03:23.266Z`. Use the civil-from-days algorithm on the
  seconds since the epoch. No dependency.
- **The writer.** A public `ShowLog` that holds the directory and the
  retention. `ShowLog::append(&self, entry: &ShowLogEntry)`:
  - takes the UTC date of `entry.observed_at` (pass the `SystemTime` with the
    entry, or keep it in the entry),
  - opens `showlog-YYYY-MM-DD.jsonl` in append mode, and creates it with mode
    `0600` if it is absent,
  - writes one line, the entry and `\n`, in one `write_all`, then flushes,
  - never returns an error to the caller. On an error, it logs one
    `tracing::warn!` with the path and the error, and returns.
- **Threads.** `ShowLog` is `Send + Sync`. A `Mutex` holds the open file and
  its date, so that two threads never mix their lines. A new date closes the
  old file and opens the new one.
- **Retention.** `ShowLog::remove_expired(&self, now: SystemTime)` removes
  each `showlog-YYYY-MM-DD.jsonl` in the directory whose date is more than
  the retention days before the date of `now`. It removes no other file. It
  warns on an error and continues.
- **Creation.** `ShowLog::open(dir, retention_days)` creates the directory
  with mode `0700` if it is absent. It returns an error only when it cannot
  make or read the directory. The caller decides what to do with that error.
- Never write a token. The entry type has no field that can hold one.

## Implementation Steps

1. Add the types and their `Serialize` implementation.
2. Add `format_observed_at`.
3. Add `ShowLog` with `open`, `append` and `remove_expired`.
4. Export the module in `src/lib.rs`.
5. Add the tests of §Acceptance Criteria.

## Acceptance Criteria

Each item is a unit test in `src/showlog.rs`:

- `format_observed_at` gives `1970-01-01T00:00:00.000Z` for the epoch,
  `2000-02-29T23:59:59.999Z` for a leap day, and
  `2026-10-19T01:03:23.266Z` for that time.
- A `track` entry serializes to one line with `"schema":
  "musicindex.showlog/1"`, `"kind": "track"`, and no key for an absent field.
- A `display` entry with no track has no `artist`, `title` or `artwork` key.
- Two entries of one day go into one file, as two lines that each parse as
  JSON.
- An entry of the next day goes into a second file.
- The file mode is `0600`, and the directory mode is `0700`.
- Two threads that append 100 entries each give 200 lines that each parse.
- `remove_expired` with a retention of 2 days removes the file of 3 days
  before. It keeps the file of 2 days before, the file of today, and a file
  with a different name.
- `append` into a directory that became read-only returns with no panic.

Also: the full gate passes.

## Test Commands

```bash
cargo fmt --all -- --check
cargo build --workspace
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
```

## Escalation Triggers

- The format of `value` cannot match the payload without a change to
  `src/livevalue.rs`.
- The time format seems to need a new dependency.

## Prompt for lower-context coding model

You are implementing one bounded task from a larger plan.

Implement only this task. Do not redesign the architecture.

Read:
- docs/adr/0017-show-log.md
- docs/plans/show-log.md
- docs/tasks/show-log-task-001-writer-module.md
- AGENTS.md
- src/livevalue.rs, src/display.rs, src/lib.rs, Cargo.toml

Goal:
- A module src/showlog.rs that writes the entries of ADR 0017 to one JSON Lines file for each UTC day. No caller uses it in this task.

Constraints:
- Follow §Constraints of the packet exactly. No new dependency. An append never returns an error to its caller. One Mutex around the open file.

Do not touch:
- every other source file, Cargo.toml, docs/adr/**

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

## Review Result

Reviewed 2026-10-19. The full gate passes with 523 tests in the workspace.
Task 002 of the instances plan ended with 512. `Cargo.toml` and `Cargo.lock`
did not change.

- The types, the time format, the daily file, the `Mutex`, the modes and
  the retention agree with §Constraints. `append` warns and returns on each
  error.
- `value` is `Option<LiveValue>`, the type that the payload serializes. So
  the log holds the same `value` object as the wire.
- `ShowLogArtwork` is a separate type from `display::Artwork`, because the
  log holds a reference and not the bytes.
- `append` takes the date from the first ten characters of `observed_at`.
  The packet permits this.
- `ShowLog::open` sets mode `0700` only on a directory that it makes. A
  directory that exists keeps its mode. That agrees with the packet. Task 003
  gives the directory through systemd with `StateDirectoryMode=0700`.
- No defect was found.
