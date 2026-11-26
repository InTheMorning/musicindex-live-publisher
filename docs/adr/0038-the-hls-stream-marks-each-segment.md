# ADR 0038: The HLS Stream Marks Each Segment

- Status: Accepted on 2026-11-26 by the operator.
- Date: 2026-11-26
- Bar: crosses repositories. The rule changes the slot text and the publisher in this repository, the display state of
  `musicindex-live-relay`, and the HLS sync of Rainbrook.
- Changes: ADR 0009 §The MusicIndex Frame. The station slot text writes the frame `musicindex.hls/2` of §The Segment
  Frame in place of `musicindex.hls/1`. The tagger of ADR 0009 keeps `musicindex.hls/1`.
- Changes: ADR 0039 §The Publisher. The display state on the wire also has `playId` (§The Display State Gets
  `playId`). ADR 0039 replaces ADR 0012, which has three "Changed by" lines.
- Changes: ADR 0035 §The Slot Text Writes The Picture File and §Invariants. The slot text also writes a segment frame
  at each chapter boundary (§The Slot Text Writes The Frames).
- Operator choice, 2026-11-26: Rainbrook on HLS shows a chapter change too early. The fix is a
  `musicindex.hls/2` frame in the HLS stream, not an ICY title change at each chapter and not a calculation in the
  app.

## Context

Terms:

- A **segment** is a part of an audio file that has its own drop file and display state (ADR 0041 §What The Producer
  Writes For An Episode File). A file with no chapters has one segment. An episode file has one segment for each
  chapter and for each gap.
- A **boundary** is the position in the file where a segment starts, in milliseconds.

On 2026-11-26 the operator compared the chapter change with the audio (station log step 50):

- v4vmusic.com reads Socket.IO with the delay of ADR 0037. The chapter change came at almost the correct time.
- Rainbrook on HLS showed the chapter change too early.

A chapter change gives no ICY title change and no HLS title change (station log step 47). Rainbrook selects the newest
display state whose song line matches the stream title (`rainbrook` ADR 0011 and ADR 0025). Each segment of an episode
has the song line of the file. Thus each chapter state matches when it arrives on the instant SSE route, and the app
shows it at once. The HLS audio is more than 3 seconds behind the server (station log step 47), and the player adds
its own buffer.

The station HLS output reads the same Liquidsoap source as the Icecast output, after the slot text. The slot text
writes `musicindex.hls/1` at each track start from the file tags. It writes no frame at a chapter boundary.

Liquidsoap 2.4.6 on the station has the source methods `insert_metadata` (with `new_track`) and `on_frame`, and
`elapsed` is exact in a frame. Thus the slot text can insert a metadata group at a file position.

The producer makes `play_id` as `ls-<track start in ms>-<seq>`, and adds `-<segment start in ms>` for a file with
chapters (`liquidsoap-now-playing/src/lib.rs`, `render_state`). The start of a segment is 0, the `start_ms` of an
entry, or the `end_ms` of an entry (`chapters::segment_at`). The relay display state has no `play_id` and no GUID
today. Its track has `album`, `artist`, `artwork`, `songLine`, `title` and `value`.

## Decision

### The Segment Frame

```json
{ "schema": "musicindex.hls/2", "playId": "ls-1795656378730-2-30665" }
```

- `playId` is the `play_id` of the segment that starts at the position of the frame, or `null`.
- `playId` is `null` for a jingle and while the slot text cannot make the identity (§The Slot Text Writes The Frames).
- The frame has no other field. A new field needs a new `schema` value.
- The frame goes in a `TXXX` frame with the description `musicindex`, as in ADR 0009 §The ID3 Frames.
- The HLS output repeats the last metadata group in each segment until the next group (ADR 0009 §The ID3 Frames).
  Thus each segment after a boundary has the frame of that boundary, and a player that connects during a segment
  gets the frame in its first HLS segment. Station log step 51 shows this. The HLS output also starts a new segment
  at each inserted group.
- The slot text writes the frame one audio frame after the track mark. Thus the short HLS segment at a track start can
  have the track tags and no `musicindex.hls/2` frame. In station log step 51 that segment was 20 ms long.

The frame does not carry the track, the artwork or the value block. The player gets them from the display state with
the same `playId`. Thus the slot text does not copy the chapter rules of the producer.

### The Slot Text Writes The Frames

- At each track start, the slot text writes a frame for position 0.
- For a file with a `podcast chapters` value that is a JSON array with one or more entries, the slot text also writes
  a frame at each distinct `start_ms` and `end_ms` above 0 of each entry. It does not examine the other fields.
- The `playId` of a frame at position `b` is `ls-<ms>-<seq>-<b>` for a file with chapters, and `ls-<ms>-<seq>` for
  a file with no chapters. `<ms>` and `<seq>` are the values that the slot text writes as `track_started` and `seq`
  in `state.json`. `<ms>` is `track_started * 1000` with the fraction removed, the same as the producer.
- The slot text makes `<ms>` from the text of `track_started` that it writes in `state.json`, not from the full value
  of `time()`. Liquidsoap writes that number with about 12 significant digits (station log step 51).
- A frame at a position that the producer does not use as a boundary names no display state. Examples: the `end_ms`
  of a chapter when the next chapter starts earlier, and an entry that the producer refuses. §The Player ignores such
  a frame.
- The position is `elapsed + cue_in` of the track, in milliseconds, as in ADR 0041 §The Position Rule. The slot text
  writes the frame in the first audio frame at or after the boundary.
