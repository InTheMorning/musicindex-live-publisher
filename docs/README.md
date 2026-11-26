# Documentation

## Order Of Work

The packets here belong to a system that spans three repositories. The
cross-repository order lives in the HitIt repository, `HitIt`:
`docs/plans/broadcast-chain-delivery-order.md`. Read it before you start.

Control-surface tasks 001 and 002 are complete. The HitIt attach-event packet is
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
- [ADR 0004: Publisher control CLI](adr/0004-publisher-control-cli.md) —
  Implemented. The target and JSON commands that let HitIt control this
  repository without writing its configuration file
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
  — Accepted. The song line, and a MusicIndex ID3 frame that a tagger on the
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
- [ADR 0013: The display state carries the album](adr/0013-display-album.md)
  — Accepted. `display.json` version 3 with the album of the history row.
  The publisher adds `album` to the display state (relay ADR 0006)
- [ADR 0014: One assembler, and each producer reports each track](adr/0014-one-assembler-and-track-reporting.md)
  — Implemented. Only this repository assembles and publishes
  blocks. Drop file version 3: each producer reports each track that plays, payable or not
- [ADR 0015: Live segments from HitIt](adr/0015-live-segments-from-v4vmm.md)
  — Accepted. HitIt starts a talk break or a live set through `segment`
  commands over `ssh`, with a split that it sets for each show
- [ADR 0016: Instances, events and the producer lock](adr/0016-instances-events-and-producer-lock.md)
  — Implemented. One instance for each show and for each event, and the rules
  of the producer lock. The binding of a drop file to the lock is deferred
- [ADR 0017: The show log](adr/0017-show-log.md)
  — Implemented. The log that HitIt reads to export an
  episode. The time is the read time of this service, and the art is a
  reference. It supersedes ADR 0003
- [ADR 0018: The HitIt configuration path](adr/0018-hitit-configuration-path.md)
  — Accepted. The producer reads the `hitit` configuration file first, then
  the `v4vmm` file. With no file, it writes a warning and uses the defaults
- [ADR 0019: The Debian package and the Docker image](adr/0019-debian-package-and-docker-image.md)
  — Accepted. cargo-deb makes the `.deb`. The Docker image holds the
  publisher only. A tag `v*` starts `.github/workflows/release.yml`
- [ADR 0021: A template unit for the Mixxx producer](adr/0021-mixxx-producer-template-unit.md)
  — Accepted. `mixxx-now-playing@<instance>.service` reads its instance
  configuration. A kernel lock lets one Mixxx producer run at a time. The
  setup helper gets `--instance`, `--target`, `--event-id`, `--json` and
  `--migrate-units`. It changes ADR 0016 §Producer Units
- [ADR 0023: A live DJ takes over the event of a station](adr/0023-dj-takeover.md)
  — Accepted. Each publisher sends its payable block again over a foreign dead
  block at each keepalive. No relay change
- [ADR 0024: A payable block needs the feed GUID and the track GUID](adr/0024-payable-block-needs-both-guids.md)
  — Accepted. The publisher publishes routes only with the two GUIDs. Each
  producer asks a route service with the two GUIDs. It changes ADR 0014
- [ADR 0029: Producer liveness, the dead block and the remoteValue title](adr/0029-producer-liveness-dead-block-and-remote-value-title.md)
  — Accepted. It replaces ADR 0005: the producer lock, the dead block, the
  relay keepalive and the expiry limit without the MIDI connector. The dead
  block title and image are settings of each target, and a track block has the title
  "artist - title". It changes ADR 0010
- [ADR 0030: station-import writes the feed value in each standard frame](adr/0030-station-import-writes-the-feed-value-in-each-standard-frame.md)
  — Accepted. The feed value replaces `TIT2`, `TPE1`, `TALB`, `TPE2`, `TRCK` and
  `TDRC` of a station file. A frame with no feed value keeps the file value
- [ADR 0032: The publisher repository owns the test station operation](adr/0032-the-publisher-repository-owns-the-test-station.md)
  — Accepted. The station steps, the station log and the station runbooks are
  in this repository. A fault in another repository goes to the orchestrator
