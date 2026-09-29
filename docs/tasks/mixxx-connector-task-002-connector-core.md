# Mixxx Connector Task 002: The Connector Core

Status: Implemented - 2026-09-28. The review corrected three defects. See
§Review Change.

Every criterion is mechanical. This packet has no visual criteria, because it
adds no user interface and opens no real device in a test.

## Goal

Add pure code to `mixxx-now-playing` for the V4V raw MIDI device. The code
gives the deck state and the mode. The poll loop does not use it yet.
Task 003 connects it.

## Files To Inspect

- `docs/adr/0006-mixxx-midi-connector.md` (§Transport, §Protocol and §When
  The Connector Is Not Available)
- `mixxx-now-playing/src/lib.rs`
- `mixxx-now-playing/src/lock.rs` (the style of a small module with tests)
- `mixxx-now-playing/Cargo.toml`

## Files Likely To Change

- `mixxx-now-playing/src/connector/mod.rs` (new)
- `mixxx-now-playing/src/connector/midi.rs` (new)
- `mixxx-now-playing/src/connector/state.rs` (new)
- `mixxx-now-playing/src/connector/device.rs` (new)
- `mixxx-now-playing/src/lib.rs`

## Do Not Touch

- `mixxx-now-playing/src/main.rs`, `cli.rs`, `config.rs`
- `src/**` (the publisher)
- `mixxx/**`, `packaging/**`, `systemd/**`, `scripts/**`
- `docs/adr/**`

## Constraints

- No new dependency. Use the standard library only.
- `midi.rs`: a parser that takes bytes in any split and gives
  `ControlChange { channel, controller, value }`.
  - It supports running status.
  - It ignores a real-time byte (`0xF8` to `0xFF`) between the bytes of a
    message, and it keeps the message.
  - It skips SysEx (`0xF0` to `0xF7`) and every other message kind.
  - It gives each control change on each channel. The state code filters the
    channel.
- `state.rs`:
  - `PROTOCOL_VERSION: u8 = 1` and `HEARTBEAT_TIMEOUT = 3 seconds`.
  - `ConnectorState` holds `play`, `track_loaded` and `duration_secs` for
    decks 1 to 4. It also holds the last loudest deck (0 to 4), the last
    heartbeat time and the last heartbeat version.
  - `apply(cc, now)` uses only channel 16. It gives a `DeckChange` only when
    a value differs from the last value. Mixxx sends `play = 0` more than one
    time at a stop.
  - A duration applies when its low part (CC 40+N) arrives. It uses the last
    high part (CC 30+N).
  - A value of 64 or more means true for `play` and `track_loaded`.
  - The loudest deck starts at 0. A value above 4 is ignored.
  - `mode(now, device_open)` gives `Connector` or `HistoryOnly(reason)`. The
    reasons are `NoDevice`, `NoHeartbeat` and `UnknownVersion`. `Connector`
    needs an open device, a heartbeat in the last 3 seconds, and version 1.
  - A method clears the deck state and the loudest deck. Task 003 calls it
    when the device closes.
  - All times come from the caller as `Instant`. No code reads the clock.
- `device.rs`:
  - `resolve_raw_device(proc_asound: &Path, dev_snd: &Path, card_id: &str)`
    reads the symbolic link `proc_asound/<card_id>`, which is `cardN`, and
    gives `dev_snd/midiCND0`. A missing link is an error with `.context(...)`
    that names the card ID.
  - `STATE_REQUEST: [u8; 3] = [0xBF, 0x01, 0x01]`.
  - `pump(reader: impl Read, sender)` reads bytes, parses them, and sends each
    control change with its arrival `Instant` into an `mpsc::Sender`. It
    returns when the reader gives end of file or an error.
  - `spawn_reader(...)` starts one thread named `connector-midi`. The thread
    resolves and opens the device for reading and writing, sends
    `DeviceOpened` with a write handle from `File::try_clone`, runs `pump`,
    then sends `DeviceClosed`. If the lookup or the open fails, it sends
    `DeviceUnavailable` with the error text. After a close or a failure, it
    waits 5 seconds and tries again. It
    also sends a wake-up on a second `mpsc::Sender<()>` after each message,
    so the poll loop can wake at once.
  - The thread stops when the event receiver is dropped.
- Log with `tracing` only in task 003. This task adds no log line.
- Document each public type and each public function that can fail, with an
  `# Errors` section.

## Implementation Steps

1. Add `connector/midi.rs` with the parser and its unit tests.
2. Add `connector/state.rs` with `ConnectorState`, `DeckChange`, `Mode` and
   their unit tests.
3. Add `connector/device.rs` with `resolve_raw_device`, `pump`,
   `spawn_reader` and the unit tests for the first two.
4. Add `connector/mod.rs` and export `connector` from `lib.rs`.

## Acceptance Criteria

