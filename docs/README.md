# Documentation

## Order Of Work

The packets here belong to a system that spans three repositories. The
cross-repository order lives in `v4vmm`:
`docs/plans/broadcast-chain-delivery-order.md`. Read it before you start.

Control-surface tasks 001 and 002 are complete. The v4vmm attach-event packet is
also complete. Follow the current cross-repository order for new work.

Show-log tasks 001 and 002 remain unstarted. Writer 001 first needs a decision
about the producer timestamp source. A scheduled real show makes that writer
the next priority. Confirm that logging works before the show.

## Architecture

- [Broadcast chain boundaries](architecture/broadcast-chain-boundaries.md) —
  what this repository consumes and produces, which neighbor owns each
  contract, and the known limits of the chain
- [Mixxx interfaces](architecture/mixxx-interfaces.md) — advisory. What Mixxx
  2.5.6 gives to an external program: controllers, deck controls, the history
  database and the track duration, with the evidence for each fact

## ADRs

- [ADR 0001: Rust now-playing lifecycle](adr/0001-rust-now-playing-lifecycle.md)
  — Implemented
- [ADR 0002: Now-playing drop-file contract](adr/0002-nowplaying-drop-file-contract.md)
  — Implemented
- [ADR 0003: Show log contract](adr/0003-show-log-contract.md) — the
  accepted append-only log contract for future episode generation. Implementation
  and the producer timestamp source remain open
- [ADR 0004: Publisher control CLI](adr/0004-publisher-control-cli.md) —
  Implemented. The target and JSON commands that let `v4vmm` control this
  repository without writing its configuration file
- [ADR 0005: Producer liveness, the dead block and the relay lease](adr/0005-producer-liveness-and-dead-block.md)
  — Implemented. The producer lock, the dead block that replaces the configured
  fallback, the relay keepalive, and a limit on the expiry timer without the
  MIDI connector
- [ADR 0006: Mixxx MIDI connector](adr/0006-mixxx-midi-connector.md) —
  Accepted. A kernel virtual MIDI port gives the producer the deck play state,
  so a stop or a pause ends the payment at once
- [ADR 0007: Commands to Mixxx through the connector](adr/0007-mixxx-connector-commands.md)
  — Implemented. Protocol version 3 adds an AutoDJ fade-now command with an
  answer, and a `mixxx-now-playing command` subcommand
- [ADR 0008: An optional display path for artwork](adr/0008-display-path.md)
  — Accepted. Artwork and track text for every track, through the stream delay,
  to the display routes of the relay. A paused stream clears the `butt` title
- [ADR 0009: Track metadata in the HLS stream](adr/0009-hls-track-metadata.md)
  — Proposed. The song line, and a MusicIndex ID3 frame that a tagger on the
  VPS releases at the ICY title. The Socket.IO live value and its `image` stay
  as the compatibility path. The tagger reads the instant relay routes, and a
  new repository holds it. Two device checks and one repository remain
- [ADR 0010: The live value payload follows the model server](adr/0010-live-value-payload-reference-shape.md)
  — Accepted. Adds `line`, `author` and `podcastName` to the payload, as
  CurioHoster sends them, and a drop file version 2 with the album and
  `play_id`. No `link` until a source exists. The app check is open
- [ADR 0011: The relay applies the stream delay](adr/0011-relay-applies-stream-delay.md)
  — Accepted. The publisher sends each block and display state at once, with
  the delay in a header. Relay ADR 0004 delays Socket.IO only. Replaces the
  schedule rules of ADR 0005 and ADR 0008 at its implementation review
- [ADR 0012: Pair the display state with its value block](adr/0012-pair-display-state-with-value-block.md)
  — Accepted. `display.json` version 2 with the exact song line and
  `play_id`. The publisher adds `songLine` and the block identity to the
  display state (relay ADR 0005)

## Plans

- [Rust now-playing utility plan](plans/rust-now-playing-utility-plan.md) — replace
  `mixxx-now-playing.py` with a Rust binary that emits V4V track metadata only
  while a V4V track is playing
- [MusicIndex live publisher plan](plans/musicindex-live-publisher-plan.md) —
  standalone systemd service that watches a drop directory for now-playing
  metadata and publishes live value splits to a MusicIndex relay
- [Publisher control CLI phase plan](plans/publisher-control-cli-phase-plan.md)
  — command-line target management and machine-readable state for the control
  surface
- [Relay lease keepalive phase plan](plans/relay-lease-keepalive.md) — the
  producer lock, the dead block and the keepalive for ADR 0005, and the order
  of work with `musicindex-live-relay` ADR 0002
- [Mixxx MIDI connector phase plan](plans/mixxx-midi-connector.md) — the
  mapping, the connector in the producer and the package setup for ADR 0006
- [Display path phase plan](plans/display-path.md) — the song file, the
  display link, the producer display output and the publisher display path for
  ADR 0008
- [Packaging pass plan](plans/packaging-pass.md) — deferred. The intended
  package behavior, the open packaging gates and the open decisions
- [Reserved event safety plan](plans/reserved-event-safety.md) — the four
  packets that keep a reserved event and its token safe, and the interim rules
  for the operator
- [HLS track metadata plan](plans/hls-track-metadata.md) — Proposed. The five
  gates of ADR 0009 and the packets that follow them. No packet exists yet
- [Live value reference shape plan](plans/live-value-reference-shape.md) —
  Ready. Two packets for ADR 0010
- [Display pairing plan](plans/display-pairing.md) — Ready. Two packets
  for ADR 0012, in one release, after relay ADR 0005
- [Relay applies the stream delay plan](plans/relay-applies-stream-delay.md)
  — Ready. Two packets for ADR 0011, in one release, after relay ADR 0004

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

