# ADR 0010: The Live Value Payload Follows The Model Server

Status: Accepted
Date: 2026-10-06

Accepted 2026-10-06 by the operator. Item 3 of §Before Acceptance, the app
check, stays open as an item of the review checklist. Item 2 was done on
2026-10-06 on the Mixxx host.

Amended 2026-10-06. The operator decided that `link` stays absent until a
source for it exists, so this ADR no longer adds `link`, `link_url` or
`link_text`. A later ADR adds them with their source. The drop file version 2
gets `play_id`, because ADR 0012 pairs a display state with its payload by
that key. Item 1 of §Before Acceptance is done.

Class: situational. Supersede this record when the namespace specification
defines the `podcast:liveValue` payload, or when the model server changes its
payload.

This ADR owns the shape of the live value payload that the publisher sends,
for a V4V track. ADR 0005 keeps the dead block. ADR 0002 keeps version 1 of the
drop file. `musicindex-live-relay` passes the payload through without a change
and owns no rule about its fields.

## Context

Podcast apps receive the live value through the Socket.IO `remoteValue` event
of the URI in `podcast:liveValue`. That tag and that event come from
podcast-namespace discussion #547, "Live Value Updates". The namespace
specification does not include them. The research report of 2026-10-06 has
the evidence for this ADR.

CurioHoster is the model server. Its author is the author of the proposal. A
capture of its test event on 2026-10-06 gave three `remoteValue` events. Each
event had these fields:

| Field | Content in the capture |
|---|---|
| `title` | The song |
| `line` | `[album, artist]` |
| `image` | The album art of that song. It changed with each song. |
| `description` | Text, or empty |
| `value` | `{model: {type, method}, destinations}` |
| `type` | `music` |
| `link` | `{text, url}`, for example a link to the album page |
| `feedGuid`, `itemGuid` | The song |
| `eventGuid`, `blockGuid` | The event and the block |
| `duration`, `startTime` | Numbers |
| `chaptersUrl`, `enclosureUrl`, `eventAPI`, `settings`, `eventTimestamp` | Values of the CurioHoster system |

`liquidsoap-vts-relay` sends the same shape with `line` and `link`.

The proposal author gave a revised shape on 2026-03-19. It has `title` for the
song, `author` for the band, `podcastName` for the album, `image`, `link`, and
a flat `value {type, method, destinations}`. The model server did not send that
shape on 2026-10-06.

This publisher sends `title`, `image`, `description`, `type`, `startTime`,
`duration`, `eventGuid`, `blockGuid`, `feedGuid`, `itemGuid` and `value`. It
does not send `line` or `link`. The drop file has `artist`, but the payload
does not use it. An app that shows `line` as the artist and the album thus
shows nothing for this stream.

The drop file has no album and no link. ADR 0002 requires a new schema version
for a new field.

## Decision

### The Payload For A V4V Track

The publisher keeps each field that it sends today, with the same meaning. It
adds these fields:

| Field | Value | When |
|---|---|---|
| `line` | `[album, artist]`. When the album is not known, `[title, artist]`. | Always |
| `author` | The artist | Always |
| `podcastName` | The album | Only when the album is known |

- `line` follows the model server. `[title, artist]` without an album follows
  `liquidsoap-vts-relay`.
- `author` and `podcastName` follow the revision of 2026-03-19. A client of the
  older shape ignores them, so they cost nothing.
- `value` keeps the nested `model`, because the model server sends that form.
  A flat `type` and `method` adjacent to `destinations` would follow the
  revision, but no deployed server sends it.
- `title` stays the song. `image` stays the artwork URL of the drop file.
- The model server also sends `link {text, url}`. This publisher does not send
  it, because no source in this chain gives a correct link for a track.

The publisher does not send `chaptersUrl`, `enclosureUrl`, `feedUrl`,
`medium`, `eventAPI`, `settings` or `eventTimestamp`. They give data about the
CurioHoster system, and this chain has no correct value for them.

### The Drop File, Version 2

The schema `musicindex.nowplaying/2` has each field of version 1 and these
fields:

