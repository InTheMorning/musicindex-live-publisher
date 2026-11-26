# Segment Frame Task 001: The Display State Gets `playId`

Status: Implemented - 2026-11-26. It runs on the test station (station log step 52).

Every criterion is mechanical. ADR 0038 has the visual items.

## Goal

The publisher adds `playId` to the track of each display state that it sends to the relay. The value is the
`play_id` of `display.json`, or `null`. ADR 0038 §The Display State Gets `playId` owns the rule. ADR 0039 §The
Publisher owns the other fields of the track. Read the two records first.

Deploy order: the relay with relay ADR 0012 must run on `api.musicindex.org` before a publisher with this change runs
on a station. The present relay refuses each track with `playId`, also with `null`. The operator does the deploy.

## Files To Read

- `docs/adr/0038-the-hls-stream-marks-each-segment.md` §The Display State Gets `playId`
- `docs/adr/0039-the-display-state-pairs-with-its-value-block.md` §The Publisher
- `src/display.rs`: `DisplayState::body_with_pairing` near line 203, `parse_track` near line 290 to 375
- `tests/display.rs`: the full body assertions near lines 415 and 1312, the missing ID and `null` track cases near
  lines 1382 and 1409, the helpers `paired_track`, `paired_payload`, `wait_for_paired_display` and
  `write_display_json`

## Files To Change

- `src/display.rs`
- `tests/display.rs`

## Base components

| Component | Status | Location | Reason |
| --- | --- | --- | --- |
| Track body of the display state | extend | src/display.rs | `body_with_pairing` already builds the track with `songLine` and `value`. It gets one more key. |
| Parse of `display.json` | reuse | src/display.rs | `parse_track` already keeps `play_id` as `Option<String>`. |
| Display tests and helpers | extend | tests/display.rs | The full body tests get the new key. The helpers write `display.json` with and without `play_id`. |

## Rules For The Implementer

- The key on the wire is `playId`. It is in each track object, also when the value is `null`.
- `playId` is `null` when `display.json` has no `play_id`, has `null`, or has an empty string. The publisher never
  sends an empty string.
- Choice of the main session, open for the operator: when `play_id` has more than 128 characters, the publisher
  sends `null` and logs a warning each time that it builds that body. Relay ADR 0012 refuses a longer value, and the relay then
  refuses the complete display state. The producers make values shorter than 40 characters.
- A `null` track (no track) does not change: the body stays `{"track": null}`.
- The pairing rule of ADR 0039 does not change. `value` is still added only when `play_id` agrees with the kept
  `play_id` of the target.
- Do not change the producers, the drop file or `display.json`.

## Mechanical Criteria

Each item is a test in `tests/display.rs`, or a change of a present test:

1. `display_body_has_play_id_of_display_json`: a `display.json` track with `play_id` `ls-12500-4-30665` gives a body
   track with `playId` `ls-12500-4-30665`.
2. `display_body_has_null_play_id_for_a_jingle`: a track with `play_id` `null`, a track with no `play_id`, and a track
   with `play_id` `""` each give `playId` `null`. The key is present in each body.
3. `display_body_null_play_id_when_too_long`: a `play_id` of 129 characters gives `playId` `null`. A `play_id` of 128
   characters is sent with no change.
4. The present full body assertions near lines 415 and 1312 include `playId`. Each other present test passes with
   no other change.
5. `paired_display_has_play_id_and_value`: a paired display state has `playId` and `value`, and the `playId` is the
   kept `play_id` of the pairing.
6. The body of a `null` track is `{"track": null}`.

Gate:

7. `cargo build && cargo test`, `cargo clippy --all-targets -- -D warnings` and `cargo fmt -- --check` pass.

## Rules For A Delegate

- Do not change `AGENTS.md`. The main session changes it when it commits the task.
- Apply the STE skill at `/home/citizen/.agents/skills/asd-ste100/SKILL.md` to code comments.
- Do not commit. Leave the change in the working tree and report it. The main session reads the diff, runs the
  checks and commits.
