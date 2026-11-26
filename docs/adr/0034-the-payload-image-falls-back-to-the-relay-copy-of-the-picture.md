# ADR 0034: The Payload Image Falls Back To The Relay Copy Of The Picture

- Status: Accepted on 2026-11-26 by the operator.
- Date: 2026-11-25
- Changes: ADR 0010, the item "`image` stays the artwork URL of the drop file".
- Changes: ADR 0011 §Send At Once, item 1. A track block waits at most 2 seconds for the picture of its play
  (§The First Send Waits For The Picture).
- Uses: ADR 0008 §The Embedded Image and ADR 0035 §The Artwork Of A Segment, without a change. ADR 0012 for the pairing
  of a display state with its payload by `play_id`. ADR 0011 for the listener delay.
- Uses: `musicindex-live-relay` ADR 0003, the routes `PUT` and `GET /v1/liveitems/{event_id}/artwork/{sha256}`.
- Depends on: a `musicindex-live-relay` record that keeps each image that a pending listener update names
  (§Before Acceptance, item 1).
- Bar: crosses repositories. The payload names an image that the relay holds, so the relay retention and this
  publisher must agree. An agent can also make the opposite choice by mistake: put the image bytes or a `data:` URL
  in the payload.
- Operator choice, 2026-11-25: the payload art comes from the chapter image, then from the image of the file. "As an
  extra fallback, consider hosting the album art on the relay as we do for non-v4v tracks." This record is that
  extra fallback. The chapter image needs a musicindex record and a consolidated station producer record first.
- Operator choice, 2026-11-25: the first send of a play waits at most 2 seconds for the picture, then goes out with
  the relay URL. If the picture is not available, the first send has no image, and the second send stays as the fallback.
  The cause: v4vmusic.com and Podcast Guru do not show the image of a second send (station log step 44).

## Context

Podcast apps get the art of a live track from the `image` field of the `remoteValue` payload (ADR 0010). The Split
Kit shows `block.image` (`thesplitkit/src/routes/(main)/live/[guid]/+page.svelte`, line 225). The reference server
changes `image` at each track (`musicindex-research`, report "Live RSS metadata delivery paths", addendum of
2026-10-06). An `image` must be a URL that the app can load.

The publisher copies `image` from the drop file (`src/livevalue.rs`, `payload_from_dropfile`). A producer writes it
only from an image URL: the `Image` tag in Mixxx, or the `image` of the station state. On 2026-11-24, 19 of the 68
station files had an embedded picture, and no file had an image URL (archived ADR 0028 §Context).

The apps read `image` in different procedures (station log step 44, 2026-11-25):

- v4vmusic.com reads `image` only from the first send of a block. It ignores the image of a second send with the same
  `blockGuid` until the page loads again.
- Podcast Guru does not change its picture during a live stream.
- CitizenRadio reads the `artwork` of the relay display state, not the payload `image`.

Thus a second send does not put the picture in these apps. The picture must be in the first send.

The display path already sends the embedded picture to the relay (ADR 0008, ADR 0035). For a target with
`display_dir`, the publisher uploads the image with `PUT /v1/liveitems/{event_id}/artwork/{sha256}`, then sends the
display state with `artwork.sha256` (`src/relay.rs`, `send_display`). This occurs for each track, payable or not. The
read route needs no token, and it sends `Cache-Control: public, max-age=31536000, immutable`
(`musicindex-live-relay/src/lib.rs`, `read_artwork`).

The relay keeps the images in memory only. It keeps at most two images for each event (`MAX_EVENT_IMAGES`). It keeps
the image of the present display state and of the state before it. A restart or the end of the lease removes all
images (`musicindex-live-relay/src/lib.rs`, `store_image` and `clear_display_after_lease`). The relay sends the
payload to the apps after the listener delay. Inference: when two tracks with pictures start during the delay, the
relay can remove an image before the apps get the payload that names it.

Terms:

- The **relay copy** is the image that the publisher uploaded to the relay for the display state of one play. This is
  a project term.
