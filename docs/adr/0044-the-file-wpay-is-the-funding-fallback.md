# ADR 0044: The File WPAY Is The Funding Fallback

- Status: Accepted on 2026-11-26 by the operator.
- Date: 2026-11-26
- Bar: crosses repositories, and an agent can make the opposite choice by mistake. Musicindex ADR 0101 (Accepted)
  writes the frame `WPAY`, and Rainbrook shows the key `funding`. Without this record, an agent follows ADR 0043, which
  reads no file frame and gives the Mixxx producer no `funding`.
- Changes: ADR 0043 §The Source. The funding link of a segment uses the same order as its routes: the fresh answer of
  the route service first, then the file `WPAY`. The Mixxx producer also writes `funding`. The parts of an episode file
  that are not an identified remote split use the file `WPAY` only.
- Uses: ADR 0041 §The Song Routes and §The Route Lookup, ADR 0043 §The Key and §The Publisher, and the crate
  `display-keys`, without a change.
- Operator requirement, 2026-11-26: the funding links of a segment agree with its boost routes. The orchestrator sent
  it. The research note is `research_notes/music-feeds/funding-and-podroll.md` §Funding And Boost Stay In Step
  (`musicindex-research` commit `d9dd4cb`).
- Operator choice, 2026-11-26: "wpay should work like value routes; fresh first, id3 fallback second". The
  orchestrator sent it.

## Context

Musicindex ADR 0101 (Accepted) writes one funding link with no label into the frame `WPAY` of a song file. Rule 5 of
the request of that record says that the funding link of a segment comes from the same source as its routes.

The routes of a song come from the route service first, and from the file tags second (ADR 0041 §The Song Routes). The
Mixxx producer uses the same order (`mixxx-now-playing/src/musicindex.rs:13`, `resolve_value_routes`). The routes of
an episode part that is not an identified remote split come from the file tags only (ADR 0041).

Station log step 60 gives these facts:

- Liquidsoap 2.4.6 gives the file frame `WPAY` as the metadata key `wpay`. A value written by `mutagen` has a NUL
  character at the end, because `mutagen` writes it into the frame.
- The Mixxx tag reader `musicindex_tags::track::read_tags` (musicindex-lib rev `fad5343`) gives the item `WPAY` in
  `TrackTags.tags`.
- Liquidsoap gives one `CHAP` frame as raw binary data, so the `WPAY` subframe of each chapter does not reach the slot
  text.

## Decision

### The File Link

- The slot text writes `wpay = m["wpay"]` into `state.json`. The format stays `musicindex-liquidsoap-state/2`, because
  the field is optional (ADR 0041 §The State File Contract). The value is empty when the file has no `WPAY`.
- The Mixxx producer reads the item `WPAY` of `TrackTags.tags`.
- Each producer removes each NUL character at the end of the value. Then the rules of ADR 0043 §The Key apply, through
  `display_keys::funding_links`. A value that the rules refuse gives no file link.
- The file link has no label.

### The Order

The funding of a segment, in this order:

1. The fresh links: the funding links of the route service answer for the GUIDs of the segment. When the track answer
   has no usable item link, the producer sends the feed request of ADR 0043 §The Source. The rules of that section
   apply.
2. Else the file link.
3. Else `null`.

While no answer has come, and after a failed request or a 404, the producer uses step 2. When a late answer gives
fresh links, the producer writes `display.json` again with the same `play_id`. The drop file does not change.

### The Station Producer

| Segment | Fresh links | File link |
| --- | --- | --- |
| A song state | The state GUIDs | The state `wpay` |
| An identified remote split | The chapter GUIDs | None. The chapter path is open. |
| A different segment of an episode file. Examples: a gap, a local split, a remote split that is not identified | None. No lookup. | The state `wpay` |
| A jingle | None | None. `funding` is `null`. |

- The producer does not ask the route service for the state GUIDs of an episode state. This replaces that request in
  ADR 0043 §The Source.
- A song state with a missing GUID gets no lookup, and uses the state `wpay`.
- The `WPAY` subframe of a `CHAP` frame is not read. A subsequent record decides the chapter path. Until then, an
  identified remote split with no fresh link has `funding` `null`.

### The Mixxx Producer

- A Mixxx track uses §The Order with the GUIDs of its tags `Feed Guid` and `Track Guid`. The fresh links come from the
  answer of the request that also gives the routes (`include=payment_routes,source_links`).
- The Mixxx producer keeps the links of a GUID pair and of a feed GUID in its cache, the same as the routes.
- When the route lookup is off, or the track has no value in one of the two GUID tags, the track uses the file link.
- The Mixxx `display.json` has `funding`: an array, or `null`. It has no `pub_date` (ADR 0042).

### The Publisher

The publisher does not change. It sends `funding` from each `display.json` (ADR 0043 §The Publisher).

### The Deployment Order

1. The relay that accepts `funding` (relay commit `1dec0b3`).
2. The slot text and the two producers of this record, in one install.
3. Rainbrook that shows the links.

## Invariants

These rules apply while this decision is current.

- The funding of a segment comes from the source of its routes: the route service for the same GUIDs, else the file
  that gives the routes.
- A fresh link always comes before the file link.

## Verification

Mechanical. Tests in this repository:

- A producer test: a song state with fresh links and a `wpay` gives the fresh links.
- A producer test: a song state with no fresh link, a 404, or no answer yet, and a `wpay`, gives the `wpay` link with no
  label. A late answer with fresh links writes `display.json` again with the same `play_id`.
- A producer test: a gap and a local split of an episode file give the state `wpay`, and the producer sends no request
  for the state GUIDs.
- A producer test: an identified remote split with no fresh link gives `funding` `null`, also when the state has a
  `wpay`.
- A producer test: a `wpay` with a NUL at the end gives the URL without the NUL. A `wpay` of `ftp://x` gives no link.
- A Mixxx test: fresh links come before the file `WPAY`. A 404, a failed request, a disabled lookup and a track with one
  GUID tag give the file `WPAY`. No link gives `funding` `null`.
- A Mixxx test: the Mixxx `display.json` has no key `pub_date`.
- A state test: a `state.json` with no `wpay` parses.

Visual. Only the operator can do this check:

- While a song with a `WPAY` and no funding link in the index plays on the test station, a tap on "Support links" in
  Rainbrook opens a sheet with the link of the file (Rainbrook ADR 0031).

## Before Acceptance

1. Done: musicindex ADR 0101 has the status Accepted (musicindex commit `cb62f9f`), with the frame `WPAY` as this
   record uses it.

## Alternatives Considered

### The Route Service Only

The producers read no file frame, as in ADR 0043. This was not selected: a track that is not in the index then has
routes from its file and no funding link.

### The File First

The file link comes before the fresh links. This was not selected: the operator selected the order of the routes.

## Consequences

- A track that is not in the index shows the funding link of its file, with no label.
- An episode file sends one request less for each file.
- An identified remote split that is not in the index shows no funding link until a record decides the chapter path.
