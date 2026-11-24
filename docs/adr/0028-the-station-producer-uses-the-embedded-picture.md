# ADR 0028: The Station Producer Uses The Embedded Picture Of The File As The Artwork Fallback

- Status: Accepted on 2026-11-24 by the operator, as at commit ca460a2. The orchestrator session relayed the decision.
  The two Open Questions stay open. The check of §Before Implementation is open.
- Date: 2026-11-24
- Changes: ADR 0026 §What The Producer Writes In `display.json`, the `artwork` row of the track table and the item
  "The producer writes no embedded image". ADR 0026 §One Display Writer, the sentence that keeps `prepare_image` and
  `embedded_artwork` in `mixxx-now-playing` and the crate `image` out of `now-playing-drop`.
- Changes: ADR 0025 §The State File Format 2, by one optional field `picture` and one picture file in the state
  folder.
- Uses: ADR 0008 §The Artwork Source and §The Embedded Image, without a change.
- Uses: ADR 0022, without a change. The operator choice "the producer does not read the media files" stays. The slot
  text reads the picture, and the producer reads only the state folder.
- Bar: crosses modules. The slot text, `liquidsoap-now-playing`, `now-playing-drop` and `mixxx-now-playing` must agree
  on the picture file and on the one picture component. An agent can also make the opposite choice by mistake: read the
  media file in the producer, or copy `prepare_image` into the station producer.
- Operator choice, 2026-11-24, relayed by the orchestrator session: option "A, then B as fallback". A is the image URL
  of `TXXX:Podcast Image` (a musicindex ADR for `station-import`). B is this record: the embedded picture of the file
  when the state has no usable URL.

## Context

The orchestrator session examined the test station on 2026-11-24:

- 19 of the 68 station files have an embedded picture. An example is
  `Songs/8ee8ceec-c0bc-41ed-a80d-5f9aa10aa10c.mp3`, "Once I Loved" by Bacalao, with an `mjpeg` attached picture. The
  display state of that song had `"artwork": null`.
- No file has `TXXX:Podcast Image`, so the state field `image` is empty for each file.
- The cause is ADR 0026: "The producer writes no embedded image." The publisher and the relay operate correctly.

The state file has no path of the media file. The path is in the Liquidsoap metadata key `filename`, and it is a path
in the container. ADR 0022 §Alternatives Considered rejected a producer that reads the media files. That needs a second
bind mount, and the path is a container path. The slot text operates in the container, so it can read the file at
that path.

These facts come from Liquidsoap at tag `v2.4.6` (the version of the test station, ADR 0022) and at commit `c222ba2`,
in `musicindex-research/sources/broadcast-audio/streaming/liquidsoap`:

1. `file.metadata.id3v2(fname)` gives the ID3v2 frames of a file. Each `apic` value goes to `string.apic.parse`,
   which gives `mime`, `picture_type` and the data (`src/core/builtins/builtins_string_extra.ml`, lines 125-151).
2. `file.metadata.flac.cover.decode(s)` decodes a FLAC `metadata_block_picture` value. It gives the data with
   `picture_type` and `mime` (`src/libs/file.liq`, lines 279-330 at `v2.4.6`).
3. `metadata.cover` and `file.cover` take the first picture. They do not select by the picture type
   (`src/libs/metadata.liq`, lines 101-183). The slot text thus does not use them.
4. `file.metadata.flac` is defined only when Liquidsoap can read FLAC tags. Else it gives an empty list
   (`src/libs/file.liq`, line 271).

Inference: `file.metadata.flac(fname)` gives each picture block of a FLAC file as a `metadata_block_picture` value.
The check of §Before Implementation examines it on the test station.

Terms:

- The **picture file** is the file in the state folder that holds the bytes of the selected embedded picture of the
  current track. This is a project term.
- The **state folder** is `musicindex_dir` of the slot text, the bind mount of ADR 0022 that holds `state.json`. This
  is a project term.

## Decision

### The Artwork Sequence

The station producer gives the artwork of a track by the first rule that applies:

1. The `image` of the state, when `image_url` accepts it (ADR 0026, without a change). The producer then does not read
   the picture file.
2. The picture file of the state, when `prepare_image` accepts its bytes (ADR 0008 §The Embedded Image).
3. Else `null`.