Show log packets. These give `v4vmm` the timeline it needs to build an episode.

- [001 — Show log writer](tasks/show-log-task-001-log-writer.md)
- [002 — Read contract and documentation](tasks/show-log-task-002-read-contract-and-docs.md)

Control surface packets. ADR 0004 governs them. They support the `v4vmm`
broadcast control surface (`v4vmm` ADR 0059). Task 001 gates `v4vmm` task 014.

- [001 — Target management commands](tasks/control-surface-task-001-target-management.md)
- [002 — Machine-readable CLI surface](tasks/control-surface-task-002-machine-readable-cli.md)

Relay lease packets. ADR 0005 governs them. Tasks 001 and 002 can go in either
order. Task 003 needs both. Task 004 needs task 003 and `musicindex-live-relay`
tasks 001 and 002.

- [001 — Producer lock and expiry maximum](tasks/relay-lease-task-001-producer-lock-and-expiry-max.md)
- [002 — The dead block](tasks/relay-lease-task-002-dead-block.md)
- [003 — Producer liveness in the publisher](tasks/relay-lease-task-003-producer-liveness.md)
- [004 — The keepalive](tasks/relay-lease-task-004-keepalive.md)

Mixxx connector packets. ADR 0006 governs them. Tasks 001 and 002 can go in
either order. Task 003 needs task 002. Task 004 needs tasks 001 and 003.
Task 005 needs tasks 001 to 003. Task 006 needs task 005. ADR 0007 governs
task 006.

- [001 — The mapping](tasks/mixxx-connector-task-001-mapping.md)
- [002 — The connector core](tasks/mixxx-connector-task-002-connector-core.md)
- [003 — The link rules and the poll loop](tasks/mixxx-connector-task-003-link-and-poll-loop.md)
- [004 — Packaging and setup](tasks/mixxx-connector-task-004-packaging-and-setup.md)
- [005 — Relink after an outage](tasks/mixxx-connector-task-005-relink-after-outage.md)
- [006 — Commands to Mixxx](tasks/mixxx-connector-task-006-commands.md)

Display path packets. ADR 0008 governs them. They are sequential. Task 004
needs relay ADR 0003 for a live test.

- [001 — The song file is never deleted](tasks/display-path-task-001-song-file.md)
- [002 — The display link and the display state](tasks/display-path-task-002-display-link.md)
- [003 — The producer display output](tasks/display-path-task-003-producer-output.md)
- [004 — The publisher display path](tasks/display-path-task-004-publisher.md)

Tag read packets. The review of display task 003 found the defect.

- [001 — The producer reads the image tag](tasks/tag-read-task-001-image-tag.md)

Reserved event safety packets. The
[reserved event safety plan](plans/reserved-event-safety.md) registers them.
Do task 001 and task 002 first. Task 004 needs task 002.

- [001 — `provision` never replaces a token](tasks/reserved-safety-task-001-provision-token.md)
- [002 — `target add --replace` keeps the fields it was not given](tasks/reserved-safety-task-002-target-replace.md)
- [003 — The setup helper keeps an existing event](tasks/reserved-safety-task-003-setup-keeps-event.md)
- [004 — The setup helper writes the display output](tasks/reserved-safety-task-004-setup-display.md)

Packets for the live value reference shape plan. ADR 0010 governs them.
Ready - 2026-10-06. Ship both in one release.

- [001 — The drop file version 2](tasks/reference-shape-task-001-dropfile-v2.md)
  — Implemented - 2026-10-06
- [002 — The payload fields](tasks/reference-shape-task-002-payload-fields.md)
  — Implemented - 2026-10-06

Packets for the relay delay plan. ADR 0011 governs them. Ready - 2026-10-06.
Ship both in one release, after the relay of ADR 0004 is deployed.

- [001 — The delay header](tasks/relay-delay-task-001-delay-header.md)
- [002 — Send at once](tasks/relay-delay-task-002-send-at-once.md)

Packets for the display pairing plan. ADR 0012 governs them. They start after
ADR 0010 task 001 and ADR 0011 task 002. Ship both in one release, after the
relay of ADR 0005 is deployed.

- [001 — The display file version 2](tasks/display-pairing-task-001-display-json-v2.md)
  — Implemented - 2026-10-06
- [002 — The pairing](tasks/display-pairing-task-002-pairing.md)
  — Implemented - 2026-10-06

## Reviews

- [Rust now-playing review checklist](reviews/rust-now-playing-review-checklist.md)
- [Now-playing / live publisher audit review](reviews/nowplaying-publisher-audit-review.md) — accuracy, stability, and security audit of both crates at `1dea27f`
- [Publisher control CLI review checklist](reviews/publisher-control-cli-review-checklist.md)
- [Publisher control CLI implementation review](reviews/publisher-control-cli-implementation-review.md)
- [Relay lease review checklist](reviews/relay-lease-review-checklist.md)
- [Mixxx connector review checklist](reviews/mixxx-connector-review-checklist.md)
- [Display path review checklist](reviews/display-path-review-checklist.md)
- [Live value reference shape review checklist](reviews/live-value-reference-shape-review-checklist.md) — open
- [Relay applies the stream delay review checklist](reviews/relay-applies-stream-delay-review-checklist.md) — open
- [Display pairing review checklist](reviews/display-pairing-review-checklist.md) — open

## Runbooks

- [MusicIndex live publisher deployment](runbooks/musicindex-live-publisher-deploy.md)
- [MusicIndex live publisher Arch package](runbooks/musicindex-live-publisher-arch-package.md)
- [MusicIndex live publisher configuration](runbooks/musicindex-live-publisher-configuration.md)