- The **relay URL** is `{endpoint}/v1/liveitems/{event_id}/artwork/{sha256}`, with `endpoint` and `event_id` of the
  target configuration. This is a project term.

## Decision

### The Payload Image Sequence

The publisher sets `image` of a track payload by the first rule that applies:

1. The `image` of the drop file, when it is not empty. No change from ADR 0010.
2. The `artwork.url` of the display state of the same play (ADR 0012 pairing by `play_id`).
3. The relay URL of the relay copy of the same play, after the relay accepted the upload with `200` or `201`.
4. Else no `image`.

- Rules 2 and 3 apply only to a target with `display_dir`. A target without `display_dir` uploads no image.
- The dead block does not change. Its image is the setting `dead_block_image` (ADR 0029, ADR 0033).
- The publisher never puts image bytes or a `data:` URL in the payload.

### The First Send Waits For The Picture

The first send of a track payload waits until the display worker of its target completes its work on the same play. The
longest wait is 2 seconds (`PAYLOAD_IMAGE_WAIT` in `src/relay.rs`).

- The wait applies only when all of these conditions occur: the target has `display_dir`, the payload has a
  `play_id`, the drop file gives no image, and the payload is not a dead block.
- The display worker completes its work on a play when the relay accepted the display state, or when it drops or disables
  the display path. It then immediately gives rule 2, rule 3 or no image to the payload worker.
- When the display worker completed its work on the play before the payload arrived, the payload goes out immediately.
- A newer payload during the wait replaces the waiting payload. The newer payload starts a new wait.
- At the end of the wait, the payload goes out with no image. The publisher writes a log line.
- A payload that arrives while the worker retries a publish or a keepalive does not wait. The second send is its
  fallback.
- The sequence is thus the upload, the display state, then the payload. Relay ADR 0011 keeps the image from the time
  that it accepts the payload (§The Relay Retention).

### The Second Send

The second send is the fallback when the picture was not available at the end of the wait.

- When rule 2 or rule 3 gives an image after the first send of a payload, the publisher sends that payload one more
  time. The second send has the same `eventGuid`, the same `blockGuid` and the same `value`. Only `image` is new.
- The publisher makes the second send one time for each play, and only while that play is the newest payload of the
  target.
- The second send adds no line to the show log (ADR 0017). The show log keeps the reference to the art.
- A lease end, or a foreign block, removes the images of the event in the relay. The republish of the last payload
  then has no relay URL. When the display resend uploads the picture again, the play gets one more second send.

### The Relay Retention

The relay URL operates only while the relay holds the image. The relay must keep each image that a pending listener
update names, until the apps got that update. `musicindex-live-relay` ADR 0011 owns that rule (commit `e4ac9c7`).
This record does not state the relay limits. Two facts of relay ADR 0011 apply to the publisher:

- The relay keeps an image only when the top-level `image` string of the payload ends with
  `/v1/liveitems/{event_id}/artwork/{sha256}`. The event must be the same, and the hash has 64 lowercase hex
  characters. A query or other
  text after the hash keeps no image.
- The relay keeps the image from the time that it accepts the payload that names it. Before that, only the display
  state keeps it. The order of this record (the upload, the display state, then the second send) thus keeps the
  image.

## Invariants

These rules apply while this decision is current.

- A payload `image` is an `http` or `https` URL, or the payload has no `image`.
- The second send never changes `blockGuid`, `eventGuid` or `value`.
- A track payload does not wait more than `PAYLOAD_IMAGE_WAIT` for its picture.
- A drop file image URL always wins over the display state and the relay copy.

## Before Acceptance

1. **The relay retention.** Done 2026-11-25: `musicindex-live-relay` ADR 0011, commit `e4ac9c7`. The relay on
   `api.musicindex.org` has it since the operator deploy of 2026-11-25 (Mixxx VPS log step S2).
2. **The second send in the apps.** On the test station, send a payload with no `image`, then the same `blockGuid`
   with an `image`. Two podcast apps that the operator uses must show the image. They must not show a new track, and a boost
   must pay the same block. A person does this check, because the apps are not in a test. Partly done 2026-11-25
   (station log step 43): the payload had the relay copy URL, and v4vmusic.com and Podcast Guru showed the image.
   A boost of 100 sats from v4vmusic.com went to the node key of the song (`02ebb6cc…`), with the custom record
   of the block. Open: a person sees that the apps show no new track at the second send.
