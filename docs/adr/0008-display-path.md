# ADR 0008: An Optional Display Path For Artwork

Status: Accepted
Date: 2026-10-04

Accepted 2026-10-04 by the operator. The items in §Before Acceptance are
done.

`musicindex-live-relay` ADR 0003 owns the relay routes, their limits and their
storage. This ADR decides what the producer and the publisher send.

## Context

The operator runs a private stream that plays V4V tracks and other tracks. A
private app plays that stream. It shows:

- the Icecast title, which `butt` reads from `now-playing.txt` and sends in
  band with the audio,
- the live value, from the relay, only while a V4V track plays,
- the artwork, which no path carries today.

Podcast apps read the `image` URL in the live value. That path does not change.

These facts constrain the design:

- The producer writes a drop file only for a V4V track. A track that pays
  nobody has no drop file, so its artwork cannot ride the drop file.
- The artwork must change when the listener hears the track. The live value
  waits for the stream delay of its target. The artwork must wait for the
  same delay.
- In the connector mode, the `Coordinator` links only a V4V row, because only
  that row can pay. A track that pays nobody has no link, so the producer
  cannot see its deck stop.
- The producer deletes `now-playing.txt` at startup, and it leaves the file
  with the last title when it stops. Tested on 2026-10-04 with `butt` 1.46.0:
  an empty file made `butt` send an empty song name, and the next title line
  came through at once. A deleted file gave no update, and `butt` kept the
  last title. So after the producer stops, the stream shows a title that does
  not play.
- `%t`, the runtime directory of the user units, is a tmpfs. A file there uses
  memory.
- A survey of the operator's library on 2026-10-04 found 29,171 audio files.
  28,347 have embedded art. The median image is 60 KiB and 500 pixels.
  - 514 images (1.8 %) are larger than 512 KiB.
  - 709 images (2.5 %) have a side longer than 1,000 pixels. The longest side
    is 4,000 pixels.
  - Four images are 10,143 KiB at only 1,600 pixels. The largest image is
    32,442 KiB.
  - JPEG is 28,260 images, PNG 84 and GIF 3.

## Decision

### Two Paths

The payment path does not change. That path is the drop file, its schema
`musicindex.nowplaying/1`, the live value, and the rules of ADR 0005, ADR 0006
and ADR 0007.

The display path is optional. It is off unless the operator turns it on. With
it off, no display file and no display request exist.

### The Display Link

In the connector mode, the producer keeps a display link adjacent to the
payment link of ADR 0006. The display link applies to each history row, V4V or
not. It follows the rules of the payment link:

- the loudest deck at the row,
- the stop and the resume,
- the new load,
- the entry into the connector mode,
- the relink after an outage.

The payment link does not change. It still applies only to a V4V row.

In the history-only mode, the display track is the latest history row, until
the next row. That mode has no stop.

### The Display State

The display state is `null` or one track:

- **A track** is present while the display link is on a deck that plays, or,
  in the history-only mode, while a history row is present.
- **`null`** applies at startup, while no display link exists, while the
  linked deck does not play, and when the producer stops. At a stop, the
  producer writes the `null` state before it exits.

The artist and the title come from the history row, the same source as
`now-playing.txt`. The display state holds them as two raw fields. The format
rules of `now-playing.txt`, for example the hyphen removal, do not apply.

### The Song File For `butt`

- The producer writes `now-playing.txt` with no text when the display state is
  `null`. It never deletes the file.
- At startup, it writes the file with no text. It does not delete it.
- When a track starts or resumes, it writes the title line again.

These rules apply also when the display path is off, because they concern the
Icecast title of every stream.

### The Producer Output

`mixxx-now-playing --display-dir DIR` turns on the display output. `DIR` must
not be the drop directory, because the publisher reads each JSON file in the
drop directory as a drop file.

The producer writes `DIR/display.json`:

```json
{
  "schema": "musicindex.display/1",
  "track": {
    "artist": "Artist",
    "title": "Title",
    "artwork": { "sha256": "<64 hex characters>", "mime": "image/jpeg" }
  }
}
```

- `track` is `null` for the `null` display state.
- `artwork` has one of three forms:
  - `{"url": "https://…"}` for an image that the app loads from its own host.
    See §The Artwork Source.
  - `{"sha256": "…", "mime": "…"}` for an embedded image that the producer
    writes into `DIR`.
  - `null` when the track has no usable image, when the file cannot be read,
    or when its drive is not mounted. That is a normal state, not an error.

The image goes into `DIR/<sha256>.jpg` or `DIR/<sha256>.png`. The producer
writes the image before `display.json`, and it writes each file to a temporary
file and renames it (AGENTS.md §6). It keeps the image of the present display
state and the image of the state before it. It deletes every other image in
`DIR`.

### The Artwork Source

- **A V4V track** is a track that the drop file rules count as V4V. If its
  `TXXX:MusicIndex Image` tag holds an `http` or `https` URL of at most 2,048
  characters, the artwork is that URL. It is the same value as `image` in the
  drop file. The producer does not read, check or change the image at that
  URL, so an animated image stays animated.
- A V4V track with no such URL uses its embedded image, by the rules of
  §The Embedded Image.
- **Any other track** uses its embedded image, by the same rules.

The survey found three embedded GIF images. One of them is in a V4V track of
the operator, and it is 32,442 KiB. That track has an image URL, so it does
not use the embedded image.

### The Embedded Image

- The producer takes the front cover. If the file has none, it takes the
  first picture.
