# Task 004 — Output sink and file lifecycle

Part of [Rust now-playing utility plan](../plans/rust-now-playing-utility-plan.md), Phase 1.

## Goal

Wire tasks 001–003 into a working binary. Write the now-playing line, and make the
metadata file exist only while a V4V track is playing. This completes Phase 1.

## Files To Inspect

- `docs/plans/rust-now-playing-utility-plan.md` — Concern 3, Phase 1
- `scripts/mixxx-now-playing.sh` — the exact now-playing line format to match
- Task 001–003 output

## Files Likely To Change

- `mixxx-now-playing/src/sink.rs` (new)
- `mixxx-now-playing/src/classify.rs` (new)
- `mixxx-now-playing/src/main.rs`
- `mixxx-now-playing/tests/lifecycle.rs` (new)

## Do Not Touch

- Task 002's query, task 003's vocabulary table
- The shell and Python scripts at repo root

## Constraints

- **Presence is the signal.** When the current track is not under the V4V root,
  the metadata file must not exist on disk. Removing it is not optional and not a
  status field. This is the primary defect being fixed.
- Writes are atomic: temp file **in the same directory**, then `rename`. A temp
  file elsewhere breaks atomicity across filesystems.
- Never write identical bytes twice. Consumers watch by inotify and re-render on
  every write.
- Removal ignores `ENOENT`.
- Match `scripts/mixxx-now-playing.sh` byte for byte on the now-playing line, including the
  hyphen-stripping quirk: strip every `-` from `artist|title`, then replace `|`
  with ` - `. So `Test-Artist` renders as `TestArtist`. Gate behind
  `--strip-hyphens`, default on.
- No trailing newline on the now-playing file — the shell script uses
  `printf '%s'`.

## Implementation Steps

1. Implement the sink:

   ```rust
   enum Presence { Present(String), Absent }

   struct OutputFile { path: PathBuf, last: Option<String> }

   impl OutputFile {
       fn set(&mut self, p: Presence) -> Result<()> {
           match p {
               Present(s) if self.last.as_deref() != Some(&s) => self.write_atomic(&s),
               Present(_) => Ok(()),
               Absent     => self.remove(),
           }
       }
   }
   ```

2. Implement `classify`: canonicalise the track path and the V4V root, then
   `path.starts_with(root)`. A path that fails to canonicalise is not V4V.
3. Clear both output files at startup, before the first poll, so a stale file from
   a crashed run is never read as truth.
4. Main loop: poll at `--poll-secs`; on a new track, write the now-playing line
   always, and set the metadata file to `Present` when V4V, `Absent` otherwise.
5. Exit when Mixxx is no longer running, matching the shell script's `pgrep` check.
6. Remove the metadata file on exit via a `Drop` guard **and** a `signal-hook`
   handler for SIGTERM and SIGINT. The Python version has no exit path at all, so
   a stale file survives every shutdown.
7. Implement `--once`: process the latest history row once and exit.

## Acceptance Criteria

- V4V track plays → metadata file exists with rendered content.
- Non-V4V track follows → metadata file **does not exist**.
- V4V track follows that → file exists again.
- Same track polled repeatedly → exactly one write, verified by mtime.
- SIGTERM → metadata file removed before exit.
- Startup with a pre-existing stale metadata file → cleared before first poll.
- Now-playing line matches `scripts/mixxx-now-playing.sh` output for the same fixture,
  compared with `cmp`.

## Test Commands

```bash
cd mixxx-now-playing
cargo clippy -- -D warnings
cargo test lifecycle
cargo test --test lifecycle -- --nocapture
```

## Expected Final Report

- Files created or changed
- The lifecycle test matrix and results
- Byte-comparison result against the shell script's output
- Confirmation that SIGTERM removes the file

## Escalation Triggers

Stop and ask before proceeding if:

- The now-playing line cannot be made byte-identical to the shell script.
- `rename` is not atomic for the chosen output location.
- Removing the metadata file appears to race a consumer reading it.
