# Show Log Task 002: The Track And Clear Entries

Status: Implemented - 2026-10-19. It needs task 001.

Every criterion is mechanical.

## Goal

With `--state-dir <dir>`, the publisher writes a `track` or `clear` entry to
`<dir>/showlog/` after each publish attempt of each target. `observed_at` is
the time at which the watcher read the drop file, or found it gone.

## Files To Inspect

- `docs/adr/0017-show-log.md` (§The Time Source, §Entries, §The Last Entry
  For A Block Wins, §The Log Never Stops A Publish)
- `src/showlog.rs` (task 001)
- `src/watcher.rs`: `payload_for_upsert`, `process_event`,
  `startup_payloads`, `producer_missing_payloads`
- `src/livevalue.rs`: `LiveValuePayload`, `payload_from_dropfile`,
  `dead_payload`
- `src/relay.rs`: `RelayPublisher::start_with_backoff`, `publish_worker`,
  `run_publish`, the keepalive republish
- `src/main.rs`: `Cli`, `run_cli`, `emit_items`
- `tests/relay.rs` (the stub relay), `tests/golden.rs`

## Files Likely To Change

- `src/livevalue.rs`
- `src/watcher.rs`
- `src/relay.rs`
- `src/main.rs`
- `tests/showlog.rs` (new)

## Do Not Touch

- The serialized payload. The golden tests must pass with no change.
- The publish rules: block identity, the dead block, the retry, the
  keepalive, the producer liveness.
- The display path. Task 003 adds the `display` entries.
- `mixxx-now-playing/**`, `Cargo.toml`, `docs/adr/**`.

## Constraints

- **The facts of a payload.** Add one field to `LiveValuePayload` with
  `#[serde(skip)]`, as `play_id` has: `log: Option<LogFacts>`. `LogFacts`
  holds `observed_at: SystemTime`, `kind` (`track` or `clear`), and for a
  track: `payable`, `artist`, `title`, `album`, `play_id`, `duration_secs`,
  `feed_guid`, `track_guid` and `image_url`, all from the drop file.
- **Where the facts come from.**
  - `payload_for_upsert` takes the time with `SystemTime::now()` after it
    reads the file, and sets `kind` `track`. `payable` is `true` when
    `value_routes` is not empty. A non-payable track keeps its drop-file
    facts, although its payload is the dead block.
  - A `Remove` event, `startup_payloads` for a target with no file, and
    `producer_missing_payloads` set `kind` `clear` with the time of that
    call.
  - `PartialEq` of `LiveValuePayload` must ignore `log`. The watcher compares
    payloads to skip an unchanged one. A new read time must not make an
    unchanged payload look new. Implement `PartialEq` by hand, or put the
    time in a wrapper whose `PartialEq` is always `true`. Explain the choice
    in a comment.
- **Where the entry is written.** The relay worker writes the entry after
  each outcome of `client.publish`: `accepted` with `seq`, `retryable`,
  `dropped`, or `fatal`. The entry takes `target`, `event_guid`,
  `block_guid` and `value` from the payload, `listener_delay_secs` from the
  target (the value that `Listener-Delay-Secs` sends), and the rest from
  `log`. A retry keeps the `observed_at` of the first read.
- **The keepalive republish** after a `409` is a publish attempt. It writes
  an entry with the facts that the payload already has.
- **A payload with no `log`** writes no entry. A test payload, for example,
  has none.
- **The log is shared.** `RelayPublisher::start_with_backoff` takes an
  `Option<Arc<ShowLog>>`. Each worker gets a clone. `None` writes nothing.
- **The flag.** `--state-dir <dir>` opens `ShowLog` at `<dir>/showlog` with
  the retention default of 365 days, and calls `remove_expired` one time at
  startup. If `ShowLog::open` fails, the publisher logs a warning and runs
  with no log. With no flag, no log. `--dry-run` writes no log.
