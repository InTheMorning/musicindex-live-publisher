# MusicIndex Live Publisher

This repository contains two Rust binaries:

- `musicindex-live-publisher` watches a local now-playing drop directory and publishes
  `musicindex.nowplaying/2` JSON files to a MusicIndex live relay.
  It transforms them into direct Podcasting 2.0 live value payloads.
- `mixxx-now-playing` watches the Mixxx history database, writes icecast-friendly
  text output (for use with eg. Butt), and can write publisher drop files for V4V tracks.

The publisher is a headless service. Producers write drop files by
temp-file-plus-rename. A drop file that exists means a track plays. A producer
removes the file when no track plays. The service then publishes the dead
block (ADR 0005), so a boost stops going to the last track.

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
| `v4vmm` | `v4vmm` | Writes the MusicIndex tags this chain reads. Registers live items. Starts and stops these services. Shows status. |
| `musicindex-live-relay` | `musicindex-live-relay` | Receives the payloads this service sends and passes them to listener apps. |

This repository has no build dependency on either one. The contracts are the
drop file, the relay HTTP API, the audio file tags, and the systemd units.

See `docs/architecture/broadcast-chain-boundaries.md` for each boundary, and
`v4vmm/docs/architecture/broadcast-chain.md` for the full chain.

`v4vmm` sends no payloads. This service is the only sender.

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

When no payable block plays, the publisher publishes the dead block (ADR
0005). The dead block pays the fixed `lnaddress` recipient
`no-v4v-track@example.invalid`. No configuration changes it.

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

Temporary mode keeps config and token under `$XDG_RUNTIME_DIR`, writes the user
unit files under `~/.config/systemd/user`, and starts services only for the
current login session:

```bash
setup-mixxx-musicindex --temporary
```

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

For an Arch local test install, use the packaged `PKGBUILD`:

```bash
cd packaging/arch
makepkg -Csi
```

See `docs/runbooks/musicindex-live-publisher-arch-package.md` for package
build, install, upgrade, and removal steps. See
`docs/runbooks/musicindex-live-publisher-configuration.md` for every
publisher and producer configuration option.

## Systemd

For a manual user-service install, copy both units to
`~/.config/systemd/user/` and run `systemctl --user daemon-reload`. The
publisher unit uses `PrivateTmp=true`, so producers must write to
`$XDG_RUNTIME_DIR/musicindex-live-publisher/<instance>/nowplaying`, not `/tmp`.

Run only one `mixxx-now-playing.service`; a desktop can only have one active
Mixxx history source. The Mixxx pipeline uses
`musicindex-live-publisher@mixxx.service`; future non-Mixxx producers should get their own
`musicindex-live-publisher@<instance>.service` instance and watch directory.

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
