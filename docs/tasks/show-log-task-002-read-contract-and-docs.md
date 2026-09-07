# Show Log Task 002: Read Contract And Documentation

## Goal

Give consumers a documented way to read the log, and document the operator
surface. This is the half `v4vmm` builds against.

## Files To Inspect

- `docs/adr/0003-show-log-contract.md`
- `docs/tasks/show-log-task-001-log-writer.md`
- `src/showlog.rs`
- `docs/architecture/broadcast-chain-boundaries.md`
- `docs/runbooks/musicindex-live-publisher-configuration.md`
- `README.md`

## Files Likely To Change

- `src/showlog.rs`
- `src/main.rs`
- `tests/showlog.rs`
- `README.md`
- `docs/architecture/broadcast-chain-boundaries.md`
- `docs/runbooks/musicindex-live-publisher-configuration.md`

## Do Not Touch

- The writer behavior from task 001
- The publish loop
- The drop-file contract

## Constraints

- A reader must tolerate a damaged last line. A crash during a show leaves a
  partial line, and that must not fail the whole read.
- A reader must ignore an unknown schema version, never guess it.
- **The supersede rule belongs in the reader**, so every consumer applies it the
  same way. The last entry for an `(event_guid, block_guid)` pair wins.
- The read helper is a convenience for a Rust consumer. The file itself stays
  the contract, because `v4vmm` may read it over `ssh` with no code from here.
- Do not add a show concept to the reader either. A caller passes a time range.

## Implementation Steps

1. Add `read_entries(path)` that returns the valid entries and the count of
   lines it skipped.
2. Add `entries_between(path, start, end)` that filters on `aired_at`.
3. Add `latest_for_blocks(entries)` that applies the supersede rule and returns
   one entry for each block, in `aired_at` order.
4. Add a `show-log` command that prints entries as JSON for a time range, so an
   operator and a remote consumer can read the log without a Rust dependency.
5. Add tests:
   - a damaged last line is skipped and the rest parse
   - an unknown schema version is skipped
   - a time range selects the right entries
   - the supersede rule keeps the last entry for a block
   - an empty file returns no entries and no error
6. Document the contract in `README.md`: the schema, the field list, both
   timestamps, and which one a consumer uses. An episode from a local encoder
   recording aligns to `observed_at`. `aired_at` describes live delivery only.
   State that the delay is an operator estimate and not a measurement.
7. Add the log to `docs/architecture/broadcast-chain-boundaries.md` as a
   produced contract, with `v4vmm` as the consumer.
8. Add the configuration values and the retention behavior to the configuration
   runbook, including where the file lives and what it holds.

## Acceptance Criteria

- A damaged last line does not fail a read.
- The supersede rule is applied in one place and tested.
- A consumer can read a time range without linking this crate.
- `README.md` documents the schema, both timestamps, and which one an episode
  uses.
- The boundaries document lists the log as a produced contract.

## Test Commands

- `cargo fmt --all -- --check`
- `cargo check --workspace --quiet`
- `cargo test --workspace --quiet`
- `cargo clippy --workspace --quiet -- -D warnings`
- `python3 /home/citizen/.claude/plugins/marketplaces/local/plugins/ste100/scripts/ste_lint.py README.md docs/architecture/broadcast-chain-boundaries.md`

## Expected Final Report Format

1. Files changed
2. Tests run
3. Behavior changed
4. Deviations from task
5. Unresolved concerns

## Escalation Triggers

- A consumer needs a field the writer does not record. That is a schema version
  change, not an addition.
- The time range filter is ambiguous for an entry that has no `aired_at`
  because the publish failed.

## Prompt for lower-context coding model

You are implementing one bounded task from a larger plan.

Implement only this task. Do not redesign the architecture.

Read:
- `docs/adr/0003-show-log-contract.md`, `src/showlog.rs`, `README.md`

Goal:
- Add a reader, a `show-log` command, and the documentation for the log
  contract.

Constraints:
- Tolerate a damaged last line. Ignore an unknown schema version.
- The supersede rule lives in the reader. Last entry for a block wins.
- The file is the contract. A consumer may read it without this crate.
- No show concept. The caller passes a time range.

Do not touch:
- the writer behavior, the publish loop, the drop-file contract

Acceptance criteria:
- Damaged last line tolerated, supersede rule tested, time range works.
- README documents the schema, both timestamps, and which one an episode uses.

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