- No token, header or file path of a token in an entry.

## Implementation Steps

1. Add `LogFacts` and the `log` field, and the `PartialEq` rule.
2. Fill `log` in the watcher.
3. Pass `Option<Arc<ShowLog>>` to the workers, and write the entry after each
   outcome.
4. Add `--state-dir` and open the log in `run_cli`.
5. Add the tests of §Acceptance Criteria.

## Acceptance Criteria

Each item is a test in `tests/showlog.rs` with the stub relay, or a unit
test:

- A payable drop file gives one `track` entry with `payable: true`,
  `publish_result: "accepted"` and the relay `seq`. Its `value` is equal to
  the `value` that the stub relay received.
- A drop file with an empty `value_routes` gives a `track` entry with
  `payable: false`. The entry has the artist and the title of the drop file,
  and the `value` of the dead block.
- A removed drop file gives a `clear` entry.
- A stub relay that answers `503` and then `200` gives a `retryable` entry,
  then an `accepted` entry. The two entries have the same `observed_at` and
  the same `block_guid`.
- A stub relay that answers `413` gives a `dropped` entry.
- A rewrite of the same drop file with no change gives no second entry.
- The log file holds no target token.
- A log directory that is read-only does not stop the publish. The stub relay
  still gets the payload.
- The golden tests pass with no change.
- With no `--state-dir`, no `showlog` directory exists after a publish.

Also: the full gate passes.

## Test Commands

```bash
cargo fmt --all -- --check
cargo build --workspace
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
```

## Escalation Triggers

- The `log` field changes a golden test.
- The watcher cannot ignore `log` in its comparison of payloads.
- A publish rule must change to write an entry.

## Prompt for lower-context coding model

You are implementing one bounded task from a larger plan.

Implement only this task. Do not redesign the architecture.

Read:
- docs/adr/0017-show-log.md
- docs/plans/show-log.md
- docs/tasks/show-log-task-002-track-and-clear-entries.md
- AGENTS.md
- src/showlog.rs, src/watcher.rs, src/livevalue.rs, src/relay.rs, src/main.rs, tests/relay.rs, tests/golden.rs

Goal:
- With --state-dir, the publisher writes a track or clear entry after each publish attempt of each target. observed_at is the time at which the watcher read the drop file or found it gone.

Constraints:
- Follow §Constraints of the packet exactly. The serialized payload must not change. PartialEq of the payload ignores the log facts. A log failure never stops a publish.

Do not touch:
- the serialized payload, the publish rules, the display path, mixxx-now-playing/**, Cargo.toml, docs/adr/**

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

Reviewed 2026-10-19. The full gate passes with 537 tests in the workspace.
Task 001 ended with 523. `Cargo.toml`, `Cargo.lock` and `tests/golden.rs` did
not change.

- The watcher takes the read time after it reads the drop file. A
  non-payable track keeps its drop-file facts with the dead block. A remove,
  a startup with no file, and a missing producer give `clear`.
- The worker writes one entry after each outcome of `run_publish`. The
  keepalive republish after a `409` goes through `run_publish`, so it writes
  an entry too. A retry keeps the first read time.
- `target` comes from the name of the relay target. `event_guid`,
  `block_guid` and `value` come from the payload. That agrees with ADR 0017.
- The agent added `RelayPublisher::start_with_show_log`. `start_with_backoff`
  keeps its signature and passes `None`. The packet said to change
  `start_with_backoff`. The new function keeps the display tests unchanged,
  and the result is the same.
- The agent grouped the worker inputs in `PublishWorkerInputs`, to stay
  under the clippy argument limit.
- The review found one risk and corrected it. The hand-written `PartialEq`
  of `LiveValuePayload` named each field. A field added later and missing
  from that list would make two different blocks equal, and the watcher
  would not publish the new block. The review changed `eq` to destructure
  `self` with every field. A new field is now a compile error there.
