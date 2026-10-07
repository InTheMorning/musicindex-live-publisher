# ADR 0012: Pair The Display State With Its Value Block

Status: Implemented
Date: 2026-10-06

Implemented 2026-10-06: display pairing tasks 001 and 002 are done, and the
chain is deployed. The named artifact is
`docs/reviews/display-pairing-review-checklist.md`, with no open item.

Accepted 2026-10-06 by the operator. The two items of §Before Acceptance
are done: relay ADR 0005 and ADR 0010 are accepted.

Class: situational. Supersede this record when the tagger of ADR 0009 gets
its pairing from a different source.

This ADR owns version 2 of `display.json` and the pairing rule of the
publisher. `musicindex-live-relay` ADR 0005 owns the two new keys on the
wire. ADR 0008 keeps version 1 of `display.json`.

## Context

ADR 0009 §The Value Identity needs two facts in each display state:

- the exact song line,
- the `eventGuid` and `blockGuid` of the live value payload of the same
  track.

The operator accepted that pairing on 2026-10-06.

The producer writes the song line, the drop file and `display.json` at one
track change. It does not know the `blockGuid`, because the publisher mints
it. The publisher knows the `blockGuid`, but it gets the drop file and
`display.json` as two files, in no fixed order.

A key that the two files share solves this. ADR 0010 adds `play_id` to the
drop file version 2: the ID of the Mixxx history row. That ID is different for
each play, also when one track plays two times.

A track that pays nobody has no drop file. Its display state has no value
identity.

## Decision

### The Producer

`display.json` goes to the schema `musicindex.display/2`. A track adds two
fields to the fields of version 1:

| Field | Type | Description |
|---|---|---|
| `song_line` | string | The first line of `now-playing.txt` for this track, exactly as the producer writes it (ADR 0009 §The Song Line) |
| `play_id` | string or null | The same value as `play_id` in the drop file of this play |

- The producer writes version 2 only. The publisher reads version 2 only. No
  one runs the chain in production, so no transition period is necessary.
- A `null` track does not change.

### The Publisher

- For each target, the publisher keeps the `play_id`, the `eventGuid` and the
  `blockGuid` of the newest track payload that it published.
- When it sends a display state, it adds to the track:
  - `songLine`, from `song_line`,
  - `value: {eventGuid, blockGuid}`, only when `play_id` agrees with the kept
    `play_id` of the target.
- A track payload can go out after the display state of the same `play_id`.
  The publisher then sends that display state again, one time, with `value`. The relay stores no image again, because the image is held.
- A dead block clears the kept `play_id`. A display state then gets no
  `value` until the next track payload.

### What Does Not Change

- The live value payload and its timing.
- The display path for a track with no drop file: it gets `songLine` and no
  `value`.
- The rules of ADR 0008 for images.

### The Deployment Order

Relay ADR 0005 must be deployed first. An older relay gives
`400 invalid_display` for a track with `songLine` or `value`.

## Invariants

These rules apply while this decision is current.

- A display state names only a block that the publisher published for the
  same `play_id`.
- The publisher never builds `value` from a text match.
- `songLine` is the line that `butt` reads, without a change.

## Before Acceptance

1. **Relay ADR 0005** is accepted.
2. **ADR 0010** is accepted, because this ADR needs `play_id` in the drop
   file version 2.

## Verification After Implementation

Mechanical:

- A producer test that `display.json` version 2 has `song_line` equal to the
  first line of `now-playing.txt`, and the same `play_id` as the drop file.
- A publisher test that a display state after its payload gets `value`.
- A publisher test that a display state before its payload goes out without
  `value`, and then again one time with `value`.
- A publisher test that a track with no drop file gets `songLine` and no
  `value`.
- A publisher test that a dead block clears the pairing.
- A publisher test that two plays of the same track get two different blocks.

## Alternatives Considered

### Pair By Artist And Title

Rejected. The live value has no artist, and the same track can play two
times. A text match can name the block of a different play.

### The Producer Mints The `blockGuid`

Rejected. The publisher mints the `blockGuid` and keeps it across a route
upgrade of one track (audit fix 002). A second minter would break that rule.

## Consequences

Positive:

- The tagger of ADR 0009 matches by equality and names the exact block.
- The ICY sync of the private app can also compare by equality with
  `songLine`. That is a decision for `citizenradio`.

Negative and risks:

- `display.json` goes to version 2. The producer and the publisher change in
  one release.
- A display state can go out two times for one track.

## References

- `docs/adr/0008-display-path.md`
- `docs/adr/0009-hls-track-metadata.md`
- `docs/adr/0010-live-value-payload-reference-shape.md`
- `docs/tasks/audit-fix-task-002-block-guid-per-track.md`
- `musicindex-live-relay`: `docs/adr/0005-display-song-line-and-value.md`
