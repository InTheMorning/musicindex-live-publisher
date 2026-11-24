# ADR 0025: The Station Producer Pays Each Chapter Song Of An Episode File

- Status: Accepted on 2026-11-22 by the operator. The orchestrator session relayed the decision. Open Questions 2
  and 4 stay open. The operator accepted §The Chapter Route Lookup on 2026-11-22, with the operator choices below.
- Date: 2026-11-22
- Changes: ADR 0022 items: §The State File Contract (a format version 2) and §What The Producer Writes (the chapter
  rows).
- Changes: ADR 0022 §The Template Unit, the rule "The producer uses no network". The producer sends HTTPS requests to
  the route service (§The Chapter Route Lookup), so the unit permits `AF_INET` and `AF_INET6`.
- Changed by: ADR 0026, accepted 2026-11-22. The state file format 2 gets the optional field `song`.
- Changed by: ADR 0027, accepted 2026-11-23. §The Chapter Route Lookup also applies to the two GUIDs of a song
  state.
- Changed by: ADR 0028, accepted 2026-11-24. The state file format 2 gets the optional field `picture`, and the
  state folder gets a picture file.
- Uses: ADR 0024 §The Assembler Examines The Two GUIDs, without a change. ADR 0011 for the listener delay.
- Uses: musicindex ADR 0075 R-VALUE-5 and the crate `musicindex-value` for the percentage of each recipient.
- Depends on: musicindex ADR 0086 (Accepted, musicindex commit 27b4cab), One Feed Parser And One Tag Crate For Songs
  And Episodes. It owns the frame names and the JSON shape of `TXXX:Podcast Chapters`. This record restates the parts
  that it uses, and ADR 0086 wins if the two differ.
- Depends on: musicindex ADR 0089 R-TAG-6 (Accepted, musicindex commit 28b32f2). It lets the tag writer put the song
  routes in a remote chapter entry.
- Bar: crosses repositories. The tag writer in HitIt, the slot text on the station and this producer must agree on the
  frame names and the chapter list. An agent can also make the opposite choice by mistake: pay the episode only, or
  pay a chapter with one GUID.
- Operator choice, 2026-11-22: when a station plays an episode file, the producer pays each chapter song, as the
  `valueTimeSplit` elements of the feed do. The orchestrator session relayed the choice
  (`publisher-chapter-pay-request.md`, commit 20bf300).
- Operator choice, 2026-11-22: the custom frames get neutral names.
- Operator choice, 2026-11-22, relayed by the orchestrator session: readers do not accept the old `MusicIndex` frame
  names. The operator tags the collection again with the new names. This choice replaces the first choice, which let
  readers accept the old names.
- Operator choice, 2026-11-22, relayed by the orchestrator session: the producer gets the routes of a remote chapter
  song from the route service with the two chapter GUIDs first. When the service gives no routes, the producer uses
  `value_routes` of the entry. musicindex ADR 0089 R-TAG-6 gives those routes. This choice replaces the first choice
  of the same day, which ignored the routes in the file.
- Operator choice, 2026-11-22, relayed by the orchestrator session: when the service gives no routes and the entry
  has no routes, the chapter pays nobody for that time, not the episode. This closes the first Open Question 4.
- Operator choice, 2026-11-22, relayed by the orchestrator session: `mixxx-now-playing` asks the route service with
  the two GUIDs. Its request with the track GUID alone is a defect against ADR 0024. The task that moves the route
  client to `now-playing-drop` corrects it. This closes the first Open Question 5.
- Operator choice, 2026-11-22, relayed by the orchestrator session: a chapter with a `title` that is null or empty
  gets the state title. ADR 0086 R-TAG-4 item 4 permits a null title.
- Revised 2026-11-22, after acceptance: §The Chapter Route Lookup is new. The song group of a remote split gets its
  routes from that section. The other rules do not change. The operator accepted the revision with the choices above.
