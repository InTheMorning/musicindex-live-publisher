# Mixxx Connector Task 005: Relink After An Outage

Status: Implemented - 2026-09-29. The visual check is open. See §Review
Result.

The acceptance criteria are mechanical. The visual check is in a separate
list.

## Goal

Protocol version 2 adds the deck sample count and an end marker for the
complete state. After an outage of the connector, the producer links the
present row again when the same track still plays on the same deck.

## Files To Inspect

- `docs/adr/0006-mixxx-midi-connector.md` (§Protocol, §How
  `mixxx-now-playing` Uses The Deck State, §Entering And Leaving The Connector
  Mode, §Relink After An Outage, §Invariants)
- `mixxx/MusicIndex-V4V-Connector.js` and `mixxx/tests/connector.test.js`
- `mixxx-now-playing/src/connector/state.rs`
- `mixxx-now-playing/src/connector/link.rs`
- `mixxx-now-playing/src/main.rs` (`drain_connector_events`)

## Files Likely To Change

- `mixxx/MusicIndex-V4V-Connector.js`
- `mixxx/tests/connector.test.js`
- `mixxx-now-playing/src/connector/state.rs`
- `mixxx-now-playing/src/connector/link.rs`
- `mixxx-now-playing/src/connector/mod.rs`
- `mixxx-now-playing/src/main.rs`, only if the event type changes
- `docs/runbooks/musicindex-live-publisher-configuration.md`, only the
  sentence about the check-4 behavior, if one exists

## Do Not Touch

- `src/**` (the publisher)
- `mixxx-now-playing/src/connector/midi.rs`, `device.rs`
- `packaging/**`, `systemd/**`, `scripts/**`
- `docs/adr/**`

## Constraints

The mapping:

- The heartbeat value is 2.
- Monitor `track_samples` for each deck.
- `V4VConnector.sampleParts(samples)` is a pure function. It gives five
  7-bit parts, from bits 28 to 34 down to bits 0 to 6. It rounds down.
  - A value that is not a finite number more than 0 gives five zeros.
  - A value above 2^35 - 1 gives five parts of 127.
  - Do not use the JavaScript bit operators, because they use 32 bits. Use
    `Math.floor` and division.
- Send CC 50+N, 60+N, 70+N, 80+N, then 90+N, when `track_samples` changes.
- The complete state for each deck: `play`, `track_loaded`, the two duration
  parts, the five sample parts. Then CC 2. Then CC 3 with the value 1.

The producer state:

- `PROTOCOL_VERSION` is 2.
- The state keeps the sample parts for each deck. The count applies when part
  5 (CC 90+N) arrives. It gives a deck change `Samples(u64)` only when the
  count differs from the last count.
- CC 3 with the value 1 gives a state-end event. Change the return type of
  `apply` if you need to, for example to an event enum with `Deck(DeckChange)`
  and `StateEnd`.
- `samples(deck)` gives the last count, or `None` before the first count.

The `Coordinator`:

- A `Samples` change on the linked deck ends the link and removes the file,
  the same as a `track_loaded` or `duration` change.
- A link keeps the sample count of its deck at the time of the link.
- When the producer leaves the connector mode with a linked row, keep a relink
  candidate in that row: the deck and the sample count. A row without a link
  gives no candidate. A new history row replaces the row, so its candidate
  goes away.
- On entry into the connector mode, do not clear the present row. Remove the
  file, end the link, keep the candidate, and send the state request. The
  startup entry has no row, so it has no candidate.
- At the first state-end event after the entry, test the candidate:
  - the deck is the loudest deck,
  - the deck plays,
  - the deck sample count is equal to the candidate count, and it is not 0.
- If each condition is true, link the row to the deck again and write the file
  with `duration_secs` from the deck. Log `tracing::info!` with the fields
  `deck` and `relink = true`.
- If a condition is false, log `tracing::info!` with the reason, and drop the
  candidate.
- A change of mode before the state-end event drops the candidate.
- A relink applies also when the ADR 0005 expiry ended during the outage.
- A deck change that arrives before the state-end event does not relink.

## Implementation Steps

1. Change the mapping and its tests.
2. Change `state.rs` and its tests.
3. Change `link.rs` and its tests.
4. Change `main.rs` only if the event type changed.

## Acceptance Criteria

Mechanical. Each item is a test:

- Mapping: `sampleParts` for 0, -1, `NaN`, 1, 127, 128, 2^32 + 5, and
  2^35 + 1.
- Mapping: the complete state has the order in §Constraints and ends with
  `[0xBF, 3, 1]`. The heartbeat value is 2.
