# The Show Log: Phase Plan

Status: Implemented 2026-10-19. The review checklist has no open item. This plan does not make rules. ADR 0017 owns them.
The operator accepted ADR 0017 on 2026-10-19.

## Goal

The publisher writes `musicindex.showlog/1`, one JSON Lines file for each UTC
day in the state directory of its instance. It writes a `track` or `clear`
entry for each publish attempt of each target, and a `display` entry for each
display state that it sends. `v4vmm` can then export an episode from the log.

## Non-Goals

- The export: RSS, chapters, value time splits, art. A `v4vmm` ADR owns it.
- A producer timestamp. ADR 0017 uses the read time of the publisher.
- An image store. ADR 0017 records references only.
- A `status` command that reads the log. It can come after this plan.
- A talk segment. ADR 0015 owns it.

## Assumptions

- No date-time crate is in `Cargo.lock`. A small function can format a
  `SystemTime` as RFC 3339 in UTC with milliseconds, with the civil-from-days
  algorithm. Tests against known times prove it. No new dependency.
- systemd gives a user unit a writable state directory with
  `StateDirectory=`, also with `ProtectHome=read-only`. `%S` is
  `$XDG_STATE_HOME`, which is usually `~/.local/state`.
- `LiveValuePayload` already carries one field that is not serialized,
  `play_id`. A second such field can carry the log facts of a payload from
  the watcher to the relay worker. The golden tests prove that the wire shape
  does not change.

## Affected Modules

| Module | Change |
|---|---|
| `src/showlog.rs` (new) | The entry types, the time format, the daily file, the append, the retention |
| `src/lib.rs` | Export of the module |
| `src/livevalue.rs` | A field of the payload, not serialized, with the log facts |
| `src/watcher.rs` | The read time and the drop-file facts of each payload |
| `src/relay.rs` | The worker writes one entry after each publish outcome |
| `src/main.rs` | The `--state-dir` flag, the start of the log, the `display` entries |
| `src/config.rs` | `show_log_retention_days`, default 365 |
| `systemd/musicindex-live-publisher@.service` | `StateDirectory=` and `--state-dir` |
| `tests/showlog.rs` (new) | Tests of the log from the publish loop |
| `README.md`, the configuration runbook | The log, its place and its retention |

## Sequence

1. [Task 001](../tasks/show-log-task-001-writer-module.md): the module
   `src/showlog.rs`, with no caller.
2. [Task 002](../tasks/show-log-task-002-track-and-clear-entries.md): the
   `track` and `clear` entries from the publish loop, and `--state-dir`.
3. [Task 003](../tasks/show-log-task-003-display-entries-and-unit.md): the
   `display` entries, the unit, the retention setting and the documents.
4. [Task 004](../tasks/show-log-task-004-off-switch.md): the key `show_log`,
   from the amendment of ADR 0017 on 2026-10-19.

Each task needs the one before it. Tasks 002 and 003 can ship by themselves.

## Schema And API Implications

- A new file contract, `musicindex.showlog/1`. This repository owns it.
- A new CLI flag `--state-dir`. With no flag, the publisher writes no log.
- A new configuration key `show_log_retention_days`.
- The live value payload on the wire does not change.

## Risk Areas

- **A log failure stops a publish.** Each write must catch its error and
  warn. A test makes the log directory read-only and shows that the publish
  continues.
- **The wire shape changes.** The new payload field must not serialize. The
  golden tests must pass with no change.
- **A token in the log.** The entry never holds a target token. A test reads
  the log after a publish and searches for the token.
- **Two workers write at once.** Each target has a worker thread. The log
  must write each line whole. One `Mutex` around the open file does this.
- **The wrong time.** `observed_at` must be the read time of the drop file,
  not the time of the publish outcome. A retry must keep the first read
  time.

## Test Strategy

- Unit tests in `src/showlog.rs`: the time format, one line for each entry,
  absent fields, the day file, the retention.
- Tests in `tests/showlog.rs` with the stub relay of `tests/relay.rs`: a
  payable track, a non-payable track, a clear, a retry, a refused payload, a
  display state, and a read-only log directory.

## Rollback Strategy

Revert the commit of each task. The log is a new file, and no other component
reads it yet.

## Review

[Review checklist](../reviews/show-log-review-checklist.md).
