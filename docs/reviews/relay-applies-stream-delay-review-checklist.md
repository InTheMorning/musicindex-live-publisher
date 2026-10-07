# The Relay Applies The Stream Delay: Review Checklist

Status: open - 2026-10-06. Each item passes except the two visual items. ADR 0011 becomes
`Implemented` only when each item passes.

## Invariants Of ADR 0011

- [x] The publisher never holds a block or a display state for the stream
  delay.
- [x] Each live value publish carries `Listener-Delay-Secs` from the
  configuration of its target. A keepalive, a display request and an artwork
  upload carry none.
- [x] The keepalive stops only after the dead block publish succeeds.

## Code

- [x] `PublishSchedule` and its tests are deleted. No unused copy remains.
- [x] No comment describes the old hold.
- [x] The tests use no wall-clock sleep.
- [x] `src/config.rs` and the setting name did not change.

## Documents

- [x] `README.md`, the configuration runbook and the example configuration
  give the new function of `stream_delay_secs`.
- [x] ADR 0005 and ADR 0008 have the dated sentences of ADR 0011 §What This
  Replaces.
- [x] `AGENTS.md` §Current State gives the new timing.
- [x] `docs/architecture/broadcast-chain-boundaries.md` moves ADR 0011 out of
  §Proposed Changes.

## Cross-Repository

- [x] `musicindex-live-relay` ADR 0004 is implemented and deployed. Done
  2026-10-06. With `stream_delay_secs = 15`, `GET /remoteValue` changed
  15.5 s to 15.6 s after `GET /metadata`.
- [x] Task 001 and task 002 ship in one release. Done 2026-10-06: package
  `r83`.
- [x] `v4vmm` shows `stream_delay_secs` as the delay that podcast apps get.
  Done 2026-10-06. No `v4vmm` screen shows the value. The doc comment of
  `PublisherTarget::stream_delay_secs` gives its meaning.

## Visual

A person must complete these items. Report each one as open until then.

- [ ] A podcast app on Socket.IO changes the block at the same time as before.
- [ ] The private app on ICY shows the artwork in time for a listener with a
  short buffer.

## Review Result

Reviewed 2026-10-06 at commit `e3e4df0`.

- `grep -rn PublishSchedule src tests` finds nothing. `src/schedule.rs` and
  `tests/schedule.rs` are deleted.
- `src/config.rs` did not change between commit `3c72b2a` and commit
  `f8b78a7`.
- Seven tests in `tests/relay.rs` cover the header. A block publish carries
  the rounded delay, and a keepalive, a display request and an artwork upload
  carry no header.
- Three tests in `src/main.rs` show that a drop file change, a display change
  and a missing producer each give their items at once.
- The tests of ADR 0011 wait for no stream delay. The sleeps in
  `tests/relay.rs` and `tests/display.rs` wait for worker threads only.

The review added one test. No test showed the keepalive rule when the dead
block publish fails. `relay_dead_block_is_retried_to_success_before_the_keepalive_stops`
gives the dead block `503` one time, and sets the producer to missing at
once. The publisher sends the dead block again until the relay accepts it,
and then sends nothing. The test fails when a retry stops at a producer
change.

The review applied ADR 0011 §What This Replaces:

- ADR 0005 has a dated sentence, and its §Invariants give the present rules.
- ADR 0008 has a dated sentence. Its §The Publisher and its verification
  list give the present rules, and the replaced invariant is removed.
- `docs/architecture/broadcast-chain-boundaries.md` describes ADR 0010,
  ADR 0011 and ADR 0012 as present behavior. Only ADR 0009 stays in §Changes
  Not Yet Implemented.

On 2026-10-06, with relay ADR 0004 on `api.musicindex.org` and
`stream_delay_secs = 15`, `GET /remoteValue` changed 15.5 s to 15.6 s after
`GET /metadata`.