- Each segment of an episode file has the artwork of the file. A chapter image is not in scope.
- A jingle has the artwork of the last song, because it keeps the `picture` of the last song (§The Slot Text Writes The
  Picture File).
- The station producer uses the same artwork rules for a V4V track and for a different track.

### The State File Format 2 Gets `picture`

| Field | Type | Use in the producer |
| --- | --- | --- |
| `picture` | string | The name of the picture file in the state folder. Empty when the track has no selected picture. |

- The field is optional. A missing field is the same as an empty field. The format version does not change (ADR 0026
  §The State File Format 2 Gets `song`).
- The producer accepts a name of the form `picture-` and 1 to 20 ASCII digits. Each other name gives no picture. The
  name thus cannot point out of the state folder.
- The state field gives no path of the media file. The producer never gets or reads a media file path.

### The Slot Text Writes The Picture File

The slot text selects the picture and writes the bytes. It does not change, reduce or decode the image.

- It reads the file at the metadata key `filename` of the current track.
- An MP3 file: it reads `file.metadata.id3v2(filename)` and parses each `apic` value with `string.apic.parse`.
- A FLAC file: it reads `file.metadata.flac(filename)` and decodes each `metadata_block_picture` value with
  `file.metadata.flac.cover.decode`.
- It takes the front cover (`picture_type` 3). If the file has none, it takes the first picture (ADR 0008 §The
  Embedded Image).
- It writes no picture of more than 64 MiB (67,108,864 bytes). The track then has an empty `picture`.
- It writes `picture-SEQ` in the state folder, where `SEQ` is the `seq` of the state. It writes the file atomically,
  as `state.json`.
- It reads and writes the picture in the write thread of `musicindex_write`, not in the `on_track` or `on_metadata`
  handler. A slow file read thus does not stop the audio.
- It reads the picture one time for each `seq`. It writes the state with the new `picture` after the picture file.
- After it writes that state, it removes the picture file of the last track.
- A jingle (`jingle` is `true`) gets no new picture. It keeps the `picture` of the last track.
- An empty `filename`, a file of a different format, a file with no picture, or a read failure gives an empty
  `picture`. A read failure writes a log line at level 2, as the write failure of `state.json`.

### The Producer Reads The Picture File

- The producer reads the picture file only when rule 1 of §The Artwork Sequence does not apply.
- It rejects a file of more than 64 MiB without a read of its content (ADR 0008 §The Embedded Image).
- It prepares the picture one time for each `seq`. Each segment of the file uses that result. A picture file that the
  slot text removed after the first read does not change the artwork of the next segment.
- A missing or unreadable picture file gives the artwork `null`. This is not an error, and the producer
  writes no line at level `warn`.
- The display writer of `now-playing-drop` writes the image into the display folder, as for Mixxx (ADR 0008 §The
  Producer Output). This part does not change.

### One Picture Component

The parts that prepare an embedded picture move from `mixxx-now-playing/src/display.rs` to `now-playing-drop`. The two
producers use them.

- The moved parts are `sniff_image`, `prepare_image`, its helper functions, `Rejection`, the four limit constants,
  `embedded_artwork` and `v4v_artwork`.
- `v4v_artwork` is the one function for the sequence "URL, else picture". The station producer calls it. It can get a
  general name in the move.
- `now-playing-drop` gets the crate `image` with the features `jpeg` and `png`, as `mixxx-now-playing` has now. The
  publisher does not depend on `now-playing-drop`, so the publisher does not get the crate.
- `file_artwork` and `read_tags_with_picture` stay in `mixxx-now-playing`. They read the media file with `lofty`, and
  only Mixxx reads its media files.
- No copy of a moved part stays. The tests of the moved parts move with the code.

### Formats

- MP3 with ID3v2.3 or ID3v2.4 `APIC` frames, and FLAC with `PICTURE` blocks, give a picture.
- Each other format gives an empty `picture`. The station files of `station-import` are MP3 or FLAC (musicindex
  ADR 0094 and ADR 0095).
- `prepare_image` accepts JPEG and PNG only, by the first bytes (ADR 0008). An `mjpeg` attached picture of an MP3 file
  is a JPEG in an `APIC` frame.

## Invariants

These rules apply while this decision is current.

- The station producer never reads a media file. Only the slot text reads it.
- A valid `image` URL of the state wins over the picture file.
- One component prepares an embedded picture for the two producers.