- Mapping: a `track_samples` change sends the five parts in order.
- State: a count applies only at part 5; the same count again gives no
  change; a count above 2^32 is correct; CC 3 = 1 gives the state-end event.
- State: a heartbeat of version 1 gives `UnknownVersion`.
- `Coordinator`: an outage and a return with the same deck, loudest, playing
  and the same count gives a write with the deck duration.
- `Coordinator`: each of these gives no write after the state-end event:
  - a different loudest deck,
  - the deck does not play,
  - a different count,
  - a count of 0,
  - a new history row during the outage,
  - a row that had no link before the outage.
- `Coordinator`: no state-end event before the next change of mode gives no
  write.
- `Coordinator`: the startup entry never relinks.
- `Coordinator`: a relink after the expiry ended during the outage writes.
- `Coordinator`: a `Samples` change on the linked deck removes the file and
  ends the link.
- Each rule above was broken on purpose, and a test failed. The report lists
  each mutation.

Also:

- The full gate passes, with `node --test mixxx/tests/`.
- `Cargo.lock` does not change.

Visual. A person checks this with Mixxx and the development setup in
`docs/plans/mixxx-midi-connector.md`. Copy the new mapping to
`~/.mixxx/controllers/` and restart Mixxx first:

- Repeat manual check 4. When the mapping is enabled again, the drop file
  comes back within about 2 seconds for the same track. The log shows
  `relink`.

## Test Commands

```bash
node --test mixxx/tests/
cargo fmt --all -- --check
cargo build --workspace
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
```

## Escalation Triggers

Stop and report if one of these occurs:

- A rule in §Constraints conflicts with ADR 0006.
- The `Coordinator` cannot keep the row at entry without a change to a rule
  that task 003 tests.
- The mapping cannot read `track_samples`.

## Prompt for lower-context coding model

You are implementing one bounded task from a larger plan.

Implement only this task. Do not redesign the architecture.

Read:
- docs/adr/0006-mixxx-midi-connector.md
- docs/tasks/mixxx-connector-task-005-relink-after-outage.md
- mixxx/MusicIndex-V4V-Connector.js, mixxx/tests/connector.test.js
- mixxx-now-playing/src/connector/state.rs, link.rs, mod.rs
- mixxx-now-playing/src/main.rs

Goal:
- Protocol version 2 adds the deck sample count (CC 50+N to 90+N, 7 bits each, high part first, applied at 90+N) and a state-end marker (CC 3 = 1). After an outage, the producer links the present row again when the same deck is loudest, plays, and has the same non-zero sample count as before the outage.

Constraints:
- Mapping: heartbeat 2; monitor track_samples; pure sampleParts with Math.floor, no bit operators; invalid or non-positive gives zeros; above 2^35 - 1 gives 127s; complete state per deck is play, track_loaded, 2 duration parts, 5 sample parts; then CC 2; then CC 3 = 1.
- State: PROTOCOL_VERSION 2; Samples(u64) change only when the count changes; CC 3 = 1 gives a state-end event; samples(deck) -> Option<u64>.
- Coordinator: a Samples change on the linked deck ends the link. A link keeps its sample count. Leaving the connector mode with a linked row keeps a candidate (deck, count) in the row. A new row drops it. Entry keeps the row, removes the file, ends the link, sends the state request. At the first state-end after the entry: loudest, plays and equal non-zero count gives a relink and a write with the deck duration; else drop the candidate. A mode change before state-end drops it. Relink applies after an expiry during the outage. Log info with deck and relink = true, or the reason.

Do not touch:
- src/**, mixxx-now-playing/src/connector/midi.rs and device.rs, packaging/**, systemd/**, scripts/**, docs/adr/**

Acceptance criteria:
- The tests in the task §Acceptance Criteria exist and pass.
- Each rule was broken on purpose and a test failed; report the list.
- The full gate passes. Cargo.lock does not change.

Test commands:
- node --test mixxx/tests/
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
6. the mutation list with results

## Review Result

Reviewed 2026-09-29. The review changed no code. Each rule was broken on
purpose, and a test failed each time.

The review accepts three deviations:

- The expiry uses an `expired` flag, and a relink clears it. Before this
  change, an expiry set the row to "never expires". A second outage after a
  relink then kept the drop file until the next history row. ADR 0006 says
  that the file goes at once when the expiry already ended.
- A sample count that the mapping never sent is the same as 0. It never
  relinks.
- The test helpers changed for protocol version 2.

When the mapping is enabled again, it sends its own complete state before its
first heartbeat. The producer is then still in the history-only mode, so that
end marker does not count. The relink test waits for the reply to the state
request of the producer.

The visual check needs Mixxx. It is open.
