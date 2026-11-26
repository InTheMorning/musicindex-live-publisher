# ADR 0035 Task 001: Comments And Runbooks Cite The Current Record

Status: Implemented - 2026-11-26. The citation in `liquidsoap-now-playing/tests/slot_text.rs` names the incident of task 012 and stays.

Every criterion is mechanical.

## Goal

ADR 0035 supersedes ADR 0022 and ADR 0025 to ADR 0028. Those five records are now in `docs/archive/adr/`. Code
comments, test comments, the slot text and the runbooks still cite them. Change each citation to the section of
ADR 0035 that restates the same rule. Change no behavior.

## Files To Read

- `docs/adr/0035-the-station-producer.md`, all sections. Each rule of the five old records is in it.
- The five archived records in `docs/archive/adr/`, only to find the rule that a citation names.

## Files To Change

Find them with this command:

```bash
grep -rln "ADR 002[25678]" --include=*.rs --include=*.liq --include=*.sh --include=*.md . | grep -v -e '^./target/' -e '^./docs/archive/' -e '^./docs/adr/' -e '^./docs/tasks/' -e '^./docs/reviews/'
```

On 2026-11-26 the command found code in `liquidsoap-now-playing/`, `now-playing-drop/`, `liquidsoap/musicindex-state.liq`,
and files in `docs/runbooks/`.

## Do Not Touch

- `docs/adr/`, `docs/archive/`, `docs/tasks/`, `docs/reviews/`, `AGENTS.md`, `docs/README.md`.
- The station log `docs/runbooks/test-station-log.md` and the Mixxx VPS log. A log records a past step, so its
  citations stay as they are.
- Any code line that is not a comment, and any string that a test compares.

## Base components

| Component | Status | Location | Reason |
| --- | --- | --- | --- |
| ADR 0035 sections | reuse | docs/adr/0035-the-station-producer.md | Each new citation names a section of this record. |

## Rules

- Write `ADR 0035 §Section Name` with the exact heading text of ADR 0035.
- When ADR 0035 has no section for a cited rule, do not guess. Leave the citation and list it in the report.
- A citation of the form "ADR 0025 and ADR 0026" becomes one citation of ADR 0035 when one section holds both rules.
- Keep each comment as short as before. Write in Simplified Technical English. The skill is
  `/home/citizen/.agents/skills/asd-ste100/SKILL.md`.

## Acceptance Criteria (Mechanical)

1. The command in §Files To Change prints only files with a citation that the report lists as not mapped.
2. `git diff` changes only comment lines, the slot text comments and runbook prose.
3. The gate passes: `cargo build && cargo test`, `cargo clippy --all-targets -- -D warnings`, `cargo fmt -- --check`.

## Rules For The Delegate

- Do not commit. Leave the change in the working tree. Report the changed files, each citation that you did not map,
  and the gate output.
- Do not make an architecture decision. If a rule of an old record is not in ADR 0035, stop for that citation and report it.
