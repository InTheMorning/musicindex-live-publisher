# ADR 0036: The HLS Stream Carries The Picture One Time For Each Song

- Status: Proposed
- Date: 2026-11-25
- Bar: crosses repositories. The rule changes the slot text of this repository and `radio.liq` of
  `musicindex-stream-test`. An agent can also make the opposite choice by mistake, because ADR 0009 forbids image data
  in an ID3 frame.
- Changes: ADR 0009 §Invariants, the item "No image data goes into an ID3 frame". An `APIC` frame with the picture of
  the song can go into the first HLS segment of each song (§The Picture Frame). Each other frame rule of ADR 0009 stays.
- Changes: ADR 0035 §The Slot Text Writes The Picture File. The slot text also gives the picture to the HLS output
  (§The Station).
- Operator choice, 2026-11-25: "If possible, embed the art into the HLS stream as a fallback." The station stream and
  the VPS stack both carry the picture.
- Operator choice, 2026-11-25: one `APIC` frame for each song, only in the first segment of the song.
- Operator choice, 2026-11-25: change the Liquidsoap setting of §The Liquidsoap Settings. Test it first on the VPS
  stack, then on the station.

## Context

Terms:

- **HLS** is HTTP Live Streaming, an established Apple protocol. The audio comes in segments of 4 seconds on the two
  streams.
- **`APIC`** is the established ID3v2 frame for an attached picture. Its body is the text encoding byte, the MIME type
  and a NUL, the picture type, the description and a NUL, then the image bytes.
- A **cover key** is a metadata key in the Liquidsoap list `settings.encoder.metadata.cover`. This is a Liquidsoap
  term.

Podcast apps get the art of a live track from the payload `image` (ADR 0034). A player that reads the ID3 frames of the
HLS stream gets the art at the time that the listener hears the song. The operator asked for the picture in the
stream as a fallback.

These facts come from the Liquidsoap source at tag `v2.4.6`, the version of the station, in `musicindex-research`
(`sources/broadcast-audio/streaming/liquidsoap`). The tag `v2.5.0` has the same code at different lines.

1. The HLS output writes one ID3 tag at the start of each segment. The tag holds only the latest metadata
   (`src/core/base/outputs/hls_output.ml`, lines 776-787).
2. With `replay_id3`, which is true by default, each later segment repeats that metadata (line 782).
3. A new metadata ends the current segment at the next frame, so its tag starts a new segment (lines 1069-1074).
   A second metadata before that cut replaces the first one, and the first one is never written (lines 1142-1147).
4. `output.file.hls` exports metadata with `export_cover_metadata:false` (line 619). The export removes each key whose
   name in lower case is a cover key (`src/core/base/stream/metadata_base.ml`, lines 51-62). The default cover keys
   are `pic`, `apic`, `metadata_block_picture` and `cover` (`src/core/base/encoder/encoder_formats.ml`, line 33).
   With the default settings, a picture thus never gets to an HLS segment.
5. The export compares the key in lower case, but it keeps the key as it is. The ID3 writer makes a frame only for
   two types of key: a frame name in upper case, or a known lower-case name such as `title`. A key `APIC` gives an
   `APIC` frame. A key `apic` gives no frame (`Utils.id3v2_of_metadata`).
6. The writer writes `APIC` as a text frame: the encoding byte 3 (UTF-8), then the value without a change. The value
   must thus hold the rest of the `APIC` body.
7. AzuraCast writes the station HLS with `output.file.hls` and `%ffmpeg(format="mpegts")`, with no `id3_version`. The
   default is ID3v2.3, which does not give a meaning to the encoding byte 3. The VPS stack sets `id3_version = 4`.
8. The reference slot text already adds `txxx` to `settings.encoder.metadata.export` for the frame of ADR 0009.

Inference, not tested: no source examined shows that AVPlayer, or a podcast app, shows an `APIC` picture of a live HLS
stream.

## Decision

### The Picture Frame

- The frame is `APIC`, from the metadata key `APIC` in upper case.
- The value of the key starts with the MIME type (`image/jpeg` or `image/png`) and a NUL. Then it has the picture
  type 3 (front cover) as one byte, an empty description and a NUL. The image bytes follow without a change. The
  writer adds the encoding byte.
- The frame goes only into the first segment of the song. Each later segment of the song has the text frames and no
  `APIC`.
- A picture of more than 512 KiB (524,288 bytes) gives no frame. The slot text cannot make an image smaller.

### One Time For Each Song

The writer of the picture sends two metadata changes for each song:

1. At the start of the song: the metadata of the song with the key `APIC`.
2. At least 1 second later: the same metadata with no `APIC`.

- With `replay_id3`, each later segment then repeats the second metadata, with no picture.
- The interval of 1 second makes sure that the first metadata cut a segment before the second one comes (§Context
  fact 3). Each song thus has one short segment of approximately 1 second.
- The two changes have the same `title` and `artist`, so the ICY title does not change.

