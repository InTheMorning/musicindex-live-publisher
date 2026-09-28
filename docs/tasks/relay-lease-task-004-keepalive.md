# Relay Lease Task 004: The Keepalive

Status: Blocked - 2026-09-27. It needs task 003 here, and
`musicindex-live-relay` ADR 0002 with its tasks 001 and 002.

Every criterion is mechanical. This packet has no visual criteria, because it
adds no user interface.

## Goal

While the producer runs, each relay worker renews the relay lease at the
interval that the relay gives. When the producer goes missing, the worker
stops the keepalive. When the relay reports an expired lease, the worker
publishes its last payload again.

## Files To Inspect

- `docs/adr/0005-producer-liveness-and-dead-block.md` (§Publisher Behavior)
- `musicindex-live-relay`: `docs/adr/0002-live-lease.md` (§The Keepalive Route
  and §Wire Changes)
- `src/relay.rs` (`RelayClient`, `PublishResponse`, `PublishOutcome`,
  `RelayPublisher`, `publish_worker`)
- `src/main.rs` (`run_watch_loop` and the producer state from task 003)
- `src/liveness.rs`
- `tests/relay.rs` (the `StubServer` pattern)

## Files Likely To Change

- `src/relay.rs`
- `src/main.rs`
- `tests/relay.rs`
- `docs/architecture/broadcast-chain-boundaries.md` (the relay contract row)

## Do Not Touch

- `mixxx-now-playing/**`
- `src/watcher.rs`, `src/livevalue.rs`, `src/config.rs`, `src/schedule.rs`
- The payload shape. The keepalive has no body.
- `musicindex-live-relay/**`

## Constraints

- The keepalive is `POST {endpoint}/v1/liveitems/{event_id}/keepalive` with the
  bearer token and no body. Build the URL with `build_url`.
- Read `keepalive_interval_secs` from the publish response and from the
  keepalive response. Make the field optional with `#[serde(default)]`.
- **The worker sends a keepalive only after a relay gave an interval.** A relay
  without the lease never receives a keepalive.
- The worker sends a keepalive only while the producer state is `Running`.
- Map the keepalive status codes:
  - `200`: renewed. Store the new interval.
  - `409`: the lease expired. Publish the last accepted payload again with
    the same `blockGuid`.
  - `401`, `403`, `404`: fatal, the same as a publish.
  - `429`, a `5xx` status and a network error: retryable with the present
    backoff.
  - Any other status: fatal.
- A new payload always goes before a keepalive. Use `recv_timeout` until the
  next keepalive time. A received payload resets that time, because a publish
  renews the lease.
- The main loop tells each worker about producer changes through the same
  channel as the payloads. Replace `mpsc::Sender<LiveValuePayload>` with a
  sender of an enum with `Publish(LiveValuePayload)` and
  `Producer(ProducerState)`.
- When the producer goes missing, the main loop sends `Producer(Missing)` to a
  worker only after `PublishSchedule` releases the dead block for that
  transition, and only if the producer is still missing at that time. The
  stream delay can be longer than the lease, so an earlier stop could let the
  lease expire before the dead block goes out.
- `Producer(Running)` is sent at once.
- No token in a log line, an error message or a `Debug` output.

## Implementation Steps

1. Add `keepalive_interval_secs: Option<u64>` to `PublishResponse`. Return it
   in `PublishOutcome::Accepted`.
2. Add `RelayClient::keepalive(&self, target: &RelayTarget) ->
   Result<KeepaliveOutcome>` with the status mapping in the constraints.
3. Add the worker command enum. Change `RelayPublisher::publish` to send
   `Publish`. Add `RelayPublisher::set_producer(state)`, which sends
   `Producer(state)` to each worker.
4. Change `publish_worker` so that it keeps the last accepted payload, the
   interval, the producer state and the next keepalive time. Follow the
   constraints for the order of work.
5. In `run_watch_loop`, call `set_producer` on each producer change from task
   003. Call it one time at startup with the first probe result.
