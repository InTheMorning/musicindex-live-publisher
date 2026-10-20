# Show Log Task 004: The Off Switch

Status: Implemented - 2026-10-19. It needs task 003.

Every criterion is mechanical.

## Goal

A top-level configuration key `show_log` turns the show log on or off for
one instance. The default is `true`. With `false`, the publisher writes no
log, also when the unit passes `--state-dir`.

## Files To Inspect

- `docs/adr/0017-show-log.md` (the amendment about `show_log`, §One Log For
  Each Instance)
- `src/config.rs`: `RawConfig`, `PublisherConfig`, `RedactedPublisherConfig`,
  `resolve_show_log_retention_days`, `load_config_bytes`, `show_config`
- `src/main.rs`: `open_show_log`, `run_cli`, `render_config_show_text`
- `tests/config.rs`
- `README.md` §Show Log, `docs/runbooks/musicindex-live-publisher-configuration.md`
  §Show Log and the list of optional top-level fields

## Files Likely To Change

- `src/config.rs`
- `src/main.rs`
- `tests/config.rs`
- `tests/display.rs`, `tests/relay.rs`, `tests/showlog.rs` (only the
  `PublisherConfig` literals, to add the new field)
- `README.md`
- `docs/runbooks/musicindex-live-publisher-configuration.md`

## Do Not Touch

- `src/showlog.rs`, the entries, the watcher and the relay worker.
- The systemd units and `scripts/setup-mixxx-musicindex.sh`. The unit keeps
  `--state-dir`. The configuration decides.
- `mixxx-now-playing/**`, `Cargo.toml`, `docs/adr/**`.

## Constraints

- `show_log` is a top-level boolean in `config.toml`. It is optional. An
  absent key gives `true`. A value that is not a boolean is a configuration
  error that names the key, as for the other keys.
- `PublisherConfig` and `RedactedPublisherConfig` get `show_log: bool`.
  `config show` prints it, in text and in JSON.
- `open_show_log` gets the value. With `false`, it returns `None` at once. It
  makes no directory, runs no `remove_expired` and opens no file. It logs one
  `tracing::info!` line that says that the configuration turns the show log
  off.
- With `false`, the publisher continues to validate
  `show_log_retention_days`. An incorrect value stays an error.
- `target add`, `target remove` and `--replace` keep the key and its line, as
  they keep the other top-level keys. A test proves it.
- The documents state the key, its default, and that the files that exist
  stay until the operator removes them.

## Implementation Steps

1. Add the key to the raw configuration and the resolved configuration.
2. Use it in `open_show_log`.
3. Print it in `config show`.
4. Change the documents.
5. Add the tests of §Acceptance Criteria.

## Acceptance Criteria

Each item is a test:

- A configuration with no `show_log` gives `true`.
- `show_log = false` gives `false`.
- `show_log = "no"` is a configuration error that names `show_log`.
- `open_show_log` with `show_log` `false` and a state directory gives `None`,
  and no `showlog` directory exists after the call.
- `open_show_log` with `show_log` `true` and a state directory opens the log,
  as before.
- `config show --json` has `show_log`.
- `target add` on a configuration with `show_log = false` keeps that line.

Also: the full gate passes.

## Test Commands

```bash
cargo fmt --all -- --check
cargo build --workspace
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
```

## Escalation Triggers

- `target add` drops or moves the new key.
- The switch seems to need a change in the unit or in the setup script.

## Prompt for lower-context coding model

You are implementing one bounded task from a larger plan.

Implement only this task. Do not redesign the architecture.

Read:
- docs/adr/0017-show-log.md
- docs/tasks/show-log-task-004-off-switch.md
- AGENTS.md
- src/config.rs, src/main.rs, tests/config.rs
- README.md, docs/runbooks/musicindex-live-publisher-configuration.md

Goal:
- A top-level configuration key show_log turns the show log on or off for one instance. The default is true. With false, the publisher writes no log, also when the unit passes --state-dir.

Constraints:
- Follow §Constraints of the packet exactly. With false, open_show_log makes no directory and removes no file. config show prints the key. target add keeps the key.

Do not touch:
- src/showlog.rs, the entries, the watcher, the relay worker, the systemd units, scripts/setup-mixxx-musicindex.sh, mixxx-now-playing/**, Cargo.toml, docs/adr/**

Acceptance criteria:
- Each item in §Acceptance Criteria of the packet is a passing test.

Test commands:
- cargo fmt --all -- --check
- cargo build --workspace
- cargo test --workspace
- cargo clippy --workspace --all-targets -- -D warnings

At the end, report:
1. files changed
2. tests run
3. behavior changed
4. deviations from task
5. unresolved concerns

## Review Result

Reviewed 2026-10-19. The full gate passes with 550 tests in the workspace.
Task 003 ended with 545. `Cargo.lock` and `tests/golden.rs` did not change.

- `open_show_log` returns `None` before it uses the state directory when
  `show_log` is `false`. It makes no directory and removes no file.
- An absent key gives `true`. A string is an error that names `show_log`.
  `config show` prints the key in text and in JSON.
- `target add` keeps the line `show_log = false`. A test proves it.
- The agent added the public constant `DEFAULT_SHOW_LOG`, as for
  `DEFAULT_SHOW_LOG_RETENTION_DAYS`.
- The review changed one log message. It had a semicolon, and it now has two
  sentences.
