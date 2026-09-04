# Task 003 — Tag reading and metadata rendering

Part of [Rust now-playing utility plan](../plans/rust-now-playing-utility-plan.md), Phase 1.

## Goal

Read embedded metadata from an audio file with `lofty`, normalise MusicIndex keys
across ID3 and Vorbis, and render the metadata text block. Also return the track
duration, which task 005 needs.

## Files To Inspect

- `docs/plans/rust-now-playing-utility-plan.md` — Metadata file format
- `~/build/v4vmm/src/metadata.rs:3395` — `id3_frame_hint()`, source of the tag vocabulary
- `~/build/v4vmm/src/api.rs:238` — `PaymentRoute` shape

## Files Likely To Change

- `mixxx-now-playing/src/tags.rs` (new)
- `mixxx-now-playing/src/render.rs` (new)
- `mixxx-now-playing/tests/fixtures/` (new — one tagged mp3, one tagged flac)

## Do Not Touch

- Task 002's history logic
- Anything under `~/build/v4vmm/` — copy the vocabulary, do not import it

## Constraints

- Use `lofty` only. Do not add the `id3` crate — `lofty` covers both tag formats.
  Verified: MP3 yields `Unknown("MusicIndex Feed Guid")`, FLAC yields
  `Unknown("MUSICINDEX FEED GUID")`, both readable through one API.
- **Exclude transcripts entirely.** Drop `USLT` and `SYLT:MusicIndex Transcript`.
  Do not summarise them, do not emit a placeholder.
- **Never inline binary payloads.** `APIC`, `PRIV` and `UFID` render as a summary
  line only: `APIC = image/jpeg, 42.1 KB`.
- Value routes stay as JSON. Do not flatten to a display summary.
- Keep the MusicIndex vocabulary as a local `const` table of roughly 20 entries.

## Implementation Steps

1. `read_tags(path) -> Result<TrackTags>` where `TrackTags` holds the MusicIndex
   fields, the remaining tag items, and `duration: Option<Duration>`.
2. Normalise keys for matching: uppercase, then collapse internal whitespace to a
   single space. `MUSICINDEX FEED GUID` and `MusicIndex Feed Guid` must both map
   to canonical `Feed Guid`.
3. Build the const vocabulary table mapping canonical names to their frame labels,
   copied from `id3_frame_hint()`. Include at minimum: Feed Guid, Track Guid,
   Publisher, Contributors, Value Routes, Nostr Handle.
4. Unknown keys are not dropped — they pass through to the `[Tags]` section.
5. Render to this layout:

   ```text
   Artist - Title

   [MusicIndex]
   Feed Guid     = 1c7a...
   Track Guid    = 9f3e...
   Publisher     = Some Publisher
   Value Routes  = embedded-id3
   [{"recipient_name":"Alice","route_type":"node","split":90}]

   [Tags]
   TIT2 = Test Title
   APIC = image/jpeg, 42.1 KB
   ```

6. The token after `Value Routes =` is the provenance marker. This task always
   emits `embedded-id3`; task 006 adds the other value.
7. Generate the two fixtures and check them in. Keep them ~1 second of silence so
   they stay small.

## Acceptance Criteria

- The MP3 fixture and the FLAC fixture both yield the same canonical MusicIndex
  field names.
- A file with no tags returns empty fields rather than erroring.
- A file `lofty` cannot open returns `Err`, not a panic.
- `duration` is populated for both fixtures.
- No transcript text appears in rendered output for a file that has one.
- No base64 appears in rendered output for a file with embedded artwork.

## Test Commands

```bash
cd mixxx-now-playing
cargo clippy -- -D warnings
cargo test tags
cargo test render
```

## Expected Final Report

- Files created, fixture sizes
- The final const vocabulary table
- Test names and results
- A rendered sample for each fixture, pasted verbatim

## Escalation Triggers

Stop and ask before proceeding if:

- `lofty` cannot read a custom key from either fixture format.
- The canonical name set needs to diverge from `id3_frame_hint()`.
- A fixture would need to exceed roughly 100 KB.
