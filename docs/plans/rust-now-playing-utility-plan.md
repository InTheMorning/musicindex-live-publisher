# Rust Now-Playing Utility Plan

Status: Proposed
Date: 2026-09-03

## Goal

Replace `mixxx-now-playing.py` with a Rust utility that does two jobs:

1. Keep `$XDG_RUNTIME_DIR/musicindex-live-publisher/mixxx/nowplaying/now-playing.txt`
   current with the playing track, matching the behaviour of
   `scripts/mixxx-now-playing.sh` exactly.
2. Write
   `$XDG_RUNTIME_DIR/musicindex-live-publisher/mixxx/nowplaying/metadata.txt`
   **only while** a track from the V4V library is playing, containing that
   track's embedded metadata with the custom MusicIndex tags surfaced first.

Presence of the metadata file is the signal. When no V4V track is playing, the
file must not exist.

"Playing" is defined by the set log plus an expiry timer: the file appears when a
V4V track enters the Mixxx history and is removed when either the next non-V4V
track starts or the timer runs out. It is a scheduling approximation, not a
measurement of audio output.

## Non-Goals

- Replacing `scripts/mixxx-now-playing.sh`. It works; it stays until the Rust binary has
  run a full show without drift.
- Detecting pause. A paused deck continues to count as playing until its timer
  expires.
- Tag editing. This utility is read-only against the Mixxx database, the audio
  files, and the MusicIndex API.
- Supporting Mixxx versions other than 2.5.6, or Flatpak install layouts.
- Reimplementing `v4vmm`. This utility reads its config and its tag vocabulary,
  and shares no code with it.
- Transcript extraction. `USLT` and `SYLT:MusicIndex Transcript` are excluded
  from the output entirely.

## Current State

### `scripts/mixxx-now-playing.sh` — works

Polls the Mixxx set log every 0.5s, writes `artist - title` on history change.
Strips every `-` from the line before substituting the `|` separator, so
`Test-Artist` renders as `TestArtist`.

### `mixxx-now-playing.py` — does not meet the goal

Verified against a synthetic Mixxx 2.5.6 database with real tagged fixtures
(one MP3 with ID3v2 TXXX frames, one FLAC with Vorbis comments):

| # | Defect | Evidence |
|---|--------|----------|
| 1 | **Wrong lifecycle.** Writes `mixxx-v4vmusic-id3.json` on every track. A non-V4V track leaves the file in place with `"status": "not_v4vmusic"`. No `unlink` call exists in the script. | Non-V4V track played; file still present |
| 2 | **ID3-only.** `v4vmm` ships mp3/flac/m4a/ogg/wav and transcodes WAV to FLAC, so FLAC tracks are a blind spot. | Tagged FLAC returned `id3_error: file does not contain an ID3 tag`, `metadata: {}` |
| 3 | **Binary frames inlined.** APIC, PRIV and UFID payloads are base64-encoded into the output. Useless for a text overlay and it bloats the file. | `extract_id3_metadata` |
| 4 | **Guesses the V4V root.** Real default is `~/V4Vmusic` (lowercase `m`) and it is user-configurable. The script matches a hardcoded `v4vmusic` path component and only works because the comparison is case-insensitive. | `v4vmm/src/config.rs:470`, `~/.config/v4vmm/config.toml` |
| 5 | **Dead defensive code.** About 350 of 500 lines probe the schema with `PRAGMA table_info`, build dynamic SQL, and test five database paths, for a schema pinned to one Mixxx version. | `latest_history_row` |

### Why the set log alone is not enough

The Mixxx set log (`Playlists.hidden = 2`) is append-only. A row means a track
**started**. There is no "stopped" row, so a track-change edge on its own leaves
the metadata file in place forever after the last track of a set. The expiry
timer in Phase 2 exists to close that gap.

## Target State

One binary, three separated concerns.

### Dependencies

