# Liquidsoap Task 002: The Producer `liquidsoap-now-playing`

Status: Ready - 2026-11-21. It starts after task 001.

Every criterion is mechanical.

## Goal

A new workspace member crate and binary, `liquidsoap-now-playing`. It reads `state.json` of the format
`musicindex-liquidsoap-state/1` and writes the drop file version 3 (ADR 0022, ADR 0024).

## Base components

| Component | Status | Location | Reason |
| --- | --- | --- | --- |
| Producer lock | reuse | mixxx-now-playing/src/lock.rs | Task 001 moves `ProducerLock` to `now-playing-drop`. Use it from there. |
| Drop file writer and sink | reuse | mixxx-now-playing/src/sink.rs | Task 001 moves `OutputFile` and the version 3 type to `now-playing-drop`. Use them from there. |
| Instance configuration read | reuse | mixxx-now-playing/src/config.rs | Task 001 moves the `--instance` read to `now-playing-drop`. Use it from there. |
| Signal handling | reuse | mixxx-now-playing/src/main.rs | Use `signal_hook::flag::register` for `SIGTERM` and `SIGINT`, as the Mixxx producer does. |
| Command line parser | reuse | mixxx-now-playing/src/cli.rs | Use the same hand-written parser style. Add no parser crate. |
| State file reader | new | liquidsoap-now-playing/src/state.rs | Searched mixxx-now-playing/src, now-playing-drop and ~/build/musicindex-lib. No reader of this format exists. |

## Files To Inspect

- `docs/archive/adr/0022-liquidsoap-producer.md`, all sections
- `docs/adr/0024-payable-block-needs-both-guids.md`
- `docs/adr/0016-instances-events-and-producer-lock.md` §The Producer Lock
- `docs/adr/0014-one-assembler-and-track-reporting.md` §Each Producer Reports Each Track
- `now-playing-drop/src/**`
- `mixxx-now-playing/src/main.rs` and `cli.rs` for the style of the loop, the signals and the parser

## Files Likely To Change

- `Cargo.toml`: add `liquidsoap-now-playing` to `members` and `default-members`.
- `Cargo.lock`
- New: `liquidsoap-now-playing/Cargo.toml`, `liquidsoap-now-playing/src/main.rs`, `src/lib.rs`, `src/state.rs`,
  `src/cli.rs` and tests.

## Do Not Touch

- `src/**`, `mixxx-now-playing/**`, `now-playing-drop/**` except a bug fix that the main session approves.
- `systemd/**`, `packaging/**`, `scripts/**`, `docs/**`.

## Constraints

- Command line: `liquidsoap-now-playing --instance NAME --state-file PATH`. The two flags are necessary. Also
  `--version` and `--help`. An unknown flag gives an error.
- The producer reads the state file each 500 milliseconds.
- **Fresh** means that `written` changed in the last 5 seconds of the monotonic clock. At start, the state is not fresh
  until `written` changes one time. Use an injected clock, so that tests do not sleep for 5 seconds.
- The states and the actions are the table of ADR 0022 §What The Producer Writes. Apply each row:
  - Not fresh, missing or not readable: no drop file, lock not held.
  - Fresh and `live` is `true`: no drop file, lock not held.
  - Fresh, `live` is `false`, `jingle` is `true`: lock held. Drop file with empty `artist`, `title` and
    `value_routes`. Each other field is `null`.
  - Fresh, `live` is `false`, `feed_guid`, `track_guid` and routes present: lock held. Drop file with the routes.
  - Fresh, `live` is `false`, other cases: lock held. Drop file with empty `value_routes`.
- Remove the drop file before the lock is released. Take the lock with the wait limit of `now-playing-drop`. A lock
  that is not free after the limit stops the process with an error that names the drop directory.
- A state file that does not parse as JSON keeps the current drop file and the current lock. A file with a different
  `format` gives one `error` log line for each change of the value, and the row "not readable".
- `value_routes` is a JSON string. Parse it a second time as a list of `PaymentRoute`. If it does not parse, use an
  empty list and write an `error` log line.
- An empty string is `null` in the drop file. `duration_secs` is `null`.
- `play_id` is `format!("ls-{}-{}", (track_started * 1000.0) as i64, seq)`.
- `value_routes_source` is `"liquidsoap-state"` when the routes come from the state file, else `null`.
- The drop file path and the target come from the instance configuration, through `now-playing-drop`.
- Do not rewrite a drop file with the same content. `OutputFile` does this.
- At `SIGTERM` or `SIGINT`, remove the drop file and exit with status 0.
- The producer uses no network.
- Write comments in Simplified Technical English (`/home/citizen/.agents/skills/asd-ste100/SKILL.md`). Follow the
  `rust-dev` skill.
- Do not commit. Leave the change in the working tree. The main session reads the diff and commits.

## Acceptance Criteria

All are mechanical. Each case is a test with `tempfile` and the injected clock:

1. Each of the five rows above gives the expected drop file and lock state.
2. A stale heartbeat after 5 seconds removes the drop file and releases the lock.
3. A file that does not parse keeps the current drop file.
4. An unknown `format` gives no drop file.
5. `value_routes` that is not valid JSON gives empty routes.
6. A routes file with no `feed_guid` gives empty routes (ADR 0024).
7. A change from `live` `true` to `false` takes the lock again and writes the drop file.
8. `cargo build --workspace`, `cargo test --workspace`, `cargo clippy --workspace --all-targets -- -D warnings` and
   `cargo fmt --all -- --check` pass.
