# Reserved Safety Task 001: `provision` Never Replaces A Token

Status: Ready - 2026-10-04.

Every criterion is mechanical.

## Goal

`provision --token-file PATH` refuses a path where a file exists. It writes a
new token file through a temporary file and a rename. A token that the relay
gave one time can then never be lost by a second `provision`.

## Context

`write_token_file` in `src/relay.rs` opens the path with `create(true)` and
`truncate(true)`. A `provision` to an existing token file thus deletes the
token in it before the relay answers. The write is also in place.

`AGENTS.md` §5 says that a token file is irreplaceable. `AGENTS.md` §6 says
that each output file goes through a temporary file and a rename. The review
of 2026-10-04 found the defect. See `docs/plans/reserved-event-safety.md`.

## Files To Inspect

- `src/relay.rs` (`write_token_file`, `RelayClient::provision`)
- `src/main.rs` (`provision`, the command parse)
- `docs/adr/0004-publisher-control-cli.md` (`provision --json`, exit codes)
- `docs/runbooks/musicindex-live-publisher-configuration.md` (Provision mode)
- `scripts/setup-mixxx-musicindex.sh` (it moves the old token before
  `provision`)

## Files Likely To Change

- `src/relay.rs`
- `src/main.rs`
- `tests/relay.rs` or a new test file
- `docs/runbooks/musicindex-live-publisher-configuration.md`

## Do Not Touch

- The relay request of `provision` and its JSON output fields
- `mixxx-now-playing/**`
- `docs/adr/**`

## Constraints

- Check the path before the relay request. Stop if a file, a directory or a
  symbolic link exists at the path. The error names the path. It also says
  that the command sent no relay request.
- The check before the request does not replace the safe write.
- Write the token to a temporary file in the same directory, with mode
  `0600`. Then link or rename it into place so that an existing path is never
  replaced. Use a call that fails when the target exists. One example is `hard_link`,
  then `remove_file` of the temporary file. Another is `renameat2` with
  `RENAME_NOREPLACE`. A plain `rename` replaces the target, so do not use it.
- The final step can fail because a file appeared. Then print the event ID
  and the path of the temporary token file, and exit with an error. The token
  stays in the temporary file. The operator then has the only copy.
- The token never appears in an argument, a log line, an error or a `Debug`
  output.
- With `--json`, an error gives one JSON object with an `error` field, as
  ADR 0004 says. The exit code is not zero.
- The setup helper moves the old token before it calls `provision`. Its
  behavior does not change.

## Implementation Steps

1. Add the path check before the relay request.
2. Change `write_token_file` to the safe write.
3. Add the tests.
4. Update the runbook section "Provision mode".

## Acceptance Criteria

Each item is a test:

- `provision` to an existing file fails, and the file content does not
  change. A stub relay records no request.
- `provision` to an existing symbolic link fails, and the link target does
  not change.
- `provision` to a new path writes the token with mode `0600`, and no
  temporary file stays in the directory.
- The safe write fails when the target appears between the check and the
  write. The existing file does not change, and the error names the
  temporary file. Test the write function directly.
- `provision --json` to an existing file prints one JSON object with
  `error`, and the exit code is not zero.
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

- No call in the standard library or the present dependencies fails when the
  target exists.
- A present test expects `provision` to replace a file.