```toml
rusqlite    = { version = "0.38", features = ["bundled"] }
lofty       = "0.22"
anyhow      = "1"
directories = "6"
serde       = { version = "1", features = ["derive"] }
serde_json  = "1"
toml        = "1.0"
signal-hook = "0.3"
reqwest     = { version = "0.13", features = ["blocking", "json"] }
```

`lofty` resolves defect 2 and supplies the track duration the expiry timer needs,
in the same read. Verified against both fixtures — one API, both tag formats,
custom keys preserved:

```text
track.mp3   Id3v2           Unknown("MusicIndex Feed Guid") = feed-guid-123
track.flac  VorbisComments  Unknown("MUSICINDEX FEED GUID") = feed-guid-789
```

`rusqlite`, `lofty`, `reqwest` and `serde_json` are already `v4vmm`
dependencies. Staying on the same versions keeps one lockfile story across the
two projects.

### Concern 1: source

`HistoryWatcher` holds one long-lived read-only connection (`?mode=ro`, plus
`PRAGMA query_only`) and one prepared statement. It retries on `SQLITE_BUSY`
rather than exiting. It emits `TrackChanged` only when `hist_id` moves.

The schema is fixed, so the join is written literally:

```sql
SELECT pt.id, l.artist, l.title, tl.location
FROM PlaylistTracks pt
JOIN Playlists p  ON p.id = pt.playlist_id
JOIN library l    ON l.id = pt.track_id
JOIN track_locations tl ON tl.id = l.location
WHERE p.hidden = 2
ORDER BY pt.pl_datetime_added DESC, pt.id DESC LIMIT 1
```

### Concern 2: classifier

Resolve the V4V root once at startup. Precedence:

1. `--v4v-root`
2. `$V4V_MUSIC_DIR`
3. `music_dir` in `~/.config/v4vmm/config.toml`
4. `~/V4Vmusic`

Canonicalise both sides, then `path.starts_with(root)`. Reading `v4vmm`'s own
config makes this correct by construction and fixes defect 4.

### Concern 3: sink

Presence is the API. This is the abstraction the Python version lacks.

```rust
enum Presence { Present(String), Absent }

struct OutputFile { path: PathBuf, last: Option<String> }

impl OutputFile {
    fn set(&mut self, p: Presence) -> Result<()> {
        match p {
            // temp file in the same directory, then rename: atomic for readers
            Present(s) if self.last.as_deref() != Some(&s) => self.write_atomic(&s),
            Present(_) => Ok(()),                    // no-op: do not flicker OBS
            Absent     => self.remove(),             // ignore ENOENT
        }
    }
}
```

The de-duplication matters. OBS text sources watch by inotify and re-render on
every write, so rewriting identical bytes twice a second makes overlays twitch.
It also absorbs the second write described under value-route resolution.

### Metadata lifetime

No trait, no play-state abstraction. A single predicate, evaluated each poll:

```rust
struct Expiry { deadline: Option<Instant> }   // None = never expires

// metadata file present  <=>  current track is V4V
//                        &&  (deadline is None || now < deadline)
```

The deadline is armed at track change as `started_at + duration + slack`, where
`duration` comes from `lofty` in the same read as the tags.

Flags:

- `--expiry duration` (default) — arm from the track's own length.
- `--expiry none` — never expire; the next track change is the only thing that
  clears the file.
- `--expiry-slack <secs>` (default 5) — added to the duration, since the timer
  only needs to be roughly right.
- `--expiry-fallback <secs>` (default 600) — used when `lofty` cannot report a
  duration, so an unreadable file cannot pin the metadata forever.

Accuracy barely matters in normal operation. During a set, the next history row
supersedes the current track long before its timer fires. The timer only actually
fires at the end of a set, when Mixxx stops, or when AutoDJ is switched off —
which is exactly the case it exists to handle.

### Value-route resolution

