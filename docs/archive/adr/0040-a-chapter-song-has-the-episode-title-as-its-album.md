# ADR 0040: A Chapter Song Has The Episode Title As Its Album

> Archived: superseded by [ADR 0041](../../adr/0041-the-station-producer.md) on 2026-11-26. ADR 0041 §The Album Of A Segment restates each live rule of this record.
> Read this record for research only.

- Status: Superseded by ADR 0041 on 2026-11-26. Accepted on 2026-11-26 by the operator.
- Date: 2026-11-26
- Bar: opposite choice by mistake. ADR 0035 told an agent to use the state album for each
  segment.
- Changes: ADR 0035 §What The Producer Writes For An Episode File (the `album` sentence) and §What The Producer Writes
  In `display.json` (the `album` row).
- Uses: ADR 0010 (`line` and `podcastName` come from the drop file `album`) and ADR 0013 (`album` in `display.json`),
  without a change.
- Operator choice, 2026-11-26: while a chapter song plays, Rainbrook shows "Homegrown Hits - Episode 158" below the
  artist, not "Homegrown Hits".
- Operator choice, 2026-11-26: the drop file has the same `album`, so the live value payload also has the episode title
  in `line` and `podcastName`.

## Context

The slot text writes the title tag and the album tag of the file into the state (ADR 0035 §The State File Contract).
For an episode file of the test station, the title is "Homegrown Hits - Episode 158" and the album is
"Homegrown Hits" (test station log step 16).

While a chapter song plays, the title line shows the chapter title. The episode title thus showed in no line. Rainbrook
shows the display `album` below the artist (Rainbrook ADR 0019).

The publisher puts the drop file `album` into the live value payload as `line[0]` and `podcastName` (ADR 0010). Other
podcast apps show these fields.

## Decision

For a segment of an episode file, the producer sets `album` in the drop file as follows:

| Segment | `album` |
| --- | --- |
| A chapter with a title that is not empty, and a state `title` that is not empty | The state `title` |
| A chapter with an empty title or no title | The state `album` |
| A chapter with a title, and an empty state `title` | The state `album` |
| A gap, a file with no chapters | The state `album` |

- A jingle drop file has no `album` (ADR 0035 §Jingle Mode). The jingle display track has the state `album`.
- Each other display track has the `album` of the drop file of the same read, the same as `artist` and `title`.
- An empty `album` is `null` in the drop file and in `display.json`.
- A chapter with no title shows the state title in the title line. Its `album` stays the state album, so the two lines
  do not show the same text.

## Invariants

These rules apply while this decision is current.

- A display track that is not a jingle has the `album` of the drop file of the same read.
- The producer does not read `album` from the chapter list.

## Verification

Mechanical. Tests in `liquidsoap-now-playing/src/lib.rs`:

- `a_chapter_with_a_title_has_the_episode_title_as_the_album`: chapters, a gap and the end, in the drop file and in
  `display.json`.
- `a_chapter_with_no_title_or_no_episode_title_has_the_state_album`.
- `display_song_file_without_chapters_gives_one_state` and `display_jingle_has_the_state_names_and_no_play_id`.
- `assert_track_of_drop` compares the display `album` with the drop file `album` in each display test.

Visual. Only the operator can do this check:

- While a chapter song of an episode plays on the test station, Rainbrook shows "Homegrown Hits - Episode 158" (or the
  number of the episode that plays) below the artist. The lock screen shows the same text.

## Consequences

- The payment metadata of a chapter song names the episode in `podcastName`.
- The relay and Rainbrook do not change. The relay accepts an `album` of at most 1,024 characters (relay ADR 0006).
