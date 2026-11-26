# ADR 0044: A DJ Track Carries Its Funding Links

- Status: Proposed
- Date: 2026-11-26
- Bar: crosses repositories, and an agent can make the opposite choice by mistake. ADR 0043 §The Source says that the
  Mixxx producer writes no `funding`. Rainbrook shows the key.
- Changes: ADR 0043 §The Source. The Mixxx producer also writes `funding`.
- Uses: ADR 0043 §The Key and §The Publisher, and the crate `display-keys`, without a change.
- Operator requirement, 2026-11-26: the funding links of a segment agree with its boost routes. The orchestrator sent
  it. The research note is `research_notes/music-feeds/funding-and-podroll.md` §Funding And Boost Stay In Step
  (`musicindex-research` commit `d9dd4cb`).

## Context

A DJ set plays tracks in Mixxx. The Mixxx producer reads the tags `Feed Guid` and `Track Guid` of the track. When the
two tags have a value, it asks the route service for the routes of the track
(`mixxx-now-playing/src/musicindex.rs:13`, `resolve_value_routes`). It uses the answer of the service when the answer
has routes. Else it uses the routes of the file tags. Thus a DJ track can be payable, and its routes come from the
GUIDs of that track.

ADR 0043 adds `source_links` to the same track request for the station producer. The Mixxx producer uses the shared
client `RouteServiceClient`, so it can get the links from the same answer. Today the Mixxx `display.json` has no key
`funding`, so Rainbrook shows no funding link for a DJ track.

## Decision

### The Key

- A track in the Mixxx `display.json` has `funding` with the rules of ADR 0043 §The Key. The crate `display-keys`
  applies them.
- A Mixxx track has no key `pub_date`. ADR 0042 does not change.

### The Source

- The Mixxx producer gets the links from the answer of the track request that gives the routes:
  `GET <endpoint>/v1/feeds/<feed_guid>/tracks/<item_guid>?include=payment_routes,source_links`, with the GUIDs of the
  tags of the track.
- When the track answer has no usable funding link, the producer sends the feed request of ADR 0043 §The Source. It
  uses the feed GUID of the track.
- The selection, the label and a bad `source_links` follow ADR 0043 §The Source.
- No lookup, and thus `funding` `null`, in these conditions:
  - The route lookup is off.
  - The track has no value in one of the two GUID tags.
  - The service gives 404, or the request fails.
- The Mixxx producer keeps its retry rule. After a failed request, it sends no second request during the same play.
- A late answer writes `display.json` again with the same `play_id`. The drop file gets the routes as before.
- The Mixxx producer keeps the links of a GUID pair and of a feed GUID in its cache, the same as the routes.
- The routes do not change. When the answer has links and no routes, the routes come from the file tags, and
  `funding` comes from the answer. The two come from the GUIDs of the same track.

### The Publisher

The publisher does not change. It sends `funding` from each `display.json` (ADR 0043 §The Publisher).

### The Deployment Order

The relay that accepts `funding` comes first. This record deploys with the Mixxx producer or after it.

## Invariants

These rules apply while this decision is current.

- The funding links of a DJ track come from the GUIDs of that track, not from the GUIDs of a different track.

## Verification

Mechanical. Tests in this repository:

- A Mixxx test: a track with the two GUID tags and an answer with an item funding link gives that link in
  `display.json`.
- A Mixxx test: a track answer with no item funding link and a feed answer with one link gives that link.
- A Mixxx test: a track with one GUID tag, a 404 and a failed request give `funding` `null`.
- A Mixxx test: an answer with links and no routes gives the routes of the tags and the links of the answer.
- A Mixxx test: a late answer writes `display.json` again with the same `play_id`.
- A Mixxx test: the Mixxx `display.json` has no key `pub_date`.

Visual. Only the operator can do this check:

- While a DJ track with a funding link plays, a tap on "Support links" in Rainbrook opens a sheet with that link
  (Rainbrook ADR 0031).

## Alternatives Considered

### No Funding For A DJ Set

The Mixxx producer writes no `funding`, as in ADR 0043. This was not selected: a payable DJ track then shows no funding
link, and the requirement of the operator asks for the two to agree.

## Consequences

- A DJ track that is not in the index shows no funding link. Its routes come from the file tags.
- The Mixxx producer sends one more request for each feed GUID with no item funding link.
