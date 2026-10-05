# Reserved Event Safety Plan

Date: 2026-10-04. This plan states no rule. It registers four packets and
names the owner of each rule.

## Goal

A reserved relay event (relay ADR 0001) has a broadcaster token that the relay
gives one time. The event lives for a long time. No command of this
repository, and no click in `v4vmm`, may destroy its token or change its
target in the publisher configuration by accident.

## Findings

A review on 2026-10-04 found these paths in this repository:

| # | Path | Effect | Owner of the rule |
|---|---|---|---|
| 1 | `provision --token-file PATH` | It truncates an existing token file and writes in place. | `AGENTS.md` §5 and §6 |
| 2 | `target add --replace` | It writes the stanza from its flags only. It removes `display_dir`. `config show` and `target list` do not show `display_dir`. | ADR 0004 §Invariants, ADR 0008 |
| 3 | `setup-mixxx-musicindex` | It replaces a config that has its marker, with no `--force`. If the token file is missing, it provisions a new ephemeral event and rewrites both units. | ADR 0004, `docs/plans/packaging-pass.md` item 5 |
| 4 | `setup-mixxx-musicindex` | It cannot write the display output. A unit that it rewrites loses `--display-dir`. | ADR 0008, `docs/plans/packaging-pass.md` item 9 |

The `v4vmm` paths are in the request
`docs/plans/reserved-event-safety-request.md` in the `v4vmm` repository.

## Packets

Do task 001 and task 002 first. Task 004 needs task 002.

- [001 — `provision` never replaces a token](../tasks/reserved-safety-task-001-provision-token.md)
- [002 — `target add --replace` keeps the fields it was not given](../tasks/reserved-safety-task-002-target-replace.md)
- [003 — The setup helper keeps an existing event](../tasks/reserved-safety-task-003-setup-keeps-event.md)
- [004 — The setup helper writes the display output](../tasks/reserved-safety-task-004-setup-display.md)

Task 003 and task 004 change the setup helper. They close items 5 and 9 of
the packaging pass. They do not need the package build of that pass.

## Interim Rules For The Operator

These rules are advisory. They stay until the packets and the `v4vmm` request
are done.

- Keep the reserved token in a file of this repository, not in a file of the
  `v4vmm` token directory.
- Keep a copy of the reserved token in a second private file.
- Do not run `setup-mixxx-musicindex` on a computer with a reserved target.
- In `v4vmm`, do not use Attach, Retry, Replace or `broadcast events forget`
  for the Mixxx target.
