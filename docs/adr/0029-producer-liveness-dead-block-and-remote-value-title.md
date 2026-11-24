# ADR 0029: Producer Liveness, The Dead Block And The RemoteValue Title

- Status: Accepted on 2026-11-24 by the operator. The orchestrator session relayed the two operator choices.
- Date: 2026-11-24
- Supersedes: ADR 0005. ADR 0005 had three "Changed by" lines (ADR 0011, ADR 0014, ADR 0016). This record restates each
  live rule of ADR 0005 and adds the dead block title setting. ADR 0005 moves to `docs/archive/adr/`.
- Changes: ADR 0010 §The Payload For A V4V Track, the item "`title` stays the song", and the invariant that each field
  keeps its meaning, for `title` only (§The Title Of A Track Block).
- Uses, without a change: ADR 0011 (each block goes out at once, and the relay applies the stream delay). ADR 0014
  (each producer reports each track). ADR 0016 §The Producer Lock (the wait limit and the lock rules of a producer).
  ADR 0024 (a payable block needs both GUIDs). ADR 0006 (the MIDI connector and the history-only mode).
- Bar: crosses repositories. The relay passes the payload to the apps. citizenradio reads the dead block title and
  pairs a display track with a payload. An agent can also make the opposite choice by mistake: let a setting change
  the destinations of the dead block.
- Operator choice 1, 2026-11-24: the dead block title is a setting of each target, for example "HeyCitizen Test
  Stream". Each other field of the dead block stays. A jingle still pays nobody.
- Revised: 2026-11-24. The orchestrator session asked for the setting `dead_block_image`. It changes only `image`
  (§Context, §The Dead Block).
- Revised: 2026-11-24. A change between the dead block and a track block gets a new `blockGuid` (§Publisher
  Behavior). On the test station with b9e35b1, a track block had the `blockGuid` of the dead block before it. The
  v4vmusic.com client skipped that track block.
- Changed by: ADR 0031, 2026-11-24. The dead block has `value.destinations` as `[]`. The destination items of §The
  Dead Block, the zero destination item of §Publisher Behavior and the zero destination invariant no longer apply.
- Operator choice 2, 2026-11-24: the `title` of a payable track block is "artist - title", for example "Anni Powell -
  Beyond the Hills". The display state does not change.

## Context

The setting `dead_block_image` came after the Podcast Guru test of f8c5792 on the test station. Podcast Guru changed
the artwork only after a pause and a play. One or two dead blocks with no `image` came before each song. Inference: the
app does not load a new picture after a block with no `image`. The setting tests that inference.

Podcast Guru shows only the `title` field of the `remoteValue` event. On the test station on 2026-11-24, it showed
"No V4V track playing" during each jingle, and only the song title during a song, for example "Beyond the Hills". The
listener then did not see the station name or the artist.

ADR 0005 made the dead block a constant, and its invariant said that no configuration changes it. Its reason was the
destinations: a configured fallback could let a station collect payments while a non-V4V track plays. The title
does not move money. ADR 0010 kept `title` as the song.

## Decision

### Three Facts Have Three Sources

| Fact | Source |
|---|---|
| Is this producer running? | The producer lock |
| Does a payable block play? | The presence of the drop file, with routes and both GUIDs (ADR 0014, ADR 0024) |
| Who gets paid? | `value_routes` in the drop file |

The producer states facts. The publisher never supplies a payee that the producer did not supply.

### The Producer Lock

- A producer holds an exclusive lock on `.producer.lock` in its drop directory while it runs. It takes the lock before
  it writes the first drop file. ADR 0016 §The Producer Lock gives the wait limit and the other producer rules.
- The publisher tests the lock at each health check. It tries a shared lock with no wait. If it gets the lock, it
  releases the lock immediately and records the producer as missing. A missing lock file also means that the producer
  is missing.
