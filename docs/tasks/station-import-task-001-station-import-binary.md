# Station Import Task 001: The Binary `station-import`

Status: Done - 2026-11-23, with musicindex-fetch at `7b36f6a`. The crate now uses musicindex-lib `5db5c80` and its `value_routes(blocks, routes)`. musicindex ADR 0092 is accepted (musicindex `35d3305`). The crate `musicindex-fetch` is in
musicindex-lib `7b36f6a`.

All criteria are mechanical, except the one item in §Visual And Manual Criteria. No test reaches the network.

## Goal

A new workspace crate `station-import/` gives the binary `station-import`. It reads one `musicL` feed of songs and the
selected episodes of one podcast feed. It writes one output folder with `songs/`, `episodes/` and `manifest.json`
(musicindex ADR 0092 R-STATION-3 to R-STATION-5). The station bundle gets the binary, and the AzuraCast runbook gets a
short section.

## Base components

| Component | Status | Location | Reason |
| --- | --- | --- | --- |
| Feed read, host limits, remote item resolve | reuse | ~/build/musicindex-lib/fetch/src/reader.rs | `FeedReader`, `ReaderConfig`, `FeedDocument::parse` and `FeedReader::resolve` of musicindex-fetch at `7b36f6a`. musicindex ADR 0092 R-STATION-1 forbids a second copy. |
| Song list of a `musicL` feed | reuse | ~/build/musicindex-lib/fetch/src/resolve.rs | `song_list` and `RemoteItemRef`. |
| Episode splits | reuse | ~/build/musicindex-lib/fetch/src/splits.rs | `FeedReader::episode_splits` gives `EpisodeSplits { splits, unresolved }`. |
| Value block of an item | reuse | ~/build/musicindex-lib/fetch/src/routes.rs | `value_routes(&[IngestPaymentRoute])` for the episode routes. |
| Enclosure download | reuse | ~/build/musicindex-lib/fetch/src/download.rs | `download_enclosure(url, path, user_agent)` gives `Download { sha256, bytes }` and leaves no file after a failure. |
| Tag write and read | reuse | ~/build/musicindex-lib/tags/src/podcast.rs | `write_song_tags`, `write_episode_tags`, `read_podcast_tags`, `NoReferences`. |
| Station bundle script | extend | scripts/make-station-bundle.sh | It also builds and copies `station-import`. |
| Station runbook | extend | docs/runbooks/liquidsoap-now-playing-azuracast.md | One new section for the import. |
| File hash after the tag write | new | station-import/src/lib.rs | Searched ~/build/musicindex-lib/fetch/src/download.rs (the hash is private to the download), src/display.rs and now-playing-drop/src/display.rs (they hash bytes in memory). `finish_file` streams the file into `sha2::Sha256`. |
| Test HTTP server | new | station-import/tests/ | Searched now-playing-drop/src (the `test-server` feature serves the relay API only), liquidsoap-now-playing/tests and ~/build/musicindex-lib/fetch/tests. The fetch tests have a server that is private to that crate. A small `std::net::TcpListener` server in the test file is sufficient. |

## Files To Inspect

- `~/build/musicindex/docs/adr/0092-the-publisher-imports-songs-and-episodes-for-the-station.md`, all of §Decision
  and §Checks. It owns each rule of this task.
- `~/build/musicindex/docs/adr/0093-the-feed-parser-and-the-tag-crate-of-the-suite.md`, R-TAG-3, R-TAG-4 and R-TAG-6.
- `~/build/musicindex-lib/fetch/src/*.rs` and `~/build/musicindex-lib/fetch/tests/*.rs` (at `7b36f6a`).
- `~/build/musicindex-lib/tags/src/podcast.rs`, `~/build/musicindex-lib/tags/src/chapters.rs`.
- `liquidsoap-now-playing/Cargo.toml` and `liquidsoap-now-playing/src/main.rs`, for the form of a workspace binary
  and of its argument parser.
- `scripts/make-station-bundle.sh`.
- `docs/runbooks/liquidsoap-now-playing-azuracast.md`.

## Files To Change

- New: `station-import/Cargo.toml`, `station-import/src/main.rs`, `station-import/src/lib.rs` and other modules in
  `station-import/src/`, `station-import/tests/*.rs`, `station-import/tests/fixtures/*`.
- `Cargo.toml` (only the lists `members` and `default-members` of `[workspace]`) and `Cargo.lock`.
- `scripts/make-station-bundle.sh`.
- `docs/runbooks/liquidsoap-now-playing-azuracast.md`: one new section §Import Songs And Episodes, before
  §Rollback. Change no other section.

## Do Not Touch

