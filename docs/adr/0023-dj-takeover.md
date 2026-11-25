# ADR 0023: A Live DJ Takes Over The Event Of A Station

- Status: Accepted on 2026-11-21 by the operator. The orchestrator session relayed the decision.
- Date: 2026-11-21
- Bar: crosses repositories and hosts. The station publisher and the DJ publisher send to one relay event. An agent can
  make the opposite choice by mistake: a station publisher that does not send the dead block at a takeover, or a relay
  change.
- Operator requirement, 2026-11-21: when a live DJ connects, the DJ takes over the MusicIndex event. The station
  producer stops for the DJ set. The orchestrator session relayed the requirement.

## Context

A station on Liquidsoap has a producer (ADR 0022) and a publisher. A live DJ connects audio to the station. The DJ
sends the values of the DJ tracks with a different producer and publisher, for example the Mixxx producer on the DJ
computer. The two publishers send to one event with one token.

These rules apply now:

- The relay accepts each publish with the token of the event. The newest publish replaces the snapshot
  (`musicindex-live-relay` ADR 0002).
- A publisher sends the dead block one time when its producer lock becomes free, and then sends no keepalive
  (ADR 0005 §Publisher Behavior).
- `GET /v1/liveitems/{event_id}/metadata` gives the snapshot of the instant timeline, with its `blockGuid`. It needs no
  token (`musicindex-live-relay` ADR 0004).
- The publisher sends a keepalive each `keepalive_interval_secs`. The relay default is 30 seconds.

The problem is the sequence. When a dead block of the station arrives after the first block of the DJ, the dead block
stays. The same occurs at the end of the set with the dead block of the DJ. The listeners then pay nobody until the next
track change of the other side. The lease end does not correct this, because a keepalive does not restore a snapshot.

## Decision

### The Station Side Is Automatic

- The Liquidsoap producer releases its lock while `live` is `true` in the state file (ADR 0022 §What The Producer
  Writes). The station publisher then sends the dead block one time and stops its keepalives.
- When `live` becomes `false`, the producer takes the lock again. The station publisher sends the block of the present
  track.

The station publisher must send the dead block at the takeover. Without it, the last station track stays payable
while the DJ audio plays and the DJ publisher has not sent a block.

### The Publisher Restores Its Block

These rules apply to each publisher, so to the station and to the DJ:

1. At each keepalive time, a publisher with a held producer lock and a payable last block reads
   `GET /v1/liveitems/{event_id}/metadata`.
2. If the snapshot is a dead block with a different `blockGuid`, the publisher sends its last block again, with the
   same `blockGuid`. This publish also renews the lease.
3. In each other condition, the publisher sends the keepalive. A snapshot of a different payable block gives a warning
   that names a second sender. The publisher does not send its block again.
4. A failed read does not stop the keepalive. The publisher sends the keepalive and reads again at the next time.

A **dead block** is a block with each destination at the address of the ADR 0005 dead block. A **payable block** is a
block that is not a dead block. These are project terms.

The result:

- The sequence of the two senders does not matter. The side that has a payable block wins over the dead block of the other
  side in at most one keepalive interval.
- In that interval, the listeners pay nobody. They never pay a wrong artist because of this rule.
- Two senders with payable blocks do not alternate. The newest publish wins, as before.

### The DJ Side Stays Manual

- The DJ starts the DJ producer when the DJ audio is connected, and stops it at the end of the set.
- The DJ publisher needs the token of the station event. Treat that copy as irreplaceable (AGENTS.md §5).

### No Relay Change

The relay does not change. The read-back uses a route that exists.

## Invariants

- A publisher sends its block again only over a dead block of a different `blockGuid`.
- A publisher sends a dead block again never because of this rule.
- A publisher with a free producer lock does no read-back.

## Verification

Mechanical. Unit tests in `src/relay.rs` with a stubbed relay cover each rule of §The Publisher Restores Its Block.
The cases are a foreign dead block, its own block, a foreign payable block, a `404`, a failed read and a free lock.

Manual. A test station has two sequences at the start of a set and two at the end. Each sequence must end with the
correct snapshot after a maximum of one keepalive interval.

## Risks

- **A DJ producer that operates out of the set.** A DJ block that is payable restores itself over the dead block of a
  non-V4V station track. The listeners then pay the DJ track for station audio. Without this rule, the same DJ
  publisher already replaces each station block at each DJ track change. The procedure of §The DJ Side Stays Manual
  prevents the two conditions. With the Mixxx connector, a stopped deck removes the drop file, and the DJ side then has
  no payable block (ADR 0006).
- One extra request goes to the relay for each keepalive.

## Alternatives Considered

### The DJ Publisher Sends Its Block Again At A Fixed Time

Rejected. It corrects only the start of the set, and only one time. The read-back corrects each sequence at each
keepalive.

### The Relay Accepts A Publish Only With The Last `seq` That The Sender Saw

Rejected. It changes the relay contract. Each publisher must read before each publish and must recover from a
conflict. The read-back gets the same result at the start and at the end of a set with no relay change.

### Two Tokens With A Priority

Rejected. It changes the relay contract and the token model. At the end of the set, the DJ lease stays active for a maximum of
90 seconds, and the relay then refuses the station. That needs a stop message, which relay ADR 0002 deferred.

### The Station Sends No Dead Block At A Takeover

Rejected. The last station track stays payable while the DJ audio plays.

## Consequences

- The research tutorial §DJ Takeover replaces the manual sequence with §The DJ Side Stays Manual.
- `src/relay.rs` gets a read of the metadata route.
- This record adds a rule to the publisher. It changes no rule of ADR 0005. Each invariant of ADR 0005 stays true,
  and the publisher sends a publish or a keepalive only while the producer lock is held.

## References

- ADR 0005, ADR 0006, ADR 0016 and ADR 0022 of this repository
- `musicindex-live-relay`: `docs/adr/0002-live-lease.md`, `docs/adr/0004-listener-timeline-delay.md`
- `docs/runbooks/musicindex-producer-tutorial.md`, §DJ Takeover
