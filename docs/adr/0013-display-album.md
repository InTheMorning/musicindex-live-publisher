# ADR 0013: The Display State Carries The Album

Status: Proposed
Date: 2026-10-06

Class: situational. Supersede this record when the display state gets its
track text from a different source.

This ADR owns version 3 of `display.json` and the `album` key of the display
body. `musicindex-live-relay` ADR 0006 owns that key on the wire. ADR 0012
keeps version 2 of `display.json`.

## Context

The private app shows the track from the display state. The display state
has the artist, the title, the artwork, the song line and the value identity
(ADR 0008, ADR 0012). It has no album.

ADR 0010 reads the album from the Mixxx history row. It puts the album in the
drop file and in the live value payload, as `podcastName` and `line[0]`. Only a
V4V track has a drop file and a payload. So an app that reads the album from
the payload shows no album for a track that pays nobody.

The producer reads the album for each history row, V4V or not (`TrackRow`).
The display path of the producer does not send it. The `Coordinator` keeps
only the artist and the title in its `DisplayTrack`. The V4V state of the
producer holds the album, but the producer clears that state for a track
that is not V4V. So the album must go with the display track, not with the
V4V state.

## Decision

### The Producer

`display.json` goes to the schema `musicindex.display/3`. A track adds one
field to the fields of version 2:

| Field | Type | Description |
|---|---|---|
| `album` | string or null | The album of the history row. `null` when Mixxx has no album, or an empty album |

- The `Coordinator` keeps the album in `DisplayTrack`, with the artist and
  the title. The album comes from the same history row.
- The producer writes version 3 only. The publisher reads version 3 only. No
  one runs the chain in production, so no transition period is necessary.
- A `null` track does not change.

### The Publisher

- When it sends a display state, the publisher adds `album` to the track when
  `album` is a string that is not empty. It adds no key when `album` is
  `null`.
- The rules of ADR 0012 for `songLine` and `value` do not change.

### What Does Not Change

- The live value payload, its `podcastName` and its `line` (ADR 0010).
- The drop file.
- The images and their rules (ADR 0008).

### The Deployment Order

Relay ADR 0006 must be deployed first. An older relay gives
`400 invalid_display` for a track with `album`.

## Invariants

These rules apply while this decision is current.

- The album of a display state comes from the same history row as its artist
  and title.
- A track that is not V4V gets its album also.
- The publisher never sends an empty `album`.

## Non-Goals

- No album in the HLS frame of ADR 0009. ADR 0009 can add it later.
- No album that the producer reads from the file tags. The source is the
  Mixxx library, as for ADR 0010.

## Before Acceptance

1. **Relay ADR 0006** is accepted.

## Verification After Implementation

Mechanical:

- A producer test that `display.json` version 3 has the album of the history
  row of a V4V track.
- The same test for a track that is not V4V.
- A producer test that a `NULL` and an empty album give `null`.
- A publisher test that a display state with an album sends `album`, and a
  display state with `null` sends no `album` key.
- A publisher test that ignores version 2 with a warning.

Visual. A person must examine this item. Report it as open until a person
completes the check.

- The private app shows the album for a V4V track and for a track that is
  not V4V.

## Alternatives Considered

### The App Pairs The Display State With The Payload

Rejected as the only source. The app reads the album from the payload whose
`blockGuid` the display state names. That works only for a V4V track, and
the app must read a second stream.

### The Album In `songLine`

Rejected. `songLine` is the exact line that `butt` sends (ADR 0009). A change
to it changes the ICY title.

## Consequences

Positive:

- An app shows the album for each track.
- The display state has all the text that the player screen needs.

Negative and risks:

- A new version of `display.json`. The producer and the publisher must ship
  in one release.
- An older relay refuses the key. The deployment order matters.

## References

- `docs/adr/0008-display-path.md`
- `docs/adr/0010-live-value-payload-reference-shape.md`
- `docs/adr/0012-pair-display-state-with-value-block.md`
- `musicindex-live-relay`: `docs/adr/0006-display-album.md`
