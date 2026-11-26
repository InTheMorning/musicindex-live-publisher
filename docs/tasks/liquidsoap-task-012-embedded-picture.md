# Liquidsoap Task 012: The Station Producer Uses The Picture File As The Artwork Fallback

Status: Complete - 2026-11-24. The operator accepted ADR 0028 on 2026-11-24. Station checks 11 to 14 passed on
2026-11-24.

Criteria 1 to 10 are mechanical. Criteria 11 to 14 are station checks for a person. The main session writes the slot
text in `liquidsoap/musicindex-state.liq`. This task does not change that file.

Review, 2026-11-24: an independent review found five defects. The main session corrected four. The producer keys its
picture result by `seq`, track start and picture name (test `restart_with_same_seq_reads_the_new_picture`). The slot
text skips a write when a new track starts during the picture read, keeps the picture at a track mark with no
metadata, and keeps a list of the files to remove. The fifth defect is a limit, recorded in ADR 0028 §Formats: an MP3
file with more than one `APIC` frame gives one frame. Operator choice is not needed: ADR 0028 keeps the rule, and the
limit affects no station file that has one `APIC` frame.

## Goal

`liquidsoap-now-playing` gives image artwork from the picture file of the state when the state has no image URL that
`image_url` accepts (ADR 0028 §The Artwork Sequence). The picture code moves from `mixxx-now-playing` to
`now-playing-drop`, so the two producers use one component (ADR 0028 §One Picture Component).

Read these documents first:

- `docs/archive/adr/0028-the-station-producer-uses-the-embedded-picture.md`, all sections.
- `docs/archive/adr/0026-the-station-producer-writes-the-display-state.md` §What The Producer Writes In `display.json`.
- `docs/adr/0008-display-path.md` §The Embedded Image.

## Base components

| Component | Status | Location | Reason |
| --- | --- | --- | --- |
| Embedded picture rules | extend | mixxx-now-playing/src/display.rs | Move `sniff_image`, `prepare_image`, `reduce`, `image_format`, `reader`, `Rejection`, the four limit constants, `JPEG_QUALITY`, `DECODE_ALLOC_LIMIT`, `embedded_artwork` and `v4v_artwork` to `now-playing-drop`. The station producer then uses the same code. |
| Display writer | reuse | now-playing-drop/src/display.rs | `DisplayOutput` already writes an `Artwork::Image` into the display folder. No change. |
| State file parser | extend | liquidsoap-now-playing/src/state.rs | One optional field `picture`. |
| Producer display track | extend | liquidsoap-now-playing/src/lib.rs | `write_display_track` uses the artwork sequence with a cache for each `seq`. |

## Steps

1. Make the module `now-playing-drop/src/picture.rs` with the moved parts. Add `pub mod picture;` to
   `now-playing-drop/src/lib.rs`. Add `image = { version = "0.25", default-features = false, features = ["jpeg",
   "png"] }` to `now-playing-drop/Cargo.toml`.
2. Rename `v4v_artwork` to `artwork_from(image_tag: Option<&str>, picture: Option<&[u8]>)`. It keeps its rule: an
   image URL that `image_url` accepts wins, else `embedded_artwork(picture)`.
3. Move the unit tests of the moved parts from `mixxx-now-playing/src/display.rs` to `now-playing-drop/src/picture.rs`.
   Keep each assertion. Move the test helper functions that they use. A test of `render_display_json` (for example
   `play_id_goes_out_as_a_string`) stays where it is, or moves to `now-playing-drop/src/display.rs` if it already
   has a copy there. Do not keep two copies of one test.
4. In `mixxx-now-playing/src/display.rs`, remove the moved parts. Keep `file_artwork`. Re-export the moved public
   items from `now_playing_drop::picture`, so `mixxx-now-playing/src/main.rs` and `mixxx-now-playing/tests/display.rs`
   change only the name `v4v_artwork` to `artwork_from`. If `mixxx-now-playing/Cargo.toml` then does not use `image`
   in `src/`, move `image` to `[dev-dependencies]` only if a test uses it. Else remove it.
5. Add `#[serde(default)] pub picture: String` to `StateFile` in `liquidsoap-now-playing/src/state.rs`, with a doc
   comment that names ADR 0028.
6. In `liquidsoap-now-playing/src/lib.rs`:
   - Give `Producer` the state folder: the parent of `state_path`.
   - Add a cache field: the `seq` and the result `Option<Artwork>` of the last picture read.
   - In `write_display_track`, compute the artwork. When `image_url` accepts the state `image`, use
     `Artwork::Url`, and do not read the picture file. Else, when the cache holds the same `seq`, use the cached
     result. Else read the picture file, call `embedded_artwork`, and store the result with the `seq`.
   - The picture name is valid only when it is `picture-` followed by 1 to 20 ASCII digits. Each other name gives
     `None` with no file read.
   - Before the read, examine the file size with `fs::metadata`. A size of more than `MAX_PICTURE_BYTES` gives
     `None` with no read of the content.
   - A missing or unreadable file gives `None` and a `tracing::debug!` line, not a warning.
