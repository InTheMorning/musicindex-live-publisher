# ADR 0026: The Station Producer Writes The Display State With The ICY Line Of The File

- Status: Proposed
- Date: 2026-11-22
- Changes: ADR 0022 §The State File Contract, by one optional field in the format 2 of ADR 0025 §The State File
  Format 2. ADR 0022 §What The Producer Writes, by a display file adjacent to the drop file.
- Uses: ADR 0008 (the display path), ADR 0012 (the pairing by `play_id`) and ADR 0013 (the album), without a change.
- Uses: ADR 0025 §What The Producer Writes For An Episode File for the segments, the `title`, the `artist` and the
  `play_id`, without a change.
- Uses: `musicindex-live-relay` ADR 0005, the key `songLine` of a display state, without a change.
- Bar: crosses modules and repositories. The slot text, `liquidsoap-now-playing`, `now-playing-drop`, the publisher
  and the app `citizenradio` must agree on the line. An agent can also make the opposite choice by mistake: put the
  line of the chapter song in `songLine`, or change the ICY title for each chapter.
- Operator choice, 2026-11-22, relayed by the orchestrator session: a field of the relay display state lets the app
  show and pay each chapter song. The ICY title of the station does not change.
- Operator choice, 2026-11-22, relayed by the orchestrator session: each display state of the station has the artist
  and the title of the chapter song. The operator rejected option B of the stop report: the artist and the title of
  the file. With option B, the app shows the episode and not the song.
- Operator choice, 2026-11-22, relayed by the orchestrator session: the `citizenradio` session changes the app. The
  app compares the ICY title with `songLine` first, and then with `artist - title`. This record does not change the
  app.

## Context

The app `citizenradio` holds the display states of the relay and releases one when its key agrees with the ICY title
of the stream (`CitizenRadio/Core/IcySync.swift`, lines 87-98). The key is `artist - title` of the display state. The
app does not read `songLine` at the date of this record.

On a station, an episode file keeps one ICY title for the full file. ADR 0025 and its slot text do not change the ICY
title at a chapter. A display state with the artist and the title of a chapter song thus never agrees with the ICY
title.

A second fact: the station producer writes no display state. The publisher sends a display state only from
`display.json` in the `display_dir` of a target (`src/main.rs`, line 214). `liquidsoap-now-playing` sets no
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
   lines 99-111). A custom field of the station can add a key (line 110). A custom field with the name `song` thus
   changes the ICY title.
5. Jingle mode gives a jingle the metadata of the last song (`azuracast.liq`, lines 680-690). The ICY title of a
   jingle is thus the line of the last song.
6. The slot text operates in the Pre-Broadcast slot, after `azuracast.handle_jingle_mode`. The metadata that it sees
   is the metadata that `output.icecast` sees.

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

| State | `display.json` |
| --- | --- |
| Not fresh, or missing, or not readable, or `live` is `true` | `{"track": null}`, one time, when the producer releases the lock |
| A song line, and a drop file | A track. See the table below. |
| A song line, and `jingle` is `true` | A track with the artist and the title of the state, the song line of the state, and `play_id` `null` |
| No song line | `{"track": null}` |

A track has these fields:

| Field | Value |
| --- | --- |
| `artist` | The `artist` of the drop file of the same segment (ADR 0025 §What The Producer Writes For An Episode File). |
| `title` | The `title` of the drop file of the same segment. |
| `album` | The `album` of the state, or `null`. |
| `artwork` | `{"url": ...}` when the `image` of the state is an HTTP or HTTPS URL. Else `null`. |
| `song_line` | The song line of the file (§The Song Line). |
| `play_id` | The `play_id` of the drop file of the same segment. |

- A **song file without chapters** gives one track for the file. Its artist and title are the artist and the title of
  the state. Its song line is `artist - title` of the state, unless the key `song` gives a different line.
- An **episode file with chapters** gives one track for each segment of ADR 0025. Each track has the artist and the
  title of its segment, and the same song line: the line of the file.
- A **jingle** gives the line of the last song, because jingle mode gives the jingle the metadata of the last song. The
  track has `play_id` `null`, so the publisher adds no `value` (ADR 0012). The drop file of the jingle pays nobody
  (ADR 0022).
- The producer writes `display.json` in the same read as the drop file, after the drop file. A late answer of the route
  service (ADR 0025 §The Chapter Route Lookup) changes the drop file with the same `play_id`. The display file does not
  change, and the publisher sends the display state again with `value` (ADR 0012 §The Publisher).
- The producer writes no embedded image. The state has no picture data.

### One Display Writer

The display writer of `mixxx-now-playing/src/display.rs` moves to `now-playing-drop`. The two producers use it. The
moved parts are `DisplayOutput`, `render_display_json`, `ShownTrack`, `Artwork`, the image types and
`write_null_at_exit`. The Mixxx parts that read a picture from a file stay in `mixxx-now-playing`. No copy of a moved
part stays.

### The Publisher And The Relay

- The publisher does not change. It reads `display.json` of the target and adds `songLine` and `value` as ADR 0012
  says.
- The relay does not change. Relay ADR 0005 accepts `songLine` of 1 to 1,024 characters.

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

Mechanical, in `mixxx-now-playing`: the tests of the display file pass after the move, with no change of their
assertions.

Manual, on the test station `heycitizens_test_radio`, with a person:

1. A chaptered episode file plays. The ICY title of the stream equals the `songLine` of each chapter state on the relay
   (`GET /v1/liveitems/EVENT_ID/display`). This check also proves the inference of §How The Station Makes The ICY Title.
2. A song file without chapters plays. The ICY title equals the `songLine` of its state.
3. After the `citizenradio` change, the app shows each chapter song at its time.

## Open Questions

1. **The null state at live.** The DJ publisher of ADR 0023 can send a display state on the same event. A null state
   from the station publisher can then remove the state of the DJ. Recommendation: the station producer writes the
   null state when it releases the lock, before the DJ is on air. The station check of ADR 0023 with a DJ publisher
   examines it.
2. **The character set of the station.** AzuraCast converts the ICY title to the character set of the station
   (`ConfigWriter.php`, line 1262). Inference: with a character set that is not UTF-8, a character that the set does
   not have changes, and the ICY title then does not equal `songLine`. The app normalizes the two texts, but it does
   not correct a lost character.
3. **A key `song` that is present and empty.** Liquidsoap then sends an empty ICY title. The slot text cannot
   distinguish an empty key from a missing key, so the producer uses items 2 to 5. Inference: AzuraCast does not set an
   empty key `song`.

## Alternatives Considered

### The Artist And The Title Of The File

Rejected by the operator. Each display state has the artist and the title of the file. The app then agrees with the
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
- The app needs the `citizenradio` change before it shows a chapter song. Before that change, a song file without
  chapters already agrees by `artist - title`.
- The station runbook gets one more configuration line.

## References

- `docs/adr/0008-display-path.md`
- `docs/adr/0012-pair-display-state-with-value-block.md`
- `docs/adr/0022-liquidsoap-producer.md`
- `docs/adr/0025-the-station-producer-pays-each-chapter-song.md`
- `musicindex-live-relay`: `docs/adr/0005-display-song-line-and-value.md`
- `citizenradio`: `CitizenRadio/Core/IcySync.swift`
