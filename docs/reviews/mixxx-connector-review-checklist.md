# Mixxx Connector Review Checklist

## Scope

Use this checklist after each Mixxx connector task packet lands, and again
after task 004.

Reviewed work:

- `docs/adr/0006-mixxx-midi-connector.md`
- `docs/plans/mixxx-midi-connector.md`
- `docs/tasks/mixxx-connector-task-001-mapping.md`
- `docs/tasks/mixxx-connector-task-002-connector-core.md`
- `docs/tasks/mixxx-connector-task-003-link-and-poll-loop.md`
- `docs/tasks/mixxx-connector-task-004-packaging-and-setup.md`
- The implementation diff for each packet

## Required Checks

Payment safety:

- In the connector mode, no drop file exists while its linked deck does not
  play.
- A history row links only to the loudest deck that the mapping reported. A
  loudest value of 0, or a deck that does not play, gives no drop file.
- A row that the producer read before it entered the connector mode never
  links.
- A change from the connector mode never writes a drop file.
- An API result never writes a drop file that a stop removed.
- In the connector mode, `duration_secs` comes from the deck, never from the
  stream headers.
- Without a heartbeat for 3 seconds, the producer uses the ADR 0005 expiry.
- The expiry source is still the stream header duration, limited by
  `--expiry-max`.

Protocol:

- The mapping and the producer use only 3-byte control change messages on
  channel 16.
- The mapping rule for the loudest deck agrees with ADR 0006 §The Loudest
  Deck. A tie gives the lower deck.
- The producer ignores a repeated value.
- An unknown protocol version gives the history-only mode.

Device and setup:

- No code makes a sequencer port, a JACK port or a PortMidi device.
- The producer finds the device from the card ID, not from a fixed number.
- The package writes nothing in `/etc` and loads no kernel module.
- Both producer unit texts set `PrivateDevices=false`, and keep every other
  sandbox setting.

Contracts and scope:

- The drop-file schema string is still `musicindex.nowplaying/1`.
- The publisher crate did not change.
- No packet changed files on its "Do Not Touch" list.
- `Cargo.lock` adds no new crate.
- `AGENTS.md` §4 names the connector rule and ADR 0006.

## Test Commands

- `node --test mixxx/tests/`
- `cargo fmt --all -- --check`
- `cargo build --workspace`
- `cargo test --workspace`
- `cargo clippy --workspace --all-targets -- -D warnings`
- `bash -n scripts/setup-mixxx-musicindex.sh`

## Manual Checks

Record the result of each check in `docs/plans/mixxx-midi-connector.md`
§Manual Checks here. A check that did not run is an open gate.

## Review Result

Status: Open - 2026-09-29. Tasks 001 to 004 are merged, and each packet
records its review. Manual checks 1 to 4 did not run. The package gates and
manual check 5 moved to `docs/plans/packaging-pass.md`.
