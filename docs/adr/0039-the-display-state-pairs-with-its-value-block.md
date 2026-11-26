# ADR 0039: The Display State Pairs With Its Value Block

- Status: Accepted on 2026-11-26 by the operator. ADR 0012 is in `docs/archive/adr/`.
- Date: 2026-11-26
- Changed by: ADR 0038, accepted 2026-11-26. The display state on the wire also has `playId`.
- Supersedes: ADR 0012 (`docs/archive/adr/0012-pair-display-state-with-value-block.md`). This record restates each live rule
  of ADR 0012 and of its three changes. It adds no rule and removes no rule.
- Uses: ADR 0013 (version 3 of `display.json` and `album`), ADR 0041 §The Song Line (the station song line), ADR 0009
  §The Song Line (the Mixxx song line), ADR 0010 (`play_id` in the drop file), relay ADR 0005 (`songLine` and `value`
  on the wire).
- Operator choice, 2026-11-26: continue with ADR 0038. ADR 0038 must change the pairing rules, and ADR 0012 has three
  "Changed by" lines (global `CLAUDE.md` §Consolidate A Changed Decision).

This record owns the fields `song_line` and `play_id` of `display.json`, and the pairing rule of the publisher.
ADR 0013 owns the version of `display.json` and `album`. Relay ADR 0005 owns `songLine` and `value` on the wire.

## Context

The tagger of ADR 0009 and the apps need two facts in each display state:

- the exact song line of the stream for the track.
- the `eventGuid` and the `blockGuid` of the live value payload of the same play.

The producer writes the drop file and `display.json`. It does not know the `blockGuid`, because the publisher mints
it. The publisher knows the `blockGuid`, but it reads the drop file and `display.json` as two files, in no fixed
order. The two files share `play_id`, which is different for each play, also when one track plays two times
(ADR 0010).

A track that pays nobody has no drop file. Its display state has no value identity.

ADR 0012 had these changes, and this record restates each of them:

- ADR 0013: `display.json` version 3 adds `album`. The pairing rules do not change.
- ADR 0026, archived: `song_line` is the ICY `StreamTitle` of the stream for the track.
- ADR 0041: it restates the change of ADR 0026 for the station producer.

## Decision

### The Producer

A track in `display.json` has these two fields. It also has the fields of ADR 0008 and ADR 0013.

| Field | Type | Description |
|---|---|---|
| `song_line` | string | The ICY `StreamTitle` of the stream for this track, exactly as the stream sends it |
| `play_id` | string or null | The same value as `play_id` in the drop file of this play |

- For the Mixxx producer, `song_line` is the first line of `now-playing.txt` (ADR 0009 §The Song Line).
- For the station producer, `song_line` is the line of ADR 0041 §The Song Line.
- The producer writes the current version of `display.json` only, and the publisher reads the current version only
  (ADR 0013).
- A `null` track has no `song_line` and no `play_id`.

### The Publisher

- For each target, the publisher keeps the `play_id`, the `eventGuid` and the `blockGuid` of the newest track payload
  that it published.
- When it sends a display state, it adds to the track:
  - `songLine`, from `song_line`.
  - `value: {eventGuid, blockGuid}`, only when `play_id` agrees with the kept `play_id` of the target.
- A track payload can go out after the display state of the same `play_id`. The publisher then sends that display
  state again, one time, with `value`. The relay stores no image again, because it holds the image.
- A dead block clears the kept `play_id`. A display state then gets no `value` until the next track payload.
- A track with no drop file gets `songLine` and no `value`.

## Invariants

These rules apply while this decision is current.

- A display state names only a block that the publisher published for the same `play_id`.
- The publisher never builds `value` from a text match.
- `songLine` is the ICY `StreamTitle` of the stream for the track, without a change.

## Verification

Mechanical. The tests of display pairing tasks 001 and 002 enforce these rules. They stay as they are:

- `display.json` has `song_line` equal to the song line of the producer, and the same `play_id` as the drop file.
- A display state after its payload gets `value`.
- A display state before its payload goes out without `value`, and then again one time with `value`.
- A track with no drop file gets `songLine` and no `value`.
- A dead block clears the pairing.
- Two plays of the same track get two different blocks.

## Alternatives Considered

### Pair By Artist And Title

Rejected. The live value has no artist, and the same track can play two times. A text match can name the block of a
different play.

### The Producer Mints The `blockGuid`

Rejected. The publisher mints the `blockGuid` and keeps it across a route upgrade of one track (audit fix 002). A
second minter would break that rule.

## Consequences

- The tagger of ADR 0009 and the apps match by equality and name the exact block.
- A display state can go out two times for one play.
- ADR 0012 is in `docs/archive/adr/`. The code comments cite this record.

## References

- `docs/adr/0008-display-path.md`
- `docs/adr/0009-hls-track-metadata.md`
- `docs/adr/0010-live-value-payload-reference-shape.md`
- `docs/adr/0013-display-album.md`
- `docs/adr/0041-the-station-producer.md`
- `docs/reviews/display-pairing-review-checklist.md`
- `musicindex-live-relay`: `docs/adr/0005-display-song-line-and-value.md`
