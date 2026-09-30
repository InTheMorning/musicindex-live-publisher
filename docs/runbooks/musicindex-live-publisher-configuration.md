# MusicIndex Live Publisher Configuration

## Purpose

List the configuration files, service flags, producer flags, and TOML fields
used by the Mixxx now-playing to MusicIndex live value pipeline.

## Runtime Files

Publisher config:

```text
~/.config/musicindex-live-publisher/mixxx/config.toml
```

Publisher tokens:

```text
~/.config/musicindex-live-publisher/mixxx/tokens/<target>.token
```

Producer config:

```text
~/.config/v4vmm/config.toml
```

Drop directory under the packaged systemd user units:

```text
$XDG_RUNTIME_DIR/musicindex-live-publisher/mixxx/nowplaying
```

OBS text output:

```text
$XDG_RUNTIME_DIR/musicindex-live-publisher/mixxx/nowplaying/now-playing.txt
```

## Publisher TOML

Example:

```toml
watch_dir = "/run/user/1000/musicindex-live-publisher/mixxx/nowplaying"
endpoint = "https://api.musicindex.org"

[[target]]
name = "default"
event_id = "replace-with-provisioned-event-guid"
token_file = "~/.config/musicindex-live-publisher/mixxx/tokens/default.token"

# The publisher uses a fixed dead block while idle or non-V4V (ADR 0005).
# No configuration changes it.
```

Required top-level fields:

- `watch_dir`: directory watched for final `*.json` drop files. The packaged
  publisher unit overrides this with `--watch-dir %t/...`.
- `endpoint`: relay base URL, for example `https://api.musicindex.org`.
- `[[target]]`: one or more publish targets.

Required target fields:

- `name`: target name matched against the producer JSON `target` field.
  `mixxx-now-playing` defaults to `default`.
- `event_id`: live item event GUID returned by provisioning. Replace the
  example placeholder before starting the service.
- `token_file`: broadcaster token path. Keep one private token file per target,
  usually under
  `~/.config/musicindex-live-publisher/<instance>/tokens/`. `~/` expands to the
  service user's home directory. `%d/<name>` is also supported for custom systemd
  units that provide `CREDENTIALS_DIRECTORY`.

Optional target fields:

- `stream_delay_secs`: seconds to hold every payload for this target before it
  reaches the relay. Defaults to `0`. Accepts fractional seconds. Values that
  are negative, not finite, or above 300 are rejected at startup with the target
  name in the error.

A `[target.fallback]` table is a load error. ADR 0005 removes the configured
fallback. The error names the ADR and the target, so an operator can remove
the table.

Destination fields, for a `value_routes` entry in the drop file:

- `name`: recipient label.
- `type`: recipient route type. Use `node` for a Lightning node pubkey
  `valueRecipient`.
- `address`: protocol-specific payment destination. For `type = "node"`, use
  the Lightning node pubkey.
- `split`: decimal string, for example `"100"` or `"49.51"`.
- `customKey`: optional custom record key.
- `customValue`: optional custom record value.
- `fee`: optional boolean.

## LNURL And Lightning Address Compatibility

The publisher is metadata-only: it publishes the assembled value block to the
MusicIndex relay and does not resolve or pay Lightning routes. Recipient
compatibility therefore depends on the app or wallet that consumes the live
value payload.

A producer supplies a track's routes in the drop file's `value_routes` array
(ADR 0002). For maximum Podcasting 2.0 compatibility, a producer uses a
Lightning node recipient:

```json
{
  "recipient_name": "Artist",
  "route_type": "node",
  "address": "03...",
  "split": 100.0
}
```

Lightning Address is supported as pass-through metadata by using the
Podcasting `lnaddress` recipient type. The current Podcasting docs describe
Lightning Address recipients as email-like addresses that consuming apps
resolve through well-known LNURL/keysend endpoints before payment. The
publisher does not perform that resolution:

```json
{
  "recipient_name": "Artist",
  "route_type": "lnaddress",
  "address": "artist@example.com",
  "split": 100.0
}
```

The dead block (ADR 0005) uses the same fixed `lnaddress` shape, at the
address `no-v4v-track@example.invalid`. No configuration changes it.

