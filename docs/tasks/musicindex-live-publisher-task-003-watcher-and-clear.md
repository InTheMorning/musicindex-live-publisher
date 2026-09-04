# Task 003 — Drop directory watcher and clear semantics

Part of [MusicIndex live publisher plan](../plans/musicindex-live-publisher-plan.md), Phase 2.

## Goal

Watch the drop directory and turn filesystem events into payloads. Emit a track
payload on create or modify, and the fallback payload on removal. Still no
network — `--dry-run` prints what would be published.

## Files To Inspect

- `docs/plans/musicindex-live-publisher-plan.md` — Input, Clearing
- Task 001's `parse`, task 002's transform and `fallback_payload`

## Files Likely To Change

- `src/watcher.rs` (new)
- `src/main.rs`
- `tests/watcher.rs` (new)

## Do Not Touch

- Task 002's transform — this task decides *when* a payload is built, not how

## Constraints

- **Removal must publish the fallback, never nothing.** If the drop file
  disappears and no payload follows, the relay keeps serving the previous track's
  destinations and listener boosts route to the wrong artist. This is the single
  most important behaviour in the project.
- Producers write with temp-file-plus-rename, so the watcher sees a rename into
  place. Handle create, modify and rename-to events; ignore temp files that do not
  match the drop-file naming rule.
- A file that fails to parse is logged and skipped. It must not stop the watcher
  or trigger the fallback — a malformed file is not the same as a stopped track.
- The watch directory may not exist at startup. Wait for it rather than exiting.
- Reuse one `blockGuid` per drop-file identity, so a modify event on the same
  track republishes the same block rather than reading as a new one.
- Debounce rapid successive events on the same path.

## Implementation Steps

1. Add flags: `--watch-dir`, `--dry-run`, `--config`, `--verbose`.
2. Set up `notify` on the watch directory in recommended mode.
3. On create, modify or rename-in: read, parse, transform, emit. Assign a
   `blockGuid` on first sight of a path and cache it; reuse on modify.
4. On remove: emit the fallback payload with a **fresh** `blockGuid`, and evict
   the cached guid for that path.
5. Handle the directory not existing: poll for it, log once, begin watching when
   it appears.
6. Under `--dry-run`, pretty-print each payload to stdout instead of publishing.
   This is the only output path in this task.
7. Emit an initial state at startup: if a drop file is already present, publish
   it; if the directory is empty, publish the fallback.

## Acceptance Criteria

- Create a drop file → track payload emitted.
- Modify it → payload re-emitted with the **same** `blockGuid`.
- Remove it → fallback payload emitted with a **different** `blockGuid`.
- Write a malformed file → logged, skipped, **no fallback emitted**.
- Write an unknown schema version → logged, skipped, no fallback emitted.
- Temp file written then renamed into place → exactly one payload, not two.
- Start with a non-existent watch directory → waits, then works once created.
- Start with a file already present → publishes it without waiting for an event.
- Tests use `tempfile` and assert on emitted payloads, not on inotify timing.

## Test Commands

```bash
cd musicindex-live-publisher
cargo clippy -- -D warnings
cargo test watcher
```

## Expected Final Report

- Files created or changed
- The event matrix and results, especially malformed-file-does-not-clear
- How debouncing was implemented and its window
- A sample `--dry-run` session covering create, modify, remove

## Escalation Triggers

Stop and ask before proceeding if:

- `notify` on this platform delivers events that cannot distinguish removal from
  a rename-away, since the two need different handling.
- Debouncing would risk dropping a genuine track change.
- The initial-state rule appears to conflict with a target having no configured
  fallback — that is task 004's validation, not something to work around here.