- `src/**`, `now-playing-drop/**`, `mixxx-now-playing/**`, `liquidsoap-now-playing/**`, `liquidsoap/**`,
  `systemd/**`, `packaging/**`, `docs/adr/**`, `AGENTS.md`, `docs/README.md`.
- `~/build/musicindex-lib`. Do not copy code from it. If an API that this task needs is missing, stop and report it.
- Do not commit. The main session reads the diff, runs the gate and commits.

## Constraints

### The crate

- `station-import/Cargo.toml`: `edition = "2024"`, `license = "AGPL-3.0-only"`, the binary name `station-import`.
- Dependencies:
  - `musicindex-fetch = { git = "https://github.com/InTheMorning/musicindex-lib", rev = "7b36f6a..." }` with the full
    40-character hash of `7b36f6a` (`git -C ~/build/musicindex-lib rev-parse 7b36f6a`).
  - `musicindex-tags` from the same `git` URL and the same `rev`, so `Split` is one type.
  - Also `anyhow`, `serde` with `derive`, `serde_json`. Use the stophammer parser types through
    `musicindex_fetch::stophammer_parser`. Add no direct dependency on `stophammer-parser`.
- Dev-dependencies: `tempfile`.
- No upload code, no AzuraCast API, no API key, no SFTP (R-STATION-5 item 2).

### The command line

```text
station-import --out DIR [--songs URL_OR_PATH] [--episode-feed URL --episode SELECTOR ...]
               [--dry-run] [--musicindex-url URL]
```

- `--out DIR`: required. The command makes the folder and its subfolders when they are missing.
- `--songs URL_OR_PATH`: one `musicL` feed. A value that starts with `http://` or `https://` is a URL, read with
  `FeedReader::read`. Another value is a file path, read with `std::fs::read` and `FeedDocument::parse`.
- `--episode-feed URL`: one podcast feed URL. `--episode SELECTOR` can occur more than one time, and needs
  `--episode-feed`. A selector of only ASCII digits is an episode number. It matches `IngestTrackData::track_number`,
  which the parser fills from `itunes:episode`, else `podcast:episode`. Another selector is an item GUID. It matches
  the trimmed `track_guid` of an item.
- At least one of `--songs` and `--episode-feed` is necessary. `--episode-feed` without `--episode` is an error.
- `--musicindex-url`: the default is `musicindex_fetch::DEFAULT_MUSICINDEX_BASE_URL`.
- The `User-Agent` is `station-import/` and the crate version.
- A usage error exits with status 2 and prints the usage line. Use a hand-written parser as in
  `liquidsoap-now-playing`. Add no argument crate.

### The library function

- `lib.rs` has a function that the tests call without a process, for example
  `pub fn run(options: &Options, config: ReaderConfig) -> Result<Manifest>`. The tests give a `ReaderConfig` with a
  `min_interval` of zero and the URL of the test server.
- `main.rs` parses the arguments, calls the function with `ReaderConfig { min_interval: RSS_HOST_INTERVAL, .. }`, and
  writes the manifest. It exits with status 1 when an item has an error (R-STATION-4 item 3), else 0.

### The song items (R-STATION-3 item 1, R-STATION-4)

For each `RemoteItemRef` of `song_list(&document.values)`, in feed order:

1. `reader.resolve(&item)`. An error gives a manifest entry with the error, the feed GUID and the item GUID of the
   reference (the item GUID can be `null`).
2. No enclosure URL gives an entry with an error.
3. The path is `songs/ITEM_GUID.EXT`. §File Names gives `EXT`.
4. A dry run stops here. The entry has the path and no hash.
5. `download_enclosure(url, &path, user_agent)`. An error gives an entry with the error.
6. `write_song_tags(&path, &SongTags { feed_guid, item_guid }, &[], &NoReferences)`. An error removes the file and
   gives an entry with the error. A file with no GUIDs pays nobody on the station.

### The episode items (R-STATION-3 item 3, R-STATION-4)

Read the feed once with `reader.read(url)`. A read error gives one error entry for each selector. For each selector,
in the order of the arguments:

1. Find the item. No match gives an error entry that names the selector.
2. The feed GUID is the trimmed `values.feed_guid`. A missing feed GUID gives an error entry.
3. `reader.episode_splits(track)`. A `BadSplit` gives an error entry.
4. The episode routes are `musicindex_fetch::value_routes(&track.payment_routes)`, else
   `value_routes(&values.feed_payment_routes)`, as a JSON string, or `None`.
