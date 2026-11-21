# Producer Template Task 001: `--instance` And The Mixxx Lock

Status: Planned 2026-11-21.

Every criterion is mechanical.

## Goal

`mixxx-now-playing --instance NAME` finds its paths, its target and its display directory as ADR 0021 §The Producer
Reads Its Instance says. Each Mixxx producer takes the Mixxx lock of ADR 0021 §One Mixxx Producer At A Time. A
producer that does not get the Mixxx lock exits with status 3.

## Base components

| Component | Status | Location | Reason |
| --- | --- | --- | --- |
| Producer lock | extend | mixxx-now-playing/src/lock.rs | Add the Mixxx lock with the same wait loop. Do not copy the loop. |
| Producer path resolution | extend | mixxx-now-playing/src/config.rs | Add the instance form to `ResolvedConfig::resolve_with_env`. |
| Producer command line | extend | mixxx-now-playing/src/cli.rs | Add `--instance`. |
| Instance name and target name rules | reuse | src/config.rs | The producer applies the same target name rule. Read `validate_target_name` and keep the same rule. |

## Files To Inspect

- `docs/adr/0021-mixxx-producer-template-unit.md`
- `docs/adr/0016-instances-events-and-producer-lock.md` §The Producer Lock
- `mixxx-now-playing/src/lock.rs`, `mixxx-now-playing/src/config.rs`, `mixxx-now-playing/src/cli.rs`,
  `mixxx-now-playing/src/main.rs`
- `src/config.rs` (`RawTarget`, `validate_target_name`) for the configuration form

## Files Likely To Change

- `mixxx-now-playing/src/lock.rs`
- `mixxx-now-playing/src/config.rs`
- `mixxx-now-playing/src/cli.rs`
- `mixxx-now-playing/src/main.rs`
- `mixxx-now-playing/tests/lifecycle.rs` or a new test file in `mixxx-now-playing/tests/`

## Do Not Touch

- `src/**`. The publisher does not change.
- `systemd/**`, `packaging/**`, `Cargo.toml`, `scripts/**`. Tasks 002 and 003 change them.
- `docs/adr/**`.
- The drop file, the song file, the display file and their forms.

## Constraints

- `--instance NAME`: the name must match `^[a-z0-9][a-z0-9-]{0,31}$`. Do not add a regex dependency.
- With `--instance`, the producer reads `${XDG_CONFIG_HOME:-$HOME/.config}/musicindex-live-publisher/NAME/config.toml`.
  It reads only `[[target]]` `name` and `display_dir`. It ignores each other key. It never writes the file.
- With `--instance`, `XDG_RUNTIME_DIR` must be set and not empty. Else the producer exits with an error.
- The paths are these. `RUN` is `$XDG_RUNTIME_DIR`:
  - drop directory `RUN/musicindex-live-publisher/NAME/nowplaying`
  - drop file `<drop directory>/<target>.json`
  - song file `<drop directory>/now-playing.txt`
  - display directory: the `display_dir` of the target, which must be `RUN/musicindex-live-publisher/NAME/display`
- Zero targets, more than one target, a different `display_dir`, or a missing configuration file give an error that
  names ADR 0021 and the configuration path.
- `--instance` with `--target`, `--id3-file`, `--txt-file` or `--display-dir` gives an error that names ADR 0021.
- The Mixxx lock file is `RUN/mixxx-now-playing/mixxx.lock`, mode `0600`. Create the directory with mode `0700` when
  it is absent.
- Extend `lock.rs` so that one wait function serves the producer lock and the Mixxx lock. Keep `LOCK_WAIT_LIMIT` at
  2 seconds and keep the error of the producer lock.
- After it gets the Mixxx lock, the producer writes its holder name into the lock file: the instance name, or the
  drop directory without `--instance`. Truncate the file first. Do not delete the file at exit.
- When the Mixxx lock is not free after the limit, read the holder name from the file. Exit with status 3. The error
  names ADR 0021 and the holder. For example: `ADR 0021: the Mixxx producer of instance mixxx holds the Mixxx lock
  in <path>. Stop mixxx-now-playing@mixxx.service first, then start this instance.`
- Take the Mixxx lock before `.producer.lock`. Hold the two locks for the life of the process.
- `--once` takes no lock. The `command` subcommand takes no lock.
- Without `--instance`, the producer keeps its current flags and defaults. It takes the Mixxx lock in watch mode
  when `XDG_RUNTIME_DIR` is set. When `XDG_RUNTIME_DIR` is not set, it exits with an error that names ADR 0021.
- Put no host name, no home path and no `/run/user/` number in code or tests. Tests use `tempfile` directories and
  set the environment of a child process.
- Write comments in Simplified Technical English (`~/.agents/skills/asd-ste100/SKILL.md`). Follow the `rust-dev`
  skill.

## Acceptance Criteria

All are mechanical.

1. A unit test shows that `--instance show` with a configuration of one target `show` resolves the drop file
   `RUN/musicindex-live-publisher/show/nowplaying/show.json` and the song file in the same directory.
2. A unit test shows that a `display_dir` equal to `RUN/musicindex-live-publisher/show/display` resolves to that
   display directory, and that no `display_dir` gives no display output.
3. Unit tests show an error that names ADR 0021 for: zero targets, two targets, a different `display_dir`, a missing
   file, a bad instance name, and `--instance` with each of the four refused flags.
4. A test shows that a second Mixxx lock in the same directory fails after the limit, and that its error names the
   holder that the first lock wrote.
5. A process test starts the producer binary twice with the same `XDG_RUNTIME_DIR` and different instances. The
   second process exits with status 3. Use a stub Mixxx database or the existing test fixtures. If the producer
   exits before the lock because Mixxx is not found, take the Mixxx lock in the test process instead and start
   the binary one time.
6. The tests of `lock.rs` for `.producer.lock` pass with no change to their assertions.
7. The gate of `AGENTS.md` passes.

## Delegate Rules

A delegate does not commit. It leaves the change in the working tree and reports the files and the gate result.
