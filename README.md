# MusicIndex Live Publisher

This repository contains two Rust binaries:

- `musicindex-live-publisher` watches a local now-playing drop directory and publishes
  `musicindex.nowplaying/3` JSON files to a MusicIndex live relay.
  It transforms them into direct Podcasting 2.0 live value payloads.
- `mixxx-now-playing` watches the Mixxx history database, writes icecast-friendly
  text output (for use with eg. Butt), and writes a publisher drop file for each
  track that plays, V4V or not (ADR 0014).

The publisher is a headless service. Producers write drop files by
temp-file-plus-rename. A drop file that exists means a track plays. A producer
removes the file when no track plays. A file that exists with an empty
`value_routes` means that the track is not payable, for example a non-V4V
track. For an absent file, and for a track that is not payable, the service
publishes the dead block (ADR 0005). So a boost stops going to the last
track.

The dead block is a constant. No configuration changes it.

## Repository Layout

```text
.
|-- mixxx-now-playing/       # Mixxx producer crate
|-- src/                     # publisher crate source
|-- systemd/                 # user service units for both binaries
|-- packaging/arch/          # local Arch package
`-- docs/                    # ADRs, plans, runbooks, tasks, reviews
```

## Related Projects

This service is one part of a chain that four repositories build separately.

| Component | Repository | Role |
|---|---|---|
| HitIt | `HitIt` | Writes the MusicIndex tags this chain reads. Registers live items. Starts and stops these services. Shows status. |
| `musicindex-live-relay` | `musicindex-live-relay` | Receives the payloads this service sends and passes them to listener apps. |

This repository has no build dependency on either one. The contracts are the
drop file, the relay HTTP API, the audio file tags, and the systemd units.

See `docs/architecture/broadcast-chain-boundaries.md` for each boundary, and
`HitIt/docs/architecture/broadcast-chain.md` for the full chain.

HitIt sends no payloads. This service is the only sender.

## Configuration

Default config path:

```text
/etc/musicindex-live-publisher/config.toml
```

The packaged setup flow runs Mixxx through the `mixxx` publisher instance. The
normal permanent config path is:

```text
~/.config/musicindex-live-publisher/mixxx/config.toml
```

Example for `musicindex-live-publisher@mixxx.service`:

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

Fields:

- `watch_dir`: directory watched for now-playing JSON files.
- `endpoint`: relay base URL.
- `target.name`: producer-facing target name. `mixxx-now-playing` defaults to
  `default`.
- `target.event_id`: live item event GUID provisioned at the relay. Replace
  the example placeholder before starting the service.
- `target.token_file`: broadcaster token path. Keep one private token file per
  target, usually under
  `~/.config/musicindex-live-publisher/<instance>/tokens/`.
- `target.stream_delay_secs`: the delay, in seconds, that a podcast app gets
  on Socket.IO (`musicindex-live-relay` ADR 0004). The relay applies this
  delay. The publisher sends it with each publish and does not wait (ADR
  0011). Defaults to `0`. See the configuration runbook for how to measure
  it.
- `show_log_retention_days`: the retention of the show log, in days (ADR
  0017). Defaults to `365`. The value must be `1` or more.
- `show_log`: turns the show log on or off for this instance (ADR 0017).
  Defaults to `true`. With `false`, the publisher writes no log, also with
  `--state-dir`.

When no payable block plays, the publisher publishes the dead block (ADR
0029). The dead block pays the fixed `lnaddress` recipient
`no-v4v-track@example.invalid`, and a payment to it fails (ADR 0033). No
configuration changes it.

## Show Log

ADR 0017 owns the show log contract. With `--state-dir`, the publisher
writes an append-only log of the show to
`<state_dir>/showlog/showlog-YYYY-MM-DD.jsonl`, one file for each UTC day.
The packaged unit sets `--state-dir` and a systemd `StateDirectory`.

The top-level key `show_log` turns the log off for one instance. The
default is `true`. With `false`, the publisher writes no log, also with
`--state-dir`. The files that exist stay until the operator removes them.

The log holds the artist and title of each track that the operator played.
A reader keeps the log for `show_log_retention_days` days, 365 by default.
The log has three kinds of entry:

- `track`: one entry each time the publisher tries to publish a track,
  payable or not.
- `clear`: one entry each time the publisher tries to publish with no
  track.
- `display`: one entry for each display update that the publisher sends.

See the configuration runbook for the full log location and the entry
schema.

## Provisioning

For Mixxx, use the setup helper. It provisions MusicIndex and starts the
services. The publisher uses a fixed dead block for idle or non-V4V playback
(ADR 0005). No setup flag changes it:

```bash
setup-mixxx-musicindex
```

In Mixxx, set Preferences, then Library, "Track duplicate distance" to 0.
Without it, a track that plays again within six tracks pays nobody. See
[Mixxx History Setup](docs/runbooks/musicindex-live-publisher-configuration.md#mixxx-history-setup).

The helper writes no unit file. The package installs the template units
`musicindex-live-publisher@.service` and `mixxx-now-playing@.service`, and the
helper enables and starts them for the instance `mixxx` (ADR 0021). A computer
that has the units of an earlier helper needs one migration. See the package
runbook, section "Upgrade".

Manual provisioning still works. Create a live item and write the one-time
broadcaster token:

```bash
musicindex-live-publisher provision \
  --endpoint https://api.musicindex.org \
  --target default \
  --token-file ~/.config/musicindex-live-publisher/mixxx/tokens/default.token
