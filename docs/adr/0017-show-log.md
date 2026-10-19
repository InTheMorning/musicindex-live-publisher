# ADR 0017: The Show Log

Status: Implemented
Date: 2026-10-19

Implemented 2026-10-19: tasks 001 to 003 of `docs/plans/show-log.md`. The
named artifact is `docs/reviews/show-log-review-checklist.md`, which has no
open item.

Accepted 2026-10-19 by the operator. This record supersedes ADR 0003, which
moved to `docs/archive/adr/`.

Amended 2026-10-19: `song_line` and `artwork` move from the `track` entry
to a new `display` entry. The producer writes the drop file and
`display.json` in no fixed order (ADR 0012), and a separate worker publishes
the display state. So a `track` entry often comes before the display state of
its play. A reader joins the two by `target` and `play_id`. No decision
reverses.

Proposed 2026-10-19.

Class: durable. This record owns a contract between this repository and
`v4vmm`.

## Context

The operator wants each live show to become a podcast episode. `v4vmm`
exports one valid RSS item with chapters, changing art and value time splits.
That export needs a record of what played and what the listeners paid. This
service is the only component that runs for the full show and sees each
track. ADR 0003 accepted this division on 2026-09-06, but it was never
implemented. Later decisions changed the facts that it depends on:

1. **The delay moved to the relay.** ADR 0003 depends on `src/schedule.rs`,
   which held each payload for the stream delay. Since ADR 0011, this service
   sends each payload at once with `Listener-Delay-Secs`, and the relay
   delays Socket.IO (relay ADR 0004). The scheduler is gone. The ADR 0003
   field `aired_at` thus records almost the same time as `observed_at`.
2. **Each track is reported.** Since ADR 0014, a track that is not payable
   gives a drop file with an empty `value_routes`. Such a track is a chapter
   of the episode. ADR 0003 was written when an absent file also meant "not
   payable".
3. **The art has a path.** Since ADR 0008, the display state carries the
   artwork as a URL, or as an embedded image with its SHA-256. The relay
   keeps two images for each event, in memory only (relay ADR 0003). The
   producer keeps only the current image and the one before it. So no copy
   of the live image lasts until an export.
4. **The time source was open.** ADR 0003 blocked its writer on a producer
   timestamp. Drop file version 3 has no timestamp.
5. **The play has an identity.** Since ADR 0010, the drop file has `play_id`.
   The Mixxx producer writes the ID of its history row, and that row names
   the file. Since ADR 0012 and ADR 0013, the display state has the song line
   and the album.

ADR 0008 tells how the producer prepares an image
(`mixxx-now-playing/src/display.rs`, `prepare_image`). An image of 512 KiB or
less, with a side of 1,000 px or less, goes out with no change. A larger
image is reduced to 1,000 px on its longest side and encoded as JPEG. The
producer reads JPEG and PNG only. So the SHA-256 of a live image can be the
hash of a reduced JPEG, and a GIF gives no live image.

## Decision

### One Log For Each Instance

This service writes an append-only show log in the state directory of its
instance. The schema string is `musicindex.showlog/1`. ADR 0003 defined the
same name, but no version 1 file was ever written, so the name is free.

- The format is JSON Lines. One line is one entry. The service appends a
  line and flushes it. A crash can damage only the last line, and a reader
  finds that when it parses.
- The service starts a new file at each UTC day:
  `showlog-YYYY-MM-DD.jsonl`.
- A configuration value sets the retention, with a default of 365 days. At
  startup, the service removes each log file that is older.
- The service does not know when a show starts or ends. `v4vmm` starts and
  stops the recording, so it gives the time range of the show.

### The Time Source

`observed_at` is the wall-clock time at which this service reads the drop
file, or finds that the drop file is gone. It is an RFC 3339 time in UTC with
milliseconds.

- The service reads the file within milliseconds of the producer write, so
  this time is the start of the track at the producer.
- A local encoder recording starts at the same point, before every delay.
  So an episode built from that recording uses `observed_at` with no
  correction.
- The entry also records `listener_delay_secs`, the value that this service
  sent in `Listener-Delay-Secs`. A recording made after Icecast can use it as
  an estimate of its offset. It is not a measurement.

A producer timestamp is not part of this record. If a measurement shows that
the read time is wrong, a later drop file version can add one.

### Entries

`kind` is one of:

- `track`: a drop file is present. Payable or not, as ADR 0014 defines. One
  entry for each publish attempt of each target.
- `clear`: the drop file is gone, and no track plays. One entry for each
  publish attempt of each target.
- `display`: this service read a new display state for a target with a
  display path. One entry for each display state that it sends to the
  relay.

A reader ignores an entry with a `kind` that it does not know. A later record,
for example a talk segment of ADR 0015, can add a kind with no new schema
version.