- [ADR 0033: The dead block has one destination that a payment cannot reach](adr/0033-the-dead-block-has-one-unreachable-destination.md)
  — Accepted. The dead block has one destination at
  `no-v4v-track@example.invalid`. It supersedes ADR 0031
- [ADR 0034: The payload image falls back to the relay copy of the picture](adr/0034-the-payload-image-falls-back-to-the-relay-copy-of-the-picture.md)
  — Accepted. When the drop file has no image URL, the payload `image` is the
  display URL or the relay URL of the picture of the same play. Relay
  ADR 0011 keeps the image. It changes ADR 0010
- [ADR 0036: The HLS stream carries the picture one time for each song](adr/0036-the-hls-stream-carries-the-picture-one-time-for-each-song.md)
  — Accepted. An `APIC` frame goes into the first segment of each song, on
  the station and on the VPS stack. It needs a change of the Liquidsoap cover
  keys. It changes ADR 0009 and ADR 0035. ADR 0041 restates its change to
  ADR 0035
- [ADR 0037: The listener delay comes from the Icecast burst](adr/0037-the-listener-delay-comes-from-the-icecast-burst.md)
  — Accepted. A target can give `icecast_burst_bytes` and
  `stream_bitrate_kbps`. The publisher sends that delay, rounded down, as
  `Listener-Delay-Secs`. It changes ADR 0011
- [ADR 0038: The HLS stream marks each segment](adr/0038-the-hls-stream-marks-each-segment.md)
  — Accepted. The station slot text writes a frame `musicindex.hls/2` with
  the `playId` of the segment at each chapter boundary. It changes ADR 0009
  and ADR 0035, and adds `playId` to the display state of ADR 0039. ADR 0041
  restates its change to ADR 0035
- [ADR 0039: The display state pairs with its value block](adr/0039-the-display-state-pairs-with-its-value-block.md)
  — Accepted. One record for the pairing rules of ADR 0012 and its three
  changes. It adds no rule. It supersedes ADR 0012
- [ADR 0041: The station producer](adr/0041-the-station-producer.md)
  — Accepted. One record for the station producer. It restates each live rule
  of ADR 0035 and its changes by ADR 0036, ADR 0038 and ADR 0040, and adds no
  rule. It supersedes ADR 0035 and ADR 0040
- [ADR 0042: An episode sends its publication date](adr/0042-an-episode-sends-its-publication-date.md)
  — Accepted. An episode state sends the `TDRC` day as `pub_date` in
  `display.json` and as `pubDate` on the wire. It changes ADR 0013 and
  ADR 0041. Relay ADR 0013 gives the wire rule, and Rainbrook ADR 0031
  shows the date
- [ADR 0043: The display state carries the funding links of the track](adr/0043-the-display-state-carries-the-funding-links.md)
  — Accepted. A display track gets `funding`, 1 to 3 links with `url` and
  `label`, from the route service. It changes ADR 0013 and ADR 0041.
  Rainbrook ADR 0031 shows the links
- [ADR 0044: A DJ track carries its funding links](adr/0044-a-dj-track-carries-its-funding-links.md)
  — Proposed. The Mixxx producer gets `funding` from the same route service
  answer as the routes of the track. It changes ADR 0043

## Plans

- [Mixxx producer template unit](plans/mixxx-producer-template-unit.md) — the
  template producer unit, the Mixxx lock, and the setup helper of ADR 0021
  — Implemented - 2026-11-21. The manual checks are open.
- [Liquidsoap producer plan](plans/liquidsoap-producer.md) — Ready. Five
  packets for ADR 0022, ADR 0023 and ADR 0024
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
- [Display pairing plan](plans/display-pairing.md) — Implemented. Two packets
  for ADR 0039, in one release, after relay ADR 0005
- [Display album plan](plans/display-album.md) — Ready. One packet for
  ADR 0013, after relay ADR 0006
- [Relay applies the stream delay plan](plans/relay-applies-stream-delay.md)
  — Ready. Two packets for ADR 0011, in one release, after relay ADR 0004
