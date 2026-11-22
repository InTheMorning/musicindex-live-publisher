# Liquidsoap Task 007: The Chapter Route Lookup And One Route Client

Status: Ready - 2026-11-22. ADR 0025 §The Chapter Route Lookup is accepted.

Every criterion is mechanical. No criterion is visual.

## Goal

The route client of `mixxx-now-playing` moves to `now-playing-drop` and asks the route service with the two GUIDs.
`liquidsoap-now-playing` uses the same client to get the song routes of each identified remote chapter. Task 006 is
complete: `render_state` already selects the service result first, then the file routes, then no routes.

## Base components

| Component | Status | Location | Reason |
| --- | --- | --- | --- |
| Route service client | extend | mixxx-now-playing/src/musicindex.rs | `MusicIndexClient` moves to `now-playing-drop/src/route_service.rs` and gets the request of the pair. Do not keep a copy in `mixxx-now-playing`. |
| Payment route type | reuse | now-playing-drop/src/payment_route.rs | `PaymentRoute` is the type of each route. |
| Song route selection | reuse | liquidsoap-now-playing/src/lib.rs | `select_song_routes` and `SongRoutes` of task 006 do not change. |
| Chapter list | reuse | liquidsoap-now-playing/src/chapters.rs | `parse_chapters` gives the identified pairs. |
| Lookup worker and cache | new | liquidsoap-now-playing/src/lookup.rs | Searched liquidsoap-now-playing/src, mixxx-now-playing/src/musicindex.rs and now-playing-drop/src. `ValueRouteResolver` starts one thread for each request and keys its cache by one track GUID. It does not fit the order and the retry rules. |
| HTTP server for tests | extend | mixxx-now-playing/tests/musicindex.rs | `StubServer` moves to `now-playing-drop/src/test_server.rs` and gets the `Hang` reply. The Mixxx tests use the moved server. Do not keep a copy. |

## Files To Inspect

- `docs/adr/0025-the-station-producer-pays-each-chapter-song.md`, all of §The Chapter Route Lookup, §What The
  Producer Writes and §Verification. It owns each rule of this task.
- `docs/adr/0024-*.md` §The Assembler Examines The Two GUIDs.
- `mixxx-now-playing/src/musicindex.rs`, `mixxx-now-playing/src/main.rs` (the use of `ValueRouteResolver`).
- `liquidsoap-now-playing/src/lib.rs`, `liquidsoap-now-playing/src/chapters.rs`, `liquidsoap-now-playing/src/main.rs`,
  `liquidsoap-now-playing/src/cli.rs`.
- `now-playing-drop/src/lib.rs`, `now-playing-drop/Cargo.toml`.

## Files To Change

- `now-playing-drop/Cargo.toml`: add `reqwest` with the features of `mixxx-now-playing/Cargo.toml`, and `tracing`
  if you need it. Add a feature `test-server` that makes the module `test_server` public outside the crate tests.
- `now-playing-drop/src/route_service.rs` (new), `now-playing-drop/src/test_server.rs` (new),
  `now-playing-drop/src/lib.rs`.
- `mixxx-now-playing/src/musicindex.rs`, `mixxx-now-playing/tests/musicindex.rs`, and `mixxx-now-playing/src/main.rs`
  only if a signature changes.
- `liquidsoap-now-playing/Cargo.toml`: a dev-dependency on `now-playing-drop` with the feature `test-server`.
- `liquidsoap-now-playing/src/lookup.rs` (new), `liquidsoap-now-playing/src/lib.rs`,
  `liquidsoap-now-playing/src/main.rs`, `liquidsoap-now-playing/src/cli.rs`.
- `Cargo.lock`.

## Do Not Touch

- `src/**` (the publisher), `systemd/**`, `packaging/**`, `docs/**`, `AGENTS.md`, `liquidsoap/**`.
- `mixxx-now-playing/src/tags.rs` and the tag names that the Mixxx reader uses. A different task changes them.
- The drop file schema. It stays version 3.
- `select_song_routes`, `identified_pair`, `merged_routes` and `local_split_routes` do not change.

