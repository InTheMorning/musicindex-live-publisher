# A Producer For Liquidsoap And A DJ Takeover: Phase Plan

Status: Ready 2026-11-21. The operator accepted ADR 0022, ADR 0023 and ADR 0024 on 2026-11-21. This plan does not make rules. ADR 0022, ADR 0023 and ADR 0024 own them.

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
| 5 | The two GUIDs of ADR 0024 in the publisher and in the Mixxx route request, with the line in `AGENTS.md` §4. | ADR 0024 |

Packet 2 applies ADR 0024 in the new producer. Packet 5 changes `src/livevalue.rs` and
`mixxx-now-playing/src/musicindex.rs`. Packet 3 changes only `src/`. It can run at the same time as packet 1 or packet 2 in a different worktree.

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
- The publisher and the Mixxx producer have a test for each case of ADR 0024 §Verification.

## Manual Checks

These checks need a test station and the operator. Report each one as open until the operator does it.

Results on 2026-11-22 (UTC), bundle `bdda4bf`, instance `heycitizens-test-radio`. The orchestrator session ran the
checks on the test host and reported them:

- Check 1: passed. `state.json` holds routes of 671 bytes, not cut.
- Check 2: passed. Liquidsoap needed approximately 60 seconds after the reboot. The relay had the dead block in that
  time.
- Check 3: passed. The dead block came 5.1 seconds after the stop.
- Check 4, station side: passed. At the DJ connection, the relay had the dead block in approximately 2 seconds. At
  the disconnection, the next AutoDJ track came on the relay. The part with a DJ publisher and the read-back of ADR 0023
  is open.
- On the same day the host had no free memory, and the kernel stopped the user manager with the two units. Nothing
  started it again. The runbook now has §Restart The User Manager. Future item 2 records the alternative.

1. The reference slot text passes the save in AzuraCast and writes `state.json` in the bind mount.
2. The runbook steps install the two units for one account with lingering. The two units start after a reboot.
3. A stop of Liquidsoap gives the dead block on the relay in approximately 6 seconds.
4. The two sequences at the start of a DJ set and the two at the end each end with the correct snapshot.

## Future Items

These items are not packets of this plan. Each one needs its own task, and a decision record when it meets the bar.

1. **A lock state command.** Operator decision of 2026-11-21, relayed by the orchestrator session: a
   command tells if a producer holds `.producer.lock` of an instance, and gives the result as JSON. HitIt ADR 0112
   (Proposed, HitIt commit `3e61236`) can then make sure that the station producer released its lock before "Go on air"
   of a DJ set. Until the command exists, the HitIt confirmation text is the only guard for the first risk in
   ADR 0023 §Risks. The command is a new public interface between repositories, so it needs a decision record.
2. **System units on a station host.** Open for the operator. ADR 0022 uses user units of one account with lingering.
   On a station host, a system unit does not need the user manager, so a failure of the user manager cannot stop it.
   This change replaces ADR 0022 §The Template Unit and the paths of the runbook, so it needs a decision record. Until
   then, the runbook gives a restart drop-in for `user@UID.service`.
