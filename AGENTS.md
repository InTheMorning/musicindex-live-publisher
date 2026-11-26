# musicindex-live-publisher Agent Guidelines

This repository holds two binaries. `musicindex-live-publisher` is a headless
service that reads now-playing drop files and sends live value payloads to a
MusicIndex relay. `mixxx-now-playing` is a producer that reports the track that
Mixxx plays.

Read `docs/architecture/broadcast-chain-boundaries.md` before a change that
crosses a repository boundary.

## Current State

`docs/README.md` lists each ADR with its status, each plan and each task
packet.

- No one runs the publisher or the relay in production. A change needs no
  transition period for an older version.
- The publisher, the producer, the dead block and the relay keepalive operate
  and have tests (ADR 0029).
- The producer uses the Mixxx MIDI connector when the V4V card and the mapping
  are present (ADR 0006). The manual checks with Mixxx 2.5.6 passed. Without
  the connector, the producer uses the history-only mode of ADR 0029.
- The operator deferred the packaging pass. `docs/plans/packaging-pass.md`
  lists the open gates. ADR 0006 becomes `Implemented` after that pass.
  - `packaging/arch/PKGBUILD` builds the package
    `musicindex-live-publisher-git` from the GitHub repository, not from the
    working tree. It provides `musicindex-live-publisher`, the name in the
    HitIt optional dependencies.
  - An agent built the package with `makepkg` from a local commit. The
    package holds each path of the plan. No one installed it. `namcap` did not
    run.
- ADR 0017, the show log, has the status `Implemented`. The review is
  `docs/reviews/show-log-review-checklist.md`, with no open item.
  - With `--state-dir`, the publisher writes the log. The template unit and
    the unit of `scripts/setup-mixxx-musicindex.sh` pass it.
  - The log is in `~/.local/state/musicindex-live-publisher/<instance>/showlog/`.
  - The top-level key `show_log = false` turns the log off for one instance.
    The default is `true` (revision of 2026-10-19).
  - The export of an episode from the log is a HitIt decision. No HitIt
    ADR exists for it yet. The time of each entry is the read time of this service. Each
  entry holds the published `value`. The art is a reference that HitIt
  resolves.
- `mixxx-now-playing command fade-now` starts an AutoDJ fade through the
  connector (ADR 0007).
- ADR 0008, the optional display path for artwork, has the status `Accepted`.
  Display tasks 001 to 004 are implemented. A target with `display_dir` sends
  the display state and the image to the relay through its stream delay. Its
  relay side is relay ADR 0003. The review of task 004 and the visual check
  are open.
- ADR 0009, track metadata in the HLS stream, has the status `Accepted`. It
  owns the song line that `butt` sends. The operator decided its tagger home
  (a new repository, not made yet) and its pairing (ADR 0039). Two device
  checks are open. No implementation exists.
- ADR 0010, the live value payload of the model server, has the status
  `Accepted`. It adds `line`, `author` and `podcastName` to the payload, and a
  drop file version 2 with the album and `play_id`. It adds no `link`. Its
  two packets are implemented. The producer writes version 2, and the
  publisher reads version 2 only. The app check is open.
- ADR 0011, the relay applies the stream delay, has the status `Accepted`. Its
  two packets are implemented. The publisher sends each block and display
  state at once, with `Listener-Delay-Secs`. Its review passed for each
  item except the two visual items.
- **Do not deploy this publisher before the relay of `musicindex-live-relay`
  ADR 0004 runs.** An older relay ignores the header, and podcast apps then
  get no delay.
- ADR 0039, the pairing of the display state with its value block, has the
  status `Accepted`. Its rules are implemented. The producer writes
  `display.json` with `song_line` and `play_id`. Each display body
  has `songLine`. A display state of the same play as the newest payload also
  has `value` with the `eventGuid` and the `blockGuid` of that payload.
- **Do not deploy this publisher before the relay of `musicindex-live-relay`
  ADR 0005 runs.** An older relay gives `400 invalid_display` for each
  display state with `songLine`.
- ADR 0013, the album in the display state, has the status `Accepted`. Its
  packet is implemented. The producer writes `display.json` version 3 with
  the album of the history row, and each display body has `album` when the
  track has one. The deployment passed. The app check is open.
