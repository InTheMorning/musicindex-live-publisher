# Reference Shape Task 002: The Payload Fields

Status: Implemented - 2026-10-06. Ship it in one release with task 001.

The acceptance criteria are mechanical. The visual checks are in a separate
list.

## Goal

The payload of a V4V track carries `line`, `author`, and when known
`podcastName`. Each field that the payload sends today keeps its
name, its form and its meaning.

## Files To Inspect

- `docs/adr/0010-live-value-payload-reference-shape.md` (§The Payload For A
  V4V Track, §Invariants)
- `docs/tasks/reference-shape-task-001-dropfile-v2.md`
- `src/livevalue.rs`: `LiveValuePayload`, `payload_from_dropfile`,
  `dead_payload`, the tests
- `src/dropfile.rs`
- `tests/golden.rs` and its fixtures

## Files Likely To Change

- `src/livevalue.rs`
- `tests/golden.rs` and its fixtures
- `README.md` (the payload example)

## Do Not Touch

- `dead_payload` and the dead block constants
- `src/relay.rs`, `src/main.rs`, `src/schedule.rs`
- The `value` form: it keeps the nested `model`
- `mixxx-now-playing/`
- `docs/adr/**`

## Constraints

- Add to `LiveValuePayload`, each skipped in the JSON when it is `None`:
  - `line: Option<Vec<String>>`
  - `author: Option<String>`
  - `podcast_name: Option<String>`, serialized as `podcastName`
- Add no `link` field. ADR 0010 decided that `link` stays absent.
- `payload_from_dropfile` sets:
  - `line` to `[album, artist]` when the drop file has an album that is not
    empty, else `[title, artist]`
  - `author` to the artist
  - `podcast_name` to the album, only when it is not empty
- `dead_payload` sets each new field to `None`. The dead block JSON does not
  change.
- The golden test builds its drop files from the CurioHoster payloads
  `tests/fixtures/hgh-example-2.json` and `hgh-example-3.json`. Set `album`
  from `line[0]` of each payload. Then compare `line` of the publisher payload
  with `line` of the reference payload.
- The existing test that a payload never has exactly the keys `event_id` and
  `metadata` stays.

## Implementation Steps

1. Add the three fields.
2. Fill them in `payload_from_dropfile`.
3. Set them to `None` in `dead_payload`.
4. Add the tests, and update the golden fixtures.
5. Update the payload example in `README.md`.

## Acceptance Criteria

Mechanical. Each item is a test:

- A drop file with an album gives `line: [album, artist]`, `author` and
  `podcastName`.
- A drop file with no album gives `line: [title, artist]`, `author`, and no
  `podcastName` key.
- No payload has a `link` key.
- The dead block JSON is equal to its JSON before this task.
- Each field from before this task keeps its name and its value in the
  golden test.
- For each CurioHoster reference payload, the publisher payload has the same
  `line`.

Also: the full gate passes.

Visual. A person must examine these items in a podcast app. Report each one
as open until a person completes the check.

- An app that shows `line` shows the album and the artist of this stream.
- The artwork from `image` changes with the track, as before.

## Test Commands

```bash
cargo fmt --all -- --check
cargo build --workspace
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
```

## Escalation Triggers

- A golden fixture changes in a field that existed before this task.
- The dead block JSON changes.

## Prompt for lower-context coding model

You are implementing one bounded task from a larger plan.

Implement only this task. Do not redesign the architecture.

Read:
- docs/adr/0010-live-value-payload-reference-shape.md
- docs/tasks/reference-shape-task-002-payload-fields.md
- src/livevalue.rs
- src/dropfile.rs
- tests/golden.rs and its fixtures
- README.md

Goal:
- A V4V track payload carries line, author, and when known podcastName. It has no link. The fields from before keep their names, forms and meanings. The dead block does not change.

Constraints:
- Follow §Constraints of the packet exactly.

Do not touch:
- dead_payload and its constants, src/relay.rs, src/main.rs, src/schedule.rs, the value form, mixxx-now-playing/, docs/adr/**

Acceptance criteria:
- Each mechanical item in §Acceptance Criteria of the packet is a passing test.

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
465 tests.

The review made these changes:

- `payload_from_dropfile` gets the album one time and uses it for `line` and
  `podcastName`.
- `livevalue_dead_payload_has_no_new_fields` compares the full key set of the
  dead block, not only the three new keys.
- The `README.md` example gives the keys in the order that the publisher
  sends them. The text names ADR 0010 and the dead block.
- The `clippy::large_enum_variant` exception on `EmitItem` has a reason.

The golden test changed as §Constraints says: `album` comes from `line[0]`,
and `line` is compared with the reference. The two CurioHoster references
have no `author` and no `podcastName`. The test thus compares those two
fields with values from `line`, not with a reference value.

The visual items stay open.
