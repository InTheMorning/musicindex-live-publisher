# ADR 0041: The Station Producer

- Status: Accepted on 2026-11-26 by the operator. ADR 0035 and ADR 0040 are in `docs/archive/adr/`.
- Date: 2026-11-26
- Changed by: ADR 0042, accepted 2026-11-26. The state gets the optional field `pub_date`.
- Changed by: ADR 0043, accepted 2026-11-26. The route lookup also gets the funding links of the segment.
- Supersedes: ADR 0035 (`docs/archive/adr/0035-the-station-producer.md`). This record restates each live rule of
  ADR 0035 and of its three changes. It adds no rule and removes no rule.
- Supersedes: ADR 0040 (`docs/archive/adr/0040-a-chapter-song-has-the-episode-title-as-its-album.md`).
- Uses: ADR 0036 §The Station and ADR 0038 §The Slot Text Writes The Frames for the HLS frames of the slot text,
  without a change. Those two records own the HLS rules, and they stay current.
- Uses: ADR 0039 for `song_line` and `play_id` in `display.json` and for the pairing, without a change.
- Uses: ADR 0008 (the display path), ADR 0010 (`line` and `podcastName` come from the drop file `album`), ADR 0011
  (the listener delay), ADR 0013 (the album), ADR 0014 (the drop file version 3), ADR 0016 (instances and the producer
  lock), ADR 0017 (the show log), ADR 0021 (the unit names) and ADR 0024 (a payable block needs the two GUIDs), without
  a change.
- Uses: ADR 0034 for the payload image when a segment has no image URL, without a change.
- Uses: `musicindex-live-relay` ADR 0003 (artwork forms) and ADR 0005 (`songLine`), without a change.
- Depends on: musicindex ADR 0093 R-TAG-2 (the frame names) and R-TAG-4 (the chapter list), with R-TAG-6 (the song
  routes in a remote entry). musicindex owns the frame names and the chapter list. This record restates the parts that
  it uses, and the musicindex record wins if the two differ.
- Depends on: musicindex ADR 0099 R-IMAGE-4 and R-IMAGE-5 (commit `263179d`): the field `image_url` of a chapter entry.
- Uses: musicindex ADR 0075 R-VALUE-5 and the crate `musicindex-value` for the percentage of each recipient.
- Operator choice, 2026-11-26: consolidate ADR 0035 before a fourth change (global `CLAUDE.md` §Consolidate A Changed
  Decision).

The operator choices that ADR 0035 and ADR 0040 recorded stay in force:

- 2026-11-21: a bind mount gives the host the state file.
- 2026-11-21: `state.json` holds all keys, so the producer does not read the media files.
- 2026-11-21: the producer and the publisher operate as one user account with lingering.
- 2026-11-22: when a station plays an episode file, the producer pays each chapter song, as the `valueTimeSplit`
  elements of the feed do.
- 2026-11-22: the custom frames get neutral names. Readers do not accept the old `MusicIndex` frame names.
  The operator tags the collection again with the new names.
- 2026-11-22: the producer gets the routes of a remote chapter song from the route service with the two chapter GUIDs
  first, then from `value_routes` of the entry. With neither, the chapter pays nobody for that time, not the episode.
- 2026-11-22: `mixxx-now-playing` asks the route service with the two GUIDs.
- 2026-11-22: a chapter with a `title` that is null or empty gets the state title.
- 2026-11-22: a field of the relay display state lets the app show and pay each chapter song. The ICY title of the
  station does not change.
- 2026-11-22, orchestrator choice from the operator goal "show and pay each chapter song": each display state of the
  station has the artist and the title of the chapter song. The operator can change it.
- 2026-11-22: no limit on the size of the chapter list until the station check fails.
- 2026-11-23: the producer gets the routes of a song file from the route service. The producer selects the lookup of
  the VTS routes or the lookup of the whole track.
- 2026-11-24: option "A, then B as fallback". A is the image URL of `TXXX:Podcast Image`. B is the embedded picture of
  the file when the state has no usable URL.
- 2026-11-25: the art of a chapter song comes from the chapter image first. The image of the file comes second, and
  the relay copy of the picture (ADR 0034) comes third.
- 2026-11-26: while a chapter song plays, Rainbrook shows "Homegrown Hits - Episode 158" below the artist, not
  "Homegrown Hits".
- 2026-11-26: the drop file has the same `album`, so the live value payload also has the episode title in `line` and
  `podcastName`.

## What This Record Replaces

ADR 0035 restated ADR 0022 and ADR 0025 to ADR 0028. This record restates ADR 0035 and these three changes:

- ADR 0036: the slot text also gives the picture to the HLS output, one time for each song (§The Slot Text Writes The
  Picture File).
- ADR 0038: the slot text also writes a segment frame at each chapter boundary (§The Slot Text Writes The Picture File
  and §Invariants).
- ADR 0040: a chapter with a title has the state title as its `album` (§The Album Of A Segment).

The archived records hold the history, the rejected alternatives and the station results of each rule. Read them
for research only.