- Review, 2026-11-22: an independent review by ChatGPT (`gpt-6-astra`) found 11 points in the first draft. This
  draft corrects points 1 to 10. Point 11 was about an empty new frame and an old frame. Readers do not read the old
  frames, so point 11 does not apply.

## Context

- A 24/7 station of HitIt plays song files and episode files. An episode file of a V4V music show holds songs of other
  artists. The feed pays each song with a `podcast:valueTimeSplit` element.
- The namespace text (`musicindex-research`, `sources/podcast-namespace/docs/tags/value-time-split.md`) gives these
  rules:
  - A split has one `remoteItem` or one or more local `valueRecipient` elements.
  - From `startTime`, the split replaces the recipients. After `duration`, the recipients of the parent block apply
    again.
  - With a `remoteItem`, `remotePercentage` is the part that the remote recipients get "on top of the default,
    non-fee" recipients. A missing value is 100. A value below 0 is 0, and a value above 100 is 100.
  - The fees of the parent block are calculated before anything goes to the split.
- The producer `liquidsoap-now-playing` reads `state.json` each 500 milliseconds (ADR 0022). The slot text writes it
  each second.
- Liquidsoap gives the method `elapsed` on a source: the time in the current track, in seconds
  (`liquidsoap/src/core/source/lang_source.ml`, lines 549-559 in the research sources). The value `track_started` of
  format 1 is the time of the `on_track` callback. It is not a time of the audio. The `on_track` and `on_metadata`
  callbacks of the slot text are synchronous. When the metadata of a track comes after the track mark, the
  `on_metadata` callback gives a new `seq` and keeps `track_started` (ADR 0022 §The State File Contract).
- Liquidsoap does not give `CHAP` frames as metadata. This is an inference of the orchestrator session: no slot text
  reads them, and it knows of no Liquidsoap decoder that gives them. A `TXXX` frame with JSON comes through as
  metadata, as the value routes frame does in the slot text of ADR 0022.
- The relay holds each block for the stream delay of the broadcaster (ADR 0011). ADR 0022 Open Question 2 records that
  nobody measured that delay for the MP3 mount and for HLS.

Terms:

- A **chapter** is one entry of the list in `TXXX:Podcast Chapters`. It is one `valueTimeSplit` of the feed. This is a
  project term.
- A **segment** is a part of an episode file with one drop file: one chapter, or one part with no chapter. This is a
  project term.
- The **position** is the time from the start of the file to the moment of the state, in milliseconds. This is a
  project term.
- A **group** is the song routes or the episode routes in a merge. This is a project term.

## Decision

### The Frame Names

musicindex ADR 0086 R-TAG-2 gives the names. The two producers read only these frames:

| Field | Frame |
| --- | --- |
| Feed GUID | `TXXX:Podcast Feed Guid` |
| Item GUID | `TXXX:Podcast Item Guid` |
| Value routes | `TXXX:Podcast Value Routes` |
| Image | `TXXX:Podcast Image` |
| Contributors | `TXXX:Podcast Contributors` |
| Description | `COMM:Podcast Description` |
| Chapters | `TXXX:Podcast Chapters` |

- The producers do not read the old `MusicIndex` frames (ADR 0086 R-TAG-2 item 2). A file with only old frames has
  no GUID, so it pays nobody (ADR 0024). The tag writer removes the old frames when it writes a file (R-TAG-2 item 5).
- The slot text reads the keys in lower case, for example `podcast item guid` (R-TAG-2 item 3). The recode exclude
  list has `podcast value routes` and `podcast chapters`, and no old key.
- The names of the drop file fields and of the state fields do not change. `track_guid` holds the item GUID.
- `mixxx-now-playing` changes its frame list to these frames. When `musicindex-lib` has the shared tags crate,
  `mixxx-now-playing` uses that crate and removes its own frame list and its reader.

### The State File Format 2

The reference slot text writes the format `musicindex-liquidsoap-state/2`. It has each field of format 1 and three new
fields:

