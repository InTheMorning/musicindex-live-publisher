# Mixxx MIDI Connector Phase Plan

Date: 2026-09-28. This plan states no rule. ADR 0006 owns the rules here. ADR
0005 owns the history-only mode.

## Goal

`mixxx-now-playing` knows when the deck of the present track stops. When this
plan is complete:

- A stop or a pause of the linked deck removes the drop file at once, and a
  resume writes it again.
- A history row pays only the artist of the loudest deck that the mapping
  reported.
- `duration_secs` in the drop file comes from the Mixxx deck in the connector
  mode.
- Without the connector, the producer operates as it does now, with the ADR
  0005 expiry.
- The package makes the V4V card, installs the mapping, and the setup script
  checks the card.

## Non-Goals

- Commands from `v4vmm` to Mixxx, other than the state request.
- The talk break block.
- A drop-file schema change. The schema stays `musicindex.nowplaying/1`.
- A change to the publisher crate.

## Assumptions

- Mixxx 2.5.6 on Linux. The mapping copies the Mixxx 2.5.6 rule for the
  loudest deck.
- Node.js 22 or later runs the mapping tests with `node --test`. The local
  version is 26.10.0. The tests use no npm package.
- The crates `tracing` and `tracing-subscriber` are already in `Cargo.lock`,
  because the publisher uses them. The producer can use them with no new crate
  in the lock file.
- A user service can open `/dev/snd/midiCND0` when the unit does not hide
  `/dev`. systemd-logind gives the seat user access to the sound devices.

## Present Behavior

- The producer reads the history database each 0.5 seconds. A new V4V row
  writes the drop file. The file stays until the next row or until the ADR
  0005 expiry ends.
- `duration_secs` comes from the stream headers, through lofty.
- A late MusicIndex API result writes the drop file again with the new routes.
- The producer unit sets `PrivateDevices=true`. So the producer cannot see
  `/dev/snd`.

## Affected Modules

| Module | Change | Task |
|---|---|---|
| `mixxx/` (new) | The mapping XML, the mapping script and its tests | 001 |
| `AGENTS.md` | The mapping test command | 001 |
| `mixxx-now-playing/src/connector/` (new) | The MIDI parser, the deck state, the mode and the device reader | 002 |
| `mixxx-now-playing/src/lib.rs` | Export `connector` | 002 |
| `mixxx-now-playing/src/connector/link.rs` (new) | The link rules | 003 |
| `mixxx-now-playing/src/main.rs`, `cli.rs` | Use the connector in the poll loop | 003 |
| `mixxx-now-playing/Cargo.toml` | `tracing` and `tracing-subscriber` | 003 |
| `AGENTS.md` §4 | The connector payment rule | 003 |
| `packaging/`, `systemd/`, `scripts/setup-mixxx-musicindex.sh` | The card files, the mapping install, the unit and the card check | 004 |
| `docs/runbooks/musicindex-live-publisher-configuration.md` | The operator setup | 004 |

## Sequence

1. Task 001: the mapping.
2. Task 002: the connector core in the producer.
3. Task 003: the link rules and the poll loop. It needs task 002.
4. Task 004: packaging and setup. It needs tasks 001 and 003.
5. The manual checks in §Manual Checks, then the review.

Tasks 001 and 002 can go in either order. Each task is one commit.

## Schema And API Implications

- **Drop file:** no field change. In the connector mode, `duration_secs`
  comes from the deck. ADR 0002 already says that nothing verifies that
  value.
- **Producer CLI:** `--connector-card ID` sets the card ID. The default is
  `V4V`. `--no-connector` turns the connector off. `--once` never uses the
  connector.
- **MIDI protocol:** version 1, as ADR 0006 §Protocol states.
- **Package:** new files in `/usr/lib/modules-load.d/`, `/usr/lib/modprobe.d/`
  and `/usr/share/mixxx/controllers/`.
- **Unit:** the producer unit sets `PrivateDevices=false`.

## Risk Areas

- **A late API result.** Today a late result writes the drop file. After a
  stop, that write would make the stopped artist payable again. Task 003 lets
  a result change the file only while it is present.
- **The order in each loop pass.** The producer must apply all MIDI messages
  that arrived before it reads the history. Otherwise a new row can link to a
  loudest deck that is out of date.
- **The startup race.** The latest history row can come from an earlier
  Mixxx session. Task 003 acts on no row until the mode is known, and the
  connector mode never links a row that it read before the entry.
- **The unit sandbox.** `PrivateDevices=true` hides `/dev/snd`. Task 004 sets
  `PrivateDevices=false`. A user service already has the device access of its
  user, so the change gives the producer no access that the user does not
  have. The other sandbox settings stay.
- **The setup script stops when the card is missing.** ADR 0006 says so. The
  card exists only after a reboot. So the message tells the operator to
  reboot after the package install.
- **The mapping copies a Mixxx rule.** ADR 0006 records this risk. The mapping
  tests fix the copy of the 2.5.6 rule.

## Test Strategy

- The mapping tests run the mapping script in the Node.js `vm` module with a
  stub `engine` and a stub `midi` object. They need no Mixxx.
- The parser, the deck state, the mode and the link rules are pure code in
  `mixxx-now-playing/src/connector/`. Unit tests give them byte sequences,
  events and times. No test opens a sound device.
- The device reader takes any `Read`. A test gives it a byte buffer.
- The card lookup takes the `/proc/asound` and `/dev/snd` directories as
  arguments. A test uses `tempfile` directories and a symbolic link.
- Each task runs the full gate in `AGENTS.md`.

## Manual Checks

These checks need a running Mixxx with the mapping on `VirMIDI 31-0`. They
are visual. The review records each result. A check that cannot run is an
open gate, not a pass.

1. Play a V4V track. Stop its deck. The drop file goes away at once. Start the
   deck again. The drop file comes back.
2. Let AutoDJ change the track. The drop file changes to the new track when
   the history row appears.
3. Stop the producer while a V4V track plays. Start it again. No drop file
   appears until the next track.
4. Disable the connector mapping in Mixxx. After 3 seconds, the producer logs
   the history-only mode. Enable it again. The producer logs the connector
   mode.
5. The installed producer unit opens `/dev/snd/midiC31D0`. The unit log shows
   the connector mode.

## Rollback

Each task is one commit. `git revert` of that commit restores the previous
behavior. No task migrates stored data. After a revert of task 004, the V4V
card stays until the next reboot.

## Tasks

- `docs/tasks/mixxx-connector-task-001-mapping.md`
- `docs/tasks/mixxx-connector-task-002-connector-core.md`
- `docs/tasks/mixxx-connector-task-003-link-and-poll-loop.md`
- `docs/tasks/mixxx-connector-task-004-packaging-and-setup.md`
- `docs/reviews/mixxx-connector-review-checklist.md`

## Later Decisions

- Commands from `v4vmm` to Mixxx, for example a talk break.
- A track identity source that is earlier than the history row.
