# Reserved Safety Task 002: `target add --replace` Keeps The Fields It Was Not Given

Status: Implemented - 2026-10-04.

Every criterion is mechanical.

## Goal

`target add --replace` changes only the fields that its flags name. It keeps
every other line of the stanza, for example `display_dir`. `config show
--json` and `target list --json` show `display_dir`. A control surface then
sees the whole target before it changes one.

## Context

`render_target_config_stanza` in `src/config.rs` writes a stanza from
`TargetConfigEdit`: `name`, `event_id`, `token_file` and
`stream_delay_secs`. With `--replace`, `add_target_to_config_text` replaces
the whole stanza with that text. Thus `display_dir` (ADR 0008) and a
`stream_delay_secs` that the caller did not give are lost.

`v4vmm` calls `target add --replace` from its Attach button. The review of
2026-10-04 found that one click removes `display_dir` from the Mixxx target.
See `docs/plans/reserved-event-safety.md`.

ADR 0004 §Invariants says: "A target command preserves unrelated
configuration content." This task makes `--replace` follow that rule for the
lines in the stanza.

## Files To Inspect

- `docs/adr/0004-publisher-control-cli.md`
- `docs/adr/0008-display-path.md` (§The Publisher)
- `src/config.rs` (`TargetConfigEdit`, `add_target_to_config_text`,
  `render_target_config_stanza`, `target_stanza_named`,
  `RedactedPublisherTarget`)
- `src/main.rs` (`target add`, `target list`, `config show`)
- `tests/config.rs`
- `docs/runbooks/musicindex-live-publisher-configuration.md` (Target
  management)

## Files Likely To Change

- `src/config.rs`
- `src/main.rs`
- `tests/config.rs`
- `docs/adr/0004-publisher-control-cli.md` (one dated amendment sentence)
- `docs/runbooks/musicindex-live-publisher-configuration.md`

## Do Not Touch

- The exit codes of ADR 0004
- `mixxx-now-playing/**`
- The relay code and the payload code

## Constraints

- With `--replace`, keep the existing stanza. Change the value of `event_id`
  and `token_file`. Change `stream_delay_secs` only when the flag is given.
  Keep every other key, comment and blank line of the stanza in its order.
- Without `--replace`, the command does not change.
- `target add` and `target add --replace` have no `--display-dir` flag in
  this task.
- `config show --json` and `target list --json` add the key `display_dir` to
  each target, with the path or `null`. The other keys do not change. ADR
  0004 allows an added key.
- Add one dated sentence to ADR 0004 under its status: `--replace` changes
  only the fields that its flags name, and it keeps the other lines of the
  stanza. This follows §Invariants. No decision changes.
- The token never appears in an output.

## Implementation Steps

1. Change the replace path so it edits the existing stanza.
2. Add `display_dir` to the redacted target.
3. Add the tests.
4. Add the ADR 0004 sentence and update the runbook.

## Acceptance Criteria

Each item is a test in `tests/config.rs`:

- `--replace` on a stanza with `display_dir` and a comment keeps both, and
  changes `event_id` and `token_file`.
- `--replace` with no `--stream-delay-secs` keeps the present delay.
  `--replace` with the flag changes it.
- `--replace` keeps the other targets and the top-level keys unchanged, byte
  for byte.
- `config show --json` and `target list --json` give `display_dir` for a
  target with the key, and `null` for a target without it.
- Each present test of `target add` passes with no change.
- Each rule was broken on purpose, and a test failed. The report lists each
  mutation.

Also: the full gate passes. `Cargo.lock` does not change.

## Test Commands

```bash
cargo fmt --all -- --check
cargo build --workspace
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
```

## Escalation Triggers

- A present test asserts that `--replace` removes a key.
- A key of the stanza is on more than one line, and the edit cannot keep it.

## Review Result

Reviewed 2026-10-04. The change applies cleanly on master after task 001, and
the full gate passes there. `Cargo.lock` did not change.

`--replace` edits the lines of the stanza in place. Before the write, the
command parses the result again. The edited target must hold the new values
and its old `display_dir`, and the rest of the file must not change. Else the
file stays as it was.

`v4vmm` decodes `target list --json` with a plain `Deserialize` and no
`deny_unknown_fields`. The new key `display_dir` thus does not break it.

The review accepts these changes to present tests:

- Two tests assert the exact key set of a target in `config show --json`.
  This task changes that key set, so each test now lists `display_dir`.
- Three struct literals in `src/main.rs` tests get `display_dir: None`.

Open items:

- A quoted key, for example `"event_id" = …`, gives an error, and the file
  does not change.
- The text output of `target list` and `config show` does not show
  `display_dir`.
