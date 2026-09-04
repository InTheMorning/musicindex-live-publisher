# Audit Fix 006 — Compensate for Stream Latency Before Publishing

Remediates finding 6 of the
[audit review](../reviews/nowplaying-publisher-audit-review.md).

## Goal

Stop publishing a track's value block seconds before listeners can hear that
track. The publisher must be able to hold each payload for a configured stream
delay so the block a listener boosts is the block they are hearing.

The delay belongs to the broadcast path, not to the track source, so it is
configured per publish target and applied by the publisher.

## Files To Inspect

- `src/main.rs:17` — `HEALTH_CHECK_INTERVAL`, and
  `:241-280` — `run_watch_loop`, whose `recv_timeout` is the only existing timer
  in the process and therefore the seam this fix hangs deadlines on.
- `src/main.rs:325-338` — `emit_payloads`, the single
  point every payload passes through on its way to the relay or to `--dry-run`
  stdout.
- `src/main.rs:60-67` — startup: `initial_payloads`
  is emitted through the same function before the loop begins.
- `src/watcher.rs:117-146` — `process_event`, which
  returns payloads for both `Upsert` and `Remove`. Note that a `Remove` returns
  the fallback payload: it is a publish, not an absence, and the delay must
  cover it.
- `src/watcher.rs:161-183` — the block-GUID rule. A
  route upgrade rewrites the same track under the *same* `blockGuid`; a new
  track mints a fresh one. That distinction is what tells a replaceable pending
  payload from one that must fire on its own schedule.
- `src/relay.rs:262-292` — `RelayPublisher::publish`
  routes by `payload.event_guid` against a `senders` map keyed by `event_id`.
  The same key identifies a target's delay.
- `src/config.rs:86-100` — `RawConfig` and
  `RawTarget`, and `:165-205` — `resolve_target`.
- `mixxx-now-playing/src/main.rs:210-217` — producer expiry. Read it to confirm
  why the delay must not live on the producer side.

## Files Likely To Change

- `src/schedule.rs` (new)
- `src/lib.rs`
- `src/config.rs`
- `src/main.rs`
- `tests/schedule.rs` (new)
- `tests/config.rs`
- `README.md`
- `docs/runbooks/musicindex-live-publisher-configuration.md`
- `packaging/arch/musicindex-live-publisher.example.toml`

## Do-Not-Touch List

- `~/build/v4vmm/` and `~/build/splitkit/` — read only, never edit. The relay
  must not learn about broadcast delay; it is a pass-through fan-out and staying
  that way is what keeps it reusable.
- `mixxx-now-playing/` — the producer keeps emitting at track-change time. Do
  not add a delay there, do not change `expiry_slack`, and do not make the
  producer aware of any stream. A producer-side delay would break `--once`,
  would delay the icecast title as well, and would leave the drop file present
  while the publisher believes the track has ended.
- `LiveValuePayload` and the payload shape. `startTime` stays `0`. This fix
  changes *when* a payload is sent, never what is in it.
- The drop-file contract (ADR 0002). No new timestamp field. Producers stay
  ignorant of broadcast timing.
- The retry and backoff logic in `publish_worker`. Delay is scheduling; backoff
  is failure recovery. Keeping them separate is why the delay goes in front of
  `RelayPublisher`, not inside it.
- The debounce window. 75 ms of filesystem-event coalescing is unrelated.

## Constraints

- The delay is per target, configured as `stream_delay_secs` on `[[target]]`,
  defaulting to `0`. Zero must reproduce today's behavior exactly.
- Validate the value: it must be finite and not negative, and must be rejected
  above a stated ceiling. A typo that parks every payload for an hour must fail
  at startup, not on air. Choose the ceiling and say why.
- **Upserts and removals are delayed identically.** Delaying the track payload
  but not the fallback opens a window where the block reads fallback while
  listeners are mid-track; the reverse overlaps two tracks. This is the single
  most important rule in the packet.
- **Pending payloads are not collapsed across blocks.** A fast track change
  queues two payloads for the same target, and both are real blocks that
  listeners will hear in sequence. Each fires at its own deadline.
- A payload whose `blockGuid` matches a still-pending payload for the same
  target replaces it **in place, keeping the original deadline**. That is the
  MusicIndex route-upgrade rewrite, which must not become a second block and
  must not push the block later.
- Ordering within a target is preserved. Ordering across targets is not, and
  must not be: a target with no delay must not be held behind a target with a
  long one.
- Startup emits immediately, regardless of the configured delay. `initial_payloads`
  recovers existing state rather than reporting a change, and the alternative
  leaves the relay serving nothing for the length of the delay. State the choice
  in the runbook so an operator is not surprised by it.
- Scheduling must not degrade the relay health check. The loop still calls
  `check_health` at least once a second.
- Scheduling accuracy must not depend on `HEALTH_CHECK_INTERVAL`. Wake on the
  earlier of the next deadline and the health interval.