5. The path is `episodes/ITEM_GUID.EXT`. A dry run stops here.
6. Download as for a song. Then `write_episode_tags(&path, &EpisodeTags { feed_guid, item_guid, value_routes, splits },
   &[], &NoReferences)`. An error removes the file and gives an error entry.

An unresolved split (`EpisodeSplits::unresolved`) is not an item error. The split stays in the tags with no title and
no routes (musicindex ADR 0093 R-TAG-6 item 2). The manifest lists each one, so the operator sees it.

### File Names

- `EXT` comes from `enclosure_type`: `audio/mpeg` and `audio/mp3` give `mp3`, `audio/flac` and `audio/x-flac` give
  `flac`, `audio/ogg` gives `ogg`, `audio/opus` gives `opus`, `audio/mp4`, `audio/x-m4a` and `audio/aac` give `m4a`,
  `audio/wav` and `audio/x-wav` give `wav`.
- Else the extension of the last path segment of the enclosure URL, when it is one of these six, in lowercase.
- Else the entry has an error.
- `ITEM_GUID` is used only when it holds only ASCII letters, digits, `-`, `_` and `.`, and does not start with `.` or
  with `sha256-`. Else the file name is `sha256-` and the SHA-256 hex of the GUID. This stops a path outside the
  output folder. The tags and the manifest keep the GUID. An empty GUID gives an error.
- Two items with the same kind and item GUID: the second gets an error entry and no download.

### The manifest (R-STATION-4 items 3 and 4)

`manifest.json` in the output folder, pretty JSON, written last, through a temporary file in the same folder and a
rename. The form:

```json
{
  "format": "station-import-manifest/1",
  "dry_run": false,
  "items": [
    {
      "kind": "song",
      "feed_guid": "...",
      "item_guid": "...",
      "title": "...",
      "feed_url": "...",
      "source_url": "...",
      "path": "songs/....mp3",
      "sha256": "...",
      "bytes": 12345,
      "tags": { "applied": 2, "removed_old_frames": [], "removed_frames": [], "skipped": [], "bad_frames": 0, "removed_id3v1": false },
      "chapters": null,
      "unresolved_splits": [],
      "error": null
    }
  ]
}
```

- `kind` is `song` or `episode`. `selector` is also present for an episode entry, with the argument text.
- `source_url` is the enclosure URL. `path` is relative to the output folder.
- In a dry run, `sha256`, `bytes` and `tags` are `null` (R-STATION-4 item 4).
- `sha256` and `bytes` are the values of the file at `path` after the tag write, not the values of the download.
  Each audio file has the mode 0644.
- `chapters` is the number of splits of an episode, else `null`.
- Each `unresolved_splits` entry is `{ "index": N, "feed_guid": "...", "item_guid": "..." or null, "error": "..." }`.
  The feed GUID and the item GUID come from the split at that index (`SplitTarget::Remote`). The error is the
  `Display` text of the `ResolveError`.
- `error` is `null`, or the `Display` text of the error. When `error` is not `null`, `sha256`, `bytes` and `tags` are
  `null`, and no audio file stays at `path`.
- The command prints one summary line to stderr: the number of songs, of episodes, of errors and of unresolved
  splits.

### The runbook section

`docs/runbooks/liquidsoap-now-playing-azuracast.md`, new section `## Import Songs And Episodes` before `## Rollback`.
Write it in Simplified Technical English (`/home/citizen/.agents/skills/asd-ste100/SKILL.md`). Give:

- That the command runs on a computer of the operator, not on the radio host, and cite musicindex ADR 0092.
- One full dry-run command and one full command, with `--songs`, `--episode-feed` and two `--episode` values.
- That the operator reads `manifest.json` first: an item with an `error` value, and each `unresolved_splits` entry.
- That the operator sends the folders `songs/` and `episodes/` through the AzuraCast SFTP account. The command has no
  upload (R-STATION-5).

Run `python3 "$HOME/.agents/skills/asd-ste100/scripts/ste_lint.py" --check --no-heuristics` on the runbook, and correct
the findings in the new section only.

### The bundle script

`scripts/make-station-bundle.sh` builds `station-import` for `x86_64-unknown-linux-musl` with the same OpenSSL
environment, and installs it in the bundle with mode `0755`. If a joint build of the three binaries fails, build
`station-import` in a second `cargo build` call. Change no other step.

## Mechanical Criteria

Each one is a test in `station-import/tests/`. The tests use a local `std::net::TcpListener` server on `127.0.0.1`
that serves fixed paths: the song feeds, the podcast feed, a MusicIndex answer for `/v1/feeds/GUID`
(`{"data":{"feed_url":"..."}}`), a 404 for an unknown path, and the audio fixtures. Copy
`mixxx-now-playing/tests/fixtures/musicindex-tagged.mp3` and `untagged.flac` into `station-import/tests/fixtures/`.

