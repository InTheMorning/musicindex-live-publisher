# Track Reporting Task 002: The Non-Payable File In The Connector Mode

Status: Implemented - 2026-10-18. It needs task 001. Ship it in one release
with task 001.

Every criterion is mechanical.

## Goal

In the connector mode, a row that is not payable links to the loudest playing
deck, as a V4V row does. The producer writes the non-payable file of task 001
while that deck plays, and removes it when the deck stops or loads a
different track.

## Files To Inspect

- `docs/adr/0014-one-assembler-and-track-reporting.md` (§Each Producer
  Reports Each Track)
- `docs/adr/0006-mixxx-midi-connector.md` (§How `mixxx-now-playing` Uses The
  Deck State, §A Resume Settles)
- `mixxx-now-playing/src/connector/link.rs`: `Coordinator::row`,
  `link_at_row`, `CurrentRow`, the deck-change handling, the resume settle,
  and the tests `non_v4v_row_ends_the_link` and
  `non_v4v_row_with_playing_loudest_deck_gives_display_track_and_no_payment_write`
- `mixxx-now-playing/src/main.rs`: `perform`, `write_current`
- The task 001 changes

## Files Likely To Change

- `mixxx-now-playing/src/connector/link.rs`
- `mixxx-now-playing/src/main.rs`, only if `write_current` needs the kind of
  the linked row

## Do Not Touch

- The history-only rules of task 001.
- The link rule, the resume settle of 2 seconds, and the new-load rule of
  ADR 0006. A non-payable row uses them with no change.
- `src/livevalue.rs`, the dead block, the display output, and
  `docs/adr/**`.

## Constraints

- A non-payable row links as a V4V row links: to the loudest deck that the
  mapping reported last, when that deck plays. No link gives no file.
- While the linked deck plays, the producer writes the non-payable file. When
  `play` of the linked deck becomes 0, it removes the file. A resume writes
  it again after the settle time. A new load ends the link and removes the
  file.
- The file duration comes from the linked deck, as for a V4V row.
- The two existing tests of a non-V4V row change their expected result: a
  non-V4V row with a playing loudest deck gives `Action::WriteFile`, not
  `Action::RemoveFile`. Rename each test so that its name states the new
  result.
- Each test of a V4V row keeps its result.
- A row that the producer cannot link gives `Action::RemoveFile`, for a
  V4V row and for a non-payable row.

## Implementation Steps

1. Task 001 gave `Row::Other` a duration and an expiry, and added a guard in
   `Coordinator::row`: `if self.mode != KnownMode::HistoryOnly` gives
   `RemoveFile`. Remove that guard. Give `Row::Other` the link and the file
   duration of a V4V row.
2. Keep the payable kind of the row in `CurrentRow`, if `write_current`
   needs it.
3. Change the two non-V4V tests, and add the tests of §Acceptance Criteria.

## Acceptance Criteria

Each item is a test in `connector/link.rs`, or a lifecycle test of the
producer:

- In the connector mode, a non-V4V row with a playing loudest deck gives
  `Action::WriteFile`, and the file has an empty `value_routes`.
- A stop of the linked deck gives `Action::RemoveFile`.
- A resume gives `Action::WriteFile` after the settle time, not before.
- A new load into the linked deck gives `Action::RemoveFile`.
- A non-V4V row with no playing loudest deck gives `Action::RemoveFile`.
- A V4V row followed by a non-V4V row on the same deck changes the file from
  payable to non-payable, with no removal between them.
- Each existing test of a V4V row passes with no change.

Also: the full gate passes.

## Test Commands

```bash
cargo fmt --all -- --check
cargo build --workspace
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
```

## Escalation Triggers

- A test of a V4V row must change to pass this task.
- The resume settle or the new-load rule needs a different time for a
  non-payable row.
- The change needs a new connector command or a change to the mapping.

## Prompt for lower-context coding model

You are implementing one bounded task from a larger plan.

Implement only this task. Do not redesign the architecture.

Read:
- docs/adr/0014-one-assembler-and-track-reporting.md
- docs/adr/0006-mixxx-midi-connector.md
- docs/tasks/track-reporting-task-002-connector-mode.md
- mixxx-now-playing/src/connector/link.rs, main.rs

Goal:
- In the connector mode, a non-payable row links to the loudest playing deck as a V4V row does. The producer writes the non-payable file while that deck plays and removes it when the deck stops or loads a different track.

Constraints:
- Follow §Constraints of the packet exactly. Each V4V test keeps its result.

Do not touch:
- the history-only rules of task 001, the link rule, the resume settle and the new-load rule of ADR 0006, src/livevalue.rs, the dead block, the display output, docs/adr/**

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

Reviewed 2026-10-18. The full gate passes with 500 tests. Only
`connector/link.rs` changed.

- `Coordinator::row` gives `Row::V4v` and `Row::Other` one path. A row that
  is not payable thus also uses the relink after an outage of ADR 0006.
- No test of a V4V row changed. Three tests of a non-V4V row changed their
  result and their name:
  - `non_v4v_row_ends_the_link` became
    `non_v4v_row_after_a_v4v_row_on_the_same_deck_writes_with_no_removal`.
  - `non_v4v_row_with_playing_loudest_deck_gives_display_track_and_no_payment_write`
    became
    `non_v4v_row_with_playing_loudest_deck_gives_display_track_and_a_payment_write`.
  - `non_v4v_row_after_a_v4v_row_removes_the_file_and_shows_the_new_track`
    became `non_v4v_row_after_a_v4v_row_writes_and_shows_the_new_track`.
- No new test was added. The changed tests assert each criterion: the write,
  the stop, the resume after 2 seconds, the new load, no link, and the change
  from a payable to a non-payable file.
- The lifecycle harness runs only the history-only mode. The content of the
  non-payable file does not depend on the mode, and the tests of task 001
  prove it.
- The review changed the comment in `Coordinator::row` to state the present
  rule only.
