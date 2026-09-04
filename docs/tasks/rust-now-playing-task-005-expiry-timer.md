# Task 005 — Expiry timer

Part of [Rust now-playing utility plan](../plans/rust-now-playing-utility-plan.md), Phase 2.

## Goal

Expire the metadata file after roughly the track's length, so it does not linger
after the last track of a set.

## Files To Inspect

- `docs/plans/rust-now-playing-utility-plan.md` — Metadata lifetime, Phase 2
- Task 003's `TrackTags.duration`
- Task 004's main loop

## Files Likely To Change

- `mixxx-now-playing/src/expiry.rs` (new)
- `mixxx-now-playing/src/cli.rs`
- `mixxx-now-playing/src/main.rs`
- `mixxx-now-playing/tests/expiry.rs` (new)

## Do Not Touch

- Task 004's sink and removal logic — the timer decides *when* `Absent` is set,
  it does not reimplement how

## Constraints

- No new dependencies. `lofty` already returns the duration in task 003's read.
- No trait, no play-state abstraction. A single struct and a predicate:

  ```rust
  struct Expiry { deadline: Option<Instant> }   // None = never expires
  ```

- **The clock must be injectable.** Tests assert deadline arithmetic in
  milliseconds and must not sleep for the length of a track.
- The timer only shortens presence. It never causes the metadata file to appear.

## Implementation Steps

1. Add flags:
   - `--expiry duration|none` (default `duration`)
   - `--expiry-slack <secs>` (default 5)
   - `--expiry-fallback <secs>` (default 600)
2. Arm the deadline at track change as `started_at + duration + slack`.
3. When `lofty` reports no duration, arm at `started_at + expiry-fallback` so an
   unreadable file cannot pin the metadata indefinitely.
4. Under `--expiry none`, `deadline` is `None` and only a track change clears the
   file — Phase 1 behaviour exactly.
5. Each poll, set the metadata file to `Absent` once the deadline has passed, even
   if no new history row has arrived.
6. Rendering and value routes are untouched by this task.

## Acceptance Criteria

- Deadline reached with no new history row → metadata file removed.
- New track arrives before the deadline → file replaced, deadline re-armed. The
  normal path during a set; the old deadline must not fire afterwards.
- `--expiry none` → file persists until the next track change.
- No duration available → fallback deadline used, not an infinite one.
- Slack is added, not subtracted: a 180s track with default slack expires at 185s.
- All timing tests run against an injected clock and complete in milliseconds.

## Test Commands

```bash
cd mixxx-now-playing
cargo clippy -- -D warnings
cargo test expiry
```

## Expected Final Report

- Files created or changed
- Test names and results, with the injected-clock approach described
- Confirmation that no test sleeps longer than 100ms

## Escalation Triggers

Stop and ask before proceeding if:

- Injecting the clock would require restructuring task 004's main loop
  substantially.
- Crossfade behaviour makes re-arming ambiguous when two history rows arrive
  within one poll interval.