## Constraints

### The client in `now-playing-drop/src/route_service.rs`

- `pub const DEFAULT_ROUTE_SERVICE: &str = "https://api.musicindex.org";`. `mixxx-now-playing/src/config.rs` keeps its
  own constant. Do not change that file.
- `pub struct RouteServiceClient`, `Clone`. `RouteServiceClient::new(base_url, timeout) -> Result<Self>` parses the URL
  as `MusicIndexClient::new` does today.
- `pub enum RouteLookup { Routes(Vec<PaymentRoute>), NoRoutes }`.
- `pub fn track_routes(&self, feed_guid: &str, item_guid: &str) -> Result<RouteLookup>`:
  - `GET <base>/v1/feeds/<feed_guid>/tracks/<item_guid>?include=payment_routes`. Push each GUID as one path segment
    with `path_segments_mut`, after the sanitize step of today. An empty GUID after the sanitize step is an error.
  - HTTP 404 gives `Ok(NoRoutes)`. A success with `data.payment_routes` missing, `null` or an empty array gives
    `Ok(NoRoutes)`. A success with routes gives `Ok(Routes(routes))`.
  - A time-out, a network error, a body that does not parse, and each other HTTP status give `Err`. The error text
    does not contain the response body.
- The client never sends a request with one GUID and never asks `/v1/tracks/...` or `/v1/feeds/<guid>` alone.

### The Mixxx resolver in `mixxx-now-playing/src/musicindex.rs`

- Delete `MusicIndexClient`, `fetch_routes`, `DetailResponse`, `EntityWithRoutes`, `LookupError` and
  `sanitize_api_path_segment` from this file. `ValueRouteResolver` holds a `RouteServiceClient`.
- `resolve_value_routes`: with the two GUIDs, call `track_routes(feed, track)`. `Routes` gives
  `RouteResolution::api` with the routes as JSON. `NoRoutes` and `Err` give the embedded resolution. With `verbose`,
  `Err` prints the warning of today. Delete the second request to `/v1/feeds/<feed_guid>` (ADR 0025 operator
  choice: the Mixxx request with the track GUID alone is a defect against ADR 0024).
- The cache key is the pair `(feed_guid, track_guid)`, not the track GUID alone.
- The public items that `main.rs` and `render.rs` use keep their names, and they operate as today.

### The lookup in `liquidsoap-now-playing/src/lookup.rs`

- `pub struct RouteLookups`. It holds an `Option<RouteServiceClient>`. With `None`, it sends no request and keeps
  only the cache rules. `Producer::new` keeps its signature and has no client.
  `Producer::with_route_service(self, client: RouteServiceClient) -> Self` adds the client.
- One worker thread for each `Producer` with a client. It gets the pairs through a channel and sends the requests one
  at a time, in the order that it gets them. It sends each result back through a second channel. The thread stops
  when its channel closes.
- A pair is `(feed_guid, item_guid)` of an identified remote split: `identified_pair` of `lib.rs` gives `Some`.
  The same pair in two chapters is one pair.
- `update(seq, pairs_in_start_order, now)`:
  - At a new `seq`: keep the results and the retry times of the pairs in the new list only. Clear the set of pairs with
    a warning. Send each pair that has no result and no open request, in the order of the list.
  - At the same `seq`: send each pair whose retry time is at or before `now`, that has no result and no open request.
- `drain(now)`: take each answer from the worker. Ignore an answer for a pair that is not in the current list.
  - `Routes(routes)`: the result is the routes. `NoRoutes`: the result is an empty list. No retry for the two.
  - `Err`: no result. The retry time is `now + 30 s`. Write one `tracing::warn!` for each pair and each `seq`, with
    the two GUIDs and the error. Never log a token or a response body.
