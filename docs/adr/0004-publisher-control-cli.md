# ADR 0004: Publisher Control CLI

Status: Accepted
Date: 2026-09-07

## Context

`v4vmm` ADR 0059 makes `v4vmm` the control surface for the broadcast chain.
That app must create live events, attach them to publisher targets, and read
publisher state.

This repository owns its configuration file and its setup rules. No neighbor can
write that file safely, because the setup commands preserve comments, markers,
backups, and token paths.

The publisher can also run without `v4vmm`. Human operators still need the
current prose output and the current service behavior.

## Decision

Add a stable command-line control surface to this repository.

The commands manage publisher configuration and expose machine-readable state.
They do not send live payloads. They do not restart services. They do not call a
neighbor repository.

The target commands are:

- `target add`
- `target list`
- `target remove`

The machine-readable commands are:

- `provision --json`
- `config show --json`
- `--version`

The human output stays the default. JSON output is opt-in.

The configuration file remains owned by this repository. A control surface calls
these commands instead of editing the file directly.

## Invariants

- This repository is the only writer of its configuration file.
- The control CLI does not publish metadata to the relay.
- The control CLI does not restart services.
- No command accepts token content as an argument.
- No command prints token content.
- A target command preserves unrelated configuration content.
- Machine-readable output is valid JSON when the caller asks for JSON.
- Exit codes are stable enough for a caller to separate missing targets,
  duplicate targets, command failure, and success.

## Non-Goals

- No remote control API.
- No service start, stop, or reset command.
- No change to the drop-file contract.
- No change to the live value payload shape.
- No token rotation.
- No dependency on `v4vmm`.

## Alternatives Considered

### Let `v4vmm` Write The Publisher Configuration

Rejected. It would copy this repository's setup rules into another repository.
Those rules would then drift.

### Add A Remote Control API Now

Rejected. A remote API needs authentication, bind defaults, and a larger threat
model. `v4vmm` ADR 0059 uses SSH for service control now.

### Put The Token In JSON Output

Rejected. A token in stdout can reach shell history, logs, and pipes. The token
file path is enough. A caller that needs the token reads the file.

## Consequences

Positive:

- `v4vmm` can manage targets without writing this repository's configuration
  file.
- Operators keep the existing human output.
- JSON output gives callers a parser-safe contract.
- Token content stays out of arguments and stdout.

Negative and risks:

- Exit codes become part of the control contract.
- The configuration edit path must preserve comments and unrelated tables.
- A target operation can fail before the service reloads the file. The caller
  must report that command failure clearly.

## Follow-Up Work

- A future remote control API, when Liquidsoap control needs more than start and
  stop.
- A token rotation command, if reserved relay items later support it.

## References

- `docs/plans/publisher-control-cli-phase-plan.md`
- `docs/tasks/control-surface-task-001-target-management.md`
- `docs/tasks/control-surface-task-002-machine-readable-cli.md`
- `docs/architecture/broadcast-chain-boundaries.md`
- `v4vmm`: `docs/adr/0059-broadcast-control-surface.md`
