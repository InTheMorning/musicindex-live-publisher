# ADR 0022: A Producer For Liquidsoap And AzuraCast

> Archived: superseded by [ADR 0035](0035-the-station-producer.md) on 2026-11-26. ADR 0035 restates
> each live rule of this record.

- Status: Superseded by ADR 0035 on 2026-11-26. Accepted on 2026-11-21 by the operator. The orchestrator session relayed the decision. Open Question 2
  stays open.
- Date: 2026-11-21
- Bar: crosses repositories. The Liquidsoap slot text in the research tutorial writes the input of this producer, and
  the station operator installs it on a server. An agent can also make the opposite choice by mistake: read the media
  files, or exit when Liquidsoap stops.
- Operator choice, 2026-11-21: a bind mount gives the host the state file. The orchestrator session relayed the
  choice.
- Operator choice, 2026-11-21: `state.json` holds all keys, so the producer does not read the media files.
- Operator choice, 2026-11-21: the producer and the publisher operate as one user account with lingering.
- Revised 2026-11-21, while Proposed: the routes row needs `feed_guid` also (ADR 0024). No decision was reversed.
- Revised 2026-11-21, while Proposed: the format gets the field `jingle`, and §Jingle Mode replaces the operator rule
  with a producer rule. The orchestrator session sent the evidence. The format is not implemented, so the version stays
  1. No decision was reversed.
- Revised 2026-11-21, after acceptance: §The Template Unit states how an AzuraCast short name with `_` becomes
  the instance name. HitIt asked the question. No decision was reversed.
- Changed by: ADR 0025, accepted 2026-11-22. A state format 2 with `elapsed`, `cue_in` and `chapters`, and the
  chapter rows of §What The Producer Writes. The slot text reads only the new `Podcast` frame names.
- Changed by: ADR 0027, accepted 2026-11-23. A song state with the two GUIDs gets the routes of the route service,
  else the routes of the state.
- Changed by: ADR 0026, accepted 2026-11-22. An optional field of the state file does not need a new format version.
  The producer writes `display.json` adjacent to the drop file.
- Revised 2026-11-22, after acceptance: the test of Open Question 1 passed, so the operator rule of §Jingle Mode is
  removed. The orchestrator session ran the test. No decision was reversed.

## Context

A station on AzuraCast plays tracks from Liquidsoap 2.4.6 in a container. The research session tested these facts on
the HeyCitizen test station (`musicindex-research`, `research_notes/azuracast/track-events-for-a-producer.md`, commit
f7217f4):

- Liquidsoap reads the six MusicIndex keys from the file tags.
- A slot text in the Pre-Broadcast slot writes a file atomically at each track change.
- Only root can read the Docker volume on the host.

The tutorial in the same folder (`musicindex-producer-tutorial.md`) changes the tested slot text. The new text writes
`state.json` one time each second into a bind mount, with all keys, a live flag and a heartbeat. Nobody tested
that version.

Terms:

- **Lingering** is the systemd function that keeps the user services of an account in operation with no login. This is
  an established systemd term.
- The **state file** is `state.json`, which the slot text writes. This is a project term.
- **Fresh** means that the value of `written` in the state file changed in the last 5 seconds. This is a project term.

## Decision

### Name And Crate

- The producer is `liquidsoap-now-playing`. It is a new member crate of this workspace. It reads the state of
  Liquidsoap, so it also operates for a Liquidsoap script without AzuraCast.
- The producer lock, the drop file writer and the read of the instance configuration move from `mixxx-now-playing` into
  a new member crate, `now-playing-drop`. The two producers use that crate. No producer has a changed copy.
- The Mixxx lock of ADR 0021 stays in `mixxx-now-playing`. It keeps one Mixxx producer for each user session. A host
  can operate one Liquidsoap producer for each station.

### The State File Contract

This repository owns the format `musicindex-liquidsoap-state/1` and the reference slot text that writes it. The
reference slot text is `liquidsoap/musicindex-state.liq`. The research tutorial restates it and names this record as
the owner.

| Field | Type | Use in the producer |
| --- | --- | --- |
| `format` | string | Must be `musicindex-liquidsoap-state/1`. |
| `seq` | integer | Changes at each track. Starts at 0 at each start of Liquidsoap. |
| `written` | number | The heartbeat. |
| `live` | boolean | `true` while a live DJ is on air. |
| `jingle` | boolean | `true` while a jingle plays in AzuraCast jingle mode. See §Jingle Mode. |
| `track_started` | number | With `seq`, it makes `play_id`. |
| `title`, `artist`, `album` | string | The drop file fields of the same name. |
| `track_guid`, `feed_guid`, `image` | string | The drop file fields of the same name. |
| `value_routes` | string | JSON text in a JSON string. The producer parses it a second time. |
| `description`, `contributors` | string | Not used. The drop file has no field for them. |

- An empty string is a missing value. The producer writes `null` for it.
- A different `format` gives an error in the log and no drop file. The producer does not guess a version.
- A field change needs a new format version. The reference slot text and the producer change in the same commit.
- `play_id` comes from `track_started` and `seq`. A restart of Liquidsoap thus gives a new `play_id`.
- When the metadata of a track comes after the track mark, `seq` changes one more time in that track.
  `track_started` keeps the time of the track mark, and `play_id` stays unique. This second change is correct. Do not
  remove it: without it the producer keeps the empty metadata of the track mark for the full track.
- `duration_secs` is `null`. The next track or the heartbeat ends the play.

### What The Producer Writes

The producer reads the state file each 500 milliseconds. It writes the drop file version 3 of ADR 0014.