- It accepts JPEG or PNG only, by the first bytes of the data, not by the
  type text in the tag.
- It rejects image data larger than 64 MiB without a further read.
- Before it decodes an image, it reads the pixel size from the image header.
  It rejects an image whose header it cannot read, or whose longest side is
  more than 4,000 pixels. The decoded image then uses at most 64 MB.
- An image of 524,288 bytes or less, with no side longer than 1,000 pixels,
  goes out unchanged.
- The producer reduces every other image to 1,000 pixels on its longest side
  and writes it as JPEG. If the result is larger than 524,288 bytes, the track
  has no image.
- The size is the cause of the reduction, not the pixel count alone. The
  survey found images of 10 MB at 1,600 pixels.

### The Publisher

A target turns on the display path with `display_dir`. The value is the
`DIR` of its producer.

- The publisher watches `DIR`. When `display.json` changes, it reads the file
  and, for an embedded image, the image bytes at once, because the producer can delete the image
  before the stream delay ends.
- It holds the display state and the bytes in the stream-delay schedule of the
  target. It releases them in order with the payloads of that target.
- At the release, it uploads an embedded image that the relay does not hold
  yet, and then it publishes the display state (relay ADR 0003). A URL needs
  no upload. It does not upload one
  image two times in one process.
- A display request never delays a payload or a keepalive. It waits behind
  them.
- A display failure gives a `tracing::warn!` line. It is never fatal. A relay
  that answers `404` or `409 event_not_reserved` turns off the display path
  for that target until the next start, with one warning.
- When the producer is missing (ADR 0005), the publisher publishes the display
  state `null`.

### Memory

- Producer: one embedded picture during a tag read, and the decoded image
  during a reduction. Two image files in `DIR`.
- Publisher: the display entries inside one stream delay, at most 300 seconds.
- Relay: the bound in relay ADR 0003.

## Invariants

- The payment path does not change when the display path is on or off.
- A display request never delays a payload or a keepalive.
- The producer never deletes `now-playing.txt`. A `null` display state gives a
  file with no text.
- No image larger than the relay limit leaves the producer.
- `DIR` holds at most two images.
- The producer never fetches an image from a URL.
- The display state and the payload of one target pass through the same
  stream delay.

## Before Acceptance

1. **The survey.** Done on 2026-10-04. A rejection would remove the artwork
   of about one track in 55, so the producer reduces large images. See
   §Context and §The Embedded Image.
2. **The `butt` check.** Done on 2026-10-04. See §Context.
3. **Relay ADR 0003** is accepted. Done on 2026-10-04.

## Verification After Implementation

Mechanical:

- A `Coordinator` test for each display link rule, for a V4V row and for a
  row that is not V4V.
- A test that the payment link of a row that is not V4V does not exist, with
  the display path on.
- A test that a `null` display state writes `now-playing.txt` with no text,
  and that startup does not delete the file.
- A test that `display.json` is written after its image, and that a third
  image deletes the first.
- A test that a V4V track with an image URL gives `{"url": …}` and writes no
  image file, that a V4V track with no URL uses its embedded image, and that a
  URL that is not `http` or `https` is not used.
- A test for each embedded image rule: no picture, a type that is not JPEG or PNG, a
  header that cannot be read, a side over 4,000 pixels, an image that goes out
  unchanged, and a reduction of a large image to 1,000 pixels and at most
  524,288 bytes.
- A test that the producer writes `now-playing.txt` with no text when it
  stops.
- A publisher test that a display entry waits for the stream delay, that it
  does not delay a payload, and that an image is uploaded once.

Visual:

- With the private app, the artwork changes when the listener hears the new
  track, for a V4V track and for a track that pays nobody.
- A pause clears the artwork and the Icecast title. A resume gives both back.

## Alternatives Considered

### Artwork In The Drop File

Rejected. A track that pays nobody has no drop file. A field change also needs
a new schema version for every producer.

### Artwork In The Live Value

Rejected. Podcast apps read the live value, and it exists only for a payable
track. An image in it would grow the payload past the relay limit.

### The Song URL Of `butt`

Rejected. `butt` polls a URL at an interval, and its behavior for an empty or
failed response is not documented. A title from the relay would also wait for
the stream delay two times: once in the publisher, and once more between the
encoder and the listener.

### An HTTP Server In The Publisher

Rejected. The private app already reads the relay. A server in the publisher
adds a network surface to a headless service.

### Reduction In The Publisher

Rejected. The producer reads the tags already, and a reduction there keeps the
large data out of the tmpfs and out of the publisher.

## Consequences

Positive:

- The private app gets artwork for every track, at the time the listener hears
  it.
- Podcast apps see no change.
- A pause clears the Icecast title, and a producer restart no longer leaves a
  stale title in `butt`.

Negative and risks:

- The producer gains an image library for the header read and the
  reduction. That is a new dependency, and `Cargo.lock` grows.
- The connector mode keeps two links.
- The display path needs a reserved event on the relay.

## References

- `docs/adr/0002-nowplaying-drop-file-contract.md`
- `docs/adr/0005-producer-liveness-and-dead-block.md`
- `docs/adr/0006-mixxx-midi-connector.md`
- `musicindex-live-relay`: `docs/adr/0001-reserved-live-items.md`
- `musicindex-live-relay`: `docs/adr/0003-display-state-and-artwork.md`
- `butt` 1.46.0: `/usr/share/doc/butt/ChangeLog` and `README`
