# Task 004 — Configuration, targets, and token loading

Part of [MusicIndex live publisher plan](../plans/musicindex-live-publisher-plan.md), Phase 3.

## Goal

Load service configuration: the watch directory, the relay endpoint, and a list
of publish targets with their event IDs, tokens, and fallback splits.

## Files To Inspect

- `docs/plans/musicindex-live-publisher-plan.md` — Configuration
- `~/build/splitkit/README.md` — how the broadcaster token is issued and stored

## Files Likely To Change

- `src/config.rs` (new)
- `src/main.rs`
- `tests/config.rs` (new)

## Do Not Touch

- Task 003's watcher logic
- Task 002's transform

## Constraints

- **Model targets as a list from the start**, even though there is one today. The
  immediate use is a single private stream; the longer-term goal is a tool
  podcasters run for public indexed music podcasts. A hardcoded single event
  would have to be unpicked to get there.
- **A target with no configured fallback is a startup error.** Refuse to start
  rather than run in a state where a stopped track leaves stale destinations
  live. This is deliberate: the degraded mode is invisible and pays the wrong
  people.
- Tokens are read from a file referenced by `token_file`, never inlined in the
  config and never written by this service. The relay issues a token exactly once
  and stores only a SHA-256 hash, so a lost token cannot be recovered.
- Warn if a `token_file` is more permissive than `0600`.
- Never log a token, not even truncated, at any verbosity.

## Implementation Steps

1. Parse this TOML shape:

   ```toml
   watch_dir = "/run/musicindex-live-publisher/nowplaying"
   endpoint  = "https://api.musicindex.org"

   [[target]]
   name       = "default"
   event_id   = "1873a383-..."
   token_file = "/etc/musicindex-live-publisher/default.token"

     [target.fallback]
     title = "Homegrown Hits"
     destinations = [
       { name = "Station", type = "node", address = "03...", split = "100" },
     ]
   ```

2. Resolve a drop file's `target` field to a configured target by `name`. An
   unmatched target name is logged and the file skipped.
3. Load each token from its file at startup, trimming trailing whitespace.
   A missing or empty token file is a startup error.
4. Validate at startup: at least one target, unique names, non-empty `event_id`,
   a fallback with at least one destination, and every fallback `split` parsing
   as a decimal.
5. Support `--config <path>`, defaulting to
   `/etc/musicindex-live-publisher/config.toml`.
6. Flags override config where both apply (`--watch-dir`, `--endpoint`).

## Acceptance Criteria

- A valid single-target config loads and validates.
- A two-target config loads; a drop file routes to the target matching its
  `target` field.
- A target with no `[target.fallback]` → startup **error**, with a message naming
  the target.
- A fallback with an empty `destinations` list → startup error.
- Missing token file → startup error naming the path.
- Token file at `0644` → warning, still loads.
- Duplicate target names → startup error.
- A drop file naming an unknown target → logged and skipped, not fatal.
- No test or log line contains a token value.

## Test Commands

```bash
cd musicindex-live-publisher
cargo clippy -- -D warnings
cargo test config
```

## Expected Final Report

- Files created or changed
- The validation rules implemented and their error messages
- Test names and results
- Confirmation that tokens appear in no log output at any verbosity

## Escalation Triggers

Stop and ask before proceeding if:

- The refuse-to-start-without-fallback rule blocks a legitimate configuration you
  encounter. Raise it; do not soften the rule to a warning.
- Systemd `LoadCredential` semantics require a different token path shape than
  `token_file` — that is task 006's concern, note it and continue.