| Field | Type | Use in the producer |
| --- | --- | --- |
| `elapsed` | number | `radio.elapsed()` at the write, in seconds. |
| `cue_in` | number | The cue-in point of the file in seconds, from the metadata key `liq_cue_in`. 0 when the key is empty. |
| `chapters` | string | The text of `TXXX:Podcast Chapters`, JSON text in a JSON string. Empty when the file has no list. |

- The producer reads format 1 and format 2. Format 1 has no chapters, so the file is one track, as before.
- The format 1 slot text of ADR 0022 reads the old `MusicIndex` keys. On a file with only the new frames, it gives no
  GUID, and the file pays nobody. The operator thus installs the new slot text when the station files get the new
  frames.
- Each other format gives the error of ADR 0022 and no drop file.
- `liq_cue_in` is the cue-in key of the Liquidsoap request functions (`liquidsoap/src/libs/request.liq`, line 417).
  These items are inferences, and the manual checks of §Verification test them:
  - `elapsed` starts at 0 at the cue-in point, so the position in the file is `elapsed + cue_in`.
  - At a crossfade, `radio.elapsed()` gives the time of the new track after its `on_track` callback.

### The Chapter List

The producer parses `chapters` as a JSON array. Each entry has these fields, as musicindex ADR 0086 gives
them:

| Field | Type | Use in the producer |
| --- | --- | --- |
| `start_ms` | integer | The start of the chapter in the file. |
| `end_ms` | integer | The end of the chapter in the file. |
| `title` | string or null | The drop file `title` of the chapter. |
| `artist` | string or null | The drop file `artist` of the chapter. |
| `feed_guid` | string or null | The feed GUID of the remote song. |
| `item_guid` | string or null | The item GUID of the remote song. |
| `remote_percentage` | number | The part of the payment for the remote song. |
| `value_routes` | array or null | A local split: the local recipients. A remote split: the song routes from the file, or null (musicindex ADR 0089 R-TAG-6). §The Chapter Route Lookup gives their use. |

- An entry with `end_ms` not above `start_ms` is not valid.
- A missing `remote_percentage` is 100. A finite value below 0 is 0, and a finite value above 100 is 100, as the
  namespace text says. A value that is not a finite number makes the entry not valid.
- An entry with no `feed_guid` and no `item_guid` is a **local split**. Each other entry is a **remote split**.
- The producer ignores a not valid entry. It writes an error in the log one time for each `seq`.
- A `chapters` text that is not a valid JSON array gives no chapters. The file is then one track with the episode
  GUIDs and routes. The producer writes an error in the log one time for each `seq`.
- A GUID that is empty or has only spaces is a missing GUID.

### The Position Rule

- The position is `round(1000 * (elapsed + cue_in))` in milliseconds. A value of `elapsed` or `cue_in` that is not a
  finite number, or a negative position, gives no chapter: the file is then one track for that read.
- The valid entries make the segments in the order of `start_ms`. An entry is active from its `start_ms` to its
  `end_ms`, or to the `start_ms` of the next entry if that is earlier. A later split thus starts at its `startTime`,
  as the namespace text says. Two entries with the same `start_ms`: the first in the list wins, and the producer
  ignores the second.
- The current chapter is the active entry with `start ≤ position < end`. When no entry is active, the episode applies.
- The track of the last `on_track` callback owns the time. During a crossfade, the next track thus owns the overlap,
  as in ADR 0022.
- The producer adds no delay for the listener. The relay holds each block for the stream delay of ADR 0011, as it does
  at a track change. ADR 0022 Open Question 2 thus also covers the time to the listener at a chapter change.
- `elapsed` can be 1 second old at the read, and the producer reads the file each 500 milliseconds. A chapter change
  can thus come to the publisher approximately 1.5 seconds late. The manual check 3 measures the real error.

### What The Producer Writes For An Episode File

This section adds rows to ADR 0022 §What The Producer Writes. The rows of ADR 0022 for not fresh, `live` and `jingle`
apply first and do not change.

