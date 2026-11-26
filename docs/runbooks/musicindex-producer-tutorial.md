# Tutorial: MusicIndex Producer Support For AzuraCast And Liquidsoap

Date: 2026-11-21.

This tutorial is for a person who operates an AzuraCast station or a Liquidsoap script.
It gives the steps that prepare a station for a MusicIndex producer.
The producer tells the MusicIndex pipeline which track plays. The pipeline then sends the value routes of that track to the listener apps.

This tutorial is advisory. The publisher ADRs and the relay ADRs own the rules of the pipeline.
The track events note (musicindex-research: `research_notes/azuracast/track-events-for-a-producer.md`) gives the source of each fact here.

Publisher ADR 0041 §The State File Contract owns the format `musicindex-liquidsoap-state/1` of `state.json`. Publisher ADR 0023 owns the DJ takeover. Publisher ADR 0024 makes the two GUIDs a condition of payment.
The operator accepted the three ADRs on 2026-11-21 (commit `cdeb182` in `musicindex-live-publisher`).

The reference slot text is `liquidsoap/musicindex-state.liq` in `musicindex-live-publisher`. The publisher runbook `docs/runbooks/liquidsoap-now-playing-azuracast.md` gives the install of the producer and the publisher.

## Status

Read this section before you start.

| Part | Status |
| --- | --- |
| The TXXX frame in the HLS stream | The segment frame `musicindex.hls/2` of publisher ADR 0038 was tested on the HeyCitizen test station on 2026-11-26 with a test slot text ([test station log](test-station-log.md), step 51). The reference slot text with this frame is not installed on the station now. |
| The rule that a payable track needs the two GUIDs | Implemented in publisher commit `68adce4` for each producer. Not released. |
| The state file (`state.json`, in a bind mount, with all keys, a live flag and a heartbeat) | Tested on the HeyCitizen test station on 2026-11-22 |
| The bind mount through `docker-compose.override.yml` | Tested on the HeyCitizen test station on 2026-11-22, also after a reboot |
| The producer `liquidsoap-now-playing` that reads `state.json` | Implemented in publisher commits `e809d75` and `b808fd5`. Not released. Manual checks 1 to 3 of the publisher plan passed on the test station. Check 4, the DJ takeover, is open. |
| An automatic DJ takeover | Accepted in publisher ADR 0023. The publisher part is in commit `1a3350d`, not released. The station producer part is in commit `e809d75`. Not released. See [DJ Takeover](#dj-takeover). |
| Jingle mode | Tested on the HeyCitizen test station on 2026-11-22. A jingle pays nobody. See [Jingle Mode](#jingle-mode). |

Do not use this tutorial on a station with listeners until each part has the status "Tested".

## How The Parts Connect

```text
media file with MusicIndex tags
  -> Liquidsoap (in the AzuraCast container) reads the tags
  -> slot text writes state.json into a bind mount, one time each second
  -> producer (on the host) reads state.json and writes the drop file musicindex.nowplaying/3
  -> publisher sends the live value payload to musicindex-live-relay
  -> listener apps
```

The slot text also puts the segment frame of publisher ADR 0038 in the HLS stream.
A player that reads HLS timed ID3 can pair the segment with its display state by `playId`.

## What You Need

- AzuraCast with Liquidsoap 2.4.6. The tests used only that version. AzuraCast 0.24.1 is the code that the research examined.
- Root access to the host, for the bind mount.
- Access to the AzuraCast web interface with the permission to edit the Liquidsoap configuration.
- Media files with the MusicIndex tags. HitIt writes these tags (`src/audio_tags.rs` in the HitIt repository). Liquidsoap reads them as `musicindex track guid`, `musicindex feed guid`, `musicindex value routes`, `musicindex image`, `musicindex description` and `musicindex contributors`.
- A time with a small number of listeners. Two steps stop the stream for some seconds.
- Sufficient memory. On a host with 2 GiB, examine `ps -C docker-proxy --no-headers | wc -l`. The AzuraCast `docker-compose.yml` publishes 150 ports, and the Docker userland proxy starts one process for each port and each IP version. On the test host, these processes used 960 MiB. To stop them, put `{ "userland-proxy": false }` in `/etc/docker/daemon.json` and restart Docker. Docker then forwards the ports with firewall rules.

The steps use two names:

- `STATION` is the AzuraCast short name of the station, for example `heycitizens_test_radio`. Only the HLS address uses it.
- `INSTANCE` is the short name with each `_` changed to `-`, for example `heycitizens-test-radio`. The folders, the slot text and the units use it. A unit instance name cannot contain `_` (publisher ADR 0021 and ADR 0041 §The Template Unit).

Replace the two names in each command and in the slot text.

## Part 1: AzuraCast

### Step 1: Make A Backup

Do these commands as root on the host:

```sh
install -d -m 700 /root/musicindex-backup
```

```sh
cp -p /var/azuracast/docker-compose.yml /root/musicindex-backup/
```

```sh
cp -p /var/lib/docker/volumes/azuracast_station_data/_data/*/config/liquidsoap.liq /root/musicindex-backup/
```

The Liquidsoap file contains passwords. Keep the backup on the server. Do not send it to a different person.

### Step 2: Make The Folder For The State File

The producer must read the state file as a user other than root.
A user other than root cannot read the Docker volumes, because `/var/lib/docker` has mode 710.
Thus the state file goes into a host folder that the container sees through a bind mount.

Liquidsoap operates as UID 1000 in the container. Make the folder for that UID:

```sh
install -d -o 1000 -g 1000 -m 755 /var/lib/musicindex/azuracast/INSTANCE
```

### Step 3: Add The Bind Mount

AzuraCast tells the operator to put local changes in `docker-compose.override.yml` (`docker-compose.sample.yml:8-9`).
An update with `docker.sh` replaces `docker-compose.yml` and keeps the override file (`docker.sh:511-515`).

If `/var/azuracast/docker-compose.override.yml` is on the host, add the volume line to its `web` service.
If it is not on the host, make it with this text:

```yaml
services:
  web:
    volumes:
      - /var/lib/musicindex/azuracast:/var/azuracast/musicindex
```

Examine the merged configuration before the next step:

```sh
cd /var/azuracast && docker compose config | grep -n musicindex
```

The output must show the source `/var/lib/musicindex/azuracast` and the target `/var/azuracast/musicindex`.

### Step 4: Make The Container Again

This step stops all of AzuraCast for some seconds.

```sh
cd /var/azuracast && docker compose up -d
```

Make sure that Liquidsoap can write in the folder:

```sh
docker exec -u azuracast azuracast touch /var/azuracast/musicindex/INSTANCE/.test
```

```sh
rm /var/lib/musicindex/azuracast/INSTANCE/.test
```

Inference: the `azuracast_updater` container makes the container again at an automatic update. It keeps the mounts of the container. Examine the mount after the first update.

### Step 5: Put The Slot Text In The Pre-Broadcast Slot

AzuraCast shows the generated configuration with some editable boxes. The boxes have no names.
Find the box that comes immediately after these two lines:

```text
# Handle "Jingle Mode" tracks by replaying the previous metadata.
radio = azuracast.handle_jingle_mode(radio)
```

That box is the Pre-Broadcast slot. It comes before `ffmpeg.encode.audio`.
Do not use the box after `azuracast.apply_crossfade`. That box is before the live DJ input, so DJ audio does not go through it.

Put the reference slot text in the Pre-Broadcast slot. Replace `INSTANCE` on the line with `musicindex_dir`.

The reference slot text is `liquidsoap/musicindex-state.liq` in the repository `musicindex-live-publisher` (commit `6c1e14a`). Publisher ADR 0041 §The State File Contract owns it.
The Debian package installs it as `/usr/share/doc/musicindex-live-publisher/liquidsoap/musicindex-state.liq`.
Do not use a copy from a different source. A field change needs a new format version, and the producer must have the same version.

How the reference slot text works (a summary, publisher ADR 0041 §The State File Contract owns the text):

- The recode line stops a log warning. Liquidsoap logs "too long to be recoded" for a value of 1024 bytes or more. The value routes can have that length.
- The TXXX part adds the segment frame of publisher ADR 0038 to the HLS tag. The frame uses the description `musicindex` and the schema `musicindex.hls/2`.
- The frame holds `playId` for the segment boundary. The producer and the slot text write the same value. The player can use it to pair the frame with the display state.
- The frame has `playId` `null` for a jingle. It has no track, artwork or value fields (publisher ADR 0038 §The Segment Frame).
- Liquidsoap drops a key that is not an ID3 frame ID, so the key must have the name `TXXX`.
- `on_track` keeps the metadata of the new track in memory. It does not write a file.
- `on_track` gets only the metadata in the frame of the track mark, else an empty list (Liquidsoap `source.ml`, `execute_on_track`). After a jingle, the metadata of the next track can come in a later frame. Then the first metadata of the track gives a new `seq`, and `track_started` keeps the time of the track mark (publisher ADR 0041 §The State File Contract). The test station showed this fault before publisher commit `6c1e14a` (the [test station log](test-station-log.md), steps 16 and 17).
- The `on_track` and `on_metadata` callbacks are synchronous, so they operate in the frame order. They only set references.
- One reference holds the sequence number, the start time, the metadata, the `rid` of the last track and the jingle flag. Thus the writer cannot mix the values of two tracks.
- `jingle` is `true` when the new track has the same `rid` as the last track. Jingle mode gives a jingle the complete metadata of the last song, and that includes its `rid`. Liquidsoap gives each new request a new `rid`. A live DJ track has an empty `rid`, so it is not a jingle. This rule is an inference from the code. It is not tested.
- The slot text does not change `track_guid` or `value_routes` for a jingle. The producer applies the rule of publisher ADR 0041 §Jingle Mode.
- `thread.run` writes `state.json` one time each second. It is the only writer. `file.write` writes a temporary file and renames it, so a reader cannot see a half-written file.
- `try` and `catch` keep the writer in operation after a write error, for example a missing folder. Without them, `thread.run` stops the task after the first error (`src/core/builtins/builtins_thread.ml:90-108` in Liquidsoap 2.4.6). The log then shows the label `musicindex`.
- `written` changes each second. If `written` does not change, Liquidsoap is stopped or the write fails. The producer uses this value as a heartbeat.
- After a write error, `file.write` can rename a file that does not have all the data (`src/libs/file.liq:149-164`). If the producer cannot parse `state.json`, it must keep its last value and read again.
- `live` is `true` while a live DJ is on air. AzuraCast sets `azuracast.to_live` when the DJ input is ready, and then skips the AutoDJ track (`ConfigWriter.php:622-631`). Inference: `live` can become `true` some frames before the DJ audio. It comes before the listeners hear the DJ, because of the stream delay.
- At the switch to the DJ, `on_track` receives the DJ metadata, or no metadata. Then `track_guid` is empty. A DJ title change after the switch does not cause `on_track`. If the switch came with no metadata, the first DJ metadata gives a new `seq`.
- In jingle mode, a jingle causes `on_track` with the metadata of the last song. Then `seq` and `track_started` change, and `jingle` is `true`. `track_guid` and the value routes stay the same. See [Jingle Mode](#jingle-mode).
- `value_routes` is a JSON text in a JSON string. The producer must parse it a second time.
- `seq` starts at 0 at each start of Liquidsoap.

Do not use the `is_live` metadata. AzuraCast 0.24.1 disables the function that sets it ("Temporarily disabled for testing", `ConfigWriter.php:602-603`).

### Jingle Mode

A MusicIndex station can use jingle mode (publisher ADR 0041 §Jingle Mode, commit `52340f9`).

The problem: `azuracast.handle_jingle_mode` replaces the metadata of a jingle with the complete metadata of the last song. It uses `metadata.map(update=false, strip=true, …)`, so the `jingle_mode` key is not available after it (`azuracast.liq:678-691`). Without a rule, a jingle pays the artist of the last song.
The Pre-Broadcast slot comes after that function, so the slot text cannot see the `jingle_mode` key.

The slot text finds a jingle because the `rid` of the jingle is the same as the `rid` of the last song. It writes the field `jingle`.
For a jingle when no live DJ is on air, the producer writes a drop file with no artist, no title and no value routes. The publisher then sends the dead block (publisher ADR 0041 §Jingle Mode).

The test on 2026-11-22 used a playlist with jingle mode and three short files. A jingle gave `"jingle": true` with the metadata of the last song. The relay had the dead block. The next song had its own value on the relay after 24 seconds.

### Step 6: Save And Restart

1. Click **Save Changes**.
2. If AzuraCast shows an error, do not restart. AzuraCast keeps the incorrect text after an error. Correct the text or remove it, and save again.
3. If AzuraCast shows no error, click **Restart Broadcasting**. The stream stops for some seconds.

On a host with a small quantity of memory, the check starts a second Liquidsoap process. On the test host, the check passed with approximately 340 MiB available.

### Step 7: Examine The Result

Read the state file two times, with some seconds between:

```sh
cat /var/lib/musicindex/azuracast/INSTANCE/state.json
```

The value of `written` must increase. At a track change, `seq` must increase and `track_guid` must change. When the metadata of a track comes after the track mark, `seq` increases one more time in that track.

Examine the HLS stream with the parser of this repository, from a computer with Python 3:

```sh
python3 scripts/hls_id3_dump.py https://YOUR-HOST/hls/STATION/hls_stream.m3u8
```

Each segment must show a `TXXX` frame with the description `musicindex`, the schema `musicindex.hls/2` and a `playId` (publisher ADR 0038 §The Segment Frame).

Make sure that the MP3 mount and the HLS stream continue to play.

### Step 8: Install The Producer And The Publisher

Do the steps of the publisher runbook `docs/runbooks/liquidsoap-now-playing-azuracast.md` in `musicindex-live-publisher`. The runbook owns these steps.

A summary:

- One user account owns the producer and the publisher. Lingering is on for that user, so systemd keeps its services in operation with no login.
- The publisher installs as the template unit `musicindex-live-publisher@INSTANCE.service`.
- The producer installs as the template unit `liquidsoap-now-playing@INSTANCE.service`. It reads `/var/lib/musicindex/azuracast/INSTANCE/state.json`, the folder of step 2.
- The packages come from a different computer. A small radio host does not have the CPU and the memory to make them.

## Part 2: Liquidsoap Without AzuraCast

The same method works in a Liquidsoap script without AzuraCast. Make these changes to the slot text:

- Put the text after the last source change and before the outputs. The source variable can have a different name than `radio`.
- Set `musicindex_dir` to a folder that the producer can read.
- Replace `azuracast.to_live()` with a test of your live input, for example `live.is_ready()`.
- Keep the `txxx` export line only if the script has an HLS output with ID3.

## DJ Takeover

Operator requirement (2026-11-21): when a live DJ connects, the DJ takes over the MusicIndex event. The producer of the station must stop sending values for the time of the DJ set.

Why: the DJ plays tracks that the station does not know. The DJ sends the value routes of those tracks with a different producer, for example the Mixxx producer on the computer of the DJ.

### Why Two Senders Do Not Work Together

- The relay accepts each publish that has the token of the event. The newest publish replaces the snapshot (relay ADR 0002).
- While a producer holds its lock, its publisher sends a block and keepalives. If the publisher finds no drop file, the block is the dead block, which pays nobody (publisher ADR 0005).
- Thus a station publisher that continues during the DJ set replaces the value of the DJ.
- Publisher ADR 0016 finds a second writer for one event only on one host. A DJ on a different host is not found.

### The Procedure Now: Manual, In This Sequence

The current publisher rules permit a takeover if the two sides change in this sequence.

At the start of the DJ set:

1. The station operator stops the station producer. The station publisher then sends the dead block one time and stops its keepalives (ADR 0005).
2. Make sure that the relay has the dead block before you continue. `GET /v1/liveitems/EVENT_ID/metadata` must show the destination `no-v4v-track@example.invalid`. The publisher finds the stopped producer only at its next health check, so this can be some seconds after step 1.
3. The DJ connects the audio to AzuraCast.
4. The DJ starts the DJ producer and the DJ publisher. The first publish of the DJ publisher replaces the dead block.

At the end of the DJ set:

1. The DJ stops the DJ producer. The DJ publisher sends the dead block one time and stops its keepalives.
2. Make sure that the relay has the dead block, with the same request as in step 2 of the start.
3. The DJ disconnects the audio.
4. The station operator starts the station producer. The station publisher sends the value of the current track.

If the sequence changes, an incorrect value can stay on the relay.
An example: the DJ publisher sends its first block, and after that the station publisher sends its dead block. Then the listeners get the dead block until the next track of the DJ.
The lease end of the relay does not correct this. It removes the snapshot after 90 seconds with no keepalive, and a keepalive cannot restore a snapshot (relay ADR 0002).

### The Automatic Takeover: Accepted

Publisher ADR 0023 (Accepted) gives the automatic takeover. The relay does not change. This is a summary. The ADR owns the rules.

- While `live` is `true`, the station producer releases its lock. The station publisher sends the dead block one time and stops its keepalives. When `live` is `false`, the producer takes the lock again.
- At each keepalive, a publisher with a held lock and a payable last block reads the relay snapshot. If the snapshot is a dead block with a different `blockGuid`, the publisher sends its last block again. A different payable block gives only a warning, so two senders do not replace each other again and again.
- Thus the sequence of the two sides has no effect on the result. An incorrect value is only a dead block, and it stays for one keepalive interval or less (30 seconds by default).
- The DJ side stays manual. The DJ starts the DJ producer when the DJ audio is connected, and stops it at the end of the set. The DJ publisher needs a copy of the token of the station event.

The publisher part is in commit `1a3350d` of `musicindex-live-publisher`. It is not in a release. The station part is in the producer `liquidsoap-now-playing` (commit `e809d75`). It is not in a release.
When a release has the two parts, this section replaces the manual procedure. Until then, use the manual procedure.

## Remove The Changes

1. Delete the MusicIndex text from the Pre-Broadcast slot. Save and restart broadcasting.
2. Remove the volume line from `docker-compose.override.yml`. Do `docker compose up -d` in `/var/azuracast`.
3. Delete `/var/lib/musicindex/azuracast/INSTANCE`.
