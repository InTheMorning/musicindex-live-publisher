# ADR 0005: Producer Liveness, The Dead Block And The Relay Lease

Status: Implemented
Date: 2026-09-27

Implemented 2026-09-28: relay lease tasks 001 to 004 are merged. The review
is `docs/reviews/relay-lease-review-checklist.md`.

Amended 2026-10-06 by ADR 0011, at its implementation review. ADR 0011
replaces three rules of §Invariants:

- a block goes through `PublishSchedule`,
- the keepalive stops after `PublishSchedule` releases the dead block,
- the reason about a delay longer than the lease.

The publisher now sends each
block at once, and the relay applies the delay (`musicindex-live-relay` ADR
0004). §Invariants gives the present rules.

Accepted 2026-09-27 by the operator. Only §Publisher Behavior depends on the
relay. The keepalive rules in that section use `musicindex-live-relay` ADR
0002, which defines the keepalive route and the interval.

Amended 2026-09-27: the acceptance no longer waits for the relay ADR or the
Mixxx duration check. The Mixxx check gives evidence for a future MIDI ADR,
not for a rule here. The startup rule now publishes one block for each target.
No decision changed.

Amended 2026-09-28: ADR 0006 is accepted. §Payment Timing Without The MIDI
Connector stays in force as the history-only mode of ADR 0006. No decision
changed.

Amended 2026-09-27 at acceptance: the keepalive stops only after the dead block
for a missing producer goes out. A stream delay longer than the lease can then
not let the lease expire first. The MIDI text cites ADR 0006, and §Verification
cites the result of the Mixxx duration check. No decision was reversed.

## Context

`musicindex-live-relay` proposes that "on air" becomes a lease that only the
broadcaster renews. The proposal is
`docs/plans/live-lease-heartbeat-proposal.md` in that repository. This service
is the broadcaster, so it must renew the lease.

The present design uses one signal for different facts. `mixxx-now-playing`
removes the drop file in these conditions:

- A non-V4V track plays.
- The producer cannot read the tags of a V4V track.
- The expiry timer ends.
- The producer stops.

For each removal, the publisher sends the target's configured fallback block.
By default, that block pays a dead route, `no-v4v-track@example.invalid`. But
`[target.fallback]` can hold real station routes. The station then collects
payments while a non-V4V track plays. That is not permitted, because a
non-V4V track can be copyrighted music.

With a lease, the publisher must know if audio is on air. A removed drop file
cannot tell that. A non-V4V track needs the event to stay on air. A stopped
producer needs the event to go off air.

A private stream uses the relay only, with dedicated clients. It has no RSS
feed. So no static `<podcast:value>` block exists for a client to use when the
relay gives no live block.

The producer does not know the deck play state. It reads the Mixxx history
database, and ADR 0001 lists pause detection as a non-goal. The producer
removes the drop file when the track duration plus a slack ends. That
duration comes from the audio stream headers through lofty
(`mixxx-now-playing/src/tags.rs`, `read_tags`). Nothing verifies it.

## Decision

### Three Facts Have Three Sources

| Fact | Source |
|---|---|
| Is this producer running? | The producer lock |
| Does a payable block play? | The presence of the drop file |
| Who gets paid? | `value_routes` in the drop file |

The producer states facts. The publisher never supplies a payee that the
producer did not supply.

### The Producer Lock

A producer holds an exclusive lock on `.producer.lock` in its drop directory
while it runs. The producer takes the lock before it writes the first drop
file. It waits for the lock, and does not fail, if a different process holds
the lock for a short time.

The publisher tests the lock at each health-check wakeup. It tries a shared
lock without a wait. If it gets the lock, it releases the lock immediately and
records the producer as missing. A missing lock file also means that the
producer is missing.

The kernel releases the lock when the producer process ends, also after
SIGKILL. The lock name starts with a dot, so the watcher ignores it as a drop
file.

The lock file becomes part of the drop-directory contract that ADR 0002 owns.
The drop-file schema does not change.

### The Dead Block

The dead block is a live value block that pays nobody. It pays the lnaddress
`no-v4v-track@example.invalid`. A payment to that address fails, and no money
moves.

The dead block is a constant in the publisher. No configuration can change its
destinations. The publisher removes `[target.fallback]` and the setup option
`--fallback-value-block`.

An explicit dead block is better than an empty relay snapshot. The behavior of
a client that receives `{}` is not known. A client can keep the last block and
pay the previous artist. The dead block gives each client a real block that
pays nobody.

"Fallback" means only the block for an error condition. The fallback is the
dead block.

### Publisher Behavior

| Producer lock | Drop file | Publisher publishes | Lease |
|---|---|---|---|
| Held | Present, with routes | The routes | Keepalive |
| Held | Present, with no routes | The dead block | Keepalive |
| Held | Absent | The dead block | Keepalive |
| Free or missing | Any | The dead block one time | No keepalive |

- The publisher never publishes a block with zero destinations. An empty
  `value_routes` list gives the dead block.
- Each transition publishes its block at once (ADR 0011). The previous block
  never stays live while the lease expires.
- A dead block and a track block go out at once, with the header
  `Listener-Delay-Secs`. The relay delays Socket.IO by that value, so each
  block reaches listeners with the audio (ADR 0011).