- The kernel releases the lock when the producer process ends, also after `SIGKILL`.
- The lock name starts with a dot, so the watcher does not read it as a drop file. The lock file is part of the drop
  directory contract of ADR 0002. The drop file schema does not change for it.

### The Dead Block

The dead block is a live value block that pays nobody. A client gets a real block, and not an empty relay snapshot
`{}`. The behavior of a client with `{}` is not known. Such a client can keep the last block and
pay the last artist. "Fallback" means only this block.

These fields are constants in the publisher. No setting changes them:

- `value.model`: `{"type": "lightning", "method": "lnaddress"}`.
- One destination: name "No V4V payment route", type `lnaddress`, address `no-v4v-track@example.invalid`, split
  "100". A payment to that address fails, so no money moves.
- No `feedGuid` and no `itemGuid`.

The publisher rejects a configuration with a `[target.fallback]` table, and it has no option `--fallback-value-block`.

The `title` is the setting `dead_block_title` of the target:

- When the setting is absent, the title is "No V4V track playing".
- The publisher trims the value. An empty value or a value of more than 256 characters is a configuration error that
  names the target.
- The setting changes only the title. It cannot make the dead block payable.

The `image` is the setting `dead_block_image` of the target:

- When the setting is absent, the dead block has no `image`.
- The publisher trims the value. The value must be an `http` or `https` URL with a host, of at most 2,048 characters.
  Each other value is a configuration error that names the target.
- The setting changes only the image. It cannot make the dead block payable.

### The Title Of A Track Block

This section changes ADR 0010. The `title` of a payable track block is the artist and the title of the drop file:

- "artist - title" when the two are not empty after a trim.
- The title alone when the artist is empty. The artist alone when the title is empty.

The display state does not change. Its `title` stays the song, and its `songLine` stays the line of the producer
(ADR 0008, ADR 0026).

The text is equal to `songLine` when the producer makes the song line from the artist and the title. It can be
different in these conditions. The publisher has only the drop file, so it does not copy the song line:

- The Liquidsoap state has a `song` value (ADR 0026 §The Song Line).
- A chapter segment of an episode file. The display shows the line of the file, and the payload names the chapter
  song that it pays.
- The Mixxx producer runs with `--strip-hyphens`.

`line`, `author` and `podcastName` do not change (ADR 0010).

### Publisher Behavior

| Producer lock | Drop file | Publisher publishes | Lease |
|---|---|---|---|
| Held | Present, payable | The track block | Keepalive |
| Held | Present, not payable | The dead block | Keepalive |
| Held | Absent | The dead block | Keepalive |
| Free or missing | Any | The dead block one time | No keepalive |

- The publisher never publishes a block with zero destinations. An empty `value_routes` list gives the dead block.
- Each block has its own `blockGuid`. A next track gets a new `blockGuid`. A change from the dead block to a track
  block gets a new `blockGuid`, also for the same track. The reverse change also gets a new `blockGuid`. Only a
  rewrite of the same track that stays a track block keeps its `blockGuid`, for example a route change.
- The publisher sends the dead block while the routes of a new track are not ready. It does not keep the block of the
  last track, because that block pays the last artist.
- Each change publishes its block at once, with the header `Listener-Delay-Secs`. The relay applies the delay
  (ADR 0011). A keepalive carries no content and no header.
- When the producer lock becomes free, the publisher publishes the dead block one time, at once. The keepalive stops
  only after that publish succeeds, and only if the producer is still missing. The relay ends the lease one lease
  duration later.
- At startup, the publisher publishes one block for each target: the track block when the lock is held and a drop
  file for that target is present, else the dead block. This replaces a block that an earlier run left on the relay.
- While the producer lock is free, the publisher ignores the drop files. A killed producer can leave a stale file.
- If the publisher process stops, no keepalive goes out, and the relay ends the lease.

The relay owns the keepalive route, the interval and the lease duration (`musicindex-live-relay` ADR 0002).

### Payment Timing Without The MIDI Connector

