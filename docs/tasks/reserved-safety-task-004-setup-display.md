# Reserved Safety Task 004: The Setup Helper Writes The Display Output

Status: Ready - 2026-10-04. Do after task 002 and task 003.

Every criterion is mechanical.

## Goal

`setup-mixxx-musicindex --display` turns on the display path of ADR 0008. The
producer unit gets `--display-dir` and the display directory in
`RuntimeDirectory=`. A unit that the helper writes again keeps the display
output when the config has a `display_dir`.

## Context

The producer unit sets `ProtectSystem=strict`. The producer can write only in
the paths of `RuntimeDirectory=`. On 2026-10-04 the operator added the display
directory to the generated unit by hand. A new run of the helper removes it.
Item 9 of `docs/plans/packaging-pass.md` records this.

The display path needs a reserved event (relay ADR 0001). The helper
provisions an ephemeral event. Thus the display output stays off by default.

## Files To Inspect

- `docs/adr/0008-display-path.md` (§The Producer Output, §The Publisher)
- `scripts/setup-mixxx-musicindex.sh` (after task 003)
- `src/main.rs` (`config show --json`, after task 002)
- `systemd/mixxx-now-playing.service`
- `docs/plans/packaging-pass.md` (item 9)

## Files Likely To Change

- `scripts/setup-mixxx-musicindex.sh`
- The helper test file
- `docs/runbooks/musicindex-live-publisher-configuration.md` (§The Display
  Path)
- `docs/plans/packaging-pass.md` (item 9: say that this task closes it for
  the helper)

## Do Not Touch

- `src/**` and `mixxx-now-playing/src/**`
- `systemd/mixxx-now-playing.service`. The packaged unit stays without the
  display output. The packaging pass decides it.
- `docs/adr/**`

## Constraints

- The display directory is
  `$runtime_root/musicindex-live-publisher/$instance/display`. It is never
  the watch directory.
- `--display` with a new config writes `display_dir` in the target stanza.
- With `--display`, the producer unit gets `--display-dir` with that
  directory, and `RuntimeDirectory=` holds both the drop directory and the
  display directory.
- `--units-only` reads the target through `config show --json`. If the
  target has `display_dir`, the producer unit gets the display output with
  that path. `--display` is then not needed.
- With `--units-only --display`, a config without `display_dir` stops the
  helper. The message says to add the line, and the helper changes no file.
- A `display_dir` that is not the expected directory stops the helper, with
  the two paths in the message.

## Implementation Steps

1. Add `--display`.
2. Read `display_dir` in `--units-only`.
3. Add the tests.
4. Update the runbook and packaging item 9.

## Acceptance Criteria

Each item is a test, with the same stubs as task 003:

- `--display` with a new config: the stanza has `display_dir`, and the
  producer unit has `--display-dir` and both runtime directories.
- No `--display`: neither unit has a display line, as today.
- `--units-only` with a config that has `display_dir`: the producer unit has
  the display output.
- `--units-only --display` with a config without `display_dir`: the helper
  stops and writes no file.
- A `display_dir` that is not the expected directory: the helper stops.
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

- `config show --json` has no `display_dir` (task 002 is not done).
- A rule here conflicts with ADR 0008.