| State | Producer lock | Drop file |
| --- | --- | --- |
| Not fresh, or missing, or not readable | Not held | No file |
| Fresh, `live` is `true` | Not held | No file |
| Fresh, `live` is `false`, `feed_guid`, `track_guid` and routes present | Held | The routes (ADR 0024) |
| Fresh, `live` is `false`, `jingle` is `true` | Held | Empty `artist`, `title` and `value_routes`. Each other field is `null`. |
| Fresh, `live` is `false`, no `feed_guid`, no `track_guid` or no routes | Held | Empty `value_routes` |

- The producer removes the drop file before it releases the lock. ADR 0005 §Publisher Behavior then gives the dead block
  one time and no keepalive.
- The producer takes the lock again with the wait limit of ADR 0016 §The Producer Lock.
- A state file that the producer cannot parse does not change the drop file. The producer reads it again. A file that
  stays bad stops the heartbeat, and the first row then applies. Inference from the tutorial: `file.write` can rename a
  file that does not have all the data after a write error.
- `value_routes` that is not valid JSON gives empty routes and an error in the log. The producer never makes a route
  from a title or a file name (AGENTS.md §4).

### The Producer Stays In Operation

The Mixxx producer exits when Mixxx is not running, and its unit starts it again (AGENTS.md §7). This producer does not
exit when Liquidsoap stops. It releases the lock and continues to read the state file. The lock gives the publisher the
same fact. The producer exits only on an error that a restart cannot correct, for example a lock wait longer than the
limit.

### The Template Unit

- The package installs the user unit `liquidsoap-now-playing@.service`. It pairs with
  `musicindex-live-publisher@%i.service`, as ADR 0016 §Producer Units shows.
- The instance name comes from the short name of the station. It follows the name rule of ADR 0021 §The Unit Names,
  `^[a-z0-9][a-z0-9-]{0,31}$`. An AzuraCast short name can contain `_`, so the instance name changes each `_` to `-`.
  Example: the short name `heycitizens_test_radio` gives the instance `heycitizens-test-radio`.
- The state folder and `musicindex_dir` in the `.liq` text use the instance name, not the short name.
- The unit gives `--instance %i --state-file /var/lib/musicindex/azuracast/%i/state.json`. A station without
  AzuraCast changes the path with a drop-in file.
- The producer uses no network. The unit sets `RestrictAddressFamilies=AF_UNIX` and `PrivateDevices=true`.
- One account with lingering owns the producer and the publisher units on the server. The runbook gives the steps.
  The package is made on a different computer, because the test host has 1 CPU and approximately 340 MiB of free
  memory.

### Jingle Mode

AzuraCast jingle mode gives a jingle the metadata of the previous track. `handle_jingle_mode` in `azuracast.liq` maps
the metadata with `update=false`, so the marker `jingle_mode` is not in the metadata at the Pre-Broadcast slot
(`musicindex-research`, `sources/broadcast-audio/streaming/azuracast/util/docker/stations/liquidsoap/azuracast.liq`,
lines 681-690). Without a rule, the producer reports the previous track for the time of the jingle, and its artist is
paid.

- The previous metadata includes its `rid`. Liquidsoap gives each new request a new `rid`.
- The slot text sets `jingle` to `true` when `rid` is not empty and is equal to the `rid` of the last track change.
  A live DJ track has an empty `rid`, so it never sets `jingle`.
- For a jingle, the producer writes the row of §What The Producer Writes for `jingle`. The publisher then sends the dead
  block. The drop file does not name the previous track, so the show log does not record a second play of it.

The orchestrator session tested the `rid` rule on 2026-11-22, with AzuraCast 0.24 and Liquidsoap 2.4.6. A jingle
with the metadata of the previous track gave `jingle` as `true`, a drop file with no title, artist, track GUID or
routes, and the dead block on the relay. The next track gave `jingle` as `false`, its own GUID and its two routes. A
MusicIndex station can use jingle mode.

## Invariants

- The producer holds the lock only while the state is fresh and `live` is `false`.
- The producer never reads a media file and never makes a route.
- An unknown `format` gives no drop file.

## Verification

Mechanical. The producer crate has unit tests with `tempfile` and an injected clock. A jingle is one of the rows. The tests cover each row of
§What The Producer Writes, the second parse of `value_routes`, an unknown format and a stale heartbeat.

Manual. The reference slot text passes `liquidsoap --check` on a test station, and the steps of the runbook pass on a
test station. A test here cannot start AzuraCast.

## Open Questions

1. **Jingle mode.** Closed on 2026-11-22. The test passed. §Jingle Mode gives the result.
2. **The time to the listener.** The relay applies the stream delay (ADR 0011). The delay for the MP3 mount and for
   HLS is not measured.

## Alternatives Considered

### The Producer Reads The Media Files

Rejected by the operator. It needs a second bind mount, and the path in the event is a container path.

### The Producer Runs As Root

Rejected. The publisher units are user units, and the drop directory would need a second owner.

### The Name `azuracast-now-playing`

Rejected. The producer reads the state of Liquidsoap. It does not use an AzuraCast interface.

### Exit When The Heartbeat Stops

Rejected. A restart each few seconds during a DJ set fills the journal, and the lock gives the same fact.

## Consequences

- AGENTS.md §7 applies the exit rule to the Mixxx producer only.
- A Liquidsoap stop or a failed write gives the dead block after at most approximately 6 seconds.
- The research tutorial links the reference slot text and names this record.

## References

- ADR 0005, ADR 0014, ADR 0016, ADR 0021 and ADR 0023 of this repository
- `musicindex-research`: `research_notes/azuracast/track-events-for-a-producer.md`
- `docs/runbooks/musicindex-producer-tutorial.md`
