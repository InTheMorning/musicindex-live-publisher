# ADR 0045: One Segment Rule For The Frames And The Drop Files

- Status: Accepted on 2026-11-27 by the operator.
- Date: 2026-11-27
- Bar: crosses repositories, and an agent can make the opposite choice by mistake. The slot text and the producer
  must make the same segment starts, and Rainbrook releases a display state only by an equal `playId`.
- Changes: ADR 0038 §The Slot Text Writes The Frames and §Invariants (§What This Record Changes).
- Changes: ADR 0041 §The Chapter List, §The Position Rule and §What The Producer Writes For An Episode File
  (§What This Record Changes).
- Operator choice, 2026-11-27: the operator selected "times only" from three options. The other two options are in
  §Alternatives Considered. Rainbrook asked for this change, and makes no change to Rainbrook ADR 0030 for now.

## Context

Terms:

- A **segment** and a **boundary** have the meanings of ADR 0038 §Context.
- A **chapter entry** is one object of the `podcast chapters` JSON array (ADR 0041 §The Chapter List).
- The **time fields** of an entry are `start_ms` and `end_ms`. The **content fields** are all other fields.

On 2026-11-26 at 22:28:21 (Rainbrook clock), Rainbrook received the HLS frame `ls-1795725349800-7-6729868`. No
display state had that `playId` (station log step 63). Episode 158 has the chapter "Death of a Harmony" from 6498968
to 6729868 ms, and the chapter "All the way home" from 6724607 to 7028534 ms. The two chapters overlap by 5261 ms.

- The slot text wrote a frame at each `start_ms` and each `end_ms` of each entry (ADR 0038 §The Slot Text Writes The
  Frames).
- The producer ends a chapter at the next `start_ms` when that start is earlier (ADR 0041 §The Position Rule). Thus at
  6729868 ms the producer segment was still "All the way home", and it wrote no new drop file.
- The HLS output repeats the last frame until the next boundary. A player that joined after 6729868 ms thus got only
  that frame, for approximately 5 minutes. Rainbrook showed "Relay track: none" from 22:31:51 to 22:33:22.

Episode 158 has six overlaps of this type. ADR 0038 accepted such a frame and asked the player to ignore it. That rule
does not help a player that joins during the segment.

A second source of the same fault: the producer refuses an entry with a content field that is not valid. An example
is a bad route list. The slot text cannot see that, and wrote frames at the times of the refused entry.

The Liquidsoap 2.4.6 typed parse `[{start_ms: int?, end_ms: int?}?]` gives `null` for one entry that is not an
object. It also gives `null` for one time that is not an integer from 0 to 2^62-1. The other entries stay (station
checks of 2026-11-27, §Verification). Thus the slot text can refuse one entry by its times, as the producer does.

## Decision

### The Segment Rule

The slot text and the producer make the segment starts of a file with one rule. The rule reads only the time fields.

1. An entry is **timed** when it is a JSON object, `start_ms` and `end_ms` are integers from 0 to 4611686018427387903
   (2^62-1), and `end_ms` is above `start_ms`. A number with a fraction or an exponent, also `150.0` or `1e3`, is not
   an integer. Each other entry has no segment and no frame.
2. Of two timed entries with the same `start_ms`, the first in the list stays. The producer and the slot text ignore
   the second.
3. The timed entries are in the order of `start_ms`. A timed entry ends at its `end_ms`, or at the `start_ms` of the
   next timed entry if that is earlier.
4. The segment starts are 0 and the `start_ms` of each timed entry. A gap starts at the `end_ms` of a timed entry that
   ends before the next timed entry. A gap also starts at the `end_ms` of the last timed entry.
5. A file has **chapter segments** when it has one or more timed entries.

The upper limit 2^62-1 is the largest integer of Liquidsoap. The producer applies the same limit.

### The Slot Text

- The slot text writes a frame at each segment start of §The Segment Rule, and at no other position.
- The `playId` has the suffix `-<segment start>` when the file has chapter segments. ADR 0038 §The Slot Text Writes
  The Frames gives the other parts of the `playId`, without a change.
