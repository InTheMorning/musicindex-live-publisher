# Publisher Control CLI Review Checklist

## Scope

Use this checklist after both control-surface task packets land.

Reviewed work:

- `docs/adr/0004-publisher-control-cli.md`
- `docs/plans/publisher-control-cli-phase-plan.md`
- `docs/tasks/control-surface-task-001-target-management.md`
- `docs/tasks/control-surface-task-002-machine-readable-cli.md`
- The implementation diff for both packets

## Required Checks

- The implementation follows ADR 0004.
- The implementation follows both task packets.
- The control CLI never sends live metadata to the relay.
- The control CLI never starts, stops, resets, or reloads a service.
- No command accepts token content as an argument.
- No command prints token content.
- Target edits preserve unrelated configuration content.
- JSON output is valid when requested.
- Human output remains the default.
- Duplicate and missing targets have distinct exit codes.
- No code path depends on `v4vmm`.
- Documentation names the new commands and their secret-handling rules.

## Test Commands

- `cargo fmt --all -- --check`
- `cargo check --workspace --quiet`
- `cargo test --workspace --quiet`
- `cargo clippy --workspace --quiet -- -D warnings`

## Review Result

Status: Pending

Required fixes:

- Pending review.

Optional improvements:

- Pending review.

Merge recommendation:

- Pending review.
