# Audit Fix 002 — Fresh `blockGuid` Per Track

Remediates finding 2 of the
[audit review](../reviews/nowplaying-publisher-audit-review.md).

## Goal

Mint a new `blockGuid` when the drop file at a watched path describes a
different track, while still reusing the same GUID for retries and for redundant
filesystem events on unchanged content.

Reproduced today: two different tracks written to the same path published under
one block GUID.

```text
track1 title=Track One block=09049e19-45bf-4806-9a3a-b065e0b64eee
track2 title=Track Two block=09049e19-45bf-4806-9a3a-b065e0b64eee
```

## Files To Inspect

- `src/watcher.rs` — `block_guids` field at `:53`,
  `payload_for_upsert` at `:126-159`, the `or_insert_with` at `:147-151`, removal
  at `:113`.
- `src/livevalue.rs:73-91` — `payload_from_dropfile`
  and the doc comment stating GUID generation is deliberately outside the
  transform so retries reuse block identity. That intent stays; only the
  watcher's caching key changes.
- `tests/watcher.rs` — existing watcher coverage.
- `docs/tasks/musicindex-live-publisher-task-003-watcher-and-clear.md` — the
  original clear semantics this must not regress.

## Files Likely To Change

- `src/watcher.rs`
- `tests/watcher.rs`

## Do-Not-Touch List

- `~/build/v4vmm/` and `~/build/splitkit/` — read only, never edit.
- `mixxx-now-playing/` — consumer-side fix only.
- `livevalue.rs` transform functions and their signatures. `payload_from_dropfile`
  must stay a pure function taking `event_guid` and `block_guid`.
- The `{event_id, metadata}` pre-send rejection in `relay.rs`.
- Clear semantics: removal publishes the fallback; a malformed or unknown-schema
  file does **not**.

## Constraints

- Same track, repeated event → same `blockGuid`. This is what makes retries and
  duplicate inotify events safe, and there are existing tests asserting a single
  payload for a temp-file-plus-rename sequence.
- Different track, same path → new `blockGuid`.
- Track identity must be derived from the drop file's content, not from the
  path. A stable, cheap choice is the parsed `DropFile` itself (it already
  derives `PartialEq`) or a hash of the raw bytes. If you hash, hash the parsed
  form, not the raw bytes — a producer rewrite that only reorders JSON keys is
  the same track.
- `eventGuid` must remain the configured `event_id` and must not change.
- Do not let the identity map grow without bound. It is keyed by path, and paths
  are removed on `Remove`, so the current shape is fine — keep that property.
- No new dependencies. `uuid` is already present.

## Implementation Steps

1. Add a failing test first. This is the verified repro:

   ```rust
   #[test]
   fn watcher_new_track_at_same_path_gets_a_new_block_guid() {
       let dir = tempfile::tempdir().unwrap();
       let path = dir.path().join("default.json");
       let mut w = DropWatcher::new(target(), Duration::from_millis(0));

       std::fs::write(&path, dropfile_json("Track One", "03one")).unwrap();
       let first = w.process_event(upsert(&path), Instant::now()).unwrap();

       std::fs::write(&path, dropfile_json("Track Two", "03two")).unwrap();
       let second = w.process_event(upsert(&path), Instant::now()).unwrap();

       assert_ne!(first[0].block_guid, second[0].block_guid);
   }
   ```

2. Change `block_guids: HashMap<PathBuf, String>` to hold both the identity and
   the GUID, for example `HashMap<PathBuf, (TrackIdentity, String)>`.
3. In `payload_for_upsert`, compute the identity from the parsed `DropFile`.
   Reuse the stored GUID when the identity matches; otherwise mint a fresh one
   and replace the entry.
4. Leave the `Remove` path at `:113` as is — it should still drop the entry.
5. Add a companion test asserting the *unchanged* case still reuses the GUID, so
   the retry property is pinned in both directions.
6. Re-run the existing watcher suite; the temp-file-plus-rename test and the
   debounce test must still pass unchanged.

## Acceptance Criteria

- Two different tracks written to the same path publish different `blockGuid`
  values.
- The same track re-published (duplicate event, or a rewrite with identical
  content) reuses its `blockGuid`.
- `eventGuid` is unchanged across all of the above.
- All eight existing tests in `tests/watcher.rs` still pass, unmodified.
- The `livevalue.rs` golden tests against `hgh-example-2.json` and
  `hgh-example-3.json` still pass.
- `cargo clippy --all-targets` clean.

## Test Commands

```bash
cd musicindex-live-publisher
cargo clippy --all-targets --offline
cargo test --offline
```

## Expected Final Report Format

- The identity representation you chose, and why.
- Confirmation that same-track reuse and different-track rotation are both
  covered by a test, named.
- Full `test result:` lines for every suite.
- Any existing test you had to modify, with justification. Modifying one is a
  yellow flag — say so explicitly.

## Escalation Triggers

- If you find yourself needing to change `payload_from_dropfile`'s signature,
  stop. The transform is pure by design and task 002 pinned that.
- If deriving identity from the parsed `DropFile` turns out to be ambiguous —
  for example if the producer emits a changing timestamp field per write — stop
  and report, because that would mean every write looks like a new track and the
  fix would rotate GUIDs on retries.
- If the existing rename test starts producing two payloads instead of one, stop:
  that indicates the debounce and identity logic are interacting, which needs a
  design decision rather than a patch.
