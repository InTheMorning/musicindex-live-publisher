# Relay Lease Task 002: The Dead Block

Status: Ready - 2026-09-27. It needs ADR 0005 to be Accepted. It does not need
the relay or task 001.

Every criterion is mechanical. This packet has no visual criteria, because it
adds no user interface.

## Goal

Replace the configured fallback with one constant dead block. No configuration
can change the dead block. An empty `value_routes` list publishes the dead
block, not a block with zero destinations.

## Files To Inspect

- `docs/adr/0005-producer-liveness-and-dead-block.md` (§The Dead Block and
  §Publisher Behavior)
- `src/config.rs` (`DEFAULT_DEAD_FALLBACK_*`, `default_dead_fallback`,
  `default_dead_fallback_value`, `resolve_fallback`, `RawFallback`,
  `TargetConfigDetails`)
- `src/livevalue.rs` (`fallback_payload`, `payload_from_dropfile`)
- `src/watcher.rs` (`WatchTarget`, `FallbackConfig`,
  `fallback_payload_for_target`, `payload_for_upsert`)
- `src/main.rs` (the `config show` text and JSON output)
- `src/lib.rs`
- `tests/config.rs`, `tests/watcher.rs`, `tests/relay.rs`, `tests/golden.rs`
- `scripts/setup-mixxx-musicindex.sh`
- `packaging/arch/PKGBUILD`, `packaging/arch/musicindex-live-publisher.install`,
  `packaging/arch/mixxx-fallback-value-block.example.toml`

## Files Likely To Change

- `src/livevalue.rs`, `src/config.rs`, `src/watcher.rs`, `src/main.rs`,
  `src/lib.rs`
- `tests/config.rs`, `tests/watcher.rs`, `tests/relay.rs`
- `scripts/setup-mixxx-musicindex.sh`
- `packaging/arch/PKGBUILD`, `packaging/arch/musicindex-live-publisher.install`
- `packaging/arch/mixxx-fallback-value-block.example.toml` (delete)
- `README.md`
- `AGENTS.md` (§4)
- `docs/runbooks/musicindex-live-publisher-configuration.md`
- `docs/runbooks/musicindex-live-publisher-deploy.md`
- `docs/runbooks/musicindex-live-publisher-arch-package.md`
- `docs/adr/0004-publisher-control-cli.md` (a dated amendment sentence)

## Do Not Touch

- `mixxx-now-playing/**`
- `src/relay.rs`, `src/schedule.rs`
- The drop-file contract
- Completed task packets and reviews in `docs/tasks/` and `docs/reviews/`

## Constraints

- The dead block values stay the same as the present default:
  - title `No V4V track playing`, no image
  - model `type = "lightning"`, `method = "lnaddress"`
  - one destination: `type = "lnaddress"`, name `No V4V payment route`,
    address `no-v4v-track@example.invalid`, split `"100"`
- The dead block has no `feedGuid` and no `itemGuid`. A client can use those
  fields to find a payee in a feed.
- Each dead block publish has a fresh `blockGuid`.
- A configuration with `[target.fallback]` fails to load. The message is:
  `ADR 0005: [target.fallback] is removed. The publisher uses a fixed dead
  block. Delete this table from target <name>.`
- An empty `value_routes` list gives the complete dead block. It does not keep
  the track title or GUIDs.
- A test that asserts the removed fallback configuration is deleted, not
  changed to pass.
- `config show` loses `fallback_configured` in text and JSON output.

## Implementation Steps

1. In `livevalue.rs`, add `pub fn dead_payload(event_guid: &str, block_guid:
   &str) -> LiveValuePayload`. Move the dead block constants from `config.rs`
   to `livevalue.rs`. Remove `fallback_payload`.
2. In `watcher.rs`, remove `FallbackConfig` and `WatchTarget.fallback`.
   Replace `fallback_payload_for_target` with a call to `dead_payload`.
3. In `payload_for_upsert`, publish `dead_payload` when
   `dropfile.value_routes` is empty. Keep the block identity rules.
4. In `config.rs`, remove the fallback parse and validation code. Return the
   error in the constraints when a target holds a `fallback` key.
5. In `main.rs`, remove `fallback_configured` from both outputs.
6. In the setup script, remove `--fallback-value-block`, `--value-block`,
   `--fallback-address` and the warning about them. Delete the example TOML
   file and the lines that install or name it in `PKGBUILD` and the
   `.install` file.