6. Add tests with `StubServer`:
   - A publish response without `keepalive_interval_secs` gives no keepalive
     request.
   - A publish response with an interval of 1 second gives a keepalive
     request, with the bearer token and an empty body.
   - `Producer(Missing)` stops the keepalive requests.
   - With a stream delay longer than the keepalive interval, keepalive
     requests continue until the dead block is published, and stop after it.
   - If the producer returns before the delayed dead block is released, the
     keepalive does not stop.
   - A keepalive `409` gives a new publish of the last payload with the same
     `blockGuid`.
   - A keepalive `403` sets the fatal state, so `check_health` fails.
   - A keepalive `503` retries.
7. Change the relay row in `docs/architecture/broadcast-chain-boundaries.md`
   to name the keepalive route and relay ADR 0002.

## Acceptance Criteria

- Each test in step 6 exists and passes.
- No test calls a public relay.
- A test asserts that the keepalive request has no body.
- `rg -n "token" src/relay.rs` shows no new log or error text that holds the
  token value.

## Test Commands

```bash
cargo fmt --all -- --check
cargo build --workspace
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
```

Optional, with a local relay built from `~/build/musicindex-live-relay` at
`127.0.0.1:8018` and `LEASE_SECS=10`: run the publisher with a test target and
confirm that the relay logs a keepalive about each 3 seconds.

## Escalation Triggers

Stop and report if one of these occurs:

- Relay ADR 0002 is not Accepted, or the relay route differs from this packet.
- A test needs wall-clock sleeps longer than 3 seconds.
- The worker needs a second thread for each target.
- A keepalive `409` arrives when the worker has no accepted payload.

## Prompt for lower-context coding model

You are implementing one bounded task from a larger plan.

Implement only this task. Do not redesign the architecture.

Read:
- docs/adr/0005-producer-liveness-and-dead-block.md
- docs/tasks/relay-lease-task-004-keepalive.md
- ../musicindex-live-relay/docs/adr/0002-live-lease.md
- src/relay.rs
- src/main.rs
- src/liveness.rs
- tests/relay.rs

Goal:
- While the producer runs, each relay worker sends `POST /v1/liveitems/{event_id}/keepalive` at the interval the relay gives. It stops when the producer goes missing. On a 409 it publishes the last accepted payload again.

Constraints:
- Bearer token, no body. Build the URL with `build_url`.
- Read an optional `keepalive_interval_secs` from the publish and keepalive responses. No interval means no keepalive.
- Keepalive only while the producer state is Running.
- Status mapping: 200 renewed; 409 republish the last accepted payload with the same blockGuid; 401/403/404 fatal; 429, 5xx and network errors retry with the present backoff; any other status fatal.
- A new payload goes before a keepalive. A publish resets the keepalive time.
- Replace the payload channel with an enum: Publish(LiveValuePayload) and Producer(ProducerState). Add RelayPublisher::set_producer.
- Send Producer(Missing) only after PublishSchedule releases the dead block for that transition, and only if the producer is still missing. Send Producer(Running) at once.
- No token in a log line, an error or a Debug output.

Do not touch:
- mixxx-now-playing/**
- src/watcher.rs, src/livevalue.rs, src/config.rs, src/schedule.rs
- The payload shape
- ../musicindex-live-relay/**

Acceptance criteria:
- StubServer tests: no interval gives no keepalive; an interval of 1 second gives a keepalive with the bearer token and an empty body; Producer(Missing) stops keepalives; with a stream delay the keepalive continues until the dead block is published; a producer that returns before the dead block is released keeps the keepalive; a 409 republishes the last payload with the same blockGuid; a 403 makes check_health fail; a 503 retries.
- No test calls a public relay.
- The relay row in docs/architecture/broadcast-chain-boundaries.md names the keepalive route and relay ADR 0002.

Test commands:
- cargo fmt --all -- --check
- cargo build --workspace
- cargo test --workspace
- cargo clippy --workspace --all-targets -- -D warnings

At the end, report:
1. files changed
2. tests run
3. behavior changed
4. deviations from task
5. unresolved concerns
