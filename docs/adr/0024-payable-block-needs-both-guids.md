# ADR 0024: A Payable Block Needs The Feed GUID And The Track GUID

- Status: Proposed
- Date: 2026-11-21
- Changes: ADR 0014 items: §Each Producer Reports Each Track, the row "Present, with routes".
- Bar: crosses repositories. HitIt writes the GUID tags, each producer reads them, and this repository assembles the
  block. An agent can also make the opposite choice by mistake: a route lookup with one GUID.
- Operator requirement, 2026-11-21: the feed GUID and the track GUID are always present together, so that a track GUID
  cannot collide. A track GUID is unique only in its feed. The orchestrator session relayed the requirement.

## Context

- ADR 0002 §Fields gives `feed_guid` and `track_guid` in the drop file as "string or null", "when known".
- ADR 0014 gives a payable track for each drop file with routes. It does not examine the GUIDs.
- The Mixxx producer asks the MusicIndex API for routes when the track GUID or the feed GUID is present
  (`mixxx-now-playing/src/musicindex.rs`, `RouteRequest::has_lookup_key`). A track GUID alone can thus find the routes
  of a different track with the same GUID in a different feed. Then a wrong artist is paid.
- The research session examined 12 media files on the test station. Each file had the two GUID tags.

## Decision

### The Assembler Examines The Two GUIDs

The publisher is the only assembler (ADR 0014). It applies this rule for each producer:

| Drop file | Publisher publishes |
| --- | --- |
| Routes, and `feed_guid` and `track_guid` are strings that are not empty | The routes |
| Routes, and `feed_guid` or `track_guid` is missing or empty | The dead block, and a warning that names the missing GUID |
| No routes | The dead block (ADR 0014) |

- The drop file stays version 3. No field changes. A file with routes and one GUID is valid. It is a track that plays
  and is not payable.
- The show log records the track as the drop file gives it.

### Each Producer Examines The Two GUIDs

The rule of the assembler is the guard. Each producer also applies it, so that a drop file does not state a payable
track that the publisher refuses:

- A producer writes routes only when it has the two GUIDs.
- A producer asks a route service with the two GUIDs, never with one.
- The Liquidsoap producer applies the rule in ADR 0022 §What The Producer Writes.

### HitIt

HitIt writes the tags (`src/audio_tags.rs` in the HitIt repository). This record needs no HitIt change, because a
file with one GUID is not payable. A HitIt rule that writes the two GUID tags together is advisory here. A HitIt
decision record owns it.

## Invariants

- The publisher never publishes routes from a drop file without a feed GUID and a track GUID.
- No producer asks a route service with one GUID.

## Verification

Mechanical. Each invariant gets a unit test:

- The publisher: routes with no `feed_guid`, routes with no `track_guid`, and routes with an empty GUID each give the
  dead block.
- The Mixxx producer: a request with one GUID does not go to the route service.

`AGENTS.md` §4 gets this payment rule in the same commit as its test.

## Alternatives Considered

### A Drop File Version 4 With Required GUIDs

Rejected. A non-V4V track has no GUIDs, and ADR 0014 requires a drop file for it. The two fields thus stay optional,
and the payable condition gets the rule.

### Only The Producers Examine The GUIDs

Rejected. The publisher is the only assembler, so it is the guard for each producer, also for a future producer.

## Consequences

- A track with routes and one GUID pays nobody. On the test station, no file has this condition.
- ADR 0014 gets a "Changed by" line for this record.

## References

- ADR 0002, ADR 0014 and ADR 0022 of this repository
- HitIt: `src/audio_tags.rs`
