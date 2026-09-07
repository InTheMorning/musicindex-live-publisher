# Control Surface Task 002: Machine-Readable CLI Surface

## Goal

Let another program read the state of this package without parsing prose. Add
`--json` to `provision`, add `--version`, and add `config show`.

## Files To Inspect

- `docs/architecture/broadcast-chain-boundaries.md`
- `docs/adr/0004-publisher-control-cli.md`
- `src/main.rs` (the `provision` function and the `Cli` parser)
- `src/config.rs`
- `src/relay.rs` (`ProvisionedLiveItem`)
- `docs/runbooks/musicindex-live-publisher-configuration.md`

## Files Likely To Change

- `src/main.rs`
- `src/config.rs`
- `README.md`
- `docs/runbooks/musicindex-live-publisher-deploy.md`

## Do Not Touch

- `src/watcher.rs`, `src/livevalue.rs`, `src/schedule.rs`
- `mixxx-now-playing/**`
- The drop-file contract
- The existing human output of `provision`, which stays the default

## Constraints

- The human output stays the default. `--json` is opt-in, so no operator
  procedure breaks.
- `--json` writes one JSON object to stdout and nothing else. Warnings and
  progress go to stderr.
- **No command prints a token.** `provision` already writes the token file, so
  the JSON output holds the token file path and never the token itself. A
  caller that needs the token reads the file. This keeps every secret out of
  stdout, out of a pipe, and out of shell history.
- `config show` prints the loaded configuration with the token content removed.
  It prints the token file path.
- `--version` prints the package version and exits zero. A caller uses it to
  separate "not installed" from "installed but not configured".
- Exit codes stay stable. A JSON error object does not change an exit code.

## Implementation Steps

1. Add a `--json` flag to the `provision` command.
2. When set, print one object with `event_id`, `token_file`, `target`,
   `metadata_url`, `remote_value_url`, `events_url`, and `socket_io_url`. Do
   not include the token.
3. Keep the current prose output when the flag is absent, including the
   sentence that the token cannot be recovered.
4. Add `--version` to the top-level parser.
5. Add a `config show` command with `--config` and `--json`.
   - the object holds `watch_dir`, `endpoint`, and a target array
   - each target holds `name`, `event_id`, `token_file`, `stream_delay_secs`,
     and whether a fallback is configured
   - **no token content, and no fallback destination address**
6. On a failure with `--json`, print one object with an `error` field to stdout
   and keep the existing exit code.
7. Add tests: provision JSON shape with a stub relay, token content absent from
   the provision JSON, config show JSON shape, token content absent from
   `config show`, `--version` output, and prose output unchanged without the
   flag.
8. Document the three additions in `README.md` and in the deployment runbook.
   State that the token reaches the token file only, and that a caller reads it
   from there.

## Acceptance Criteria

- `provision` without `--json` prints exactly what it printed before.
- `provision --json` prints one object and nothing else on stdout, and that
  object holds no token.
- `config show --json` never holds token content.
- `--version` exits zero and prints the version.
- Exit codes are unchanged.

## Test Commands

- `cargo fmt --all -- --check`
- `cargo check --workspace --quiet`
- `cargo test --workspace --quiet`
- `cargo clippy --workspace --quiet -- -D warnings`

## Expected Final Report Format

1. Files changed
2. Tests run
3. Behavior changed
4. Deviations from task
5. Unresolved concerns

## Escalation Triggers

- A caller needs a field that the loaded configuration does not hold.
- A caller genuinely cannot read the token file, for example across a network
  boundary. Report it rather than adding a token to stdout.

## Prompt for lower-context coding model

You are implementing one bounded task from a larger plan.

Implement only this task. Do not redesign the architecture.

Read:
- `src/main.rs`, `src/config.rs`, `src/relay.rs`
- `docs/adr/0004-publisher-control-cli.md`

Goal:
- Add `provision --json`, `--version`, and `config show [--json]`.

Constraints:
- Human output stays the default. `--json` prints one object on stdout only.
- No command prints a token. `provision --json` prints the token file path.
- `config show` removes token content and fallback addresses.
- Exit codes do not change.

Do not touch:
- watcher, live value, schedule, `mixxx-now-playing`, the drop-file contract

Acceptance criteria:
- Prose output unchanged without the flag.
- `config show --json` holds no secret.
- `--version` exits zero.

Test commands:
- `cargo fmt --all -- --check`
- `cargo test --workspace --quiet`
- `cargo clippy --workspace --quiet -- -D warnings`

At the end, report:
1. files changed
2. tests run
3. behavior changed
4. deviations from task
5. unresolved concerns
