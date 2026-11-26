# Segment Frame Task 002: The Slot Text Writes The Segment Frame

Status: Implemented - 2026-11-26. The station check (criterion 4) passes (station log step 52).

Criteria 1 to 3 are mechanical. Criterion 4 is a station check, because no test runs Liquidsoap (ADR 0038
§Verification After Implementation). ADR 0038 has the visual items.

## Goal

The reference slot text `liquidsoap/musicindex-state.liq` writes the frame `musicindex.hls/2` of ADR 0038 at each
boundary, in place of the frame `musicindex.hls/1`. ADR 0038 §The Segment Frame and §The Slot Text Writes The Frames
own the rules. Read them first.

The station test of ADR 0038 used a test slot text with this change (station log step 51). The main session keeps
that text in `.idea/adr0038-test-slot.liq`. It is not in Git. Its insert block is below, for reference.

Install order: install the new slot text on the station only after task 001 runs on the station. Rainbrook then gets
the frame and the display state with the same `playId` together.

## Files To Read

- `docs/adr/0038-the-hls-stream-marks-each-segment.md`
- `liquidsoap/musicindex-state.liq`: the export line 10, `musicindex_id3` lines 12 to 32, `musicindex_state` line 38,
  the handlers lines 58 to 71, the last line 250
- `docs/runbooks/musicindex-producer-tutorial.md` near lines 157 to 175 and 210 to 222
- `docs/runbooks/test-station-log.md` step 51

## Files To Change

- `liquidsoap/musicindex-state.liq`
- `docs/runbooks/musicindex-producer-tutorial.md`: the items about the TXXX part and the HLS check describe
  `musicindex.hls/2` and cite ADR 0038. The check line says that each segment has a `musicindex` frame with
  `schema` `musicindex.hls/2` and a `playId`.

## Base components

| Component | Status | Location | Reason |
| --- | --- | --- | --- |
| Reference slot text | extend | liquidsoap/musicindex-state.liq | The present file has the state, the handlers and the TXXX export. The insert is the last operator of this file. |
| State reference | reuse | liquidsoap/musicindex-state.liq | `musicindex_state` holds seq, start time, metadata and jingle. The frame reads it and keeps no second copy. |
| HLS ID3 reader | reuse | scripts/hls_id3_dump.py | The station check reads the frames with it. |
| Station bundle | reuse | scripts/make-station-bundle.sh | It copies and checksums the file. It needs no change. |

## Rules For The Implementer

- Keep the line `settings.encoder.metadata.export` with `txxx`. Remove `musicindex_id3` and its `metadata.map`.
- Append the insert block after `thread.run(every=1., musicindex_write)`. It is then the last operator, after the
  handlers that write `state.json`.
- The block does these steps for each audio frame (ADR 0038 §The Slot Text Writes The Frames):
  - When `seq` changes and the metadata is not empty, it makes the list of boundaries: 0, and each `start_ms` and
    `end_ms` above 0 of the `podcast chapters` entries. A jingle gets only 0.
  - The position is `elapsed + cue_in` of the track, in milliseconds.
  - When one or more boundaries are at or before the position, it writes one frame for the largest of them, and
    removes them from the list.
  - `<ms>` comes from the text of `track_started`: `int_of_float(float_of_string(json.stringify(started)) * 1000.)`.
  - `playId` is `ls-<ms>-<seq>-<b>` for a file with one or more chapter entries, `ls-<ms>-<seq>` for a file with no
    chapters, and `null` for a jingle.
  - The inserted group has the non-empty `title`, `artist`, `album` and `song` of the file, and the `TXXX` frame. It
    uses `new_track=false`.
- Write comments in the style of the present file. Each comment cites ADR 0038 by its section.

Reference block from the station test (station log step 51):

```liquidsoap
musicindex_inner = radio
radio = insert_metadata(radio)
musicindex_frame = ref((-1, [], false))

def musicindex_boundaries(text) =
  entries =
    try
      let json.parse (e : [{start_ms: int?, end_ms: int?}]) = text
      e
    catch _ do
      []
    end
  values = list.flatten(list.map(fun (e) -> [null.get(default=0, e.start_ms), null.get(default=0, e.end_ms)], entries))
  (list.length(entries) > 0, list.filter(fun (b) -> b > 0, values))
end

def musicindex_on_frame() =
  let (seq, started, m, _, jingle) = musicindex_state()
  let (frame_seq, _, _) = musicindex_frame()
  if seq != frame_seq and list.length(m) > 0 then
    let (chapters, bounds) = musicindex_boundaries(m["podcast chapters"])
    musicindex_frame := (seq, if jingle then [0] else list.append([0], bounds) end, chapters)
  end
  let (frame_seq, left, chapters) = musicindex_frame()
  if frame_seq == seq and list.length(left) > 0 then
    pos = int_of_float((source.methods(musicindex_inner).elapsed() + float_of_string(default=0., m["liq_cue_in"])) * 1000.)
    passed = list.filter(fun (b) -> b <= pos, left)
    if list.length(passed) > 0 then
      musicindex_frame := (seq, list.filter(fun (b) -> b > pos, left), chapters)
      b = list.fold(fun (a, x) -> if x > a then x else a end, 0, passed)
      ms = int_of_float(float_of_string(json.stringify(started)) * 1000.)
      id = if chapters then "ls-#{ms}-#{seq}-#{b}" else "ls-#{ms}-#{seq}" end
      play_id = if jingle then null else id end
      frame = json.stringify(compact=true, {schema = "musicindex.hls/2", playId = play_id})
      keep = list.filter(fun (kv) -> snd(kv) != "", [("title", m["title"]), ("artist", m["artist"]), ("album", m["album"]), ("song", m["song"])])
      radio.insert_metadata(new_track=false, list.append(keep, [("TXXX", "musicindex\000#{frame}")]))
    end
  end
end
radio.on_frame(before=true, synchronous=true, musicindex_on_frame)
```

The test block also logged each frame at level 3. The reference text does not log each frame.

## Mechanical Criteria

1. `liquidsoap/musicindex-state.liq` has no `musicindex.hls/1` and no `metadata.map`. It has `musicindex.hls/2`, and
   the insert block is after the `thread.run` line. A `grep` in the report shows this.
2. The tutorial has no `musicindex.hls/1` in the description of the reference slot text.
3. `cargo build && cargo test`, `cargo clippy --all-targets -- -D warnings` and `cargo fmt -- --check` pass. The ADR
   guard test `tests/adr_guard.rs` passes.

## Station Check

The main session does this check, after task 001 runs on the station. It records a station log step.

4. `liquidsoap --check` of the new slot text in a copy of the generated `liquidsoap.liq` gives exit 0. After the
   install, for one episode, the list of frame positions in the HLS segments agrees with the list of boundaries in
   the chapter list, and each frame `playId` equals `play_id` in `display.json`.

## Rules For A Delegate

- Do not change `AGENTS.md`. The main session changes it when it commits the task.

- Apply the STE skill at `/home/citizen/.agents/skills/asd-ste100/SKILL.md` to comments and to the tutorial.
- Do not install anything on the station.
- Do not commit. Leave the change in the working tree and report it. The main session reads the diff, runs the
  checks and commits.