| Field | Content |
|---|---|
| `schema` | `musicindex.showlog/1` |
| `kind` | `track` or `clear` |
| `observed_at` | The read time of this service |
| `target` | The name of the publish target |
| `event_guid` | The event of the target |
| `block_guid` | The block that this service published. For a non-payable track and for `clear`, the dead block. |
| `payable` | `true` when the drop file has value routes |
| `play_id` | From the drop file, when present |
| `artist`, `title`, `album` | From the drop file. `album` when present. |
| `duration_secs` | From the drop file, when present |
| `feed_guid`, `track_guid` | From the drop file, when present |
| `image_url` | The `image` of the drop file, when present |
| `value` | The `value` object of the published payload, exactly as sent |
| `listener_delay_secs` | The value sent in `Listener-Delay-Secs` |
| `publish_result` | `accepted`, `retryable`, `dropped` or `fatal` |
| `seq` | The relay sequence number, when the relay accepted the payload |

A `track` entry and a `clear` entry have the same fields. A field that has no
value is absent, not `null`.

A `display` entry has these fields:

| Field | Content |
|---|---|
| `schema`, `kind`, `observed_at`, `target`, `event_guid` | As above. `observed_at` is the time at which this service read `display.json`. |
| `play_id` | From the display state, when present |
| `artist`, `title`, `album`, `song_line` | From the display state. Absent for `{"track": null}`. |
| `artwork` | `{"url": …}`, or `{"sha256": …, "mime": …}`, or absent |

A reader joins a `display` entry to a `track` entry by `target` and
`play_id`. A `display` entry with no track marks the end of the display
state.

`value` is the copy of what the live listeners paid. The value time splits of
the episode come from it, not from the drop file. A reader thus pays the same
recipients as the live show. It does not rebuild a block with the rules of
ADR 0005 and ADR 0010.

### The Art Is A Reference

This service never stores an image. The log records only references:

- `image_url`, the public image of the track, when the drop file has one.
- `artwork` of a `display` entry, the live image. A SHA-256 identifies the
  image that went out live. That image can be a reduced JPEG, so the hash is
  not always the hash of the embedded original.
- `play_id`, so that `v4vmm` can find the history row and the file.

`v4vmm` resolves the art of each chapter when it exports: the full image, an
animated GIF, or no image. The operator decides there which image to
publish.

### The Last Entry For A Block Wins

A later accepted entry with the same `target`, `event_guid` and `block_guid`
supersedes an earlier one. This follows a route revision, in which the routes
of a track arrive after the track starts (ADR 0010). A reader keeps the last
accepted entry for each block, at the `observed_at` of the first entry of
that block.

### The Log Never Stops A Publish

A failure to write the log gives a `tracing` warning, and the publish
continues. Payment routing is the first duty of this service.

## Invariants

- The log is append-only. No line is changed or removed in place.
- A token never goes into the log.
- A log failure never stops a publish.
- `observed_at` is the time of every entry. Each interface and each episode
  uses it.
- `value` is the published value, with no change.
- This service stores no image, and generates no RSS, chapter or episode.
- A reader ignores an unknown schema version and an unknown `kind`.

## Non-Goals

- The export: the RSS item, chapters, value time splits and the upload of the
  art. That is a `v4vmm` decision.
- A producer timestamp.
- A talk segment. ADR 0015 owns it.
- A measured stream delay.

## Alternatives Considered

### Amend ADR 0003

Rejected. Its premises 1 and 4 are false now, and this record removes its
field `aired_at`. A change that reverses a decision needs a new record.

### A Producer Timestamp In Drop File Version 4

Rejected for now by the operator on 2026-10-19. The read time is accurate to
milliseconds and needs no contract change in the producer.

### This Service Stores Each Image

Rejected by the operator on 2026-10-19. The image already exists in the
music file or at its URL. A copy would make this service a media store, with
retention and disk use. It would also hold art that the operator may not have
the right to publish. `v4vmm` decides that at export.

### Record The Drop File Only

Rejected. Then a reader must rebuild each block with the rules of ADR 0005
and ADR 0010. It also cannot tell a published block from a refused one.

### SQLite

Rejected. The log is append-only and read whole. A text file serves that,
with no dependency and no corruption mode of a database.

## Consequences

- A show recorded after the implementation can become an episode later.
- The value time splits of an episode pay what the live listeners paid.
- The log holds the artist and title of each track that the operator played.
  It is a record of the operator's show, kept for the retention period.
- A broadcaster's `status` view can read the end of the log, to show what
  this service sent last.
- If a file or its history row is gone before the export, `v4vmm` cannot find
  its embedded art. The hash shows a changed file, but nothing can recover
  the original.

## References

- ADR 0003 (superseded, in `docs/archive/adr/`), ADR 0005, ADR 0008, ADR 0010,
  ADR 0011, ADR 0012, ADR 0013, ADR 0014, ADR 0015
- `musicindex-live-relay` ADR 0003 and ADR 0004
- `mixxx-now-playing/src/display.rs`, `prepare_image`
