# Audit Fix 001 — Safe Atomic Write In The Producer Sink

Remediates finding 1 of the
[audit review](../reviews/nowplaying-publisher-audit-review.md).

## Goal

Make `OutputFile::write_atomic` refuse to write through a pre-existing path, so
a hostile symlink planted at the predictable temp path cannot redirect the write
to an unrelated file.

Reproduced today: an attacker-planted symlink caused the sink to overwrite an
unrelated file with attacker-chosen content, running as the producer's user.

## Files To Inspect

- `mixxx-now-playing/src/sink.rs` — `write_atomic` at `:43-66`, temp path at
  `:55`, `fs::write` at `:56`.
- `mixxx-now-playing/src/config.rs:12-13` — the `/tmp` defaults that make the
  attack reachable in the default configuration.
- `mixxx-now-playing/tests/lifecycle.rs` — existing sink lifecycle coverage and
  its `TempDir` idiom.

## Files Likely To Change

- `mixxx-now-playing/src/sink.rs`
- `mixxx-now-playing/tests/lifecycle.rs` (or a new `tests/sink_security.rs`)

## Do-Not-Touch List

- `~/build/v4vmm/` and `~/build/splitkit/` — read only, never edit.
- `musicindex-live-publisher/` — this packet is producer-side only.
- The `Presence` enum and the presence-is-the-API contract. Removal on `Absent`,
  no-op on unchanged content, and the absence of a trailing newline in the
  now-playing text file are all load-bearing and must not change.
- `render_now_playing_line` formatting parity with `scripts/mixxx-now-playing.sh`.

## Constraints

- The write must stay atomic from a reader's perspective: temp file in the
  **same directory**, then `rename`. Do not switch to a write-in-place scheme,
  and do not move the temp file to another directory — `rename` across
  filesystems is not atomic.
- The temp file must be created with `O_EXCL` semantics
  (`OpenOptions::new().create_new(true)`), so an existing file or symlink at
  that path is an error rather than a target.
- A collision on the temp path must not be fatal to the daemon. Retry with a
  fresh name a bounded number of times, then return an error for that write.
- Do not make the temp name guessable from the output path alone. Process ID is
  not sufficient — it is readable from `/proc`.
- Clean up the temp file if the write or rename fails. Leaking `.tmp` files into
  a watched drop directory would feed the publisher garbage.
- On Unix, create the temp file with mode `0600`.
- Keep `remove_file_if_exists` ignoring `ENOENT`.

## Implementation Steps

1. Add a failing test first. This is the verified repro — it currently fails:

   ```rust
   #[test]
   fn sink_refuses_to_write_through_a_planted_symlink() {
       let dir = tempfile::tempdir().unwrap();
       let victim = dir.path().join("victim");
       std::fs::write(&victim, b"ORIGINAL\n").unwrap();

       let out = dir.path().join("metadata.txt");
       // Mirror whatever temp-name scheme write_atomic uses.
       let temp = dir.path().join(format!(".metadata.txt.{}.tmp", std::process::id()));
       std::os::unix::fs::symlink(&victim, &temp).unwrap();

       let mut sink = OutputFile::new(&out);
       let _ = sink.set(Presence::Present("attacker-controlled".into()));

       assert_eq!(std::fs::read_to_string(&victim).unwrap(), "ORIGINAL\n");
   }
   ```

   Note: once the temp name includes randomness this exact test can no longer
   guess the path, so it stops proving anything. Keep it as a regression guard
   for the `create_new` behaviour by testing the helper that opens the temp file
   directly, or by planting a symlink at every candidate name the helper can
   produce within one seeded run. Do not delete the case.
2. Replace `fs::write(&temp_path, content)` with an explicit
   `OpenOptions::new().write(true).create_new(true).mode(0o600)` open followed by
   `write_all`. Gate `.mode()` behind `#[cfg(unix)]`.
3. Add randomness to the temp file name in addition to the PID.
4. Wrap creation in a bounded retry loop for `ErrorKind::AlreadyExists`.
5. On any error after the temp file exists, remove it before returning.
6. Add a second test asserting a normal write still lands atomically and that
   the temp file does not survive a successful write.

## Acceptance Criteria

- A symlink planted at the temp path is never followed; the victim file is
  unchanged and the sink either succeeds via a different temp name or returns an
  error.
- No `.tmp` file remains in the output directory after a successful write, or
  after a failed one.
- Existing sink behaviour is unchanged: `Present` with identical content does not
  rewrite the file (mtime stable), `Absent` removes it, and a repeated `Absent`
  is not an error.
- The now-playing text file still has no trailing newline.
- `cargo clippy --all-targets` clean.

## Test Commands

```bash
cd mixxx-now-playing
cargo clippy --all-targets --offline
cargo test --offline
```

## Expected Final Report Format

- What changed, by file and function.
- The temp-name scheme you chose and why it is not guessable.
- The retry bound you picked.
- Test output: the new test names, plus the full `test result:` lines.
- Confirmation that the `/tmp` defaults in `config.rs:12-13` were left alone,
  and whether you think they should move (do not move them in this packet).

## Escalation Triggers

- If making the temp name unguessable requires a new dependency, stop and ask.
  Prefer `std` plus what is already in the lock file over adding `rand`.
- If `create_new` turns out to break an existing lifecycle test in a way that
  suggests the sink is being called concurrently, stop — that is a different and
  larger bug than this packet covers.
- If you conclude the `/tmp` default should change as part of the fix, raise it
  rather than doing it here; it is a deployment-contract change that affects the
  runbook and the publisher's watch directory.
