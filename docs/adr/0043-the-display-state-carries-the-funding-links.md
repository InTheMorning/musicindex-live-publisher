# ADR 0043: The Display State Carries The Funding Links Of The Track

- Status: Proposed on 2026-11-26.
- Date: 2026-11-26
- Bar: crosses repositories. The rule changes the station producer and the publisher in this repository, the display
  state of `musicindex-live-relay`, and the display of Rainbrook.
- Changes: ADR 0013 §The Producer and §The Publisher. A track in `display.json` version 3 can also have `funding`.
- Changes: ADR 0041 §The Route Lookup and §What The Producer Writes In `display.json`. The lookup also gets the
  funding links, and a late answer can change `display.json`.
- Uses: stophammer ADR 0082 (`source_links` with `link_type` `funding` and `label`), relay ADR 0013 §The Next Key, and
  ADR 0042 for the pattern of an optional display key, without a change.
- Operator choice, 2026-11-26 (research note `musicindex-research` commit `1b6f4d0`,
  `research_notes/music-feeds/funding-and-podroll.md` §The Live Chain): path B. The display state carries the funding
  links of the current track. The live value payload does not change. The live chain does not carry the podroll.

## Context

The tag `podcast:funding` gives a link to a support page. Its parent is `<channel>` or `<item>`. The attribute `url`
is necessary. The node text is a label, and the specification asks for 128 characters or fewer. stophammer keeps a
longer label without a cut (stophammer ADR 0082).

stophammer keeps each funding link with its label (stophammer ADR 0082). The track route and the feed route give them
with `include=source_links`. Each entry has `link_type`, `url`, `label` and `position`. The `url` is `null` when its
scheme is not `http` or `https`. The include parser of stophammer accepts a list with commas (`src/query.rs`).

The station producer asks the route service for the routes of a chapter song and of a song file, with the two GUIDs
(ADR 0041 §The Route Lookup). The default route service is `api.musicindex.org`, which is stophammer.

Two sources of the links were examined:

- **The file tags.** `station-import` receives the funding links of the feed through `musicindex-fetch`, but it maps no
  frame. The frame `WPAY` keeps one URL with no label. A tag source needs a new frame or a new chapter list field
  (musicindex ADR 0093). It also needs a change of `musicindex-lib` and a new tag pass over the collection.
- **The route service.** The producer adds `source_links` to the request that it already sends. It needs no change in
  the files and gives the label.

The relay refuses a display track with a key that it does not know (`musicindex-live-relay/src/lib.rs:2637`). Relay
ADR 0013 §The Next Key adds a key as one table row. The key needs a shape and limits that a test can check. The relay
refuses a display publish body above 8 KiB.

## Decision

### The Key

A track of `display.json` version 3 has one more optional field. The wire key of the display track has the same name.

| Field | Type | Description |
| --- | --- | --- |
| `funding` | array or null | The funding links of the current track, 1 to 3 entries. |

Each entry is an object:

| Key | Type | Rule |
| --- | --- | --- |
| `url` | string, necessary | 1 to 512 characters. The scheme is `http` or `https`, and the URL has a host. No white space and no control character. |
| `label` | string, optional | 1 to 128 characters, with no white space at the start or at the end. No key when the feed gives no text. |

The rules of the relay (relay ADR 0013 §The Form Of funding) make these points exact. The producer and the publisher
apply each one:

- **Scheme.** The compare of `http` or `https` ignores the case. `HTTPS://example.com` is accepted.
- **Host.** After `://`, one character or more comes before the first `/`, `?` or `#`. `https:///support` is refused.
- **White space and control characters.** The Rust functions `char::is_whitespace` and `char::is_control` find them.
- **Label edges.** The label is equal to the result of Rust `str::trim`, which removes Unicode white space.
- **Length.** The two limits count characters, not bytes.
- **The same URL.** Two entries have the same `url` when the two strings are equal, character by character. The
  compare does not change the case and does not parse the URL.

- An entry has no other key.
- Two entries do not have the same `url`.
- In `display.json`, a track with no funding link has `funding` `null`.
- On the wire, a track with no funding link has no key `funding`. The relay refuses `null` and an empty list.

Example:

```json
"funding": [
  {"url": "https://example.com/support", "label": "Support the band"},
  {"url": "https://example.com/merch"}
]
```

Three entries of the maximum size can use more than 3 KiB. The relay has no limit for `artist` and `title`, so the
publisher applies the body rule of §The Publisher.

### The Source

The producer gets the links from the route service with the GUIDs of the segment:

| Segment | GUIDs of the lookup |
| --- | --- |
| An identified remote split | The chapter GUIDs |
| A different segment of an episode file. Examples: a gap, a local split, a remote split that is not identified | The state GUIDs |
| A song state | The state GUIDs |
| A jingle, a `live` state | No lookup. `funding` is `null`. |

- A segment with a GUID that is missing in the row that applies gets no lookup, and `funding` is `null`. For example,
  an identified remote split has two chapter GUIDs, so it gets a lookup when the state has one GUID.

- **The track request.** The producer adds `source_links` to the request of ADR 0041 §The Route Lookup:
  `GET <endpoint>/v1/feeds/<feed_guid>/tracks/<item_guid>?include=payment_routes,source_links`. For the state GUIDs of
  an episode state, the producer sends the same request and does not use its routes. ADR 0041 §The Route Lookup
  continues to own the routes.