## Context

A station on AzuraCast plays song files and episode files from Liquidsoap 2.4.6 in a container. A slot text in the
Pre-Broadcast slot writes `state.json` into a bind mount. The producer `liquidsoap-now-playing` on the host reads that
file and writes the drop file and the display file for the publisher.

An episode file of a V4V music show holds songs of other artists. The feed pays each song with a
`podcast:valueTimeSplit` element. The file holds that list in `TXXX:Podcast Chapters`. Since musicindex ADR 0099, each
entry can also hold the image URL of its song.

Terms:

- **Lingering** is the systemd function that keeps the user services of an account in operation with no login. This is
  an established systemd term.
- The **state file** is `state.json`, which the slot text writes. This is a project term.
- The **state folder** is `musicindex_dir` of the slot text, the bind mount that holds `state.json`. This is a project
  term.
- **Fresh** means that the value of `written` in the state file changed in the last 5 seconds. This is a project term.
- A **chapter** is one entry of the list in `TXXX:Podcast Chapters`. It is one `valueTimeSplit` of the feed. This is a
  project term.
- A **segment** is a part of an episode file with one drop file: one chapter, or one part with no chapter. This is a
  project term.
- The **position** is the time from the start of the file to the moment of the state, in milliseconds. This is a
  project term.
- A **group** is the song routes or the episode routes in a merge. This is a project term.
- The **route service** is the MusicIndex API. The Mixxx producer uses the same service (ADR 0024). This is a project
  term.
- The **picture file** is the file in the state folder that holds the bytes of the selected embedded picture of the
  current track. This is a project term.
- The **song line** is the ICY title that the station sends for a file. This is a project term.

## Decision

### Name And Crates

- The producer is `liquidsoap-now-playing`, a member crate of this workspace. It reads the state of Liquidsoap, so it
  also operates for a Liquidsoap script without AzuraCast.
- The crate `now-playing-drop` holds the producer lock, the drop file writer, the read of the instance configuration,
  the route client, the display writer and the picture component. The two producers use it. No producer has a changed
  copy of a part of it.
- The Mixxx lock of ADR 0021 stays in `mixxx-now-playing`. A host can operate one Liquidsoap producer for each
  station.

### The State File Contract

This repository owns the state format and the reference slot text `liquidsoap/musicindex-state.liq`. The research
tutorial restates it and names this record as the owner.

The slot text writes the format `musicindex-liquidsoap-state/2`. The producer reads format 1 and format 2, and no
other format.

| Field | Format | Type | Use in the producer |
| --- | --- | --- | --- |
| `format` | 1, 2 | string | `musicindex-liquidsoap-state/1` or `musicindex-liquidsoap-state/2`. |
| `seq` | 1, 2 | integer | Changes at each track. Starts at 0 at each start of Liquidsoap. |
| `written` | 1, 2 | number | The heartbeat. |
| `live` | 1, 2 | boolean | `true` while a live DJ is on air. |
| `jingle` | 1, 2 | boolean | `true` while a jingle plays in AzuraCast jingle mode (§Jingle Mode). |
| `track_started` | 1, 2 | number | With `seq`, it makes `play_id`. |
| `title`, `artist`, `album` | 1, 2 | string | The drop file fields of the same name. |
| `track_guid`, `feed_guid`, `image` | 1, 2 | string | The drop file fields of the same name. `image` is the URL of `TXXX:Podcast Image`. |
| `value_routes` | 1, 2 | string | JSON text in a JSON string. The producer parses it a second time. |
| `description`, `contributors` | 1, 2 | string | Not used. The drop file has no field for them. |
| `elapsed` | 2 | number | `radio.elapsed()` at the write, in seconds. |
| `cue_in` | 2 | number | The cue-in point in seconds, from the metadata key `liq_cue_in`. 0 when the key is empty. |
| `chapters` | 2 | string | The text of `TXXX:Podcast Chapters`, JSON text in a JSON string. Empty when the file has no list. |
| `song` | 2, optional | string | `m["song"]`. Empty when the metadata has no key `song`. |
| `picture` | 2, optional | string | The name of the picture file. Empty when the track has no selected picture. |

- An empty string is a missing value. The producer writes `null` for it.
- A different `format` gives an error in the log and no drop file. The producer does not guess a version.
- A new required field needs a new format version. An optional field does not. A missing optional field is the same
  as an empty field. The reference slot text and the producer change in the same commit.
- `play_id` comes from `track_started` and `seq`. A restart of Liquidsoap thus gives a new `play_id`.
- When the metadata of a track comes after the track mark, `seq` changes one more time in that track.
  `track_started` keeps the time of the track mark, and `play_id` stays unique. Do not remove this second change:
  without it, the producer keeps the empty metadata of the track mark for the full track.
- `duration_secs` is `null`. The next track or the heartbeat ends the play.
- Format 1 has no chapters, no `song` and no `picture`. A format 1 file is one track.

### The Frame Names

musicindex ADR 0093 R-TAG-2 gives the names. The slot text and `mixxx-now-playing` read only these frames:

