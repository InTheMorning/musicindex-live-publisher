# Liquidsoap Task 005: A Payable Block Needs The Two GUIDs

Status: Ready - 2026-11-21. It starts after tasks 001 and 003.

Every criterion is mechanical.

## Goal

The publisher publishes routes only from a drop file with a feed GUID and a track GUID. The Mixxx producer asks the
route service only with the two GUIDs (ADR 0024).

## Base components

| Component | Status | Location | Reason |
| --- | --- | --- | --- |
| Payload assembler | extend | src/livevalue.rs | Add the GUID condition where a drop file becomes a payable payload or the dead block. |
| Route request | extend | mixxx-now-playing/src/musicindex.rs | `RouteRequest::has_lookup_key` needs the two GUIDs. |
| Payment rules in AGENTS.md | extend | AGENTS.md | Add the rule to §4 with its test. |

## Files To Inspect

- `docs/adr/0024-payable-block-needs-both-guids.md`
- `src/livevalue.rs`, `src/watcher.rs` (where the publisher selects the dead block for a drop file)
- `mixxx-now-playing/src/musicindex.rs`
- `AGENTS.md` §4

## Files Likely To Change

- `src/livevalue.rs` or `src/watcher.rs`, at the one place that selects the dead block
- `mixxx-now-playing/src/musicindex.rs`
- `AGENTS.md` §4, one bullet
- Tests in `src/` or `tests/`

## Do Not Touch

- `liquidsoap-now-playing/**`, `now-playing-drop/**`, `systemd/**`, `packaging/**`, `docs/**`.
- The drop file schema. It stays version 3.

## Constraints

- A drop file with routes, and a `feed_guid` or a `track_guid` that is `null` or an empty string, gives the dead
  block. Write one `tracing::warn!` with the target name and the name of the missing field.
- The show log entry for that file still records the track, as the drop file gives it.
- `has_lookup_key` is true only when the two GUIDs are present and not empty. Embedded routes without the two GUIDs
  give no routes.
- The new `AGENTS.md` §4 bullet: "A block pays its routes only when the drop file has a feed GUID and a track GUID
  (ADR 0024). A track GUID is unique only in its feed." Write it in Simplified Technical English.
- Write comments in Simplified Technical English (`/home/citizen/.agents/skills/asd-ste100/SKILL.md`). Follow the
  `rust-dev` skill.
- Do not commit. Leave the change in the working tree. The main session reads the diff and commits.

## Acceptance Criteria

All are mechanical:

1. Tests: routes with `feed_guid` `null`, routes with `track_guid` `null` and routes with an empty GUID each give the
   dead block.
2. Test: routes with the two GUIDs give the routes.
3. Test: a Mixxx route request with one GUID does not go to the route service.
4. `cargo test --workspace`, `cargo clippy --workspace --all-targets -- -D warnings` and
   `cargo fmt --all -- --check` pass.
