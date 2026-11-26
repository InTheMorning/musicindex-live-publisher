# Display Keys Task 003: The Station Producer Gets The Funding Links

Status: Ready - 2026-11-26. ADR 0043 and ADR 0044 govern this task.

Each criterion is mechanical. No visual check applies to this task.

## Goal

The station producer writes `funding` in `display.json` for the segment that plays. The order is the links of the route
service answer first, then the file `WPAY` of the state, then `null`. The routes, the list of lookup pairs and the drop
file do not change.

## Context

- ADR 0044 §The File Link, §The Order and §The Station Producer give each rule of this task. ADR 0043 §The Source gives
  the parse of the answer and the feed request. ADR 0041 §The Route Lookup gives the cache, the time limit and the retry
  rules, which also apply to the feed requests.
- Task 001 made `display_keys::funding_links` and `FundingLink`. Task 002 made the `ShownTrack` field `funding`. The
  station producer sets it to an empty slice now (`liquidsoap-now-playing/src/lib.rs`, `write_display_track`).
- `now-playing-drop/src/route_service.rs`: `RouteServiceClient::track_routes` sends
  `GET /v1/feeds/<f>/tracks/<i>?include=payment_routes` and gives a `RouteLookup`. A 404 gives `NoRoutes`.
- `liquidsoap-now-playing/src/lookup.rs`: `RouteLookups` has one worker thread with ordered requests, the results, the
  outstanding requests, the retry times (30 seconds) and one warning for each pair.
- `liquidsoap-now-playing/src/lib.rs`: `Producer::poll` makes the list `pairs`. A song state gives its state pair
  (`song_pair`). An episode state gives the identified chapter pairs (`identified_pair`). A jingle gives no pair.
- `poll` calls `drain` and `update`, and calls `write_display_track` at each poll. `DisplayOutput::write` does not write
  equal content. Thus a late answer shows at the next poll.
- `mixxx-now-playing/src/musicindex.rs` calls `track_routes` (`resolve_value_routes`).
- Station log step 60: Liquidsoap gives the key `wpay` with a NUL character at the end.

## The Rule

### The File Link

- `display-keys` gets `pub fn file_funding(raw: &str) -> Vec<FundingLink>`. It removes each NUL character at the end of
  `raw`, then gives `funding_links([(value, None)])`. An empty value gives an empty list.
- Slot text `liquidsoap/musicindex-state.liq`: add `wpay = m["wpay"]` after the `pub_date` line, with the same form.
  The format stays `musicindex-liquidsoap-state/2`.
- `StateFile`: add `wpay: Option<String>` with `#[serde(default)]`.

### The Route Service

- `track_routes` (or a replacement that gives the routes and the links) sends `include=payment_routes,source_links`.
  The routes result does not change for each answer that gave routes before. A 404 gives no routes and no links.
- Each entry of `data.source_links` has `link_type`, `url`, `label` and `position`. Parse `source_links` as a JSON
  value, and then parse each entry alone:
  - An entry that is not an object, or that has no string `url`, is ignored.
  - A `source_links` that is not a list gives no links.
  - These two conditions do not change the routes and are not an error. They do not cause a retry.
- The funding links of an answer: the entries with `link_type` `"funding"`, in the sequence of `position`, then
  `display_keys::funding_links`. Use a stable sort. An entry with no integer `position` goes after the others.
- A new function gets the channel links: `GET <endpoint>/v1/feeds/<feed_guid>?include=source_links`. It uses the same
  path segment check and the same parse. A 404 gives no links.
- Update the Mixxx call with no change of behavior. The Mixxx producer continues to use only the routes of the
  answer. Task 004 adds the Mixxx funding.

### The Lookup

- The list of lookup pairs does not change. The producer sends no request for the state pair of an episode state.
- `RouteLookups` keeps the funding links of each pair with its routes.
- When the answer of a pair has no usable funding link (this includes a 404), the producer sends the feed request for
  the feed GUID of that pair. It sends one feed request for each feed GUID, with a cache by feed GUID. The retry, the
  warning and the time limit rules of a pair apply to a feed GUID. At a new `seq`, keep the feed results of the feed
  GUIDs that are also in the new list of pairs.
- Use the same worker thread for the two request kinds, so that the sequence stays one queue.

### The Display Track

The segment that plays selects the fresh links and the file link. Use the same segment selection as the drop file of
the same read.

| Segment | Fresh links | File link |
| --- | --- | --- |
| A song state | Its state pair, when the state has the two GUIDs | `file_funding` of the state `wpay` |
| An identified remote split | Its chapter pair (`identified_pair`) | None |
| A different segment of an episode state (a gap, a local split, a remote split that is not identified, the part after the last chapter) | None | `file_funding` of the state `wpay` |
| A jingle | None | None |

- The fresh links of a pair: the item links when one or more is usable, else the channel links of its feed GUID.
- `funding`: the fresh links when they are not empty. Else the file link. Else an empty slice, which writes `null`.
- While no answer has come, and after a failed request or a 404, the fresh links are empty.
- The drop file does not change. A late answer changes only `display.json`, with the same `play_id`.

## Base components