7. Add the tests of the criteria below. Use the existing test helpers of `liquidsoap-now-playing/src/lib.rs` (for
   example `display_reads`). Extend a helper when a test needs a picture file in the state folder. Do not copy a
   helper.
8. Set the status line of this file to `Implemented - 2026-11-24, pending review`. Do not change other documents.

## Files To Change

- `now-playing-drop/Cargo.toml`, `now-playing-drop/src/lib.rs`, `now-playing-drop/src/picture.rs` (new).
- `mixxx-now-playing/Cargo.toml`, `mixxx-now-playing/src/display.rs`, `mixxx-now-playing/src/main.rs`,
  `mixxx-now-playing/tests/display.rs`.
- `liquidsoap-now-playing/src/state.rs`, `liquidsoap-now-playing/src/lib.rs`, `liquidsoap-now-playing/Cargo.toml`
  only if a test needs a dev dependency.
- `Cargo.lock`.
- This task file, status line only.

## Acceptance Criteria

Mechanical, in `liquidsoap-now-playing/src/lib.rs` tests (ADR 0028 §Verification). Make test images with the crate
`image` in the test, or with small fixed bytes. The test names are suggestions:

1. `a_valid_image_url_wins_over_the_picture_file`: the artwork is `{"url": ...}`.
2. `an_empty_image_uses_a_jpeg_picture_file`: the artwork has the `sha256` of the JPEG bytes, and the file
   `<sha256>.jpg` is in the display folder.
3. `a_refused_image_url_uses_a_png_picture_file`: an `image` of `ftp://host/a.png` and a PNG picture file give an
   image artwork.
4. `no_picture_a_missing_file_or_a_gif_gives_null_artwork`.
5. `a_picture_name_that_is_not_picture_digits_gives_null_artwork`: `../state.json`, `picture-`, `picture-12a`.
6. `a_picture_file_over_64_mib_gives_null_artwork`: the test makes a sparse file with `File::set_len`.
7. `each_chapter_segment_has_the_same_picture_artwork`: a chapter sequence. The test removes the picture file after
   the first segment, and each segment has the same image artwork.
8. `a_jingle_with_the_picture_of_the_last_song_gives_that_artwork`.
9. `a_format_2_state_without_picture_parses`.

Mechanical, in `now-playing-drop` and `mixxx-now-playing`:

10. The moved tests pass in `now-playing-drop`. `mixxx-now-playing/tests/display.rs` passes with the same assertions.

Gate:

- `cargo build --workspace`, `cargo test --workspace`, `cargo clippy --workspace --all-targets -- -D warnings` and
  `cargo fmt --all -- --check` pass.

Station checks, for a person (ADR 0028 §Verification), after the operator installs the slot text:

11. `Songs/8ee8ceec-c0bc-41ed-a80d-5f9aa10aa10c.mp3` plays. The relay display state has an image artwork.
12. A FLAC file with a picture plays. The display state has an image artwork.
13. A file with no picture plays. The artwork is `null`, and the state folder holds no picture file of an earlier
    track.
14. A jingle plays after the file of check 11. The display state has the image artwork of that file.

Result, 2026-11-24 02:12 to 02:25 UTC, from the orchestrator session on the test station: checks 11 to 14 passed. The
station had the binaries of bundle 9203797 and the slot text of commit 8165038. Bundle 9203797 with its own slot text
did not start Liquidsoap (a parse error). Commit 8165038 corrected it and added the test `slot_text`.

- 11: `picture-4` was a JPEG of 700 x 700 pixels. The relay artwork (seq 58) had the `sha256` of `picture-4`.
- 12: a copy of a station FLAC file with a PNG front cover. The relay artwork (seq 66) had the `sha256` of the PNG.
  The slot text removed the picture file of the earlier track.
- 13: the same FLAC file with no picture. The state had an empty `picture`, the folder had no picture file, and the
  relay artwork (seq 68) was `null`.
- 14: a jingle after a song with `picture-7`. The state had `jingle` true and kept `picture-7`. The relay (seq 64)
  kept the artwork.

Not examined: an `APIC` frame with no image data, and the 64 MiB limit on the station. The orchestrator examined the
relay display state, not the app.

## Rules For The Delegate

- Write each code comment in Simplified Technical English. The skill is `/home/citizen/.agents/skills/asd-ste100/SKILL.md`.
- Do not commit. Leave the change in the working tree and report it.
- Do not change `liquidsoap/musicindex-state.liq`, an ADR, `AGENTS.md` or `docs/README.md`.
- If a step needs a decision that ADR 0028 does not give, stop and report it.