- **Do not deploy this publisher before the relay of `musicindex-live-relay`
  ADR 0006 runs.** An older relay gives `400 invalid_display` for each
  display state with `album`.
- ADR 0014 has the status `Implemented`.
  - Only this repository assembles and publishes a block or a display state.
    HitIt and each producer do not make a block.
  - Each producer reports each track that plays, payable or not, through
    version 3 of the drop file. The producer writes version 3. For a non-V4V
    track, it writes a file with an empty `value_routes`.
  - The review is `docs/reviews/track-reporting-review-checklist.md`, with no
    open item.
- ADR 0016 has the status `Implemented`. The review is
  `docs/reviews/instances-review-checklist.md`, with no open item.
  - A producer waits at most 2 seconds for its lock, then exits with an
    error.
  - `target add` and the startup of the publisher refuse an event that a
    different instance on the same host holds.
- ADR 0015 has the status `Accepted`, and no implementation exists. In it,
  HitIt starts a talk break or a live set through `segment` commands. It
  needs a HitIt ADR before implementation.
- ADR 0018 has the status `Accepted`. The code follows it.
  `mixxx-now-playing` reads `~/.config/hitit/config.toml` first, then
  `~/.config/v4vmm/config.toml`. With no file, it writes a warning.
- ADR 0019 has the status `Accepted`. It applies musicindex ADR 0084.
  cargo-deb makes the `.deb` from `[package.metadata.deb]` in `Cargo.toml`.
  `Dockerfile` makes an image with the publisher only. A tag `v*` starts
  `.github/workflows/release.yml`, which builds the two packages. A local
  build of the `.deb` passed. Two checks stay open until the first run of
  `release.yml`: the image build and the library entries in `Depends`.
- musicindex ADR 0083 owns the version form. `build.rs` calls the crate
  `musicindex-build-info` with the prefix `MUSICINDEX_LIVE_PUBLISHER`.
  `--version` prints the line of R-VERSION-2. The publisher has no HTTP
  server and no screen, so R-VERSION-3 and R-VERSION-4 do not apply.
- ADR 0021 has the status `Accepted`. Its three packets in
  `docs/plans/mixxx-producer-template-unit.md` are implemented. The two manual
  checks of that plan are open: the operator runs
  `setup-mixxx-musicindex --migrate-units` on the computer of the private
  stream, then switches to a show instance and back.

  ADR 0021 changes ADR 0016 §Producer Units. The Mixxx producer is the
  template unit `mixxx-now-playing@<instance>.service`, and one Mixxx producer
  runs at a time. HitIt uses these units to keep a show stream apart from the
  private stream.
- ADR 0023 and ADR 0024 have the status `Accepted`. The check with a DJ
  publisher and the last review are open.
  - ADR 0023 lets a live DJ take over the event of a station. Each publisher
    sends its payable block again over a foreign dead block. The relay does
    not change.
  - ADR 0024 makes a block payable only with a feed GUID and a track GUID.
    A track GUID is unique only in its feed.
- The binary `station-import` (crate `station-import/`, task 001 of the
  station import) is complete. It makes a folder of song and episode files
  with their GUID and chapter tags and `manifest.json`, for the AzuraCast SFTP
  account. musicindex ADR 0092 owns its rules, and musicindex-fetch of
  musicindex-lib holds its feed read and download. It converts a WAV song to
  FLAC through musicindex-convert of musicindex-lib (musicindex ADR 0094
  R-CONVERT-7). flac is optional but recommended on the computer that runs
  it. With `--format mp3`, it writes only MP3 files: the MP3 file of the
  feed, else a conversion with ffmpeg through musicindex-convert (musicindex
  ADR 0095, task 002). It writes the image URL of the feed in
  `TXXX:Podcast Image` of each file, and a file that has that frame keeps it
  (musicindex ADR 0096 R-IMAGE-3). Open Point 1 of that ADR (keep the old
  frame or use the feed value) has no operator answer. The code keeps the old
  frame. It has no upload code. The feed value replaces each standard frame
  of the map of ADR 0030, for example `TIT2`, `TPE1` and `TALB`. A frame with
  no feed value keeps the value of the file.
