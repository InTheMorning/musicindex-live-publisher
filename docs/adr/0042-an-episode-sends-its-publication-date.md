# ADR 0042: An Episode Sends Its Publication Date

- Status: Proposed on 2026-11-26.
- Date: 2026-11-26
- Bar: crosses repositories. The rule changes the slot text and the producer in this repository, the display state of
  `musicindex-live-relay`, and the display of Rainbrook.
- Changes: ADR 0013 §The Producer and §The Publisher. A track in `display.json` version 3 can also have `pub_date`.
- Changes: ADR 0041 §The State File Contract. The state gets the optional field `pub_date`.
- Uses: ADR 0041 §The Route Lookup (the term **episode state**) and musicindex ADR 0092 (`station-import` writes the
  item `pubDate` into `TDRC`), without a change.
- Operator choice, 2026-11-26: only an episode sends its date, and only in the display state. The live value payload
  does not change.

## Context

`station-import` writes the `pubDate` of the feed item into the frame `TDRC` as the UTC day, in the format `YYYY-MM-DD`.
On the test station, `ffprobe` gives `date=2026-11-12` for the file of episode 158 (station log step 55).

No step after the file sends that date:

- The slot text does not write a date into `state.json`.
- `display.json` version 3 has no date (ADR 0013).
- The relay refuses a display track with a key that it does not know (`musicindex-live-relay/src/lib.rs:2637`).

The publisher ignores a key of `display.json` that it does not know (`src/display.rs`, `parse_track`). An optional
key thus needs no new version of `display.json`.

## Decision

### The Slot Text

The slot text writes `pub_date = m["date"]` into `state.json`. The format stays `musicindex-liquidsoap-state/2`,
because the field is optional (ADR 0041 §The State File Contract).

| Field | Format | Type | Use in the producer |
| --- | --- | --- | --- |
| `pub_date` | 2, optional | string | `m["date"]`. Empty when the metadata has no key `date`. |

### The Producer

A track in `display.json` version 3 has one more optional field:

| Field | Type | Description |
| --- | --- | --- |
| `pub_date` | string or null | The publication day of the episode, `YYYY-MM-DD` |

- The producer writes `pub_date` only for an episode state. Each segment of the episode file has the same `pub_date`:
  each chapter, each gap and the part after the last chapter.
- A song state, a jingle and a state with no chapter list give `null`.
- The producer writes the date only when the first 10 characters of the state `pub_date` are a day in the format
  `YYYY-MM-DD` that exists in the Gregorian calendar. Else it writes `null`. For example, `2026-02-30` gives `null`,
  because the relay refuses the full display state for a day that does not exist (relay ADR 0013). A longer value, for example `2026-11-12T08:00:00`, gives its first 10
  characters.
- The drop file does not change.
- The Mixxx producer does not change. Its `display.json` has no `pub_date`.

### The Publisher

- When it sends a display state, the publisher adds `pubDate` to the track when `pub_date` is a string that is not
  empty. It adds no key when `pub_date` is `null` or missing.
- Relay ADR 0013 owns the name and the format of `pubDate` on the wire: exactly 10 characters `YYYY-MM-DD`, a day that
  exists. The relay refuses `null` and an empty string, so the publisher sends no key for a track with no date.

### What Does Not Change

- The live value payload (ADR 0010).
- The drop file.
- The rules of ADR 0039 for `songLine`, `value` and `playId`.

### The Deployment Order

1. The relay that accepts `pubDate`.
2. The slot text, the producer and the publisher of this record, in one install.
3. Rainbrook that shows the date.

An older relay gives `400 invalid_display` for each display state of an episode with `pubDate`.

## Invariants

These rules apply while this decision is current.

- Only a display track of an episode state has a `pub_date` that is not `null`.
- Each segment of one episode file has the same `pub_date`.
- The publisher does not send an empty `pubDate`.

## Before Acceptance

1. A relay ADR accepts `pubDate` in the display track, and gives its format. Relay ADR 0013 (relay commit `0375ea4`)
   has the status Proposed and agrees with this record. It waits for the acceptance of this record.
2. A Rainbrook ADR tells where the app shows the date.
3. On the test station, `state.json` gets `2026-11-12` from `m["date"]` for episode 158. If the key has a different
   name in Liquidsoap, §The Slot Text changes before acceptance.

## Verification

Mechanical. Tests in this repository:

- A producer test: an episode state with `pub_date` `2026-11-12` gives that `pub_date` in `display.json` for a
  chapter, a gap and the part after the last chapter.
- A producer test: a song state, a jingle and a state with no chapter list give `pub_date` `null`.
- A producer test: an empty value, `2026-13-40`, `2026-02-30` and `12 Nov 2026` give `null`. `2026-11-12T08:00:00` gives
  `2026-11-12`.
- A publisher test: `pub_date` gives `pubDate` on the wire, and `null` or a missing key gives no key.

Visual. Only the operator can do this check:

- While episode 158 plays on the test station, Rainbrook shows its date where the Rainbrook ADR tells.

## Consequences

- A chapter song shows the date of the episode, not the release date of the song.
- An episode file with no chapter list sends no date. If this is necessary in the future, a new record changes the term of an
  episode file.