| Field | Type | Required | Description |
|---|---:|---:|---|
| `album` | string or null | no | The album of the playing track. |
| `play_id` | string or null | no | The identity of this play of the track. The Mixxx producer writes the ID of its history row. |

- The producer reads `album` from the same Mixxx library row as `artist` and
  `title`.
- `play_id` is different for each play, also when the same track plays two
  times. ADR 0012 uses it. It is in version 2 now, so the pairing needs no
  version 3.
- The producer writes version 2 only. The publisher reads version 2 only. No
  one runs the chain in production, so no transition period is necessary.

### What Does Not Change

- The dead block of ADR 0005.
- The timing. ADR 0005 owns it today. ADR 0011 proposes to move the delay to
  the relay. This ADR does not change it.
- The separation of the two body forms at the relay. The payload never has
  exactly the keys `event_id` and `metadata`.

## Invariants

These rules apply while this decision is current.

- Each field that the publisher sent before this ADR keeps its name, its form
  and its meaning.
- `line` is present in each payload for a V4V track and has two strings.
- The publisher never sends a field with a value that this chain does not
  know. A missing album gives no `podcastName`.
- `value` has the nested `model` form.

## Before Acceptance

1. **The `link` source.** Done 2026-10-06. The operator decided that `link`
   stays absent until a source exists.
2. **The album column.** Done 2026-10-06. The `library` table of the
   operator's Mixxx database has the column `album`, of type `varchar(64)`.
3. **The app check.** Play the CurioCaster test feed
   `https://curiocaster.com/rss/feed.xml` in each app that the operator tests.
   Record which payload fields each app shows. If no app shows `line`,
   reconsider the field.

## Verification After Implementation

Mechanical:

- A payload test for `line` with an album and without one, and for `author`.
- A payload test that `podcastName` is not present when the drop file has no
  album, and that no payload has `link`.
- A payload test that each field from before this ADR keeps its name and form.
- A drop-file test that version 2 parses with and without each new field.
- A drop-file test that version 1 is ignored.
- A producer test that `play_id` differs for two plays of the same track.
- The existing test that the payload never has exactly the keys `event_id`
  and `metadata` stays green.

Visual. A person must examine these items in a podcast app. Report each one as
open until a person completes the check.

- An app that shows `line` shows the artist and the album of the playing
  track from this stream.
- The artwork from `image` changes with the track, as before.

## Alternatives Considered

### Only The Revised Shape Of 2026-03-19

Rejected. The model server does not send it, so apps that work with the model
server expect the older shape.

### Copy Each CurioHoster Field

Rejected. `enclosureUrl`, `eventAPI` and `settings` give data about that
system. A value that this chain cannot make correct is worse than no value.

### The Relay Artwork URL As `image`

Deferred. The publisher could send the relay artwork route of ADR 0008 as
`image` for a track with an embedded image only. Then each track would have
artwork on this path too. But the relay keeps two images only, in memory, and
only for a reserved event. A late fetch could get `404`. Make this decision
after the app check.

## Consequences

Positive:

- Apps that show `line` show the artist and the album of this stream.
- The payload serves the deployed shape and the revised shape.
- No field that a client reads today changes.

Negative and risks:

- The drop file goes to version 2. The producer and the publisher change in
  one commit.
- The payload becomes larger by less than 1 KiB. The relay limit for a publish is
  64 KiB.
- The proposal can change again before it enters the specification.

## References

- `docs/adr/0002-nowplaying-drop-file-contract.md`
- `docs/adr/0005-producer-liveness-and-dead-block.md`
- `docs/adr/0008-display-path.md`
- `docs/adr/0009-hls-track-metadata.md`
- `musicindex-live-relay`: `docs/research/curiohoster-livevalue-socketio-examples.md`, §Capture Of 2026-10-06
- The research report: `~/build/musicindex-research/reports/Live RSS metadata delivery paths.md`
- The capture: `~/build/musicindex-research/sources/captures/2026-10-06-curiohoster-event-test-remoteValue.txt`
- podcast-namespace discussion #547:
  https://github.com/Podcastindex-org/podcast-namespace/discussions/547
- `liquidsoap-vts-relay`: https://github.com/v4vmusic/liquidsoap-vts-relay
