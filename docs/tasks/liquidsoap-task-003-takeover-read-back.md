# Liquidsoap Task 003: The Publisher Restores Its Block Over A Foreign Dead Block

Status: Ready - 2026-11-21.

Every criterion is mechanical.

## Goal

At each keepalive time, the publisher reads the relay snapshot. It sends its last block again when the snapshot is a
dead block of a different sender (ADR 0023 §The Publisher Restores Its Block).

## Base components

| Component | Status | Location | Reason |
| --- | --- | --- | --- |
| Relay client | extend | src/relay.rs | Add one read of `GET /v1/liveitems/{event_id}/metadata` to `RelayClient`. Use `build_url` and the same client. |
| Keepalive cycle | extend | src/relay.rs | Add the read-back to `run_keepalive`. Use the path of `republish_after_lease_expiry` for the publish again. Do not copy it. |
| Dead block | extend | src/livevalue.rs | Add a test function for a dead block, adjacent to `dead_payload` and `DEAD_BLOCK_ADDRESS`. |
| Relay stub | extend | tests/relay.rs | Add a response for the metadata route to `StubServer`. |

## Files To Inspect

- `docs/adr/0023-dj-takeover.md`
- `docs/archive/adr/0005-producer-liveness-and-dead-block.md` §Publisher Behavior
- `src/relay.rs`: `RelayClient::keepalive`, `run_keepalive`, `republish_after_lease_expiry`, `publish_worker`
- `src/livevalue.rs`: `dead_payload`, `LiveValuePayload`
- `tests/relay.rs`
- `~/build/musicindex-live-relay/README.md`, the section of `GET /v1/liveitems/{event_id}/metadata`. The response is
  `{"event_id", "seq", "updated_at", "renewed_at", "lease_expires_at", "metadata"}`. `metadata` is the published
  payload. The route needs no token.

## Files Likely To Change

- `src/relay.rs`
- `src/livevalue.rs`
- `tests/relay.rs`

## Do Not Touch

- `mixxx-now-playing/**`, `now-playing-drop/**`, `Cargo.toml`, `Cargo.lock`. A different task changes them now.
- `systemd/**`, `packaging/**`, `scripts/**`, `docs/**`.

## Constraints

- A **dead block** is a payload whose `value.destinations` list is not empty and has the dead block address in each
  `address`. Each other payload is payable.
- Do the read only when the producer state is `Running` and the last accepted payload is payable.
- The read sends no `Authorization` header. It uses the timeout of the client.
- Read outcomes:
  - `200` with a dead block, and its `blockGuid` is not the `blockGuid` of the last accepted payload: publish the last
    accepted payload again, with the same `blockGuid`. Then send the display `Resend`, as after a lease expiry. Do not
    send a keepalive in that cycle.
  - `200` with a payable block of a different `blockGuid`: write one `tracing::warn!` that names a second sender on the
    event, with the target name and the event ID. Then send the keepalive.
  - `200` with the same `blockGuid`, `404`, a different status, a network error or a body that does not parse: send the
    keepalive. Write a `debug` or `warn` line for an error. A failed read never stops the worker.
- Never publish a dead block again because of this rule.
- Do not put a token in a log line or in an error.
- The existing keepalive tests now see a metadata request before each keepalive. Add the metadata response to their
  stub sequences. Do not remove an existing assertion. Change only the request index or the response list.
- Write comments in Simplified Technical English (`/home/citizen/.agents/skills/asd-ste100/SKILL.md`). Follow the
  `rust-dev` skill.
- Do not commit. Leave the change in the working tree. The main session reads the diff and commits.

## Acceptance Criteria

All are mechanical. Each case is a test in `tests/relay.rs` or in `src/relay.rs`:

1. A foreign dead block on the relay: the next request after the read is a publish of the last payload, with the same
   `blockGuid`.
2. The own block on the relay: the next request is a keepalive.
3. A foreign payable block: the next request is a keepalive, and no publish follows.
4. A `404` and a failed read: the next request is a keepalive.
5. A missing producer: no metadata request and no keepalive.
6. A last accepted dead block: no metadata request before the keepalive.
7. A unit test in `src/livevalue.rs`: `dead_payload` is a dead block, and a payload with a real address is not.
8. `cargo test --workspace`, `cargo clippy --workspace --all-targets -- -D warnings` and
   `cargo fmt --all -- --check` pass.