- The function `musicindex_boundaries` of `liquidsoap/musicindex-state.liq` holds the rule, between the lines
  `# BEGIN musicindex_boundaries` and `# END musicindex_boundaries`. `scripts/check-slot-boundaries.sh` reads it
  there.

### The Producer

- The producer makes its segments with §The Segment Rule.
- A timed entry with a content field that is not valid (ADR 0041 §The Chapter List) is a segment with no chapter. It
  has the drop file of a gap: the episode identity, the episode routes, and the state title and artist. It is not
  part of the gap before or after it.
- The decision if a file is a song state or an episode state does not change. It uses the entries that are valid in
  all fields, as before. Thus the route lookup, the `wpay` choice and `pub_date` do not change.
- The `play_id` has the suffix `-<segment start>` when the file has chapter segments, and no suffix otherwise.
- When the producer refuses an entry by its times or by its content, it writes an error in the log. It writes one
  error for each `seq`, as before.

### What This Record Changes

- ADR 0038 §The Slot Text Writes The Frames: §The Slot Text above replaces the item about a frame at each `start_ms`
  and `end_ms`. The item about a frame that names no display state no longer
  applies to the station. The other items stay.
- ADR 0038 §Invariants: the item "The slot text never decides which chapter entry is valid" becomes: the slot text
  decides only from the time fields, with §The Segment Rule. The producer decides from the content fields.
- ADR 0038 §The Player stays. A state can still arrive after its frame.
- ADR 0041 §The Chapter List and §The Position Rule: §The Segment Rule makes the segments in place of "the valid
  entries". An entry with a bad content field is a segment with no chapter, and not part of a gap.
- ADR 0041 §What The Producer Writes For An Episode File: the `play_id` suffix follows §The Producer above.

## Invariants

These rules apply while this decision is current.

- At each position of a file, the frame in the HLS stream and the drop file of the producer have the same `playId`.
- The slot text and the producer read only the time fields to make the segment starts.
- A change to §The Segment Rule changes the slot text, the producer and the shared cases in one commit.

## Verification

Mechanical:

- `liquidsoap-now-playing/tests/segment-boundaries.json` holds the shared cases: overlaps, gaps, a chapter in a
  chapter, the same start, refused content, bad times, and episode 158 at 1:52.
- A producer test reads each case and compares the segment starts and the suffix of the producer with the case.
- `scripts/check-slot-boundaries.sh` runs the slot text function on each case. A test on the build host cannot do this
  check, because the build host has no Liquidsoap. Run the script in the AzuraCast container of the test station
  before each deploy of a slot text change, and record the result in the station log. On 2026-11-27 all 28 cases
  passed, and a changed copy of the function failed with exit 1.

Visual. A person must examine this item. Report it as open until a person completes it.

- In Rainbrook on HLS, join the test station during the overlap of episode 158 at approximately 1:52:05. The screen
  shows "All the way home" and not "Relay track: none".

## Alternatives Considered

### The Producer Writes The Segment List

The producer writes the segment list in a file, and the slot text reads it. Not selected. It has one source. But the list
arrives approximately 1 second after the track start, and the slot text must find an old list by its `seq`. The slot
text also needs a fallback when the producer stops.

### The Slot Text Copies All Producer Checks

The slot text also checks the content fields. Rejected. The route checks follow the types of `now-playing-drop`, and a
copy in Liquidsoap can disagree with no failure. ADR 0038 §Alternatives Considered rejected a copy of the chapter
rules for this reason.

### The Player Ignores A Frame With No State

Rainbrook keeps the current state, or uses the relay snapshot after a join, when a frame names no state. Not selected
for now by the operator. It protects against each cause, but the station then sends frames with no meaning.

## References

- `docs/adr/0038-the-hls-stream-marks-each-segment.md`
- `docs/adr/0041-the-station-producer.md`
- `docs/runbooks/test-station-log.md`, step 63
- `rainbrook`: `docs/adr/0030` (the HLS frame sync)
