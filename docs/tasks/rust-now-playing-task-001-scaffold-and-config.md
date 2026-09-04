# Task 001 — Crate scaffold and configuration resolution

Part of [Rust now-playing utility plan](../plans/rust-now-playing-utility-plan.md), Phase 1.

## Goal

Create the `mixxx-now-playing` crate and implement configuration resolution: CLI
flags, the V4V library root, and the MusicIndex endpoint. No Mixxx access, no tag
reading, no output files yet.

## Files To Inspect

- `docs/plans/rust-now-playing-utility-plan.md` — Target State, Concern 2
- `~/build/v4vmm/src/config.rs:248` — `config_path()`, the `~/.config/v4vmm/config.toml` location
- `~/build/v4vmm/src/config.rs:469` — `default_music_dir()`, confirms `~/V4Vmusic`
- `~/build/v4vmm/src/api.rs:7` — `DEFAULT_BASE_URL`

## Files Likely To Change

- `mixxx-now-playing/Cargo.toml` (new)
- `mixxx-now-playing/src/main.rs` (new)
- `mixxx-now-playing/src/cli.rs` (new)
- `mixxx-now-playing/src/config.rs` (new)

## Do Not Touch

- `scripts/mixxx-now-playing.sh`, `mixxx-now-playing.py`, `mixxx-skip`, `mixxx-autodj-nfs.sh`
- Anything under `~/build/v4vmm/` — read only, never edit
- `docs/plans/`

## Constraints

- Rust 2024 edition idioms. `anyhow::Result` at fallible boundaries.
- No `unwrap()` or `expect()` outside `#[cfg(test)]`.
- `cargo clippy -- -D warnings` must be clean.
- Do not add dependencies beyond those listed in the plan's Target State.
- Parse `~/.config/v4vmm/config.toml` with `serde` + `toml` into a struct holding
  only the two fields needed (`music_dir`, `musicindex_endpoint`), both optional.
  A missing or malformed config file is not an error; fall through to defaults.

## Implementation Steps

1. Create the crate at `mixxx-now-playing/` with binary name `mixxx-now-playing`.
   Add exactly the dependencies from the plan's Target State table.
2. Define CLI flags: `--db-file`, `--txt-file`, `--id3-file`, `--v4v-root`,
   `--poll-secs` (default 0.5), `--once`, `--format text|json` (default `text`),
   `--strip-hyphens` (default true), `--verbose`.
3. Implement V4V root resolution with this precedence, first match wins:
   1. `--v4v-root`
   2. `$V4V_MUSIC_DIR`
   3. `music_dir` in `~/.config/v4vmm/config.toml`
   4. `~/V4Vmusic`
4. Implement MusicIndex endpoint resolution: `musicindex_endpoint` from the same
   config file, else `https://api.musicindex.org`.
5. Expand a leading `~/` in any path read from config or flags.
6. Canonicalise the resolved V4V root at startup. If it does not exist, log a
   warning and keep the uncanonicalised path — do not exit.
7. `main` resolves config, prints it under `--verbose`, and exits 0.

## Acceptance Criteria

- `cargo build` succeeds; `cargo clippy -- -D warnings` is clean.
- Each precedence level is covered by a unit test using `tempfile` for the config
  file and an explicit env/flag override.
- A malformed `config.toml` falls back to `~/V4Vmusic` without erroring.
- `--verbose` prints the resolved V4V root and endpoint.
- No file is written or removed by this task.

## Test Commands

```bash
cd mixxx-now-playing
cargo fmt --check
cargo clippy -- -D warnings
cargo test config
```

## Expected Final Report

- Files created, with line counts
- Precedence test names and pass/fail
- Any deviation from the plan's flag list, with reason
- `cargo clippy` output summary

## Escalation Triggers

Stop and ask before proceeding if:

- The plan's dependency versions do not resolve against the current registry.
- `~/.config/v4vmm/config.toml` uses field names other than `music_dir` and
  `musicindex_endpoint`.
- Any step appears to require reading the Mixxx database or an audio file — that
  belongs to tasks 002 and 003.
