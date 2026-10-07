# Display Pairing Task 002: The Pairing

Status: Implemented - 2026-10-06. Ship it after relay ADR 0005 is deployed.

Every criterion is mechanical.

## Goal

The display body carries `songLine`. The display state of a V4V track also
carries `value {eventGuid, blockGuid}` of the payload of the same play. A
display state that goes out before its payload goes out again, one time,
with `value`.

## Files To Inspect

- `docs/adr/0012-pair-display-state-with-value-block.md` (§The Publisher,
  §Invariants)
- `musicindex-live-relay`: `docs/adr/0005-display-song-line-and-value.md`
- `docs/tasks/display-pairing-task-001-display-json-v2.md`
- `src/livevalue.rs`: `LiveValuePayload`, `payload_from_dropfile`,
  `dead_payload`
- `src/display.rs`: `DisplayState::body`, `DisplayEntry`, `DisplayPath`
- `src/main.rs`: `emit_items` and the code that sends payloads and display
  entries
- `tests/display.rs`, `tests/relay.rs`

## Files Likely To Change

- `src/livevalue.rs`
- `src/display.rs`
- `src/main.rs`
- `tests/display.rs`
- `README.md`

## Do Not Touch

- The JSON of the live value payload
- The image upload rules, also the rule that one image is uploaded one time
- `src/relay.rs` except for the display entry type, if it must change
- `mixxx-now-playing/`
- `docs/adr/**`

## Constraints

- `LiveValuePayload` gets `play_id: Option<String>` with `#[serde(skip)]`.
  `payload_from_dropfile` copies it from the drop file. `dead_payload` sets
  `None`. The payload JSON does not change.
- For each target, keep the `play_id`, the `eventGuid` and the `blockGuid` of
  the newest track payload that went out. A dead block clears it.
- `DisplayState::body` adds `songLine` from `song_line`. It adds
  `value: {"eventGuid": …, "blockGuid": …}` only when the `play_id` of the
  track agrees with the kept `play_id` of the target.
- A track payload can go out after the display state of the same `play_id`,
  when that state went out with no `value`. Then send that display state
  again, one time, with `value`. Do not upload its image again.
- A `null` display state has no `songLine` and no `value`.
- Never build `value` from the artist or the title.

## Implementation Steps

1. Add `play_id` to the payload and fill it.
2. Keep the pairing for each target where payloads go out.
3. Change `DisplayState::body`.
4. Add the second send.
5. Add the tests.
6. Update the display body example in `README.md`.

## Acceptance Criteria

Each item is a test:

- A display state after its payload has `songLine` and `value` with the
  `eventGuid` and the `blockGuid` of that payload.
- A display state before its payload goes out without `value`. When the
  payload goes out, the same state goes out again, one time, with `value`.
  Its image is not uploaded again.
- A display state of a track with no drop file has `songLine` and no
  `value`.
- After a dead block, a display state has no `value`.
- Two plays of the same track: the second display state names the second
  block, not the first.
- The payload JSON is equal to its JSON before this task. The golden test
  passes with no change.

Also: the full gate passes.

## Test Commands

```bash
cargo fmt --all -- --check
cargo build --workspace
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
```

## Escalation Triggers

- Payloads and display entries do not pass one place where the pairing can
  see both in order.
- ADR 0011 task 002 is not done, so a payload or a display state still waits
  in a schedule.

## Prompt for lower-context coding model

You are implementing one bounded task from a larger plan.

Implement only this task. Do not redesign the architecture.

Read:
- docs/adr/0012-pair-display-state-with-value-block.md
- ../musicindex-live-relay/docs/adr/0005-display-song-line-and-value.md
- docs/tasks/display-pairing-task-002-pairing.md
- src/livevalue.rs, src/display.rs, src/main.rs
- tests/display.rs, tests/relay.rs
- README.md

Goal:
- Add songLine to each display body.
- Add value to the display state of the same play as the newest track payload.
- Send a display state again one time when its payload follows it.

Constraints:
- Follow §Constraints of the packet exactly. Never build value from the artist or the title.

Do not touch:
- The payload JSON, the image upload rules, src/relay.rs except the display entry type, mixxx-now-playing/, docs/adr/**

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

Reviewed 2026-10-06. `Cargo.lock` did not change. The full gate passes with
480 tests. The six pairing tests passed in five more runs.

The first implementation failed the review:

- The resend of a display state that went out before its payload never
  occurred. After a send, `pending` is empty, and the rule needed `pending`
  to equal `last_sent`.
- A dead block kept its `eventGuid` and `blockGuid`.
- The tests examined only the body helper, with literal values. No test
  drove the workers.

The second implementation corrected the resend and the dead block. Then the
review found one more defect and corrected it. `last_sent_paired` was true
when any pairing existed, not only when the sent body had `value`. A display
state of a second play that went out before its payload was then never sent
again. `DisplayState::pairs_with` now holds the pairing rule. The body and the
resend both use it.

The review wrote these worker tests again or added them. Each one sends
through the real workers to the stub relay:

- `pairing_display_after_payload_includes_value`
- `pairing_display_before_payload_resends_with_value`
- `pairing_two_plays_of_one_track_name_the_second_block`
- `pairing_second_play_before_its_payload_resends_with_the_second_block`

Two mutation checks show that the tests find the defects. The test of the
resend fails when the resend line is removed. The test of the second play
fails with the earlier `last_sent_paired` rule.

The configuration runbook gives the display body with `songLine` and `value`.
`README.md` does not describe `display.json`.

Live check, 2026-10-06, on `api.musicindex.org` with package `r83`. Mixxx
had "Track duplicate distance" 0. A new load of the same track gave a new
history row and the block `b3790cee`. The first play had the block
`26be808f`. The display state of the second play named `b3790cee` in the
same poll as the block.
