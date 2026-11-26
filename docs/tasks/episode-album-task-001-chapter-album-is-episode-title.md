# Episode Album Task 001: A Chapter Song Has The Episode Title As Its Album

Status: Implemented - 2026-11-26. ADR 0040 governs this task. The Rainbrook session wrote this task for the
publisher session.

Operator choice, 2026-11-26: the drop file has the episode title as `album` also. The live value payload thus has
the episode title in `line` and `podcastName` (ADR 0010).

Each criterion is mechanical. The visual check is in a different list.

## Goal

While a chapter song of an episode file plays, the station sends the episode
title as the `album`. Rainbrook then shows "Homegrown Hits - Episode 158"
below the artist, not "Homegrown Hits". The lock screen shows the same text.

## Context

Rainbrook shows the relay `album` below the artist (Rainbrook ADR 0019,
`Rainbrook/PlayerView.swift:87`). Rainbrook does not change.

The producer sets `album` from the state for each segment (ADR 0035 §What The
Producer Writes For An Episode File, and the table in §What The Producer Writes
In `display.json`). Two places in the code apply this rule:

- `liquidsoap-now-playing/src/lib.rs:488` (the drop file).
- `liquidsoap-now-playing/src/lib.rs:247` (the display track).

Test station log step 16 records the tags of the episode files. The title tag is
"Homegrown Hits - Episode 15x", and the album tag is "Homegrown Hits". The state
file has the two values in `title` and `album`.

The relay accepts an `album` of at most 1,024 characters
(`musicindex-live-relay/src/lib.rs:102`). The relay does not change.

## The Rule

Operator choice of 2026-11-26: send "Homegrown Hits - Episode 158", not
"Homegrown Hits", while a chapter song plays.

- A segment with a chapter that has a title that is not empty: `album` is the
  state `title` when it is not empty. If the state `title` is empty, `album` is
  the state `album`.
- A chapter with no title: `album` is the state `album`. The title falls
  back to the state title, so the two lines must not show the same text.
- A gap segment, a file with no chapters and a jingle: `album` is the state
  `album`. This does not change.
- The display track of a segment that is not a jingle takes `album` from the
  drop file of the same read. It does the same for `artist` and `title` (ADR
  0035 §Invariants).

## Base components

| Component | Status | Location | Reason |
| --- | --- | --- | --- |
| Segment drop file render | extend | /home/citizen/build/musicindex-live-publisher/liquidsoap-now-playing/src/lib.rs | The function that sets `title` and `artist` for a chapter also sets `album`. |
| Display track write | extend | /home/citizen/build/musicindex-live-publisher/liquidsoap-now-playing/src/lib.rs | `write_display_track` copies `artist` and `title` from the drop file. |
| Chapter drop test helpers | reuse | /home/citizen/build/musicindex-live-publisher/liquidsoap-now-playing/src/lib.rs | `chapter_drops` and `chapter_patch` make a state with chapters. |

## Decision Record

The publisher session decides the record. The recommendation is a new ADR 0040
with `- Changes: ADR 0035`. It changes two items of ADR 0035:

- The `album` sentence in §What The Producer Writes For An Episode File.
- The `album` row in §What The Producer Writes In `display.json`.

The bar condition is `opposite choice by mistake`. The
ADR text tells an agent to use the state album. ADR 0035 then gets its
third `- Changed by:` line. ADR 0035, the new ADR, `docs/adr/README.md` and the
code change go in the same commit.

## Files To Inspect

- `docs/adr/0035-the-station-producer.md`
- `liquidsoap-now-playing/src/lib.rs`
- `liquidsoap-now-playing/src/state.rs`
- `docs/runbooks/test-station-log.md`, step 16

## Files Likely To Change

- `liquidsoap-now-playing/src/lib.rs`
- `docs/adr/0035-the-station-producer.md`
- A new ADR in `docs/adr/` and its row in `docs/adr/README.md`

## Do Not Touch

- The Rainbrook repository and the relay repository.
- `liquidsoap/*.liq`. The state file has the two values.
- The Mixxx producer.

## Acceptance Criteria

Mechanical. Each item is a test in `liquidsoap-now-playing`:

- A chapter with a title gives the drop file `album` equal to the state `title`.
- The display track of that segment has the same `album` as its drop file.
- A chapter with an empty title gives the state `album`.
- A chapter with a title and an empty state `title` gives the state `album`.
- A gap segment, a file with no chapters and a jingle give the state `album`.
- The first test fails when the code uses the state `album` for each segment.

Also: the full gate passes. `Cargo.lock` does not change.

Visual. Only the operator can do this check:

- While a chapter song of an episode plays on the test station, Rainbrook shows
  "Homegrown Hits - Episode 158" (or the number of the episode that plays) below
  the artist. The lock screen shows the same text.

## Test Commands

```bash
cargo fmt --all -- --check
cargo build --workspace
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
```

## Escalation Triggers

- The relay or a consumer other than Rainbrook uses `album` for a match or a
  key. Stop and report to the operator.
- The ADR 0035 text gives `album` a second use that this task does not name.

A delegate does not commit. The publisher session reads the diff, runs the gate
and makes the commit. Write prose in Simplified Technical English
(`/home/citizen/.agents/skills/asd-ste100/SKILL.md`).
