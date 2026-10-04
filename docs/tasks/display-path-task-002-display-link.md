# Display Path Task 002: The Display Link And The Display State

Status: Implemented - 2026-10-04. See §Review Result.

Every criterion is mechanical.

## Goal

The `Coordinator` keeps a display link for every history row, V4V or not,
next to the payment link. It gives a display state: a track or `null`. The
song file follows the display state.

## Files To Inspect

- `docs/adr/0008-display-path.md` (§The Display Link, §The Display State,
  §The Song File For `butt`)
- `docs/adr/0006-mixxx-midi-connector.md` (§How `mixxx-now-playing` Uses The
  Deck State, §Entering And Leaving The Connector Mode, §Relink After An
  Outage)
- `mixxx-now-playing/src/connector/link.rs` (all of it)
- `mixxx-now-playing/src/main.rs` (`perform`, `process_track`)

## Files Likely To Change

- `mixxx-now-playing/src/connector/link.rs`
- `mixxx-now-playing/src/main.rs`

## Do Not Touch

- The payment link rules and their tests in `link.rs`. Every present test must
  pass with no change.
- `mixxx-now-playing/src/connector/state.rs`, `midi.rs`, `device.rs`,
  `command.rs`
- `src/**` (the publisher)
- `docs/adr/**`

## Constraints

- The display link has the same rules as the payment link: the loudest deck
  at the row, the stop, the resume, the new load, the entry into the connector
  mode, the relink after an outage. It applies to `Row::Other` too.
- Put the display link in the `Coordinator` next to the payment link. Share
  the rule code with the payment link where you can, so the two cannot drift.
- `Row::Other` must carry the artist and the title for the display state.
  Change the row type as needed, without a change to the payment decisions.
- The `Coordinator` gives a display state change as a new action, for example
  `Action::Display(DisplayState)`, with `DisplayState::Track { artist, title,
  v4v }` or `DisplayState::Null`. `v4v` says if the row is a V4V row, so task
  003 can choose the artwork source.
- The display state is `Null` at startup, while no display link exists, while
  the linked deck does not play, and when the producer stops.
- In the history-only mode, the display state is the latest row until the next
  row.
- `main.rs` writes `now-playing.txt` from the display state: the title line of
  the row for a track, no text for `Null`. It keeps the present line format.
- With `--no-connector`, the display state is the latest row, as in the
  history-only mode. The song file then behaves as today, except task 001.

## Implementation Steps

1. Add the display link and the display state to the `Coordinator`.
2. Add the display action.
3. Change `main.rs` so the song file follows the display action.
4. Add the tests.

## Acceptance Criteria

Each item is a `Coordinator` test:

- A non-V4V row with a playing loudest deck gives a display track and no
  payment write.
- The linked deck of a non-V4V row stops: display `Null`. It starts again:
  the display track again.
- A new load on the display-linked deck gives display `Null`.
- An entry into the connector mode gives display `Null`, and a prior row
  never gets a display link, except by the relink rule.
- A relink after an outage restores the display track of the row.
- A V4V row gives both a payment write and a display track. A stop of its
  deck gives both a remove and display `Null`.
- Every present payment test passes with no change.
- Each display rule was broken on purpose, and a test failed. The report lists
  each mutation.

Also: the full gate passes.

## Test Commands

```bash
cargo fmt --all -- --check
cargo build --workspace
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
```

## Escalation Triggers

- The display link cannot share the rule code without a change to a payment
  test.
- A rule here conflicts with ADR 0006 or ADR 0008.

## Prompt for lower-context coding model

Implement only this task. Read docs/adr/0008-display-path.md, ADR 0006
§How mixxx-now-playing Uses The Deck State, §Entering And Leaving The
Connector Mode and §Relink After An Outage, this packet,
mixxx-now-playing/src/connector/link.rs and mixxx-now-playing/src/main.rs.
Add a display link for every row, with the same rules as the payment link, and
a display state action. The song file follows the display state. Do not change
any payment decision or payment test. Write the tests in §Acceptance Criteria,
with a mutation list. Run the test commands. Report: 1. files changed 2. tests
run 3. behavior changed 4. deviations 5. unresolved concerns 6. mutations.

## Review Result

Reviewed 2026-10-04. The review changed no code. The test module of
`link.rs` has only additions, so no payment test changed. Each display rule
was broken on purpose, and a test failed each time. A payment mutation also
made the present payment tests fail.

The payment link and the display link share the rule code: `RowLink`,
`LinkEffect`, `link_at_row` and `relink_refusal`.

The review accepts two deviations:

- A display change is not an `Action`. The present payment tests compare full
  action lists, and they must not change. The `Coordinator` gives
  `take_display_change()`, and `main.rs` reads it after each call.
- `Row::Other` carries no text. The artist and the title go to
  `history_row(row, DisplayTrack)`.

Behavior to note: when the producer leaves the connector mode, the display
state does not change. A title that shows stays until the next history row,
because the history-only mode has no stop. The rule `Null` gives an empty song
file has no test in `main.rs`. The `Coordinator` tests and manual check 1 of
the review checklist cover it.