- `--dry-run` honours the delay, so an operator can time the output against the
  stream without publishing.
- No wall-clock time. Use `Instant`, which is monotonic; a clock step during a
  show must not fire the queue early or park it.
- No new dependency and no async runtime. The plan's reasoning still holds: one
  event per track change, not a request stream.

## Implementation Steps

1. Add `src/schedule.rs` with a `PublishSchedule` type built from a map of
   `event_guid` to delay. Give it three operations: schedule a payload against a
   `now`, take the payloads that are due at a `now`, and report the next
   deadline. Keep `Instant` a parameter rather than reading the clock inside, so
   the whole thing is testable without sleeping.
2. Implement same-block replacement inside `schedule`: scan pending entries for
   a matching `(event_guid, blockGuid)` pair and overwrite the payload while
   leaving `due_at` alone.
3. Implement `take_due` so it extracts every due entry in insertion order and
   retains the rest, rather than popping only from the front. Head-of-line
   blocking across targets is the bug this avoids.
4. Export the type from `src/lib.rs` alongside the existing re-exports.
5. Add `stream_delay_secs: Option<f64>` to `RawTarget` and a resolved
   `stream_delay: Duration` on `PublisherTarget`. Validate in `resolve_target`
   next to `validate_event_id`.
6. In `main`, build the schedule from the resolved targets. Emit
   `initial_payloads` directly as today, then route every payload from
   `process_event` through the schedule instead of straight to `emit_payloads`.
7. In `run_watch_loop`, compute the `recv_timeout` as the earlier of the next
   deadline and `HEALTH_CHECK_INTERVAL`, and drain due payloads on every
   iteration — after an event and after a timeout alike.
8. Log at debug when a payload is queued, with the target and the delay, and at
   info when a delayed payload is released. An operator diagnosing alignment
   needs to see both ends of the hold.
9. Log the configured delay per target in the existing startup `loaded publisher
   config` line, or immediately after it. A misconfigured delay must be visible
   in the journal without reading the config file.
10. Tests, all synthetic-clock, none sleeping: zero delay is pass-through; a
    delayed payload is withheld before its deadline and released at it; a
    fallback from a `Remove` is delayed by the same amount as the track; two
    different blocks both fire, in order; a same-block rewrite replaces without
    extending the deadline; two targets with different delays do not block each
    other; the next-deadline calculation is correct with an empty and a
    populated queue.
11. Config tests: default is zero; a valid value resolves; negative, non-finite,
    and over-ceiling values are rejected with the target named in the message.
12. Update `README.md`, the configuration runbook, and the example TOML.
    Document how to measure the delay — play a marked track and compare the
    local clock against a real client — and state plainly that butt's own song
    delay is a different term and does not substitute for this one.

## Acceptance Criteria

- `stream_delay_secs` defaults to `0`, and a config without it produces
  byte-identical publishing behavior to today.
- A track payload and the fallback that follows its removal are held for the
  same duration.
- Two tracks changing inside the delay window both publish, in order, each at
  its own deadline.
- A route-upgrade rewrite under the same `blockGuid` publishes once, at the
  original deadline.
- A zero-delay target publishes immediately while a long-delay target has work
  pending.
- Negative, non-finite, and over-ceiling delays fail at startup with the target
  name in the error.
- The relay health check still runs at least once a second with payloads pending.
- Startup payloads are not delayed, and the runbook says so.
- No test sleeps for the length of a delay.
- `cargo clippy --all-targets` clean.

## Test Commands

```bash
cd musicindex-live-publisher
cargo clippy --all-targets --offline -- -D warnings
cargo test --offline
```

```bash
systemd-analyze verify systemd/musicindex-live-publisher.service
```

## Expected Final Report Format

- The chosen ceiling for `stream_delay_secs`, with one sentence on why.
- The same-block replacement rule, in two lines: what is compared, what is kept.
- Confirmation, from a run rather than from reading, that a zero-delay config
  behaves as before — name the test.
- The startup-emits-immediately decision, restated, so it is on the record.
- Full `test result:` lines and the `systemd-analyze verify` output.

## Escalation Triggers

- If holding payloads in the main loop turns out to interact badly with the
  fatal-exit path from audit fix 005 — a fatal target while payloads are queued,
  and the process exiting with a fallback still pending — stop and report. The
  interaction between "exit loudly" and "hold before publishing" is a design
  decision, not an implementation detail.
- If a shutdown path is needed to flush pending payloads, stop and report rather
  than adding signal handling in this packet. The publisher has no graceful
  shutdown today; a delay queue makes that gap wider, but closing it is its own
  change with its own risk.
- If the correct delay turns out to vary within a show — a listener joining
  mid-stream sees a different buffer than one who has been connected for an hour
  — stop. A single configured constant is the deliberate scope of this packet,
  and per-listener alignment is a relay and player problem, not a publisher one.
