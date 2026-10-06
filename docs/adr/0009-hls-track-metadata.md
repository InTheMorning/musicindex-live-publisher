# ADR 0009: Track Metadata In The HLS Stream

Status: Proposed
Date: 2026-10-05

Amended 2026-10-06 after the research report on live metadata paths. §Two
Paths no longer calls `podcast:liveValue` a Podcasting 2.0 standard. §Context
names the ID3v2.3 defect of the test stack. ADR 0010 now owns the shape of the
compatibility payload.

Amended 2026-10-06 a second time. With the delay in the publisher, each
display state arrived late at the tagger, by almost the full delay. §The Join
Point now requires the instant routes of `musicindex-live-relay` ADR 0004.
ADR 0011 removes the delay from the publisher.

Amended 2026-10-06 a fourth time. The operator decided the tagger home and
accepted the pairing. §The Value Identity names ADR 0010, ADR 0012 and relay
ADR 0005.

Amended 2026-10-06 a third time. §Where The Tagger Runs records that the
tagger serves other broadcasters, and that it can run on any host that reads
their Icecast mount.

Class: situational. Supersede this record when a different component joins the
track metadata to the audio, or when the HLS stream stops.

This ADR owns two things:

- the song line that `butt` sends as the ICY `StreamTitle`,
- the content of the ID3 frames in the HLS stream.

`musicindex-live-relay` ADR 0003 owns the display state on the wire.
`citizenradio` ADR 0011 owns how the private app selects a track. This ADR
cites those records and does not restate their rules.

## Context

The operator sends one stream in two forms. The HLS form is new. A test stack
on the VPS makes it. No repository holds that stack:

```text
Mixxx ─▶ butt ─▶ Icecast ─┬─▶ ICY listeners
                          └─▶ liquidsoap ─▶ HLS listeners
```

`butt` puts the song line into the audio as the ICY `StreamTitle`. The song
line therefore arrives at each listener at the time that the listener hears
the track. Liquidsoap 2.4.1 copies `StreamTitle` without a change into the
`TIT2` frame at the start of each HLS segment. A test on 2026-10-06 found
`TIT2` = `Jethro Tull - Budapest` in a segment.

The private app uses the song line as a key (`citizenradio` ADR 0011). It
holds the display states that it receives from the relay, and applies one
when the song line names it. This works, but it has these limits:

- The song line is only text. The app must compare text to find the artwork.
- The song line and the display state do not always agree. The format rules
  of `now-playing.txt` change the song line, and ADR 0008 §The Display State
  keeps the raw fields in the display state.
- The live value has no artist. No component can pair a value block with the
  song line. A boost therefore pays the newest block, and near a track change
  that block can belong to the next track.
- No record owns the song line. `render_metadata_text` in
  `mixxx-now-playing/src/render.rs` makes it, and the app depends on it.

Metadata keeps its position in the audio only from the point where a
component puts it into the stream. The ICY title has the exact position,
because `butt` puts it in on the broadcast machine. The live value and the
display state travel separately from the audio. They depend on the stream
delay of ADR 0008. One delay cannot agree with each listener.

The research report of 2026-10-06 examined the known senders of the live
value. None of them changes the block at the time that the listener hears the
track. CurioHoster and `liquidsoap-vts-relay` change it at the playout time of
the server. This timing problem thus applies to the whole ecosystem, not only
to this chain.

The liquidsoap ID3 writer (`ocaml-metadata`) has two properties that affect
the frame format:

- It writes each text frame with the encoding byte 3, UTF-8. ID3v2.3 does not
  give a meaning to that byte. ID3v2.4 does. The test stack used
  `id3_version=3` until 2026-10-06, so its tags did not agree with ID3v2.3.
  ffmpeg accepted them.
- It writes a `TXXX` frame as the encoding byte followed by the raw value. A
  valid `TXXX` frame thus needs the value `description` + NUL + `text`.

## Decision

### Two Paths

The chain keeps two paths while the operator designs the next one:

- **The compatibility path.** Podcast apps receive the live value block
  through the Socket.IO `remoteValue` event of the relay. They show its
  `image` URL as the artwork of the live item. The tag `podcast:liveValue` and the
  event come from podcast-namespace discussion #547. The namespace
  specification does not include them. CurioHoster is the model server.

  ADR 0010 owns the payload of this path, and this ADR does not change it.
- **The next-generation path.** A player that reads the MusicIndex frame of
  this ADR gets the track, the artwork and the value identity in the audio
  stream.

The next-generation path adds data. It never replaces a field, an event
or a timing rule of the compatibility path. A player that does not read the
frame gets the same data as before, from the same source.

### The Join Point

The HLS packager on the VPS joins the track metadata to the audio. A tagger
on the VPS, with liquidsoap, does the match that the app does today, one time, before
the packaging:

1. The tagger receives the display states of the event from the instant SSE
   route of the relay, `/display/events` (relay ADR 0004).
2. It keeps a list of the last 4 states.
3. Each ICY title change releases one state, by the selection rules of
   `citizenradio` ADR 0011.
4. The tagger puts the MusicIndex frame for that state into the same metadata
   group as the ICY title. The frame thus has the exact position of the ICY
   title.

A state that arrives after its ICY title goes in when it arrives. That is the
same rule as ADR 0011, and the position is late by the time that the state
was late.

The join works only when each state arrives before its ICY title. The
publisher must thus send each state at once (ADR 0011), and the tagger must
read a route that does not wait for the listener delay. The same rule applies
to the app. It reads the display states and the live value from the instant
SSE routes, not from Socket.IO.

### Where The Tagger Runs

Other broadcasters use the public relay and `v4vmm`, each with an own Icecast
server. The tagger thus serves each broadcaster, not only the operator.

- The tagger reads the Icecast stream of one broadcaster over HTTP. It does
  not need to run on the Icecast host. The ICY title and the audio travel
  together, so each frame keeps its position on any network path.
- It can run on the host of the broadcaster, or on a different host that
  reads the mount. An example is one stack for each broadcaster on the VPS of
  the operator.
- Each instance has two settings: the Icecast mount URL and the relay event
  of the broadcaster. The event must be reserved, because the display routes
  need a reserved event (relay ADR 0003).
- The tagger and liquidsoap ship as one package that a broadcaster can run.
  A tagger that operates only on the VPS of the operator does not agree with
  this ADR.

### The Song Line

The song line is the first line of `now-playing.txt`:

- `artist + " - " + title`, after the format rules of `now-playing.txt`,
- an empty line for the `null` display state (ADR 0008 §The Song File For
  `butt`).

A change to this format is a change to this ADR.

### The ID3 Frames

Each HLS segment starts with one ID3v2.4 tag. Liquidsoap repeats the last
group at each segment, so a listener that joins late gets it.

| Frame | Content |
|---|---|
| `TIT2` | The song line, as `butt` sends it. No change. |
| `TXXX` with the description `musicindex` | The MusicIndex frame, a JSON object. See below. |
| `PRIV` `com.apple.streaming.transportStreamTimestamp` | Written by liquidsoap for packed audio. This contract does not include it. |

`TIT2` stays for each player that does not read the MusicIndex frame, and for
the app fallback.

### The MusicIndex Frame

```json
{
  "schema": "musicindex.hls/1",
  "track": {
    "artist": "Artist",
    "title": "Title",
    "artwork": { "sha256": "<64 lowercase hex characters>", "mime": "image/jpeg" }
  },
  "value": { "eventGuid": "<eventGuid>", "blockGuid": "<blockGuid>" }
}
```

- `track` has the shape of the display track of ADR 0008 §The Producer
  Output, with the same three `artwork` forms. The frame identifies an image.
  It never holds image data.
- `track` is `null` when the ICY title is empty. The tagger writes that frame
  also, so a pause clears the display at the time that the listener hears it.
- `value` names the live value payload of the track. It is `null` for a track
  that pays nobody, and `null` while §The Value Identity is not available.
- The JSON is at most 4,096 bytes in UTF-8.
- The frame has no field that this ADR does not name. A new field needs a new
  `schema` value.
- When the ICY title names no held state, the group has no MusicIndex frame.
  The player then uses `TIT2`.

### The Meaning Of `value`

`value` names the only live value block that a boost may pay for this audio.
A player finds the payload with the same `eventGuid` and `blockGuid` in the
payloads that it received from the relay. If it does not hold that payload, it
does not pay a different block.

### The Value Identity

Today no component can pair a display state with its live value payload. The
producer makes the display state and the drop file. The publisher mints the
`blockGuid`. The relay rejects an unknown display field.

This ADR selects this source for the pairing:

1. The producer adds `play_id` to the drop file (ADR 0010) and to
   `display.json`, with the exact song line (ADR 0012).
2. The publisher adds `songLine` and `value {eventGuid, blockGuid}` to the
   display state of the same `play_id` (ADR 0012).
3. The relay accepts and passes through those two keys (relay ADR 0005).
4. The tagger matches the ICY title to `songLine` by equality, not by the
   text rules of ADR 0011. It copies `value` into the frame.

Until those changes exist, the tagger uses the ADR 0011 rules and writes
`value` as `null`.

## Invariants

These rules apply while this decision is current.

- The live value payload, its Socket.IO delivery and its `image` field do not
  change for the next-generation path. ADR 0002, ADR 0005 and ADR 0008 keep
  their rules for that payload.
- The tagger reads from the relay. It never publishes to the relay.

- `TIT2` holds the ICY title without a change.
- Only an ICY title change releases a MusicIndex frame, or the late arrival of
  its state. A timer never releases one.