Mechanical. Each item is a unit test:

- Parser: a complete message; a message split over three reads; running
  status for two messages; a real-time byte inside a message; a SysEx block
  before a message; a note-on message that gives nothing.
- State: a repeated `play = 0` gives one change; a duration of 200 from CC
  31 = 1 then CC 41 = 72; a duration high part with no low part gives no
  change; channel 1 messages change nothing; a loudest value of 5 is ignored.
- Mode: no device gives `NoDevice`; no heartbeat gives `NoHeartbeat`; a
  heartbeat 2.9 seconds ago gives `Connector`; a heartbeat 3.1 seconds ago
  gives `NoHeartbeat`; version 2 gives `UnknownVersion`.
- Device: `resolve_raw_device` with a `tempfile` directory and a symbolic
  link `V4V -> card31` gives `<dev_snd>/midiC31D0`; a missing link gives an
  error that names the card ID.
- `pump` with a byte buffer sends each control change and returns at end of
  file.
- No new dependency. `Cargo.lock` does not change.
- The full gate passes.

## Test Commands

```bash
cargo fmt --all -- --check
cargo build --workspace
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
```

## Escalation Triggers

Stop and report if one of these occurs:

- The reader thread needs a non-blocking read or a feature outside the
  standard library.
- The `/proc/asound/<ID>` link has a different form from `cardN`.
- You think the state must hold more than four decks.

## Prompt for lower-context coding model

You are implementing one bounded task from a larger plan.

Implement only this task. Do not redesign the architecture.

Read:
- docs/adr/0006-mixxx-midi-connector.md
- docs/tasks/mixxx-connector-task-002-connector-core.md
- mixxx-now-playing/src/lib.rs
- mixxx-now-playing/src/lock.rs

Goal:
- Add a pure connector core in mixxx-now-playing/src/connector/: a MIDI byte parser, the deck state and mode, and the raw device reader. Do not connect it to main.rs.

Constraints:
- Standard library only. No new dependency.
- Parser: running status, real-time bytes inside a message are ignored, SysEx and other message kinds are skipped.
- ConnectorState: channel 16 only; decks 1 to 4 with play, track_loaded, duration_secs; loudest deck 0 to 4 (ignore above 4); heartbeat time and version. apply(cc, now) gives a DeckChange only when a value changes. Duration applies when CC 40+N arrives. Values of 64 or more are true.
- mode(now, device_open): Connector needs an open device, a heartbeat less than 3 s old and version 1. Else HistoryOnly(NoDevice | NoHeartbeat | UnknownVersion).
- The caller gives every Instant.
- resolve_raw_device(proc_asound, dev_snd, card_id) follows the cardN link to dev_snd/midiCND0.
- STATE_REQUEST = [0xBF, 0x01, 0x01].
- pump(reader, sender) parses bytes and sends each control change with its Instant.
- spawn_reader: one thread "connector-midi". Open read and write, send DeviceOpened with a try_clone write handle, pump, send DeviceClosed, wait 5 s, try again. A lookup or open failure sends DeviceUnavailable with the error text, then waits 5 s. Send a wake-up after each message. Stop when the receiver is dropped.
- No log lines in this task.

Do not touch:
- mixxx-now-playing/src/main.rs, cli.rs, config.rs
- src/**, mixxx/**, packaging/**, systemd/**, scripts/**, docs/adr/**

Acceptance criteria:
- The unit tests listed in the task §Acceptance Criteria exist and pass.
- Cargo.lock does not change.
- The full gate passes.

Test commands:
- cargo fmt --all -- --check
- cargo build --workspace
- cargo test --workspace
- cargo clippy --workspace --all-targets -- -D warnings

At the end, report:
1. files changed
2. tests run
3. behavior changed
4. deviations from task
5. unresolved concerns

## Review Change

Changed 2026-09-28 in the review:

- A duration change was reported only when the low 7 bits changed. A new
  track of 328 seconds after a track of 200 seconds has the same low part, so
  the link would not end. The state now compares the full duration. A test
  covers this case.
- The reader opened the device for reading only. The write handle for
  `STATE_REQUEST` came from that file, so no request could reach Mixxx. The
  reader now opens the device for reading and writing.
- The reader sent a wake-up only at an open or a close. `pump` now sends a
  wake-up after each read that gives a control change.

The review also changed the API for task 003:

- `DeckChange` holds a deck number and a `DeckChangeKind`: `Play`,
  `TrackLoaded` or `Duration`.
- `duration_secs(deck)` gives `None` until the mapping sends a duration.
- `clear()` also clears the heartbeat. After a new open, the mode is the
  connector mode only after the first heartbeat.
- `spawn_reader` takes a `DeviceLocation` and returns a `Result`. It no longer
  stops the process when the thread cannot start.

Each rule was broken on purpose, and a test failed each time.