- [Each producer reports each track plan](plans/track-reporting.md)
- [Instances, events and the producer lock plan](plans/instances-and-producer-lock.md)
- [Show log plan](plans/show-log.md)
  — Implemented. Two packets for ADR 0014, in one release

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


Control surface packets. ADR 0004 governs them. They support the HitIt
broadcast control surface (HitIt ADR 0059). Task 001 gates HitIt task 014.

- [001 — Target management commands](tasks/control-surface-task-001-target-management.md)
- [002 — Machine-readable CLI surface](tasks/control-surface-task-002-machine-readable-cli.md)

Studio client packets. ADR 0020 governs them after packet a writes it. Studio
ADR 0110 owns the interface. The task waits for Studio task 073.

- [001 — Start and end a show through Studio](tasks/studio-client-task-001-studio-client.md)

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

Packets for the display pairing plan. ADR 0039 governs them. They start after
ADR 0010 task 001 and ADR 0011 task 002. Ship both in one release, after the
relay of ADR 0005 is deployed.

- [001 — The display file version 2](tasks/display-pairing-task-001-display-json-v2.md)
  — Implemented - 2026-10-06
- [002 — The pairing](tasks/display-pairing-task-002-pairing.md)
  — Implemented - 2026-10-06

Packet for the display album plan. ADR 0013 governs it. Ship it after relay
ADR 0006 is deployed.

- [001 — The display file version 3](tasks/display-album-task-001-display-json-v3.md)
  — Implemented - 2026-10-06

Packets for the track reporting plan. ADR 0014 governs them. Ship both in one
release.

- [001 — The drop file version 3, history-only mode](tasks/track-reporting-task-001-dropfile-v3-history-only.md)
  — Implemented - 2026-10-18
- [002 — The non-payable file in the connector mode](tasks/track-reporting-task-002-connector-mode.md)
  — Implemented - 2026-10-18

Packets for the instances plan. ADR 0016 governs them. Each packet can ship
by itself.

- [001 — The producer lock wait limit](tasks/instances-task-001-producer-lock-wait.md)
  — Implemented - 2026-10-18
- [002 — One instance for each event](tasks/instances-task-002-event-ownership-checks.md)
  — Implemented - 2026-10-18

Packets for the Mixxx producer template plan. ADR 0021 governs them. Each
packet needs the one before it.

- [001 — `--instance` and the Mixxx lock](tasks/producer-template-task-001-instance-and-mixxx-lock.md)
  — Implemented - 2026-11-21
- [002 — The template unit and the packages](tasks/producer-template-task-002-template-unit-and-packaging.md)
  — Implemented - 2026-11-21
- [003 — The setup helper and the migration](tasks/producer-template-task-003-setup-helper-and-migration.md)
  — Implemented - 2026-11-21

Packets for the show log plan. ADR 0017 governs them. Each packet needs the
one before it.

- [001 — The writer module](tasks/show-log-task-001-writer-module.md)
  — Implemented - 2026-10-19
- [002 — The track and clear entries](tasks/show-log-task-002-track-and-clear-entries.md)
  — Implemented - 2026-10-19
- [003 — The display entries, the unit and the documents](tasks/show-log-task-003-display-entries-and-unit.md)
  — Implemented - 2026-10-19
- [004 — The off switch](tasks/show-log-task-004-off-switch.md)
  — Implemented - 2026-10-19. The amendment of ADR 0017 that adds `show_log`

Packets for the station producer record. ADR 0041 governs them.

- [001 — Comments and runbooks cite the current record](tasks/adr-0035-task-001-cite-the-current-record.md)
  — Implemented - 2026-11-26

Packets for the listener delay. ADR 0037 governs them.

- [001 — The delay from the Icecast burst](tasks/listener-delay-task-001-icecast-burst.md)
  — Implemented - 2026-11-26

Packets for the segment frame. ADR 0038 governs them.

- [001 — The display state gets `playId`](tasks/segment-frame-task-001-publisher-play-id.md)
  — Implemented - 2026-11-26
- [002 — The slot text writes the segment frame](tasks/segment-frame-task-002-slot-text-frame.md)
  — Implemented - 2026-11-26

Packets for the episode album. ADR 0041 §The Album Of A Segment governs them.

