# ADR 0003: Show log contract

Status: Accepted
Date: 2026-09-06

Amended 2026-09-07: the first draft told a consumer to build chapter and value
time split start times from `aired_at`. That is wrong. `butt` records at the
encode point, which is before the encoder queue, the icecast queue, and the
player buffer that `stream_delay_secs` compensates for. A recorded episode
therefore aligns to `observed_at`, and `aired_at` describes live delivery
only.

## Context

A live show sends payment routing to listeners while it plays, and then it is
gone. The operator wants the show to become a podcast episode: one MP3, one
Podcasting 2.0 RSS item, chapters, and value time split blocks that pay the
same artists the live listeners paid.

The division of work is settled. This service keeps a detailed log of what
happened. `v4vmm` reads that log and generates the episode XML and JSON. This
service generates nothing.

That division is correct because this service is the only component that sees
the whole show. It watches the drop directory, so it observes every track
change from every producer. It runs for the length of the broadcast. `v4vmm`
can be closed while a show runs, so `v4vmm` cannot observe the show, but it
holds the library metadata and the operator, so it is the right place to build
an episode.

Two facts about this service control the contract.

**A published payload is delayed.** `src/schedule.rs` holds each payload for the
target stream delay, because listeners hear a track several seconds after the
producer writes the drop file. The recording comes from the encoder output, so
the recording timeline matches the delayed time. A consumer that uses the drop
time puts every chapter and every value time split early by the whole delay.

**A block can be revised.** The schedule replaces a pending payload that repeats
an `(eventGuid, blockGuid)` pair. That is the MusicIndex value-route upgrade,
where the routes for a track arrive after the track starts. The log therefore
can hold more than one entry for one block, and only the last one is correct.

## Decision

This service writes an append-only show log. The schema string is
`musicindex.showlog/1`. This repository owns the contract.

### Format

The log is JSON Lines. One line is one event. The service appends a line and
flushes it.

JSON Lines is correct here because the log is written once, appended to for
hours, and read whole later. A crash can damage only the last line, and a
consumer detects that by parsing. A database would add a dependency and a
failure mode for a file that is never updated in place.

### Entry fields

| Field | Description |
|---|---|
| `schema` | `musicindex.showlog/1` |
| `kind` | `track` when a track plays, `clear` when playback stops |
| `target` | The publish target name |
| `event_guid` | The live item this entry belongs to |
| `block_guid` | The block identity for this track |
| `observed_at` | Wall clock time the producer wrote the drop file |
| `aired_at` | Wall clock time this service published the payload |
| `stream_delay_secs` | The delay applied between the two times above |
| `artist` | From the drop file |
| `title` | From the drop file |
| `duration_secs` | From the drop file, when present |
| `image` | From the drop file, when present |
| `feed_guid` | From the drop file, when present |
| `track_guid` | From the drop file, when present |
| `value_routes` | From the drop file |
| `value_routes_source` | From the drop file, when present |
| `publish_result` | `accepted`, `dropped`, `fatal`, or `retryable` |
| `seq` | The relay sequence number, when the relay accepted the payload |

### Real time is canonical

`v4vmm`, the encoder, and this service all assume and present real time, which
is `observed_at`. One timeline serves the operator interface, the recording,
and the episode. Decided on 2026-09-07.

`aired_at` exists for one job: the live socket.io delivery path. It never
appears in an operator interface and never drives an episode.

An artifact that does not sit in real time must be corrected into it. It does
not get a second timeline of its own.

### Which time a consumer uses

The log records two times because they answer two different questions.

`observed_at` is when the track started at the producer. A local encoder
recording is made at the encode point, which is before every delay that
`stream_delay_secs` covers. **An episode built from that recording aligns to
`observed_at`.**

`aired_at` is when this service sent the payload to the relay. It exists so a
live listener's app flips the value block near the moment that listener hears
the change. It is a delivery time for the live path and nothing more. It does
not describe the audio, and it must not drive a recorded episode.

A recorder that pulls the stream after the icecast server sits on the other
side of that delay and aligns closer to `aired_at`. A consumer must therefore
know which side of the delay its recording came from. The log states the facts
and does not choose.