| Component | Status | Location | Reason |
| --- | --- | --- | --- |
| Funding rules | extend | /home/citizen/build/musicindex-live-publisher/display-keys/src/lib.rs | `file_funding` uses `funding_links`. Task 004 uses it too. |
| Route service client | extend | /home/citizen/build/musicindex-live-publisher/now-playing-drop/src/route_service.rs | The same request gets the links. The feed request uses the same client and path check. |
| Lookup cache and worker | extend | /home/citizen/build/musicindex-live-publisher/liquidsoap-now-playing/src/lookup.rs | The links and the feed results use the same cache, retry and worker. |
| State file | extend | /home/citizen/build/musicindex-live-publisher/liquidsoap-now-playing/src/state.rs | `StateFile` gets the optional field `wpay`. |
| Slot text | extend | /home/citizen/build/musicindex-live-publisher/liquidsoap/musicindex-state.liq | `musicindex_write` gets one line. |
| Funding key of the display writer | reuse | /home/citizen/build/musicindex-live-publisher/now-playing-drop/src/display.rs | The `ShownTrack` field `funding`. |
| Test HTTP server | reuse | /home/citizen/build/musicindex-live-publisher/now-playing-drop/src/test_server.rs | It answers by the request target. |

## Files To Inspect

- `docs/adr/0044-the-file-wpay-is-the-funding-fallback.md`
- `docs/adr/0043-the-display-state-carries-the-funding-links.md` §The Key and §The Source
- `docs/adr/0041-the-station-producer.md` §The Route Lookup
- `display-keys/src/lib.rs`
- `now-playing-drop/src/route_service.rs`, `now-playing-drop/src/test_server.rs`
- `liquidsoap-now-playing/src/lookup.rs`, `liquidsoap-now-playing/src/lib.rs`, `liquidsoap-now-playing/src/state.rs`
- `liquidsoap/musicindex-state.liq`
- `mixxx-now-playing/src/musicindex.rs`

## Files Likely To Change

- `display-keys/src/lib.rs`
- `now-playing-drop/src/route_service.rs`
- `liquidsoap-now-playing/src/lookup.rs`, `liquidsoap-now-playing/src/lib.rs`, `liquidsoap-now-playing/src/state.rs`
- `liquidsoap/musicindex-state.liq`
- `mixxx-now-playing/src/musicindex.rs` (the call only)

## Do Not Touch

- The publisher `src/` and `tests/display.rs`.
- `now-playing-drop/src/display.rs`, except a defect that blocks this task. Report such a change.
- The drop file format.
- The Mixxx behaviour. Task 004 owns it.
- `docs/adr/`, `AGENTS.md`, `docs/README.md`.

## Acceptance Criteria

Mechanical. Each item is a test:

- A `display-keys` test: `file_funding("https://example.com/pay\0")` gives one link `https://example.com/pay` with no
  label. `file_funding("")`, `file_funding("\0")` and `file_funding("ftp://x")` give an empty list.
- A route service test: the request has `include=payment_routes,source_links`, and the routes do not change.
- A route service test: an answer with correct routes and a `source_links` that is not a list gives the routes and no
  links. An entry that is not an object is ignored, and the other entries stay.
- A route service test: the feed request has the path `/v1/feeds/<f>` and `include=source_links`. A 404 gives no links.
- A producer test: a track answer with two item funding links and one channel link gives the two item links, in the
  sequence of `position`.
- A producer test: a track answer with no item funding link and a feed answer with one link gives that link.
- A producer test: a URL with the scheme `ftp`, a URL of 513 characters, a second entry with the same URL and a fourth
  entry are not in `funding`. A URL with a space, a URL with a control character and `https:///support` are not in
  `funding`. `HTTPS://example.com` is in `funding`. A label of 200 characters gives its first 128 characters. An
  empty label gives no key.
- A producer test: a song state with fresh links and a `wpay` gives the fresh links.
- A producer test: a song state with a `wpay` gives the `wpay` link with no label in each of these conditions: no answer
  yet, a 404 for the track and the feed, a failed request, and a state with a missing GUID. A late answer with fresh
  links then writes `display.json` again with the same `play_id`, and the drop file does not change.
- A producer test: a gap, a local split and the part after the last chapter of an episode state with a `wpay` give the
  `wpay` link. The request list has no state pair.
- A producer test: an identified remote split with no fresh link gives `funding` `null`, also when the state has a
  `wpay`. An identified remote split with fresh links gives the fresh links.
- A producer test: a jingle with a `wpay` gives `funding` `null`.
- A producer test: two segments with the same feed GUID and no item link cause one feed request.
- A state test: a `state.json` with no `wpay` parses, and `wpay` is `None`.
- The test `an_episode_state_asks_only_for_its_chapter_songs` gets only the new `include` value.
- The existing route tests and the Mixxx tests pass with no change of their routes.
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
- The segment selection of the drop file is not available to `write_display_track` with no change of the drop file.
  Stop and report.

A delegate does not commit. The publisher session reads the diff, runs the gate and makes the commit. Write prose and
code comments in Simplified Technical English (`/home/citizen/.agents/skills/asd-ste100/SKILL.md`).
