# Rust now-playing review checklist

Use this checklist for the final diff against
[ADR 0001](../adr/0001-rust-now-playing-lifecycle.md), the
[phase plan](../plans/rust-now-playing-utility-plan.md), and the six task
packets under `docs/tasks/`.

## Reviewed Artifact

- `mixxx-now-playing/` crate
- Documentation updates under `docs/`

## Required Checks

- Pass/fail recorded for each task packet's acceptance criteria.
- No edits to root shell/Python scripts unless explicitly requested.
- Runtime dependencies match the phase plan; test-only dependencies are isolated
  under `dev-dependencies`.
- No `unwrap()` or `expect()` outside tests.
- `cargo fmt --check` passes.
- `cargo clippy -- -D warnings` passes.
- `cargo test` passes.

## Architecture Drift

- History watcher keeps one read-only SQLite connection and one prepared query.
- V4V classification is based on canonical `path.starts_with(root)`.
- Metadata-file presence remains the public signal.
- Expiry only removes metadata; it never creates metadata.
- MusicIndex lookup happens off the poll path and discards stale results.

## Missing Tests

- Config precedence and malformed config fallback.
- History empty table, first row, repeated poll, and appended row.
- MP3 and FLAC MusicIndex key normalisation.
- Transcript exclusion and binary-artwork summarisation.
- V4V/non-V4V lifecycle and startup cleanup.
- Expiry with injected clock and no long sleeps.
- API success, track-404/feed-success, unreachable fallback, no routes, cache,
  and late-result discard.

## Merge Recommendation

Merge only when all required checks pass or the remaining gaps are documented
with explicit rollback impact.