The **episode group** is usable when the state has the two episode GUIDs and routes with a percentage. A remote split
is **identified** when the entry has the two GUIDs and a `remote_percentage` above 0. The **song group** of an
identified split is usable when the song routes of §The Chapter Route Lookup have a percentage.

| Segment | Identity in the drop file | Routes |
| --- | --- | --- |
| No chapter: before the first chapter, after the last chapter, in a gap, or a file with no list | The episode GUIDs | The episode routes |
| An identified remote split, the song group usable | The chapter GUIDs | §The Merged Routes |
| An identified remote split, no song routes | The chapter GUIDs | No routes: the publisher sends the dead block |
| A remote split that is not identified | The episode GUIDs | The episode routes |
| A local split | The episode GUIDs | §The Local Split |

- At each change of segment, the producer writes a new drop file, the same as at a track change.
- The publisher applies ADR 0024 to each drop file. The producer applies it also: routes only with a feed GUID and an
  item GUID in the identity of the drop file, and episode routes only when the episode group is usable.
- `title` is the chapter title when it is a string that is not empty, and the state title otherwise. `artist` is
  the chapter artist when it is a string that is not empty, and the state artist otherwise. `album` and `image` come
  from the state.
- `play_id` is `ls-<track_started in ms>-<seq>` for a file with no valid chapter, as in ADR 0022. For a file with
  chapters, it is `ls-<track_started in ms>-<seq>-<segment start in ms>`. Each segment thus gets its own line in the
  show log (ADR 0017).
- `duration_secs` stays `null`.

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
- The episode fee routes keep their percentage `F`, as in item 2 of §The Merged Routes. The local recipients get
  `R = 100 - F`, divided with `percentages`.
- The episode routes with no fee get nothing for that time.

### The Chapter Route Lookup

The **route service** is the MusicIndex API. The Mixxx producer uses the same service (ADR 0024). This is a project
term.

- **The request.** `GET <endpoint>/v1/feeds/<feed_guid>/tracks/<item_guid>?include=payment_routes`. The routes are
  `data.payment_routes` of the answer. This path finds the track by the two GUIDs, because an item GUID is unique only
  in its feed (ADR 0024). The stophammer OpenAPI text gives this path as "Get track by feed and GUID". The website
  session examined stophammer commit d4daf10: the answer has the routes only with `include=payment_routes` or
  `include=value_blocks`, and the answer is 404 for GUIDs that the service does not know. The producer never asks with
  one GUID, and it never uses the value block of the feed in place of the track.
- **The endpoint.** The option `--route-service URL` of `liquidsoap-now-playing`. The default is
  `https://api.musicindex.org`, the default of the Mixxx producer. Inference: that host serves the stophammer API. The
  station check 5 of §Verification tests it.
- **The time of the lookup.** The producer asks for each identified remote split when it first reads a new `seq` with
  a chapter list. One worker thread sends the requests one at a time, in the order of `start_ms`. So the lookup of a
  chapter starts at the start of the file, not at the chapter change. Each request has a time limit of 5 seconds, the
  same as the Mixxx producer.
- **The cache.** The cache holds one result for each pair of GUIDs of the current file. At a new `seq`, the producer
  keeps the results of the pairs that are also in the new list, and removes the other results. A result is the
  routes, or "no routes" for an answer 404 or an answer with an empty route list. A file that plays again later thus
  gets new routes from the service.
- **No answer.** A time-out, a network error, an answer that the producer cannot parse, or an HTTP error other than
  404 is not a result. The producer asks again after 30 seconds, while the file plays. It writes one `warn` line for
  each pair and each `seq`.
- **The song routes.** The producer selects the song routes of an identified remote split in this order:
  1. The routes of the service result, when they have a percentage. The service has the current value block of the
     song. Routes in a file stay the same after the song feed changes.
  2. Else `value_routes` of the entry, when they have a percentage. This applies while no result has come, after a
     404, and after an empty route list.
  3. Else no song routes. The chapter pays nobody for that time.