- [001 — A chapter song has the episode title as its album](tasks/episode-album-task-001-chapter-album-is-episode-title.md)
  — Implemented - 2026-11-26

Packets for the display keys. ADR 0042 and ADR 0043 govern them.

- [001 — The crate `display-keys` and the publisher keys](tasks/display-keys-task-001-crate-and-publisher.md)
  — Implemented - 2026-11-26
- [002 — The station producer writes the episode date](tasks/display-keys-task-002-producer-pub-date.md)
  — Implemented - 2026-11-26
- [003 — The station producer gets the funding links](tasks/display-keys-task-003-producer-funding.md)
  — Ready - 2026-11-26

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
- [Display pairing review checklist](reviews/display-pairing-review-checklist.md)
- [Track reporting review checklist](reviews/track-reporting-review-checklist.md)
  — the named artifact for the `Implemented` status of ADR 0014
- [Display album review checklist](reviews/display-album-review-checklist.md) — open
- [Instances review checklist](reviews/instances-review-checklist.md) — the
  named artifact for the `Implemented` status of ADR 0016
- [Show log review checklist](reviews/show-log-review-checklist.md) — the
  named artifact for the `Implemented` status of ADR 0017

## Runbooks

- [MusicIndex live publisher deployment](runbooks/musicindex-live-publisher-deploy.md)
- [MusicIndex live publisher Arch package](runbooks/musicindex-live-publisher-arch-package.md)
- [MusicIndex live publisher configuration](runbooks/musicindex-live-publisher-configuration.md)
- [MusicIndex producer tutorial](runbooks/musicindex-producer-tutorial.md) — the
  station side of the station producer, for an AzuraCast station
- [Test station log](runbooks/test-station-log.md) — each step on the
  HeyCitizen test station, with its result

## Archive

Superseded decisions and their packets. Read them for research only.

- [ADR 0003: Show log contract](archive/adr/0003-show-log-contract.md)
  — Superseded by ADR 0017 on 2026-10-19
- [ADR 0005: Producer liveness, the dead block and the relay lease](archive/adr/0005-producer-liveness-and-dead-block.md)
  — Replaced by ADR 0029 on 2026-11-24
- [ADR 0012: Pair the display state with its value block](archive/adr/0012-pair-display-state-with-value-block.md)
  — Superseded by ADR 0039 on 2026-11-26
- [ADR 0022: A producer for Liquidsoap and AzuraCast](archive/adr/0022-liquidsoap-producer.md)
  — Superseded by ADR 0035 on 2026-11-26
- [ADR 0025: The station producer pays each chapter song of an episode file](archive/adr/0025-the-station-producer-pays-each-chapter-song.md)
  — Superseded by ADR 0035 on 2026-11-26
- [ADR 0026: The station producer writes the display state with the ICY line of the file](archive/adr/0026-the-station-producer-writes-the-display-state.md)
  — Superseded by ADR 0035 on 2026-11-26
- [ADR 0027: The station producer asks the route service for a song file](archive/adr/0027-the-station-producer-asks-the-route-service-for-a-song.md)
  — Superseded by ADR 0035 on 2026-11-26
- [ADR 0028: The station producer uses the embedded picture of the file as the artwork fallback](archive/adr/0028-the-station-producer-uses-the-embedded-picture.md)
  — Superseded by ADR 0035 on 2026-11-26
- [ADR 0031: The dead block has no destination](archive/adr/0031-the-dead-block-has-no-destination.md)
  — Superseded by ADR 0033 on 2026-11-25
- [ADR 0035: The station producer](archive/adr/0035-the-station-producer.md)
  — Superseded by ADR 0041 on 2026-11-26
- [ADR 0040: A chapter song has the episode title as its album](archive/adr/0040-a-chapter-song-has-the-episode-title-as-its-album.md)
  — Superseded by ADR 0041 on 2026-11-26
- [Show log task 001 — Show log writer](archive/tasks/show-log-task-001-log-writer.md)
  — never started
- [Show log task 002 — Read contract and documentation](archive/tasks/show-log-task-002-read-contract-and-docs.md)
  — never started
