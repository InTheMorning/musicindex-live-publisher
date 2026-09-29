# Mixxx Connector Task 003: The Link Rules And The Poll Loop

Status: Implemented - 2026-09-29. See §Review Result.

The acceptance criteria are mechanical. The live checks are in
`docs/plans/mixxx-midi-connector.md` §Manual Checks, and the review records
them.

## Goal

The producer uses the connector. In the connector mode, a history row links
to the loudest deck. A stop of that deck removes the drop file, and a resume
writes it again. Without the connector, the producer operates as it does now.

## Files To Inspect

- `docs/adr/0006-mixxx-midi-connector.md` (§How `mixxx-now-playing` Uses The
  Deck State, §When The Connector Is Not Available, §Entering And Leaving The
  Connector Mode, §Invariants)
- `docs/adr/0005-producer-liveness-and-dead-block.md` (§Payment Timing
  Without The MIDI Connector)
- `docs/tasks/mixxx-connector-task-002-connector-core.md`
- `mixxx-now-playing/src/connector/`
- `mixxx-now-playing/src/main.rs` (`run`, `Runtime`, `process_latest`,
  `process_track`, `expire_current_metadata`, `apply_value_route_result`,
  `install_signal_flags`, `sleep_interruptibly`)
- `mixxx-now-playing/src/cli.rs`
- `mixxx-now-playing/src/expiry.rs`
- `Cargo.toml` (the `tracing` versions and features)

## Files Likely To Change

- `mixxx-now-playing/src/connector/link.rs` (new)
- `mixxx-now-playing/src/connector/mod.rs`
- `mixxx-now-playing/src/main.rs`
- `mixxx-now-playing/src/cli.rs`
- `mixxx-now-playing/Cargo.toml`
- `Cargo.lock`, only for the new dependency lines of `mixxx-now-playing`
- `AGENTS.md` (§4)

## Do Not Touch

- `src/**` (the publisher)
- `mixxx-now-playing/src/connector/midi.rs`, `state.rs`, `device.rs`, except
  for a defect that you report
- `mixxx-now-playing/src/history.rs`, `sink.rs`, `render.rs`, `tags.rs`
- `mixxx/**`, `packaging/**`, `systemd/**`, `scripts/**`
- `docs/adr/**`

## Constraints

Put every decision in a pure type in `connector/link.rs`, named
`Coordinator`. `main.rs` does only the file writes, the renders and the
device writes that the `Coordinator` asks for. The tests use the
`Coordinator`. They do not start the binary.

The `Coordinator` rules:

- **Startup gate.** It allows no history poll until the mode is known. The
  mode is known at the first heartbeat, at `DeviceUnavailable`, or 3 seconds
  after the start.
- **A new V4V row in the connector mode.** It links the row to the loudest
  deck in `ConnectorState` if that value is 1 to 4 and that deck plays.
  Otherwise the row has no link, and it asks to remove the drop file.
- **A linked row** asks to write the drop file with `duration_secs` from the
  deck. A deck with no known duration gives no `duration_secs`.
- **A non-V4V row, or a row whose tags cannot be read,** ends the link. The
  present code already removes the file for these rows.
- **Deck changes for the linked deck:**
  - `play` becomes false: remove the drop file. The link stays.
  - `play` becomes true: write the drop file again.
  - `track_loaded` or `duration` changes: remove the drop file, and end the
    link.
- **A change to the connector mode:** ask to send `STATE_REQUEST`, remove the
  drop file, and end the link. A row that was read before the change never
  links.
- **A change from the connector mode:** do not write the drop file. The ADR
  0005 expiry applies from the time of the row. If it has ended, remove the
  drop file at once.
- **The expiry** applies only in the history-only mode. In the connector mode
  a long track keeps its file while its deck plays.
- **A MusicIndex API result** may change the drop file only while the
  `Coordinator` reports the file as present.
- **`DeviceClosed`** clears the deck state. The mode then changes on the next
  pass.

The poll loop:

- Each pass does these steps in this order:
  1. drain all connector events,
  2. apply the mode change,
  3. apply the deck changes,
  4. apply the expiry,
  5. apply the API results,
  6. poll the history, if the startup gate allows it.
- Give the reader thread a clone of the wake-up sender from
  `install_signal_flags`, so a MIDI message ends the sleep at once.
- The expiry keeps its present source: the stream header duration from lofty,
  limited by `--expiry-max`. Only `duration_secs` in the drop file changes
  source in the connector mode.
- `--once` never starts the connector.

The command line:

- `--connector-card ID` sets the card ID. The default is `V4V`.
- `--no-connector` turns the connector off. The producer then always uses the
  history-only mode, and it has no startup gate.

Logs:

- Add `tracing` and `tracing-subscriber` to `mixxx-now-playing/Cargo.toml`
  with the same versions and features as the root `Cargo.toml`.
- Start a subscriber that writes to stderr and reads `RUST_LOG`.
- A change to the history-only mode logs `tracing::warn!` with a `reason`
  field. A change to the connector mode logs `tracing::info!`.
- A row with no link logs `tracing::info!` with a `loudest` field.
- Do not change the existing `println!` and `eprintln!` lines.

