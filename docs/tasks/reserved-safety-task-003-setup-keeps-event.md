# Reserved Safety Task 003: The Setup Helper Keeps An Existing Event

Status: Ready - 2026-10-04.

Every criterion is mechanical.

## Goal

`setup-mixxx-musicindex` never replaces a config that names a real event,
unless the operator gives `--force`. A new option `--units-only` writes the
two user units again and changes no config and no token. An operator can then
upgrade the units of a computer with a reserved event.

## Context

`can_replace_config` in `scripts/setup-mixxx-musicindex.sh` allows a
replace when the config has the generated marker. The token check stops the
helper only when `tokens/default.token` exists. On a computer where the
target uses another token file, the helper provisions a new ephemeral event,
writes a new config and rewrites both units. The reserved event then leaves
the config.

Item 5 of `docs/plans/packaging-pass.md` asks for an upgrade path that changes
only the units. The review of 2026-10-04 found the replace path. See
`docs/plans/reserved-event-safety.md`.

## Files To Inspect

- `scripts/setup-mixxx-musicindex.sh`
- `tests/` and `mixxx-now-playing/tests/` for the present helper tests
- `docs/plans/packaging-pass.md` (items 4 and 5)
- `docs/runbooks/musicindex-live-publisher-arch-package.md`
- `packaging/arch/musicindex-live-publisher.install`

## Files Likely To Change

- `scripts/setup-mixxx-musicindex.sh`
- The helper test file
- `docs/runbooks/musicindex-live-publisher-arch-package.md`
- `packaging/arch/musicindex-live-publisher.install` (the message)
- `docs/plans/packaging-pass.md` (item 5: say that this task closes it)

## Do Not Touch

- `src/**` and `mixxx-now-playing/src/**`
- `docs/adr/**`
- The unit text, except as task 004 changes it

## Constraints

- A config may be replaced without `--force` only when it does not exist, or
  when it holds the placeholder event ID. The marker alone no longer allows
  it.
- With `--force`, the helper backs up the config as today.
- When the helper stops because of the config, the message names the event
  ID in the config. It says that no relay request ran, and it names
  `--units-only`.
- Do the config check and the token check before the relay request, as
  today.
- `--units-only`:
  - It writes the publisher unit and the producer unit, with a backup of
    each, as today.
  - It reads no token, writes no config, writes no token and sends no relay
    request.
  - It needs an existing config file. Without one, it stops.
  - It runs `systemctl --user daemon-reload`. It restarts the two units only
    without `--no-start`.
- The card check of ADR 0006 runs in each mode.

## Implementation Steps

1. Change `can_replace_config`.
2. Add `--units-only`.
3. Add the tests.
4. Update the runbook, the install message and packaging item 5.

## Acceptance Criteria

Each item is a test. Run the helper with a stub `musicindex-live-publisher`
that records each call, a temporary `XDG_CONFIG_HOME`, `--no-start`, and
`MUSICINDEX_ASOUND_DIR`:

- A config with the marker and a real event ID, and no token file: the
  helper stops, the config does not change, and the stub records no
  `provision`.
- The same with `--force`: the helper backs up the config and provisions.
- A config with the placeholder event ID: the helper provisions, as today.
- `--units-only` with a config: both units are written, and the config and
  each file in the token directory do not change. The stub records no call.
- `--units-only` with no config: the helper stops.
- Each rule was broken on purpose, and a test failed. The report lists each
  mutation.

Also: the full gate passes.

## Test Commands

```bash
cargo fmt --all -- --check
cargo build --workspace
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
```

## Escalation Triggers

- The repository has no test for the setup helper, and a new test needs a
  new dependency.
- A change here conflicts with ADR 0006 §Card Setup.
