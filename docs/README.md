# Documentation

## Architecture

- [Broadcast chain boundaries](architecture/broadcast-chain-boundaries.md) —
  what this repository consumes and produces, which neighbor owns each
  contract, and the known limits of the chain

## ADRs

- [ADR 0001: Rust now-playing lifecycle](adr/0001-rust-now-playing-lifecycle.md)
- [ADR 0002: Now-playing drop-file contract](adr/0002-nowplaying-drop-file-contract.md)

## Plans

- [Rust now-playing utility plan](plans/rust-now-playing-utility-plan.md) — replace
  `mixxx-now-playing.py` with a Rust binary that emits V4V track metadata only
  while a V4V track is playing
- [MusicIndex live publisher plan](plans/musicindex-live-publisher-plan.md) —
  standalone systemd service that watches a drop directory for now-playing
  metadata and publishes live value splits to a MusicIndex relay

## Tasks

Implementation packets for the now-playing plan. Strictly sequential — each
builds on the previous.

Phase 1 — parity and lifecycle:

- [001 — Crate scaffold and configuration resolution](tasks/rust-now-playing-task-001-scaffold-and-config.md)
- [002 — Mixxx history source and test harness](tasks/rust-now-playing-task-002-history-source.md)
- [003 — Tag reading and metadata rendering](tasks/rust-now-playing-task-003-tag-reading.md)
- [004 — Output sink and file lifecycle](tasks/rust-now-playing-task-004-sink-and-lifecycle.md)

Phase 2 — expiry:

- [005 — Expiry timer](tasks/rust-now-playing-task-005-expiry-timer.md)

Phase 3 — value routes:

- [006 — MusicIndex value-route resolution](tasks/rust-now-playing-task-006-musicindex-resolution.md)

Implementation packets for the live publisher plan. Strictly sequential.

Phase 1 — contract and transform:

- [001 — Crate scaffold, drop-file contract, and ADR](tasks/musicindex-live-publisher-task-001-scaffold-and-contract.md)
- [002 — Live value transform and payload assembly](tasks/musicindex-live-publisher-task-002-live-value-transform.md)

Phase 2 — watcher:

- [003 — Drop directory watcher and clear semantics](tasks/musicindex-live-publisher-task-003-watcher-and-clear.md)

Phase 3 — publish:

- [004 — Configuration, targets, and token loading](tasks/musicindex-live-publisher-task-004-config-and-targets.md)
- [005 — Relay client and publish loop](tasks/musicindex-live-publisher-task-005-relay-client.md)

Phase 4 — deploy:

- [006 — Systemd deployment and producer alignment](tasks/musicindex-live-publisher-task-006-deploy-and-alignment.md)

Remediation packets from the 2026-09-04 audit. Independent of each other —
each can be done alone, but 002, 004, and 005 gate any run against a live relay.

- [001 — Safe atomic write in the producer sink](tasks/audit-fix-task-001-sink-atomic-write.md)
- [002 — Fresh `blockGuid` per track](tasks/audit-fix-task-002-block-guid-per-track.md)
- [003 — History query must not skip tracks without locations](tasks/audit-fix-task-003-history-left-join.md)
- [004 — Drop directory trust checks](tasks/audit-fix-task-004-drop-dir-trust.md)
- [005 — A fatal publish failure must not fail silently](tasks/audit-fix-task-005-fatal-publish-exit.md)
- [006 — Compensate for stream latency before publishing](tasks/audit-fix-task-006-stream-delay-compensation.md)

## Reviews

- [Rust now-playing review checklist](reviews/rust-now-playing-review-checklist.md)
- [Now-playing / live publisher audit review](reviews/nowplaying-publisher-audit-review.md) — accuracy, stability, and security audit of both crates at `1dea27f`

## Runbooks

- [MusicIndex live publisher deployment](runbooks/musicindex-live-publisher-deploy.md)
- [MusicIndex live publisher Arch package](runbooks/musicindex-live-publisher-arch-package.md)
- [MusicIndex live publisher configuration](runbooks/musicindex-live-publisher-configuration.md)
