# Display Path Task 004: The Publisher Display Path

Status: Implemented - 2026-10-04.

The acceptance criteria are mechanical. The visual check is in a separate
list.

## Goal

A target with `display_dir` sends the display state and its image to the
relay, through the stream delay of that target. A display request never
delays a payload or a keepalive.

## Files To Inspect

- `docs/adr/0008-display-path.md` (§The Publisher, §Invariants)
- `musicindex-live-relay`: `docs/adr/0003-display-state-and-artwork.md`
  (the routes and the status codes)
- `src/config.rs` (the target configuration)
- `src/main.rs` (`run_watch_loop`, `emit_payloads`, `update_producer_state`)
- `src/schedule.rs` (`PublishSchedule`)
- `src/relay.rs` (`RelayPublisher`, the workers, the status handling)
- `tests/relay.rs` (the stub relay pattern)

## Files Likely To Change

- `src/config.rs`
- `src/display.rs` (new)
- `src/schedule.rs`
- `src/main.rs`
- `src/relay.rs`
- `src/lib.rs`
- `tests/relay.rs`, `tests/config.rs`, `tests/schedule.rs`
- `docs/runbooks/musicindex-live-publisher-configuration.md`

## Do Not Touch

- `mixxx-now-playing/**`
- The payload and keepalive rules and their tests
- `docs/adr/**`

## Constraints

- `display_dir` is an optional field of a target. It must not be the
  `watch_dir`. A wrong value is a configuration error with the target name.
- The watch loop also watches each `display_dir`. A display event never starts
  a producer probe.
- When `display.json` changes, the publisher reads it and, for an embedded
  image, the image bytes at once. A file with an unknown schema is ignored
  with a warning. A missing image file gives the display state with
  `artwork: null` and a warning.
- The schedule holds display entries next to payloads, in one order for each
  target, with the same delay. Generalize `PublishSchedule` to a scheduled item
  with a payload or a display entry. The present payload tests must pass with
  no change.
- One display worker thread serves every target. A payload worker never sends
  a display request. A display request never waits for a payload worker.
- The display worker:
  - uploads an embedded image that it did not upload before in this process,
    then publishes the display state,
  - sends only the latest state of a target when several wait,
  - retries a network error, a `5xx` or a `429` with backoff, at most each 30
    seconds, with the latest state only,
  - after `409 artwork_missing`, uploads the image again and publishes again,
    one time,
  - after `404` or `409 event_not_reserved`, turns off the display path of
    that target until the next start, with one `tracing::warn!` line,
  - logs every other failure with `tracing::warn!`. No display failure is
    fatal.
- When a keepalive gets `409` and the publisher sends its last payload again,
  the display worker also sends the latest display state of that target
  again. A lease end clears the display state and the images in the relay
  (relay display state tasks 001 and 002). Added 2026-10-04.
- When the producer becomes missing, the publisher schedules the display state
  `null` for each display target, next to the dead block.
- The display publish body holds only the key `track`. Do not send the
  `schema` key of `display.json`. The relay refuses an unknown key with
  `400 invalid_display` (relay display state task 001). Added 2026-10-04.
- A display request uses the broadcaster token of the target. The token never
  appears in a log line, an error or a `Debug` output (AGENTS.md §5).
- The configuration runbook describes `display_dir`, restates ADR 0008 and
  names it as the owner.

## Implementation Steps

1. Add `display_dir` to the configuration.
2. Add `display.rs`: the display entry, the file read and the schema check.
3. Generalize the schedule.
4. Add the display worker and the relay requests.
5. Add the watch, the producer-missing rule and the tests.
6. Add the runbook section.

## Acceptance Criteria

Mechanical. Each item is a test:

- A display entry is released after the stream delay of its target, in order
  with the payloads.
- A payload is sent while an image upload of the display worker is still
  open. Use a stub relay that holds the upload.
- An image is uploaded once for two display states that name it.
- Of three waiting states for a target, only the last is sent.
- `409 artwork_missing` gives one new upload and one new publish.
- `404` and `409 event_not_reserved` turn off the display path of the target,
  and a later payload still goes out.
- A producer that becomes missing gives the display state `null` after the
  delay.
- `display_dir` equal to `watch_dir` is a configuration error.
- A display event does not start a producer probe.
- No log line or `Debug` output holds the token.
- Each rule was broken on purpose, and a test failed. The report lists each
  mutation.

Also: the full gate passes.

Visual. A person checks this with the relay of ADR 0003 and the private app:

- The artwork changes when the listener hears the new track, for a V4V track
  and for a track that pays nobody.

## Test Commands

```bash
cargo fmt --all -- --check
cargo build --workspace
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
```

## Escalation Triggers

- The schedule cannot hold display entries without a change to a payload test.
- The display worker needs a change to a payload worker.
- A rule here conflicts with ADR 0008 or relay ADR 0003.

## Prompt for lower-context coding model

Implement only this task. Read docs/adr/0008-display-path.md, relay ADR 0003
for the routes, this packet, and the publisher files in §Files To Inspect. Add
the display path for targets with display_dir, with one display worker that
never blocks a payload, exactly as §Constraints says. Do not change a payload
or keepalive rule or test. Write the tests in §Acceptance Criteria, with a
mutation list. Run the test commands. Report: 1. files changed 2. tests run
3. behavior changed 4. deviations 5. unresolved concerns 6. mutations.

## Review Result

Reviewed 2026-10-04. The full gate passes, and the 27 mapping tests pass.
`Cargo.lock` adds `sha2` to the publisher package only. That crate was in the
lock already.

No payload or keepalive test changed. The `config()` helper in
`tests/relay.rs` got one line, `display_dir: None`, because the target type
has a new field. The review accepts that line.

One display thread serves every target. A payload worker sends no display
request. After a lease republish, a payload worker puts one `Resend` command
on the display channel, and that send never blocks. The token is only in
`RelayTarget`, and its `Debug` output hides the token.

The review accepts these deviations:

- The display tests are in `tests/display.rs`.
- A resend clears the image cache of the target. The `artwork_missing` path
  still has a test.
- After a refused upload, the worker sends the state with `artwork: null`.
- The publisher checks the first bytes of an image against its type.
- At startup, the display state goes out with no stream delay, as the startup
  payload does.
- `config show` and `target list` do not show `display_dir`.

Open items:

- `target add --replace` writes the target from its flags only. It removes a
  `display_dir` from the file. The control command has no `--display-dir`
  option. This needs a task before a control surface uses `--replace` on a
  target with a display path.
- The watch on a display directory is added one time. If that directory is
  deleted and made again, the publisher gets no display event until a
  restart.
- The visual check is open.
