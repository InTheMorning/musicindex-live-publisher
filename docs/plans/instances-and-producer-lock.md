# Instances, Events And The Producer Lock: Phase Plan

Status: Implemented 2026-10-18. The review checklist has no open item. This plan does not make rules. ADR 0016 owns them.
The operator accepted ADR 0016 on 2026-10-18.

## Goal

- A producer waits at most 2 seconds for its lock. After that, it exits with
  an error that names the drop directory.
- `target add` refuses an `event_id` that a different instance holds.
- At startup, the publisher exits with an error when a different instance
  holds one of its events. The error names the two instances.

## Non-Goals

- No session binding of a drop file to the lock holder. ADR 0016 defers it.
- No systemd watchdog. ADR 0016 keeps it as an open question.
- No producer template unit. No producer that can run more than one time
  exists yet.
- No check across two hosts. Only the custody of the token prevents a
  second writer there.
- No new exit code. The refusal of `target add` uses exit code 1. ADR 0004
  owns the exit codes of the control CLI.

## Assumptions

- Each instance uses the layout of the template unit:
  `<root>/<instance>/config.toml`, where `<root>` is a directory with the
  name `musicindex-live-publisher`. A configuration at a different path,
  for example `/etc/musicindex-live-publisher/config.toml`, has no instance.
  The check then does not run.
- The producer and the publisher start no child process. No
  `std::process::Command` exists in either crate. So rule 3 of
  §The Producer Lock already holds.
- `std::fs::File::try_lock` gives `TryLockError::WouldBlock` while a
  different file handle holds a shared or an exclusive lock.

## Affected Modules

| Module | Change |
|---|---|
| `mixxx-now-playing/src/lock.rs` | `ProducerLock::acquire` tries the lock until a limit of 2 seconds, then gives an error |
| `src/config.rs` | A function finds the other instances and their events, and a function refuses an event that a different instance holds |
| `src/main.rs` | `target add` and the startup of the publisher call the check |
| `tests/config.rs` | The tests of the check, with an instance layout in `tempfile` |
| The configuration runbook | The rule of one instance for each event, the errors, and the lock wait |

## Sequence

1. [Task 001](../tasks/instances-task-001-producer-lock-wait.md): the lock
   wait limit in the producer.
2. [Task 002](../tasks/instances-task-002-event-ownership-checks.md): the
   event checks in the publisher.

The two tasks are independent. Each task can ship by itself.

## Schema And API Implications

- The drop file and the payload do not change.
- `target add` can fail with a new error message and exit code 1.
- The publisher can stop at startup with a new error. The unit restarts it
  five times, then systemd marks it `failed` (mandate 7).

## Risk Areas

- **A wrong refusal.** The check must not count the instance itself. A
  `target add --replace` of a target in the same instance must work.
- **One instance stops another.** A configuration of a different instance
  that does not parse must not stop this instance. The check logs a warning
  and skips that file.
- **A wrong root.** The check must read only the instance directories of
  `<root>`. It must never read `/etc/*/config.toml`.
- **A producer that gives up too early.** The publisher holds a shared lock
  for less than one second during a health check. A limit of 2 seconds
  waits out that hold. A test shows that the producer gets the lock when the
  shared lock goes before the limit.

## Test Strategy

- Unit tests in `mixxx-now-playing/src/lock.rs` with short limits. No test
  waits 2 seconds.
- Tests in `tests/config.rs` with a layout of two or three instances in a
  `tempfile` directory.

## Rollback Strategy

Revert the commit of each task. No stored data depends on the change.

## Review

[Review checklist](../reviews/instances-review-checklist.md).
