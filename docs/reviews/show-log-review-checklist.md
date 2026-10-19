# The Show Log: Review Checklist

Status: closed - 2026-10-19. Each item passes. This checklist is the named
artifact for the `Implemented` status of ADR 0017.

## Invariants Of ADR 0017

- [x] The log is append-only. No line is changed or removed in place. Only
  the retention removes a whole file.
- [x] No token goes into the log. A test in task 002 shows it.
- [x] A log failure never stops a publish. A test in task 002 shows it.
- [x] `observed_at` is the read time of the drop file or of `display.json`.
  A retry keeps the first read time.
- [x] `value` is the published value, with no change.
- [x] The publisher stores no image.

## Code

- [x] The serialized payload did not change. The golden tests pass with no
  change.
- [x] The watcher ignores the log facts when it compares payloads.
- [x] One `Mutex` keeps the lines of two workers apart.
- [x] `Cargo.toml` and `Cargo.lock` did not change.
- [x] No test sleeps for a real day. The day change uses a `SystemTime`
  that the test gives.

## Documents

- [x] `README.md` and the configuration runbook describe the log and name
  ADR 0017.
- [x] ADR 0017 gets the status `Implemented`, with a dated sentence that
  names this checklist.
- [x] `AGENTS.md` §Current State describes the present.

## Manual

- [x] One show with the unit gives a file of the day in
  `~/.local/state/musicindex-live-publisher/mixxx/showlog/`, with `track`,
  `clear` and `display` entries.

## Manual Result

The operator ran the check on 2026-10-19 with the unit that
`scripts/setup-mixxx-musicindex.sh --units-only` wrote.

- `showlog-2026-10-19.jsonl` has mode `-rw-------`.
- Two restarts gave a `clear` entry and a `display` entry each, at
  16:47:25.771Z and 16:47:27.977Z.
- Three tracks gave a `track` entry and a `display` entry each: "Nothing Is
  Easy", "Kick Your Arse" and "Bipolar". Each publish was `accepted`.
