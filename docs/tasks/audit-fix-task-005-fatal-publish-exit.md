# Audit Fix 005 — A Fatal Publish Failure Must Not Fail Silently

Remediates finding 5 of the
[audit review](../reviews/nowplaying-publisher-audit-review.md).

## Goal

Stop a fatal relay rejection from putting the service into a permanently
disabled state that still reports `active`. When a target can no longer publish,
that must become visible — as a process exit that systemd restarts and reports,
not as an endless stream of warnings from a live-looking daemon.

## Files To Inspect

- `src/relay.rs:297-370` — `publish_worker`, the
  `disabled` flag at `:304`, the drain-and-warn branch at `:306-312`, and the
  fatal branch at `:336-345`.
- `src/relay.rs:150-175` — the status-code mapping.
  401, 403, and 404 are `Fatal`; 413 is `Dropped`; 429 and 5xx are `Retryable`.
- `src/relay.rs:262-292` — `RelayPublisher::publish`
  and the `senders` map. Note it already returns an error when a worker has
  stopped, which is the seam this fix uses.
- `src/main.rs:296-308` — `emit_payloads`, which
  propagates that error out of the watch loop.
- `systemd/musicindex-live-publisher.service:9-10` —
  `Restart=on-failure`, `RestartSec=5s`.
- `tests/relay.rs` — the local-stub harness; note
  `:390`, which refuses to run against `api.musicindex.org`.

## Files Likely To Change

- `src/relay.rs`
- `src/main.rs`
- `systemd/musicindex-live-publisher.service`
- `tests/relay.rs`
- `docs/runbooks/musicindex-live-publisher-deploy.md`

## Do-Not-Touch List

- `~/build/v4vmm/` and `~/build/splitkit/` — read only, never edit.
- `mixxx-now-playing/` — consumer-side only.
- The status-code classification. 401/403/404 stay fatal, 413 stays dropped, 429
  and 5xx stay retryable. This packet changes what *happens* on fatal, not what
  counts as fatal.
- The `{event_id, metadata}` pre-send rejection, and the `Err(_)` arm that drops
  a malformed payload without retrying. A payload we refuse to send is a bug in
  us, not a relay outage, and must not take the process down.
- Token redaction in the `Debug` impls. Never log a token, at any verbosity,
  including in the new exit path.

## Constraints

- On `Fatal`, the worker must terminate rather than set a flag and keep draining.
- Termination must reach the main thread. Dropping the receiver makes the next
  `RelayPublisher::publish` return `Err`, which `emit_payloads` already
  propagates out of `run_watch_loop` — prefer that existing seam over a panic or
  a `process::exit` from a worker thread.
- The process must exit non-zero so `Restart=on-failure` fires.
- Log the reason once, at error level, naming the target and the HTTP status,
  immediately before the worker stops.
- Restart must not become a hot loop. A 401 from a revoked token will recur
  every restart, so add `StartLimitIntervalSec`/`StartLimitBurst` to the unit so
  systemd gives up and marks the unit failed instead of restarting forever.
  Choose values and state them.
- A fatal failure on one target must not silently take down publishing for other
  targets without saying so. With a shared process, exiting affects all targets —
  that is acceptable and arguably correct, but the log line must make clear which
  target caused it.
- No test may contact `api.musicindex.org`. Use the local stub harness already
  in `tests/relay.rs`.

## Implementation Steps

1. Add a failing test first: drive a worker against a stub that returns 401, then
   assert the publisher reports the target as unavailable on the next publish
   rather than accepting the payload silently.
2. Remove the `disabled` flag. In the `Fatal` arm, log at error level and
   `return` from `publish_worker`.
3. Confirm the resulting `SendError` path: `RelayPublisher::publish` already maps
   a closed channel to `"relay publish worker for event_id {…} stopped"`. Verify
   that message is what surfaces, and that it names the target usefully.
4. In `main`, make sure that error reaches `main`'s return rather than being
   swallowed. `anyhow::Result` from `main` already exits non-zero — confirm it,
   do not assume.
5. Add `StartLimitIntervalSec=` and `StartLimitBurst=` to the `[Unit]` section of
   the service file.
6. Add a test that a `Dropped` outcome (413) does **not** stop the worker, and
   one that a `Retryable` outcome still retries — these are the regressions this
   change could plausibly cause.
7. Update the runbook: the 401/403/404 failure modes should now say the service
   exits and how to see it (`systemctl status`, `journalctl -u`), plus what to do
   when the unit has hit the start limit.

## Acceptance Criteria

- A 401, 403, or 404 stops the worker and causes the process to exit non-zero.
- The exit is preceded by exactly one error log naming the target and status.
- No token appears in any log line on this path.
- 413 still drops the single payload and keeps the worker alive.
- 429 and 5xx still retry with backoff, and a newer payload still supersedes a
  pending retry.
- The unit has a start limit, and the runbook explains it.
- All existing tests in `tests/relay.rs` pass; the ignored local-relay test stays
  ignored.
- `cargo clippy --all-targets` clean.

## Test Commands

```bash
cd musicindex-live-publisher
cargo clippy --all-targets --offline
cargo test --offline
```

```bash
systemd-analyze verify systemd/musicindex-live-publisher.service
```

## Expected Final Report Format

- The new fatal path, described in three lines: what logs, what stops, what the
  exit code is.
- The `StartLimitIntervalSec` and `StartLimitBurst` values chosen, with a
  sentence on why.
- Confirmation, from an actual run rather than from reading, that `main` exits
  non-zero on this path — paste the `echo $?`.
- Confirmation that no token is logged, and how you checked.
- Full `test result:` lines, and the `systemd-analyze verify` output.

## Escalation Triggers

- If exiting the process turns out to be the wrong call for a multi-target
  deployment — one dead target killing three healthy ones — stop and report
  before implementing a per-target retry-with-long-backoff instead. That is a
  design decision, not an implementation detail, and it interacts with the
  fallback-on-clear guarantee.
- If `main` does not actually exit non-zero on the propagated error, stop: that
  is a separate bug and it invalidates the whole approach of this packet.
- If removing the `disabled` flag causes a test to hang rather than fail, stop —
  it means a worker's channel is being kept alive somewhere unexpected.