| Field | Frame |
| --- | --- |
| Feed GUID | `TXXX:Podcast Feed Guid` |
| Item GUID | `TXXX:Podcast Item Guid` |
| Value routes | `TXXX:Podcast Value Routes` |
| Image | `TXXX:Podcast Image` |
| Contributors | `TXXX:Podcast Contributors` |
| Description | `COMM:Podcast Description` |
| Chapters | `TXXX:Podcast Chapters` |

- The readers do not read the old `MusicIndex` frames. A file with only old frames has no GUID, so it pays nobody
  (ADR 0024).
- The slot text reads the keys in lower case, for example `podcast item guid`. The recode exclude list has
  `podcast value routes` and `podcast chapters`, and no old key.
- The names of the drop file fields and of the state fields do not change. `track_guid` holds the item GUID.

### Jingle Mode

AzuraCast jingle mode gives a jingle the metadata of the previous track, with its `rid`. Liquidsoap gives each new
request a new `rid`.

- The slot text sets `jingle` to `true` when `rid` is not empty and is equal to the `rid` of the last track change.
  A live DJ track has an empty `rid`, so it never sets `jingle`.
- For a jingle, the producer writes the `jingle` row of §What The Producer Writes. The publisher then sends the dead
  block. The drop file does not name the previous track, so the show log does not record a second play of it.
- The test of 2026-11-22 with AzuraCast 0.24 and Liquidsoap 2.4.6 passed. A MusicIndex station can use jingle mode.

### What The Producer Writes

The producer reads the state file each 500 milliseconds. It writes the drop file version 3 of ADR 0014. The first row
that applies wins:

| State | Producer lock | Drop file |
| --- | --- | --- |
| Not fresh, or missing, or not readable | Not held | No file |
| Fresh, `live` is `true` | Not held | No file |
| Fresh, `jingle` is `true` | Held | Empty `artist`, `title` and `value_routes`. Each other field is `null`. |
| Fresh, an episode state (§The Route Lookup) | Held | One drop file for each segment (§What The Producer Writes For An Episode File). |
| Fresh, a song state | Held | The routes of §The Song Routes. |
| Fresh, each other state | Held | The routes of the state when `feed_guid`, `track_guid` and routes are present (ADR 0024). Else empty `value_routes`. |

- The producer removes the drop file before it releases the lock. The publisher then gives the dead block one time and
  no keepalive (ADR 0029).
- The producer takes the lock again with the wait limit of ADR 0016 §The Producer Lock.
- A state file that the producer cannot parse does not change the drop file. The producer reads it again. A file that
  stays bad stops the heartbeat, and the first row then applies.
- `value_routes` that is not valid JSON gives empty routes and an error in the log. The producer never makes a route
  from a title or a file name.
- `value_routes_source` is `liquidsoap-state` when the routes are not empty.

### The Chapter List

The producer parses `chapters` as a JSON array. Each entry has these fields (musicindex ADR 0093 R-TAG-4 and ADR 0099
R-IMAGE-4):

| Field | Type | Use in the producer |
| --- | --- | --- |
| `start_ms` | integer | The start of the chapter in the file. |
| `end_ms` | integer | The end of the chapter in the file. |
| `title` | string or null | The drop file `title` of the chapter. |
| `artist` | string or null | The drop file `artist` of the chapter. |
| `feed_guid` | string or null | The feed GUID of the remote song. |
| `item_guid` | string or null | The item GUID of the remote song. |
| `remote_percentage` | number | The part of the payment for the remote song. |
| `value_routes` | array or null | A local split: the local recipients. A remote split: the song routes from the file, or null. |
| `image_url` | string or null, optional | The image URL of the song of a remote split (§The Artwork Of A Segment). |

- An entry with `end_ms` not above `start_ms` is not valid.
- A missing `remote_percentage` is 100. A finite value below 0 is 0, and a finite value above 100 is 100. A value
  that is not a finite number makes the entry not valid.
- An entry with no `feed_guid` and no `item_guid` is a **local split**. Each other entry is a **remote split**.
- The producer ignores a not valid entry. It writes an error in the log one time for each `seq`.
- A `chapters` text that is not a valid JSON array gives no chapters. The file is then one track with the episode
  GUIDs and routes. The producer writes an error in the log one time for each `seq`.
- A GUID that is empty or has only spaces is a missing GUID.
- A missing `image_url` is the same as null. An `image_url` that is not a string, or that `image_url` of
  `now-playing-drop` refuses, is the same as null. It does not make the entry not valid.
- The producer does not refuse an entry with a field that it does not know (musicindex ADR 0099 R-IMAGE-5 item 3).

### The Position Rule

- The position is `round(1000 * (elapsed + cue_in))` in milliseconds. A value of `elapsed` or `cue_in` that is not a
  finite number, or a negative position, gives no chapter: the file is then one track for that read.
