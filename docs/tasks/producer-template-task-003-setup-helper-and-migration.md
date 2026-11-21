# Producer Template Task 003: The Setup Helper And The Migration

Status: Planned 2026-11-21.

Every criterion is mechanical.

## Goal

`setup-mixxx-musicindex` has the interface of ADR 0021 §The Setup Helper. It writes no unit file. It does the
migration of ADR 0021 §Migration with `--migrate-units`.

## Base components

| Component | Status | Location | Reason |
| --- | --- | --- | --- |
| Setup helper | extend | scripts/setup-mixxx-musicindex.sh | Add the flags. Remove the unit writers and the modes. |
| Setup helper tests | extend | tests/setup_helper.rs | Keep the stub publisher and the stub systemctl. Replace the tests of `--units-only`. |
| Configuration writer | reuse | scripts/setup-mixxx-musicindex.sh | Keep the reserved safety rules: placeholder check, backup, `--force`. |

## Files To Inspect

- `docs/adr/0021-mixxx-producer-template-unit.md`
- `scripts/setup-mixxx-musicindex.sh`
- `tests/setup_helper.rs`
- `systemd/mixxx-now-playing@.service`, `systemd/musicindex-live-publisher@.service` (task 002)
- `docs/tasks/reserved-safety-task-003-setup-keeps-event.md`, `docs/tasks/reserved-safety-task-004-setup-display.md`

## Files Likely To Change

- `scripts/setup-mixxx-musicindex.sh`
- `tests/setup_helper.rs`
- `docs/runbooks/musicindex-live-publisher-configuration.md`, `docs/runbooks/musicindex-live-publisher-deploy.md`,
  `docs/runbooks/musicindex-live-publisher-arch-package.md`: the setup commands and the unit names

## Do Not Touch

- `src/**`, `mixxx-now-playing/**`, `systemd/**`, `packaging/**`, `Cargo.toml`.
- `docs/adr/**`.

## Constraints

- Flags: `--instance NAME` (default `mixxx`), `--target NAME` (default `default`), `--endpoint URL`,
  `--event-id ID`, `--token-file PATH`, `--display`, `--force`, `--no-start`, `--json`, `--migrate-units`, `-h`.
- Remove `--permanent`, `--temporary` and `--units-only`. Each removed flag gives an error that names ADR 0021 and
  the replacement.
- Validate the instance name with `^[a-z0-9][a-z0-9-]{0,31}$`. Validate the target name with the same rule.
- `--event-id` and `--token-file` go together. The token file must exist, must be a regular file and not a symbolic
  link, and must have mode `0600`. The helper sends no relay request and does not copy, move or read the token.
- The configuration is `${XDG_CONFIG_HOME:-$HOME/.config}/musicindex-live-publisher/<instance>/config.toml`, mode
  `0600`, directory mode `0700`. It has one `[[target]]`. With `--display`, the target has
  `display_dir = "$XDG_RUNTIME_DIR/musicindex-live-publisher/<instance>/display"`.
- Keep the reserved safety rules for an existing configuration and an existing token.
- Keep the MIDI card check.
- The helper writes no unit file. Remove `write_generated_unit` and the unit text.
- For the instance `mixxx` without `--no-start`: `systemctl --user daemon-reload`, then
  `systemctl --user enable --now musicindex-live-publisher@mixxx.service mixxx-now-playing@mixxx.service`.
- For each other instance: no `systemctl` command. `started` is `false` in the JSON.
- `--json`: the last line of standard output is one JSON object with `instance`, `target`, `event_id`,
  `config_file`, `publisher_unit`, `producer_unit` and `started`. All other text goes to standard error. Write the
  JSON with `printf` and escape `"` and `\`. Do not add a dependency on `jq`.
- `--migrate-units` makes the seven steps of ADR 0021 §Migration. It uses the generated marker to find the files of
  the earlier helper. It moves files to `<file>.bak.<stamp>`. It never deletes a configuration or a token. A second
  run makes no change and says so.
- Read no input from the terminal.
- Use `$HOME`, `$XDG_CONFIG_HOME` and `$XDG_RUNTIME_DIR`. Put no home path, no `/run/user/` number, no host name, no
  event ID and no token in the script, the tests or the runbooks. A test event ID is an obvious test value.
- Comments, help text and runbook text in Simplified Technical English (`~/.agents/skills/asd-ste100/SKILL.md`).

## Acceptance Criteria

All are mechanical. Each is a test in `tests/setup_helper.rs` with the stub publisher and the stub `systemctl`.

1. A run with `--instance show --target show --event-id E --token-file T --no-start --json` writes only the
   configuration of `show`. It sends no provision call. It writes no unit file. The JSON line has the seven keys and
   `"started":false`.
2. A run with `--instance show` and no `--no-start` runs no `systemctl` command.
3. A run with no `--instance` provisions, writes the configuration of `mixxx`, and runs `daemon-reload` and
   `enable --now` for the two `mixxx` units.
4. A token file with mode `0644`, a missing token file, a token link, or `--event-id` without `--token-file` stops
   the helper. No file changes.
5. A bad instance name and each removed flag stop the helper with an error that names ADR 0021.
6. `--migrate-units` with the two generated units of the earlier helper and the stub systemctl reporting them
   enabled and active: it disables and stops `mixxx-now-playing.service`, moves the two files to a backup, runs
   `daemon-reload`, and enables and starts the two templates for `mixxx`. The configuration does not change.
7. A second `--migrate-units` changes no file and runs no `enable` or `start`.
8. `--migrate-units` with a unit file that has no generated marker stops without `--force`.
9. The existing tests of the reserved safety rules pass, changed only for the new flags.
10. The gate of `AGENTS.md` passes.

## Delegate Rules

A delegate does not commit. It leaves the change in the working tree and reports the files and the gate result.
