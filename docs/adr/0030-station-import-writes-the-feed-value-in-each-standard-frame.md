# ADR 0030: station-import Writes The Feed Value In Each Standard Frame

- Status: Accepted on 2026-11-24 by the operator. The orchestrator session relayed the decision.
- Date: 2026-11-24
- Bar: opposite choice by mistake. Task 001 of the station import kept each artist, album and
  title of the file (orchestrator choice of 2026-11-23). Commit d388865 changed only the title, and kept the artist and
  the album of the file.
- Uses, without a change: musicindex ADR 0092 R-STATION-4 item 2 (a song file gets the tags of `write_song_tags`, and an
  episode file gets the tags of `write_episode_tags`). musicindex ADR 0093 R-TAG-3, R-TAG-4 and R-TAG-11. musicindex
  ADR 0096 R-IMAGE-3.
- Operator choice, 2026-11-24: station-import always replaces a standard ID3 frame when the RSS feed gives a value for
  it. The file keeps a frame only when the feed has no value for it.

## Context

On the test station, the file `Songs/8b850466-1a23-4298-b14c-a1099816e9a6.mp3` had the `TIT2` "Ts in face 20OCT2024
XXX_16Bit_Aria_Master_(MTC) (1)". That is a work name of the artist in the source file. The feed item title is
"Tiddies In My Face (Single)". The source is row N10 of the test station log, musicindex-research 7c8785d.

The station shows the frames of the file. The feed is the source that the artist publishes for the listener.

musicindex ADR 0092 R-STATION-4 item 2 names only the tags of the two tag functions: the GUID, route, image and
chapter frames. It does not name the standard frames. station-import gives them to the tag functions as edits. So
this rule is a rule of the publisher, and musicindex ADR 0092 does not change.

## Decision

### The Frame Map

station-import gives these edits to `write_song_tags` and `write_episode_tags`:

| Frame | Feed value of a song | Feed value of an episode |
|---|---|---|
| `TIT2` | The item `title` | The item `title` |
| `TPE1` | The item `itunes:author`, else the channel `itunes:author` | The same rule |
| `TALB` | The channel `title` of the song feed | The channel `title` of the show feed |
| `TPE2` | The channel `itunes:author` of the song feed | The channel `itunes:author` of the show feed |
| `TRCK` | The item `itunes:episode`, else `podcast:episode` | The same rule |
| `TDRC` | The UTC day of the item `pubDate`, as `YYYY-MM-DD` | The same rule |

- An edit replaces the frame of the file. The tag crate writes one frame for each frame ID.
- A frame with no feed value gets no edit. The file keeps its value. An empty text and a track number that is not
  more than zero are not values.
- A chapter title is the title of its song item (musicindex ADR 0093 R-TAG-4 item 2). The chapter artist follows
  task 001 and musicindex ADR 0093 R-TAG-4 item 4. This record does not change them.

### Frames Outside The Map

- `TCON`: the parser of the suite gives no category of an item. The file keeps its genre.
- `APIC`: the feed gives an image URL, not an image. station-import downloads no image. The file keeps its embedded
  picture. The URL goes into `TXXX:Podcast Image` (musicindex ADR 0096).
- `TXXX:Podcast Image` is not a standard frame. musicindex ADR 0096 R-IMAGE-3 item 2 keeps the frame of the file, and
  its Open Point 1 owns a change.
- `TLAN`: the feed gives a language tag, for example `en-us`. `TLAN` needs an ISO 639-2 code. The code map is in HitIt
  (HitIt ADR 0080 §13), not in musicindex-lib. A shared map needs its own task.
- `WOAF` and `WOAR`: the feed gives a list of links. The selection of the page link needs its own rule (HitIt ADR 0080
  §2).
- `TPOS`: the feed gives a season, not a disc number.
- `TLEN`: the tag crate owns it (musicindex ADR 0093 R-TAG-11).

A frame moves into the map only with a change of this record.

### Durable And Situational Rules

Each rule of this record is situational. It describes the station import of this record. Remove it when a record of the
station import supersedes this record.

## Verification

Mechanical, in `station-import/tests/import.rs`:

- `file_frames_get_the_feed_values_and_a_frame_with_no_feed_value_stays`: a file with its own `TPE1`, `TALB`, `TPE2`,
  `TRCK`, `TDRC` and `TCON` gets the feed value of each frame in the map. `TCON` keeps the value of the file.
- `file_with_a_different_title_gets_the_item_title_and_artist`: a file with its own `TIT2` and `TPE1` gets the item
  title and the item author.
- `episode_file_with_a_different_title_gets_the_item_title`: an episode file gets the item title, the channel title
  and the episode number. Each chapter has the title of its song item.

Manual, on the test station: after a new import, the file
`Songs/8b850466-1a23-4298-b14c-a1099816e9a6.mp3` has the `TIT2` "Tiddies In My Face (Single)".

## Alternatives Considered

- **Keep each value of the file (task 001, 2026-11-23).** Rejected. The file can hold a work name of the artist.
- **Replace only `TIT2` (commit d388865).** Rejected by the operator. The artist and the album of the file have the
  same problem.
- **Write the feed values in musicindex ADR 0092.** Rejected. That record names only the frames of the tag functions.
  The standard frames are edits of the caller.

## Consequences

- A station file shows the values of the feed. The file does not keep a value of the source file when the feed gives a value for that frame.
- The 68 station files need a new import before they have the new frames.
- citizenradio and the station producer read the same frames. Their code does not change.

## References

- `docs/tasks/station-import-task-001-station-import-binary.md`, criterion 10
- musicindex `docs/adr/0092-the-publisher-imports-songs-and-episodes-for-the-station.md`
- musicindex `docs/adr/0096-the-station-import-writes-the-image-url-of-each-file.md`
