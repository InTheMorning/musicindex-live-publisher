# Display Path Task 003: The Producer Display Output

Status: Implemented - 2026-10-04.

Every criterion is mechanical.

## Goal

With `--display-dir DIR`, the producer writes `DIR/display.json` and the
embedded image of each track, by the rules of ADR 0008.

## Files To Inspect

- `docs/adr/0008-display-path.md` (§The Producer Output, §The Artwork Source,
  §The Embedded Image, §Invariants)
- `mixxx-now-playing/src/connector/link.rs` (the display action of task 002)
- `mixxx-now-playing/src/main.rs`, `cli.rs`, `tags.rs`, `sink.rs`
- `mixxx-now-playing/Cargo.toml`

## Files Likely To Change

- `mixxx-now-playing/src/display.rs` (new)
- `mixxx-now-playing/src/lib.rs`
- `mixxx-now-playing/src/main.rs`
- `mixxx-now-playing/src/cli.rs`
- `mixxx-now-playing/src/tags.rs`, to read the embedded pictures
- `mixxx-now-playing/Cargo.toml` and `Cargo.lock`

## Do Not Touch

- The payment link and the drop file
- `src/**` (the publisher)
- `docs/adr/**`

## Constraints

- `--display-dir DIR` turns the output on. Without it, no display file is
  written. `DIR` equal to the drop directory is a startup error that names
  ADR 0008.
- `display.json` has the schema `musicindex.display/1` and the shape of ADR
  0008 §The Producer Output.
- The artwork source, for a display track:
  - A V4V track with an `http` or `https` URL of at most 2,048 characters in
    its `MusicIndex Image` tag gives `{"url": …}`. The producer does not fetch
    the URL and writes no image file.
  - Every other track uses its embedded image.
- The embedded image rules:
  - the front cover, else the first picture,
  - JPEG or PNG only, by the first bytes,
  - data over 64 MiB is rejected,
  - the pixel size comes from the header before a decode. A header that cannot
    be read, or a side over 4,000 pixels, gives no image,
  - an image of 524,288 bytes or less, with no side over 1,000 pixels, goes out
    unchanged,
  - every other image is reduced to 1,000 pixels on its longest side and
    written as JPEG. A result over 524,288 bytes gives no image.
- Add the crate `image` with `default-features = false` and only the `jpeg`
  and `png` features. Use it for the header read, the decode, the reduction
  and the JPEG encode.
- The image file is `DIR/<sha256>.jpg` or `DIR/<sha256>.png`, with the
  SHA-256 of the bytes that go out. Write it before `display.json`. Write both
  through a temporary file and a rename.
- After each `display.json` write, keep only the image of the present state and
  of the state before it. Delete every other image file in `DIR`.
- A display `Null` writes `{"schema": "musicindex.display/1", "track": null}`.
  The producer writes it at startup and before it exits.
- A file that cannot be read, or a drive that is not mounted, gives
  `artwork: null` and no error.
- Do not rewrite `display.json` when its content did not change.

## Implementation Steps

1. Add the picture read to `tags.rs`.
2. Add `display.rs` with the artwork source, the image rules, the writes and
   the retention, as pure functions where possible.
3. Add `--display-dir` and connect the display action to `display.rs`.
4. Add the tests.

## Acceptance Criteria

Each item is a test:

- A V4V track with a URL gives `{"url": …}` and writes no image file.
- A V4V track with no URL uses its embedded image.
- A URL with a scheme other than `http` or `https` is not used.
- A track with no picture gives `artwork: null`.
- A GIF picture gives `artwork: null`.
- A header that cannot be read gives `artwork: null`.
- A side over 4,000 pixels gives `artwork: null`, with no decode.
- A small JPEG goes out with the same bytes.
- A large image comes out as a JPEG of at most 1,000 pixels and at most
  524,288 bytes.
- `display.json` is written after its image.
- A third image deletes the first.
- `--display-dir` equal to the drop directory fails at startup.
- `Null` at startup and at exit writes `"track": null`.
- Each rule was broken on purpose, and a test failed. The report lists each
  mutation.

Also: the full gate passes. `Cargo.lock` adds only the `image` crate, the
`sha2` crate and their dependencies. The operator approved `sha2` on
2026-10-04. The producer uses it for the SHA-256 of the image file name.

## Test Commands

```bash
cargo fmt --all -- --check
cargo build --workspace
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
```

## Escalation Triggers

- `lofty` cannot give the picture data or its type.
- The `image` crate needs a feature other than `jpeg` and `png` for these
  rules.
- A rule here conflicts with ADR 0008.

## Prompt for lower-context coding model

Implement only this task. Read docs/adr/0008-display-path.md, this packet,
task 002, and the producer files in §Files To Inspect. Add the display output
behind --display-dir, exactly as §Constraints says, with the image crate
limited to its jpeg and png features. Do not touch the payment path. Write the
tests in §Acceptance Criteria, with a mutation list. Run the test commands.
Report: 1. files changed 2. tests run 3. behavior changed 4. deviations
5. unresolved concerns 6. mutations.

## Review Result

Reviewed 2026-10-04. The present tests on master did not change. The full
gate passes, and the 27 mapping tests pass. `Cargo.lock` adds only `image`,
`sha2` and their dependencies.

The first version had a separate picture read. A probe on the 71 V4V files
showed that one read is sufficient, so the rework removed that read. The
rework also replaced a SHA-256 written by hand with the crate `sha2`.

The review accepts these deviations:

- `ShutdownCleanup` is the only code that writes the display `null` at exit.
  A second write in `run()` had no test that could fail.
- The image URL is not trimmed. The display URL is the same value as `image`
  in the drop file. An empty value gives no URL, and the track then uses its
  embedded image.

Each display rule was broken on purpose, and a test failed each time. A test
reads a real file with the `TXXX:MusicIndex Image` tag and gets the URL.

Known limit: `lofty` 0.22 does not read a picture frame larger than 16 MiB.
Such a track gets no artwork, and its tag read passes. The 64 MiB check stays,
as the packet requires.
