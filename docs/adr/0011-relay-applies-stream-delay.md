# ADR 0011: The Relay Applies The Stream Delay

Status: Proposed
Date: 2026-10-06

Class: situational. Supersede this record with `musicindex-live-relay` ADR
0004.

`musicindex-live-relay` ADR 0004 owns the two timelines, the header and its
limits. This ADR decides what the publisher sends and when.

## Context

The publisher holds each block and each display state for the stream delay of
its target, in `PublishSchedule`. Each relay transport thus gets them late.
ADR 0005 and ADR 0008 depend on that schedule.

Podcast apps apply a block when it arrives. For them the delay is correct, and
the operator wants their behavior to stay as it is.

The private app and the tagger of ADR 0009 wait for an in-band key, the ICY
title, before they apply a display state. They need each state before its key
arrives. The delay makes them late. The tagger is late by almost the full
delay, because the ICY title reaches the VPS 1 to 3 seconds after the track
change.

Relay ADR 0004 gives each event two timelines. Socket.IO and
`GET /remoteValue` wait for the delay that the broadcaster sends with each
publish. SSE and the other reads are instant.

The operator decided on 2026-10-06:

- The broadcaster side sends the delay to the relay.
- `v4vmm` lets the broadcaster change it.
- Socket.IO continues to work as it does for current apps.
- SSE becomes instant.

## Decision

### Send At Once

- The publisher sends each track block, each dead block and each display
  state at once. It does not hold them.
- `PublishSchedule` retires.
- Each publish has the header `Listener-Delay-Secs`. Its value is the
  `stream_delay_secs` of the target, rounded to the nearest whole second.
- A keepalive has no header. It carries no content.

### The Setting

- `stream_delay_secs` keeps its name, its limit of 300 seconds and its place in
  the configuration file. Only its function changes: the publisher sends it to
  the relay and does not wait for it.
- `v4vmm` changes it through the commands of ADR 0004, as before. A change
  applies from the next publish.

### The Lease And The Dead Block

- When the producer lock becomes free, the publisher publishes the dead block
  one time, at once. The keepalive stops after that publish succeeds.
- Relay ADR 0004 keeps the order on the listener timeline. The `{}` of a lease
  expiry thus reaches Socket.IO after the dead block, also when the delay is
  longer than the lease.

### The Display Path

- The publisher reads `display.json` and its image as before, and uploads and
  publishes them at once.
- The display state and the payload of a target no longer share a delay. Both
  go out at once. The display routes of the relay are instant.

### The Order Of Deployment

A relay without ADR 0004 ignores the header. With this ADR and such a relay,
Socket.IO gets each block with no delay. The relay with ADR 0004 must thus be
deployed first.

### What This Replaces

When the operator accepts this ADR:

- ADR 0005 §Invariants loses three rules. This ADR replaces them:
  - a block goes through `PublishSchedule`,
  - the keepalive stops only after `PublishSchedule` releases the dead block,
  - the reason about a delay longer than the lease.
- ADR 0008 loses two items. This ADR replaces them:
  - the invariant that the display state and the payload of one target pass
    through the same stream delay,
  - the schedule rules in §The Publisher.
- Each of the two ADRs gets a dated sentence that names this ADR and the rules
  that it replaces.
- Audit fix 006 put the delay in the publisher. Its decision is replaced. The
  task file stays as a record of finished work.

## Invariants

These rules apply while this decision is current.

- The publisher never holds a block or a display state for the stream delay.
- Each publish carries `Listener-Delay-Secs` from the configuration of its
  target.
- The keepalive stops only after the dead block publish succeeds.

## Before Acceptance

1. **Relay ADR 0004** is accepted.
2. **The `v4vmm` view.** `v4vmm` shows `stream_delay_secs` as the delay that
   podcast apps get, not as a delay of every transport. Its text changes in that
   repository.
3. **The app fallback.** `citizenradio` ADR 0011 has a rule for a session with
   no ICY: a display state applies after the audio that AVPlayer holds. With
   instant display states, that rule shows each state earlier than before.
   The app owner accepts that, or changes the rule.

## Verification After Implementation

Mechanical:

- A publish carries `Listener-Delay-Secs` with the rounded value of the
  target.
- A track change publishes the block at once, with an injected clock.
- A display state is uploaded and published at once.
- A free producer lock gives one dead block at once. The keepalive stops after
  that publish succeeds.
- A keepalive has no header.

Visual. A person must examine these items. Report each one as open until a
person completes the check.

- A podcast app on Socket.IO changes the block at the same time as before,
  with the same `stream_delay_secs`.
- The private app on ICY shows the artwork when the listener hears the track,
  also for a listener with a short buffer.

## Alternatives Considered

### Two Targets, One Delayed And One Instant

Rejected. A drop file names one target. One track would need two drop files
or a fan-out in the publisher, and two relay events.

### Keep The Delay In The Publisher For Socket.IO Only

Not possible. The publisher sends one publish to one event. It cannot give one
transport of the relay a different time.

## Consequences

Positive:

- The ICY-sync fallback and the tagger get each display state and each block
  before the key that releases them.
- The publisher loses its schedule and the rules that tie the keepalive to it.
- Socket.IO keeps the same timing for podcast apps.

Negative and risks:

- The deployment order matters. A publisher with this ADR and an old relay
  gives podcast apps no delay.
- The no-ICY fallback of the private app becomes earlier. See §Before
  Acceptance item 3.

## References

- `docs/adr/0004-publisher-control-cli.md`
- `docs/adr/0005-producer-liveness-and-dead-block.md`
- `docs/adr/0008-display-path.md`
- `docs/adr/0009-hls-track-metadata.md`
- `docs/tasks/audit-fix-task-006-stream-delay-compensation.md`
- `musicindex-live-relay`: `docs/adr/0004-listener-timeline-delay.md`
- `musicindex-live-relay`: `docs/interoperability.md`
- `citizenradio`: `docs/adr/0011-icy-sync-of-the-relay-display.md`
