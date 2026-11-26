# ADR 0027: The Station Producer Asks The Route Service For A Song File

> Archived: superseded by [ADR 0035](../../adr/0035-the-station-producer.md) on 2026-11-26. ADR 0035 restates
> each live rule of this record.

- Status: Superseded by ADR 0035 on 2026-11-26. Accepted on 2026-11-23 by the operator. The orchestrator session relayed the decision.
- Date: 2026-11-23
- Changes: ADR 0022 §What The Producer Writes, the last row of the table ("no routes" gives empty `value_routes`). A
  song state of §The Producer Selects The Lookup gets the routes of §The Song Routes.
- Changes: ADR 0025 §The Chapter Route Lookup, two items. "The time of the lookup" applies also to the state GUIDs of a
  song state. The last item ("The producer sends no request ... for a file with format 1") does not apply to a song
  state.
- Uses: ADR 0024 (a producer asks the route service with the two GUIDs, never with one), without a change.
- Uses: ADR 0025 §The Chapter Route Lookup for the request, the endpoint, the time limit, the cache, the retry and the
  late answer, without a change.
- Bar: opposite choice by mistake. Without this record, an agent can read ADR 0025 and keep the lookup for chapter
  songs only. Then a song file with its two GUIDs and no routes in the file pays nobody.
- Operator choice, 2026-11-23, relayed by the orchestrator session: the producer gets the routes of a song file from
  the route service (option 1). The rejected option 2 is in §Alternatives Considered.
- Operator choice, 2026-11-23, relayed by the orchestrator session: the producer selects the lookup of the VTS routes
  or the lookup of the whole track (§The Producer Selects The Lookup).

## Context

- `station-import` writes a song file with the tags of `write_song_tags` (musicindex ADR 0092 R-STATION-4 item 2).
  These tags are the two GUIDs of the song (musicindex ADR 0093 R-TAG-3). The file has no
  `TXXX:Podcast Value Routes`.
- On the test station, an imported song played with its two GUIDs and `value_routes: []`, and the relay got the dead
  block. The route service gave HTTP 200 with 4 payment routes for the same two GUIDs. Evidence: musicindex-research
  commit 53bf07f, `research_notes/azuracast/test-station-log.md` step 18.
- ADR 0025 §The Chapter Route Lookup asks the route service only for an identified remote split of an episode file.
  ADR 0022 §What The Producer Writes gives empty `value_routes` to a state with no routes.
- The Mixxx producer already asks the route service for a song with the two GUIDs (ADR 0024).

Terms:

- A **song state** and an **episode state** are the states of §The Producer Selects The Lookup. These are project
  terms.
- The **state GUIDs** are `feed_guid` and `track_guid` of the state file. This is a project term.

## Decision

### The Producer Selects The Lookup

The operator rule, in the words of the operator: "The producer should detect if it needs to lookup VTS routes or if it
needs to look up for the whole track." VTS is `podcast:valueTimeSplit`.

For a fresh state with `live` and `jingle` `false`, the producer selects one lookup:

1. The state has a chapter list: the producer looks up the routes of each identified remote split (ADR 0025 §The
   Chapter Route Lookup). This is an **episode state**.
2. The state has no chapter list, and the two state GUIDs are not empty: the producer looks up the routes of the whole
   track with the two state GUIDs. This is a **song state**.
3. Else the producer looks up nothing.

A state of format 1 has no chapter list. A state of format 2 has no chapter list when its list is empty, missing or not
valid (ADR 0025 §The Chapter List).

This record does not change the routes of an episode state or of one of its segments.

### The Song Route Lookup

- The producer asks the route service for the state GUIDs of a song state when it first reads a new `seq`.
- The request, the endpoint, the time limit, the cache, the retry after 30 seconds, the `warn` line and the late answer
  are the same as in ADR 0025 §The Chapter Route Lookup. One worker sends all the requests of the producer.
- A late answer writes the drop file again with the new routes. The `play_id` does not change.
- The producer sends no request for the state GUIDs of an episode state, a jingle, a `live` state or a state with one
  GUID.

### The Song Routes

The producer selects the routes of a song state in this order:

1. The routes of the service result, when they have a percentage. The service has the current value block of the song.
2. Else `value_routes` of the state, when they have a percentage. This applies while no result has come, after a 404
   and after an empty route list.
3. Else empty `value_routes`, as before. The publisher then gives the dead block (ADR 0024).

`value_routes_source` stays `liquidsoap-state` when the routes are not empty.

## Invariants

- The producer never asks the route service with one GUID (ADR 0024).
- A song state with service routes and with routes in the file pays the service routes.
- A song state with no service routes and no routes in the file gives empty `value_routes`.
- An episode state sends no request for its state GUIDs.

## Verification

Mechanical, in `liquidsoap-now-playing` tests:

1. A song state of format 1 and a song state of format 2 with an empty chapter list each give one request, with the
   two state GUIDs.
2. A song state with a service result that has routes writes the service routes, also when the state has routes.
3. A song state with a 404 or an empty route list writes the routes of the state. With no routes in the state, it
   writes empty `value_routes`.
4. A late service answer writes the new routes with the same `play_id`.
5. A jingle, a `live` state, a state with one GUID and an episode state with a chapter send no request for the state
   GUIDs.

A station check for a person:

6. The test station plays an imported song with no routes in the file. The relay gets a payable block with the routes
   of the route service.

## Open Questions

None.

## Alternatives Considered

### `station-import` Writes The Routes In The Song File

The import writes `TXXX:Podcast Value Routes` for a song, as it does for an episode. This changes musicindex ADR 0092
R-STATION-4 item 2, which musicindex owns. Also the routes in the file stay the same after the song feed changes. The
operator rejected this option on 2026-11-23.

### The Producer Also Asks For The State GUIDs Of An Episode

An episode file from `station-import` has its routes in the file. The lookup for the episode is a new rule with no
fault behind it. It can be a later record.

## Consequences

- An imported song file pays its song with the current routes of the route service. A song file needs no tag write
  after its feed changes the value block.
- The producer sends one request for each song. A station with no network access to the route service writes the
  routes of the file, as before.
- The producer test that asserted no request for a format 1 state changes: a format 1 song state now gives a request.

## References

- ADR 0022, ADR 0024, ADR 0025.
- musicindex ADR 0092 R-STATION-4, musicindex ADR 0093 R-TAG-3.
- musicindex-research commit 53bf07f, `research_notes/azuracast/test-station-log.md` step 18.
