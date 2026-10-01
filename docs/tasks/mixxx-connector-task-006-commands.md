# Mixxx Connector Task 006: Commands To Mixxx

Status: Implemented - 2026-09-30. The visual checks passed on 2026-10-01.
See §Review Result.

The acceptance criteria are mechanical. The visual checks are in a separate
list.

## Goal

Protocol version 3 of ADR 0007. The mapping does the AutoDJ fade-now command
and answers. `mixxx-now-playing command fade-now` sends the command and gives
an exit code for the result.

## Files To Inspect

- `docs/adr/0007-mixxx-connector-commands.md` (all of it)
- `docs/adr/0006-mixxx-midi-connector.md` (§Transport and §Protocol)
- `mixxx/MusicIndex-V4V-Connector.midi.xml`, `mixxx/MusicIndex-V4V-Connector.js`
  and `mixxx/tests/connector.test.js`
- `mixxx-now-playing/src/connector/` (all modules)
- `mixxx-now-playing/src/main.rs` (`main`) and `mixxx-now-playing/src/cli.rs`

## Files Likely To Change

- `mixxx/MusicIndex-V4V-Connector.midi.xml`
- `mixxx/MusicIndex-V4V-Connector.js`
- `mixxx/tests/connector.test.js`
- `mixxx-now-playing/src/connector/state.rs`
- `mixxx-now-playing/src/connector/command.rs` (new)
- `mixxx-now-playing/src/connector/mod.rs`
- `mixxx-now-playing/src/main.rs`
- `mixxx-now-playing/src/cli.rs`, only if the command parser goes there
- `docs/runbooks/musicindex-live-publisher-configuration.md` (§MIDI Connector)

## Do Not Touch

- `src/**` (the publisher)
- `mixxx-now-playing/src/connector/link.rs`, except the version in a test
  helper
- `packaging/**`, `systemd/**`, `scripts/**`
- `docs/adr/**`

## Constraints

The mapping:

- The heartbeat value is 3.
- The XML maps a second input: status `0xBF`, control 4, to the script
  function `V4VConnector.command`.
- `V4VConnector.command(channel, control, value)`:
  - Code 1: if `[AutoDJ],enabled` is 1, set `[AutoDJ],fade_now` to 1, then to
    0, then send CC 4 with the value 1. Otherwise send CC 5 with the value 1.
  - Any other code: send CC 5 with that code. Set no control.

The producer:

- `PROTOCOL_VERSION` is 3.
- The state ignores CC 4 and CC 5 from Mixxx. They do not change a deck and
  they do not give an event.

The command line:

- `main` handles `command` as the first argument before it parses the
  producer options and before `ResolvedConfig::resolve`. The command needs no
  Mixxx database and no output paths.
- The form is `mixxx-now-playing command fade-now [--connector-card ID]
  [--timeout SECS]`. The default card ID is `V4V`. The default timeout is 2
  seconds. The timeout is one limit for the full command.
- A wrong command line gives exit code 2 and one usage line on stderr.
- Put the logic in `connector/command.rs` as a function that takes a
  receiver of `DeviceEvent`, a writer, the command code and a deadline. It
  returns an outcome: `Done`, `Refused`, `NotSent` or `Unknown`. The tests
  use this function with a channel and a `Vec<u8>`. No test opens a device.
- The function:
  1. waits for a heartbeat with the value 3. Other heartbeat values do not
     count.
  2. writes `[0xBF, 0x04, code]` in one `write_all` call.
  3. waits for CC 4 or CC 5 on channel 16 with the same value as the code.
     Other messages do not count.
  4. gives `NotSent` if the deadline comes before step 2, and `Unknown` if the
     deadline comes after step 2.
- The binary opens the device with the card lookup of `device.rs` and reads
  it with `pump` in one thread. It does not use `spawn_reader`, because a
  command does not open the device again. A device that the command cannot
  find or open gives `NotSent`.
- Exit codes: `Done` 0, `Refused` 3, `NotSent` 4, `Unknown` 5. Write one line
  to stderr for each outcome other than `Done`. The line for `Unknown` says
  that the command can have run.
- The command does not take the producer lock, and it does not change a drop
  file.
- The command does not start the `tracing` subscriber output for the
  producer. Use `eprintln!` for its one line, because the line is the command
  output for a person or a script.

The runbook:

- Add a subsection "Commands To Mixxx" to §MIDI Connector. It restates ADR
  0007, names it as the owner, and lists the command and the exit codes. It
  says that a caller must not repeat a command after exit code 5 without a
  check of the deck state.

## Implementation Steps

1. Change the mapping and its tests.
2. Change `PROTOCOL_VERSION` and the state, and their tests.
3. Add `connector/command.rs` and its tests.
4. Change `main.rs` for the subcommand and the exit codes.
5. Add the runbook subsection.

## Acceptance Criteria

Mechanical. Each item is a test:

- Mapping: code 1 with AutoDJ enabled sets `fade_now` to 1 then to 0, and
  sends `[0xBF, 4, 1]`.
- Mapping: code 1 with AutoDJ disabled sets no control, and sends
  `[0xBF, 5, 1]`.
- Mapping: code 9 sets no control, and sends `[0xBF, 5, 9]`.
- Mapping: the heartbeat value is 3.
- State: a heartbeat of version 3 gives the connector mode. A heartbeat of
  version 2 gives `UnknownVersion`. CC 4 and CC 5 give no event.
