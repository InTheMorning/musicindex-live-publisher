# MusicIndex Live Publisher

This repository contains two Rust binaries:

- `musicindex-live-publisher` watches a local now-playing drop directory, turns
  `musicindex.nowplaying/1` JSON files into direct Podcasting 2.0 live value
  payloads, and publishes them to a MusicIndex live relay.
- `mixxx-now-playing` watches the Mixxx history database, writes icecast-friendly
  text output (for use with eg. Butt), and can write publisher drop files for V4V tracks.

The publisher is a headless service. Producers write drop files with
temp-file-plus-rename; presence means a track is playing, and file removal means
the service publishes fallback metadata so boosts stop routing to the previous
track. If no station fallback is configured, it publishes a dead fallback route
and logs a warning.

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
| `musicindex-live-relay` | `splitkit` | Receives the payloads this service sends and passes them to listener apps. |

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

# Optional: add [target.fallback] to receive station fallback payments.
# If omitted, the publisher logs a warning and uses a dead fallback route.
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
- `target.stream_delay_secs`: seconds to hold each payload so the published
  value block lines up with what listeners hear. Defaults to `0`. See the
  configuration runbook for how to measure it.
- `target.fallback`: optional station-owned fallback value block used when
  playback clears. If omitted, the publisher warns and uses a dead fallback
  route: `lnaddress` recipient `no-v4v-track@example.invalid`.

## Provisioning

For Mixxx, use the setup helper. With no fallback route options, it provisions
MusicIndex and starts the services using a dead fallback route for idle/non-V4V
playback:

```bash
setup-mixxx-musicindex
```

Temporary mode keeps config and token under `$XDG_RUNTIME_DIR`, writes the user
unit files under `~/.config/systemd/user`, and starts services only for the
current login session:

```bash
setup-mixxx-musicindex --temporary
```

To receive station payments when no V4V track is playing, pass a fallback value
block:

```bash
setup-mixxx-musicindex --fallback-value-block ~/.config/musicindex-live-publisher/mixxx-fallback.toml
```

The value-block fragment is TOML for `[target.fallback]`. The model and
destination fields are copied into the published value block; the setup helper
does not rewrite recipient `type`, `customKey`, or `customValue` fields:

```toml
title = "Homegrown Hits"

[model]
type = "lightning"
method = "keysend"

[[destinations]]
name = "Sharpie"
type = "node"
address = "YOUR_LIGHTNING_NODE_PUBKEY"
split = "10"
customKey = "696969"
customValue = "5"
```

Replace `YOUR_LIGHTNING_NODE_PUBKEY` before running the setup script; it
rejects placeholder fallback destinations before provisioning.

For a single Lightning node recipient, the shortcut is:

```bash
setup-mixxx-musicindex --fallback-address "$MUSICINDEX_FALLBACK_ADDRESS"
```

For a Lightning Address fallback recipient, use:

```bash
setup-mixxx-musicindex \
  --fallback-type lnaddress \
  --value-method lnaddress \
  --fallback-address station@example.com
```

Manual provisioning still works. Create a live item and write the one-time
broadcaster token:

```bash
musicindex-live-publisher provision \
  --endpoint https://api.musicindex.org \
  --target default \
  --token-file ~/.config/musicindex-live-publisher/mixxx/tokens/default.token
```

The command prints the `[[target]]` stanza to paste into the config. For
additional events, repeat provisioning with another `--target` value and another
token file, then add another `[[target]]` stanza. If a token is lost, provision a
new live item, replace that target stanza, and advertise the new `event_id`
wherever listeners discover the live value block.

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
