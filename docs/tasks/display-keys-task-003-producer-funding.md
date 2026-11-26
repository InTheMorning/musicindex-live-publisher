# Display Keys Task 003: The Station Producer Gets The Funding Links

Status: Ready - 2026-11-26. ADR 0043 governs this task. Task 002 must be committed before this task starts.

Each criterion is mechanical. No visual check applies to this task.

## Goal

The station producer gets the funding links of the segment that plays from the route service, and writes them as
`funding` in `display.json`. The routes and the drop file do not change.

## Context

- ADR 0043 §The Source gives each rule of this task. ADR 0041 §The Route Lookup gives the cache, the time limit and the
  retry rules, which also apply to the new requests.
- Task 001 made `display_keys::funding_links` and `FundingLink`. Task 002 made the `ShownTrack`
  field `funding`, which the station producer sets to an empty slice.
- `now-playing-drop/src/route_service.rs`: `RouteServiceClient::track_routes` sends
  `GET /v1/feeds/<f>/tracks/<i>?include=payment_routes` and gives a `RouteLookup`. A 404 gives `NoRoutes`.
- `liquidsoap-now-playing/src/lookup.rs`: `RouteLookups` has one worker thread with ordered requests, the results, the
  outstanding requests, the retry times (30 seconds) and one warning for each pair.
- `liquidsoap-now-playing/src/lib.rs`: `Producer::poll` makes the list `pairs` (line 167), calls `update` and `drain`,
  and calls `write_display_track` at each poll. `DisplayOutput::write` does not write equal content. Thus a late answer
  shows at the next poll when `write_display_track` reads the current links.
- `now-playing-drop/src/test_server.rs` is the HTTP server for the tests.

## The Rule

### The Route Service

- `track_routes` (or a replacement that gives the routes and the links) sends `include=payment_routes,source_links`.
  The routes result does not change for each answer that gave routes before.
- Each entry of `data.source_links` has `link_type`, `url`, `label` and `position`. Parse `source_links` as a JSON
  value, and then parse each entry alone:
  - An entry that is not an object, or that has no string `url`, is ignored.
  - A `source_links` that is not a list gives no links.
  - These two conditions do not change the routes and are not an error. They do not cause a retry.
- The funding links of an answer: the entries with `link_type` `"funding"`, in the sequence of `position`, then
  `display_keys::funding_links`. Use a stable sort. An entry with no integer `position` goes after the others.
- A new function gets the channel links: `GET <endpoint>/v1/feeds/<feed_guid>?include=source_links`. It uses the same
  path segment check and the same parse. A 404 gives no links.

### The Lookup

- The lookup list of an episode state is the chapter pairs of `poll` and the pair of the state GUIDs. Add the state
  pair only when the state has the two GUIDs. Put the state pair after the chapter pairs. A song state, a jingle and a `live` state do not
  change. The producer does not use the routes of the state pair of an episode state. `render_state` must continue to
  use the service routes only where ADR 0041 §The Route Lookup says.
- `RouteLookups` keeps the funding links of each pair next to its routes.
- When the answer of a pair has no usable funding link (this includes a 404), the producer sends the feed request for
  the feed GUID of that pair. It sends one feed request for each feed GUID, with a cache by feed GUID. The retry, the
  warning and the time limit rules of a pair apply to a feed GUID. At a new `seq`, keep the feed results of the feed
  GUIDs that are also in the new list of pairs.
- Use the same worker thread for the two request kinds, so that the sequence stays one queue.

### The Display Track

- The segment that plays selects the pair:
  - An identified remote split: the chapter GUIDs (the function `identified_pair`).
  - Each other segment of an episode state, and a song state: the state GUIDs.
  - A jingle: no pair. A `live` state writes no track.
  - A pair with a missing GUID: no pair.
- `funding` for the selected pair: the item links when at least one is usable, else the channel links of its feed GUID,
  else empty. While no answer has come, it is empty. An empty list writes `funding` `null`.
- The drop file does not change. A late answer changes only `display.json`, with the same `play_id`.

## Base components

