# Task 006 — Systemd deployment and producer alignment

Part of [MusicIndex live publisher plan](../plans/musicindex-live-publisher-plan.md), Phase 4.

## Goal

Ship the service: a hardened systemd unit, and alignment of
`mixxx-now-playing`'s JSON output with the drop-file contract so the two connect
end to end.

## Files To Inspect

- `docs/plans/musicindex-live-publisher-plan.md` — Service hardening, Phase 4
- `docs/adr/0002-nowplaying-drop-file-contract.md` — task 001's contract
- `~/build/splitkit/systemd/musicindex-live-relay.service` — the hardening template
- `mixxx-now-playing/src/render.rs` — its JSON output path

## Files Likely To Change

- `systemd/musicindex-live-publisher.service` (new)
- `README.md` (new)
- `mixxx-now-playing/src/render.rs`
- `docs/runbooks/musicindex-live-publisher-deploy.md` (new)

## Do Not Touch

- `mixxx-now-playing`'s **text** output format. Only its JSON path changes.
  The text format is a separate consumer contract.
- The relay's own unit file

## Constraints

- Model the unit on the relay's: `NoNewPrivileges=true`, `ProtectSystem=strict`,
  `ProtectHome=true`, `PrivateTmp=true`, `Restart=on-failure`, `RestartSec=5s`.
- Add `ReadWritePaths=` for the watch directory, since `ProtectSystem=strict`
  makes the filesystem read-only otherwise.
- Load the token with `LoadCredential=` so it is not readable process-wide.
  Update task 004's `token_file` resolution to accept `%d/`-style credential
  paths if needed.
- `mixxx-now-playing --format json` must emit exactly the drop-file schema. Do
  not invent a second schema or a translation shim between the two projects.
- **`PrivateTmp=true` isolates `/tmp`.** If the watch directory sits under `/tmp`
  the service cannot see the producer's files. Use a
  `RuntimeDirectory=` path instead, and say so in the runbook.

## Implementation Steps

1. Write the unit with the hardening above, `RuntimeDirectory=` for the watch
   directory, and `LoadCredential=` for the token.
2. Align `mixxx-now-playing`'s JSON output to `musicindex.nowplaying/1`: emit
   `schema`, `target`, `artist`, `title`, `duration_secs`, `image`, `feed_guid`,
   `track_guid`, `value_routes`, `value_routes_source`. Add a `--target` flag,
   defaulting to `default`.
3. Confirm `mixxx-now-playing` already writes via temp-file-plus-rename in the
   same directory, as the contract requires. Fix if not.
4. Write the runbook: provisioning a live item, installing both units, the
   `RuntimeDirectory` path, verifying with `--dry-run`, and re-provisioning after
   a lost token.
5. Write the crate README: what it does, config reference, the `provision` flow.
6. Add the runbook to `docs/README.md`.

## Acceptance Criteria

- `systemd-analyze verify` passes on the unit.
- The service starts under systemd, sees files written by `mixxx-now-playing`
  into the runtime directory, and publishes to a locally built relay.
- A JSON file produced by `mixxx-now-playing --format json` parses with task
  001's `parse` and round-trips through the task 002 transform.
- Stopping the track removes the drop file and the fallback payload is published.
- The token is not readable from the service's environment or `/proc`.
- Runbook and README exist; runbook is linked from `docs/README.md`.

## Test Commands

```bash
systemd-analyze verify systemd/musicindex-live-publisher.service
cargo test -p mixxx-now-playing render
cargo test -p musicindex-live-publisher
```

## Expected Final Report

- Files created or changed in **both** crates
- The final unit file, pasted
- End-to-end evidence: track plays, drop file appears, payload reaches the relay,
  track stops, fallback published
- Any drop-file schema change needed to fit `mixxx-now-playing`'s real output, and
  whether ADR 0002 was updated to match

## Escalation Triggers

Stop and ask before proceeding if:

- Aligning `mixxx-now-playing` would break its text output or its existing tests.
- `ProtectSystem=strict` blocks a path the service genuinely needs beyond the
  watch directory.
- The two projects disagree on a field's meaning — update ADR 0002 deliberately
  rather than making the producer and consumer diverge.