### The delay is an estimate, not a measurement

`stream_delay_secs` is an operator setting. No measurement exists for what a
post-icecast listener actually hears. No measurement exists for the latency
`butt` adds to a local recording. Both values are unknown today.

The log therefore records the configured delay as a field rather than folding
it into one corrected timestamp. A later calibration can adjust an episode
without a second show, because the raw facts remain.

### The last entry for a block wins

A later entry with the same `event_guid` and `block_guid` supersedes an earlier
one. A consumer keeps the last entry for each pair.

### This service does not know about shows

The log is continuous. It holds no show start and no show end, because this
service cannot know them. A consumer selects a time range.

`v4vmm` knows the range, because it starts and stops the encoder recording. The
recording window is the show.

### The log never blocks a publish

A failure to write the log is logged as a warning and does not stop the publish
loop. Payment routing is the primary duty of this service. A missing episode is
a smaller loss than a broadcast that stops.

### Location and retention

The log lives in the state directory of the publisher instance. A configuration
value sets the retention period. The service removes a log file older than that
period at startup.

## Invariants

- The log is append-only. A line is never rewritten and never removed in place.
- A token never appears in the log.
- A log failure never stops a publish.
- `aired_at` is the published time. `observed_at` is the producer time. The
  service records both and corrects neither.
- Real time is `observed_at`. Every interface and every episode uses it.
- An unknown schema version is ignored by a consumer, never guessed.
- This service generates no RSS, no chapters, and no episode.

## Alternatives Considered

### Log only the drop-file content

Rejected. Without `block_guid` and the publish result, a consumer cannot tell a
published track from one the relay refused, and cannot follow a route revision.

### Record one corrected timestamp

Rejected. A single corrected time bakes in a delay value that nobody has
measured. If the setting is wrong, every episode made before the correction is
wrong and cannot be repaired. Two raw times and the delay value keep every
episode repairable.

### Let `v4vmm` observe the show

Rejected. The chain must run when `v4vmm` is closed, which is the point of
`v4vmm` ADR 0059. A recorder that stops when the operator closes a laptop
records nothing.

### Generate the episode in this service

Rejected. Episode generation needs library metadata, MusicIndex data, and an
operator to name and describe the episode. This service is headless and holds
none of that. It stays a small service that records facts.

### Use SQLite for the log

Rejected. The log is append-only and is read whole. SQLite adds a dependency
and a corruption mode to a workload that a text file serves correctly. The
relay repository chose SQLite for durable identity, which is a different
workload.

### One log file for each show

Rejected. This service cannot detect a show boundary. A gap in playback is not
a show end, because a broadcaster pauses.

## Consequences

Positive:

- A show recorded from now on can become an episode later, even before the
  generation code exists.
- The delay problem is solved once, in the component that owns the delay.
- A route revision is visible instead of silently lost.
- `v4vmm` reads a documented file rather than reconstructing a timeline.

Negative and risks:

- A new file to rotate, retain, and back up.
- The log holds artist and title text for every track, which is a record of
  what an operator played.
- A consumer that ignores the supersede rule produces an episode with stale
  routes, and nothing detects that at generation time.
- A consumer that picks the wrong timestamp offsets every chapter by the stream
  delay. Nothing detects that either, so the choice is stated in this contract
  rather than left to the reader.
- The contract now has a second version to maintain beside the drop file.

## Follow-Up Work

- Episode generation in `v4vmm`, which is a separate ADR in that repository.
- A podping step after the feed updates.
- Whether the log should record the encoder recording state, once `v4vmm`
  controls it.
- Measure the real post-icecast delay and any `butt` recording latency, so the
  estimate becomes a calibration.
- Whether the live delay belongs at the relay, applied only to the socket.io
  emission, rather than at this service. See the interoperability note in the
  `splitkit` repository.

## References

- ADR 0002 - Now-playing drop-file contract
- `src/schedule.rs` - the stream delay this contract depends on
- `docs/architecture/broadcast-chain-boundaries.md`
- `v4vmm`: `docs/research/broadcast-recording-and-feed-publishing.md`
- `v4vmm`: `docs/adr/0059-broadcast-control-surface.md`