- A keepalive carries no content and no header.
- When the producer lock becomes free, the publisher publishes the dead block
  one time, at once. The keepalive stops only after that publish succeeds,
  and only if the producer is still missing. The relay ends the lease one
  lease duration later. The relay keeps the order of its listener timeline,
  so the `{}` of the lease end reaches Socket.IO after the dead block.
- At startup, the publisher publishes one block for each target. It is the
  track block when the producer lock is held and a drop file for that target
  is present. In all other conditions, it is the dead block. This replaces a
  snapshot that a previous run left on the relay.
- While the producer lock is free, the publisher ignores the drop files. A
  killed producer can leave a file, and that file is stale.
- If the publisher process stops, no keepalive goes out, and the relay ends
  the lease.

The relay owns the keepalive route, the interval and the lease duration. This
service does not choose them. This design does not need a relay stop message,
because the dead block gives the immediate clear.

### Payment Timing Without The MIDI Connector

This rule is situational. ADR 0006 limits it to the mode in which the MIDI
connector is not available. It is deleted if ADR 0006 removes that mode.

- The expiry timer never uses an RSS duration or the `TLEN` tag. Both are
  written by parties outside the audio stream. v4vmm can write `TLEN`.
- The expiry timer uses the duration from the audio stream headers, limited to
  a maximum. The default maximum is 600 seconds. `--expiry-max` sets it and
  replaces `--expiry-fallback`.
- A track with no stream duration uses the maximum.
- `duration_secs` in the drop file comes from the same source. ADR 0002 records
  that source and records that nothing verifies it.

The maximum limits the time for which a stopped deck keeps an artist payable.
A wrong or crafted VBR header cannot extend that time past the maximum.

## Invariants

- The publisher never publishes a block with zero destinations.
- The dead block pays no real destination. No configuration changes it.
- The publisher sends a keepalive only while the producer lock is held.
- A removed drop file, an empty route list and a free producer lock each
  publish the dead block before any lease expiry.
- The expiry timer never reads an RSS duration or `TLEN`.
- The expiry timer never exceeds the configured maximum plus the slack.

## Verification

Mechanical. Each invariant gets a unit test in the crate that owns it:

- The publisher: zero destinations, the dead block constant, the keepalive
  condition and the transitions. Watcher tests use `tempfile` and a lock held
  by the test.
- The producer: the lock, the duration source and the maximum.

The Mixxx duration check is done. On 2026-09-27, for a 200.04-second VBR MP3
with no VBR header, the Mixxx deck showed 200.02 s and lofty showed 617.81 s.
So the stream header value can be wrong by a factor of three, and the maximum
is necessary. The evidence is in `docs/architecture/mixxx-interfaces.md`,
§Track Duration.

## Changes At Acceptance

These documents state the present rule. Each one changes in the same commit
as the code that makes it wrong. The task packets in
`docs/plans/relay-lease-keepalive.md` name the owner of each change:

- ADR 0002: record the producer lock and the provenance of `duration_secs`.
  Its invariant "Removing a file means stopped" changes to "Removing a file
  means no payable block". Its invariant "A present file means playing"
  changes to "A present file means a block plays".
- `AGENTS.md` §4: "When playback clears, publish the fallback" changes to
  "When no payable block plays, publish the dead block".
- `README.md` and the configuration runbook: remove `[target.fallback]` and
  `--fallback-value-block`.
- ADR 0004: `config show --json` loses `fallback_configured`. On 2026-09-27,
  no code in `v4vmm/src` read that field, so `v4vmm` needs no change.

## Non-Goals

- A talk break block. v4vmm must publish it while `mixxx-now-playing` owns the
  drop directory. That needs its own decision.
- A station split. v4vmm owns it.
- The MIDI link to Mixxx. ADR 0006 owns it.
- An RSS feed for a private stream.

## Alternatives Considered

### The Producer Owns A Station Fallback

Rejected. Each producer would hold payment configuration, and a non-V4V track
could pay the station.

### Stop The Keepalive When The Drop File Is Removed

Rejected. A non-V4V track removes the drop file. The event would go off air
during a long non-V4V track, and each client would then do something unknown
with `{}`.

### A Non-Payable Drop File For Each Non-V4V Track

Rejected. It needs a new schema version. The producer lock gives the same
separation with no schema change.

### Detect A Missing Producer From The Drop File Age

Rejected. It needs a timer and a rewrite of unchanged files, which AGENTS.md
§6 does not permit. The lock is exact and needs no timer.

### Use The Mixxx Library Duration Now

Deferred. The library value before a track loads probably comes from the
file headers. The deck value after a load probably comes from the decoder.
Neither point is verified.

## Consequences

Positive:

- A removed drop file no longer has two meanings for the lease.
- No configuration can make the station collect payments during a non-V4V
  track.
- A killed producer cannot leave a payable block on air.
- The relay needs only a keepalive. A stop message is optional.

Negative and risks:

- A DJ who stops a V4V track in the middle keeps its artist payable until the
  expiry ends. The maximum limits this, and MIDI removes it.
- A client that shows payment errors shows them for the dead block.
- `config show --json` loses one field. A caller that reads it breaks.

## References

- `docs/plans/relay-lease-keepalive.md`
- `docs/adr/0001-rust-now-playing-lifecycle.md`
- `docs/adr/0002-nowplaying-drop-file-contract.md`
- `docs/adr/0004-publisher-control-cli.md`
- `musicindex-live-relay`: `docs/plans/live-lease-heartbeat-proposal.md`
- `musicindex-live-relay`: `docs/adr/0002-live-lease.md`