Value routes are the payload that matters downstream, so they are resolved
rather than merely copied. MusicIndex API is authoritative; the embedded
`TXXX:MusicIndex Value Routes` frame is the fallback when lookup fails.

Order of attempts, mirroring `v4vmm/src/api.rs:533` (`fetch_value_routes`):

1. `GET /v1/tracks/{track_guid}?include=payment_routes`, using
   `TXXX:MusicIndex Track Guid`.
2. `GET /v1/feeds/{feed_guid}?include=payment_routes`, using
   `TXXX:MusicIndex Feed Guid`, when no track guid is present or the track
   lookup 404s.
3. The embedded frame, verbatim, on any failure — network error, timeout,
   non-2xx, or unparseable body.

Base URL comes from `musicindex_endpoint` in `~/.config/v4vmm/config.toml`,
defaulting to `https://api.musicindex.org` (`v4vmm/src/api.rs:7`).

**The lookup must never sit on the poll path.** It runs on a worker thread. The
metadata file is written immediately using the embedded routes; if resolution
later succeeds for the same `hist_id`, the file is rewritten once with the API
result. Consequence: at most two writes per track, and the `OutputFile` dedupe
suppresses the second when the two agree. Resolved routes are cached by track
guid so repeat plays in a long set do not re-hit the API.

The output records which source won, so a downstream consumer can tell
authoritative data from a stale embedded copy.

### Metadata file format

Plain text, not JSON. MusicIndex block first, because that is the point of the
file. Binary frames are summarised, never inlined. Transcripts are dropped.

Value routes are emitted as JSON — they are consumed programmatically later, so
they stay machine-parseable rather than being flattened into a display summary.

```text
Artist - Title

[MusicIndex]
Feed Guid     = 1c7a...
Track Guid    = 9f3e...
Publisher     = Some Publisher
Contributors  = Alice: vocals, guitar
Value Routes  = musicindex-api
[{"recipient_name":"Alice","route_type":"node","address":"03ab...","split":90,"fee":false},
 {"recipient_name":"Bob","route_type":"node","address":"02cd...","split":10,"fee":false}]

[Tags]
TIT2 = Test Title
TPE1 = Test Artist
APIC = image/jpeg, 42.1 KB
```

The token after `Value Routes =` is the provenance marker: `musicindex-api` or
`embedded-id3`. The JSON array follows on subsequent lines and matches the
`PaymentRoute` shape at `v4vmm/src/api.rs:238`.

Keys are normalised before matching — uppercase, then collapse whitespace — so
`MUSICINDEX FEED GUID` from FLAC and `MusicIndex Feed Guid` from MP3 both land on
`Feed Guid`. The vocabulary is a const table of roughly 20 entries, copied from
`v4vmm/src/metadata.rs:3395` (`id3_frame_hint`) rather than shared, to keep this
utility standalone. A `--format json` flag stays available for machine consumers.

## Phases

### Phase 1 — parity and lifecycle

Source, classifier, sink. The metadata file appears when a V4V track enters
history and is removed when the next non-V4V row lands. Value routes come from
the embedded frame only. Transcripts are excluded. No expiry yet.

Both output files are cleared at startup, so a stale file from a crashed run is
never read as truth. A `Drop` guard plus a `signal-hook` handler removes the
metadata file on exit; the Python version has no exit path at all, so a stale
file survives every Mixxx shutdown.

Delivers correct behaviour for an uninterrupted set. The file lingers after the
final track.

### Phase 2 — expiry timer

Add `Expiry` and the four flags above. Closes the lingering case, which is the
last correctness gap for the intended use.

`lofty` already returns the duration when the tags are read, so this costs no new
dependency and no second file read.

### Phase 3 — MusicIndex value-route resolution

Add the worker thread, the two-step API lookup, the guid cache, and the
provenance marker. Phase 1's embedded-frame path becomes the fallback rather
than the only source.

Last because every earlier phase is fully useful without network access, and
because this is the only phase that can fail due to something outside the
machine.

