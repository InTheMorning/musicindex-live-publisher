# Liquidsoap Task 001: The Crate `now-playing-drop`

Status: Ready - 2026-11-21.

Every criterion is mechanical.

## Goal

Move the parts that each producer needs from `mixxx-now-playing` into a new workspace member crate,
`now-playing-drop` (ADR 0022 §Name And Crate). The Mixxx producer then uses that crate. The behavior of the Mixxx
producer does not change.

## Base components

| Component | Status | Location | Reason |
| --- | --- | --- | --- |
| Producer lock | extend | mixxx-now-playing/src/lock.rs | Move `ProducerLock`, `LOCK_WAIT_LIMIT` and the one wait function. `MixxxLock` stays and calls the moved wait function. |
| Drop file sink | extend | mixxx-now-playing/src/sink.rs | Move `OutputFile`, `Presence`, `remove_file_if_exists` and `ensure_empty_file`. |
| Drop file version 3 writer | extend | mixxx-now-playing/src/render.rs | Move the version 3 struct out of `render_metadata_json_with_routes` as a public type with plain fields. The Mixxx function builds it from its tags. |
| Payment route type | extend | mixxx-now-playing/src/musicindex.rs | Move `PaymentRoute`. |
| Instance configuration read | extend | mixxx-now-playing/src/config.rs | Move the read of ADR 0021 §The Producer Reads Its Instance: the name rule, the configuration read and the instance paths. |

## Files To Inspect

- `docs/archive/adr/0022-liquidsoap-producer.md` §Name And Crate
- `docs/adr/0016-instances-events-and-producer-lock.md` §The Producer Lock
- `docs/adr/0021-mixxx-producer-template-unit.md` §The Producer Reads Its Instance
- `mixxx-now-playing/src/lock.rs`, `sink.rs`, `render.rs`, `musicindex.rs`, `config.rs`, `lib.rs`, `main.rs`
- `mixxx-now-playing/Cargo.toml`, `Cargo.toml`

## Files Likely To Change

- `Cargo.toml`: add `now-playing-drop` to `members` and `default-members`.
- `Cargo.lock`
- New: `now-playing-drop/Cargo.toml`, `now-playing-drop/src/lib.rs` and its modules.
- `mixxx-now-playing/Cargo.toml` and the five source files above.

## Do Not Touch

- `src/**`. The publisher does not change.
- `systemd/**`, `packaging/**`, `scripts/**`, `docs/**`.
- The test assertions in `mixxx-now-playing/tests/**` and in the moved tests. Change only `use` paths.

## Constraints

- Move the code. Do not leave a copy in `mixxx-now-playing`. A `pub use` in `mixxx-now-playing` is permitted only when
  a test in `mixxx-now-playing/tests/` needs the old path.
- Move the unit tests of the moved code with the code.
- `now-playing-drop` has the edition 2024 and the license `AGPL-3.0-only`. Its dependencies are a subset of the
  current dependencies of `mixxx-now-playing`. Add no new external crate.
- The drop file text that the Mixxx producer writes does not change, byte for byte.
- Each public item and each public function that can fail has a doc comment, with `# Errors` where it applies.
- Write comments in Simplified Technical English (`/home/citizen/.agents/skills/asd-ste100/SKILL.md`). Follow the
  `rust-dev` skill.
- Do not commit. Leave the change in the working tree. The main session reads the diff and commits.

## Acceptance Criteria

All are mechanical.

1. `cargo build --workspace` passes.
2. `cargo test --workspace` passes. No assertion of a moved or existing test changes.
3. `cargo clippy --workspace --all-targets -- -D warnings` passes.
4. `cargo fmt --all -- --check` passes.
5. `grep -rn "struct ProducerLock\|struct OutputFile\|struct PaymentRoute" mixxx-now-playing/src` gives no result.
