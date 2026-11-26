# Liquidsoap Task 006: The Station Producer Pays Each Chapter Song

Status: Ready - 2026-11-22. ADR 0025 and its §The Chapter Route Lookup are accepted.

Every criterion is mechanical. No criterion is visual.

## Goal

`liquidsoap-now-playing` reads the state format 2 of ADR 0025. In an episode file with a chapter list, it writes one
drop file for each segment. ADR 0025 §What The Producer Writes gives the identity and the routes. The reference slot
text writes format 2 and reads only the new `Podcast` keys. The route service lookup is a different task. In this
task, the map of service results stays empty, so a remote split uses the routes of the file.

## Base components

| Component | Status | Location | Reason |
| --- | --- | --- | --- |
| Percentage of each recipient | reuse | ../musicindex-lib/value/src/lib.rs | `musicindex_value::percentages`, ADR 0075 R-VALUE-5. Use it at the pinned rev of `Cargo.toml`. |
| Payment route type | reuse | now-playing-drop/src/payment_route.rs | `PaymentRoute` holds each route of each group. |
| Route merge | new | now-playing-drop/src/merge.rs | Searched now-playing-drop/src, src/livevalue.rs, mixxx-now-playing/src and ../musicindex-lib/value. No code merges two value blocks. |
| State parse | extend | liquidsoap-now-playing/src/state.rs | Add format 2 and its three fields. |
| Drop file render | extend | liquidsoap-now-playing/src/lib.rs | `render_state` selects the segment. |
| Chapter list and position | new | liquidsoap-now-playing/src/chapters.rs | Searched liquidsoap-now-playing/src, mixxx-now-playing/src and ../musicindex-lib. No chapter or position code exists. |
| Slot text | extend | liquidsoap/musicindex-state.liq | Format 2 and the new keys. |

## Files To Inspect

- `docs/archive/adr/0025-the-station-producer-pays-each-chapter-song.md`. It owns each rule of this task. Read all of it.
- `docs/archive/adr/0022-liquidsoap-producer.md` §What The Producer Writes and §Jingle Mode.
- `liquidsoap-now-playing/src/lib.rs`, `liquidsoap-now-playing/src/state.rs`
- `now-playing-drop/src/lib.rs`, `now-playing-drop/src/payment_route.rs`
- `../musicindex-lib/value/src/lib.rs`
- `liquidsoap/musicindex-state.liq`

## Files To Change

- `Cargo.toml` (workspace root): no change to the root package. Add nothing there.
- `now-playing-drop/Cargo.toml`: add `musicindex-value = { git = "https://github.com/InTheMorning/musicindex-lib", rev = "e6d11118d33069c26c01368f7de71524988039c6" }`.
- `now-playing-drop/src/merge.rs` (new) and `now-playing-drop/src/lib.rs`
- `liquidsoap-now-playing/src/state.rs`, `liquidsoap-now-playing/src/lib.rs`
- `liquidsoap-now-playing/src/chapters.rs` (new)
- `liquidsoap/musicindex-state.liq`
- `Cargo.lock`

## Do Not Touch

- `src/**` (the publisher), `mixxx-now-playing/**`, `systemd/**`, `packaging/**`, `docs/**`, `AGENTS.md`.
- The drop file schema. It stays version 3. `Version3DropFile` does not change.

## Constraints

### The merge in `now-playing-drop/src/merge.rs`

Give two public functions. Each returns `Vec<PaymentRoute>`. A route is a fee route when `fee == Some(true)`.

1. `merged_routes(episode: &[PaymentRoute], song: &[PaymentRoute], remote_percentage: f64)`. ADR 0025 §The Merged
   Routes items 1 to 7:
   - `pe = percentages(episode splits)`. `F` is the total of `pe` of the episode fee routes. Each episode fee route
     keeps its `pe`.
   - `R = 100 - F`. When `episode` is empty, `F` is 0, so `R` is 100.
   - The song part is `remote_percentage * R / 100`. When the episode has no route with no fee, the song part is `R`.
   - The episode part is `(100 - remote_percentage) * R / 100`. `percentages` of the splits of the episode routes with
     no fee divides it. Calculate that again over those routes only.
   - `percentages` of the splits of all song routes, fee routes included, divides the song part.
   - The output order: the song routes, then the episode routes with no fee, then the episode fee routes. Each route
     keeps each field other than `split`.
   - A route with no percentage, or with a split that rounds to 0, is not in the output. Round each split to 3 decimal
     places.
