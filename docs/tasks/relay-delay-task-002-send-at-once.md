# Relay Delay Task 002: Send At Once

Status: Implemented - 2026-10-06. Deploy it only after the relay of
`musicindex-live-relay` ADR 0004 runs.

The acceptance criteria are mechanical. The visual checks are in a separate
list.

## Goal

The publisher sends each block and each display state at once.
`PublishSchedule` is deleted. The keepalive stops after the dead block
publish, by the order of the worker channel.

## Files To Inspect

- `docs/adr/0011-relay-applies-stream-delay.md` (each section)
- `docs/adr/0005-producer-liveness-and-dead-block.md` (§Invariants)
- `docs/adr/0008-display-path.md` (§The Publisher)
- `docs/tasks/relay-delay-task-001-delay-header.md`
- `src/main.rs`: `run_watch_loop`, `update_producer_state`, `emit_items`,
  `missing_transitions_for_released`, `next_wakeup`, and the tests that build
  a `PublishSchedule`
- `src/schedule.rs`, `tests/schedule.rs`
- `src/display.rs`: `schedule_dir`, `schedule_null`
- `src/relay.rs`: `publish_worker`, `run_publish`
- `README.md`, `docs/runbooks/musicindex-live-publisher-configuration.md`,
  `packaging/arch/musicindex-live-publisher.example.toml`

## Files Likely To Change

- `src/main.rs`
- `src/display.rs`
- `src/lib.rs` (the `schedule` export)
- `src/schedule.rs` (deleted)
- `tests/schedule.rs` (deleted)
- `README.md`
- `docs/runbooks/musicindex-live-publisher-configuration.md`
- `packaging/arch/musicindex-live-publisher.example.toml`

## Do Not Touch

- `src/relay.rs` except for comments that name `PublishSchedule`
- `src/config.rs`. The setting keeps its name, its limit and its parse rules.
- `mixxx-now-playing/`
- `docs/adr/**`. The reviewer adds the dated sentences to ADR 0005 and ADR
  0008.

## Constraints

- Each payload, each dead block and each display entry goes through the same
  path as the startup items today: `emit_items`. That path already pairs a
  dead block with the `Producer(Missing)` command of its target.
- The worker channel of a target keeps the order of its commands. A dead
  block thus reaches the worker before `Producer(Missing)`.
- The planner examined `run_publish` on 2026-10-06. It retries a block until
  the relay accepts it, drops it, or the worker stops. A `Producer` command
  that arrives during a retry only sets the state. After the block, a missing
  producer gives no keepalive (`keepalive_wait`). The keepalive thus stops
  after the dead block publish, with no new code. Do not change this logic.
- `next_wakeup` gives the health check interval, because no deadline exists.
- Delete `src/schedule.rs` and `tests/schedule.rs`. Remove the export. Copy
  no part of them into another file.
- Comments that name `PublishSchedule` or the stream-delay hold change to the
  new rule. A comment does not describe the old design.
- The documents say that `stream_delay_secs` is the delay that podcast apps
  get on Socket.IO. The relay applies it. The publisher sends it with each
  publish and does not wait.

## Implementation Steps

1. Change `run_watch_loop` and `update_producer_state` to call `emit_items`
   at once.
2. Change `DisplayPath::schedule_dir` and `schedule_null` to give items for
   `emit_items`. Rename them if the new names are clearer.
3. Simplify `next_wakeup`.
4. Delete the schedule module and its tests. Change the tests in
   `src/main.rs` that used it.
5. Add the tests.
6. Update the three documents.

## Acceptance Criteria

Mechanical. Each item is a test:

- A drop-file change publishes its block at once. No time passes in the test.
- A display change uploads its image and publishes its state at once.
- A free producer lock gives one dead block for each target at once. The
  worker gets `Producer(Missing)` after that block, and the keepalive stops.
- A producer that returns before the dead block publish still gets no stale
  `Producer(Missing)`. The present test of that rule passes.
- `grep -rn PublishSchedule src tests` finds nothing.
- The existing tests that do not use the schedule pass with no change.

Also: the full gate passes.

Visual. A person must examine these items. Report each one as open until a
person completes the check.

- A podcast app on Socket.IO changes the block at the same time as before,
  with the same `stream_delay_secs`.
- The private app on ICY shows the artwork when the listener hears the track,
  also for a listener with a short buffer.

## Test Commands

```bash
cargo fmt --all -- --check
cargo build --workspace
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
```

## Escalation Triggers

- The retry logic of `run_publish` differs from §Constraints.
- A test outside the schedule tests depends on a held payload.
- The display path needs the image bytes after the producer deletes them.

## Prompt for lower-context coding model

You are implementing one bounded task from a larger plan.

Implement only this task. Do not redesign the architecture.

Read:
- docs/adr/0011-relay-applies-stream-delay.md
- docs/adr/0005-producer-liveness-and-dead-block.md
- docs/adr/0008-display-path.md
- docs/tasks/relay-delay-task-002-send-at-once.md
- src/main.rs, src/display.rs, src/schedule.rs, src/relay.rs, src/lib.rs
- tests/schedule.rs
- README.md, docs/runbooks/musicindex-live-publisher-configuration.md, packaging/arch/musicindex-live-publisher.example.toml

Goal:
- Send each block and each display state at once through emit_items.
- Delete PublishSchedule.
- Keep the keepalive stop after the dead block. The order of the worker channel gives it.

Constraints:
- Follow §Constraints of the packet exactly. Delete the schedule code. Do not keep it unused.

Do not touch:
- src/relay.rs except comments, src/config.rs, mixxx-now-playing/, docs/adr/**

Acceptance criteria:
- Each mechanical item in §Acceptance Criteria of the packet is a passing test.

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

Reviewed 2026-10-06. The full gate passes. `grep -rn PublishSchedule src
tests` finds nothing.

The review accepts these decisions of the task:

- `ScheduledItem` became the private type `EmitItem` in `src/main.rs`. Only
  the binary uses it now, so the library exports nothing for it.
- `update_producer_state` gives its items to `run_watch_loop`, which makes
  one `emit_items` call for each loop pass. That keeps the function testable
  without a publisher.
- `schedule_dir` and `schedule_null` were in `src/main.rs`, not in
  `src/display.rs` as the packet said. They are now `items_for_dir` and
  `null_items`.
- The four tests of the stream-delay schedule in `tests/display.rs` are
  deleted. They asserted the rule that ADR 0011 replaces.

The keepalive rule: `emit_items` sends every item, then `Producer(Missing)`
for each target whose dead block it sent. The worker channel keeps that
order, and `run_publish` retries the dead block until the relay accepts it.
The keepalive thus stops after the dead block. The existing tests of
`missing_transitions_for_released` and of the worker cover the two parts.

The review made one change. `AGENTS.md` and
`docs/architecture/broadcast-chain-boundaries.md` said that the publisher
holds each block. They now say that it sends at once, and that this publisher
must not be deployed before the relay of ADR 0004 runs.

Open: the dated sentences in ADR 0005 and ADR 0008 (ADR 0011 §What This
Replaces) wait for the review checklist, with its visual checks.