7. Change the documents:
   - `AGENTS.md` §4: replace "When playback clears, publish the fallback" with
     "When no payable block plays, publish the dead block (ADR 0005)". Add:
     "The dead block is a constant. No configuration changes it."
   - `README.md` and the three runbooks: remove the fallback configuration and
     the setup options. Say that the publisher uses a fixed dead block.
   - ADR 0004: add a dated sentence that says `fallback_configured` is removed
     by ADR 0005.
8. Add tests:
   - `dead_payload` has the values in the constraints, no GUIDs, and a split
     string `"100"`.
   - Two `dead_payload` calls from the watcher give two different
     `blockGuid` values.
   - A drop file with `"value_routes": []` publishes the dead block.
   - A drop file removal publishes the dead block.
   - A configuration with `[target.fallback]` fails, and the message contains
     `ADR 0005`.
   - `config show --json` has no `fallback_configured` key.

## Acceptance Criteria

- Each test in step 8 exists and passes.
- `grep -rn "FallbackConfig\|fallback_payload\|fallback_configured" src tests`
  finds nothing.
- `grep -rn "fallback-value-block\|fallback-address" scripts packaging README.md
  docs/runbooks` finds nothing.
- `AGENTS.md` §4 names the dead block and ADR 0005.
- No publish path can produce a payload with zero destinations. A test proves
  this for the empty-route drop file.

## Test Commands

```bash
cargo fmt --all -- --check
cargo build --workspace
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
bash -n scripts/setup-mixxx-musicindex.sh
```

## Escalation Triggers

Stop and report if one of these occurs:

- A golden fixture in `tests/fixtures/` holds a fallback payload that the dead
  block changes.
- `target add` or another command writes `[target.fallback]`.
- The setup script needs a fallback value for a reason that this packet does
  not name.
- You find a caller outside this repository that reads `fallback_configured`.

## Prompt for lower-context coding model

You are implementing one bounded task from a larger plan.

Implement only this task. Do not redesign the architecture.

Read:
- docs/adr/0005-producer-liveness-and-dead-block.md
- docs/tasks/relay-lease-task-002-dead-block.md
- src/livevalue.rs
- src/watcher.rs
- src/config.rs
- src/main.rs
- src/lib.rs
- tests/config.rs, tests/watcher.rs, tests/relay.rs, tests/golden.rs
- scripts/setup-mixxx-musicindex.sh
- packaging/arch/PKGBUILD, packaging/arch/musicindex-live-publisher.install
- /home/citizen/.agents/skills/asd-ste100/SKILL.md (for all document prose)

Goal:
- Replace the configured fallback with one constant dead block from `dead_payload` in `src/livevalue.rs`.
- An empty `value_routes` list and a drop-file removal publish the dead block.
- `[target.fallback]` becomes a load error that names ADR 0005.

Constraints:
- Keep the present dead block values: title "No V4V track playing", no image, model lightning/lnaddress, one lnaddress destination "No V4V payment route" at no-v4v-track@example.invalid with split "100".
- The dead block has no feedGuid and no itemGuid. Each publish gets a fresh blockGuid.
- The load error text is: "ADR 0005: [target.fallback] is removed. The publisher uses a fixed dead block. Delete this table from target <name>."
- Delete tests that assert the removed fallback configuration. Do not change them to pass.
- Remove `fallback_configured` from `config show` text and JSON.
- Remove the setup options `--fallback-value-block`, `--value-block` and `--fallback-address`, and the example TOML with its packaging lines.
- Update AGENTS.md §4, README.md, the three runbooks and ADR 0004 as step 7 of the task file states.

Do not touch:
- mixxx-now-playing/**
- src/relay.rs, src/schedule.rs
- The drop-file contract
- Completed task packets and reviews

Acceptance criteria:
- Tests: dead_payload values and no GUIDs; two dead blocks get different blockGuids; empty value_routes publishes the dead block; removal publishes the dead block; [target.fallback] fails with "ADR 0005"; config show --json has no fallback_configured.
- `grep -rn "FallbackConfig\|fallback_payload\|fallback_configured" src tests` finds nothing.
- `grep -rn "fallback-value-block\|fallback-address" scripts packaging README.md docs/runbooks` finds nothing.
- AGENTS.md §4 names the dead block and ADR 0005.

Test commands:
- cargo fmt --all -- --check
- cargo build --workspace
- cargo test --workspace
- cargo clippy --workspace --all-targets -- -D warnings
- bash -n scripts/setup-mixxx-musicindex.sh

At the end, report:
1. files changed
2. tests run
3. behavior changed
4. deviations from task
5. unresolved concerns
