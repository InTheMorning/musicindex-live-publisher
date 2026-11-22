# ADR 0025: The Station Producer Pays Each Chapter Song Of An Episode File

- Status: Proposed
- Date: 2026-11-22
- Changes: ADR 0022 items: §The State File Contract (a format version 2) and §What The Producer Writes (the chapter
  rows).
- Uses: ADR 0024 §The Assembler Examines The Two GUIDs, without a change. ADR 0011 for the listener delay.
- Uses: musicindex ADR 0075 R-VALUE-5 and the crate `musicindex-value` for the percentage of each recipient.
- Depends on: musicindex ADR 0086 (Proposed, musicindex commit 50037a2), One Feed Parser And One Tag Crate For Songs
  And Episodes. It owns the frame names and the JSON shape of `TXXX:Podcast Chapters`. This record restates the parts
  that it uses, and ADR 0086 wins if the two differ.
- Bar: crosses repositories. The tag writer in HitIt, the slot text on the station and this producer must agree on the
  frame names and the chapter list. An agent can also make the opposite choice by mistake: pay the episode only, or
  pay a chapter with one GUID.
- Operator choice, 2026-11-22: when a station plays an episode file, the producer pays each chapter song, as the
  `valueTimeSplit` elements of the feed do. The orchestrator session relayed the choice
  (`publisher-chapter-pay-request.md`, commit 20bf300).
- Operator choice, 2026-11-22: the custom frames get neutral names. Readers also accept the old `MusicIndex` names.
- Review, 2026-11-22: an independent review by ChatGPT (`gpt-6-astra`) found 11 points in the first draft. This
  draft corrects points 1 to 10. §The Frame Names gives the reason for the rule of point 11.

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
  format 1 is the time of the `on_track` callback, which operates with `synchronous=false`. It is not a time of the
  audio.
- Liquidsoap does not give `CHAP` frames as metadata. This is an inference of the orchestrator session: no slot text
  reads them, and it knows of no Liquidsoap decoder that gives them. A `TXXX` frame with JSON comes through as
  metadata, as `musicindex value routes` does.
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

musicindex ADR 0086 gives the names. The two producers read the new name and the old name of each frame:

| Field | New frame | Old frame |
| --- | --- | --- |
| Feed GUID | `TXXX:Podcast Feed Guid` | `TXXX:MusicIndex Feed Guid`, `TXXX:MusicIndex Feed GUID` |
| Item GUID | `TXXX:Podcast Item Guid` | `TXXX:MusicIndex Track Guid` |
| Value routes | `TXXX:Podcast Value Routes` | `TXXX:MusicIndex Value Routes` |
| Image | `TXXX:Podcast Image` | `TXXX:MusicIndex Image` |
| Contributors | `TXXX:Podcast Contributors` | `TXXX:MusicIndex Contributors` |
| Description | `COMM:Podcast Description` | `COMM:MusicIndex Description` |
| Chapters | `TXXX:Podcast Chapters` | none |

- A new frame with a value that is not empty wins. Otherwise the old frame applies.
- Reason: Liquidsoap gives `""` for a key that is not there and for a key with an empty value. The slot text thus
  cannot tell the two apart. The Mixxx reader uses the same rule, so that the two producers give the same result for one
  file. A file with a cleared new frame and an old frame thus keeps the old value. Open Question 4 asks the tag writer
  to remove the old frames.
- The slot text reads the keys in lower case, for example `podcast item guid`. It adds `podcast value routes` and
  `podcast chapters` to the recode exclude list, and keeps `musicindex value routes` there.
- The names of the drop file fields and of the state fields do not change. `track_guid` holds the item GUID.
- `mixxx-now-playing` reads the new and the old names in its frame list now. When `musicindex-lib` has the shared
  tags crate, `mixxx-now-playing` uses that crate and removes its own frame list and its reader.

### The State File Format 2

The reference slot text writes the format `musicindex-liquidsoap-state/2`. It has each field of format 1 and three new
fields:

| Field | Type | Use in the producer |
| --- | --- | --- |
| `elapsed` | number | `radio.elapsed()` at the write, in seconds. |
| `cue_in` | number | The cue-in point of the file in seconds, from the metadata key `liq_cue_in`. 0 when the key is empty. |
| `chapters` | string | The text of `TXXX:Podcast Chapters`, JSON text in a JSON string. Empty when the file has no list. |

- The producer reads format 1 and format 2. Format 1 has no chapters, so the file is one track, as before. A station
  with the old slot text thus operates until its operator installs the new slot text.
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
| `title` | string | The drop file `title` of the chapter. |
| `feed_guid` | string or null | The feed GUID of the remote song. |
| `item_guid` | string or null | The item GUID of the remote song. |
| `remote_percentage` | number | The part of the payment for the remote song. |
| `value_routes` | array | The routes of the remote song, or the local recipients. |

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

The **episode group** is usable when the state has the two episode GUIDs and routes with a percentage. The **song
group** of a remote split is usable when the entry has the two GUIDs, routes with a percentage and a
`remote_percentage` above 0.

