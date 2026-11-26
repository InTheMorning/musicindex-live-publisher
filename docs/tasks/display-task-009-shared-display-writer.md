# Display Task 009: One Display Writer In `now-playing-drop`

Status: Ready - 2026-11-22. ADR 0026 §One Display Writer is accepted.

Criteria 1 to 4 are mechanical. Criterion 5 is a manual check in the last inspection of the diff. No criterion is
visual.

## Goal

The display writer of `mixxx-now-playing/src/display.rs` moves to a new module `now-playing-drop/src/display.rs`.
`mixxx-now-playing` uses the moved writer. No copy of a moved part stays. Task 010 then uses the same writer in
`liquidsoap-now-playing`.

## Base components

| Component | Status | Location | Reason |
| --- | --- | --- | --- |
| Display writer | extend | mixxx-now-playing/src/display.rs | `DisplayOutput`, `render_display_json`, `ShownTrack`, `Artwork`, `ImageMime`, `ImageFile`, `image_url`, `write_null_at_exit` and `sha256_hex` move to `now-playing-drop/src/display.rs`. `ShownTrack.play_id` becomes a string. Do not keep a copy. |
| Directory compare | reuse | now-playing-drop/src/instance.rs | `same_directory` is there now. Remove the copy in `mixxx-now-playing/src/display.rs`. |
| Atomic file write | extend | now-playing-drop/src/sink.rs | `OutputFile::write_atomic` and the `write_atomic` of the display writer do the same steps. Make one crate function that takes a path and bytes, and use it in the two places. |
| Picture preparation | reuse | mixxx-now-playing/src/display.rs | `sniff_image`, `prepare_image`, `reduce`, `embedded_artwork`, `v4v_artwork` and `file_artwork` stay in Mixxx, so `now-playing-drop` does not use the crate `image`. |

## Files To Inspect

- `docs/archive/adr/0026-the-station-producer-writes-the-display-state.md` §One Display Writer. It owns each rule of this
  task.
- `mixxx-now-playing/src/display.rs`, `mixxx-now-playing/src/main.rs` (the `ShownTrack` at about line 533),
  `mixxx-now-playing/tests/display.rs`, `mixxx-now-playing/tests/lifecycle.rs`.
- `now-playing-drop/src/lib.rs`, `now-playing-drop/src/sink.rs`, `now-playing-drop/src/instance.rs`,
  `now-playing-drop/Cargo.toml`.

## Files To Change

- `now-playing-drop/src/display.rs` (new), `now-playing-drop/src/lib.rs` (`pub mod display;`),
  `now-playing-drop/src/sink.rs`, `now-playing-drop/Cargo.toml` (add `sha2` with the version and features of
  `mixxx-now-playing/Cargo.toml`, and `tracing` only if it is necessary).
- `mixxx-now-playing/src/display.rs`, `mixxx-now-playing/src/main.rs`, `mixxx-now-playing/Cargo.toml` (remove `sha2`
  when no Mixxx code uses it).
- `mixxx-now-playing/tests/*.rs` only when a path or a type changes.
- `Cargo.lock`.

## Do Not Touch

- `src/**` (the publisher), `liquidsoap-now-playing/**`, `liquidsoap/**`, `systemd/**`, `packaging/**`, `docs/**`,
  `AGENTS.md`.
- The content of `display.json` that Mixxx writes. It does not change.
- The constants of the picture limits (`MAX_PICTURE_BYTES`, `MAX_SOURCE_SIDE`, `MAX_OUTPUT_SIDE`, `MAX_OUTPUT_BYTES`)
  and `Rejection`. They stay in Mixxx.

## Constraints

- `now-playing-drop/src/display.rs` has the module comment of ADR 0008 and ADR 0026 §One Display Writer, and these
  public items: `DISPLAY_SCHEMA`, `DISPLAY_FILE_NAME`, `MAX_URL_CHARS`, `ImageMime`, `ImageFile`, `Artwork`,
  `ShownTrack`, `image_url`, `render_display_json`, `DisplayOutput`, `write_null_at_exit`, `sha256_hex`. The private
  helpers `image_in_display_json` and `is_image_file_name` move with them.
- `ImageMime::from_extension` and `ImageMime::from_mime` stay private to the module if only the module uses them.
  `ImageMime::format` (the `image::ImageFormat` map) does not move. Mixxx gets a private function
  `fn image_format(mime: ImageMime) -> ImageFormat` in its `display.rs`.
- `ImageFile::new(mime, bytes)` becomes public, because `prepare_image` in Mixxx makes an `ImageFile`. It computes the
  SHA-256 as today.
- `ShownTrack` stays `Copy`. `play_id` becomes `Option<&'a str>`. `render_display_json` writes it as a JSON string
  without a change, as today.
- In `mixxx-now-playing/src/main.rs`, the producer makes the string from `hist_id` with `to_string()` and gives a
  reference. The `display.json` of Mixxx is thus the same as before.
- `mixxx-now-playing/src/display.rs` keeps the path of each moved item with `pub use now_playing_drop::display::{...};`,
  so `mixxx_now_playing::display::sha256_hex` and the other paths in the Mixxx tests stay. Its module comment says
  that the writer is in `now-playing-drop` (ADR 0026 §One Display Writer).
- `same_directory` in Mixxx: remove the copy. If a Mixxx path uses `display::same_directory`, use
  `now_playing_drop::same_directory` or the path that `now-playing-drop` exports.
- The atomic write: one `pub(crate)` or `pub` function in `now-playing-drop/src/sink.rs`, for example
  `pub fn write_atomic(path: &Path, bytes: &[u8]) -> Result<()>`. Keep the error texts of today. `OutputFile` uses it.
- Each code comment is in Simplified Technical English. Read `/home/citizen/.agents/skills/asd-ste100/SKILL.md`.

### The tests

- Each test of a moved part moves to `now-playing-drop/src/display.rs` with the code. Each assertion stays the same.
- A moved test that makes an image with the crate `image` makes an `ImageFile` with `ImageFile::new` and fixed bytes.
  Use bytes with the first bytes of a JPEG or a PNG when the test needs a valid type. Change only the setup.
- A test of a picture rule (`sniff_image`, `prepare_image`, the reduce, the side limit, `v4v_artwork`) stays in Mixxx.
- `same_directory_compares_resolved_paths` moves only if `now-playing-drop` has no test of the same function. Do not
  keep two.
- Add one test in `mixxx-now-playing`: a `ShownTrack` with `play_id` `Some("42")` gives `"play_id": "42"` in
  `display.json`.

## Acceptance Criteria

1. Mechanical: `grep -n "fn render_display_json\|struct DisplayOutput\|struct ShownTrack\|fn same_directory\|fn sha256_hex\|fn write_atomic"
   mixxx-now-playing/src/*.rs` shows nothing.
2. Mechanical: `cargo tree -p now-playing-drop -e normal | grep -c " image v"` gives `0`.
3. Mechanical: the moved tests pass in `now-playing-drop`. The Mixxx tests pass. The test of `play_id` `"42"` passes.
4. Mechanical: `cargo build`, `cargo test --workspace`, `cargo clippy --workspace --all-targets -- -D warnings` and
   `cargo fmt --all -- --check` pass.
5. Manual, in the last review of the diff: each moved test keeps its assertions. A test cannot compare a test with its
   earlier version.