- **An empty route list.** Main-session choice, 2026-11-22, which the operator asked for: an answer with an empty
  route list is the same as a 404. Reason: the service then gives no recipient to pay. The routes in the file are
  the only recipients of the song, and they pay the song owner. Without them, the chapter pays nobody.
- **A late answer.** When a result comes while its chapter plays, the producer writes the drop file again with the
  new song routes. The `play_id` of the segment does not change.
- The producer sends no request for a jingle, for a `live` state, for a local split, for a remote split that is not
  identified or for a file with format 1.

### The Mixxx Producer

`mixxx-now-playing` reads the frames of §The Frame Names. It does not read the chapter list. It pays an episode file
as one track with the episode GUIDs and routes. It asks the route service with the two GUIDs, as the operator choice
of 2026-11-22 says. A chapter rule for Mixxx needs its own record.

## Invariants

- A drop file with routes has a feed GUID and an item GUID (ADR 0024), for a chapter also.
- Episode routes are in a drop file only when the state has the two episode GUIDs.
- The producer never makes a GUID or a route from a chapter title.
- The episode fees come before the split, in each segment.
- The total of the splits is 100, within the rounding of item 7, or the list is empty.
- The producer reads format 1 and format 2, and no other format.
- The producer asks the route service only with the two GUIDs of a chapter, and only with the path of the pair.
- An identified remote split with no song routes pays nobody. It never pays the episode in place of the song.

## Verification

Mechanical, in `liquidsoap-now-playing` with `tempfile` and an injected clock:

- A state sequence plays an episode file through three chapters, a gap and the end of the file. The test examines each
  drop file: the identity, the routes, the title and the `play_id`.
- The position 1 millisecond before, at, and 1 millisecond after the start and the end of a chapter, with a cue-in of
  12.5 seconds.
- Two overlapping entries: the later entry starts at its `start_ms`. Two entries with the same `start_ms`.
- A remote split with `item_guid` null, with an empty GUID and with a GUID of spaces gives the episode identity and
  routes.
- A remote split with the two GUIDs and an episode with one GUID gives the song routes at 100.
- A local split with `remote_percentage` 0, 50 and 100 gives the same routes.
- A chapter with `artist` null and `title` null gives the state artist and the state title.
- An identified remote split with `value_routes` in the file and no service result gives the chapter GUIDs and the
  merged routes with the file routes.
- An identified remote split with `value_routes` null and no service result gives the chapter GUIDs and no routes.
- A state of format 1 gives one track, as before.
- A `chapters` text that is not valid JSON gives one track and one error line.

Mechanical, in `now-playing-drop`, for the merge:

- The example of §The Merged Routes item 8.
- `remote_percentage` 0, 50 and 100, each with the song group or the episode group empty.
- Song shares of 0 and 0, a missing share, a negative share, and one route alone.
- A small part that rounds to 0.
- `remote_percentage` of -5, 105 and missing.

Mechanical, for §The Chapter Route Lookup, with a local HTTP server in the test:

- The request path has the two GUIDs and `include=payment_routes`. An entry with one GUID sends no request.
- An answer with routes gives the merged routes with the service routes, also when the entry has file routes.
- An answer 404 and an answer with an empty route list give the file routes, or no routes, and no second request.
- A time-out gives the file routes, or no routes, and a second request after 30 seconds of the injected clock.
- An answer that comes after the chapter start gives a second drop file with the service routes and the same
  `play_id`.
- At a new `seq`, a result for a pair in the new list stays in the cache. A result for a pair not in the list goes.

Mechanical, in `mixxx-now-playing`:

- The reader reads a file with the frames of §The Frame Names.
- A file with only the old `MusicIndex` frames gives no GUID and no routes.
- The route request has the feed GUID and the track GUID in the path of the pair.

Manual, on the test station `heycitizens_test_radio`, with a person:

0. Before the new slot text and the new producer go on the station, the operator tags the station files again with
   the frames of §The Frame Names. A file with only old frames pays nobody (ADR 0024).
1. The new slot text passes the save in AzuraCast. A `TXXX:Podcast Chapters` frame of approximately 4 KiB comes into
   `state.json` without a change.
2. A tagged episode file plays. The relay shows each chapter song at its time, with the merged routes.
3. The time error at a chapter change, with and without a crossfade.
4. A file with a cue-in point gives the correct `cue_in`, and the chapter change comes at the correct time.
5. The station host gets the routes of a chapter song from `https://api.musicindex.org` with the path of §The Chapter
   Route Lookup.

## Open Questions

1. **The artist of a chapter.** Closed on 2026-11-22. ADR 0086 at commit a353c91 adds `artist` to the chapter list.
2. **A seek in the file.** Inference: AzuraCast does not seek a file on air. If `elapsed` follows a seek, the position
   rule stays correct.
3. **The size of the chapter list.** Operator choice, 2026-11-22, relayed by the orchestrator session: no limit until
   the station check 1 of §Verification fails.
4. **A chapter song that MusicIndex does not know.** The operator said on 2026-11-22 that such a song implies a nested
   podcast. This record does not give a rule for it. The file routes or no routes apply until a later record.

## Alternatives Considered

### A Drop File Version 4 With `remote_percentage` And The Episode Routes

The producer gives the song routes, the episode routes and `remote_percentage`, and the publisher makes the merged
list. This needs a new drop file version and a change in the assembler, and the result on the relay is the same. The
producer has all the data, so the merge is in the producer, and the drop file stays version 3.

### The Position From `written - track_started`

The first draft used this rule. `track_started` is the time of the synchronous `on_track` callback, not a time of the
audio. Late metadata gives a new `seq` with the same `track_started`. The rule also does not include the cue-in point. `elapsed` comes from the source.

### The Song Routes For A Chapter With Only The Feed GUID

A remote item with only a feed GUID can mean the value block of the feed. ADR 0024 does not permit a payable block with
one GUID, and the operator requirement of that record says that the two GUIDs are always present together. So the
episode recipients get that time.

### Only The Song Routes In The Chapter List

The producer uses only `value_routes` of the entry and sends no network request. The operator did not select it on
2026-11-22: the routes in a file become old when the song feed changes. They are the second source of §The Chapter
Route Lookup.

### Only The Route Service

The first revision of 2026-11-22 ignored the routes in the file, and a chapter with no service result paid the
episode. The operator replaced it on the same day. A song that the service does not know then pays its owner from the
file routes, and a chapter never pays the episode in place of the song.

### Readers Also Accept The Old Frame Names

The first operator choice let readers read the old `MusicIndex` frames when the new frame was empty. The operator
replaced that choice on 2026-11-22. Each file gets the new frames, and readers keep one set of names.

## Consequences

- The slot text, the state format, the producer and the Mixxx frame list change. The publisher and the relay do not
  change.
- An episode file with no chapter list operates as before, with the episode GUIDs.
- A file that has only the old `MusicIndex` frames stops payment when the new slot text or the new Mixxx frame list
  goes into use. The operator tags each such file again before that time.
- The producer gets a dependency on `musicindex-value`.
- The station producer sends HTTPS requests to the route service. The publisher on the same host already sends
  requests to `api.musicindex.org`.
- The route client of `mixxx-now-playing/src/musicindex.rs` moves to `now-playing-drop` and gets the request of the
  pair, so that the two producers use one client.

## References

- `musicindex-research/research_notes/music-feeds/publisher-chapter-pay-request.md` (commit 20bf300).
- `musicindex-research/research_notes/music-feeds/shared-feed-and-tag-request.md` (commit 20bf300).
- `musicindex-research/sources/podcast-namespace/docs/tags/value-time-split.md`.
- ADR 0011, ADR 0017, ADR 0022, ADR 0024. musicindex ADR 0075.