- This repository owns the testing and the operation of the HeyCitizen test
  station (ADR 0032). The station log is `docs/runbooks/test-station-log.md`.
  The Mixxx VPS `bastone.heycitizen.xyz` is a different host. It runs the
  stack of `musicindex-stream-test` with pure Liquidsoap. That folder has no
  Git repository, so `docs/runbooks/mixxx-vps-log.md` records each change
  that a decision needs on that VPS, and each change that is done.
  The publisher session installs its bundles there and does the agent checks.
  A fault in another repository goes to the orchestrator as a handoff with the
  step number of the log. The SSH key is in `.local-keys/`. Never commit it.
- Open: the producer tutorial and the AzuraCast runbook have some of the same
  steps. A merge of the two is not done (ADR 0032).
- Deferred by the operator on 2026-11-25: an animated GIF that is embedded in
  an audio file. ADR 0008 refuses it, and the relay accepts only JPEG and PNG.
  A GIF that a feed gives as a URL goes to the apps as that URL.
- Open: at a jingle, Rainbrook does not show the station lines (station log
  step 36). The relay output is correct. The orchestrator has the handoff for
  the Rainbrook session.
- ADR 0034 has the status `Accepted`. `src/relay.rs` implements it: when the
  drop file has no image URL, the first send of a play waits at most 2 seconds
  for the display URL or the relay URL of the uploaded picture. If the picture
  is not available, the payload worker sends the payload one more time when
  the picture is available. The tests are at the end of `tests/display.rs`.
  On the test station, v4vmusic.com changes the picture at each replay after a
  jingle, with no page reload (station log step 46).
  Open, from the review of 2026-11-25, in the code from before the wait: a
  publish retry reads the channel and not the queue of the second send check,
  so an older queued payload can go after a newer one. A permanent refusal
  of a newer payload leaves the older play open to a second send. Relay ADR 0011 keeps each
  image that a pending listener update names, and the relay runs it. Item 2 of
  its §Before Acceptance is partly done: v4vmusic.com and Podcast Guru show the
  image, and a boost pays the song. The check for no new track is open.
- ADR 0041 has the status `Accepted`. It is the one record for the station
  producer `liquidsoap-now-playing`. It replaces ADR 0035 and ADR 0040, which
  are in `docs/archive/adr/`. The producer reads `state.json` from the slot
  text, pays each chapter song of an episode file, writes `display.json` with
  `song_line` as the ICY title of the file, asks the route service with the
  two GUIDs, and uses the chapter `image_url`, then the image of the file,
  then the relay copy (ADR 0034). A chapter with a title has the episode
  title as its `album`, in the drop file, in `display.json` and in the live
  value payload. That album shows in Rainbrook on the test station (station
  log step 56). The iOS lock screen shows no album line (station log step
  57). The producer code is complete.
  The station checks of the display state need a person and are open.
  `station-import` uses musicindex-lib `cd21d45`, so each chapter entry of an
  imported episode has the `image_url` of its song (musicindex ADR 0099).
- ADR 0036 has the status `Accepted`, and no implementation exists. The HLS
  stream carries the picture one time for each song. Liquidsoap removes cover
  keys from the HLS metadata by default, so the script must change
  `settings.encoder.metadata.cover`. The first test is on the VPS stack with a
  fixed picture (§Before Acceptance item 1).
- ADR 0037 has the status `Accepted`. A target can give `icecast_burst_bytes`
  and `stream_bitrate_kbps` in place of `stream_delay_secs`. The publisher
  calculates the delay, rounds it down, and sends it as `Listener-Delay-Secs`.
  Listener delay task 001 implements it. The test station has the two fields
  (station log step 48). In v4vmusic.com a chapter change comes at almost the
  correct time. The boost check of ADR 0037 is open.
- ADR 0038 has the status `Accepted`. The station slot text writes a frame
  `musicindex.hls/2` with the `playId` of the segment at each chapter boundary,
  and Rainbrook releases the display state with the same `playId`. Cause:
  Rainbrook on HLS shows a chapter change too early (station log step 50).
  Relay ADR 0012 runs on `api.musicindex.org`, and Rainbrook builds
  Rainbrook ADR 0030. Segment frame tasks 001 (`playId`) and 002 (the slot
  text) run on the test station, and the station check of task 002 passes
  (station log step 52). In Rainbrook on HLS the chapter change comes with
  the chapter song (station log step 53). The boost at the start of a chapter
  song is open.
