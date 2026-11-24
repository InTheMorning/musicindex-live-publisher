# Liquidsoap Task 011: The Station Producer Asks The Route Service For A Song File

Status: Done - 2026-11-23. The operator accepted ADR 0027. Criterion 7 passed on the test station on 2026-11-23.

Criteria 1 to 6 are mechanical. Criterion 7 is a station check for a person.

## Goal

`liquidsoap-now-playing` gets the routes of a song state from the route service with the two state GUIDs, else from
the state (ADR 0027). An imported song file with no routes in the file then pays its song.

## Base components

| Component | Status | Location | Reason |
| --- | --- | --- | --- |
| Route lookup worker and cache | extend | liquidsoap-now-playing/src/lookup.rs | The worker of ADR 0025 also takes the state GUIDs of a song state. Only the log text changes. |
| Route service client | reuse | now-playing-drop/src/route_service.rs | The request of ADR 0025 §The Chapter Route Lookup, with no change. |
| Producer and drop file | extend | liquidsoap-now-playing/src/lib.rs | `poll` gives the song pair to the lookup, and `render_state` selects the song routes. |

## Files To Change

- `liquidsoap-now-playing/src/lib.rs`, `liquidsoap-now-playing/src/lookup.rs`.
- `docs/adr/0027-the-station-producer-asks-the-route-service-for-a-song.md`, and the `Changed by` lines of ADR 0022
  and ADR 0025. `docs/README.md` and `AGENTS.md`.

## Acceptance Criteria

Mechanical, in `liquidsoap-now-playing/src/lib.rs` tests (ADR 0027 §Verification):

1. `a_song_state_asks_for_the_two_state_guids`: a song state of format 1, and of format 2 with an empty chapter list,
   each give one request, `/v1/feeds/feed/tracks/track?include=payment_routes`.
2. `a_song_state_pays_the_service_routes_over_the_state_routes`.
3. `a_song_state_with_no_service_routes_pays_the_state_routes_or_nobody`: for a 404 and for an empty route list, the
   state routes, or empty `value_routes` when the state has no routes. One request only.
4. `a_late_answer_for_a_song_keeps_the_play_id`.
5. `producer_skips_route_requests_for_jingles_live_one_guid_and_local_splits` and
   `an_episode_state_asks_only_for_its_chapter_songs`: no request for the state GUIDs.

Gate:

6. `cargo build --workspace`, `cargo test --workspace`, `cargo clippy --workspace --all-targets -- -D warnings` and
   `cargo fmt --all -- --check` pass.

Station check, for a person:

7. The test station plays an imported song with no routes in the file. The relay gets a payable block with the routes
   of the route service.

Result of criterion 7 (orchestrator, 2026-11-23, bundle `a30d701`): the song "Bulldozer" by Horseheads played with
no routes in the file. The producer got 4 routes from the route service, and the relay got a payable block at seq
991.
