# Task 006 — MusicIndex value-route resolution

Part of [Rust now-playing utility plan](../plans/rust-now-playing-utility-plan.md), Phase 3.

## Goal

Prefer authoritative value routes from the MusicIndex API, falling back to the
embedded ID3 frame when lookup fails. Record which source won.

## Files To Inspect

- `docs/plans/rust-now-playing-utility-plan.md` — Value-route resolution
- `~/build/v4vmm/src/api.rs:533` — `fetch_value_routes()`, the lookup order to mirror
- `~/build/v4vmm/src/api.rs:238` — `PaymentRoute` shape
- `~/build/v4vmm/src/api.rs:7` — `DEFAULT_BASE_URL`
- `~/build/musicindex/api.json` — endpoint spec

## Files Likely To Change

- `mixxx-now-playing/src/musicindex.rs` (new)
- `mixxx-now-playing/src/render.rs`
- `mixxx-now-playing/src/main.rs`
- `mixxx-now-playing/tests/musicindex.rs` (new)

## Do Not Touch

- Task 004's sink — the API path produces content, it does not write files
- Task 005's expiry arithmetic

## Constraints

- **The lookup must never sit on the poll path.** It runs on a worker thread. A
  slow or unreachable API must not delay the now-playing line or the first write
  of the metadata file.
- Write the metadata file immediately using the embedded frame. If resolution
  later succeeds for the same `hist_id`, rewrite once. At most two writes per
  track; task 004's dedupe suppresses the second when both agree.
- Discard a late API result whose `hist_id` is no longer current.
- Set a request timeout. Do not let a hung connection leak a worker per track.
- Cache resolved routes by track guid so repeat plays in a long set do not re-hit
  the API.

## Implementation Steps

1. Add `--no-api` to force the embedded-frame path, and `--api-timeout <secs>`
   (default 5).
2. Deserialise `PaymentRoute` with the field set from `v4vmm/src/api.rs:238`:
   `recipient_name`, `route_type`, `split`, `fee`, `address`, `custom_key`,
   `custom_value`. All optional.
3. Implement the lookup in this order, mirroring `fetch_value_routes()`:
   1. `GET /v1/tracks/{track_guid}?include=payment_routes` using
      `TXXX:MusicIndex Track Guid`
   2. `GET /v1/feeds/{feed_guid}?include=payment_routes` using
      `TXXX:MusicIndex Feed Guid`, when no track guid exists or the track lookup
      returns 404
   3. The embedded frame verbatim, on any failure — network error, timeout,
      non-2xx, or unparseable body
4. Base URL comes from task 001's resolved endpoint.
5. Set the provenance marker in the rendered output: `musicindex-api` when the API
   supplied the routes, `embedded-id3` otherwise.
6. Failures are logged under `--verbose` and are never fatal.

## Acceptance Criteria

- API returns routes → marker is `musicindex-api`, routes are the API's.
- Track lookup 404s, feed lookup succeeds → marker is `musicindex-api`.
- API unreachable → marker is `embedded-id3`, routes are the embedded frame's, and
  the metadata file appeared without waiting for the timeout.
- No embedded frame and no API → `Value Routes` line is absent, not empty or null.
- Second play of the same track in one run → no second API request.
- A late result for a superseded `hist_id` is discarded, not written.
- Tests run against a local stub server. No test contacts the real API.

## Test Commands

```bash
cd mixxx-now-playing
cargo clippy -- -D warnings
cargo test musicindex
```

## Expected Final Report

- Files created or changed
- The four fallback cases and their observed markers
- Measured delay between track change and first metadata write with the API
  unreachable — must be well under the timeout
- Confirmation no test performed a real network request

## Escalation Triggers

Stop and ask before proceeding if:

- `/v1/tracks/{guid}` returns a payload shape that does not match `PaymentRoute`.
- `include=payment_routes` is rejected by the live API.
- Threading the worker's result back would require an async runtime — the plan
  specifies `reqwest` blocking on a worker thread, not `tokio`.
