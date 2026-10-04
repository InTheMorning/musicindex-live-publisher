# Tag Read Task 001: The Payment Tag Read Skips The Cover Art

Status: Ready - 2026-10-04.

Every criterion is mechanical.

## Goal

A V4V file with a large embedded picture still gives its tags, so the track
pays its artist. The tag read never reads the cover art.

## Context

`read_tags` in `mixxx-now-playing/src/tags.rs` calls `lofty::read_from_path`
with the default options. Those options read the cover art. `lofty` 0.22 limits
each allocation to 16 MiB. The art survey of 2026-10-04 found a V4V file with a
32 MiB embedded picture. For that file, the tag read fails. The producer then
writes no drop file, so the track pays nobody.

No rule changes. This task corrects a defect in the payment path
(AGENTS.md §4: use the supplied metadata).

## Files To Inspect

- `mixxx-now-playing/src/tags.rs` (`read_tags`, `collect_pictures`)
- `mixxx-now-playing/src/main.rs` (`process_track`)
- `mixxx-now-playing/tests/tags.rs`
- `lofty` 0.22: `ParseOptions::read_cover_art`

## Files Likely To Change

- `mixxx-now-playing/src/tags.rs`
- `mixxx-now-playing/tests/tags.rs`

## Do Not Touch

- `mixxx-now-playing/src/connector/**`
- The drop-file fields and their values
- `src/**` (the publisher)
- `docs/adr/**`

## Constraints

- `read_tags` reads with `ParseOptions::new().read_cover_art(false)`. It reads
  the tags and the audio properties. It gives the same `TrackTags` as before,
  except that it holds no picture.
- If a present caller uses a picture from `read_tags`, report it before you
  change the caller.
- Do not change the global `lofty` options.
- No new dependency.

## Acceptance Criteria

Each item is a test:

- A file with an embedded picture larger than 16 MiB gives its tags and its
  duration from `read_tags`. Make the file in the test, in a `tempfile`
  directory.
- The same file with its MusicIndex tags gives a drop file through the
  present render path. The drop file has the same fields as for a file with no
  picture.
- Every present test passes with no change.
- The new test fails when `read_cover_art(false)` is removed.

Also: the full gate passes. `Cargo.lock` does not change.

## Test Commands

```bash
cargo fmt --all -- --check
cargo build --workspace
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
```

## Escalation Triggers

- `read_cover_art(false)` does not prevent the large allocation for one of the
  file types.
- A present caller needs the picture from `read_tags`.
