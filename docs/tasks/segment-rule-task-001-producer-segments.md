# Segment Rule Task 001: The Producer Makes Its Segments From The Time Fields

Status: Implemented - 2026-11-27. ADR 0045 governs this task.

Each criterion is mechanical. ADR 0045 §Verification has the visual check, and this task does not own it.

## Goal

The station producer makes its segment starts with ADR 0045 §The Segment Rule. A chapter entry with good time fields
and a bad content field is a segment with no chapter. It is no longer part of a gap. The song or episode decision, the
route lookup, the `wpay` choice and `pub_date` do not change.

## Context

- ADR 0045 §The Segment Rule and §The Producer give each rule of this task. ADR 0041 §The Chapter List gives the
  content checks, which do not change.
- `liquidsoap-now-playing/src/chapters.rs`: `parse_chapters` gives the entries that are valid in all fields, in the
  order of `start_ms`, with one entry for each `start_ms`. `segment_at` selects the segment at a position. `parse_entry`
  checks the time fields and the content fields together.
- `liquidsoap-now-playing/src/lib.rs` uses the result of `parse_chapters` in two ways:
  - `chapters.is_empty()` decides if the state is a song state or an episode state (lines near 173, 207, 292 and 484).
    This decision must not change: it uses the entries that are valid in all fields.
  - `chapters::segment_at` gives the segment and the `play_id` suffix (`render_state`, near line 527). The segment must
    now come from the timed entries.
- The slot text already has the new rule (`liquidsoap/musicindex-state.liq`, function `musicindex_boundaries`). Do not
  change the slot text. The station check of 2026-11-27 passed all cases.
- `liquidsoap-now-playing/tests/segment-boundaries.json` has 28 cases. Each case has `name`, `chapters` (the raw text
  of the state field `chapters`), `boundaries` (the segment starts, with 0) and `suffix` (true when the file has chapter
  segments). Do not change the cases. If a case looks wrong, stop and report it.

## The Rule

### The Timed Entries

- A timed entry is a JSON object with `start_ms` and `end_ms` that `Value::as_i64` gives, from 0 to
  4_611_686_018_427_387_903 (2^62-1), and `end_ms` above `start_ms`. Put the limit in one named constant with a
  comment that cites ADR 0045 §The Segment Rule.
- Keep the first timed entry for each `start_ms` in the list order. Then sort by `start_ms`. The result is the same as
  a stable sort and then the removal of the second entry with the same start.
- Each timed entry gets the content check of ADR 0041 §The Chapter List. The result is `Option<Chapter>`: `None` when a
  content field is not valid. Use the present `parse_entry` checks for the content fields. Do not make a second copy of
  them.
- The error log does not change. The producer writes one error for each `seq` when it refuses an entry by its times
  or by its content. It also writes one when `chapters` is not a JSON array.

### The Data Shape

- `parse_chapters` gives one type that holds the timed entries, each with its `Option<Chapter>`. Select the name. For
  example: `ChapterList { spans: Vec<Span> }` and `Span { start_ms, end_ms, chapter: Option<Chapter> }`.
- The type gives the entries that are valid in all fields, for the song or episode decision and for the lookup pairs.
  It also tells if the file has chapter segments (one or more timed entries).
- `segment_at` reads the timed entries. A segment of a timed entry with no chapter has `chapter: None` and the
  `start_ms` of that entry. The gap rules do not change, but they use the timed entries.

### The Callers

- Each `chapters.is_empty()` in `lib.rs` becomes "no entry is valid in all fields". The behavior of these callers does
  not change.
- `render_state` and the `wpay` choice use `segment_at` on the timed entries. A segment with `chapter: None` uses the
  present rules for a segment with no chapter.
- The `play_id` suffix comes when `segment_at` gives a segment, as now. `segment_at` gives no segment when the file has
  no timed entry.

## Base components

| Component | Status | Location | Reason |
| --- | --- | --- | --- |
| Chapter parse and segment selection | extend | /home/citizen/build/musicindex-live-publisher/liquidsoap-now-playing/src/chapters.rs | The timed entries and the content check use the same parse. |
| Producer state render | extend | /home/citizen/build/musicindex-live-publisher/liquidsoap-now-playing/src/lib.rs | The callers use the new data shape. |
| Shared segment cases | reuse | /home/citizen/build/musicindex-live-publisher/liquidsoap-now-playing/tests/segment-boundaries.json | The producer test and the slot text check read the same cases. |

## Files To Inspect

- `docs/adr/0045-one-segment-rule-for-the-frames-and-the-drop-files.md`
- `docs/adr/0041-the-station-producer.md`, §The Chapter List, §The Position Rule and §What The Producer Writes For An
  Episode File
- `liquidsoap-now-playing/src/chapters.rs`
- `liquidsoap-now-playing/src/lib.rs`
- `liquidsoap-now-playing/tests/segment-boundaries.json`
- `liquidsoap/musicindex-state.liq`, function `musicindex_boundaries` (read only)

## Files Likely To Change

- `liquidsoap-now-playing/src/chapters.rs`
- `liquidsoap-now-playing/src/lib.rs`

## Do Not Touch

- `liquidsoap/musicindex-state.liq`
- `liquidsoap-now-playing/tests/segment-boundaries.json`
- `scripts/check-slot-boundaries.sh`
- Each ADR, `AGENTS.md` and `docs/README.md`
- Each crate other than `liquidsoap-now-playing`

## Acceptance Criteria

Each criterion is mechanical.

1. A test reads `tests/segment-boundaries.json` with `include_str!`. For each case it calls `parse_chapters` on a state
   with that `chapters` text. It calls `segment_at` at each candidate position: 0, and each integer `start_ms` and
   `end_ms` from 0 to 2^62-1 in the raw JSON. The set of the `start_ms` values of the result, sorted, equals
   `boundaries`. "The file has chapter segments" equals `suffix`.

   A failure message names the case and cites ADR 0045 §The Segment Rule.
2. A test: an entry with good times and `"title": 5` between two valid chapters gives a segment with `chapter: None`
   at its `start_ms`. The drop file of that segment has the episode GUIDs, the episode routes, the state title and the
   `play_id` suffix of its `start_ms`.
3. A test: the overlap of episode 158 (`6498968` to `6729868`, then `6724607` to `7028534`). At position 6729868 the
   `play_id` ends with `-6724607`.
4. A test: a file where each timed entry has a bad content field is still a song state for the lookup pairs and the
   `wpay` choice, as before. Its `play_id` has the suffix of the segment.
5. Each test that passed before passes. If a test asserts the old rule (a refused entry is part of a gap), change it to
   the rule of ADR 0045, and name it in the report.
6. `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets -- -D warnings` and `cargo test --workspace`
   pass.

## Rules For The Delegate

- Write each code comment in Simplified Technical English. Read `/home/citizen/.agents/skills/asd-ste100/SKILL.md`.
- Do not commit. Leave the change in the working tree, and report the files that you changed and the check results.
- Do not make an architecture decision. If a rule of this task does not fit the code, stop and report.