- The valid entries make the segments in the order of `start_ms`. An entry is active from its `start_ms` to its
  `end_ms`, or to the `start_ms` of the next entry if that is earlier. Two entries with the same `start_ms`: the first
  in the list wins, and the producer ignores the second.
- The current chapter is the active entry with `start ≤ position < end`. When no entry is active, the episode applies.
- The track of the last `on_track` callback owns the time. During a crossfade, the next track thus owns the overlap.
- The producer adds no delay for the listener. The relay holds each block for the stream delay (ADR 0011).
- `elapsed` can be 1 second old at the read, and the producer reads the file each 500 milliseconds. A chapter change
  can thus come to the publisher approximately 1.5 seconds late.

### What The Producer Writes For An Episode File

The **episode group** is usable when the state has the two episode GUIDs and routes with a percentage. A remote split
is **identified** when the entry has the two GUIDs and a `remote_percentage` above 0. The **song group** of an
identified split is usable when the song routes of §The Route Lookup have a percentage.

| Segment | Identity in the drop file | Routes |
| --- | --- | --- |
| No chapter: before the first chapter, after the last chapter, in a gap, or a file with no list | The episode GUIDs | The episode routes |
| An identified remote split, the song group usable | The chapter GUIDs | §The Merged Routes |
| An identified remote split, no song routes | The chapter GUIDs | No routes: the publisher sends the dead block |
| A remote split that is not identified | The episode GUIDs | The episode routes |
| A local split | The episode GUIDs | §The Local Split |

- At each change of segment, the producer writes a new drop file, the same as at a track change.
- The producer applies ADR 0024: routes only with a feed GUID and an item GUID in the identity of the drop file, and
  episode routes only when the episode group is usable.
- `title` is the chapter title when it is a string that is not empty, and the state title otherwise. `artist` is
  the chapter artist when it is a string that is not empty, and the state artist otherwise. `album` follows §The Album
  Of A Segment. `image` follows §The Artwork Of A Segment.
- `play_id` is `ls-<track_started in ms>-<seq>` for a file with no valid chapter. For a file with chapters, it is
  `ls-<track_started in ms>-<seq>-<segment start in ms>`. Each segment thus gets its own line in the show log
  (ADR 0017).
- `duration_secs` stays `null`.

### The Album Of A Segment

The slot text writes the title tag and the album tag of the file into the state. For an episode file, the title is
the episode title, for example "Homegrown Hits - Episode 158", and the album is the show, for example "Homegrown Hits".
While a chapter song plays, the title line shows the chapter title, so the album line shows the episode title.

| Segment | `album` in the drop file |
| --- | --- |
| A chapter with a title that is not empty, and a state `title` that is not empty | The state `title` |
| A chapter with an empty title or no title | The state `album` |
| A chapter with a title, and an empty state `title` | The state `album` |
| A gap, a file with no chapters | The state `album` |

- A jingle drop file has no `album` (§What The Producer Writes). The jingle display track has the state `album`.
- Each other display track has the `album` of the drop file of the same read, the same as `artist` and `title`.
- An empty `album` is `null` in the drop file and in `display.json`.
- A chapter with no title shows the state title in the title line. Its `album` stays the state album, so the two lines
  do not show the same text.
- The publisher puts the drop file `album` into the live value payload as `line[0]` and `podcastName` (ADR 0010).

### The Merged Routes

The value block of the relay has one list of destinations. The producer makes that list from the song group and the
episode group, with the namespace rules:

1. `percentages` of `musicindex-value` gives the percentage of each episode route in the episode block (musicindex
   ADR 0075 R-VALUE-5). `F` is the total of the percentages of the episode fee routes.
2. Each episode fee route keeps its percentage.
3. `R = 100 - F` is the part after the fees. The song group gets `remote_percentage * R / 100`. The episode routes
   with no fee get `(100 - remote_percentage) * R / 100`.
4. In each part, `percentages` divides the part between the routes of that part. In the song group, a fee route of the
   song is one more route of that part.
5. When the episode group is not usable, `F` is 0 and the song group gets 100. When the episode group has no route
   with no fee, the song group gets `R`.
6. A route with no percentage, and a route with a split that rounds to 0, is not in the list. The fee flag and the
   other fields of a route do not change.
7. Each split is rounded to 3 decimal places.
8. Example: the episode routes Alice 95 and a fee of 5, `remote_percentage` 90, and two song routes of 50 each. The
   splits are 42.75 and 42.75 for the song, 9.5 for Alice and 5 for the fee.

### The Local Split

A local split replaces the episode recipients for its time. `remote_percentage` does not apply.

- The episode group must be usable, because the identity is the episode. If it is not usable, the segment has no
  routes.
- The episode fee routes keep their percentage `F`. The local recipients get `R = 100 - F`, divided with
  `percentages`.
- The episode routes with no fee get nothing for that time.

### The Route Lookup

For a fresh state with `live` and `jingle` `false`, the producer selects one lookup:

1. The state has a chapter list: the producer looks up the routes of each identified remote split. This is an
   **episode state**.
