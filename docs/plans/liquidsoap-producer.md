# A Producer For Liquidsoap And A DJ Takeover: Phase Plan

Status: Proposed 2026-11-21. It waits for the acceptance of ADR 0022 and ADR 0023. The task packets come after the
acceptance. This plan does not make rules. ADR 0022 and ADR 0023 own them.

## Goal

- `liquidsoap-now-playing` reads `state.json` of the format `musicindex-liquidsoap-state/1` and writes the drop file
  version 3 (ADR 0022).
- The two producers use one crate for the producer lock, the drop file writer and the instance configuration.
- Each publisher sends its payable block again over a foreign dead block (ADR 0023).
- The package installs `liquidsoap-now-playing@.service`, and a runbook gives the server install with one account and
  lingering.

## Non-Goals

- No display path for the Liquidsoap producer. The drop file holds the `image` URL. ADR 0008 stays for Mixxx.
- No change to the drop file, the payload or the relay.
- No deployment to a server. The operator controls each server change.
- No setup helper for a server. The runbook gives the commands. A helper can come after the first install.

## Base Components

| Component | Status | Location | Reason |
| --- | --- | --- | --- |
| Producer lock | extend | `mixxx-now-playing/src/lock.rs` | `ProducerLock` moves to `now-playing-drop`. `MixxxLock` stays. |
| Drop file writer | extend | `mixxx-now-playing/src/sink.rs`, `mixxx-now-playing/src/render.rs` | `OutputFile` and the version 3 type move to `now-playing-drop`. The writer takes plain fields, not Mixxx tags. |
| Instance configuration read | extend | `mixxx-now-playing/src/config.rs` | The `--instance` read of ADR 0021 moves to `now-playing-drop`. |
| Payment route type | extend | `mixxx-now-playing/src/musicindex.rs` | `PaymentRoute` moves with the writer. |
| Relay client | extend | `src/relay.rs` | It gets the read of the metadata route. |
| Dead block test | extend | `src/livevalue.rs` | A test for a dead block, adjacent to `dead_payload`. |
| Template unit | extend | `systemd/mixxx-now-playing@.service` | The new unit follows its pairing lines. |
| Split arithmetic | reuse | `~/build/musicindex-lib/value` | Not needed. The producer passes routes and does no arithmetic. Examined and not used. |

## Order

| Phase | Packet | Depends on |
| --- | --- | --- |
| 1 | Move the shared producer parts into `now-playing-drop`. No change of behavior. | ADR 0022 |
| 2 | `liquidsoap-now-playing`: state file parse, heartbeat, `live`, lock, drop file. | Packet 1 |
| 3 | The publisher read-back of ADR 0023 §The Publisher Restores Its Block. | ADR 0023 |
| 4 | `liquidsoap/musicindex-state.liq`, the template unit, the `.deb` assets, `packaging/arch/PKGBUILD` and the runbook. | Packet 2 |

Packet 3 changes only `src/`. It can run at the same time as packet 1 or packet 2 in a different worktree.

## Gate

Each packet ends with the gate of `AGENTS.md`: `cargo build`, `cargo test --workspace`,
`cargo clippy --workspace --all-targets -- -D warnings` and `cargo fmt --all -- --check`. The main session runs the
gate, reads the diff and makes the commit.

## Mechanical Criteria

- The producer crate has a test for each row of ADR 0022 §What The Producer Writes.
- The producer crate has a test for an unknown `format`, for bad `value_routes` JSON and for a stale heartbeat. The
  tests use an injected clock.
- The Mixxx producer tests pass with no change of their assertions after packet 1.
- `src/relay.rs` has a test for each case of ADR 0023 §Verification.

## Manual Checks

These checks need a test station and the operator. Report each one as open until the operator does it.

1. The reference slot text passes the save in AzuraCast and writes `state.json` in the bind mount.
2. The runbook steps install the two units for one account with lingering. The two units start after a reboot.
3. A stop of Liquidsoap gives the dead block on the relay in approximately 6 seconds.
4. The two sequences at the start of a DJ set and the two at the end each end with the correct snapshot.