- **The feed request.** When the track answer has no usable funding link, the producer sends
  `GET <endpoint>/v1/feeds/<feed_guid>?include=source_links` one time for each feed GUID of the current file.
- **The selection.** The producer keeps each entry with `link_type` `funding` and a `url` that §The Key accepts, in the
  order of `position`. The item links come first. The producer uses the channel links only when no item link is
  usable. It keeps the first 3 entries after it removes a second entry with the same `url`.
- **The label.** The producer removes the white space at the start and at the end of `label` with `str::trim`. An empty
  label gives no key. A label of more than 128 characters gets its first 128 characters, and the producer removes the
  white space at the end again.
- **The cache, the time limit and a missing answer.** The rules of ADR 0041 §The Route Lookup apply to each track
  request, with the pair of GUIDs as the key. The same rules apply to each feed request, with the feed GUID as the key.
  At a new `seq`, the producer keeps the feed results of the feed GUIDs that are also in the new state. A 404 gives no
  links. While no result has come, `funding` is `null`.
- **A bad `source_links`.** The producer ignores an entry that it cannot parse. A `source_links` that is not a list
  gives no links. The two cases do not change the result for the routes, and they are not a reason to ask again.
- **A late answer.** When the links of the segment that plays arrive, the producer writes `display.json` again with the
  same `play_id`. The drop file does not change.
- The producer does not read funding links from the file tags or from the chapter list.
- The drop file does not change.
- The Mixxx producer does not change. Its `display.json` has no `funding`.

### The Publisher

- The publisher adds `funding` to the display track on the wire when `funding` has at least one entry that §The Key
  accepts.
- The publisher sends only the entries that §The Key accepts, at most the first 3. It adds no key when no entry stays.
  A wrong entry thus does not make the relay refuse the full display state.
- **The body rule.** When the display publish body with `funding` is more than 8,192 bytes, the publisher sends the
  body again without `funding`. It writes one `warn` line for each `play_id`.
- Relay ADR 0013 owns the name, the shape and the limits of `funding` on the wire. If the relay row differs from §The
  Key, this record changes before acceptance.

### What Does Not Change

- The live value payload (ADR 0010).
- The drop file.
- The rules of ADR 0039 for `songLine`, `value` and `playId`, and the rules of ADR 0042 for `pub_date`.

### The Deployment Order

1. The relay that accepts `funding`.
2. The producer and the publisher of this record, in one install.
3. Rainbrook that shows the links.

An older relay gives `400 invalid_display` for each display state with `funding`.

## Invariants

These rules apply while this decision is current.

- Each `funding` entry in `display.json` and on the wire passes the rules of §The Key.
- The publisher does not send `funding` as `null` or as an empty list.
- The funding links of a segment come from the GUIDs of that segment, not from the GUIDs of another segment.

## Before Acceptance

1. Done: relay ADR 0013 §The Form Of funding has the row for `funding` (relay commit `4db75a1`). It agrees with §The
   Key. The relay build of `funding` starts when the operator accepts this record.
2. Done: Rainbrook ADR 0031 (Proposed, Rainbrook commit `837d051`) adds a "Support links" button between AirPlay and
   Boost. A tap opens a sheet with one row for each link: the label and the host name, or the host name alone.
3. A request to `api.musicindex.org` with `include=payment_routes,source_links` for a chapter song of episode 158 gives
   `payment_routes` and `source_links` in one answer.

## Verification

Mechanical. Tests in this repository:

- A producer test: a track answer with two item funding links and one channel link gives the two item links, in the
  order of `position`.
- A producer test: a track answer with no item funding link and a feed answer with one link gives that link.
- A producer test: a URL with the scheme `ftp`, a URL of 513 characters, a second entry with the same URL and a fourth
  entry are not in `funding`. A URL with a space, a URL with a control character and `https:///support` are not in
  `funding`. `HTTPS://example.com` is in `funding`. A label of 200 characters gives its first 128 characters. An empty
  label gives no key.
- A producer test: a chapter song and a gap of one episode file get the links of their own GUIDs.
- A producer test: a jingle, a `live` state and a 404 give `funding` `null`.
- A producer test: a late answer writes `display.json` again with the same `play_id`, and the drop file does not change.
- A route service test: the request has `include=payment_routes,source_links`, and the routes do not change.
- A route service test: an answer with valid routes and a `source_links` that is not a list gives the routes and no
  links.
- A producer test: two segments with the same feed GUID and no item link cause one feed request.
- A publisher test: a valid list gives `funding` on the wire. `null`, an empty list and a list with no valid entry give
  no key. A list of five valid entries gives three. A track with `funding` and a body of more than 8,192 bytes is sent
  without `funding`.

Visual. Only the operator can do this check:

- While a chapter song with a funding link plays on the test station, a tap on "Support links" in Rainbrook
  opens a sheet with that link (Rainbrook ADR 0031).

## Alternatives Considered

### The File Tags

`station-import` writes the links into a frame of the file. This gives the links without a network request. It was
not selected: it needs a change of musicindex ADR 0093, of `musicindex-lib` and of `station-import`, and a new tag pass
over the collection. `WPAY` has no label.

### No Key: The App Asks stophammer

The app gets the links from stophammer with `feedGuid` and `itemGuid` (path A of the research note). The operator
selected path B.

## Consequences

- A track that is not in the index shows no funding link.
- A change of the funding links in a feed shows on the station after stophammer reads the feed again. No new tag pass
  is necessary.
- The producer sends one more request for each feed GUID with no item funding link, and one request for the state
  GUIDs of each episode file.