## Before Implementation

On the test station, a person or an agent with access runs the slot text functions in the Liquidsoap of AzuraCast:

1. `file.metadata.id3v2` on `Songs/8ee8ceec-c0bc-41ed-a80d-5f9aa10aa10c.mp3` gives an `apic` value, and
   `string.apic.parse` gives `image/jpeg` and its `picture_type`.
2. `file.metadata.flac` on a FLAC file with a picture gives a `metadata_block_picture` value. This check proves the
   inference of §Context. If it fails, the implementation stops and reports. The FLAC part then needs a new decision.

## Verification

Mechanical, in `liquidsoap-now-playing` with `tempfile` and an injected clock:

- A state with a valid `image` URL and a valid picture file gives the artwork `{"url": ...}`.
- A state with an empty `image` and a JPEG picture file gives an image artwork with the `sha256` of the prepared bytes,
  and the image file is in the display folder.
- A state with an `image` that `image_url` refuses and a PNG picture file gives an image artwork.
- A state with no `picture`, with a missing picture file, or with a picture file of a GIF gives the artwork `null`.
- A `picture` of `../state.json`, of `picture-`, or of `picture-12a` gives the artwork `null`.
- A picture file of more than 64 MiB gives the artwork `null`. The test uses a sparse file.
- A chapter sequence with a picture file gives the same image artwork for each segment. The test removes the picture
  file after the first segment.
- A jingle state with the `picture` of the last song gives that image artwork.
- A format 2 state without the field `picture` parses and gives the artwork of rule 1 or `null`.

Mechanical, in `now-playing-drop` and `mixxx-now-playing`: the tests of the moved parts pass in `now-playing-drop`. The
tests of `mixxx-now-playing/tests/display.rs` pass without a change of an assertion.

Manual, in the last review of the diff: the moved tests keep their assertions, and no copy of a moved part stays. A test
cannot compare a test with its earlier version or find a copied function.

Manual, on the test station `heycitizens_test_radio`, with a person. A test in this repository cannot run the slot
text, because CI has no Liquidsoap:

1. `Songs/8ee8ceec-c0bc-41ed-a80d-5f9aa10aa10c.mp3` plays. The relay display state
   (`GET /v1/liveitems/EVENT_ID/display`) has an image artwork, and the app shows the picture.
2. A FLAC file with a picture plays. The display state has an image artwork.
3. A file with no picture plays. The display state has the artwork `null`, and the state folder holds no picture file
   of an earlier track.
4. A jingle plays after file 1. The display state has the image artwork of file 1.

## Open Questions

1. **Memory on the test host.** `prepare_image` can use 64 MB to decode one image (ADR 0008 §The Embedded Image). The
   test host has approximately 340 MiB of free memory (ADR 0022). The station check examines it.
2. **Other formats.** An Ogg or M4A file gives no picture. No station file has these formats at the date of this
   record.

## Alternatives Considered

### The Producer Reads The Media File

Rejected. The state gets `filename`, and the producer reads the picture with `lofty`, as Mixxx. The operator rejected
this in ADR 0022: it needs a second bind mount, and the path is a container path.

### `file.cover` In The Slot Text

Rejected. It is one line of slot text, but it takes the first picture and not the front cover. ADR 0008 §The Embedded
Image needs the front cover.

### The Picture In `state.json` As Base64

Rejected. The slot text writes `state.json` each second. A picture of some megabytes in each write costs disk writes and
a large parse each 500 milliseconds in the producer.

### A New Crate For The Picture Code

Rejected. `now-playing-drop` already holds the display writer of the two producers, and only the producers depend on
it. A new crate adds a member and gives no protection.

## Consequences

- A station file with an embedded front cover or picture gives artwork with no `TXXX:Podcast Image`.
- The slot text gets the picture part, and the operator installs the new slot text on the station.
- `liquidsoap-now-playing` gets the crate `image` through `now-playing-drop`, so its build takes more time.

## References

- `docs/adr/0008-display-path.md`
- `docs/adr/0022-liquidsoap-producer.md`
- `docs/adr/0025-the-station-producer-pays-each-chapter-song.md`
- `docs/adr/0026-the-station-producer-writes-the-display-state.md`
- `liquidsoap/musicindex-state.liq`
- `mixxx-now-playing/src/display.rs`
- `now-playing-drop/src/display.rs`
