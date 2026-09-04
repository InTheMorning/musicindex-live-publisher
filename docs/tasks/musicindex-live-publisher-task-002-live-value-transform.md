# Task 002 — Live value transform and payload assembly

Part of [MusicIndex live publisher plan](../plans/musicindex-live-publisher-plan.md), Phase 1.

## Goal

Turn a parsed drop file into a Podcasting 2.0 live value payload matching the
shape real listening apps consume. Pure functions. No I/O, no network.

This is the substance of the project. Get it exactly right.

## Files To Inspect

- `docs/plans/musicindex-live-publisher-plan.md` — Transform, Verified payload semantics
- `~/build/splitkit/hgh-example-2.json` and `hgh-example-3.json` — the two `type: "music"` reference payloads
- `~/build/splitkit/README.md` — the wrapped-versus-direct discrimination rule
- Task 001's `DropFile`

## Files Likely To Change

- `src/livevalue.rs` (new)
- `tests/golden.rs` (new)

## Do Not Touch

- Task 001's drop-file schema
- `~/build/splitkit/hgh-example-*.json` — read as fixtures, never edit

## Constraints

- **`split` is emitted as a decimal string, not a number.** Verified in all three
  reference payloads: `"49"`, `"0.49"`, `"49.51"`. `90.0` must render `"90"`, not
  `"90.0"` and not `90`.
- **`duration` is seconds as a float** for music blocks — `187.326`, `213.875`.
  Do not convert to milliseconds. Example 1's `316800` is a `chapter` block with
  different semantics; **do not use example 1 as a reference.**
- `startTime` is `0` for music blocks. No stream-elapsed clock exists.
- `eventGuid` is the live item's `event_id` and is stable across tracks.
  `blockGuid` is a fresh UUID v4 per drop file.
- Omit `customKey`, `customValue` and `fee` when the source value is null. Do not
  emit `null`.
- **The assembled payload must never have exactly the two keys `event_id` and
  `metadata`.** The relay treats that exact key set as the wrapped form and would
  route it down the wrong path.
- Output field names are camelCase (`customKey`, `blockGuid`, `startTime`) while
  the input is snake_case. Do not apply a blanket rename rule to both types.

## Implementation Steps

1. Implement the destination mapping:

   | Drop file (`PaymentRoute`) | Live value destination |
   |---|---|
   | `recipient_name` | `name` |
   | `route_type` | `type` |
   | `address` | `address` |
   | `split` (f64) | `split` (decimal **string**) |
   | `custom_key` | `customKey`, omitted when null |
   | `custom_value` | `customValue`, omitted when null |
   | `fee` | `fee`, omitted when null |

2. Implement split formatting: shortest decimal string that round-trips. `90.0`
   to `"90"`, `0.49` to `"0.49"`, `49.51` to `"49.51"`.
3. Assemble the block:

   ```json
   {
     "title": "<title>",
     "image": "<omitted when absent>",
     "description": "",
     "type": "music",
     "startTime": 0,
     "duration": 187.326,
     "eventGuid": "<event_id>",
     "blockGuid": "<uuid v4>",
     "feedGuid": "<feed_guid, omitted when absent>",
     "itemGuid": "<track_guid, omitted when absent>",
     "value": {
       "model": { "type": "lightning", "method": "keysend" },
       "destinations": [ ... ]
     }
   }
   ```

4. Take `blockGuid` as a parameter rather than generating it inside the transform,
   so tests are deterministic and task 003 can reuse one guid across a republish.
5. Provide a `fallback_payload(...)` that builds the same shape from configured
   station destinations, for task 003's clear path.

## Acceptance Criteria

- Golden test: a drop file derived from `hgh-example-2.json` assembles to a
  payload with the same key set, types, and destination shape. Same for
  `hgh-example-3.json`.
- `split` renders as a string in every case; a test asserts `"0.49"` and
  explicitly rejects `0.49`.
- Null `custom_key` produces no `customKey` key at all, not `"customKey": null`.
- Empty `value_routes` produces `destinations: []`, not a missing `value` block.
- A test asserts the payload's top-level key set is never exactly
  `{event_id, metadata}`.
- `duration` is absent when the drop file has no `duration_secs`, rather than 0.
- `feed_guid` and `track_guid` are emitted as `feedGuid` and `itemGuid` when
  present, and omitted when absent.
- Two calls with different `blockGuid` produce payloads differing only in that
  field.

## Test Commands

```bash
cd musicindex-live-publisher
cargo clippy -- -D warnings
cargo test livevalue
cargo test --test golden
```

## Expected Final Report

- Files created
- The assembled payload for each of the two music examples, pasted verbatim
- A diff of assembled output against the reference example, with any intentional
  difference explained
- Test names and results

## Escalation Triggers

Stop and ask before proceeding if:

- A reference example has a field the transform cannot source from a drop file.
- The two music examples disagree on a field's type or units.
- Split values in a real drop file do not sum to 100 — report it, do not
  normalise silently.
