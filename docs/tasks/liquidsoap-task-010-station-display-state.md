# Liquidsoap Task 010: The Station Producer Writes The Display State

Status: Ready - 2026-11-22. ADR 0026 is accepted. Task 009 is complete: the display writer is in
`now-playing-drop/src/display.rs`.

All criteria are mechanical. No criterion is visual. The station checks of ADR 0026 §Verification are for a person
and are not in this task.

## Goal

`liquidsoap-now-playing` writes `display.json` in the display directory of its target, with the song line of the file
(ADR 0026). The slot text writes the field `song`. The station runbook gives `display_dir`.

## Base components

| Component | Status | Location | Reason |
| --- | --- | --- | --- |
| Display writer | reuse | now-playing-drop/src/display.rs | `DisplayOutput`, `ShownTrack`, `Artwork` and `image_url` of task 009. Do not make a second writer. |
| Instance settings | reuse | now-playing-drop/src/instance.rs | `InstanceSettings.display_dir` is the ADR 0021 path, or `None`. |
| State file parser | extend | liquidsoap-now-playing/src/state.rs | `StateFile` gets the optional field `song`. |
| Producer and drop file | extend | liquidsoap-now-playing/src/lib.rs | `Producer` and `render_state` give the display state from the drop file of the same read. The drop file does not change. |
| Slot text | extend | liquidsoap/musicindex-state.liq | One more field `song` in the JSON of `musicindex_write`. |
| Station runbook | extend | docs/runbooks/liquidsoap-now-playing-azuracast.md | §Write The Configuration gets `display_dir`. |

## Files To Inspect

- `docs/adr/0026-the-station-producer-writes-the-display-state.md`, all of §Decision, §Invariants and §Verification.
  It owns each rule of this task.
- `docs/adr/0025-the-station-producer-pays-each-chapter-song.md` §What The Producer Writes For An Episode File and
  §Verification (the chapter sequence that the tests use).
- `liquidsoap-now-playing/src/lib.rs` (the tests too), `liquidsoap-now-playing/src/state.rs`,
  `liquidsoap-now-playing/src/main.rs`.
- `now-playing-drop/src/display.rs`, `now-playing-drop/src/instance.rs`.
- `mixxx-now-playing/src/main.rs`, the function `write_display`, for the error rule.
- `docs/runbooks/musicindex-live-publisher-configuration.md` lines 90-130, for the target field `display_dir`.

## Files To Change

- `liquidsoap-now-playing/src/state.rs`, `liquidsoap-now-playing/src/lib.rs`, `liquidsoap-now-playing/src/main.rs`.
- `liquidsoap/musicindex-state.liq`.
- `docs/runbooks/liquidsoap-now-playing-azuracast.md`, only §Write The Configuration.
- `liquidsoap-now-playing/Cargo.toml` and `Cargo.lock` only if a dependency is necessary.

## Do Not Touch

- `src/**` (the publisher), `now-playing-drop/**`, `mixxx-now-playing/**`, `systemd/**`, `packaging/**`,
  `docs/adr/**`, `AGENTS.md`.
- The drop file. Each test of today passes with no change of an assertion.
- The format version of the state file. It stays `musicindex-liquidsoap-state/2`.

## Constraints

### The state file

- `StateFile` gets `#[serde(default)] pub song: String`. A missing field gives an empty string.

### The song line (ADR 0026 §The Song Line)

- A function, for example `fn song_line(state: &StateFile) -> Option<String>`, uses the first row that applies:
  1. `song`, when it is not empty.
  2. `artist - title`, when the two are not empty.
  3. `artist`, when it is not empty.
  4. `title`, when it is not empty.
  5. `None`.
- It uses the strings without a change. It does not trim and does not change the case.
- A song line of more than 1,024 characters (`chars().count()`) gives the display state `null`.

### The display state (ADR 0026 §What The Producer Writes In `display.json`)

- `Producer` holds `Option<DisplayOutput>`: `Some` only when `settings.display_dir` is `Some`. `Producer::new` does no
  I/O.
- A new `pub fn start(&mut self) -> Result<()>` calls `DisplayOutput::start` (the null state at start). `main.rs`
  calls it before the first poll. Without a display directory, it does nothing.