- ADR 0042 has the status `Accepted`. An episode state sends its
  publication day in the display state. Relay ADR 0013 gives the name and the
  format of `pubDate`, and Rainbrook ADR 0031 (Proposed) shows the date. The
  station check of the Liquidsoap key `date` is open and comes before the
  deploy. Display keys tasks 001 and 002 implement it and are not deployed.
- ADR 0043 has the status `Accepted`. The display track gets `funding`, a
  list of 1 to 3 funding links of the segment. The station producer gets them
  from the route service with `include=source_links`. Relay ADR 0013 has the
  row for `funding`, and Rainbrook ADR 0031 (Proposed) shows the links.
  The route service gives the routes and the links in one answer (station
  log step 58). The publisher sends `funding` (display keys task 001), and
  the station producer writes `funding` `null`. Display keys task 003 (the
  lookup) is stopped. It gets a new text for ADR 0044 after the acceptance.
- The deploy order of ADR 0042 and ADR 0043 is the relay, then the publisher,
  then Rainbrook.
- ADR 0044 has the status `Proposed`. The funding of a segment uses the
  order of its routes: the route service answer first, then the file `WPAY`,
  then `null`. The Mixxx producer also writes `funding`. Do not start code
  for it before the operator accepts ADR 0044 and musicindex ADR 0101.
- Relay commit `1dec0b3` accepts `pubDate` and `funding` (relay ADR 0013).
  The publisher sends the two keys only after
  `https://api.musicindex.org/v1/liveitems/info` gives `git_revision`
  `1dec0b3` or a subsequent revision.
- This item needs a new ADR before work starts: a track identity source that
  is earlier than the Mixxx history row.
- Studio client task 001 is not started. The publisher will start and end a
  show in its Studio feed through the crate `studio-app-client`. The task waits
  for Studio task 073. Its packet a writes ADR 0020 first. Studio ADR 0110 owns
  the interface.

## Build / Lint / Test Commands

```bash
cargo build --release              # Production build
cargo build                        # Debug build
cargo test --workspace             # All tests in both crates
cargo test --test golden           # One integration test file
cargo fmt --all -- --check         # Check formatting
cargo fmt --all                    # Auto-format
cargo clippy --workspace -- -D warnings
node --test 'mixxx/tests/*.test.js' # Tests for the Mixxx mapping script
```

`.github/workflows/ci.yml` runs `cargo fmt`, `cargo clippy`,
`cargo test --workspace` and `node --test` on each push and each pull
request. Change a command here and in the workflow in the same commit.

`tests/adr_guard.rs` runs the ADR link guard of the project-baseline skill
(§A Binding Rule Lives In A Decision Record). The test gives
`--bar-from 0019` to the guard. The guard requires a `- Bar:` line in each
current ADR from number 0019. On CI the runner has no `~/.agents`, and the test passes without the guard.

The workspace holds the publisher at the root and `mixxx-now-playing` as a
member. Always pass `--workspace` so a change in one crate cannot break the
other without a failure.

## Conventions

- Rust edition 2024. `cargo fmt` defaults. No `rustfmt.toml`.
- Types and enums use PascalCase. Functions, variables, and modules use
  snake_case. Constants use SCREAMING_SNAKE_CASE.
- Group imports by std, external, then `crate::`.
- Return `anyhow::Result` with `.context(...)` for a failure that reaches an
  operator.
- Use `tracing` for logging, not `println!`. Include fields:
  `tracing::warn!(target = %name, "publish failed")`.
- Module-level doc comments with `//!`. Document every public type and every
  public function that can fail, with an `# Errors` section.

## Foundational Mandates

### 1. Contract Ownership

- **This repository owns the drop-file contract.** A field change needs a new
  schema version. Unknown versions are ignored, never guessed.
- **This repository owns its configuration file and its systemd units.** No
  other program writes them. A control surface calls the commands here.
- The relay owns the wire format for a published payload. Match it. Do not
  invent a field.

### 2. ADR-First

- Do not implement an architectural change before an ADR in `docs/adr/`
  records it.
- A status is one of `Proposed`, `Accepted`, `Implemented`, or
  `Superseded by ADR NNNN`, each with a date.
- Amend an ADR in place only when the amendment does not reverse a decision,
  and add a dated sentence that says what changed.

### 3. Documentation Discipline

- Documentation lives under `docs/`, in `adr/`, `plans/`, `tasks/`, `reviews/`,
  `runbooks/`, and `architecture/`. Keep the repository root small.