2. The state has no chapter list, and the two state GUIDs are not empty: the producer looks up the routes of the whole
   track with the two state GUIDs. This is a **song state**.
3. Else the producer looks up nothing.

A state has no chapter list when its format is 1, or when its list is empty, missing or not valid.

- **The request.** `GET <endpoint>/v1/feeds/<feed_guid>/tracks/<item_guid>?include=payment_routes`. The routes are
  `data.payment_routes` of the answer. The producer never asks with one GUID, and it never uses the value block of the
  feed in place of the track.
- **The endpoint.** The option `--route-service URL`. The default is `https://api.musicindex.org`, the default of the
  Mixxx producer.
- **The time of the lookup.** The producer asks when it first reads a new `seq`. For an episode state, one worker
  thread sends a request for each identified remote split, one at a time, in the order of `start_ms`. Each request has
  a time limit of 5 seconds.
- **The cache.** The cache holds one result for each pair of GUIDs of the current file. At a new `seq`, the producer
  keeps the results of the pairs that are also in the new state, and removes the other results. A result is the
  routes, or "no routes" for an answer 404 or an answer with an empty route list.
- **No answer.** A time-out, a network error, an answer that the producer cannot parse, or an HTTP error other than
  404 is not a result. The producer asks again after 30 seconds, while the file plays. It writes one `warn` line for
  each pair and each `seq`.
- **A late answer.** When a result comes while its segment or its song plays, the producer writes the drop file again
  with the new routes. The `play_id` does not change.
- The producer sends no request for a jingle, a `live` state, a local split, a remote split that is not identified, a
  state with one GUID, or the state GUIDs of an episode state.

The song routes of an identified remote split, in this order:

1. The routes of the service result, when they have a percentage.
2. Else `value_routes` of the entry, when they have a percentage. This applies while no result has come, after a 404
   and after an empty route list.
3. Else no song routes. The chapter pays nobody for that time.

### The Song Routes

The routes of a song state, in this order:

1. The routes of the service result, when they have a percentage.
2. Else `value_routes` of the state, when they have a percentage.
3. Else empty `value_routes`. The publisher then gives the dead block (ADR 0024).

### The Song Line

This section applies to a mount with ICY metadata. Liquidsoap sends ICY metadata for MP3, AAC, FLAC and WAV, and
not for Ogg (Liquidsoap `icecast2.ml`). The producer makes the song line from the state:

1. `song`, when it is not empty.
2. Else `artist - title`, when `artist` and `title` are not empty.
3. Else `artist`, when it is not empty.
4. Else `title`, when it is not empty.
5. Else no song line.

- This is the rule that Liquidsoap `output.icecast` uses for the ICY title when AzuraCast gives no `icy_song`
  argument (Liquidsoap `icecast2.ml`, `icy_song`).
- The producer uses the strings of the state without a change. It does not trim them and does not change the case.
- The song line is a property of the file. Each chapter of the file has the song line of the file, not the line of the
  chapter song.

### What The Producer Writes In `display.json`

The producer writes `display.json` with the schema `musicindex.display/3` of ADR 0013 in the display directory of its
target, only when the target has `display_dir`. The path is the folder `display` in the run directory of the instance
(ADR 0021).

The first row that applies wins:

| State | `display.json` |
| --- | --- |
| The producer starts | `{"track": null}` |
| The producer stops | `{"track": null}`, before it exits |
| Not fresh, or missing, or `live` is `true` | `{"track": null}`, one time, when the producer releases the lock |
| Fresh, but the producer cannot parse it | No change |
| No song line, or a song line of more than 1,024 characters | `{"track": null}`. Relay ADR 0005 refuses a longer line. |
| `jingle` is `true` | A track with the artist, the title and the album of the state, the song line of the state, and `play_id` `null` |
| Each other state with a drop file | A track. See the table below. |

| Field | Value |
| --- | --- |
| `artist` | The `artist` of the drop file of the same segment. |
| `title` | The `title` of the drop file of the same segment. |
| `album` | The `album` of the drop file of the same segment (§The Album Of A Segment). |
| `artwork` | §The Artwork Of A Segment. |
| `song_line` | The song line of the file. |
| `play_id` | The `play_id` of the drop file of the same segment. |

- A song file without chapters gives one track for the file.
- An episode file with chapters gives one track for each segment. Each track has the artist and the title of its
  segment, and the song line of the file.
- A jingle has `play_id` `null`, so the publisher adds no `value` (ADR 0012). After an episode file, the jingle track
  shows the artist and the title of the episode. The show log gets a `display` entry for this track. It has `play_id`
  `null` and is not a play (§Jingle Mode).
- An identified remote split with no song routes shows the chapter song and pays nobody. Its drop file gives the dead
  block, which has `play_id` `null`, so the display state gets no `value`.
- The producer writes `display.json` in the same read as the drop file, after the drop file.
- A late answer of the route service changes the drop file with the same `play_id`. The display file does not change.
  When the first payload was the dead block, the publisher sends the display state again, one time, with `value`
  (ADR 0012).
- The station producer writes a track only while it holds the producer lock.

