# Publisher Control CLI Phase Plan

## Status

Accepted - 2026-09-07.

## Goal

Add the command-line surface that lets another program manage publisher targets
and read publisher state without editing configuration files directly.

## Non-Goals

- No remote control API.
- No service start, stop, or reset command.
- No change to the drop-file contract.
- No change to the live value payload.
- No token rotation.

## Current State

- `provision` creates a live item and writes a target configuration.
- Human-readable output is the default.
- The configuration file can be read by this repository.
- No command can add, list, or remove one target.
- No command prints loaded configuration as JSON.
- No `--version` command exists for a control surface probe.

## Target State

- `target add` adds or replaces one target.
- `target list` lists targets as text or JSON.
- `target remove` removes one target.
- `provision --json` prints one JSON object with no token content.
- `config show --json` prints loaded configuration with secrets removed.
- `--version` exits zero and prints the package version.

## Affected Modules

- `src/main.rs`
- `src/config.rs`
- `src/relay.rs`
- `tests/config.rs`
- `README.md`
- `docs/runbooks/musicindex-live-publisher-configuration.md`
- `docs/runbooks/musicindex-live-publisher-deploy.md`

## Proposed Sequence

1. Add target management commands.
2. Add machine-readable state commands.
3. Review the implementation against ADR 0004 and the task packets.
4. Set ADR 0004 and this plan to `Implemented` when the review records no open
   gates.

## Schema And API Implications

No database schema changes exist.

The command-line interface gains new commands and output forms. The existing
human output remains the default.

Exit codes for duplicate and missing targets become a supported contract.

## Risks

| Risk | Mitigation |
|---|---|
| A config edit drops operator comments | Use a document-preserving TOML edit path where possible. Add tests with comments. |
| A token leaks through output | Add tests that command output does not contain token content. |
| A caller cannot distinguish command states | Use distinct exit codes for duplicate and missing targets. |
| JSON output breaks operator procedures | Keep JSON opt-in and keep current prose output unchanged. |

## Test Strategy

- Unit tests for target add, list, replace, and remove.
- Unit tests that comments and unrelated tables survive target edits.
- CLI tests for JSON output shape.
- Tests that token content is absent from every command output.
- Workspace build, test, format, and clippy checks.

## Rollback Strategy

The changes are additive. Existing human commands remain available.

A rollback removes the new commands and leaves the earlier configuration loader
in place.

## References

- `docs/adr/0004-publisher-control-cli.md`
- `docs/tasks/control-surface-task-001-target-management.md`
- `docs/tasks/control-surface-task-002-machine-readable-cli.md`
- `v4vmm`: `docs/plans/broadcast-chain-delivery-order.md`
