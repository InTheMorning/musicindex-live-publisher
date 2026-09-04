# Audit Fix 004 — Drop Directory Trust Checks

Remediates finding 4 of the
[audit review](../reviews/nowplaying-publisher-audit-review.md).

## Goal

Refuse to treat a drop directory as trustworthy when anyone other than the
service user can write to it, and refuse to read drop files that user does not
own. The contents of these files become payment destinations; today any local
user who can create a `*.json` file in the watched directory can redirect boosts.

## Files To Inspect

- `src/watcher.rs:126-159` — `payload_for_upsert`,
  which reads and parses without any provenance check.
- `src/watcher.rs:218-229` — `final_drop_files`, the
  startup scan.
- `src/main.rs:200-224` — `wait_for_watch_dir`, the
  natural place for the directory check.
- `src/config.rs:274-300` —
  `warn_if_token_file_permissive`, the existing permission-inspection idiom to
  follow for style.
- `systemd/musicindex-live-publisher.service:15-17` —
  `RuntimeDirectory` and `RuntimeDirectoryMode`, which determine what the
  deployed directory actually looks like.

## Files Likely To Change

- `src/main.rs`
- `src/watcher.rs`
- `tests/watcher.rs`
- `docs/runbooks/musicindex-live-publisher-deploy.md`

## Do-Not-Touch List

- `~/build/v4vmm/` and `~/build/splitkit/` — read only, never edit.
- `mixxx-now-playing/` — consumer-side only.
- Clear semantics: removal publishes the fallback; malformed or unknown-schema
  files do not. A file rejected for *ownership* must behave like a malformed
  file — skipped, no fallback — not like a removal.
- The `is_final_drop_file` rule (visible, `.json`). Ownership is a separate check
  layered on top.

## Constraints

- **Directory check, at startup, fatal.** If `watch_dir` is group-writable or
  world-writable, refuse to start. This is the same reasoning as the
  refuse-to-start-without-a-fallback rule: the degraded mode is invisible and
  pays the wrong people, so it must be a hard failure rather than a warning.
- Allow an explicit opt-out flag for the group-writable case, because the
  intended deployment has the producer and the service sharing a group. Suggested
  shape: accept group-writable only when the directory's group matches the
  service's own GID. World-writable is never acceptable and must not have an
  opt-out.
- **Per-file check, at read, non-fatal.** A drop file not owned by the service
  user (or by root) is skipped with a warning. Do not publish the fallback for
  it — an attacker must not be able to force a fallback publish by planting a
  file.
- Check ownership on the opened file where practical, not on the path before
  opening. Use `File::open` then `metadata()` on the handle, so the check and the
  read see the same inode.
- Never log the file's contents on rejection. Log the path, the owning UID, and
  the expected UID.
- `#[cfg(unix)]` for all of it, with a no-op on other platforms, matching the
  existing `warn_if_token_file_permissive` pattern.
- No new dependencies. `std::os::unix::fs::MetadataExt` provides `uid`, `gid`,
  and `mode`.

## Implementation Steps

1. Add a helper that validates a directory: stat it, reject `0o002`
   unconditionally, reject `0o020` unless the GID matches the process GID.
2. Call it from `wait_for_watch_dir` after the directory is confirmed to exist,
   returning an error that names the offending mode in octal.
3. Add a helper that validates an opened file handle's owner against
   `unsafe { libc::getuid() }` — or, to avoid adding `libc`, against the UID of
   the watch directory established at startup. Prefer the latter; pass the
   expected UID into `DropWatcher` at construction.
4. Call it from `payload_for_upsert` immediately after opening, before `parse`.
   Return `Ok(None)` on rejection, matching the malformed-file path.
5. Apply the same check in `final_drop_files` or in the `payload_for_upsert` it
   already funnels through — confirm the startup scan cannot bypass it.
6. Tests:
   - A `0o777` watch directory is rejected at startup.
   - A `0o770` watch directory whose group matches is accepted.
   - A drop file owned by a different UID is skipped and produces no payload.
   - A skipped file does **not** trigger a fallback publish.
   - Ownership rejection does not disturb a valid file processed in the same scan.
7. Update the runbook's Failure Modes section with the new startup error and
   what to do about it.

## Acceptance Criteria

- A world-writable watch directory is a startup error, with the mode in the
  message.
- A group-writable directory is accepted only when the group matches; otherwise
  it is a startup error.
- A foreign-owned drop file is skipped with a warning and publishes nothing —
  neither a track payload nor a fallback.
- All eight existing tests in `tests/watcher.rs` still pass.
- The runbook documents the new failure mode.
- `cargo clippy --all-targets` clean.

## Test Commands

```bash
cd musicindex-live-publisher
cargo clippy --all-targets --offline
cargo test --offline
```

The foreign-ownership test cannot create a file owned by another UID without
privileges. Structure it so the expected UID is injected into `DropWatcher`
rather than read from the process, so the test can set an expected UID that
deliberately does not match the file it just wrote. Do not write a test that
requires root, and do not mark it `#[ignore]`.

## Expected Final Report Format

- The exact rules implemented, as a short table: condition, fatal or skip.
- How the expected UID reaches `DropWatcher`, and why that choice keeps the test
  runnable unprivileged.
- Confirmation that a rejected file does not trigger a fallback, with the test
  name that proves it.
- The runbook diff.
- Full `test result:` lines.

## Escalation Triggers

- If the deployed `RuntimeDirectory` turns out to be `0750` and `root`-owned
  while the producer runs as a normal user, the directory check will make the
  service refuse to start on a configuration that already could not work. Report
  it — that is the `User=` gap listed under Optional Improvements in the review,
  and it needs fixing in the unit rather than being worked around here.
- If you conclude the per-file ownership check needs `libc` as a dependency,
  stop and ask rather than adding it.
- If any existing watcher test starts failing because it writes drop files into
  a `TempDir` with permissive default modes, stop and report before loosening
  the rule — the fix is likely in the test setup, not the rule.