2. `local_split_routes(episode: &[PaymentRoute], local: &[PaymentRoute])`. ADR 0025 §The Local Split: the episode fee
   routes keep `pe`. The local routes get `R = 100 - F`, divided with `percentages`. The episode routes with no fee
   get nothing. The same rounding and removal rules apply.
3. The caller does `remote_percentage` clamping. The merge functions take a value in 0 to 100.

### The state in `state.rs`

- `STATE_FORMAT_1 = "musicindex-liquidsoap-state/1"` and `STATE_FORMAT_2 = "musicindex-liquidsoap-state/2"`. The
  producer accepts the two. Each other format gives the error and the clear of today.
- `StateFile` gets `elapsed: Option<f64>`, `cue_in: Option<f64>` and `chapters: Option<String>`, each with
  `#[serde(default)]`. A format 1 file has none of them.

### The chapters in `chapters.rs`

- Parse `chapters` as a JSON array of `serde_json::Value`. Parse each entry alone, so that one bad entry does not stop
  the other entries. Text that is not a JSON array gives no chapters and one `tracing::error!` for each `seq`.
- An entry is not valid in these conditions. The producer ignores it and writes one `tracing::error!` for each `seq`:
  - `start_ms` or `end_ms` is missing or is not an integer of 0 or more.
  - `end_ms` is not above `start_ms`.
  - `remote_percentage` is present and is not a number. A missing or `null` value is 100. Clamp a number to 0..100.
  - Main-session choice: a local split with no percentage on its routes. Such a split cannot pay its recipients.
- A GUID that is `null`, empty or only spaces is missing. An entry with no feed GUID and no item GUID is a local split.
  Each other entry is a remote split.
- `title` and `artist` are a string or `null`. `null` or empty gives the state value. Main-session choice: musicindex
  ADR 0086 R-TAG-4 item 4 lets `title` be `null`.
- `value_routes` is a JSON array of routes, or `null`. For a remote split, these are the file routes of musicindex
  ADR 0089 R-TAG-6. `null` or missing gives no file routes. A value that is not an array of routes makes the entry not
  valid.
- Segments: sort the valid entries by `start_ms` with a stable sort. Of two entries with the same `start_ms`, keep the
  first in the list. An entry is active from its `start_ms` to `min(end_ms, next start_ms)`.
- The position is `round(1000 * (elapsed + cue_in))`. A missing or non-finite `elapsed` or `cue_in`, or a negative
  position, gives no segment: the file is one track for that read. A missing `cue_in` in format 2 is 0.
- The current chapter is the active entry with `start <= position < end`.

### The render in `lib.rs`

- The rows of ADR 0022 for not fresh, `live` and `jingle` apply first and do not change. A jingle ignores chapters.
- The episode group is usable when the state has the two GUIDs and its routes have at least one percentage. A remote
  split is identified when the entry has the two GUIDs and `remote_percentage` is above 0.
- The song routes of an identified split: the service result for the pair when it has a percentage, else the file
  routes when they have a percentage, else none (ADR 0025 §The Chapter Route Lookup).
- Apply the table of ADR 0025 §What The Producer Writes:
  - No chapter at the position, or no valid chapter: the episode GUIDs and the episode routes, as today.
  - An identified remote split with song routes: the chapter GUIDs, and `merged_routes` with the episode routes when
    the episode group is usable, and with an empty episode list when it is not.
  - An identified remote split with no song routes: the chapter GUIDs and no routes. The publisher sends the dead
    block.
  - A remote split that is not identified: the episode GUIDs and the episode routes.
  - A local split: the episode GUIDs. `local_split_routes` when the episode group is usable, and no routes when it is
    not.
- Keep ADR 0024: a drop file has routes only with a feed GUID and an item GUID in its identity.
- `title` and `artist` of a chapter come from the entry, with the fallback above. `album` and `image` come from the
  state.
