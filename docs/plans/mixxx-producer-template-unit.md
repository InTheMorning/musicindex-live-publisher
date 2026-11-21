# A Template Unit For The Mixxx Producer: Phase Plan

Status: Planned 2026-11-21. The operator accepted ADR 0021 on 2026-11-21. This plan does not make rules. ADR 0021 owns them.

## Goal

- `mixxx-now-playing --instance NAME` finds its paths, its target and its display directory from the instance name
  and the instance configuration.
- One Mixxx producer runs at a time in a user session. A second one exits with status 3.
- The package installs `mixxx-now-playing@.service` and no longer installs `mixxx-now-playing.service`.
- `setup-mixxx-musicindex` has the interface of ADR 0021 §The Setup Helper and the migration of ADR 0021
  §Migration. It writes no unit file.

## Non-Goals

- No simulcast with one Mixxx producer (ADR 0021 Open Question 1).
- No command to remove an instance (ADR 0021 Open Question 2).
- No change to the publisher binary, the drop file or the payload.
- No change in HitIt. HitIt has its own ADR and tasks.

## Order

| Phase | Task | Depends on |
| --- | --- | --- |
| 1 | `docs/tasks/producer-template-task-001-instance-and-mixxx-lock.md` | ADR 0021 |
| 2 | `docs/tasks/producer-template-task-002-template-unit-and-packaging.md` | Task 001 |
| 3 | `docs/tasks/producer-template-task-003-setup-helper-and-migration.md` | Task 002 |

Tasks 001 and 003 change different files. Task 003 needs the unit names of task 002. Task 002 is small, so the
order stays linear.

## Gate

Each task ends with the gate of `AGENTS.md`: `cargo build`, `cargo test --workspace`,
`cargo clippy --workspace --all-targets -- -D warnings` and `cargo fmt --all -- --check`. The main session runs the
gate, reads the diff and makes the commit. Before each commit, the main session examines the diff for host names,
home paths, `/run/user/` numbers, event IDs and tokens.

## Visual And Manual Checks

These checks need a person and a real Mixxx session. They stay open until the operator runs them:

1. `setup-mixxx-musicindex --migrate-units` on the computer of the private stream. The private stream then plays a
   track to its event as before.
2. A switch from `mixxx` to a show instance and back, with Mixxx playing. The private event gets the dead block
   during the show. The show event gets no track after the switch back.

## Review

The last review of each diff before its commit uses the criteria of its task file.
