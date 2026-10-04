# Display Path Review Checklist

## Scope

Use this checklist after each display path task lands, and again after task
004.

Reviewed work:

- `docs/adr/0008-display-path.md`
- `docs/plans/display-path.md`
- `docs/tasks/display-path-task-001-song-file.md` to
  `docs/tasks/display-path-task-004-publisher.md`
- The implementation diff for each task

## Required Checks

Payment isolation:

- The drop file, its schema and the live value do not change.
- Every payment link test passes with no change.
- No display request runs in a payload worker. A payload never waits for a
  display request.
- The dead block rules do not change.

The song file:

- No code path deletes `now-playing.txt`.
- A `null` display state writes the file with no text, also at startup and at
  exit.

The producer output:

- `DIR` is never the drop directory.
- `DIR` holds two images at most.
- An image is written before `display.json`, each through a rename.
- The producer never fetches a URL.
- No image larger than 524,288 bytes leaves the producer.
- No decode starts before the pixel size check.

The publisher:

- A display entry passes through the stream delay of its target.
- No display failure is fatal.
- The token never appears in a log line, an error or a `Debug` output.

Scope:

- No task changed a file on its "Do Not Touch" list.
- `Cargo.lock` adds only the `image` crate and its dependencies.

## Test Commands

- `cargo fmt --all -- --check`
- `cargo build --workspace`
- `cargo test --workspace`
- `cargo clippy --workspace --all-targets -- -D warnings`
- `node --test mixxx/tests/`

## Manual Checks

Record each result here. A check that did not run is an open gate.

1. A pause in Mixxx clears the Icecast title and the artwork. A resume gives
   both back.
2. After the producer stops, the stream shows no title. Pass on 2026-10-04,
   after task 001: `systemctl --user stop` gave an empty song name in `butt`.
3. With the private app, the artwork changes when the listener hears the new
   track. Check a V4V track and a track that pays nobody.

## Review Result

Status: Open.
