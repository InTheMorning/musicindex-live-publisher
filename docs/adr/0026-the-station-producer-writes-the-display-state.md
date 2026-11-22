# ADR 0026: The Station Producer Writes The Display State With The ICY Line Of The File

- Status: Accepted on 2026-11-22, accepted by the operator as at commit ba8cf78. The orchestrator session relayed the
  decision. The four Open Questions stay open.
- Date: 2026-11-22
- Changes: ADR 0025 §The State File Format 2, by one optional field `song`. ADR 0022 §The State File Contract, the
  rule "A field change needs a new format version" (ADR 0022 line 77): an optional field does not need a new version.
  ADR 0022 §What The Producer Writes, by a display file adjacent to the drop file.
- Changes: ADR 0012 §The Producer and its invariant "`songLine` is the line that `butt` reads". `song_line` is the
  ICY `StreamTitle` of the stream for the track: the first line of `now-playing.txt` for Mixxx (ADR 0009), and the
  song line of §The Song Line for a station.
- Uses: ADR 0008 (the display path), the pairing of ADR 0012 by `play_id`, and ADR 0013 (the album), without a change.
- Uses: ADR 0025 §What The Producer Writes For An Episode File for the segments, the `title`, the `artist` and the
  `play_id`, without a change.
- Uses: `musicindex-live-relay` ADR 0005, the key `songLine` of a display state, without a change.
- Bar: crosses modules and repositories. The slot text, `liquidsoap-now-playing`, `now-playing-drop`, the publisher
  and the app `citizenradio` must agree on the line. An agent can also make the opposite choice by mistake: put the
  line of the chapter song in `songLine`, or change the ICY title for each chapter.
- Operator choice, 2026-11-22, relayed by the orchestrator session: a field of the relay display state lets the app
  show and pay each chapter song. The ICY title of the station does not change.
- Orchestrator choice, 2026-11-22, from the operator goal (show and pay each chapter song). The operator can
  change it. Each display state of the station has the artist and the title of the chapter song. The orchestrator rejected
  option B of the stop report: the artist and the title of the file. With option B, the app shows the episode and not
  the song.
- Orchestrator choice, 2026-11-22, from the operator goal (show and pay each chapter song). The operator can
  change it. The `citizenradio` session changes the app. This record does not change the app.

## Context

The app `citizenradio` holds the display states of the relay and releases one when its key agrees with the ICY title
of the stream (`CitizenRadio/Core/IcySync.swift`, lines 87-99). The key changed in `citizenradio` commit 7ef3d2e (app ADR 0024).
Now it is `songLine` of the display state when the state has one. Else it is `artist - title` (lines 119-126). The app uses one key for each state.

On a station, an episode file keeps one ICY title for the full file. ADR 0025 and its slot text do not change the ICY
title at a chapter. A display state with the artist and the title of a chapter song thus never agrees with the ICY
title.

A second fact: the station producer writes no display state. The publisher sends a track only from `display.json`
in the `display_dir` of a target (`src/main.rs`, line 214). It also sends a null state by itself when the producer
lock becomes free (`src/main.rs`, line 1147). `liquidsoap-now-playing` sets no
`display_dir`, and the station runbook gives none. On a station, the relay thus has no display state for a track.

`mixxx-now-playing` writes `display.json` with `song_line`, the line that `butt` sends as the ICY `StreamTitle` (ADR
0009 §The Song Line, ADR 0012). Relay ADR 0005 gives that line to the app as `songLine`. The same field can carry the
ICY title of the station file.

### How The Station Makes The ICY Title

These facts come from the sources in `musicindex-research/sources/broadcast-audio/streaming`: Liquidsoap 2.5.0 at
commit `c222ba2`, and AzuraCast 0.24.1 at commit `2a52da3`.

1. `output.icecast` sends the ICY metadata at each metadata change (`liquidsoap/src/core/outputs/icecast2.ml`, lines
   546-557). When the exported metadata has the key `song`, the ICY `song` is that value.
2. When the key `song` is missing, the function `icy_song` makes the value (`icecast2.ml`, lines 205-218):
   - `artist - title`, when `artist` and `title` are not empty,
   - else `artist`, when it is not empty,
   - else `title`, when it is not empty,
   - else no value, and the ICY title does not change.
