# Relay Delay Task 001: The Delay Header

Status: Implemented - 2026-10-06. Ship it in one release with task 002,
after the relay of ADR 0004 is deployed.

Every criterion is mechanical.

## Goal

Each publish of the live value carries the stream delay of its target in the
header `Listener-Delay-Secs`. A keepalive and a display request carry no
header. No timing changes in this task.

## Files To Inspect

- `docs/adr/0011-relay-applies-stream-delay.md` (§Send At Once)
- `musicindex-live-relay`: `docs/adr/0004-listener-timeline-delay.md` (§The
  Delay)
- `src/relay.rs`: `RelayTarget`, `RelayTarget::from_config`,
  `RelayClient::publish_value`, `RelayClient::keepalive`,
  `RelayClient::publish_display`
- `src/config.rs`: `PublisherTarget`, `stream_delay`
- `tests/relay.rs` (the stub relay that records each request)

## Files Likely To Change

- `src/relay.rs`
- `tests/relay.rs`

## Do Not Touch

- `src/schedule.rs`, `src/main.rs`. Task 002 changes them.
- `src/config.rs`. The setting does not change.
- `docs/adr/**`

## Constraints

- Add `listener_delay_secs: u64` to `RelayTarget`. `from_config` sets it from
  `target.stream_delay`, rounded to the nearest whole second. A half second
  rounds up.
- `publish_value` adds the header `Listener-Delay-Secs` with that value to
  each request. This also covers the publish again after a lease expiry,
  because it uses `publish_value`.
- `keepalive`, `publish_display` and `upload_artwork` add no header.
- The `Debug` form of `RelayTarget` shows the new field. It still redacts the
  token.

## Implementation Steps

1. Add the field and set it in `from_config`.
2. Add the header in `publish_value`.
3. Add the tests.

## Acceptance Criteria

Each item is a test in `tests/relay.rs` with the stub relay:

- A target with `stream_delay_secs = 12.0` sends `Listener-Delay-Secs: 12`.
- `12.4` sends `12`, and `12.5` sends `13`.
- A target with no `stream_delay_secs` sends `0`.
- A keepalive request has no `Listener-Delay-Secs` header.
- A display request and an artwork upload have no `Listener-Delay-Secs`
  header.
- The existing tests pass with no change.

Also: the full gate passes.

## Test Commands

```bash
cargo fmt --all -- --check
cargo build --workspace
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
```

## Escalation Triggers

- `RelayTarget` is built in a place other than `from_config`, and that place
  has no delay.
- The stub relay cannot record request headers.

## Prompt for lower-context coding model

You are implementing one bounded task from a larger plan.

Implement only this task. Do not redesign the architecture.

Read:
- docs/adr/0011-relay-applies-stream-delay.md
- docs/tasks/relay-delay-task-001-delay-header.md
- src/relay.rs
- src/config.rs
- tests/relay.rs

Goal:
- Each live value publish carries Listener-Delay-Secs, the rounded stream delay of its target. A keepalive, a display request and an artwork upload carry no header. Change no timing.

Constraints:
- Follow §Constraints of the packet exactly.

Do not touch:
- src/schedule.rs, src/main.rs, src/config.rs, docs/adr/**

Acceptance criteria:
- Each item in §Acceptance Criteria of the packet is a passing test.

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

## Review Result

Reviewed 2026-10-06. The full gate passes: `cargo fmt`, `cargo build`,
`cargo test` and `cargo clippy` for the workspace. One test stays ignored,
because it needs a live relay, as before this task.

The review accepts these items:

- `round_listener_delay_secs` uses whole milliseconds, and a half second
  rounds up. The tests cover 12.4 seconds, 12.5 seconds and 0 seconds.
- Only `publish_value` sends the header. The republish after a lease expiry
  uses the same method, so it also sends the header.
- `tests/display.rs` has one new field in its `RelayTarget` literal. The
  build needs it. No assertion changed.
- The stub relay of `tests/relay.rs` records a body that is not JSON as
  `null`. The artwork upload test needs it. No earlier assertion changed.

No timing changed. `PublishSchedule` still holds each payload.