- `results(&self) -> &SongRoutes` gives the map for `render_state`.
- In `Producer::poll`, after the fresh, `live` and lock steps: parse the chapters one time. For a jingle, for format 1
  and for a state with no chapters, the pair list is empty. Call `drain(now)`, then `update`, then `render_state` with
  the parsed chapters and `results()`. `render_state` takes the parsed chapters and does not parse them again. A late
  answer thus changes the drop file at the next read, and the `play_id` stays the same.
- The tests of task 006 that set `producer.song_routes` put the routes into the cache of `RouteLookups` with a
  `#[cfg(test)]` method. Their assertions do not change.

### The command line

- `--route-service URL`. The default is `DEFAULT_ROUTE_SERVICE`. `main.rs` makes a `RouteServiceClient` with a time
  limit of 5 seconds and calls `with_route_service`. A URL that does not parse stops the start with an error. Update
  `HELP`.

### The test server in `now-playing-drop/src/test_server.rs`

- `#[cfg(any(test, feature = "test-server"))]`. A `std::net::TcpListener` on `127.0.0.1:0` and one thread. The test
  gives a handler `Fn(&str) -> Reply`, where the argument is the request target (path and query).
  `Reply::Answer { status: u16, body: String }` writes an HTTP/1.1 answer with `Content-Length` and
  `Connection: close`. `Reply::Hang(Duration)` sleeps and then closes with no answer.
- `base_url()` and `requests() -> Vec<String>` (each request target, in order).

### General

- Follow the `rust-dev` skill. No `unwrap` or `expect` outside tests.
- Write each comment in Simplified Technical English (`/home/citizen/.agents/skills/asd-ste100/SKILL.md`).
- Do not commit. Leave the change in the working tree. The main session reads the diff and commits.

## Acceptance Criteria

All are mechanical. Each test uses the test server, `tempfile` and an injected `Instant`. A test that waits for the
worker polls in a loop with a limit of 5 seconds of real time.

`now-playing-drop` (client):

1. The request target is `/v1/feeds/F/tracks/I?include=payment_routes` for the GUIDs `F` and `I`.
2. Routes give `Routes`. 404, an empty array, `null` and a missing field give `NoRoutes`. HTTP 500, a body that is not
   JSON, and a `Hang` longer than the time limit give `Err`.
3. An empty GUID or a GUID of spaces gives `Err` and sends no request.

`mixxx-now-playing`:

4. The resolver with the two GUIDs sends one request to the pair path, and no request to `/v1/feeds/<feed>` alone.
5. The tests of today pass. A test that used the old path changes to the pair path.

`liquidsoap-now-playing`:

6. A format 2 state with two identified chapters and one chapter with one GUID sends two requests, in the order of
   `start_ms`. The paths have the two GUIDs and `include=payment_routes`.
7. An answer with routes gives the merged routes with the service routes, also when the entry has file routes.
8. An answer 404 and an answer with an empty array give the file routes, or no routes. The producer sends no second
   request after 60 seconds of the injected clock.
9. A `Hang` gives the file routes, or no routes, and a second request after 30 seconds of the injected clock, not
   before.
10. A drop file at the chapter time before the answer, then the same `play_id` with the service routes after it.
11. At a new `seq`, a result for a pair that is also in the new list stays: no second request. A result for a pair
    that is not in the new list goes: when that pair comes back at a later `seq`, the producer asks again.
12. A jingle, a format 1 state and a local split send no request.
13. `--route-service` parses. A missing value is an error. The default is `DEFAULT_ROUTE_SERVICE`.

Gate:

14. `cargo build`, `cargo test --workspace`, `cargo clippy --workspace --all-targets -- -D warnings` and
    `cargo fmt --all -- --check` pass.
15. `grep -rn "fetch_routes\|MusicIndexClient" mixxx-now-playing/src` shows nothing.