3. AzuraCast exports the keys `artist`, `title`, `album` and `song`
   (`azuracast/util/docker/stations/liquidsoap/azuracast.liq`, line 12). It makes `output.icecast` with no
   `icy_song` argument (`backend/src/Radio/Backend/Liquidsoap/ConfigWriter.php`, lines 1255-1281), so item 2 applies.
4. AzuraCast sets `title` and `artist` from the media record of the file (`backend/src/Radio/AutoDJ/Annotations.php`,
   lines 100-113). A custom field of the station can add a key (line 112). A custom field with the name `song` thus
   changes the ICY title.
5. Jingle mode gives a jingle the metadata of the last song (`azuracast.liq`, lines 680-690). The ICY title of a
   jingle is thus the line of the last song.
6. The slot text operates in the Pre-Broadcast slot, after `azuracast.handle_jingle_mode`. The metadata that it sees
   at a track start is the metadata that `output.icecast` sees.
7. An empty value does not go to the output. `azuracast.liq` line 690 calls `metadata.map(strip=true, ...)`, and
   `strip` removes each key with an empty value (`liquidsoap/src/core/operators/map_metadata.ml`, lines 56-58). The
   slot text `metadata.map` also uses `strip`, which is true by default (`map_metadata.ml`, line 34). An empty `song`
   is thus the same as a missing `song`, in Liquidsoap and in the producer.
8. Liquidsoap sends ICY metadata by default for MP3, AAC, FLAC and WAV, and not for Ogg (`icecast2.ml`, lines
   363-376). This record applies to a mount with ICY metadata.

Inference: Icecast gives the ICY `song` value to the listener as `StreamTitle`. The test of §Verification item 1
examines it.

## Decision

### The Song Line

The **song line** of a file is the ICY title that the station sends for that file. The producer makes it from the
state, with the rule of §How The Station Makes The ICY Title items 1 and 2:

1. `song`, when it is not empty.
2. Else `artist - title`, when `artist` and `title` are not empty.
3. Else `artist`, when it is not empty.
4. Else `title`, when it is not empty.
5. Else no song line.

- The producer uses the strings of the state without a change. It does not trim them and does not change the case.
- The song line is a property of the file. Each chapter of the file has the song line of the file, not the line of the
  chapter song.

### The State File Format 2 Gets `song`

The slot text writes one more field in the format `musicindex-liquidsoap-state/2`:

| Field | Type | Use in the producer |
| --- | --- | --- |
| `song` | string | `m["song"]`. Empty when the metadata has no key `song`. |

- The field is optional. A missing field is the same as an empty field. A slot text of format 2 without `song` thus
  stays valid. The producer then makes the song line from items 2 to 5, and a station without the key `song` gets the
  correct line.
- The format version does not change. Format 2 is not on a station at the date of this record. The comment in the slot
  text that asks for a new version at each field change gets the exception for an optional field.
- Format 1 has no `song`. The producer makes the song line of a format 1 state from items 2 to 5.

### What The Producer Writes In `display.json`

The producer writes `display.json` with the schema `musicindex.display/3` of ADR 0013 in the display directory of its
target. It writes the file only when the target has `display_dir`. The ADR 0021 rule for the path does not change: the
path is the folder `display` in the run directory of the instance.

The first row that applies wins:

| State | `display.json` |
| --- | --- |
| The producer starts | `{"track": null}` (ADR 0008) |
| The producer stops | `{"track": null}`, before it exits (ADR 0008) |
| Not fresh, or missing, or `live` is `true` | `{"track": null}`, one time, when the producer releases the lock |
| Fresh, but the producer cannot parse it | No change, as the drop file (ADR 0022) |
| No song line, or a song line of more than 1,024 characters | `{"track": null}`. Relay ADR 0005 refuses a longer line. |
| `jingle` is `true` | A track with the artist and the title of the state, the song line of the state, and `play_id` `null` |
| Each other state with a drop file | A track. See the table below. |

A track has these fields:

| Field | Value |
| --- | --- |
| `artist` | The `artist` of the drop file of the same segment (ADR 0025 §What The Producer Writes For An Episode File). |
| `title` | The `title` of the drop file of the same segment. |
| `album` | The `album` of the state, or `null`. |
| `artwork` | `{"url": ...}` when `image_url` accepts the `image` of the state: HTTP or HTTPS, and at most 2,048 characters (relay ADR 0003). Else `null`. |
| `song_line` | The song line of the file (§The Song Line). |
| `play_id` | The `play_id` of the drop file of the same segment. |

- A **song file without chapters** gives one track for the file. Its artist and title are the artist and the title of
  the state. Its song line is `artist - title` of the state, unless the key `song` gives a different line.
- An **episode file with chapters** gives one track for each segment of ADR 0025. Each track has the artist and the
  title of its segment, and the same song line: the line of the file.
- A **jingle** gives the line of the last song, because jingle mode gives the jingle the metadata of the last song. The
  track has `play_id` `null`, so the publisher adds no `value` (ADR 0012). The drop file of the jingle pays nobody
  (ADR 0022). After an episode file, the jingle track shows the artist and the title of the episode, not of the last
  chapter song. The show log gets a `display` entry for this track. It has `play_id` `null` and is not a play
  (ADR 0022 §Jingle Mode).
- An **identified remote split with no song routes** shows the chapter song and pays nobody. Its drop file gives the
  dead block, which has `play_id` `null` (`src/livevalue.rs`, line 321). The display state thus gets no `value`.
- A **remote split that is not identified** shows the chapter song and pays the episode (ADR 0025).
- The producer writes `display.json` in the same read as the drop file, after the drop file.
- A late answer of the route service (ADR 0025 §The Chapter Route Lookup) changes the drop file with the same
  `play_id`. The display file does not change. Two cases:
  - The first payload had file routes. The display state already has `value`, and the late answer keeps the
    `blockGuid`. No second display state is necessary.
  - The first payload was the dead block. The publisher sends the display state again, one time, with `value`
    (ADR 0012 §The Publisher, `src/relay.rs` lines 1566-1577).
- The producer writes no embedded image. The state has no picture data.

### One Display Writer

The display writer of `mixxx-now-playing/src/display.rs` moves to `now-playing-drop`. The two producers use it. The
moved parts are `DisplayOutput`, `render_display_json`, `ShownTrack`, `Artwork`, `image_url`, the image types and
`write_null_at_exit`. The parts that read and prepare a picture (`prepare_image`, `embedded_artwork`, `file_artwork`)
stay in `mixxx-now-playing`, so `now-playing-drop` does not get the crate `image`. No copy of a moved part stays.

- `ShownTrack.play_id` changes from `Option<i64>` to a string. The station `play_id` is `ls-...` (ADR 0025). Mixxx
  gives the history row ID as a string, so its `display.json` does not change.
- The tests of the moved parts move to `now-playing-drop` with the code.

### The Publisher And The Relay

- The publisher does not change. It reads `display.json` of the target and adds `songLine` and `value` as ADR 0012
  says.
- The relay does not change. Relay ADR 0005 accepts `songLine` of 1 to 1,024 characters. Its definition names the
  line of `butt`. The relay session can make it general in its own record.

### The Runbook

The station runbook adds `display_dir` to the target of the configuration, with the path of ADR 0021:

```toml
display_dir = "/run/user/UID/musicindex-live-publisher/INSTANCE/display"
```

## Invariants

These rules apply while this decision is current.

- Each display state of a file has the song line of that file. A chapter state never has the line of its chapter
  song.
- The producer never changes the ICY title.
- A display state has the `play_id` of the drop file of the same segment, or `null` for a jingle.
- The station producer writes a track only while it holds the producer lock.
- The producer writes `display.json` after the drop file of the same read.

## Verification

Mechanical, in `liquidsoap-now-playing` with `tempfile` and an injected clock:

- A chapter sequence of ADR 0025 §Verification gives one display state for each segment. Each state has the artist and
  the title of its drop file, the `play_id` of its drop file, and the same `song_line`: `artist - title` of the state.