1. `dry_run_writes_the_manifest_and_no_audio`: a local `musicL` file with two `remoteItem` elements, one with a
   `feedUrl` and one with only a `feedGuid`. After the run, `manifest.json` has two song entries with the GUIDs, the
   source URL and the planned path, `sha256` is `null`, and the folders `songs/` and `episodes/` hold no file
   (ADR 0092 §Checks, R-STATION-4 item 4).
2. `run_writes_songs_with_their_guids`: the same input without `--dry-run`. Each song file exists,
   `read_podcast_tags` gives the feed GUID and the item GUID of the entry, and `sha256` is the SHA-256 of the file
   (ADR 0092 §Checks, R-STATION-4 item 2).
3. `run_writes_an_episode_with_its_chapters`: a podcast feed with one episode, selected by GUID, with two
   `valueTimeSplit` elements. One split song resolves, and one split song feed gives 404. The episode file has the
   GUIDs, and `read_podcast_tags(..).chapters()` gives two entries. The manifest has `chapters: 2` and one
   `unresolved_splits` entry with the feed GUID of the 404 song. The run has no item error.
4. `episode_selected_by_number`: the same feed with `podcast:episode` 7 on the episode. The selector `7` selects it.
5. `missing_episode_is_an_error_entry_and_the_run_continues`: a selector that matches nothing gives an error entry,
   and the other items are still written. The function result reports one error, so `main` exits with status 1.
6. `failed_download_leaves_no_file`: a song whose enclosure gives 404. The entry has an error and no file is at its
   path.
7. `unsafe_item_guid_gets_a_hash_file_name`: the item GUID `../escape` gives the file
   `songs/sha256-HEX.mp3` and no file outside the output folder. The tags and the manifest keep `../escape`.
8. Unit tests for the argument parser: the usage errors of §The Command Line, and the two selector forms.
9. `manifest_hash_and_size_are_of_the_tagged_file`: for two songs and one episode, `sha256` and `bytes` of each
   entry are the values of the file at `path`. The hash is not the hash of the served file. The mode of each
   file is 0644.

Operator choice (orchestrator, 2026-11-23, ADR 0092 R-STATION-4 item 3): the manifest hash describes the file at
`path` after the tag write, because the operator examines the copy on the station with it. The manifest has no
second hash field. The audio files get the mode 0644, so that a copy that keeps the mode is readable in the
AzuraCast container.

10. `file_with_no_title_gets_the_item_title_artist_and_album` and `file_with_a_title_keeps_it`: a downloaded file
    with no `TIT2`, `TPE1` or `TALB` gets the item title, the artist and the channel title. A value that the file
    has stays.
11. `chapter_artist_is_the_item_author_else_the_channel_author`: a chapter song with an item `itunes:author` gets it
    as the chapter `artist`. A chapter song with no item author gets the channel `itunes:author`. `author` does not
    change. The unit tests `display_artist_*` in `src/lib.rs` also show that an artist that is not empty stays as
    it is.

Operator choice (orchestrator, 2026-11-23, title frames; ADR 0092 does not name them): station-import writes a
title, artist or album frame only when the downloaded file has no value for it. The title is the item title. The
artist is the item `itunes:author`, else the channel `itunes:author`. The album is the channel title. This applies
to songs and episodes. The edits go through `write_song_tags` and `write_episode_tags`.

Operator choice (orchestrator, 2026-11-23, chapter artist; musicindex ADR 0093 R-TAG-4 item 4 says that the caller
gives it): when a chapter song has no artist, station-import sets the chapter `artist` with the same artist rule,
in the same function `display_artist`. A chapter artist that is not empty stays. `author` stays the channel
`itunes:author`. The producer and publisher ADR 0025 do not change.

## Visual And Manual Criteria

- The manual run with the real feed. The main session does it after the gate. The task does not do it.

## Checks

Run each one in the repository root. Each must pass:

```sh
cargo build --workspace
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all -- --check
bash -n scripts/make-station-bundle.sh
```

## Report

Give the files changed, the test names, the output of the checks, and each API of musicindex-fetch or
musicindex-tags that was missing or that did not work as this file states.

Operator choice (orchestrator, 2026-11-23, after test station step 18 of musicindex-research commit 53bf07f): an item
GUID that is not safe for a file name gets the file name `sha256-` and the SHA-256 hex of the GUID, for example a URL
item GUID of a song feed. The tags and the manifest keep the GUID. The runbook says that the import cannot tag a WAV
file. The import does not convert a WAV file to FLAC.