- `play_id`: a file with no valid chapter keeps `ls-<track_started ms>-<seq>`. A file with at least one valid chapter
  uses `ls-<track_started ms>-<seq>-<segment start ms>`. A segment with no chapter starts at the end of the segment
  before it, or at 0.
- `value_routes_source` stays `"liquidsoap-state"` when routes are present.
- Each read writes the drop file again, as today. `OutputFile` writes only when the content changes.

### The slot text `liquidsoap/musicindex-state.liq`

- The header comment names `musicindex-liquidsoap-state/2` and ADR 0025.
- The recode exclude list is `["podcast value routes", "podcast chapters"]`. No old key.
- Read only the new keys: `podcast feed guid`, `podcast item guid`, `podcast value routes`, `podcast image`,
  `podcast description`, `podcast contributors` and `podcast chapters`. No `musicindex ...` key remains. The HLS ID3
  function also uses the new GUID keys. Keep its frame owner `musicindex` and its schema `musicindex.hls/1`.
- Add `elapsed = source.methods(radio).elapsed()`, `cue_in = float_of_string(default=0., m["liq_cue_in"])` and
  `chapters = m["podcast chapters"]`.

### General

- Follow the `rust-dev` skill. No `unwrap` or `expect` outside tests.
- Write each comment in Simplified Technical English (`/home/citizen/.agents/skills/asd-ste100/SKILL.md`).
- Do not commit. Leave the change in the working tree. The main session reads the diff and commits.

## Acceptance Criteria

All are mechanical. Each test is in the named crate and uses `tempfile` or pure values with an injected `Instant`.

`now-playing-drop` (merge):

1. The example of ADR 0025 §The Merged Routes item 8 gives 42.75, 42.75, 9.5 and 5.
2. `remote_percentage` 0, 50 and 100, each with the song list empty and with the episode list empty. The total of the
   splits is 100 within 0.01, or the output is empty.
3. Song shares `[0, 0]`, a missing share, a negative share, and one song route alone.
4. A small part that rounds to 0 is not in the output.
5. `local_split_routes` with one episode fee route of 5 and two local routes of 1 and 3 gives 5, 23.75 and 71.25.

`liquidsoap-now-playing`:

6. A state sequence plays an episode file through three chapters, a gap and the end of the file. The test examines
   each drop file: the identity, the routes, the title and the `play_id`.
7. The position 1 millisecond before, at, and 1 millisecond after the start and the end of a chapter, with `cue_in`
   12.5.
8. Two overlapping entries: the later entry starts at its `start_ms`. Two entries with the same `start_ms`: the first
   wins.
9. A remote split with `item_guid` null, with an empty GUID and with a GUID of spaces gives the episode identity and
   routes.
10. A remote split with the two GUIDs and an episode with one GUID gives the song routes at 100.
11. A local split with `remote_percentage` 0, 50 and 100 gives the same routes.
12. A chapter with `artist` null and `title` null gives the state artist and title.
13. A state of format 1 gives one track, as before. All the tests of today pass with no change of their assertions.
14. A `chapters` text that is not valid JSON gives one track.
15. `remote_percentage` of -5, 105, missing and `"x"`: clamped, clamped, 100 and not valid.
16. A jingle state with chapters gives the jingle drop file of ADR 0022.
17. An identified remote split with file routes and no service result, with a result for a different pair, and with
    an empty result gives the merged routes with the file routes.
18. A service result with routes wins over the file routes.
19. An identified remote split with `value_routes` null and no service result gives the chapter GUIDs and no routes.

Gate:

20. `cargo build`, `cargo test --workspace`, `cargo clippy --workspace --all-targets -- -D warnings` and
    `cargo fmt --all -- --check` pass.
21. `grep -n "musicindex " liquidsoap/musicindex-state.liq` shows no metadata key with the old prefix.

Operator choice (orchestrator, 2026-11-23, message after the station check of slot commit 6c1e14a): ADR 0025 Context
and §The Position From `written - track_started` are corrected in place. They now say that the slot callbacks are
synchronous and that late metadata gives a new `seq`. This is a correction of facts. No rule changes. ADR 0022 §The
State File Contract gets one sentence about the second `seq` change.
