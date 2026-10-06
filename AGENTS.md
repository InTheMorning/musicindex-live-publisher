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
  and have tests (ADR 0005).
- The producer uses the Mixxx MIDI connector when the V4V card and the mapping
  are present (ADR 0006). The manual checks with Mixxx 2.5.6 passed. Without
  the connector, the producer uses the history-only mode of ADR 0005.
- The operator deferred the packaging pass. The package files exist, but no
  one built or installed a package that holds them. `docs/plans/packaging-pass.md` lists
  the open gates. ADR 0006 becomes `Implemented` after that pass.
- ADR 0003, the show log contract, has the status `Accepted`. Its
  implementation has not started.
- `mixxx-now-playing command fade-now` starts an AutoDJ fade through the
  connector (ADR 0007).
- ADR 0008, the optional display path for artwork, has the status `Accepted`.
  Display tasks 001 to 004 are implemented. A target with `display_dir` sends
  the display state and the image to the relay through its stream delay. Its
  relay side is relay ADR 0003. The review of task 004 and the visual check
  are open.
- ADR 0009, track metadata in the HLS stream, has the status `Proposed`. It
  owns the song line that `butt` sends. The operator decided its tagger home
  (a new repository, not made yet) and its pairing (ADR 0012). Two device
  checks are open. No implementation exists.
- ADR 0010, the live value payload of the model server, has the status
  `Proposed`. It adds `line`, `author` and `podcastName` to the payload, and a
  drop file version 2 with the album and `play_id`. It adds no `link`. Two
  items in its §Before Acceptance are open. No implementation exists.
- ADR 0011, the relay applies the stream delay, has the status `Accepted`. Its
  two packets are ready. They ship after the relay of ADR 0004 is deployed.
  Until then, the publisher holds each block and display state for the stream
  delay (ADR 0005, ADR 0008).
- ADR 0012, the pairing of the display state with its value block, has the
  status `Proposed`. It needs relay ADR 0005, ADR 0010 and ADR 0011. No
  implementation exists.
- These items need a new ADR before work starts:
  - the talk break block from `v4vmm`,
  - a track identity source that is earlier than the Mixxx history row.

## Build / Lint / Test Commands

```bash
cargo build --release              # Production build
cargo build                        # Debug build
cargo test --workspace             # All tests in both crates
cargo test --test golden           # One integration test file
cargo fmt --all -- --check         # Check formatting
cargo fmt --all                    # Auto-format
cargo clippy --workspace -- -D warnings
node --test mixxx/tests/           # Tests for the Mixxx mapping script
```

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
  payable block plays, publish the dead block (ADR 0005). Never leave the last
  track in place.
- When the MIDI connector is available, a drop file exists only while its
  linked deck plays (ADR 0006).
- The dead block is a constant. No configuration changes it.
- A `split` is a decimal string in the payload, never a number. `90.0` renders
  `"90"` and `0.49` renders `"0.49"`.
- Each track gets a fresh `blockGuid`.
- Never derive a split from a title, a file name, or a fuzzy match. Use the
  supplied metadata only.
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
- The producer exits cleanly when the player is not running. The unit restarts
  it.

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
| `v4vmm` | Writes the MusicIndex tags this chain reads. Registers events. Starts and stops these services. Sends no payloads. |
| `musicindex-live-relay` | The relay. Receives the payloads this service sends. |

Do not add a build dependency on either one. Every contract is a file format,
an HTTP call, or a command-line interface.
