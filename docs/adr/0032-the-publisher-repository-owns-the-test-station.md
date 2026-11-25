# ADR 0032: The Publisher Repository Owns The Test Station Operation

- Status: Accepted on 2026-11-25 by the operator. The orchestrator session relayed the decision in musicindex-research
  commit dd3d547, `research_notes/azuracast/station-operations-brief.md`.
- Date: 2026-11-25
- Bar: crosses repositories. The station steps, the station log and the handoffs move from musicindex-research to this
  repository. The orchestrator, CitizenRadio and the relay sessions send and get the station work through these rules.
- Operator choices, 2026-11-25:
  - The publisher session owns the testing and the operation of the HeyCitizen test station (`radio.heycitizen.xyz`).
  - The test station log, the producer tutorial and `hls_id3_dump.py` move to this repository (commit 58ced30).
  - The operator does not make a repository `musicindex-station` now.

## Context

Before this record, the orchestrator session in musicindex-research installed the publisher bundles on the test
station and kept the station log. The publisher session wrote the code and the bundles. Each station check then needed
a message from one session to the other, and a fault went through two sessions before a code change.

The station runs AzuraCast. The station producer of this repository (the slot text `liquidsoap/musicindex-state.liq`
and `liquidsoap-now-playing`) is the only code of the suite that runs on the station.

## Decision

1. This repository owns the steps on the test station, the station log `docs/runbooks/test-station-log.md` and the
   station runbooks `docs/runbooks/musicindex-producer-tutorial.md` and `docs/runbooks/liquidsoap-now-playing-azuracast.md`.
2. The publisher session installs its bundles on the station and does the agent checks. It asks the operator only for
   a check that needs a person, for example a check in a listener app.
3. A fault in another repository goes to the orchestrator as a handoff. The handoff gives the step number of the
   station log as evidence. The publisher session does not send the handoff to the owning session.
4. A new repository for the station comes only when the operator approves an alternative to AzuraCast. Then the
   station producer and the station documents move to that repository in one change.

Each step on the station gets a row in the station log. The row gives the time in UTC, the command or the change, the
source and the result. The SSH key stays in `.local-keys/`, which `.git/info/exclude` ignores. No commit holds the key.

### Durable And Situational Rules

Each rule of this record is situational. It describes the test station on AzuraCast. Remove it when a record about a
station repository supersedes this record.

## Verification

Manual. The rules are about the work of sessions, so a test cannot examine them. The station log shows each step with
its result, and the orchestrator reads each handoff.

## Alternatives Considered

- **The orchestrator keeps the station.** Rejected by the operator. Each check needs two sessions.
- **A new repository `musicindex-station` now.** Rejected by the operator. It comes with an alternative to AzuraCast.

## Consequences

- The publisher session can install a bundle and examine the result without a message to another session.
- The orchestrator removes the moved files from musicindex-research and adds a link to this repository.
- The producer tutorial and `docs/runbooks/liquidsoap-now-playing-azuracast.md` have some of the same steps. A merge
  of the two is open.

## References

- musicindex-research commit dd3d547, `research_notes/azuracast/station-operations-brief.md`
- `docs/runbooks/test-station-log.md`