This rule is situational. ADR 0006 limits it to the history-only mode, in which the MIDI connector is not available.
Delete it when ADR 0006 removes that mode.

- The expiry timer never uses an RSS duration or the `TLEN` tag.
- The expiry timer uses the duration from the audio stream headers, limited to a maximum. The default maximum is 600
  seconds. `--expiry-max` sets it. A track with no stream duration uses the maximum.
- `duration_secs` in the drop file comes from the same source. ADR 0002 records it, and nothing verifies it.

On 2026-09-27, a VBR MP3 file of 200 seconds gave 617.81 seconds from the stream header. Thus the maximum is necessary.
The evidence is in `docs/architecture/mixxx-interfaces.md` §Track Duration.

## Invariants

- The publisher never publishes a block with zero destinations.
- The dead block pays no real destination. No setting changes its model or its destination.
- The publisher sends a keepalive only while the producer lock is held.
- A removed drop file, an empty route list and a free producer lock each publish the dead block before a lease expiry.
- In the history-only mode, the expiry timer never reads an RSS duration or `TLEN`, and never exceeds the configured
  maximum plus the slack.

## Verification

Mechanical, in the publisher:

- The existing tests of zero destinations, the dead block, the keepalive condition and the changes stay green.
- A target with no `dead_block_title` gives the title "No V4V track playing". A target with `dead_block_title =
  "HeyCitizen Test Stream"` gives that title in each dead block of that target, and the other target keeps its title.
- An empty `dead_block_title`, a value of only spaces, and a value of 257 characters each give a configuration error
  that names the target.
- A dead block with a configured title has the same model and destination as the default dead block.
- A target with no `dead_block_image` gives a dead block with no `image`. A target with `dead_block_image =
  "https://example.com/station.png"` gives that `image` in each dead block of that target, with the same model and
  destination.
- An empty `dead_block_image` gives a configuration error that names the target. A value that is not an `http` or
  `https` URL, and a value of more than 2,048 characters, also give that error.
- The same track, first with no routes and then with routes, gives two blocks with two `blockGuid` values.
- A payable drop file with the artist "Anni Powell" and the title "Beyond the Hills" gives the payload `title` "Anni
  Powell - Beyond the Hills". An empty artist gives the title alone.

Mechanical, in the producers: the existing tests of the lock, the duration source and the maximum stay green.

Manual, by a person, because a test cannot reach the app. On the test station, Podcast Guru shows the configured title
during a jingle. It shows "artist - title" during a song. With `dead_block_image`, it shows the artwork of each song with
no pause and play.

## Non-Goals

- A talk break block, a station split, an RSS feed for a private stream.
- A dead block with a payable destination. That needs a new decision.

## Alternatives Considered

- **A station fallback in the producer or in the configuration.** Rejected. A non-V4V track could pay the station.
- **Stop the keepalive when the drop file is removed.** Rejected. A long non-V4V track would take the event off air.
- **Detect a missing producer from the drop file age.** Rejected. The lock is exact and needs no timer.
- **The display `songLine` as the payload title.** Rejected. The publisher sends the payload from the drop file, and
  the display state can come later or not at all.

## Consequences

- A station can show its name and its image during a jingle. The dead block still pays nobody.
- An app that shows only `title` shows the artist of a song.
- citizenradio decides that a block is not payable from the title "No V4V track playing" or from the address host
  `example.invalid`. With a configured title, only the host check applies.
- citizenradio `BoostPairing` pairs a display track with a payload by `blockGuid`. When the display track has no
  `value`, it compares the display title with the payload title. That comparison does not match a payload title of
  "artist - title". The citizenradio session owns that code.

## References

- `docs/archive/adr/0005-producer-liveness-and-dead-block.md`
- `docs/adr/0010-live-value-payload-reference-shape.md`
- `docs/adr/0011-relay-applies-stream-delay.md`
- `docs/adr/0016-instances-events-and-producer-lock.md`
- `docs/runbooks/musicindex-live-publisher-configuration.md` §Dead Block
