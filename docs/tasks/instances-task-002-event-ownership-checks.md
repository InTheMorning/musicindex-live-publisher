# Instances Task 002: One Instance For Each Event

Status: Ready - 2026-10-18.

Every criterion is mechanical.

## Goal

`target add` refuses an `event_id` that a different instance holds. At
startup, the publisher exits with an error when a different instance holds
one of its events. The error names the two instances.

## Files To Inspect

- `docs/adr/0016-instances-events-and-producer-lock.md` (§One Instance For
  Each Event)
- `docs/adr/0004-publisher-control-cli.md` (the exit codes)
- `src/config.rs`: `load_config`, `add_target_to_config`, `RawConfig`,
  `RawTarget`, `ConfigEditError`
- `src/main.rs`: `run_target_command`, the startup after `load_config`,
  `command_exit_code`
- `tests/config.rs`
- `systemd/musicindex-live-publisher@.service` (the `--config` path)
- `docs/runbooks/musicindex-live-publisher-configuration.md` (the instance
  layout, the control CLI)

## Files Likely To Change

- `src/config.rs`
- `src/main.rs`
- `tests/config.rs`
- `docs/runbooks/musicindex-live-publisher-configuration.md`

## Do Not Touch

- The configuration format and the validation of one configuration.
- The exit codes 2 and 3 of `ConfigEditError`. Add no exit code.
- `target list`, `target remove`, `config show`, `provision`.
- The relay client, the watcher and the producer.
- `docs/adr/**`.

## Constraints

- **The instance layout.** A configuration path has an instance only when
  all of these are true:
  - its file name is `config.toml`,
  - its parent directory has a name, the instance name,
  - the parent of that directory, the root, has the name
    `musicindex-live-publisher`.

  Otherwise the check does not run. It reads no file and gives no error.
- **The other instances.** Each directory `<root>/<name>` with
  `<name>` different from the instance name, and with a file
  `<root>/<name>/config.toml`. Sort them by name, so an error is the same
  on each run.
- **The events of another instance.** Parse its file as a `toml::Table`.
  Take the string `event_id` of each table in the array `target`, with the
  spaces at the two ends removed. Do not use the full validation of
  `load_config`.
- **A file that does not parse.** Log `tracing::warn!` with the path, and
  skip that file. The error of a different instance must not stop this
  instance.
- **The comparison.** Compare event identifiers after the removal of the
  spaces at the two ends, with exact case.
- **The error.** It names ADR 0016, the event, the other instance, its
  configuration path and this instance. For example: `ADR 0016: event
  <id> belongs to instance <other> (<path>). Instance <this> cannot use it.
  Remove the target from one instance.`
- **`target add`.** The check runs before the file is written, also with
  `--replace`. A refused `target add` leaves the file unchanged and exits
  with code 1.
- **Startup.** The check runs directly after `load_config`, before the
  publisher contacts the relay or watches the drop directory.
- Put the check in `src/config.rs` as one public function that the two
  callers use. It reads only configuration files and writes nothing.
- No new dependency.

## Implementation Steps

1. Add the function that finds the instance layout of a path.
2. Add the function that reads the events of the other instances.
3. Add the public check, which takes the configuration path and the event
   identifiers of this instance.
4. Call it in `target add` and at startup.
5. Add the tests of §Acceptance Criteria.
6. In the configuration runbook, state the rule, the two errors, and the
   limit to one host. Name ADR 0016 as the owner.

## Acceptance Criteria

Each item is a test in `tests/config.rs` or in `src/config.rs`. A test makes
the layout `<tmp>/musicindex-live-publisher/<instance>/config.toml` with
`tempfile`.

- Instance `b` holds event `X`. `target add` in instance `a` with event `X`
  gives an error that names `b`, `a` and `X`. The file of `a` does not
  change.
- Instance `b` holds event `Y`. `target add` in instance `a` with event `X`
  succeeds.
- `target add --replace` of a target in instance `a` with its own event
  succeeds. The instance does not count itself.
- Instance `b` holds `" X "` with spaces. An add of `X` in `a` is refused.
- Instance `b` holds `x`. An add of `X` in `a` succeeds.
- The configuration of instance `b` does not parse. An add in `a` succeeds.
- A directory `c` with no `config.toml` is ignored.
- A configuration at `<tmp>/config.toml`, with no instance layout, gives no
  check and no error, also when `<tmp>/b/config.toml` holds the same event.
- The startup check of instance `a`, whose configuration holds `X`, gives an
  error that names `a` and `b` when `b` holds `X`.
- The existing tests of `target add` pass with no change to their
  assertions.

Also: the full gate passes.

## Test Commands

```bash
cargo fmt --all -- --check
cargo build --workspace
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
```

## Escalation Triggers

- An existing test of `target add` must change its assertion.
- The refusal seems to need a new exit code.
- The startup check needs the relay, or a state file.

## Prompt for lower-context coding model

You are implementing one bounded task from a larger plan.

Implement only this task. Do not redesign the architecture.

Read:
- docs/adr/0016-instances-events-and-producer-lock.md
- docs/plans/instances-and-producer-lock.md
- docs/tasks/instances-task-002-event-ownership-checks.md
- AGENTS.md
- src/config.rs, src/main.rs, tests/config.rs, systemd/musicindex-live-publisher@.service
- docs/runbooks/musicindex-live-publisher-configuration.md

Goal:
- target add refuses an event_id that a different instance holds, and the publisher exits at startup when a different instance holds one of its events. The error names the two instances.

Constraints:
- Follow §Constraints of the packet exactly. The check runs only for the layout <root named musicindex-live-publisher>/<instance>/config.toml. A file of another instance that does not parse gives a warning and is skipped. No new exit code. No new dependency.

Do not touch:
- the configuration format and its validation, the exit codes 2 and 3, target list, target remove, config show, provision, the relay client, the watcher, the producer, docs/adr/**

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
