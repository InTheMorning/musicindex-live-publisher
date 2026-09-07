# Publisher Control CLI Implementation Review

## Result

Pass - 2026-09-07.

Merge recommendation: merge.

## Reviewed Artifacts

- `docs/adr/0004-publisher-control-cli.md`
- `docs/plans/publisher-control-cli-phase-plan.md`
- `docs/tasks/control-surface-task-001-target-management.md`
- `docs/tasks/control-surface-task-002-machine-readable-cli.md`
- Commit `a5b434e`: target management commands
- Commit `459854c`: machine-readable CLI surface

## Invariant Review

| Invariant | Result | Evidence |
|---|---|---|
| This repository is the only writer of its configuration file | Pass | `target add` and `target remove` edit the file through `src/config.rs`. Neighbor repositories do not write it. |
| The control CLI does not publish metadata to the relay | Pass | The target commands use config edit helpers. `config show` reads TOML only. `provision` creates a live item only. |
| The control CLI does not restart services | Pass | No control command runs `systemctl`, `journalctl`, or unit reload commands. |
| No command accepts token content as an argument | Pass | Commands accept `--token-file`. No parser accepts a token value. |
| No command prints token content | Pass | Tests cover target list JSON, `provision --json`, and `config show --json`. |
| A target command preserves unrelated configuration content | Pass | Tests cover comments and unrelated fallback tables. |
| Machine-readable output is valid JSON when requested | Pass | Binary tests parse `target list --json`, `provision --json`, and `config show --json`. |
| Exit codes distinguish missing and duplicate targets | Pass | Binary tests verify exit code `2` for duplicate targets and `3` for missing targets. |

## Task Packet Review

Control surface task 001 is complete.

- `target add`, `target list`, and `target remove` exist.
- Duplicate and missing target states have distinct exit codes.
- Token file validation uses the token path only.
- Config edits write through a temporary file and rename.
- Documentation describes the commands.

Control surface task 002 is complete.

- `provision --json` exists.
- `config show --json` exists.
- `--version` exists.
- JSON command failures return one JSON object with an `error` field.
- Documentation describes token handling for machine-readable output.

## Drift Review

No architectural drift found.

The implementation did not change the drop-file contract, the live value
payload shape, the watcher, the scheduler, the relay publish path, systemd
units, or `mixxx-now-playing`.

## Test Evidence

Last recorded local verification for the task series:

- `cargo fmt --all -- --check`: pass
- `cargo check --workspace --quiet`: pass
- `cargo test --workspace --quiet`: pass
- `cargo clippy --workspace --quiet -- -D warnings`: pass
- `git diff --check`: pass

## Required Fixes

None.

## Optional Improvements

- Add shell-level examples for the exact `config show` JSON body after the
  first `v4vmm` consumer lands.

## Open Gates

None.