| Segment | Identity in the drop file | Routes |
| --- | --- | --- |
| No chapter: before the first chapter, after the last chapter, in a gap, or a file with no list | The episode GUIDs | The episode routes |
| A remote split, the song group usable | The chapter GUIDs | §The Merged Routes |
| A remote split, the song group not usable | The episode GUIDs | The episode routes |
| A local split | The episode GUIDs | §The Local Split |

- At each change of segment, the producer writes a new drop file, the same as at a track change.
- The publisher applies ADR 0024 to each drop file. The producer applies it also: routes only with a feed GUID and an
  item GUID in the identity of the drop file, and episode routes only when the episode group is usable.
- `title` is the chapter title for a chapter, and the state title for a part with no chapter. `artist`, `album` and
  `image` come from the state. The chapter list has no artist. Open Question 1 records this.
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

### The Mixxx Producer

`mixxx-now-playing` reads the new frame names. It does not read the chapter list. It pays an episode file as one track
with the episode GUIDs and routes. A chapter rule for Mixxx needs its own record.

## Invariants

- A drop file with routes has a feed GUID and an item GUID (ADR 0024), for a chapter also.
- Episode routes are in a drop file only when the state has the two episode GUIDs.
- The producer never makes a GUID or a route from a chapter title.
- The episode fees come before the split, in each segment.
- The total of the splits is 100, within the rounding of item 7, or the list is empty.
- The producer reads format 1 and format 2, and no other format.

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
- A state of format 1 gives one track, as before.
- A `chapters` text that is not valid JSON gives one track and one error line.

Mechanical, in `now-playing-drop`, for the merge:

- The example of §The Merged Routes item 8.
- `remote_percentage` 0, 50 and 100, each with the song group or the episode group empty.
- Song shares of 0 and 0, a missing share, a negative share, and one route alone.
- A small part that rounds to 0.
- `remote_percentage` of -5, 105 and missing.

Mechanical, in `mixxx-now-playing`:

- The reader reads a file with only the old names, a file with only the new names, and a file with an old and a new
  frame with different values.

Manual, on the test station `heycitizens_test_radio`, with a person:

1. The new slot text passes the save in AzuraCast. A `TXXX:Podcast Chapters` frame of approximately 4 KiB comes into
   `state.json` without a change.
2. A tagged episode file plays. The relay shows each chapter song at its time, with the merged routes.
3. The time error at a chapter change, with and without a crossfade.
4. A file with a cue-in point gives the correct `cue_in`, and the chapter change comes at the correct time.

## Open Questions

1. **The artist of a chapter.** The JSON shape of musicindex ADR 0086 at commit 50037a2 has no `artist`. The drop
   file then shows the artist of the episode with the song title. The orchestrator session asked for an `artist`
   field, `null` when unknown. When ADR 0086 adds it, the producer uses it.
2. **A seek in the file.** Inference: AzuraCast does not seek a file on air. If `elapsed` follows a seek, the position
   rule stays correct.
3. **The size of the chapter list.** Operator choice, 2026-11-22, relayed by the orchestrator session: no limit until
   the station check 1 of §Verification fails.
4. **The old frames.** Must the tag writer remove the old `MusicIndex` frames when it writes the new frames? Without
   that, a cleared new frame keeps the old value (§The Frame Names). The orchestrator session asked ADR 0086 for this
   rule.

## Alternatives Considered

### A Drop File Version 4 With `remote_percentage` And The Episode Routes

The producer gives the song routes, the episode routes and `remote_percentage`, and the publisher makes the merged
list. This needs a new drop file version and a change in the assembler, and the result on the relay is the same. The
producer has all the data, so the merge is in the producer, and the drop file stays version 3.

### The Position From `written - track_started`

The first draft used this rule. `track_started` is the time of a callback that operates with `synchronous=false`, not
a time of the audio. The rule also does not include the cue-in point. `elapsed` comes from the source.

### The Song Routes For A Chapter With Only The Feed GUID

A remote item with only a feed GUID can mean the value block of the feed. ADR 0024 does not permit a payable block with
one GUID, and the operator requirement of that record says that the two GUIDs are always present together. So the
episode recipients get that time.

### The New Frame Wins When It Is Present And Empty

This rule lets a writer clear a value. The slot text cannot see the difference between an empty frame and no frame,
so the two producers would give different results for one file.

## Consequences

- The slot text, the state format, the producer and the Mixxx frame list change. The publisher and the relay do not
  change.
- An episode file with no chapter list operates as before, with the episode GUIDs.
- The producer gets a dependency on `musicindex-value`.

## References

- `musicindex-research/research_notes/music-feeds/publisher-chapter-pay-request.md` (commit 20bf300).
- `musicindex-research/research_notes/music-feeds/shared-feed-and-tag-request.md` (commit 20bf300).
- `musicindex-research/sources/podcast-namespace/docs/tags/value-time-split.md`.
- ADR 0011, ADR 0017, ADR 0022, ADR 0024. musicindex ADR 0075.