- `clear()` writes the display state `null` after it removes the drop file and releases the lock. `shutdown()` thus
  writes `null` before the exit. `DisplayOutput` writes no file when the content does not change, so the null state
  goes out one time.
- A state that the producer cannot parse while fresh (`keep_while_fresh`) does not change `display.json`.
- After `self.output.set(Presence::Present(..))`, and only then, the producer writes the track. The `artist`, the
  `title` and the `play_id` come from the drop file of the same read. Do not compute them a second time. For example,
  `render_state` can return them with the drop file content.
- A jingle: the `artist` and the `title` of the state, the song line of the state, `play_id` `None`.
- `album`: the `album` of the state when it is not empty, else `None`.
- `artwork`: `Artwork::Url` when `image_url` accepts the `image` of the state, else `None`. No embedded image.
- A display write error gives one `tracing::warn!` line with the directory and the error, and the producer continues,
  as `write_display` in Mixxx. It never stops the drop file.
- Each code comment is in Simplified Technical English and names the ADR rule. Read
  `/home/citizen/.agents/skills/asd-ste100/SKILL.md`.

### The slot text

- In `musicindex_write`, add `song = m["song"]` after `artist = m["artist"]`.
- Change line 3 of the comment to this text: `# liquidsoap-now-playing reads that file. A new required field needs a
  new format version. An optional field does not (ADR 0026).`

### The runbook

- In §Write The Configuration, after the `target add` step and before the `config show` step, add one step in STE:
  open `~/.config/musicindex-live-publisher/INSTANCE/config.toml`, and in the `[[target]]` table with the name
  `default`, add this line:

  ```toml
  display_dir = "/run/user/UID/musicindex-live-publisher/INSTANCE/display"
  ```

  Say that the producer writes `display.json` there (ADR 0026), that the unit makes the folder, and that without the
  line the relay gets no display state. Replace `UID` as in the step before.
- Run `python3 ~/.agents/skills/asd-ste100/scripts/ste_lint.py --no-heuristics` on the runbook. Correct a finding in
  the new text when it is not a technical name.

### The tests (ADR 0026 §Verification)

Put them in the test module of `liquidsoap-now-playing/src/lib.rs`, with `tempfile` and an injected `Instant`. Use a
`settings` helper with `display_dir: Some(..)`. Each test reads `display.json` and the drop file. Write one test for
each item:

1. The chapter sequence of ADR 0025 §Verification gives one display state for each segment. Each state has the
   `artist`, the `title` and the `play_id` of the drop file of the same read, and the same `song_line`: `artist -
   title` of the state.
2. A state with `song` not empty gives that value as `song_line` for each segment.
3. A state with only `artist` gives `artist`. A state with only `title` gives `title`. A state with neither gives
   `{"track": null}`.
4. A song file without chapters gives one display state with the song line `artist - title`.
5. A jingle gives the `artist`, the `title` and the song line of the state, and `play_id` `null`.
6. `live` `true` gives `{"track": null}`.
7. A target without `display_dir` gives no `display.json`.
8. A format 2 state without the field `song` parses, and gives the song line `artist - title`.
9. A song line of 1,025 characters gives `{"track": null}`. A song line of 1,024 characters gives a track.
10. An `image` that is not an HTTP or HTTPS URL gives `"artwork": null`. An HTTPS `image` gives `{"url": ...}`.
11. `start` gives `{"track": null}`. After a track, `shutdown` gives `{"track": null}`.
12. A state that does not parse while fresh, after a track, keeps the `display.json` of the track.

## Acceptance Criteria

1. Mechanical: the 12 tests of §The tests pass.
2. Mechanical: each test of today in `liquidsoap-now-playing` passes with no change of an assertion.
3. Mechanical: `grep -n 'song = m\["song"\]' liquidsoap/musicindex-state.liq` shows one line.
   `grep -n 'display_dir = "/run/user/UID/musicindex-live-publisher/INSTANCE/display"'
   docs/runbooks/liquidsoap-now-playing-azuracast.md` shows one line.
4. Mechanical: `cargo build`, `cargo test --workspace`, `cargo clippy --workspace --all-targets -- -D warnings` and
   `cargo fmt --all -- --check` pass.
