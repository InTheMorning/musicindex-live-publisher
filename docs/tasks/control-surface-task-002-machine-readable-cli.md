# Control Surface Task 002: Machine-Readable CLI Surface

Status: Implemented - 2026-09-07 (`459854c`). Every criterion is mechanical.
This packet has no visual criteria, because it adds no user interface.

The contract below records what shipped, read from `src/main.rs` and
`src/config.rs` on 2026-09-08. `v4vmm` ADR 0059 task 009 does not yet consume
`--version`, so it still reports one service state fewer. That follow-up is
recorded below and is not done.

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

## Contract With The Control Surface

`v4vmm` runs these commands over a transport and parses the result. A change
here is a cross-repository change.

`--version` prints the bare version on one line and exits `0`. It does not print
the package name:

```text
0.4.1
```

The caller separates three cases, so keep them distinct:

| Observation | Meaning to the caller |
|---|---|
| the binary is absent, or the shell reports "not found" | not installed |
| `--version` exits `0`, and `config show --json` holds an empty target array | installed but not configured |
| `--version` exits `0`, and at least one target is present | installed and configured |

`config show --json` prints `RedactedPublisherConfig`. Its target elements are
`RedactedPublisherTarget`, which carries the four keys that `target list --json`
prints in control-surface task 001, plus the fallback flag. The two commands use
two types, so they can drift. Keep the shared keys identical:

```json
{
  "watch_dir": "/run/user/1000/musicindex-live-publisher/mixxx/nowplaying",
  "endpoint": "https://relay.example",
  "targets": [
    {
      "name": "default",
      "event_id": "01J8Z...",
      "token_file": "/home/operator/.config/musicindex-live-publisher/mixxx/tokens/default",
      "stream_delay_secs": 0.0,
      "fallback_configured": false
    }
  ]
}
```

**Do not let the two commands drift.** A target in `config show` and the same
target in `target list` carry identical values for the shared keys.

An empty target array prints `[]`, never an absent key and never `null`.

A failure with `--json` prints one object with an `error` field on stdout and
keeps the exit code.

## What This Needs From v4vmm Afterwards

`v4vmm` `ServiceState` today holds `Active`, `Inactive`, `Failed`,
`NotInstalled`, `NotReachable`, and `Unknown`. It has no state for "installed
but not configured", so it cannot use the middle row of the table above.

Landing this packet does not change `v4vmm` on its own. A `v4vmm` packet must
add the state and read it from these two commands. Until then, an installed and
unconfigured publisher keeps reporting as it does now.

Record that follow-up before this packet is called complete.

## Acceptance Criteria

- `provision` without `--json` prints exactly what it printed before.
- `provision --json` prints one object and nothing else on stdout, and that
  object holds no token.
- `config show --json` never holds token content.
- Exit codes are unchanged.
- `--version` prints the bare version on one line, without the package name, and
  exits `0`.
- A target in `config show --json` and the same target in `target list --json`
  carry identical values for `name`, `event_id`, `token_file`, and
  `stream_delay_secs`.
- An empty target array prints `[]`.

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
