# ADR 0014: One Assembler, And Each Producer Reports Each Track

Status: Accepted
Date: 2026-10-18

Accepted 2026-10-18 by the operator. No implementation exists. Until it
exists, the producer writes version 2 and removes the drop file for a non-V4V
track.

Class: durable. These are layering and ownership rules.

## Context

Two programs want to set the live value of one event:

- The Mixxx producer states which V4V track plays (ADR 0002, ADR 0005,
  ADR 0006).
- `v4vmm` must start a talk break or a live set when no track plays. ADR 0015
  proposes that input.

The operator recorded a show of thesplitkit.com on 2026-10-17. The research
note is
`~/build/musicindex-research/reports/2026-10-17-pixel-palace-curiohoster-show.md`.
In thesplitkit, one browser dashboard is the only writer. Other systems send
requests to it (`nextBlock`, `buildBlock`). Two writers never race.

ADR 0005 removed `[target.fallback]`, because a station must not collect
payments while a non-V4V track plays. Such a track can be copyrighted music.

Today a non-V4V track and a stopped deck give the same signal. For a non-V4V
history row, the producer removes the drop file
(`mixxx-now-playing/src/main.rs`, `process_track`). ADR 0002 says "Removing a
file means no payable block". So the publisher cannot tell if a track plays.
An input that pays someone while no track plays, such as a segment of
ADR 0015, could then pay that person for a non-V4V track.

ADR 0005 rejected "A Non-Payable Drop File For Each Non-V4V Track". Its reason
was producer liveness: the producer lock gives that fact with no schema
change. That reason stays correct, and the lock stays the source of liveness.
This ADR needs a different fact, if a track plays. The lock cannot give it.

## Decision

### One Assembler

`musicindex-live-publisher` is the only program in this chain that assembles
a live value block or a display state. It is also the only program that
publishes them to the relay.

- A producer writes facts to the drop directory. It does not make a block.
- `v4vmm` sends requests through the commands of this repository. It does not
  make a block and does not publish to the relay.
- The relay continues to accept a publish from each holder of a correct
  token. This rule binds this chain only. The relay does not check it.

### Each Producer Reports Each Track

A producer reports each track that plays, payable or not. This rule applies to
the Mixxx producer and to each future producer.

This ADR owns version 3 of the drop file, `musicindex.nowplaying/3`. It has
the fields of version 2. Only the meaning of presence changes:

| Drop file | Meaning |
|---|---|
| Present, with routes | A payable track plays. |
| Present, with an empty `value_routes` | A track plays that is not payable, for example a non-V4V track, or a V4V track with no readable routes. |
| Absent | No track plays. |

- The Mixxx producer writes a file with an empty `value_routes` for a non-V4V
  history row. It does not remove the file.
- With the MIDI connector, the producer removes the file when the linked deck
  stops (ADR 0006). Without the connector, the expiry timer of ADR 0005 removes
  it.
- A producer that cannot tell if a track plays must not write version 3.
- The publisher reads version 3 only. No one runs the chain in production, so
  no transition period is necessary.

The publisher behavior of ADR 0005 does not change for a drop file alone. A
present file with routes gives the track. A present file with no routes, and
an absent file, give the dead block. The new meaning matters for an input
that pays while no track plays (ADR 0015).

### What Does Not Change

- The dead block, its constant destinations, and the lease rules of ADR 0005.
- The fields of the drop file.
- The payload keys of ADR 0010 for a track.

## Invariants

- Only this repository assembles and publishes a live value block or a
  display state for this chain.
- Each producer reports each track that plays, payable or not.

## Alternatives Considered

### Each Program Publishes Its Own Block

Rejected. Two writers race at the relay, and the payload rules of this
repository would be copied into `v4vmm`. The research of 2026-10-17 shows the
value of one writer.

### A New Field For A Track That Is Not Payable

Rejected. ADR 0002 permits a present file with an empty `value_routes`,
and ADR 0005 gives the dead block for it. A new field
would hold the same fact two times.

## Consequences

- The Mixxx producer changes its non-V4V path. It writes a file with no
  routes, and it does not remove the file.
- On acceptance, ADR 0002 and ADR 0005 got a dated amendment, and
  `AGENTS.md` changed. For version 3, "Removing a file means no payable
  block" becomes "Removing a file means no track plays". Versions 1 and 2 keep
  the earlier meaning.

## References

- ADR 0002, ADR 0005, ADR 0006, ADR 0010 of this repository
- ADR 0015, which uses the new meaning of presence
- `~/build/musicindex-research/reports/2026-10-17-pixel-palace-curiohoster-show.md`