- The inserted metadata group repeats the `title` and the `artist` of the file. The ICY title thus stays the same, and
  the HLS `TIT2` and `TPE1` stay the same.
- The insert is the last operator of the slot text, after the handlers that write `state.json`. The handlers of the
  slot text and of AzuraCast thus never see an inserted group.
- The insert never makes a new track (`new_track` is `false`).

### The Display State Gets `playId`

- The publisher adds `playId` to each display state that it sends, with the `play_id` of `display.json`, or `null`.
- The publisher sends `null` for a missing `play_id`, never an empty string.
- Relay ADR 0012 owns the wire rule. On 2026-11-26 the relay session agreed to it, with the status Proposed (relay
  commit `13a2635`). `playId` is optional, a string of 1 to 128 characters, or `null`. The relay passes it through
  and does not compare it.
- The relay with relay ADR 0012 must run on `api.musicindex.org` before a publisher sends `playId`. The present relay
  refuses each track with `playId`, also with `null`.

### The Player

Rainbrook ADR 0030 will own this rule (Proposed, agreed by the Rainbrook session on 2026-11-26). This record asks
Rainbrook for this rule on HLS:

- A `musicindex.hls/2` frame with a `playId` releases the held display state with the same `playId`.
- A frame that names no held state changes nothing. A state that arrives after its frame is released when it
  arrives, as in ADR 0009 §The Join Point.
- A frame with `playId` `null`, and a stream with no `musicindex.hls/2` frame, keep the ICY rule of Rainbrook
  ADR 0011.
- Held states with different `playId` values do not replace each other, and the app holds enough states for the HLS
  delay. Rainbrook ADR 0030 owns these two app rules.

## Invariants

These rules apply while this decision is current.

- A segment frame never changes the ICY title, and never makes a new track.
- The slot text never decides which chapter entry is valid. The producer owns that decision.
- The frame never holds the track, the artwork, the routes or the value block.
- The frame names a display state only by an equal `playId`. No component infers the segment from text.

## Before Acceptance

1. **The `playId` equality.** Done 2026-11-26, pass, station log step 51. On the station, write the frames for one episode in a test slot text, and read the
   HLS segments. Make sure that each frame at a real boundary has the same `playId` as the producer `display.json`
   of that segment. This proves that Liquidsoap and the producer make the same `<ms>` from `track_started`. Also
   read two or more HLS segments after one boundary, and make sure that each has the same frame.
2. **The ICY title.** Done 2026-11-26, pass, station log step 51. Make sure that the ICY title of `/radio.mp3` does not change at a frame, and that AzuraCast
   records no new song.
3. **The handoffs.** Done 2026-11-26: the relay session agreed, in relay ADR 0012. The Rainbrook session agreed, in
   Rainbrook ADR 0030. Each session builds after this record is accepted.
4. **The pairing record.** Done 2026-11-26: ADR 0039 is accepted.

## Verification After Implementation

Mechanical:

- A publisher test that the display state has `playId` equal to the `play_id` of `display.json`, and `null` for a
  jingle.
- A relay test for `playId`: a string, `null`, and a string of more than 128 characters, which is refused.
- A check of the slot text on the station, because no test runs Liquidsoap: for one episode, the list of frame
  positions in the HLS segments agrees with the list of boundaries in the chapter list. The station log records it.

Visual. A person must examine these items in Rainbrook on HLS. Report each one as open until a person completes the
check.

- The picture and the lines of a chapter change when the listener hears the chapter song. Done 2026-11-26, pass,
  station log step 53.
- A boost during the first seconds of a chapter song pays the block of that song. Partly done 2026-11-26, station log
  step 53: a boost in the first seconds after the end of a chapter song paid the block of the episode. A boost at the
  start of a chapter song is open.

## Alternatives Considered

### The ICY Title Changes At Each Chapter

Not selected by the operator. It needs no app change. But AzuraCast records each chapter as a song, and the ICY text
is a text match, not an identity.

### The App Adds The Chapter Offset

Not selected by the operator. Each app must calculate the time, and the display state must carry the offset.

### The Frame Carries The Track

Rejected. The slot text must then copy the chapter rules of the producer, for the title, the artist and the valid
entries. Two copies of one rule can disagree.

### The Frame Carries The Song GUIDs

Rejected. Two segments in a row can have the episode GUIDs, for example a gap and a local split. The relay display
state also has no GUID.

## Consequences

- Rainbrook on HLS shows each segment when the listener hears it, also when the HLS delay changes.
- The publisher, the relay and Rainbrook each change. Until all three change, Rainbrook keeps the present behavior.
- A frame that arrives without its display state does nothing until the state arrives. A slow relay thus gives a late
  change, not an incorrect one.
- The ICY path and Socket.IO do not change.
- The tagger of ADR 0009 for the VPS stack keeps `musicindex.hls/1`. It can adopt this frame in a later record.

## References

- `docs/adr/0009-hls-track-metadata.md`
- `docs/adr/0039-the-display-state-pairs-with-its-value-block.md`
- `docs/adr/0041-the-station-producer.md`
- `docs/adr/0037-the-listener-delay-comes-from-the-icecast-burst.md`
- `docs/runbooks/test-station-log.md`, steps 47, 50 and 51
- `musicindex-live-relay`: `docs/adr/0012-display-play-id.md`
- `rainbrook`: `docs/adr/0011-icy-sync-of-the-relay-display.md`, `docs/adr/0025-the-icy-text-of-an-hls-stream.md`
