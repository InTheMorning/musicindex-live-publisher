# ADR 0001: Rust now-playing lifecycle

Status: Implemented
Date: 2026-09-03

Implemented 2026-09-04: `mixxx-now-playing/` implements this ADR. The audit
review `docs/reviews/nowplaying-publisher-audit-review.md` of that date
reviewed the implementation against the task packets in
`docs/plans/rust-now-playing-utility-plan.md`. The status was corrected on
2026-09-30.

Accepted 2026-09-03.

Amended 2026-09-28: ADR 0006 supersedes the non-goal "Detecting pause or exact
audio output state" when the MIDI connector is available. Without the
connector, the producer still cannot detect a pause.

## Context

The original `scripts/mixxx-now-playing.sh` kept a now-playing text file current
with the latest Mixxx set-log entry. The Python replacement attempts to expose
V4V metadata, but it leaves the metadata file present for non-V4V tracks, reads
only ID3 tags, guesses the V4V root, and reconnects to SQLite on every poll.

The consumer API for V4V metadata is file presence. When a V4V track is playing,
the metadata file exists. When no V4V track is playing, the metadata file does
not exist.

## Decision

Build a standalone Rust binary in `mixxx-now-playing/` that keeps the now-playing
text file compatible with the shell script and manages a separate metadata file
whose presence tracks V4V playback.

The binary uses:

- A long-lived read-only `rusqlite` connection to the Mixxx 2.5.6 set-log query.
- A startup-resolved V4V root from flags, environment, v4vmm config, or
  `~/V4Vmusic`.
- `lofty` for cross-format embedded metadata and duration reads.
- Atomic output-file replacement and deduplicated writes.
- An expiry timer to remove metadata after the final track of a set.
- A worker thread for MusicIndex API value-route resolution.

## Alternatives Considered

- Keep the Python script and patch the lifecycle defect. Rejected because it
  remains ID3-only and keeps broad schema probing that the pinned Mixxx version
  does not need.
- Extend the shell script. Rejected because cross-format tag reading, expiry, and
  API fallback logic are awkward and fragile in shell.
- Import v4vmm code directly. Rejected because this utility should remain small,
  standalone, and read-only against external state.

## Consequences

- The Rust binary can be tested with a synthetic Mixxx database and small audio
  fixtures.
- Runtime rollback is simple because the shell script remains unchanged.
- The metadata file may disappear during a long pause unless `--expiry none` is
  used.
- Value routes may be rewritten once after track change when the API result
  arrives.

## Invariants

- The metadata file must not exist for non-V4V tracks.
- The now-playing text output must preserve the shell script's formatting quirks
  by default.
- SQLite access is read-only and keeps one connection open.
- Tag reading is read-only and excludes transcripts.
- API lookup cannot block the poll path.
- Output writes are atomic and identical content is not rewritten.

## Non-Goals

- Replacing `scripts/mixxx-now-playing.sh` immediately.
- Detecting pause or exact audio output state. ADR 0006 supersedes this
  non-goal when the MIDI connector is available.
- Editing tags, the Mixxx database, or MusicIndex data.
- Supporting Mixxx schemas beyond the documented 2.5.6 layout.
- Reimplementing v4vmm.
