# Tag Read Task 001: The Producer Reads The Image Tag

Status: Ready - 2026-10-04.

Every criterion is mechanical.

## Goal

The producer reads the `TXXX:MusicIndex Image` tag. The drop file then carries
its value in `image`, and podcast apps can show the artwork. The display path
of ADR 0008 can then use the image URL of a V4V track.

## Context

`MUSICINDEX_VOCABULARY` in `mixxx-now-playing/src/tags.rs` has no `Image`
entry, so `musicindex_value("Image")` is always `None`. The drop file field
`image` exists in ADR 0002, but the producer never fills it. The audit review
`docs/reviews/nowplaying-publisher-audit-review.md` records this gap. `v4vmm`
writes the tag with the frame `TXXX:MusicIndex Image`. A probe on 2026-10-04
read all 71 files of the operator's V4V folder with `read_tags`. Each read
passed, and each gave `Image: None`.

No schema changes. The field exists, and this task gives it a value.

## Files To Inspect

- `docs/adr/0002-nowplaying-drop-file-contract.md` (the `image` field)
- `mixxx-now-playing/src/tags.rs` (`MUSICINDEX_VOCABULARY`)
- `mixxx-now-playing/src/render.rs`
- `mixxx-now-playing/tests/render.rs`, `tests/tags.rs` and their fixtures
- `docs/runbooks/musicindex-live-publisher-deploy.md` §Known Gaps

## Files Likely To Change

- `mixxx-now-playing/src/tags.rs`
- `mixxx-now-playing/tests/tags.rs`, `tests/render.rs`
- `docs/runbooks/musicindex-live-publisher-deploy.md`

## Do Not Touch

- `src/**` (the publisher)
- The other vocabulary entries
- `docs/adr/**`

## Constraints

- Add `MusicIndexFrame::new("Image", "TXXX:MusicIndex Image")`.
- The drop file `image` holds the tag value as it is. The producer does not
  fetch, check or change the URL.
- An empty tag value gives `image: null`.
- Remove the line about `image` from §Known Gaps in the deploy runbook.

## Acceptance Criteria

Each item is a test:

- A file with `TXXX:MusicIndex Image` gives that value from
  `musicindex_value("Image")`.
- The rendered drop file of that file holds the value in `image`.
- A file with no such tag gives `image: null`.
- The new tests fail when the vocabulary entry is removed.

Also: the full gate passes. `Cargo.lock` does not change.

## Test Commands

```bash
cargo fmt --all -- --check
cargo build --workspace
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
```

## Escalation Triggers

- The publisher changes `image` in a way that this task does not expect. Check
  `src/livevalue.rs` and the golden tests.
