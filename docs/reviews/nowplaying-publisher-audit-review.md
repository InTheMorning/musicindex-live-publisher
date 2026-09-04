# Now-Playing / Live Publisher Audit Review

Audit date: 2026-09-04. Scope requested: accuracy, stability, security.

## Reviewed Artifact

Commit `1dea27f` ("Add systemd service file and comprehensive tests for
musicindex-live-publisher"), working tree clean.

Two crates, both fully implemented against their task packets:

- `mixxx-now-playing/` — producer. Plan:
  [rust-now-playing-utility-plan](../plans/rust-now-playing-utility-plan.md).
- `musicindex-live-publisher/` — consumer and relay client. Plan:
  [musicindex-live-publisher-plan](../plans/musicindex-live-publisher-plan.md).

Also reviewed: `systemd/musicindex-live-publisher.service`,
[the deployment runbook](../runbooks/musicindex-live-publisher-deploy.md), and
`scripts/mixxx-now-playing.sh` as the parity reference.

Baseline measured, not assumed:

- `cargo clippy --all-targets` — clean on both crates.
- `cargo test` — 37 passed, 0 failed, 1 ignored (the ignored test needs a local
  relay via `LOCAL_RELAY_ENDPOINT`).

Findings 1, 2, 3, and 8 below were reproduced with throwaway tests run against
the real code. The repro bodies are carried in the fix packets. Everything else
is a code reading with a file:line anchor.

## Pass/Fail

**Fail.** Not for build or test quality — both are good — but because five
defects are live, and three of them cause the system to do the wrong thing
quietly rather than stop.

The recurring shape across the high-severity findings: the failure is invisible.
A stale track keeps displaying, a stale block GUID keeps publishing, a disabled
target keeps logging "active". Nothing crashes, so nothing alerts.

## Required Fixes

Ordered by risk. Each has a fix packet.

**Status as of 2026-09-04:** findings 2, 3, and 5 are **fixed** and verified
end to end against a local relay. Findings 1 and 4 remain open; the user-scope
systemd deployment puts the drop directory in a `0700` runtime directory, which
makes finding 4 unreachable in that configuration and reduces finding 1 to the
now-playing text file in `/tmp`.

Finding 6 was added after the original audit, from an operator report that the
icecast title and the live value block disagree on air. It is appended in
discovery order rather than risk order; by risk it belongs with 2 and 5, because
it moves money to the wrong destination.

### 1. Symlink attack on the output sink — arbitrary file overwrite — OPEN

`mixxx-now-playing/src/sink.rs:55` builds a predictable temp path
(`.<basename>.<pid>.tmp`) and `sink.rs:56` writes it with `fs::write`, which
follows symlinks. The default output directory is `/tmp`
(`mixxx-now-playing/src/config.rs:12-13`).

A local user can pre-plant that path as a symlink and have the producer clobber
any file writable by your user, with content the attacker chooses. Reproduced:
the victim file ended up containing `attacker-controlled`. The `/tmp` sticky bit
does not help — the damage is done by the write, before the rename.

Fix packet: [audit-fix-task-001](../tasks/audit-fix-task-001-sink-atomic-write.md).

### 2. `blockGuid` is reused for every track — FIXED

`src/watcher.rs:147-151` keys `block_guids` by path
with `or_insert_with`, and only clears on remove. The deployed producer rewrites
one stable path (`default.json`) per track, so every track in a session
publishes under the same block GUID. Reproduced:

```text
track1 title=Track One block=09049e19-45bf-4806-9a3a-b065e0b64eee
track2 title=Track Two block=09049e19-45bf-4806-9a3a-b065e0b64eee
```

Block identity is what listening apps use to attribute a boost to a segment.

Fix packet: [audit-fix-task-002](../tasks/audit-fix-task-002-block-guid-per-track.md).

### 3. Wrong song displayed when `library.location` is NULL or dangling — FIXED

`mixxx-now-playing/src/history.rs:15` inner-joins `track_locations`. The bash
reference never did. The affected row does not merely vanish — the query
silently returns the *previous* history row instead.

Reproduced against a synthetic Mixxx 2.5.6 database: bash reports
`101|Orphan-Artist|Orphan Song`; the binary keeps showing
`TestArtist - Good Song`. On a live stream that is a stale track name displayed
indefinitely.

Fix packet: [audit-fix-task-003](../tasks/audit-fix-task-003-history-left-join.md).

### 4. The drop directory is trusted implicitly — OPEN

`src/watcher.rs:126-159` reads any non-hidden `*.json`
in `watch_dir` and turns its `value_routes` straight into payment destinations.
Nothing checks the directory's ownership or mode. Combined with the `/tmp`
default from finding 1, a local user can redirect boosts to their own node.

Fix packet: [audit-fix-task-004](../tasks/audit-fix-task-004-drop-dir-trust.md).

### 5. A fatal publish failure disables the target silently and permanently — FIXED

`src/relay.rs:343` sets `disabled = true` on 401, 403,
or 404. The worker then drains the channel forever, logging a warning per
payload (`relay.rs:304-312`). The process stays alive, so `Restart=on-failure`
never fires and `systemctl status` still reads `active`.

Meanwhile the relay keeps serving whatever payload it last accepted. If the
token expires mid-track, the track ends, the fallback publish is dropped, and
boosts keep routing to that track's destinations — **precisely the misrouting
the mandatory fallback exists to prevent**. The degraded mode is invisible and
pays the wrong people, which is the same reasoning that made a missing fallback
a startup error.

Fix packet: [audit-fix-task-005](../tasks/audit-fix-task-005-fatal-publish-exit.md).

### 6. Stream latency is never compensated, so every block flips early — OPEN

The publisher emits at wall-clock track-change time. Listeners hear the track
several seconds later. Nothing in the chain closes that gap: grep for
`delay|latency|offset|compensat` across `src/`, the drop-file contract, and the
config schema returns only HTTP retry backoff.

The two metadata paths diverge downstream of the producer, which writes both
outputs in the same call (`mixxx-now-playing/src/main.rs:178-185, 202`):

- The icecast title reaches the client *in band*. butt posts it to
  `/admin/metadata`, and icecast injects it into the outgoing byte stream, so it
  drains through the client's playout buffer alongside the audio it labels.
  Alignment is inherited, not computed.
- The live value block reaches the client out of band: drop file, inotify
  (75 ms debounce, `src/watcher.rs:16`), HTTPS POST, then
  `splitkit/src/lib.rs:231-250`, which stores and fans out on Socket.IO and SSE
  with no scheduling. It shares no buffer with the audio.

Publisher-side latency is under a second (0.5 s Mixxx poll, 75 ms debounce, one
round trip). Stream latency is 5 to 30 seconds. That difference is the defect,
and it opens two misattribution windows on every track:

1. **Early flip.** For the length of the stream delay after each local track
   change, the published block is track N+1 while listeners still hear track N.
   A boost in that window pays the next artist.
2. **Early clear.** Producer expiry fires at local
   `start + duration + 5 s slack` (`mixxx-now-playing/src/main.rs:210-217`).
   The drop file is removed, the publisher emits the fallback, and the tail of
   the track routes to the station fallback — or, with no `[target.fallback]`
   configured, to the dead placeholder `no-v4v-track@example.invalid`
   (`src/config.rs:18`), where the boost is lost outright.

This is the same failure class as findings 2 and 5 — money reaching the wrong
destination — and it survives both of their fixes.

Origin: the plan
([musicindex-live-publisher-plan:62](../plans/musicindex-live-publisher-plan.md))
and [task 002](../tasks/musicindex-live-publisher-task-002-live-value-transform.md)
both record "`startTime` is `0` for music blocks. No stream-elapsed clock is
needed." That is true of the payload shape and was read as timing being out of
scope entirely. Curiohoster does keep a broadcast clock: in example 3 of
`splitkit/docs/research/curiohoster-livevalue-socketio-examples.md`,
`broadcastTimestamp - eventTimestamp` is 1723.160 s against a `startTime` of
1723.151 s.

Fix packet: [audit-fix-task-006](../tasks/audit-fix-task-006-stream-delay-compensation.md).

## Optional Improvements

Medium, no packet written:

- **The unit as written cannot receive files from the producer.** The service
  has no `User=`, so it runs as root, and `RuntimeDirectory=` creates
  `/run/musicindex-live-publisher/nowplaying` as `root:root` with
  `RuntimeDirectoryMode=0750`. The runbook then instructs you to run
  `mixxx-now-playing --id3-file /run/.../default.json` as yourself. That is
  `EACCES`. Needs a `User=`/`Group=` shared with the producer and `0770`. Second-tier
  hardening (`SystemCallFilter`, `RestrictAddressFamilies`, `PrivateDevices`,
  `CapabilityBoundingSet`) is also absent — inherited from the splitkit
  template, not introduced here.
- **No scheme check on the publisher endpoint.** `relay.rs:build_url` accepts
  `http://` and sends the broadcaster token as a bearer header in cleartext. The
  asymmetry is the tell: the producer *does* validate scheme
  (`mixxx-now-playing/src/config.rs:normalize_musicindex_endpoint`), and it is
  the side that carries no credential.
- **One malformed tag kills the producer daemon.** `render.rs:70` propagates a
  `serde_json` error from the embedded `TXXX:MusicIndex Value Routes` frame, and
  `main.rs:192` propagates it out of the poll loop. Reproduced:
  `expected ident at line 1 column 2`. `read_tags` failures are handled
  gracefully immediately above; this path should match.
- **Unbounded `SQLITE_BUSY` retry.** `history.rs:70-75` loops on busy with a
  50 ms sleep, no `sqlite3_busy_timeout` and no cap. If Mixxx holds a write lock
  the poll blocks indefinitely, expiry never fires, and the drop file is never
  cleared — stale splits stay live.
- **Route cache never expires** (`musicindex.rs`), so updated splits are not
  picked up until restart.
- **Duplicate `event_id` across targets collapses silently.** `relay.rs` keys
  `senders` by `event_id`, but `config.rs:resolve_target` validates only `name`
  uniqueness. The second target overwrites the first and payloads publish with
  the wrong target's token. One line in `resolve_config` fixes it.

Low:

- `image` is always null. `render.rs:81` reads `musicindex_value("Image")`, but
  `MUSICINDEX_VOCABULARY` has no `Image` entry, so the field is dead. This is
  the artwork question left open in the live publisher plan.
- `tags.rs:frame_match_key` splits on the *last* colon, so
  `UFID:http://musicbrainz.org` becomes `//musicbrainz.org` and can never match.
  Lofty-known keys also arrive prefixed (`TXXX:BARCODE` vs the vocabulary's
  `BARCODE`) and land in `[Tags]` instead of `[MusicIndex]`.
- `mixxx_process_running()` runs `pgrep -i mixxx`, which matches the tool's own
  `comm` (`mixxx-now-playi`) and `mixxx-autodj-nf`. Only the own PID is
  excluded, so a second instance keeps the loop alive after Mixxx exits. It also
  spawns two processes per second indefinitely.
- `Runtime::new` deletes the now-playing text file at startup, blanking OBS on
  every restart. The bash script never did this.
- `write_token_file` chmods to `0600` *after* writing, leaving a window at the
  previous mode when the file already exists.
- The retry loop drains FIFO rather than jumping to latest, so an outage
  republishes a backlog of stale now-playing payloads in order.
- `watcher.rs:231 path_identity` is not canonicalized.

## Architectural Drift

Minor and mostly defensible.

- **Hand-rolled sqlite FFI.** `history.rs` carries roughly 150 lines of `unsafe`
  while `rusqlite` is a dependency used only for its `ffi` module. The code is
  correct: read-only, `query_only`, strings copied before reset, `column_text`
  before `column_bytes`, statement finalized before the connection closes. But
  that last property rests on **field declaration order** in `HistoryWatcher`
  (`statement` before `connection`) with no comment saying so. Reordering those
  two fields is a use-after-free. Either document the invariant or move to the
  safe API.
- **Cross-crate dev-dependency.** `mixxx-now-playing` dev-depends on
  `musicindex-live-publisher` by path for the drop-file contract test. Deliberate
  and useful, but it means the producer's test suite cannot build without the
  consumer crate present.
- **Route lookup ordering.** The plan specified track → feed → embedded. In
  practice a non-404 error on the track lookup returns embedded immediately and
  skips the feed lookup (`musicindex.rs:resolve_value_routes`). Arguably fine;
  worth noting as an intentional deviation rather than an oversight.

Nothing else drifted. The drop-file contract, the wrapped-payload rejection, the
presence-as-API sink, and the fallback-on-clear semantics are all implemented as
designed.

## Missing Tests

- No test writes through the sink with a hostile pre-existing temp path.
- No test drives two *different* tracks through the same drop-file path, which
  is exactly the deployed pattern and is why finding 2 survived a passing suite.
- No test covers a history row whose `library.location` is NULL or dangling.
- No test asserts the process exits, or otherwise signals, after a fatal publish
  outcome — only that the outcome is classified as fatal.
- No test feeds a malformed embedded `Value Routes` frame through the JSON
  render path.
- `systemd-analyze verify` is in the runbook but not in any test command list.

## Merge Recommendation

Already merged, so this reads as a remediation list rather than a merge gate.

Do not run this against a live relay with real splits until findings 2, 4, and 5
are fixed — those three are the ones that can move money to the wrong
destination. Finding 1 should be fixed before the producer runs on any machine
with another local user. Finding 3 is display-only but visible to your audience.

The medium items are safe to batch afterwards, with one exception: the `User=`
gap means the documented deployment cannot work as written, so it should be
corrected alongside the packets rather than deferred.