- Command: a heartbeat of version 3 and then CC 4 = 1 gives `Done`, and the
  writer holds exactly `[0xBF, 0x04, 0x01]`.
- Command: CC 5 = 1 gives `Refused`.
- Command: no heartbeat gives `NotSent`, and the writer is empty.
- Command: only heartbeats of version 2 give `NotSent`, and the writer is
  empty.
- Command: a heartbeat and no answer gives `Unknown`.
- Command: CC 4 with a different value, or on a different channel, before the
  deadline does not give `Done`.
- The command line: a missing command name, an unknown command name, and a
  `--timeout` that is not a positive number each give exit code 2. Use the
  binary for these tests. No test opens a device.
- Each rule above was broken on purpose, and a test failed. The report lists
  each mutation.

Also:

- The full gate passes, with `node --test mixxx/tests/`.
- `Cargo.lock` does not change.

Visual. A person checks these with Mixxx and the development setup in
`docs/plans/mixxx-midi-connector.md`. Copy the new mapping to
`~/.mixxx/controllers/` and restart Mixxx first:

- With AutoDJ on and one track playing, `mixxx-now-playing command fade-now`
  starts the transition and exits with 0.
- With AutoDJ off, it exits with 3, and nothing changes in Mixxx.
- With the mapping disabled, it exits with 4 after about 2 seconds.
- The development producer stays in the connector mode during these checks.

## Test Commands

```bash
node --test mixxx/tests/
cargo fmt --all -- --check
cargo build --workspace
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
```

## Escalation Triggers

Stop and report if one of these occurs:

- A rule in §Constraints conflicts with ADR 0007.
- The command cannot stop its reader thread at the deadline without a new
  dependency. A thread that blocks in `read` can stay until the process
  exits, because the process exits at once after the outcome.
- `Cargo.lock` needs a change.

## Prompt for lower-context coding model

You are implementing one bounded task from a larger plan.

Implement only this task. Do not redesign the architecture.

Read:
- docs/adr/0007-mixxx-connector-commands.md
- docs/adr/0006-mixxx-midi-connector.md (§Transport, §Protocol)
- docs/tasks/mixxx-connector-task-006-commands.md
- mixxx/MusicIndex-V4V-Connector.midi.xml, mixxx/MusicIndex-V4V-Connector.js, mixxx/tests/connector.test.js
- mixxx-now-playing/src/connector/ (all), mixxx-now-playing/src/main.rs, mixxx-now-playing/src/cli.rs

Goal:
- Protocol version 3: the mapping does command code 1 (AutoDJ fade-now) and answers CC 4 (did) or CC 5 (refused). `mixxx-now-playing command fade-now` sends it and exits 0, 2, 3, 4 or 5.

Constraints:
- Mapping: heartbeat 3; XML input 0xBF control 4 to V4VConnector.command; code 1 with [AutoDJ],enabled 1 sets fade_now 1 then 0 and sends CC 4 = 1, else CC 5 = 1; other codes send CC 5 = code.
- Producer: PROTOCOL_VERSION 3; CC 4 and CC 5 from Mixxx give no event.
- main handles `command` before the producer options and before ResolvedConfig::resolve.
- connector/command.rs: a function over a DeviceEvent receiver, a writer, the code and a deadline. Wait for heartbeat value 3; one write_all of [0xBF, 0x04, code]; wait for CC 4 or CC 5 on channel 16 with the same value. Deadline before the write gives NotSent, after gives Unknown.
- The binary opens the device with the card lookup and reads it with pump in one thread; a device that cannot be found or opened gives NotSent.
- Exit codes: Done 0, usage 2, Refused 3, NotSent 4, Unknown 5. One eprintln! line for each non-Done outcome.
- No producer lock and no drop file change.
- Runbook §MIDI Connector gets "Commands To Mixxx", restating ADR 0007 and naming it as the owner.

Do not touch:
- src/**, packaging/**, systemd/**, scripts/**, docs/adr/**
- mixxx-now-playing/src/connector/link.rs, except the version in a test helper

Acceptance criteria:
- The tests in the task §Acceptance Criteria exist and pass.
- Each rule was broken on purpose and a test failed; report the list.
- The full gate passes. Cargo.lock does not change.

Test commands:
- node --test mixxx/tests/
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
6. the mutation list with results

## Review Result

Reviewed 2026-09-30. The review changed no code. Each rule was broken on
purpose, and a test failed each time. No test opens a device.

The review accepts these deviations:

- A failed write gives `Unknown` (exit code 5), not `NotSent`. A part of the
  message can have reached Mixxx, so the command line cannot prove that the
  command was not sent.
- A reader that stops ends the wait at once. It gives `NotSent` before the
  write and `Unknown` after it.
- A `--timeout` that is too large for the clock gives exit code 2.
- `device.rs` makes `open_device` public, so the command uses the same open
  code as the producer.

The visual checks need Mixxx. They are open.

Changed 2026-10-01: the visual checks passed with Mixxx 2.5.6.

- With AutoDJ on, the fade started, and the exit code was 0.
- With AutoDJ off, the mapping refused the command, and the exit code was 3.
- With the mapping disabled, the command was not sent, and the exit code was
  4. The producer logged `history-only mode` with `reason=NoHeartbeat` only in
  this check.