## Risks

| Risk | Mitigation |
|------|------------|
| Long pause outruns the timer, so the metadata file disappears mid-track and does not return until the next track | Accepted. `--expiry none` disables the timer entirely for setups where this matters more than the lingering case |
| Track has no readable duration, pinning the file indefinitely | `--expiry-fallback`, default 600s |
| Crossfade means the next history row lands before the current track ends | Timer only shortens presence; the track-change edge remains authoritative for the start, and supersession is the normal path during a set |
| MusicIndex API is slow or down, delaying the metadata file | Lookup runs off the poll path; file is written immediately from the embedded frame and rewritten only if resolution succeeds |
| API rewrite causes a visible second write per track | `OutputFile` dedupe suppresses it when results agree; guid cache prevents repeats within a set |
| Mixxx holds a write lock during the set-log insert | Read-only connection plus `SQLITE_BUSY` retry instead of exit |
| `v4vmm` changes its MusicIndex tag names | Const table is local; unknown keys still pass through to the `[Tags]` section |
| Overlay reads the file mid-write | Temp file in the same directory, then `rename` |
| Mixxx killed rather than stopped, so no signal fires | Systemd `ExecStopPost` removes the file as a backstop |
| Hyphen stripping mangles real artist names | Keep behind `--strip-hyphens`, default on for parity; document the effect |

## Test Strategy

The diagnostic harness built during this analysis becomes the test fixture:

- `tempfile` builds a synthetic Mixxx 2.5.6 database with `track_locations`,
  `library`, `Playlists` (`hidden = 2`) and `PlaylistTracks`.
- Small tagged MP3 and FLAC fixtures are checked into `tests/fixtures/`.
- Track cases: V4V MP3, V4V FLAC, non-V4V track, missing file, untagged file,
  history table empty.
- Lifecycle cases assert presence, not content: after a non-V4V track the
  metadata file must not exist; after the deadline passes it must not exist.
- Expiry cases inject the clock rather than sleeping, so deadline arithmetic,
  `--expiry none`, slack, and the no-duration fallback are all testable in
  milliseconds.
- Value-route cases: API returns routes, API 404s and falls back to feed guid,
  API unreachable and falls back to the embedded frame, no embedded frame and no
  API. Each asserts the provenance marker.
- `--once` mode drives every case without a running Mixxx.

## Rollback Strategy

`scripts/mixxx-now-playing.sh` stays in the repository and keeps working. Both are
plain processes writing to the same runtime directory, so rollback is stopping
the Rust binary and starting the shell script. Running the Rust binary with a different
`--txt-file` allows a side-by-side comparison against the shell script over a
full show before cutover.

Phases 2 and 3 each roll back at runtime without a rebuild: `--expiry none`
restores Phase 1 lifetime behaviour, and `--no-api` forces the embedded-frame
path.

Deploy as a systemd `--user` unit with `Restart=always` and:

```ini
ExecStopPost=/bin/rm -f %t/musicindex-live-publisher/mixxx/nowplaying/metadata.txt
```

as a backstop behind the `Drop` guard.

## Decisions

- **Pause detection is out of scope; an expiry timer approximates track
  lifetime instead.** Reading the currently playing track is simple, and every
  mechanism for observing true play state cost a dependency or a heuristic for
  a case that matters only at the end of a set.
- **Value routes render as JSON.** The data is consumed programmatically later,
  so it stays machine-parseable instead of being flattened into a display
  summary.
- **Transcripts are excluded.** Track identity is established through the
  MusicIndex API using the embedded guids, with the embedded value-route frame
  as the fallback when lookup fails. Transcript text serves neither path.
- **Output lives in the per-session runtime directory.** The default path is
  `$XDG_RUNTIME_DIR/musicindex-live-publisher/mixxx/nowplaying`, with
  `~/.cache/musicindex-live-publisher/mixxx/nowplaying` as the non-systemd
  fallback.
