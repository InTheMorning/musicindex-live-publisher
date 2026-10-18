# ADR 0015: Live Segments From v4vmm

Status: Proposed
Date: 2026-10-18

Class: situational. Supersede this record when a different input replaces
the segment command or the segment file.

## Context

When Mixxx stops, the publisher sends the dead block (ADR 0005), which pays
nobody. A boost during talk or during a live act pays nobody. The list of
items that need a new ADR names this "talk break block from `v4vmm`".

The operator recorded a show of thesplitkit.com on 2026-10-17. The research
note is
`~/build/musicindex-research/reports/2026-10-17-pixel-palace-curiohoster-show.md`.
These results apply:

- During the live sets, a block with `type: podcast` paid the show's own
  recipients. Boosts during the live acts paid the show.
- The dashboard sent each switch two or three times, from two send paths. A
  listener client had to dedupe the messages by `blockGuid`.
- No field told an app which band played. The display text and the split were
  one block, so the host could not change one without the other.
- After a pause, the last song stayed the value for 8.5 minutes. A pause
  signal did not stop the payment.

ADR 0014 makes the publisher the only assembler, and makes each producer
report each track that plays, payable or not.

Publisher ADR 0004 says that its CLI sends no live payload, and it rejects a
remote API. `v4vmm` ADR 0059 §Remote Control Uses SSH First controls a remote
host through `ssh`.

## Decision

### Two Inputs

Two inputs tell the publisher what to publish and when:

| Input | Writer | What it states |
|---|---|---|
| The drop file (ADR 0014) | A producer | Which track plays, and its routes, or that the track is not payable |
| The segment file | `v4vmm`, through the `segment` commands | That a talk break or a live set starts or ends, and its routes |

The publisher selects one block from the two inputs (§Selection).

### Live Segments

A live segment is a time in which no track plays and the operator wants a
known split. Examples are a talk break and a live set.

`v4vmm` starts and ends a segment with these commands. It runs them through
`ssh` on the publisher host:

```
musicindex-live-publisher segment start --instance NAME --target NAME < segment.json
musicindex-live-publisher segment end --instance NAME --target NAME
musicindex-live-publisher segment show --instance NAME --target NAME --json
```

- `segment start` reads one JSON object from standard input. No route and no
  text goes in a command argument.
- The command checks the object, and then writes it to the segment file of
  the target. The write uses a temporary file and a rename (mandate 6).
- The publisher daemon watches the segment file, as it watches the drop file.
- `segment end` removes the segment file.
- A second `segment start` replaces the segment, with a new `blockGuid`.

The segment object:

| Field | Type | Required | Description |
|---|---|---|---|
| `schema` | string | yes | `musicindex.segment/1` |
| `kind` | string | yes | `talk` or `live_set` |
| `title` | string | yes | The text that an app shows, for example the band name |
| `song_line` | string or null | no | A second line of display text |
| `value_routes` | list | yes | The routes for this segment, in the shape of the drop file. An empty list selects the dead block. |
| `max_secs` | integer | yes | The maximum length of the segment. 1 to 14400. |

The publisher never adds a route that the object did not supply. It never
changes a route. It never derives a route from `title`.

### The Split Of A Segment Is Set For Each Show

`v4vmm` holds the setting for each show. The setting selects one of these:

- **The dead block:** `v4vmm` sends an empty `value_routes`. This is the
  default for a show with no setting.
- **The show split:** `v4vmm` sends the routes of the show in
  `value_routes`.

The publisher does not keep this setting. It uses only the routes of the
current segment object.

### Selection

| Producer lock | Drop file | Segment | Publisher publishes | Lease |
|---|---|---|---|---|
| Held | Present, with routes | Any | The track. The segment ends. | Keepalive |
| Held | Present, with no routes | Any | The dead block. The segment ends. | Keepalive |
| Held | Absent | Active | The segment block | Keepalive |
| Held | Absent | None | The dead block | Keepalive |
| Free or missing | Any | Any | The dead block one time. The segment ends. | No keepalive |

- A track always wins, payable or not. When a drop file appears, the publisher
  removes the segment file. It publishes the track, or the dead block for a
  track that is not payable.
- A segment ends after `max_secs`. The publisher then removes the segment file
  and publishes the dead block.
- When the producer lock becomes free, the segment ends. This keeps the
  liveness rules of ADR 0005.
- At startup, the publisher removes a segment file that an earlier run left.
  A segment from before a restart is stale.

### The Segment Block

- The block has `type` `podcast`, a new `blockGuid` for each segment, `title`
  from the object, and `line` `[song_line, show]` when the object has
  `song_line`.
- Each split is a decimal string (mandate 4).
- An empty route list gives the dead block, never a block with zero
  destinations (ADR 0005).
- The publisher sends one publish for each change, with
  `Listener-Delay-Secs` (ADR 0011). It sends no duplicate publish of the
  same block.

### The Display State

A target with `display_dir` also sends a display state for the segment
(ADR 0008, ADR 0012):

- `track` holds `title`, and also `songLine` when the object has `song_line`.
- `value` holds the `eventGuid` and the `blockGuid` of the segment block.

So an app can show the band name while the split stays on the show
recipients.

### What Does Not Change

- The dead block, its constant destinations, and the lease rules of ADR 0005.
- The drop file of ADR 0014. A producer writes no segment.
- A station share in a track block. That needs its own ADR.

## Invariants

- A segment never pays a route that its object did not supply.
- A track that plays always ends a segment, payable or not.
- A segment never outlives `max_secs`, the producer lock, or a restart.
- A segment with no routes publishes the dead block.
- No route and no token goes in a command argument.

## Open Questions

1. **A talk show with no Mixxx.** The selection table ends a segment when the
   producer lock is free. A show with no producer gets no segment. A segment
   lease of its own would change ADR 0005.
2. **`show` in `line`.** The segment object has no show name. `v4vmm` can
   send it, or the publisher can read it from the target configuration.

## Before Acceptance

- ADR 0014 is accepted. Without its drop file version 3, a segment can pay
  the station for a non-V4V track. Done on 2026-10-18.
- `v4vmm` records the per-show setting and the `ssh` call in its own ADR,
  which cites this ADR.

## Alternatives Considered

### A Local HTTP API

Rejected for the reasons of ADR 0004: authentication, bind defaults, and a
larger threat model.

### The Segment Split Is Always The Show Split

Rejected. The operator wants a setting for each show. Some shows pay nobody
during talk.

### The Show Split When No Track Plays

Rejected. ADR 0005 removed this as `[target.fallback]`. The station would
collect payments for a non-V4V track.

## Consequences

- `v4vmm` can start a talk break or a live set with a known split and a band
  name, and the publisher stays the only writer.
- The publisher gets a second watched file and three commands.

## References

- ADR 0004, ADR 0005, ADR 0008, ADR 0011, ADR 0012, ADR 0014, ADR 0016 of
  this repository
- `v4vmm` ADR 0059
- `musicindex-live-relay` ADR 0002, ADR 0003 and ADR 0004
- `~/build/musicindex-research/reports/2026-10-17-pixel-palace-curiohoster-show.md`
- `~/build/musicindex-research/reports/curiohoster-architecture.md`