- No image data goes into an ID3 frame.
- The tag is ID3v2.4.
- A frame never names a value block that the publisher did not pair with that
  track. The tagger never infers `value` from text.

## Before Acceptance

1. **The `TXXX` check.** Give liquidsoap a metadata value that holds a NUL.
   Make sure that it writes a valid `TXXX` frame with the description
   `musicindex`. Make sure that AVPlayer gives the description and the text.
2. **The ID3v2.4 check.** `radio.liq` has `id3_version=4` since 2026-10-06.
   Deploy it, and read the tag header of a segment. Make sure that AVPlayer
   still gives `TIT2` as `commonKeyTitle`.
3. **The tagger home.** Decided 2026-10-06: a new repository holds the tagger
   code and its package. The operator names it when it is made. This item
   closes when that repository exists. §Where The Tagger Runs gives the
   requirements.
4. **The pairing.** Decided 2026-10-06: the operator accepted §The Value
   Identity. Relay ADR 0005 and ADR 0012 hold the changes.
5. **The instant routes.** Done 2026-10-06: relay ADR 0004 and ADR 0011 are
   accepted.

## Verification After Implementation

Mechanical:

- A tagger test for each selection rule: a match, a late state, no match, and
  an empty ICY title.
- A tagger test that the JSON has only the named fields, at most 4,096 bytes,
  and `value` = `null` without a paired identity.
- A tagger test that it never builds `value` from a text match.
- A tagger test that it reconnects to the relay with `Last-Event-ID`, and that
  it then holds each state that it missed.
- A producer test that the song line agrees with §The Song Line, also for the
  `null` state.

Visual. A person must examine these items on an iPhone. Report each one as
open until a person completes the check.

- On HLS, the artwork changes when the listener hears the new track.
- On HLS, a pause clears the display when the listener hears the pause.
- A boost during the first seconds of a new track pays the block of the track
  that the listener hears.

## Alternatives Considered

### Image Data In The ID3

Rejected. Liquidsoap repeats the last group at the start of each 2-second
segment. A 100 KB image would add about 100 KB to each segment.

### The Full Live Value In The ID3

Rejected. The HLS stream would not need the relay. But no standard defines a
live value in ID3, so a second payment channel would exist for one app only.

### A Fixed Delay On The VPS With No ICY Match

Rejected. The tagger would put each relay state after a measured delay
between Mixxx and the VPS. That brings back a guessed delay, and it depends on
the open per-transport delay question in `musicindex-live-relay`
`docs/interoperability.md`.

### A Local Encoder With Rich Metadata

Deferred. Liquidsoap on the broadcast machine could replace `butt` and send
Ogg with full comments to the VPS. That join is the most exact. It needs new
software on the broadcast machine and a new producer path. Use it if the VPS
join is not exact enough.

### The `butt` Song URL

Rejected for the same reasons as ADR 0008 §The Song URL Of `butt`. `butt`
also cannot send a different ICY `StreamUrl` for each track.

## Consequences

Positive:

- On HLS, the app gets the artwork and the value identity at the time that the
  listener hears the track. It needs no text match.
- A boost can pay the block of the track that plays. This removes the risk
  near a track change that §Context describes.
- The song line has an owner.
- ICY listeners and podcast apps see no change. The compatibility path stays
  as it is.

Negative and risks:

- The tagger is a new process with a relay subscription. While it is down,
  HLS has only `TIT2`, and the app uses its fallback.
- §The Value Identity changes three repositories.
- A state that arrives late gives a late frame.
- The compatibility path keeps its timing problem on HLS. The publisher holds
  each payload for one stream delay for each target. An HLS listener hears a
  track later than an ICY listener. A podcast app that plays the HLS form
  thus changes the value block before its listener hears the track. Two relay
  events, one for each form with its own target delay, can correct this. That
  is a separate decision for `v4vmm` and this repository.
- On the compatibility path, a podcast app shows artwork only for a V4V track
  with an `image` URL. The dead block of ADR 0005 has no image.

## References

- `docs/adr/0002-nowplaying-drop-file-contract.md`
- `docs/adr/0008-display-path.md`
- `docs/adr/0010-live-value-payload-reference-shape.md`
- `docs/adr/0012-pair-display-state-with-value-block.md`
- `musicindex-live-relay`: `docs/adr/0005-display-song-line-and-value.md`
- `musicindex-live-relay`: `docs/adr/0003-display-state-and-artwork.md`
- `musicindex-live-relay`: `docs/interoperability.md`
- `citizenradio`: `docs/adr/0011-icy-sync-of-the-relay-display.md`
- The test stack: `~/build/musicindex-stream-test/README.md`
- The research report: `~/build/musicindex-research/reports/Live RSS metadata delivery paths.md`
- podcast-namespace discussion #547, "Live Value Updates":
  https://github.com/Podcastindex-org/podcast-namespace/discussions/547
