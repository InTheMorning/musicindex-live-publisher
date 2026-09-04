# Task 001 — Crate scaffold, drop-file contract, and ADR

Part of [MusicIndex live publisher plan](../plans/musicindex-live-publisher-plan.md), Phase 1.

## Goal

Create the `musicindex-live-publisher` crate, define and document the drop-file
contract, and implement parsing for it. No transform, no watching, no network.

## Files To Inspect

- `docs/plans/musicindex-live-publisher-plan.md` — Input: the drop-file contract
- `~/build/v4vmm/src/api.rs:238` — `PaymentRoute`, whose field names the contract reuses
- `~/build/splitkit/README.md` — relay API, for context on what this feeds

## Files Likely To Change

- `Cargo.toml` (new)
- `src/lib.rs` (new)
- `src/dropfile.rs` (new)
- `docs/adr/0002-nowplaying-drop-file-contract.md` (new)

## Do Not Touch

- `mixxx-now-playing/` — a separate crate, currently in flight
- `~/build/v4vmm/` and `~/build/splitkit/` — read only, never edit
- The repository root. **Do not create a Cargo workspace.** Converting would move
  `mixxx-now-playing/Cargo.lock` to the root and conflict with in-flight work.

## Constraints

- Rust 2024 edition. `anyhow::Result` at fallible boundaries.
- No `unwrap()` or `expect()` outside `#[cfg(test)]`.
- `cargo clippy -- -D warnings` clean.
- `serde` field names must match `PaymentRoute` exactly — `recipient_name`,
  `route_type`, `address`, `split`, `fee`, `custom_key`, `custom_value` — so
  producers can serialise their existing type with no translation layer.
- A file whose `schema` is unrecognised is **logged and ignored**, never guessed
  at. Same for malformed JSON. Neither is fatal to the process.

## Implementation Steps

1. Create the crate at `musicindex-live-publisher/` as a sibling of
   `mixxx-now-playing/`. Add only the dependencies the plan lists.
2. Define the drop-file types:

   ```rust
   struct DropFile {
       schema: String,            // "musicindex.nowplaying/1"
       target: String,            // publish target name; "default" today
       artist: String,
       title: String,
       duration_secs: Option<f64>,
       image: Option<String>,
       feed_guid: Option<String>,
       track_guid: Option<String>,
       value_routes: Vec<PaymentRoute>,
       value_routes_source: Option<String>,
   }
   ```

3. `parse(bytes) -> Result<Option<DropFile>>`: `Ok(None)` for an unrecognised
   schema version, `Err` only for unreadable input. Accept exactly
   `musicindex.nowplaying/1`.
4. Write ADR 0002 documenting the contract: the schema string, every field, the
   presence-is-the-signal rule, and the requirement that producers write to a
   temp file in the same directory then `rename`. State that the contract is
   owned by this project so producers depend on it rather than on each other.
5. Add the ADR to `docs/README.md` under ADRs.

## Acceptance Criteria

- `cargo build` succeeds; `cargo clippy -- -D warnings` clean.
- A well-formed drop file round-trips through `serde` unchanged.
- `musicindex.nowplaying/2` parses to `Ok(None)`, not an error.
- Malformed JSON returns `Err` without panicking.
- Missing optional fields (`image`, `duration_secs`, guids) parse fine.
- An empty `value_routes` array parses fine — it is a valid state.
- ADR 0002 exists and is linked from `docs/README.md`.

## Test Commands

```bash
cd musicindex-live-publisher
cargo fmt --check
cargo clippy -- -D warnings
cargo test dropfile
```

## Expected Final Report

- Files created, with line counts
- The final `DropFile` struct definition
- Test names and results
- Confirmation no workspace file was created at the repository root

## Escalation Triggers

Stop and ask before proceeding if:

- The plan's dependency versions do not resolve.
- The contract appears to need a field that `mixxx-now-playing` cannot produce —
  raise it rather than inventing a fallback.
- Any step seems to require reading `mixxx-now-playing`'s source to know the
  schema. The contract is defined here and that project aligns to it, not the
  other way round.
