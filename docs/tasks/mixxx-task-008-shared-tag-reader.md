# Mixxx Task 008: The Shared Tag Reader With The Podcast Frames

Status: Ready - 2026-11-22. Operator choice of 2026-11-22: `mixxx-now-playing` uses `musicindex-tags` at
musicindex-lib `3b7129a`.

Every criterion is mechanical. No criterion is visual.

## Goal

`mixxx-now-playing` reads the tags with `musicindex_tags::track` and deletes its own copy. The reader then reads the
`Podcast` frames of musicindex ADR 0086 R-TAG-2, for example `TXXX:Podcast Item Guid`, and the new row "Chapters"
(`TXXX:Podcast Chapters`, musicindex ADR 0089). It does not read the old `MusicIndex` frames.

## Base components

| Component | Status | Location | Reason |
| --- | --- | --- | --- |
| Tag reader | reuse | ../musicindex-lib/tags/src/track.rs | `musicindex_tags::track` has the same items as `mixxx-now-playing/src/tags.rs`. Only the frame names and the row "Chapters" are different. Delete the copy. |
| Test fixtures | extend | mixxx-now-playing/tests/fixtures/musicindex-tagged.mp3 | The two tagged fixtures get the `Podcast` frame names. The values do not change. |

## Files To Change

- `mixxx-now-playing/Cargo.toml`: add `musicindex-tags` (git musicindex-lib, rev `3b7129a`, the full hash).
- `mixxx-now-playing/src/tags.rs`: delete.
- `mixxx-now-playing/src/lib.rs`: `pub use musicindex_tags::track as tags;`, so that each `crate::tags::...` and
  `mixxx_now_playing::tags::...` path stays.
- `mixxx-now-playing/tests/fixtures/musicindex-tagged.mp3` and `musicindex-tagged.flac`: the frame names change from
  `MusicIndex X` to `Podcast X`, and `MusicIndex Track Guid` changes to `Podcast Item Guid`.
- The tests that write or examine an old frame name.
- `Cargo.lock`.

## Do Not Touch

- The labels of the `[MusicIndex]` section in the metadata file. The row labels, for example "Track Guid", do not
  change.
- `src/**`, `liquidsoap-now-playing/**`, `packaging/**`.

## Acceptance Criteria

All are mechanical.

1. `mixxx-now-playing/src/tags.rs` does not exist. `grep -rn "MusicIndex Track Guid\|MusicIndex Value Routes"
   mixxx-now-playing` shows nothing.
2. The tests of today pass with the new fixtures. The row "Track Guid" comes from `TXXX:Podcast Item Guid`.
3. `cargo build`, `cargo test --workspace`, `cargo clippy --workspace --all-targets -- -D warnings` and
   `cargo fmt --all -- --check` pass.