`AGENTS.md` §4 gets one bullet: "When the MIDI connector is available, a drop
file exists only while its linked deck plays (ADR 0006)."

## Implementation Steps

1. Add `connector/link.rs` with the `Coordinator`, its action type and its
   unit tests.
2. Add the command-line options in `cli.rs` and their parse tests.
3. Add the log setup and the dependencies.
4. Change `main.rs`: start the reader, follow the loop order, and do the
   actions of the `Coordinator`. Keep the rendered content of the linked
   track, so a resume can write it again.
5. Add the bullet to `AGENTS.md` §4.

## Acceptance Criteria

Mechanical. Each item is a unit test of the `Coordinator`:

- The startup gate blocks the history before the mode is known. It opens at
  the first heartbeat, at `DeviceUnavailable`, and after 3 seconds.
- A row with loudest deck 2, and deck 2 plays: write, with the deck 2
  duration.
- A row with loudest deck 0: no write.
- A row with loudest deck 2, and deck 2 does not play: no write.
- The linked deck stops: remove. It starts again: write.
- A new track on the linked deck (`track_loaded` or `duration`): remove, and
  a later `play = true` writes nothing.
- A deck change on a deck that is not linked: no action.
- A change to the connector mode with a present file: `STATE_REQUEST` and
  remove. The row that was read before the change does not link after the
  state arrives.
- A change from the connector mode after a stop: no write.
- A change from the connector mode when the expiry of the row has ended:
  remove.
- In the connector mode, the expiry does not remove the file of a deck that
  plays.
- After a stop, an API result does not write.
- A row with a stream header duration of 617 seconds and a deck duration of
  200 seconds gives `duration_secs` 200.
- `--no-connector` gives the present behavior: no gate and the expiry
  applies.

Also:

- The `cli.rs` tests cover `--connector-card` and `--no-connector`.
- `Cargo.lock` adds no new crate. Only the dependency list of
  `mixxx-now-playing` changes.
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

- A rule in §Constraints conflicts with ADR 0006.
- `Cargo.lock` needs a new crate.
- The `Coordinator` cannot hold a rule without a file or a device.
- The loop order cannot be kept without a large change to `Runtime`.

## Prompt for lower-context coding model

You are implementing one bounded task from a larger plan.

Implement only this task. Do not redesign the architecture.

Read:
- docs/adr/0006-mixxx-midi-connector.md
- docs/adr/0005-producer-liveness-and-dead-block.md
- docs/tasks/mixxx-connector-task-003-link-and-poll-loop.md
- mixxx-now-playing/src/connector/
- mixxx-now-playing/src/main.rs
- mixxx-now-playing/src/cli.rs
- mixxx-now-playing/src/expiry.rs

Goal:
- Connect the connector core to the producer poll loop. Put every decision in a pure Coordinator in connector/link.rs. main.rs only does the writes that the Coordinator asks for.

Constraints:
- Startup gate: no history poll until the first heartbeat, DeviceUnavailable, or 3 s.
- Connector mode, new V4V row: link to the loudest deck if it is 1 to 4 and plays, else remove the file. A linked row writes with duration_secs from the deck.
- Linked deck play false: remove, keep link. Play true: write again. track_loaded or duration change: remove, end link.
- Enter connector mode: send STATE_REQUEST, remove the file, end the link. A row read before the entry never links.
- Leave connector mode: never write. ADR 0005 expiry from the row time applies. If ended, remove at once.
- Expiry applies only in history-only mode. Expiry source stays lofty, limited by --expiry-max.
- An API result writes only while the file is present.
- DeviceClosed clears the deck state.
- Loop order: drain events, mode change, deck changes, expiry, API results, history poll.
- The reader thread gets a clone of the wake-up sender.
- --once never starts the connector. --connector-card ID (default V4V). --no-connector.
- Add tracing and tracing-subscriber with the root versions and features. warn! on history-only with a reason field, info! on connector mode, info! on a row with no link with a loudest field. Keep existing println!/eprintln!.
- AGENTS.md §4 bullet: "When the MIDI connector is available, a drop file exists only while its linked deck plays (ADR 0006)."

Do not touch:
- src/**, mixxx/**, packaging/**, systemd/**, scripts/**, docs/adr/**
- mixxx-now-playing/src/connector/midi.rs, state.rs, device.rs (report a defect instead)
- mixxx-now-playing/src/history.rs, sink.rs, render.rs, tags.rs

Acceptance criteria:
- Each Coordinator unit test in the task §Acceptance Criteria exists and passes.
- cli.rs tests cover --connector-card and --no-connector.
- Cargo.lock adds no new crate.
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

## Review Result

Reviewed 2026-09-29. The implementation holds each rule in §Constraints. The
review changed no code.

The implementation has two deviations. The review accepts both:

- At startup, the first row after the entry into the connector mode does not
  link. That row existed before the entry. ADR 0006 now says this.
- `tests/lifecycle.rs` passes `--no-connector`. On a computer with a V4V card,
  the test result would otherwise depend on Mixxx.

Each payment rule was broken on purpose, and a test failed each time. The
manual checks in `docs/plans/mixxx-midi-connector.md` did not run. That gate
is open.