### The Artwork Of A Segment

The image of a segment has two forms. The drop file `image` must be a URL. The display `artwork` can also be a
picture. The rules for the drop file `image`:

1. A segment of a remote split with an `image_url`: that URL (musicindex ADR 0099 R-IMAGE-5 item 2). A remote split
   is identified or not identified.
2. Else the `image` of the state, when `image_url` of `now-playing-drop` accepts it.
3. Else `null`. The publisher then uses the display artwork of the same play (ADR 0034).

The rules for the display `artwork`. The first rule that applies wins:

1. The `image_url` of the remote split of the segment, as `{"url": ...}`.
2. The `image` of the state, as `{"url": ...}`, when `image_url` of `now-playing-drop` accepts it: HTTP or HTTPS, and
   at most 2,048 characters (relay ADR 0003). The producer then does not read the picture file.
3. The picture file of the state, when `prepare_image` accepts its bytes (ADR 0008 §The Embedded Image).
4. Else `null`.

- A local split, a segment with no chapter, and a song file use rules 2 to 4.
- The producer uses the same artwork rules for a V4V track and for a different track.
- A jingle has the artwork of the last song, because it keeps the `picture` of the last song.

### The Slot Text Writes The Picture File

The slot text selects the picture and writes the bytes. It does not change, reduce or decode the image.

- It reads the file at the metadata key `filename` of the current track.
- An MP3 file: it reads `file.metadata.id3v2(filename)` and parses each `apic` value with `string.apic.parse`.
  Liquidsoap keeps one value for each metadata key, so an MP3 file with more than one `APIC` frame gives one frame.
- A FLAC file: it reads `file.metadata.flac_native(filename)`. It separates the `metadata_block_picture` value into
  its picture blocks by the lengths in each block, and skips the separator `", "` between two blocks. It decodes each
  block with `file.metadata.flac.cover.decode`.
- It takes the front cover (`picture_type` 3). If the file has none, it takes the first picture.
- It writes no picture of more than 64 MiB (67,108,864 bytes, counted with `string.bytes.length`). The track then has
  an empty `picture`.
- It writes `picture-SEQ` in the state folder, where `SEQ` is the `seq` of the state. It writes the file atomically,
  as `state.json`.
- It reads and writes the picture in the write thread of `musicindex_write`, not in the `on_track` or `on_metadata`
  handler. A slow file read thus does not stop the audio.
- It reads the picture one time for each `seq`. It writes the state with the new `picture` after the picture file.
- If a new track starts during the picture read, it does not write the state of that cycle.
- After it writes that state, it removes the picture files of the earlier tracks. A failed write keeps them in the
  list for the next write.
- A jingle gets no new picture. It keeps the `picture` of the last track. A track mark with no metadata also keeps it.
- An empty `filename`, a file of a different format, a file with no picture, or a read failure gives an empty
  `picture`. A read failure writes a log line at level 2.
- The slot text also gives the picture to the HLS output, one time for each song. ADR 0036 §The Station owns that
  rule.
- The slot text also writes a segment frame `musicindex.hls/2` at the start of each track and at each chapter
  boundary. ADR 0038 §The Slot Text Writes The Frames owns that rule.

### The Producer Reads The Picture File

- The producer accepts a `picture` name of the form `picture-` and 1 to 20 ASCII digits. Each other name gives no
  picture. The name thus cannot point out of the state folder.
- The producer reads the picture file only when rules 1 and 2 of the display `artwork` do not apply.
- It rejects a file of more than 64 MiB without a read of its content.
- It prepares the picture one time for each track. The key is `seq`, `track_started` and `picture`, because `seq`
  starts again at 0 after a restart of Liquidsoap. The producer clears the result when it releases the lock.
- Each segment of the file uses that result. A picture file that the slot text removed after the first read does not
  change the artwork of the next segment.
- A missing or unreadable picture file gives the artwork `null`. This is not an error, and the producer writes no line
  at level `warn`.
- The display writer of `now-playing-drop` writes the image into the display folder (ADR 0008 §The Producer Output).

### One Display Writer And One Picture Component

- `now-playing-drop` holds the display writer: `DisplayOutput`, `render_display_json`, `ShownTrack`, `Artwork`,
  `image_url`, the image types and `write_null_at_exit`. `ShownTrack.play_id` is a string.
- `now-playing-drop` holds the picture component: `sniff_image`, `prepare_image`, its helper functions, `Rejection`,
  the four limit constants, `embedded_artwork` and the function for the sequence "URL, else picture". It has the crate
  `image` with the features `jpeg` and `png`.
- `file_artwork` and `read_tags_with_picture` stay in `mixxx-now-playing`. They read the media file with `lofty`, and
  only Mixxx reads its media files.
- The publisher does not depend on `now-playing-drop`.

### The Producer Stays In Operation

The producer does not exit when Liquidsoap stops. It releases the lock and continues to read the state file. The lock
gives the publisher the same fact. The producer exits only on an error that a restart cannot correct, for example a
lock wait longer than the limit.

