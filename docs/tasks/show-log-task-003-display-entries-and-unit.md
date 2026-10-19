# Show Log Task 003: The Display Entries, The Unit And The Documents

Status: Implemented - 2026-10-19. It needs task 002. The manual check is open.

Every criterion is mechanical, except one visual item in §Manual Check.

## Goal

The publisher writes a `display` entry for each display state that it sends.
The template unit gives each instance a state directory and passes
`--state-dir`. The retention is a configuration value. The documents describe
the log.

## Files To Inspect

- `docs/adr/0017-show-log.md` (the amendment, §Entries, the `display` entry)
- `src/showlog.rs`, and the task 002 changes in `src/main.rs`
- `src/main.rs`: `DisplayPath`, `startup_items`, `items_for_dir`,
  `changed_entries`, `emit_items`
- `src/display.rs`: `DisplayState`, `DisplayTrack`, `Artwork`
- `src/config.rs`: `RawConfig`, `PublisherConfig`, `load_config_bytes`
- `systemd/musicindex-live-publisher@.service`
- `README.md`, `docs/runbooks/musicindex-live-publisher-configuration.md`

## Files Likely To Change

- `src/main.rs`
- `src/config.rs`
- `systemd/musicindex-live-publisher@.service`
- `tests/showlog.rs`
- `tests/config.rs`
- `README.md`
- `docs/runbooks/musicindex-live-publisher-configuration.md`

## Do Not Touch

- The display publish rules, the pairing of ADR 0012, and the display
  worker.
- The `track` and `clear` entries of task 002.
- `mixxx-now-playing/**`, `Cargo.toml`, `docs/adr/**`.

## Constraints

- **The `display` entry.** Write it in the main loop, at the point where a
  changed display entry becomes an `EmitItem::Display`. Use the same changed
  entries that the loop sends, so an unchanged `display.json` gives no entry.
  `observed_at` is `SystemTime::now()` after the read of `display.json`.
- **Its fields.** `target` and `event_guid` of the entry. From the display
  track: `play_id`, `artist`, `title`, `album`, `song_line`, and `artwork` as
  `{"url": …}` or `{"sha256": …, "mime": …}`. A state with no track gives a
  `display` entry with none of these fields.
- **A startup state** and the `null` state of a missing producer also give a
  `display` entry, because the loop sends them.
- **The write never blocks the send.** The entry is written before or after
  the send, and a log failure only warns.
- **The retention.** A top-level configuration key
  `show_log_retention_days`, an integer of 1 or more, default 365. A value of
  0 or less is a configuration error that names the key. `--state-dir` uses
  it. `config show` prints it.
- **The unit.** Add `StateDirectory=musicindex-live-publisher/%i` and
  `StateDirectoryMode=0700`. Add `--state-dir %S/musicindex-live-publisher/%i`
  to `ExecStart`. Keep `ProtectHome=read-only`.
- **The documents.** `README.md` and the configuration runbook get a short
  section on the show log. It gives these facts:
  - the directory, and one file for each UTC day
  - the retention
  - the three kinds of entry
  - ADR 0017 owns the contract
  - the log holds the artist and title of each track that the operator
    played

## Implementation Steps

1. Write the `display` entry in the main loop.
2. Add `show_log_retention_days` to the configuration.
3. Change the unit.
4. Change the documents.
5. Add the tests of §Acceptance Criteria.

## Acceptance Criteria

Each item is a test:

- A new `display.json` with an embedded image gives one `display` entry with
  `play_id`, `song_line` and `artwork` with `sha256` and `mime`.
- A `display.json` with an image URL gives `artwork` with `url`.
- A `{"track": null}` state gives a `display` entry with no `artist`,
  `title` or `artwork` key.
- A second write of the same `display.json` gives no second entry.
- `show_log_retention_days = 0` is a configuration error that names the key.
- A configuration with no `show_log_retention_days` gives 365.
- A test reads `systemd/musicindex-live-publisher@.service` and finds
  `StateDirectory=musicindex-live-publisher/%i` and
  `--state-dir %S/musicindex-live-publisher/%i`.

Also: the full gate passes.

## Manual Check

Visual. The operator runs one show with the unit and checks that
`~/.local/state/musicindex-live-publisher/mixxx/showlog/` holds a file of the
day with `track`, `clear` and `display` entries. Report this item as open
until the operator runs it.

## Test Commands

```bash
cargo fmt --all -- --check
cargo build --workspace
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
```

## Escalation Triggers

- A `display` entry needs a change to the display worker.
- `ProtectHome=read-only` stops the write to the state directory.
- The test of the unit file seems to need a systemd tool.

## Prompt for lower-context coding model

You are implementing one bounded task from a larger plan.

Implement only this task. Do not redesign the architecture.

Read:
- docs/adr/0017-show-log.md
- docs/plans/show-log.md
- docs/tasks/show-log-task-003-display-entries-and-unit.md
- AGENTS.md
- src/showlog.rs, src/main.rs, src/display.rs, src/config.rs, systemd/musicindex-live-publisher@.service
- README.md, docs/runbooks/musicindex-live-publisher-configuration.md

Goal:
- The publisher writes a display entry for each display state that it sends. The template unit gives each instance a state directory and passes --state-dir. The retention is a configuration value. The documents describe the log.

Constraints:
- Follow §Constraints of the packet exactly. An unchanged display.json gives no entry. A log failure never blocks a send. Keep ProtectHome=read-only.

Do not touch:
- the display publish rules, the pairing of ADR 0012, the display worker, the track and clear entries, mixxx-now-playing/**, Cargo.toml, docs/adr/**

Acceptance criteria:
- Each item in §Acceptance Criteria of the packet is a passing test. Report the manual check as open.

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

Reviewed 2026-10-19. The full gate passes with 545 tests in the workspace.
Task 002 ended with 537. `Cargo.toml`, `Cargo.lock` and `tests/golden.rs` did
not change.

- `changed_entries` is the only place where a changed display entry becomes
  an `EmitItem::Display`. The `display` entry is written there, so an
  unchanged `display.json` gives no entry, and the startup state and the
  `null` state of a missing producer give one.
- The read time comes after the read of `display.json`.
- A map from the event to the target name gives `target`, because one
  display directory can serve more than one target.
- `show_log_retention_days` replaces the constant of task 002. A value of 0
  or less is a configuration error that names the key. `config show` prints
  it.
- The unit has `StateDirectory=musicindex-live-publisher/%i`,
  `StateDirectoryMode=0700` and `--state-dir %S/musicindex-live-publisher/%i`.
  `ProtectHome=read-only` did not change.
- The agent also added `--state-dir` to the CLI options of the runbook.
- No defect was found.
- The manual check is open until the operator runs a show with the unit.
