# Relay Title Task 001: The Dead Block Title Setting And The "Artist - Title" Track Title

Status: Complete - 2026-11-24. The main session reviewed the diff and ran the gate. The Podcast Guru check of ADR 0029 §Verification is open. ADR 0029 owns the rules. The operator accepted the two choices on 2026-11-24.

All criteria are mechanical. The main session moves ADR 0005 to the archive and changes the other documents. This task
changes only the files in §Files To Change.

## Goal

1. Each target has the optional setting `dead_block_title`. The dead block of that target has that title. With no
   setting, the title is "No V4V track playing" (ADR 0029 §The Dead Block).
2. The `title` of a payable track payload is "artist - title" (ADR 0029 §The Title Of A Track Block).

Read `docs/adr/0029-producer-liveness-dead-block-and-remote-value-title.md` first, all sections.

## Base components

| Component | Status | Location | Reason |
| --- | --- | --- | --- |
| Dead block builder | extend | src/livevalue.rs | `dead_payload` gets the title as a parameter. The model and the destination stay constants. |
| Track payload builder | extend | src/livevalue.rs | `payload_from_dropfile` makes the "artist - title" text. |
| Target configuration | extend | src/config.rs | `RawTarget` and `PublisherTarget` get `dead_block_title`, with the validation of the other target fields. |
| Watcher target | extend | src/watcher.rs | `WatchTarget` carries the resolved title to each `dead_payload` call. |

## Steps

1. In `src/livevalue.rs`:
   - Make `DEAD_BLOCK_TITLE` public as `DEFAULT_DEAD_BLOCK_TITLE`, with the value "No V4V track playing". Change its doc
     comment: it is the title when the target has no `dead_block_title` (ADR 0029).
   - Change `dead_payload(event_guid, block_guid)` to `dead_payload(event_guid, block_guid, title: &str)`. Change its
     doc comment: the model and the destination are constants, and the title comes from the target (ADR 0029). Each
     other field stays.
   - In `payload_from_dropfile`, set `title` to the trimmed artist and the trimmed title joined with " - ". When one of
     them is empty after the trim, use the other one alone. Add a short private function for this text, with a doc
     comment that names ADR 0029 §The Title Of A Track Block. Each other field stays, also `line` and `author`.
2. In `src/watcher.rs`:
   - Add `pub dead_block_title: String` to `WatchTarget`.
   - Pass `&target.dead_block_title` to each `dead_payload` call, also through `dead_payload_with_log`.
3. In `src/config.rs`:
   - Add `dead_block_title: Option<String>` to `RawTarget` (and to the other raw target struct that the loader uses, if
     it parses the same stanza).
   - Add `pub dead_block_title: String` to `PublisherTarget`, and to its `Debug` output.
   - Resolve the value where the loader resolves `stream_delay_secs`: absent gives `DEFAULT_DEAD_BLOCK_TITLE`. Trim
     the value. An empty value after the trim, or a value of more than 256 characters (`chars().count()`), is an
     error with the text `target NAME dead_block_title ...` that names the fault. Add the constant
     `MAX_DEAD_BLOCK_TITLE_CHARS = 256`.
   - `PublisherTarget::watch_target` copies the title.
   - Do not change `TargetConfigEdit`, the summaries, `config show` or the stanza editor.
4. Change each `WatchTarget { ... }` literal and each `dead_payload(...)` call in `src/main.rs`, `tests/watcher.rs`,
   `tests/showlog.rs` and `tests/relay.rs`. Use `DEFAULT_DEAD_BLOCK_TITLE` where a test does not test the setting.
   Export `DEFAULT_DEAD_BLOCK_TITLE` from the crate root adjacent to `dead_payload`.
5. The tests that assert the title "No V4V track playing" stay correct with the default. Change a test that asserts
   the track payload `title` to the new text.
6. Add the tests of §Acceptance Criteria.
7. Set the status line of this file to `Implemented - 2026-11-24, pending review`.

## Files To Change

- `src/livevalue.rs`, `src/watcher.rs`, `src/config.rs`, `src/lib.rs` (export only), `src/main.rs`.
- `tests/watcher.rs`, `tests/showlog.rs`, `tests/relay.rs`, and a test file of the configuration if one exists.
- This task file, status line only.

## Acceptance Criteria

Mechanical. The test names are suggestions:

1. `dead_block_title_defaults_when_the_setting_is_absent`: a configuration with no `dead_block_title` gives
   `PublisherTarget.dead_block_title == "No V4V track playing"`.
2. `dead_block_title_is_read_and_trimmed`: `dead_block_title = "  HeyCitizen Test Stream "` gives "HeyCitizen Test
   Stream".
3. `empty_or_long_dead_block_title_is_an_error`: `""`, `"   "` and a value of 257 characters each give an error that
   contains the target name and `dead_block_title`. A value of 256 characters is accepted.
4. `dead_block_with_a_title_keeps_its_model_and_destination`: `dead_payload("e", "b", "Station")` has the title
   "Station", and its `value` is equal to the `value` of `dead_payload("e", "b", DEFAULT_DEAD_BLOCK_TITLE)`. It has no
   `image`, `feedGuid` or `itemGuid`.
5. A watcher test with two targets, one with the title "HeyCitizen Test Stream" and one with the default: the dead
   block of each target has its own title.
6. `track_payload_title_is_artist_dash_title`: the artist "Anni Powell" and the title "Beyond the Hills" give "Anni
   Powell - Beyond the Hills". An empty artist gives "Beyond the Hills". `line` and `author` do not change.

Gate:

- `cargo build --workspace`, `cargo test --workspace`, `cargo clippy --workspace --all-targets -- -D warnings` and
  `cargo fmt --all -- --check` pass.

## Rules For The Delegate

- Write each code comment in Simplified Technical English. The skill is `/home/citizen/.agents/skills/asd-ste100/SKILL.md`.
- Do not commit. Leave the change in the working tree and report it.
- Do not change an ADR, `AGENTS.md`, `docs/README.md` or a runbook.
- If a step needs a decision that ADR 0029 does not give, stop and report it.
