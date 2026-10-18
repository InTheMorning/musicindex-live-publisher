# Instances, Events And The Producer Lock: Review Checklist

Status: open - 2026-10-18. ADR 0016 becomes `Implemented` only when each item
passes.

## Invariants Of ADR 0016

- [ ] Each event has one instance on one host. `target add` and the startup
  check refuse a second instance. Tests in task 002 show it.
- [ ] The producer lock is a kernel lock on a local file system. The lock
  file is in the drop directory in `$XDG_RUNTIME_DIR`.
- [ ] No producer gives the lock descriptor to a child process.
- [ ] A producer waits at most 2 seconds for the lock. A test in task 001
  shows it.

## Code

- [ ] The check reads only the instance directories of a root with the name
  `musicindex-live-publisher`. A test shows that a path with no instance
  layout gives no check.
- [ ] A configuration of a different instance that does not parse does not
  stop this instance.
- [ ] A refused `target add` leaves the file unchanged.
- [ ] No test waits for `LOCK_WAIT_LIMIT`.
- [ ] `Cargo.lock` did not change, or the review records why.

## Manual Checks

These checks have no test, because no incident occurred (ADR 0016
§The Producer Lock).

- [ ] `grep -rn 'process::Command\|Command::new' src mixxx-now-playing/src`
  gives no result. A child process would need a review of rule 3.
- [ ] `systemd/musicindex-live-publisher@.service` keeps the drop directory
  in `%t`, which is `$XDG_RUNTIME_DIR`.

## Documents

- [ ] The configuration runbook states the rule of one instance for each
  event, the two errors, the limit to one host, and the lock wait. It names
  ADR 0016.
- [ ] ADR 0016 gets the status `Implemented`, with a dated sentence that
  names this checklist.
- [ ] `AGENTS.md` §Current State describes the present.