```

The command prints the `[[target]]` stanza to paste into the config.
Use `--json` when another program calls this command. The JSON object contains
the token file path and never the token.

You can also update the target list with the publisher CLI. These commands keep
the token as a file path. They do not print token content.

```bash
musicindex-live-publisher target add \
  --config ~/.config/musicindex-live-publisher/mixxx/config.toml \
  --name late-night \
  --event-id "$EVENT_ID" \
  --token-file ~/.config/musicindex-live-publisher/mixxx/tokens/late-night.token

musicindex-live-publisher target list \
  --config ~/.config/musicindex-live-publisher/mixxx/config.toml \
  --json

musicindex-live-publisher target remove \
  --config ~/.config/musicindex-live-publisher/mixxx/config.toml \
  --name late-night

musicindex-live-publisher config show \
  --config ~/.config/musicindex-live-publisher/mixxx/config.toml \
  --json

musicindex-live-publisher --version
```

Use `--replace` with `target add` to replace an existing target stanza.
`config show --json` reports the loaded config, target names, event IDs, token
file paths, and stream delays. It never prints token content.

For additional events, repeat provisioning with another `--target` value and
another token file, then add another `[[target]]` stanza. If a token is lost,
provision a new live item, replace that target stanza, and advertise the new
`event_id` wherever listeners discover the live value block.

## Arch Package

For an Arch install, use the packaged `PKGBUILD`. It builds the newest
commit of `master` from GitHub:

```bash
cd packaging/arch
makepkg -Csi
```

See `docs/runbooks/musicindex-live-publisher-arch-package.md` for package
build, install, upgrade, and removal steps. See
`docs/runbooks/musicindex-live-publisher-configuration.md` for every
publisher and producer configuration option.

## Systemd

The package installs two template units. Each instance has the units
`musicindex-live-publisher@<instance>.service` and
`mixxx-now-playing@<instance>.service`. The instance reads its config from
`~/.config/musicindex-live-publisher/<instance>/config.toml`. The publisher
unit uses `PrivateTmp=true`, so producers must write to
`$XDG_RUNTIME_DIR/musicindex-live-publisher/<instance>/nowplaying`, not `/tmp`.

Only one Mixxx producer runs at a time. A second one exits with status 3, and
systemd does not start it again (ADR 0021 §One Mixxx Producer At A Time). To
change the instance, stop the producer of the first instance, then start the
producer of the second instance.

Run foreground validation before enabling:

```bash
musicindex-live-publisher \
  --config ~/.config/musicindex-live-publisher/mixxx/config.toml \
  --watch-dir "$XDG_RUNTIME_DIR/musicindex-live-publisher/mixxx/nowplaying" \
  --dry-run \
  --verbose
```

See the deployment runbook for full install and verification steps:
`docs/runbooks/musicindex-live-publisher-deploy.md`.

## Payload Shape

The publisher sends a direct live value payload for each V4V track. This is
an example:

```json
{
  "title": "Makin' Beans",
  "image": "https://example.com/album-art.jpg",
  "description": "",
  "type": "music",
  "startTime": 0,
  "duration": 187.326,
  "eventGuid": "1873a383-d918-44e1-b6ff-c3598189dab6",
  "blockGuid": "477583d6-747d-40eb-a927-ee5d9622ebe8",
  "feedGuid": "acddbb03-064b-5098-87ca-9b146beb12e8",
  "itemGuid": "c7003607-233e-40e8-b2fa-6465127d0076",
  "line": ["Stay Awhile", "Able and The Wolf"],
  "author": "Able and The Wolf",
  "podcastName": "Stay Awhile",
  "value": {
    "model": { "type": "lightning", "method": "keysend" },
    "destinations": [
      {
        "type": "node",
        "name": "Artist Node",
        "address": "0368fed1c2fc35...",
        "split": "100"
      }
    ]
  }
}
```

ADR 0010 owns `line`, `author` and `podcastName`:

- `line` is `[album, artist]`, the form that the model server sends. When the
  drop file has no album, `line` is `[title, artist]`.
- `author` is the artist.
- `podcastName` is the album. When the drop file has no album, the payload
  has no `podcastName`.
- The payload has no `link`.

The dead block has none of these three fields. Its JSON did not change
(ADR 0005).
