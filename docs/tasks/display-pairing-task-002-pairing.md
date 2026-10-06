# Display Pairing Task 002: The Pairing

Status: Ready after task 001, ADR 0010 task 001 and ADR 0011 task 002. Ship
it after relay ADR 0005 is deployed.

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