- A state with `song` not empty gives that value as `song_line` for each segment.
- A state with only `artist`, with only `title`, and with neither gives `artist`, `title`, and `{"track": null}`.
- A song file without chapters gives one display state with the song line `artist - title`.
- A jingle gives the artist, the title and the song line of the state, and `play_id` `null`.
- `live` `true` gives `{"track": null}`.
- A target without `display_dir` gives no `display.json`.
- A format 2 state without the field `song` parses.
- A song line of 1,025 characters gives `{"track": null}`.
- An `image` that is not an HTTP or HTTPS URL gives the artwork `null`.
- The producer start gives `{"track": null}`. The producer stop gives `{"track": null}` before the exit.

Mechanical, in `now-playing-drop` and `mixxx-now-playing`: the tests of the display file pass after the move. A
`play_id` of the history row ID gives the same `display.json` as before.

Manual, in the last review of the diff: the moved tests keep their assertions. A test cannot compare a test with its
earlier version.

Manual, on the test station `heycitizens_test_radio`, with a person:

1. A chaptered episode file plays. The ICY title of the stream equals the `songLine` of each chapter state on the relay
   (`GET /v1/liveitems/EVENT_ID/display`). This check also proves the inference of §How The Station Makes The ICY Title.
2. A song file without chapters plays. The ICY title equals the `songLine` of its state.
3. The app at `citizenradio` commit 7ef3d2e or later shows each chapter song.

## Open Questions

1. **The null state at live.** The DJ publisher of ADR 0023 can send a display state on the same event. `live`
   becomes `true` after the DJ is on air. The station publisher also sends a null state by itself when the lock
   becomes free (`src/main.rs`, line 1147). So a null state from the station can come after the state of the DJ and
   remove it. The same race occurs at the end of the set. The restore rule of ADR 0023 covers value blocks only, not
   display states. The station check of ADR 0023 with a DJ publisher examines it.
2. **The character set of the station.** AzuraCast converts the ICY title to the character set of the station
   (`ConfigWriter.php`, line 1262). Inference: with a character set that is not UTF-8, a character that the set does
   not have changes, and the ICY title then does not equal `songLine`. The app normalizes the two texts, but it does
   not correct a lost character.
3. **Metadata in the middle of a track.** AzuraCast can add metadata during a track (`ConfigWriter.php`, line 683,
   `add_custom_metadata_command`). The ICY title then changes, and `state.json` does not. The display state then does
   not agree with the ICY title until the next track.
4. **The time of a chapter change.** Each chapter state of a file has the same song line. The ICY title thus does not
   give the time of a chapter change. The relay sends a display state at once (relay ADR 0004), and the listener hears
   the audio later, by the stream buffer. Inference: the app shows the next chapter song and pays its routes early, by
   that buffer. This question is related to the listener delay of ADR 0022 Open Question 2. This record does not
   decide it.

## Alternatives Considered

### The Artist And The Title Of The File

Rejected by the orchestrator. Each display state has the artist and the title of the file. The app then agrees with the
ICY title with no app change, but it shows the episode and not the chapter song.

### A New ICY Title For Each Chapter

Rejected by the operator. The station sends a new ICY title at each chapter. This needs a change of the AzuraCast
metadata path, and other listeners of the stream see the chapter songs as tracks.

### The Producer Makes Only `artist - title`

Rejected. It is the correct line for most stations, but a custom field `song` gives a different ICY title. The field
`song` costs one line in the slot text.

### A Format 3 For The Field `song`

Rejected. Format 2 is not on a station, and the field is optional. A format 3 adds a version check and gives no
protection.

## Consequences

- A station sends a display state for each segment. The relay then has a state for each track and each chapter.
- The app shows a chapter song from `citizenradio` commit 7ef3d2e. An older app uses `artist - title` and agrees
  only for a song file without chapters.
- The station runbook gets one more configuration line.

## References

- `docs/adr/0008-display-path.md`
- `docs/adr/0012-pair-display-state-with-value-block.md`
- `docs/adr/0022-liquidsoap-producer.md`
- `docs/adr/0025-the-station-producer-pays-each-chapter-song.md`
- `musicindex-live-relay`: `docs/adr/0005-display-song-line-and-value.md`
- `citizenradio`: `CitizenRadio/Core/IcySync.swift`