| Component | Status | Location | Reason |
| --- | --- | --- | --- |
| Route service client | extend | /home/citizen/build/musicindex-live-publisher/now-playing-drop/src/route_service.rs | The same request gets the links. The feed request uses the same client and path check. |
| Lookup cache and worker | extend | /home/citizen/build/musicindex-live-publisher/liquidsoap-now-playing/src/lookup.rs | The links and the feed results use the same cache, retry and worker. |
| Funding rules | reuse | /home/citizen/build/musicindex-live-publisher/docs/tasks/display-keys-task-001-crate-and-publisher.md | `funding_links` in `display-keys/src/lib.rs`, which task 001 makes. |
| Funding key of the display writer | reuse | /home/citizen/build/musicindex-live-publisher/docs/tasks/display-keys-task-002-producer-pub-date.md | The `ShownTrack` field `funding` in `now-playing-drop/src/display.rs`, which task 002 makes. |
| Test HTTP server | reuse | /home/citizen/build/musicindex-live-publisher/now-playing-drop/src/test_server.rs | It answers by the request target. |

## Files To Inspect

- `docs/adr/0043-the-display-state-carries-the-funding-links.md`
- `docs/adr/0041-the-station-producer.md` §The Route Lookup
- `now-playing-drop/src/route_service.rs`, `now-playing-drop/src/test_server.rs`
- `liquidsoap-now-playing/src/lookup.rs`, `liquidsoap-now-playing/src/lib.rs`

## Files Likely To Change

- `now-playing-drop/src/route_service.rs`
- `liquidsoap-now-playing/src/lookup.rs`, `liquidsoap-now-playing/src/lib.rs`

## Do Not Touch

- The publisher `src/` and `tests/display.rs`.
- `display-keys/` and the `display.json` writer of task 002, except a defect that blocks this task. Report such a
  change.
- `liquidsoap/*.liq` and the drop file format.
- `mixxx-now-playing/`. When the signature of `track_routes` changes, update the Mixxx call with no change of
  behaviour. The Mixxx producer continues to use only the routes of the answer.
- `docs/adr/`, `AGENTS.md`, `docs/README.md`.

## Acceptance Criteria

Mechanical. Each item is a test:

- A route service test: the request has `include=payment_routes,source_links`, and the routes do not change.
- A route service test: an answer with correct routes and a `source_links` that is not a list gives the routes and no
  links. An entry that is not an object is ignored, and the other entries stay.
- A producer test: a track answer with two item funding links and one channel link gives the two item links, in the
  sequence of `position`.
- A producer test: a track answer with no item funding link and a feed answer with one link gives that link.
- A producer test: a URL with the scheme `ftp`, a URL of 513 characters, a second entry with the same URL and a fourth
  entry are not in `funding`. A URL with a space, a URL with a control character and `https:///support` are not in
  `funding`. `HTTPS://example.com` is in `funding`. A label of 200 characters gives its first 128 characters. An
  empty label gives no key.
- A producer test: a chapter song and a gap of one episode file get the links of their own GUIDs.
- A producer test: a jingle, a 404 for the track and the feed, and a segment with a missing GUID give `funding` `null`.
- A producer test: a late answer writes `display.json` again with the same `play_id`, and the drop file does not change.
- A producer test: two segments with the same feed GUID and no item link cause one feed request.
- The test `an_episode_state_asks_only_for_its_chapter_songs` changes for ADR 0043: the request list has the new
  `include` value, and it has the state pair after the chapter pairs when the state has the two GUIDs. Rename the test
  to agree.
- The existing route tests pass with no change of their routes.
- The full gate passes.

## Test Commands

```bash
cargo fmt --all -- --check
cargo build --workspace
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
```

## Escalation Triggers

- The routes of a segment change because of this task. Stop and report.
- The worker needs a second thread or a change of the time limit. Stop and report.

A delegate does not commit. The publisher session reads the diff, runs the gate and makes the commit. Write prose and
code comments in Simplified Technical English (`/home/citizen/.agents/skills/asd-ste100/SKILL.md`).
