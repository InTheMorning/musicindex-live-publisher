# The Relay Applies The Stream Delay: Review Checklist

Status: open. Use this checklist after relay delay task 002. ADR 0011 becomes
`Implemented` only when each item passes.

## Invariants Of ADR 0011

- [ ] The publisher never holds a block or a display state for the stream
  delay.
- [ ] Each live value publish carries `Listener-Delay-Secs` from the
  configuration of its target. A keepalive, a display request and an artwork
  upload carry none.
- [ ] The keepalive stops only after the dead block publish succeeds.

## Code

- [ ] `PublishSchedule` and its tests are deleted. No unused copy remains.
- [ ] No comment describes the old hold.
- [ ] The tests use no wall-clock sleep.
- [ ] `src/config.rs` and the setting name did not change.

## Documents

- [ ] `README.md`, the configuration runbook and the example configuration
  give the new function of `stream_delay_secs`.
- [ ] ADR 0005 and ADR 0008 have the dated sentences of ADR 0011 §What This
  Replaces.
- [ ] `AGENTS.md` §Current State gives the new timing.
- [ ] `docs/architecture/broadcast-chain-boundaries.md` moves ADR 0011 out of
  §Proposed Changes.

## Cross-Repository

- [x] `musicindex-live-relay` ADR 0004 is implemented and deployed. Done
  2026-10-06. With `stream_delay_secs = 15`, `GET /remoteValue` changed
  15.5 s to 15.6 s after `GET /metadata`.
- [x] Task 001 and task 002 ship in one release. Done 2026-10-06: package
  `r83`.
- [ ] `v4vmm` shows `stream_delay_secs` as the delay that podcast apps get.

## Visual

A person must complete these items. Report each one as open until then.

- [ ] A podcast app on Socket.IO changes the block at the same time as before.
- [ ] The private app on ICY shows the artwork in time for a listener with a
  short buffer.
