# MusicIndex Live Publisher

This repository contains two Rust binaries:

- `musicindex-live-publisher` watches a local now-playing drop directory, turns
  `musicindex.nowplaying/1` JSON files into direct Podcasting 2.0 live value
  payloads, and publishes them to a MusicIndex live relay.
- `mixxx-now-playing` watches the Mixxx history database, writes OBS-friendly
  text output, and can write publisher drop files for V4V tracks.

The publisher is a headless service. Producers write drop files with
temp-file-plus-rename; presence means a track is playing, and file removal means
the service publishes the configured fallback splits so boosts stop routing to
the previous track.

## Repository Layout

```text
.
|-- mixxx-now-playing/       # Mixxx producer crate
|-- src/                     # publisher crate source
|-- systemd/                 # user service units for both binaries
|-- packaging/arch/          # local Arch package
`-- docs/                    # ADRs, plans, runbooks, tasks, reviews
```

## Configuration

Default config path:

```text
/etc/musicindex-live-publisher/config.toml
```

The packaged systemd user unit passes
`--config %h/.config/musicindex-live-publisher/config.toml`, so the normal test
config path is:

```text
~/.config/musicindex-live-publisher/config.toml
```

Example for the packaged user unit:

```toml
watch_dir = "/run/user/1000/musicindex-live-publisher/nowplaying"
endpoint = "https://api.musicindex.org"

[[target]]
name = "default"
event_id = "replace-with-provisioned-event-guid"
token_file = "%d/default.token"

  [target.fallback]
  title = "Homegrown Hits"
  destinations = [
    { name = "Station", type = "node", address = "03...", split = "100" },
  ]
```

Fields:

- `watch_dir`: directory watched for now-playing JSON files.
- `endpoint`: relay base URL.
- `target.name`: producer-facing target name. `mixxx-now-playing` defaults to
  `default`.
- `target.event_id`: live item event GUID provisioned at the relay. Replace
  the example placeholder before starting the service.
- `target.token_file`: broadcaster token path. Under systemd, use
  `%d/default.token`; it resolves through `CREDENTIALS_DIRECTORY`.
- `target.fallback`: station-owned live value destinations used when playback
  clears.

## Provisioning

Create a live item and write the one-time broadcaster token:

```bash
musicindex-live-publisher provision \
  --endpoint https://api.musicindex.org \
  --token-file ~/.config/musicindex-live-publisher/default.token
```

The command prints the `[[target]]` stanza to paste into the config. Keep
`token_file = "%d/default.token"` when running under the packaged systemd user
unit. If the token is lost, provision a new live item, replace the target
stanza, and advertise the new `event_id` wherever listeners discover the live
value block.

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
`$XDG_RUNTIME_DIR/musicindex-live-publisher/nowplaying`, not `/tmp`.

Run foreground validation before enabling:

```bash
musicindex-live-publisher \
  --config ~/.config/musicindex-live-publisher/config.toml \
  --watch-dir "$XDG_RUNTIME_DIR/musicindex-live-publisher/nowplaying" \
  --dry-run \
  --verbose
```

See the deployment runbook for full install and verification steps:
`docs/runbooks/musicindex-live-publisher-deploy.md`.