### The Template Unit

- The package installs the user unit `liquidsoap-now-playing@.service`. It pairs with
  `musicindex-live-publisher@%i.service` (ADR 0016 §Producer Units).
- The instance name comes from the short name of the station. It follows the name rule of ADR 0021 §The Unit Names,
  `^[a-z0-9][a-z0-9-]{0,31}$`. The instance name changes each `_` of an AzuraCast short name to `-`. Example: the short
  name `heycitizens_test_radio` gives the instance `heycitizens-test-radio`.
- The state folder and `musicindex_dir` in the slot text use the instance name, not the short name.
- The unit gives `--instance %i --state-file /var/lib/musicindex/azuracast/%i/state.json`. A station without
  AzuraCast changes the path with a drop-in file.
- The unit sets `PrivateDevices=true` and `RestrictAddressFamilies=AF_INET AF_INET6 AF_UNIX`. The producer sends
  HTTPS requests to the route service.
- One account with lingering owns the producer and the publisher units on the server. The runbook gives the steps.
- The station runbook gives `display_dir` to the target:
  `display_dir = "/run/user/UID/musicindex-live-publisher/INSTANCE/display"`.

### The Mixxx Producer

`mixxx-now-playing` reads the frames of §The Frame Names with the shared tags crate of `musicindex-lib`. It has no
frame list and no reader of its own. It does not read the chapter list. It pays an episode file
as one track with the episode GUIDs and routes. It asks the route service with the two GUIDs. A chapter rule for Mixxx
needs its own record.

### The Publisher And The Relay

- The publisher reads `display.json` of the target and adds `songLine` and `value` (ADR 0012).
- Relay ADR 0005 accepts `songLine` of 1 to 1,024 characters.
- The relay accepts an `album` of at most 1,024 characters (relay ADR 0006).

## Invariants

These rules apply while this decision is current.

- The producer holds the lock only while the state is fresh and `live` is `false`.
- The producer never reads a media file, and never makes a GUID or a route from a title or a file name. Only the slot
  text reads a media file.
- An unknown `format` gives no drop file.
- A drop file with routes has a feed GUID and an item GUID (ADR 0024), for a chapter also.
- Episode routes are in a drop file only when the state has the two episode GUIDs.
- The episode fees come before the split, in each segment.
- The total of the splits is 100, within the rounding of §The Merged Routes item 7, or the list is empty.
- The producer asks the route service only with two GUIDs, and only with the path of the pair.
- An identified remote split with no song routes pays nobody. It never pays the episode in place of the song.
- A song state with service routes and with routes in the file pays the service routes.
- Each display state of a file has the song line of that file. A chapter state never has the line of its chapter
  song. The producer never changes the ICY title.
- A display state has the `play_id` of the drop file of the same segment, or `null` for a jingle.
- The producer writes `display.json` after the drop file of the same read.
- A chapter `image_url` wins over the image of the file. A valid `image` URL of the state wins over the picture file.
- One component prepares an embedded picture for the two producers.
- The slot text never decides which chapter entry is valid. The producer owns that decision (ADR 0038).
- A display track that is not a jingle has the `album` of the drop file of the same read.
- The producer does not read `album` from the chapter list.

## Verification

Mechanical, in `liquidsoap-now-playing` with `tempfile` and an injected clock. The tests exist for each restated rule,
and they stay green:

- Each row of §What The Producer Writes, the second parse of `value_routes`, an unknown format and a stale heartbeat.
- A chapter sequence through three chapters, a gap and the end of the file: the identity, the routes, the title and
  the `play_id` of each drop file. The position at the limits of a chapter with a cue-in of 12.5 seconds. Overlapping
  entries and entries with the same `start_ms`. Remote splits with a missing, an empty and a space GUID. A local split
  with `remote_percentage` 0, 50 and 100. A chapter with `artist` and `title` null.
- The file routes and no routes of an identified remote split with no service result.
- The route lookup with a local HTTP server: the request path, an answer with routes, 404, an empty list, a time-out
  with the second request after 30 seconds, a late answer with the same `play_id`, and the cache at a new `seq`.
- A song state of format 1 and of format 2 gives one request with the two state GUIDs. A jingle, a `live` state, a
  state with one GUID and an episode state send no request for the state GUIDs.
- The display state of each segment, the song line rules, a jingle, `live`, no `display_dir`, a format 2 state without
  `song` or `picture`, and a song line of 1,025 characters.
- The artwork rules: a valid URL, a JPEG and a PNG picture file, a refused URL with a picture, a missing picture, a GIF,
  the bad names `../state.json`, `picture-` and `picture-12a`, a sparse file of more than 64 MiB, the same artwork for
  each segment after the picture file is removed, and a jingle.

Mechanical, in `now-playing-drop`: the merge example of §The Merged Routes item 8, the limits of `remote_percentage`,
the share cases and a part that rounds to 0. The tests of the display writer and of the picture component.

Mechanical, for §The Artwork Of A Segment:

- A remote split with an `image_url` gives that URL in the drop file `image` and in the display `artwork`. A state
  `image` that `image_url` refuses gives the drop file `image` `null`. This is
  also true when the state has an `image` and a picture file.
- These values of `image_url` give the drop file `image` of the state and the display `artwork` of rules 2 to 4:
  null, missing, not a string, an `ftp:` URL and a URL of 2,049 characters.
- A local split with an `image_url` gives the image of the state. A chapter entry with an unknown field is valid.

Mechanical, for §The Album Of A Segment, in `liquidsoap-now-playing/src/lib.rs`:

- `a_chapter_with_a_title_has_the_episode_title_as_the_album`: chapters, a gap and the end, in the drop file and in
  `display.json`.
- `a_chapter_with_no_title_or_no_episode_title_has_the_state_album`.
- `display_song_file_without_chapters_gives_one_state` and `display_jingle_has_the_state_names_and_no_play_id`.
- `assert_track_of_drop` compares the display `album` with the drop file `album` in each display test.

Manual, in the last review of the diff: no copy of a moved part stays. A test cannot find a copied function.

Manual, on the test station `heycitizens_test_radio`, with a person. A test here cannot start AzuraCast:

1. The reference slot text passes the save in AzuraCast. A `TXXX:Podcast Chapters` frame of approximately 4 KiB comes
   into `state.json` without a change.
2. A tagged episode file plays. The relay shows each chapter song at its time, with the merged routes. The ICY title
   equals the `songLine` of each chapter state.
3. The time error at a chapter change, with and without a crossfade.
4. A file with a cue-in point gives the correct `cue_in`, and the chapter change comes at the correct time.
5. The station host gets the routes of a chapter song from `https://api.musicindex.org`.
6. An imported song with no routes in the file gives a payable block with the routes of the route service. Passed on
   2026-11-23.
7. Files with an embedded picture, a FLAC file with a picture, a file with no picture, and a jingle after a picture
   give the correct display artwork. Passed on 2026-11-24 on the relay display state. The app was not examined.
8. After a new run of `station-import`, an episode file plays. Each chapter song with an image in its feed shows that
   image on the relay display state and in the payload `image`.
9. While a chapter song of an episode plays, Rainbrook shows the episode title below the artist. Station log step 54
   passed for the files, and step 56 passed in Rainbrook. The iOS lock screen shows no album line (step 57), so it is
   not a part of this check.

HitIt and `station-import` must write the new frames in all the files first. Before that, the operator does not
install the new slot text or the new Mixxx reader on a station.

## Open Questions

1. **The time to the listener.** The relay applies the stream delay (ADR 0011). The delay for the MP3 mount and for
   HLS is not measured. The relay sends a display state at once, so the app can show a chapter song early by the
   stream buffer (inference).
2. **A seek in the file.** Inference: AzuraCast does not seek a file on air. If `elapsed` follows a seek, the position
   rule stays correct.
3. **A chapter song that MusicIndex does not know.** The operator said on 2026-11-22 that such a song implies a nested
   podcast. The file routes or no routes apply until a later record.
4. **The null state at live.** The DJ publisher of ADR 0023 can send a display state on the same event. A null state
   from the station can come after the state of the DJ and remove it. The station check of ADR 0023 with a DJ
   publisher examines it.
5. **The character set of the station.** Inference: with a character set that is not UTF-8, the ICY title does not
   equal `songLine` for a character that the set does not have.
6. **Metadata in the middle of a track.** AzuraCast can add metadata during a track. The ICY title then changes, and
   `state.json` does not.
7. **Memory on the test host.** `prepare_image` can use 64 MB to decode one image. The test host has approximately
   340 MiB of free memory.
8. **Other formats.** An Ogg or M4A file gives no picture.

## Alternatives Considered

The archived records give the rejected alternatives of each restated rule. For the chapter image rule:

### The Chapter Image Only In The Display State

Rejected. Podcast apps read only the payload `image` (ADR 0034 §Context). The operator asked for the chapter art in
the client apps.

### The Producer Looks Up The Chapter Image In The Route Service

Not selected. The tag writer already reads the song feed for the chapter entry (musicindex ADR 0099), and the route
service answer has no image field in this chain.

## Consequences

- One record holds the station producer. The reading path loses ADR 0035 and ADR 0040.
- A chapter song with an image in its feed shows that image in the apps, after a new run of `station-import`.
- The payment metadata of a chapter song names the episode in `podcastName`.
- Code comments and documents that cite ADR 0035 or ADR 0040 for a live rule cite this record. The section names did
  not change.

## References

- `liquidsoap/musicindex-state.liq`
- `liquidsoap-now-playing/src/`, `now-playing-drop/src/`
- `docs/runbooks/musicindex-producer-tutorial.md`
- musicindex ADR 0093, ADR 0099 and ADR 0075
- `musicindex-research/sources/podcast-namespace/docs/tags/value-time-split.md`
- `musicindex-research/sources/broadcast-audio/streaming` (Liquidsoap and AzuraCast sources)
