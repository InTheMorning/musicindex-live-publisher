# The Show Log: Review Checklist

Status: open - 2026-10-19. ADR 0017 becomes `Implemented` only when each item
passes.

## Invariants Of ADR 0017

- [ ] The log is append-only. No line is changed or removed in place. Only
  the retention removes a whole file.
- [ ] No token goes into the log. A test in task 002 shows it.
- [ ] A log failure never stops a publish. A test in task 002 shows it.
- [ ] `observed_at` is the read time of the drop file or of `display.json`.
  A retry keeps the first read time.
- [ ] `value` is the published value, with no change.
- [ ] The publisher stores no image.

## Code

- [ ] The serialized payload did not change. The golden tests pass with no
  change.
- [ ] The watcher ignores the log facts when it compares payloads.
- [ ] One `Mutex` keeps the lines of two workers apart.
- [ ] `Cargo.toml` and `Cargo.lock` did not change.
- [ ] No test sleeps for a real day. The day change uses a `SystemTime`
  that the test gives.

## Documents

- [ ] `README.md` and the configuration runbook describe the log and name
  ADR 0017.
- [ ] ADR 0017 gets the status `Implemented`, with a dated sentence that
  names this checklist.
- [ ] `AGENTS.md` §Current State describes the present.

## Manual

- [ ] One show with the unit gives a file of the day in
  `~/.local/state/musicindex-live-publisher/mixxx/showlog/`, with `track`,
  `clear` and `display` entries.