- Write documentation prose in ASD-STE100 Simplified Technical English. Short
  sentences, active voice, no semicolons, and one instruction for each
  sentence.
- Update `docs/README.md` when a document is added.

### 4. Payment Safety

This service routes money. These rules are not style preferences.

- **A stale destination sends a listener boost to the wrong artist.** When no
  payable block plays, publish the dead block (ADR 0029). Never leave the last
  track in place.
- When the MIDI connector is available, a drop file exists only while its
  linked deck plays (ADR 0006).
- The dead block pays nobody. It has one destination at
  `no-v4v-track@example.invalid`, and a payment to it fails (ADR 0033). Never
  send it with no destination: the v4vmusic.com client rejects that block and
  keeps the last song payable. No setting changes its model or its destination.
  Only its title and its image are settings of each target,
  `dead_block_title` and `dead_block_image` (ADR 0029).
- The `title` of a payable track block is "artist - title" (ADR 0029). The
  display state keeps the song as its `title`.
- A `split` is a decimal string in the payload, never a number. `90.0` renders
  `"90"` and `0.49` renders `"0.49"`.
- Each track gets a fresh `blockGuid`. A change between the dead block and a
  track block also gets a fresh `blockGuid`, because a client skips a block
  with the GUID of the last block (ADR 0029).
- Never derive a split from a title, a file name, or a fuzzy match. Use the
  supplied metadata only.
- A block pays its routes only when its drop file has a feed GUID and a
  track GUID (ADR 0024). A track GUID is unique only in its feed.
- The assembled payload must never hold exactly the keys `event_id` and
  `metadata`. That key set makes the relay read it as the wrapped form, and
  listener apps then receive no splits. A unit test asserts this.

### 5. Secret Handling

- A broadcaster token is returned by the relay one time and cannot be
  recovered. Treat a token file as irreplaceable.
- Token files use mode `0600`. The parent directory uses mode `0700`.
- Never place a token in a log line, in an error message, in a command
  argument, or in a `Debug` output. `PublisherTarget` and `RelayTarget` have
  manual `Debug` implementations for this reason. Keep them.
- Only `provision --json` prints a token, because the caller must store it.

### 6. Output Safety

- Write every output file to a temporary file in the same directory, then
  rename. A reader must never see a partial file.
- Do not rewrite a file when the content did not change.
- The drop directory is one instance for each producer. Two producers must not
  share one directory.

### 7. Failure Behavior

- A wrong or revoked token is fatal and repeats on every restart. Exit rather
  than fail in silence. The unit sets `StartLimitBurst=5`, so systemd stops the
  restart loop and marks the unit `failed`.
- A retryable relay failure uses backoff. A permanent failure does not retry.
- The Mixxx producer exits cleanly when Mixxx is not running. The unit
  restarts it.
- The Liquidsoap producer stays in operation when Liquidsoap stops. It
  removes the drop file and releases the lock (ADR 0041).

### 8. Test Strategy

- Golden-file tests compare an assembled payload against the real example
  payloads in `tests/fixtures/`.
- Watcher tests use `tempfile`. Do not depend on a real inotify race.
- Relay tests use a stubbed status code or a local relay built from
  `~/build/musicindex-live-relay`. Do not call a public relay in a test.
- Every payment rule in section 4 has a test.

## Neighbors

| Repository | Relation |
|---|---|
| HitIt | Writes the MusicIndex tags this chain reads. Registers events. Starts and stops these services. Sends no payloads. |
| `musicindex-live-relay` | The relay. Receives the payloads this service sends. |
| musicindex-research | The orchestrator session. It gets each handoff for Rainbrook, the relay and the other sessions (ADR 0032). |
| HeyCitizen test station | `radio.heycitizen.xyz`, AzuraCast. This repository operates it (ADR 0032). A new station repository comes only with an alternative to AzuraCast. |

Do not add a build dependency on either one. Every contract is a file format,
an HTTP call, or a command-line interface.

HitIt ADR 0096 renamed the app v4vmm to HitIt. In archived records, finished
tasks, reviews, dated captures and accepted ADRs, the name v4vmm means HitIt.
The checkout is `~/build/HitIt/` and the repository is `InTheMorning/HitIt`.
In those records, the path `~/build/v4vmm/` means `~/build/HitIt/`.
