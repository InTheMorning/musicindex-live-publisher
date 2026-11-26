# Display Keys Task 004: The Mixxx Producer Gets The Funding Links

Status: Implemented - 2026-11-26. ADR 0044 governs this task. It came after task 003, because it uses the
route service functions and `display_keys::file_funding` of task 003.

Each criterion is mechanical. No visual check applies to this task.

## Goal

The Mixxx producer writes `funding` in `display.json`. The order is the links of the route service answer first, then
the file `WPAY` of the track, then `null`. The routes and the drop file do not change.

## Context

- ADR 0044 §The File Link, §The Order and §The Mixxx Producer give each rule of this task. ADR 0043 §The Source gives
  the feed request.
- Task 003 gives the funding links of a track answer, the feed request for the channel links and
  `display_keys::file_funding`.
- `mixxx-now-playing/src/musicindex.rs`:
  - `resolve_value_routes` sends the track request and gives a `RouteResolution`.
  - `ValueRouteResolver::request` starts one thread for each lookup. It has a cache by GUID pair. It keeps a resolution
    in the cache only when the routes came from the service.
- `mixxx-now-playing/src/main.rs`:
  - `process_track` reads the tags and stores a `CurrentTrack`. A track that is not V4V, or a V4V track with tags that
    the producer cannot read, is not payable (ADR 0014).
  - `apply_value_route_result` applies an answer to the current track. It ignores an answer with no service routes now.
  - `apply_display_output` writes `display.json` with `funding: None`.
- The tag reader `musicindex_tags::track::read_tags` (musicindex-lib rev `fad5343`) gives the file frame `WPAY` as the
  item with the key `WPAY` in `TrackTags.tags` (station log step 60).

## The Rule

### The File Link

- The file link of a payable track: `display_keys::file_funding` of the value of the first item of `TrackTags.tags`
  with the key `WPAY`. No item gives an empty list.
- A track that is not payable has no file link and no lookup. Its `funding` is `null`.

### The Fresh Links

- The resolution of a lookup also gives the fresh links: the item links of the track answer when one or more is usable,
  else the channel links of the feed request for its feed GUID.
- The producer sends the feed request only when the track answer has no usable item link. This includes a 404.
- The pair cache keeps the fresh links with the routes, with the rule that it uses now: only when the routes came from
  the service.
- A new cache by feed GUID keeps the links of each successful feed answer. A 404 gives an empty list, which the cache
  also keeps. A failed request is not kept.
- A failed track request gives no fresh links and the embedded routes, as now.

### The Display Track

- `funding`: the fresh links when they are not empty. Else the file link. Else an empty slice, which writes `null`.
- While no answer has come, the fresh links are empty. Thus the first `display.json` of a track has the file link.
- The track uses the file link when the route lookup is off. It also uses the file link when one of the tags
  `Feed Guid` and `Track Guid` has no value.
- An answer for the current track with fresh links writes `display.json` again with the same `play_id`. This is also
  true when the answer has no service routes.
- An answer with no service routes does not change the drop file and the routes, as now.
- The Mixxx `display.json` has no key `pub_date`.

## Base components

| Component | Status | Location | Reason |
| --- | --- | --- | --- |
| File link rule | reuse | /home/citizen/build/musicindex-live-publisher/docs/tasks/display-keys-task-003-producer-funding.md | `file_funding` in `display-keys/src/lib.rs`, which task 003 makes. |
| Route service client | reuse | /home/citizen/build/musicindex-live-publisher/now-playing-drop/src/route_service.rs | Task 003 adds the links and the feed request. |
| Mixxx route resolver | extend | /home/citizen/build/musicindex-live-publisher/mixxx-now-playing/src/musicindex.rs | The resolution and the cache get the links. |
| Mixxx runtime | extend | /home/citizen/build/musicindex-live-publisher/mixxx-now-playing/src/main.rs | `CurrentTrack` keeps the links, and `apply_display_output` writes them. |
| Funding key of the display writer | reuse | /home/citizen/build/musicindex-live-publisher/now-playing-drop/src/display.rs | The `ShownTrack` field `funding`. |
| Test HTTP server | reuse | /home/citizen/build/musicindex-live-publisher/now-playing-drop/src/test_server.rs | It answers by the request target. |

## Files To Inspect

- `docs/adr/0044-the-file-wpay-is-the-funding-fallback.md`
- `docs/adr/0043-the-display-state-carries-the-funding-links.md` §The Source
- `docs/adr/0014-*.md` (the payable track)
- `mixxx-now-playing/src/musicindex.rs`, `mixxx-now-playing/src/main.rs`, `mixxx-now-playing/src/display.rs`
- `now-playing-drop/src/route_service.rs`, `now-playing-drop/src/test_server.rs`
- `display-keys/src/lib.rs`

## Files Likely To Change

- `mixxx-now-playing/src/musicindex.rs`
- `mixxx-now-playing/src/main.rs`
- `mixxx-now-playing/src/display.rs` (the test of task 002 that expects no key `funding`)

## Do Not Touch

- The publisher `src/` and `tests/display.rs`.
- `liquidsoap-now-playing/`, `liquidsoap/`, `display-keys/` and `now-playing-drop/`, except a defect that blocks this
  task. Report such a change.
- The drop file format and the route rules of ADR 0014.
- `docs/adr/`, `AGENTS.md`, `docs/README.md`.

## Acceptance Criteria

Mechanical. Each item is a test:

- A Mixxx test: a track with fresh item links and a `WPAY` gives the fresh links in `display.json`.
- A Mixxx test: a track with no item link, a feed answer with one link and a `WPAY` gives the feed link.
- A Mixxx test: a track with a `WPAY` gives the `WPAY` link with no label in each of these conditions: a 404 for the
  track and the feed, a failed request, a disabled lookup, and a track with only one of the two GUID tags.
- A Mixxx test: the first `display.json` of a track with a pending lookup has the `WPAY` link. A late answer with fresh
  links writes `display.json` again with the fresh links and the same `play_id`.
- A Mixxx test: a late answer with fresh links and no service routes does not change the drop file.
- A Mixxx test: a `WPAY` value with a NUL at the end gives the URL without the NUL.
- A Mixxx test: a track with no link of the two kinds gives `funding` `null`. A track that is not payable gives
  `funding` `null` and sends no request, also when its file has a `WPAY`.
- A Mixxx test: two tracks of one feed with no item link and a successful feed answer cause one feed request.
- A Mixxx test: the Mixxx `display.json` has no key `pub_date`.
- The existing Mixxx route tests pass with no change of their routes.
- The full gate passes.

## Test Commands

```bash
cargo fmt --all -- --check
cargo build --workspace
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
```

## Escalation Triggers

- The routes or the drop file of a track change because of this task. Stop and report.
- `read_tags` does not give `WPAY` in `TrackTags.tags`. Stop and report.

A delegate does not commit. The publisher session reads the diff, runs the gate and makes the commit. Write prose and
code comments in Simplified Technical English (`/home/citizen/.agents/skills/asd-ste100/SKILL.md`).