### The Liquidsoap Settings

The script that makes the HLS output sets these values before each output:

- `settings.encoder.metadata.cover` without `apic`. The other cover keys stay.
- `settings.request.metadata_decoders.recode.exclude` with `apic`, so the recode does not change a picture of a file.
- `settings.encoder.metadata.export` with `apic`. It is in the default list. A script that sets its own list must add
  it.

These settings apply to each output of the Liquidsoap instance.

### The VPS Stack

`radio.liq` of `musicindex-stream-test` gets the settings of §The Liquidsoap Settings. The VPS stack reads the Icecast
stream of `butt`, so it has no media file. The picture for a Mixxx song must come from the relay display state, and
the tagger of ADR 0009 does that. Until the tagger exists, the first test of §Before Acceptance uses a fixed test
picture at each ICY title change.

### The Station

The slot text makes the two metadata changes of §One Time For Each Song from the picture that it already selects
(ADR 0035 §The Slot Text Writes The Picture File).

- It inserts the metadata after its own `on_track` and `on_metadata` handlers. The handlers thus do not see the two
  changes, and `seq` does not change.
- The picture is the picture of the file. A chapter of an episode file does not get a new picture in the stream,
  because the chapter image is a URL (ADR 0035 §The Artwork Of A Segment).
- A jingle gets no frame, because it keeps the metadata of the last song.

## Invariants

These rules apply while this decision is current.

- An `APIC` frame is only in the first segment of a song.
- No segment has more than one `APIC` frame.
- The two metadata changes never change the ICY title or `seq`.

## Before Acceptance

1. **The VPS test.** Deploy `radio.liq` with the settings and a fixed JPEG of less than 512 KiB at each ICY title
   change. Read the ID3 tag of the segments with a parser. The first segment after a title change must have one
   `APIC` frame with the correct bytes. The later segments must have `TIT2` and no `APIC`.
2. **AVPlayer.** On an iPhone, play the VPS stream in an app that logs `AVPlayerItem` timed metadata. The log must
   show the picture data at the song change. A person does this check.
3. **A podcast app.** Play the VPS stream in each app that the operator tests, and record which app shows the
   picture. If no app shows it, the operator decides if the frame stays.
4. **The station export list.** The Pre-Playlists slot of the test station sets `settings.encoder.metadata.export` to
   an old list (station log). Remove that setting, or add `apic` to it.
5. **The ID3v2.3 frame.** On the station HLS stream, read one `APIC` frame with ffprobe and with the AVPlayer log of
   item 2. Both must give the picture, with the encoding byte 3 in ID3v2.3.
6. **The other outputs.** With the station settings, the MP3 mount keeps the same ICY title, and its bitrate does not
   increase at a song change. The AzuraCast "now playing" data has no picture key.
7. **The station handlers.** Play two songs. `seq` in `state.json` changes one time for each song. It changes two
   times when the metadata comes after the track mark (ADR 0035 §The State File Contract). The two inserted changes
   do not change it.

## Verification After Implementation

Mechanical:

- The slot text has no test runner in this repository. The reference slot text passes `liquidsoap --check` with the
  station version, in the station log.

Manual, with a person:

- Items 1, 2, 5, 6 and 7 of §Before Acceptance, again after each change of the slot text or of `radio.liq`.

## Open Questions

1. **The size limit.** 512 KiB is the proposed value. A segment of 4 seconds at 128 kbit/s is approximately 64 KiB, so
   the first segment of a song can be 9 times larger. A slow phone can stop to load it.
2. **The tagger picture.** The tagger of ADR 0009 must get the picture bytes from the relay display state. Its record
   owns that rule.

## Alternatives Considered

### The Picture In Each Segment

Rejected by the operator on 2026-11-25. With `replay_id3`, a picture of 100 KiB adds 100 KiB to each segment of 4
seconds.

### A URL In The Frame

Not selected. An `APIC` frame with the MIME type `-->` holds a URL, but the same cover rule removes it. A `WXXX` frame
holds a URL, but no player shows it as art (inference).

### `replay_id3 = false`

Not possible on the station. AzuraCast writes the HLS output, and the slot text cannot change its parameters. A
listener that starts in the middle of a song would also get no title until the next song.

## Consequences

- A player that reads `APIC` can show the art of the song at the time that the listener hears it.
- Each song adds one short segment and one larger segment to the stream.
- The settings apply to each output of the instance. §Before Acceptance item 6 examines the effect.

## References

- `docs/adr/0009-hls-track-metadata.md`
- `docs/adr/0035-the-station-producer.md`
- `liquidsoap/musicindex-state.liq`
- `musicindex-stream-test`: `liquidsoap/radio.liq`
- `musicindex-research`: `sources/broadcast-audio/streaming/liquidsoap` (tag `v2.4.6`) and
  `sources/broadcast-audio/streaming/azuracast` (`backend/src/Radio/Backend/Liquidsoap/ConfigWriter.php`)
