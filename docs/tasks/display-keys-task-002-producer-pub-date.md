# Display Keys Task 002: The Station Producer Writes The Episode Date

Status: Ready - 2026-11-26. ADR 0042 governs this task. Task 001 must be committed before this task starts.

Each criterion is mechanical. No visual check applies to this task.

## Goal

The slot text writes `pub_date` into `state.json`. The station producer writes `pub_date` for each segment of an
episode state into `display.json`. The station producer also writes `funding` `null` for each track, as a preparation
for task 003. The Mixxx producer writes no key of the two.

## Context

- ADR 0042 §The Slot Text and §The Producer.
- ADR 0043 §The Key: in `display.json`, a station track with no funding link has `funding` `null`.
- Task 001 made the crate `display-keys` with `pub_day` and `FundingLink`.
- The slot text `liquidsoap/musicindex-state.liq` writes `state.json` in `musicindex_write` (the `album` line is the
  model).
- `liquidsoap-now-playing/src/state.rs` `StateFile` has the optional fields with `#[serde(default)]`.
- `liquidsoap-now-playing/src/lib.rs` `write_display_track` builds the `ShownTrack`. `render_state` receives the parsed
  chapter list. An episode state is a state that is not a jingle and has a chapter list that is not empty.
- `now-playing-drop/src/display.rs` has `ShownTrack`, `TrackJson` and `render_display_json`.
- `mixxx-now-playing/src/main.rs` builds a `ShownTrack` for Mixxx. `mixxx-now-playing/src/display.rs` has a test that
  builds one.

## The Rule

- Slot text: add `pub_date = m["date"]` after `album = m["album"]`. The format stays `musicindex-liquidsoap-state/2`.
- `StateFile`: add `pub_date: Option<String>` with `#[serde(default)]`.
- `ShownTrack` gets one new field that holds the keys of the station producer. A suggested shape:
  `station: Option<StationKeys<'a>>`, with `StationKeys { pub_date: Option<&'a str>, funding: &'a [FundingLink] }`.
  - When `station` is `Some`, `display.json` has `pub_date` (a string, or `null`) and `funding` (an array of 1 to 3
    objects, or `null` when the slice is empty). Each object has `url` and, when present, `label`.
  - When `station` is `None`, `display.json` has no key `pub_date` and no key `funding`.
  - `now-playing-drop` gets the dependency `display-keys = { path = "../display-keys" }` for `FundingLink`.
- The station producer always gives `Some`:
  - `pub_date`: `display_keys::pub_day` of the state `pub_date` for an episode state. `None` for a song state, a jingle
    and a state with no chapter list. Each segment of an episode state has the same value: each chapter, each gap and
    the part after the last chapter.
  - `funding`: an empty slice in this task.
- The Mixxx producer gives `None`.
- The drop file does not change.

## Base components

| Component | Status | Location | Reason |
| --- | --- | --- | --- |
| Day rule | reuse | /home/citizen/build/musicindex-live-publisher/docs/tasks/display-keys-task-001-crate-and-publisher.md | `pub_day` in `display-keys/src/lib.rs`, which task 001 makes. |
| Display writer | extend | /home/citizen/build/musicindex-live-publisher/now-playing-drop/src/display.rs | `ShownTrack` and `TrackJson` get the station keys. ADR 0041 names one display writer. |
| State file | extend | /home/citizen/build/musicindex-live-publisher/liquidsoap-now-playing/src/state.rs | `StateFile` gets the optional field. |
| Slot text | extend | /home/citizen/build/musicindex-live-publisher/liquidsoap/musicindex-state.liq | `musicindex_write` gets one line. |

## Files To Inspect

- `docs/adr/0042-an-episode-sends-its-publication-date.md`
- `display-keys/src/lib.rs`
- `now-playing-drop/src/display.rs`
- `liquidsoap-now-playing/src/lib.rs`, `liquidsoap-now-playing/src/state.rs`
- `mixxx-now-playing/src/main.rs`, `mixxx-now-playing/src/display.rs`
- `liquidsoap/musicindex-state.liq`

## Files Likely To Change

- `liquidsoap/musicindex-state.liq`
- `liquidsoap-now-playing/src/state.rs`, `liquidsoap-now-playing/src/lib.rs`
- `now-playing-drop/Cargo.toml`, `now-playing-drop/src/display.rs`
- `mixxx-now-playing/src/main.rs`, `mixxx-now-playing/src/display.rs`
- `Cargo.lock` (the new path dependency only)

## Do Not Touch

- `src/` of the publisher and `tests/display.rs`. Task 001 owns them.
- `display-keys/` except a defect that blocks this task. Report such a change.
- `now-playing-drop/src/route_service.rs` and `liquidsoap-now-playing/src/lookup.rs`. Task 003 owns them.
- `docs/adr/`, `AGENTS.md`, `docs/README.md`.

## Acceptance Criteria

Mechanical. Each item is a test:

- A producer test: an episode state with `pub_date` `2026-11-12` gives `pub_date` `"2026-11-12"` in `display.json` for
  a chapter, a gap and the part after the last chapter.
- A producer test: a song state, a jingle and a state with no chapter list give `pub_date` `null`.
- A producer test: an episode state with `pub_date` empty, `2026-13-40`, `2026-02-30` or `12 Nov 2026` gives `null`.
  `2026-11-12T08:00:00` gives `"2026-11-12"`.
- A producer test: each station track in `display.json` has `funding` `null`.
- A `now-playing-drop` test: `StationKeys` with two links renders `funding` as two objects, and the object with no
  label has no key `label`. `station` `None` renders no key `pub_date` and no key `funding`.
- A Mixxx test: the Mixxx `display.json` has no key `pub_date` and no key `funding`.
- A state test: a `state.json` with no `pub_date` parses, and `pub_date` is `None`.
- The existing tests pass. A change of an expected `display.json` text in an existing station test adds only the keys
  `pub_date` and `funding`.
- The full gate passes.

## Test Commands

```bash
cargo fmt --all -- --check
cargo build --workspace
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
```

## Escalation Triggers

- The term "episode state" of this task does not agree with the code. Stop and report.
- The station keys need a change of the drop file. Stop and report.

A delegate does not commit. The publisher session reads the diff, runs the gate and makes the commit. Write prose and
code comments in Simplified Technical English (`/home/citizen/.agents/skills/asd-ste100/SKILL.md`).