Direct LNURL-pay URLs or bech32 LNURL strings are not a documented Podcasting
`valueRecipient` type in the current namespace docs. This publisher will not
block an agreed custom `type`/`method`, but client support is not guaranteed.
Use `lnaddress` when you want standard Lightning Address behavior, or `node`
when you need the broadest V4V streaming support.

References:

- Podcasting 2.0
  [`valueRecipient`](https://podcasting2.org/docs/podcast-namespace/tags/value-recipient)
  docs.
- Podcasting 2.0
  [`lnaddress`](https://podcasting2.org/docs/podcast-namespace/examples/value/lnaddress)
  example.
- LNURL [LUD-06 payRequest](https://raw.githubusercontent.com/lnurl/luds/luds/06.md)
  and [LUD-16 Lightning Address](https://raw.githubusercontent.com/lnurl/luds/luds/16.md).

## Dead Block

Each target gets the dead block (ADR 0005) when no payable block plays. This
is a publisher safety measure, not a Podcasting namespace requirement.

The publisher posts the dead block at startup when the watched directory has
no current track. It posts the dead block again when a producer removes the
drop file, or reports an empty `value_routes` list. Without the dead block,
the relay keeps serving the last published track payload. The relay has no
clear command of its own, so only a new publish can replace that payload. A
stale track payload can then send a boost to the last song, after play moves
on, stops, or changes to non-V4V audio.

The dead block is a constant:

```json
{
  "title": "No V4V track playing",
  "value": {
    "model": { "type": "lightning", "method": "lnaddress" },
    "destinations": [
      {
        "name": "No V4V payment route",
        "type": "lnaddress",
        "address": "no-v4v-track@example.invalid",
        "split": "100"
      }
    ]
  }
}
```

That fixed block gives the stream no usable payment route when no V4V track
plays. No configuration changes it.

Validation rules:

- There must be at least one target.
- Target names must be unique and non-empty.
- `event_id` must be non-empty and must not be the example placeholder.
- A `[target.fallback]` table in a target stanza is a load error that names
  ADR 0005.
- Empty token files are rejected.
- `stream_delay_secs` must be finite, must not be negative, and must not exceed
  300.

## Stream Delay

The publisher sees a track change the instant the producer writes the drop file.
Listeners hear that track several seconds later, after the encoder, the icecast
queue, and their own player buffer. The icecast title survives that gap because
it travels in band with the audio; a relay payload does not. With
`stream_delay_secs = 0`, the value block therefore flips to the next track while
listeners still hear the previous one, and a boost sent in that window pays the
wrong artist. Set the delay to close the gap.

What the delay covers:

- Track payloads and the dead block that follows a removal are held for the
  same duration, so a set never has a gap or an overlap.
- Two tracks changing inside one delay window both publish, in order, each at
  its own deadline.
- A producer rewrite of the same track — the MusicIndex value-route upgrade —
  replaces the pending payload and keeps the original deadline, so it publishes
  once and on time.
- Startup is not delayed. Existing drop files are recovered state, not a track
  change, and holding them would leave the relay serving nothing.
- `--dry-run` honours the delay, so you can time stdout against the stream
  without publishing.

Measuring it:

1. Start playback of a track with an obvious opening.
2. Note the local wall-clock time at which the publisher logs the track — run
   with `--verbose` and watch for `holding live value payload for stream delay`
   and `releasing live value payload after stream delay`.
3. Note the wall-clock time at which a real listening client, on a normal
   network, hears that opening.
4. The difference is the delay. Re-measure from a cold client join, because
   icecast's burst buffer affects the first seconds of a connection.

butt's own song-update delay is a different term. It shifts the icecast title
only, and does not substitute for this setting. If you have one configured,
measure with it in place rather than adding the two together.

Verify the running value in the journal. The service logs one
`configured publish target` line per target at startup, carrying
`stream_delay_secs`.

## Publisher Instances

The Mixxx pipeline uses `musicindex-live-publisher@mixxx.service`. It reads
`~/.config/musicindex-live-publisher/mixxx/config.toml` and watches:

```text
$XDG_RUNTIME_DIR/musicindex-live-publisher/mixxx/nowplaying
```

Run only one `mixxx-now-playing.service`; Mixxx has one active desktop history
source. If a future non-Mixxx producer needs its own pipeline, use a separate
publisher instance with its own config and drop directory:

```text
~/.config/musicindex-live-publisher/<instance>/config.toml
~/.config/musicindex-live-publisher/<instance>/tokens/default.token
$XDG_RUNTIME_DIR/musicindex-live-publisher/<instance>/nowplaying
```

The package installs `musicindex-live-publisher@.service` for this instance
layout. The future producer must write its drop file into that instance's watch
directory.

## Publisher CLI

Run mode:

```bash
musicindex-live-publisher [OPTIONS]
```

Options:

- `--config <path>`: config file path. Default:
  `/etc/musicindex-live-publisher/config.toml`.
- `--watch-dir <path>`: override the config `watch_dir`.
- `--endpoint <url>`: override the config `endpoint`.
- `--dry-run`: print live value payloads to stdout instead of publishing.
- `--verbose`: enable debug logging.
- `--version`: print the package version and exit.

Provision mode:

```bash
musicindex-live-publisher provision \
  --endpoint <url> \
  --target <name> \
  --token-file <path>
```

Provisioning writes the broadcaster token with mode `0600`, prints the
provisioned `event_id`, and does not print the token. `--target` defaults to
`default` when omitted.

Add `--json` when another program calls `provision`. The JSON object contains
`event_id`, `token_file`, `target`, `metadata_url`, `remote_value_url`,
`events_url`, and `socket_io_url`. It does not contain the token.

Target management:

```bash
musicindex-live-publisher target add \
  --config <path> \
  --name <name> \
  --event-id <event_id> \
  --token-file <path>

musicindex-live-publisher target list --config <path> --json

musicindex-live-publisher target remove --config <path> --name <name>

musicindex-live-publisher config show --config <path> --json
```

`target add` validates that the token file exists and can be read. It writes the
token file path to the config. It does not read or print token content.

Use `--stream-delay-secs <seconds>` to set a delay for the target. Use
`--replace` to replace an existing target stanza.

`target list --json` prints target names, event identifiers, token file paths,
and stream delays. It does not print token content.

`target remove` removes the target stanza only. It does not remove the token
file and does not contact the relay.

`config show --json` prints `watch_dir`, `endpoint`, and the target array. Each
target contains `name`, `event_id`, `token_file`, and `stream_delay_secs`. It
does not print token content.

When a JSON command fails after parsing its flags, stdout contains one object
with an `error` field. The exit code stays the same as the non-JSON command.

## Producer TOML

`mixxx-now-playing` reads optional V4V Music Manager settings from:

```text
~/.config/v4vmm/config.toml
```

Example:

```toml
music_dir = "~/V4Vmusic"
musicindex_endpoint = "https://api.musicindex.org"
```

Fields:

- `music_dir`: V4V music root. Overridden by `V4V_MUSIC_DIR` and
  `--v4v-root`.
- `musicindex_endpoint`: MusicIndex API base URL for resolving value routes.

## Producer CLI

```bash
mixxx-now-playing [OPTIONS]
```

Options:

- `--db-file <path>`: Mixxx SQLite history database. Default:
  `~/.mixxx/mixxxdb.sqlite`.
- `--txt-file <path>`: now-playing text output for OBS. Default:
  `$XDG_RUNTIME_DIR/musicindex-live-publisher/mixxx/nowplaying/now-playing.txt`,
  or `~/.cache/musicindex-live-publisher/mixxx/nowplaying/now-playing.txt` when
  `XDG_RUNTIME_DIR` is not set.
- `--id3-file <path>`: metadata output. In JSON mode this is the publisher drop
  file. Default:
  `$XDG_RUNTIME_DIR/musicindex-live-publisher/mixxx/nowplaying/metadata.txt`,
  or `~/.cache/musicindex-live-publisher/mixxx/nowplaying/metadata.txt` when
  `XDG_RUNTIME_DIR` is not set. The Mixxx user unit overrides this to
  `default.json`.
- `--v4v-root <path>`: V4V music root.
- `--poll-secs <seconds>`: Mixxx history poll interval. Default: `0.5`.
- `--once`: process the current latest track once and exit.
- `--format <text|json>`: metadata output format. Use `json` for the publisher.
  Default: `text`.
- `--target <name>`: JSON drop-file target. Default: `default`.
- `--expiry <duration|none>`: clear metadata after track duration or never.
  Default: `duration`.
- `--expiry-slack <seconds>`: extra seconds added to known track duration.
  Default: `5`.
- `--expiry-max <seconds>`: maximum expiry time for a track. A longer or
  unknown duration uses this value. Default: `600`.
- `--no-api`: use embedded tag value routes only; do not query MusicIndex.
- `--api-timeout <seconds>`: MusicIndex route lookup timeout. Default: `5`.
- `--strip-hyphens`: strip hyphens in the plain text now-playing line. Default.
- `--no-strip-hyphens`: preserve hyphens in the text output.
- `--verbose`: print resolved paths and API status.
- `--connector-card <id>`: card ID of the MIDI connector card. Default: `V4V`.
  See [MIDI Connector](#midi-connector).
- `--no-connector`: do not use the MIDI connector. The producer uses the
  history-only mode of ADR 0005 with `--expiry-max`.

## MIDI Connector

ADR 0006 owns the rules in this section. This section restates them. If this
section and ADR 0006 are different, ADR 0006 applies.

The MIDI connector gives the producer the deck state of Mixxx. When the linked
deck stops, the producer removes the drop file at once. Without the connector,
the producer uses the expiry of ADR 0005.

### Card Setup

The package installs two files. They make the V4V card at boot:

```text
# /usr/lib/modules-load.d/musicindex-v4v-midi.conf
snd-virmidi

# /usr/lib/modprobe.d/musicindex-v4v-midi.conf
options snd-virmidi enable=1 index=31 id=V4V midi_devs=1
```

The package install does not load the kernel module. The V4V card exists
after the next reboot. After the reboot, do this check:

```bash
cat /proc/asound/cards
```

The output shows card 31 with the ID `V4V`.

`setup-mixxx-musicindex` checks the card before it writes a file:

- If `/proc/asound/V4V` does not exist, the script stops. Reboot after the
  package install, and then run the script again.
- If the card is card 31, the script prints the port name `VirMIDI 31-0`.
- If the card has a different number N, the script prints a warning with the
  port name `VirMIDI N-0` and continues. Use that port name in Mixxx.

### Merge The Options

Do these steps only if the computer already uses `snd-virmidi` for other
cards. The module has one set of options for all its cards. Two
`options snd-virmidi` lines conflict.

1. Write `/etc/modprobe.d/musicindex-v4v-midi.conf`. A file in `/etc` with
   the same name replaces the package file.
2. Put one line with all cards in that file. For two other cards, the line
   is:

   ```text
   options snd-virmidi enable=1,1,1 index=-1,-1,31 id=Synth1,Synth2,V4V midi_devs=1,1,1
   ```

3. Remove the other `options snd-virmidi` line from the computer.
4. Reboot.

Only the V4V card gets a fixed number. The value `-1` gives a card the lowest
free number. Do not give another card a fixed low number. A USB card can take
that number first at boot, and then the kernel cannot make the virtual card.

An ID from the option keeps only letters and digits. So the IDs of the other
cards can change. Change each script that opens their raw devices by the old
ID. The port names in Mixxx do not change, because they use the card number.

### Mixxx Controller Setup

The package installs the mapping files in `/usr/share/mixxx/controllers/`.
Mixxx finds MIDI devices only at startup. So start Mixxx after the reboot.

1. In Mixxx, open Preferences, then Controllers.
2. Select the controller `VirMIDI 31-0`.
3. Select the mapping "MusicIndex V4V Connector".
4. Enable the controller.
5. Click OK.

If the setup script gave a different port name, use that port name in step 2.

### Operator Rules

- Disable a controller in Mixxx before you unplug it, if its mapping sends
  feedback such as LED output.
- If such a controller was unplugged, disable it in Preferences. The
  connector output then comes back with no restart.
- Do not leave a controller enabled whose device is not connected, if its
  mapping sends feedback.
- Enable only one Mixxx controller with the connector mapping.
- Do not connect a different source to the connector port, in ALSA or in JACK.

### Producer Options

- `--connector-card <id>`: the card ID of the connector card. Default: `V4V`.
  The producer finds the card number from `/proc/asound/<id>`.
- `--no-connector`: the producer does not open the MIDI device. It uses the
  history-only mode of ADR 0005.

### Producer Mode In The Log

Show the producer log:

```bash
journalctl --user -u mixxx-now-playing
```

The producer writes these lines at the default log level `info`:

- `connector mode`, at level `INFO`: the producer receives heartbeats from
  the mapping.
- `history-only mode`, at level `WARN`: the producer uses the expiry of
  ADR 0005. The `reason` field gives the cause:
  - `NoDevice`: the producer cannot open the raw MIDI device.
  - `NoHeartbeat`: no heartbeat arrived in the last 3 seconds.
  - `UnknownVersion`: the heartbeat gives a protocol version that the
    producer does not know.

Set `RUST_LOG` in the unit environment to change the log level.

### Commands To Mixxx

ADR 0007 owns the rules in this subsection. This subsection restates them. If
this subsection and ADR 0007 are different, ADR 0007 applies.

The producer binary sends a command to Mixxx through the connector card:

```bash
mixxx-now-playing command fade-now [--connector-card ID] [--timeout SECS]
```

- `fade-now` starts the AutoDJ transition to the next track immediately. It sets
  `[AutoDJ],fade_now`. It is not `skip_next`.
- `--connector-card <id>`: the card ID of the connector card. Default: `V4V`.
- `--timeout <secs>`: the time limit for the full command. Default: 2
  seconds.

The command waits for a heartbeat of protocol version 3. Then it sends the
command and waits for the answer of the mapping. The mapping does the command
only when AutoDJ is enabled. The command does not use the producer lock and
does not change a drop file. It can run while the producer runs.

Exit codes:

| Code | Meaning |
|---|---|
| 0 | The mapping did the command. It set the control. |
| 2 | The command line is not correct. |
| 3 | The mapping refused the command. For example, AutoDJ is disabled. |
| 4 | The command was not sent. The raw device is not available, or no heartbeat of version 3 arrived before the timeout. |
| 5 | The command was sent, but no answer arrived before the timeout. The result is not known. |

Exit code 0 does not mean that the transition started. Mixxx can ignore
`fade_now`, for example during a transition. The deck state shows the result.

The command writes one line to stderr for each exit code other than 0.

After exit code 5, do not send the command again before you examine the deck
state. Mixxx can have done the command, and the answer can be lost.

## Drop File Contract

The producer writes final `*.json` files by temp-file-plus-rename. The publisher
ignores non-JSON files and publishes the dead block when the final file
disappears.

Current schema:

```json
{
  "schema": "musicindex.nowplaying/1",
  "target": "default",
  "artist": "Alice",
  "title": "Track Title",
  "duration_secs": 187.326,
  "image": null,
  "feed_guid": "feed-guid",
  "track_guid": "track-guid",
  "value_routes": [
    {
      "recipient_name": "Alice",
      "route_type": "node",
      "address": "03nodepubkey",
      "split": 90.0,
      "fee": false,
      "custom_key": null,
      "custom_value": null
    }
  ],
  "value_routes_source": "musicindex-api"
}
```

## Recommended Test Config

For foreground dry-run testing outside systemd, use absolute token paths:

```toml
watch_dir = "/tmp/musicindex-live-publisher-nowplaying"
endpoint = "http://127.0.0.1:8018"

[[target]]
name = "default"
event_id = "local-event-guid"
token_file = "/tmp/musicindex-live-publisher-default.token"
```

Create the directory, then run:

```bash
install -d -m 0700 /tmp/musicindex-live-publisher-nowplaying
musicindex-live-publisher --config /tmp/publisher.toml --dry-run --verbose
```