3. **The public endpoint.** Make sure that the `endpoint` of each station target is the public HTTPS URL of the
   relay, not a local address. An app must load the relay URL. Done 2026-11-25 for the test station, station log
   step 39. The one target has `https://api.musicindex.org`. From outside the station, an artwork GET of a hash that
   the relay does not hold gives `404 artwork_not_found`. A GET with 200 was not possible: each station file has an
   image URL, so the relay held no uploaded picture of the event.

## Verification After Implementation

Mechanical. The tests are in the section "The payload image" at the end of `tests/display.rs`.

A payload image that arrives while the payload of the same play waits for a retry goes into that retry. A payload image
that arrives during a keepalive retry goes out after that retry. A payload that the main loop sends after the worker
examines its channel goes out immediately after the second send. The publisher cannot prevent this sequence.

- A test for each rule of §The Payload Image Sequence in the first send: a drop file URL wins, a display URL, a relay
  copy, and no image.
- A test that the first send goes after the upload and the display state.
- A test that a drop file URL, a dead block and a target without `display_dir` do not wait.
- A test that no artwork and a refused upload release the first send before the end of the wait.
- A test that a display state before the payload gives one send with the image.
- A test that a newer payload replaces a waiting payload.
- A test that a slow upload gives a first send with no image, then a second send. The second send keeps `eventGuid`,
  `blockGuid` and `value`.
- A test that the second send does not occur when a newer payload of the target exists.
- A test that the second send adds no show log line.
- A test that a republish after a lease end has no relay URL, and that a second send follows the new upload.
- A test that the relay URL is the top-level `image` of the payload, in the form of relay ADR 0011. No text follows
  the hash. If `image` moves, the relay stops the retention, and no relay test finds it.

Visual. A person must examine these items in an app. Report each one as open until a person completes the check.

- On the test station, a song with only an embedded picture shows that picture in v4vmusic.com. Done 2026-11-25,
  station log step 43.
- At the next song, the app shows the picture of the next song, not the old one.
- After a jingle, a replay of the same song shows the picture of the song in v4vmusic.com, with no page reload. Done
  2026-11-25 with publisher `2f30e4f`, station log step 46. The picture came 1 to 2 seconds before the audio.

## Alternatives Considered

### Wait For The Display State Before The First Send, With No Limit

Rejected. A slow upload or a relay fault then holds the payment block for an unknown time. A boost in that time pays
the last track. §The First Send Waits For The Picture selects a wait with a limit of 2 seconds.

### Only The Second Send

Rejected 2026-11-25. Before that date, the first send never waited, and the second send gave the image. v4vmusic.com
and Podcast Guru do not show the image of a second send (§Context).

### Image Bytes Or A `data:` URL In The Payload

Rejected. A picture can be up to 512 KiB on the relay. The relay sends the payload again to each app that connects,
and ADR 0010 gives a URL in `image`.

### An Image Host That Is Not The Relay

Not selected. It needs a new service and a new token. The relay already holds the picture for the display path.

## Consequences

- A podcast app shows the embedded picture of a station song with no image URL.
- The payment block of a play with a picture arrives late by the time of the upload, at most 2 seconds. A boost in
  that time pays the last track. The upload usually takes less than one second.
- A relay restart removes the relay copies. A payload from before the restart then names an image that is missing.
  The next track gives a new relay copy.
- The relay serves the image to each app. That adds load on the relay at each track change.
- An app that caches the image keeps it for one year. The path holds the hash of the bytes, so a cached image is
  always correct.

## References

- `docs/adr/0010-live-value-payload-reference-shape.md`
- `docs/adr/0035-the-station-producer.md`
- `musicindex-live-relay/docs/adr/0003-display-state-and-artwork.md`
- `musicindex-research/reports/Live RSS metadata delivery paths.md`
