# Control Surface Task 001: Target Management Commands

Status: Implemented - 2026-09-07 (`a5b434e`). Every criterion is mechanical.
This packet has no visual criteria, because it adds no user interface.

`v4vmm` ADR 0059 task 014 consumes these commands. The contract below records
what shipped, read from `src/main.rs` and `src/config.rs` on 2026-09-08. It is
not a proposal. Another repository parses this, so a change here is a
cross-repository change.

## Goal

Add commands that add, list, and remove a publish target in the configuration
file. This lets a control surface manage targets without writing the file.

## Files To Inspect

- `docs/architecture/broadcast-chain-boundaries.md`
- `docs/adr/0004-publisher-control-cli.md`
- `docs/runbooks/musicindex-live-publisher-configuration.md`
- `src/config.rs`
- `src/main.rs` (the `Cli` parser and the `Command` enum)
- `scripts/setup-mixxx-musicindex.sh` (the marker and backup rules)
- `tests/config.rs`

## Files Likely To Change

- `src/config.rs`
- `src/main.rs`
- `tests/config.rs`
- `docs/runbooks/musicindex-live-publisher-configuration.md`
- `README.md`

## Do Not Touch

- `src/watcher.rs`, `src/livevalue.rs`, `src/relay.rs`, `src/schedule.rs`
- `mixxx-now-playing/**`
- The drop-file contract
- The systemd unit files

## Constraints

- **This repository stays the only writer of its configuration file.** These
  commands exist so that another program never needs to write it.
- Preserve the whole file. Keep comments, key order, and unrelated tables. A
  rewrite that drops operator comments is a defect.
- Write with a temporary file in the same directory, then rename.
- A target name is unique. `add` with a name that exists fails, unless
  `--replace` is given.
- Never accept a token on the command line. Accept a token file path.
- `remove` deletes the target stanza. It does not delete the token file, and it
  does not call the relay.
- These commands do not restart the service. The caller does that.
- Exit with a distinct non-zero code for "target not found" and for "target
  exists", so a caller can separate them.

## Implementation Steps

1. Add a `Target` variant to the `Command` enum with three subcommands: `add`,
   `list`, and `remove`.
2. `target add`:
   - required: `--name`, `--event-id`, `--token-file`
   - optional: `--config`, `--stream-delay-secs`, `--replace`
   - validate that the token file exists and is readable
   - reject an event identifier that holds a newline or a control character
   - append the `[[target]]` table, or replace it when `--replace` is given
3. `target list`:
   - optional `--config` and `--json`
   - print the name, the event identifier, the token file path, and the stream
     delay for each target
   - **never print token content**
4. `target remove`:
   - required `--name`, optional `--config`
   - remove the matching table only
5. Add a config edit module that reads the file as text, applies the change, and
   writes it back. Use a TOML document model that keeps formatting where the
   dependency allows. When formatting cannot be preserved, append the new table
   at the end of the file and say so in the runbook.
6. Add tests: add to an empty config, add a second target, duplicate name
   without and with `--replace`, remove a present target, remove an absent
   target, list as text, list as JSON, and a config that holds comments.
7. Add a test that no command prints token content.
8. Document the three commands in the configuration runbook and in `README.md`.

## Contract With The Control Surface

`v4vmm` runs these commands over a transport and parses the result. These are
not implementation details. A change here is a cross-repository change.

`target list --json` prints one object that holds a `targets` array. It is not
a bare array:

```json
{
  "targets": [
    {
      "name": "default",
      "event_id": "01J8Z...",
      "token_file": "/home/operator/.config/musicindex-live-publisher/mixxx/tokens/default",
      "stream_delay_secs": 0.0
    }
  ]
}
```

The element keys match `TargetConfigSummary` in `src/config.rs`. An empty target
list prints `{"targets": []}`.

Exit codes, because the caller separates these states without reading text:

| Code | Meaning | Constant |
|---|---|---|
| 0 | success | |
| 2 | target exists, from `add` without `--replace` | `EXIT_TARGET_EXISTS` |
| 3 | target not found, from `remove` | `EXIT_TARGET_NOT_FOUND` |
| 1 | every other failure, including an unknown subcommand | |

**Exit `2` does not mean a usage error here.** A caller must not read `2` as
"this publisher has no target commands". An older publisher without these
commands fails with `1` and the message `unknown target subcommand`, so the
caller separates the two by message, not by code alone.

## Acceptance Criteria

- A second target can be added to an existing configuration without `--force`
  and without a rewrite of the whole file.
- Comments in the configuration file survive an add and a remove.
- `target list --json` is machine-readable and holds no token content.
- Duplicate name and missing name return distinct exit codes.
- The service reads a configuration written by these commands without a change
  to `load_config`.
- `target list --json` prints `{"targets": [...]}` with the four element keys
  named in the contract above, and an empty list prints `{"targets": []}`.
- `remove` of an absent target exits `3`. `add` of an existing name without
  `--replace` exits `2`.

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

- The TOML dependency cannot edit a document and keep comments. Report the
  limit and the fallback you chose.
- `load_config` needs a change to read the written file. That is a contract
  change and needs a decision first.

## Prompt for lower-context coding model

You are implementing one bounded task from a larger plan.

Implement only this task. Do not redesign the architecture.

Read:
- `docs/architecture/broadcast-chain-boundaries.md`
- `docs/adr/0004-publisher-control-cli.md`
- `src/config.rs`, `src/main.rs`, `tests/config.rs`

Goal:
- Add `target add`, `target list`, and `target remove` commands that edit the
  configuration file.

Constraints:
- This repository stays the only writer of its config. Preserve comments and
  unrelated tables. Write with a temporary file and a rename.
- Never accept or print a token. Accept a token file path.
- Distinct non-zero exit codes for "not found" and "exists".
- Do not restart the service and do not call the relay.

Do not touch:
- watcher, live value, relay, schedule, `mixxx-now-playing`, unit files

Acceptance criteria:
- A second target can be added without rewriting the file.
- Comments survive. `list --json` holds no token.
- `load_config` reads the result unchanged.

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
