# MusicIndex Live Publisher Deployment

## Purpose

Build, install, and run `musicindex-live-publisher` and `mixxx-now-playing` as
**systemd user services**, publishing live value payloads to a MusicIndex relay.

User scope, not system scope, because Mixxx runs as your desktop user. The
producer writes the drop file and the publisher reads it, so running both as the
same user removes the file-ownership problem entirely: the drop directory lives
at `$XDG_RUNTIME_DIR/musicindex-live-publisher/mixxx/nowplaying`, mode `0700`,
owned by you. No `sudo` is needed for anything except copying the two binaries
into `/usr/bin`.

## Prerequisites

- A Rust toolchain.
- Mixxx, with its library at `~/.mixxx/mixxxdb.sqlite`.
- A V4V music directory (default `~/V4Vmusic`).
- A relay endpoint. `https://api.musicindex.org` for production, or a locally
  built `musicindex-live-relay` for rehearsal.
- Optional station fallback split details. If omitted, the service starts,
  logs a warning, and publishes a dead fallback route while idle or playing
  non-V4V audio.

## Arch Package Install

On Arch Linux, prefer the local package:

```bash
cd /home/citizen/build/musicindex-live-publisher/packaging/arch
makepkg -Csi
```

See the [Arch package runbook](musicindex-live-publisher-arch-package.md) for
package-specific build, install, upgrade, and removal steps. The manual build
path below is still useful on non-Arch hosts or while debugging.

## Quick Mixxx Setup

After the binaries are installed, the setup helper provisions the MusicIndex
live item, writes the one-time broadcaster token, generates
`musicindex-live-publisher@mixxx.service` and `mixxx-now-playing.service`, then
starts the pipeline. With no fallback route options, idle/non-V4V playback uses
the default dead fallback route:

```bash
setup-mixxx-musicindex
```

Use temporary mode for a rehearsal that only lasts for the current login
session:

```bash
setup-mixxx-musicindex --temporary
```

Permanent mode writes config and tokens under
`~/.config/musicindex-live-publisher/mixxx/` and enables both user services.
Temporary mode writes config and tokens under `$XDG_RUNTIME_DIR` and only starts
the services; the generated unit files remain under `~/.config/systemd/user`.

To receive station payments when no V4V track is playing, pass a fallback value
block fragment:

```bash
setup-mixxx-musicindex --fallback-value-block ~/.config/musicindex-live-publisher/mixxx-fallback.toml
```

The fallback value block fragment is TOML for `[target.fallback]`. The model
and destination fields are copied into the published value block; recipient
`type`, `customKey`, and `customValue` are not rewritten:

```toml
title = "Homegrown Hits"
image = "https://example.com/station-art.png"

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

For a single Lightning node recipient, the shortcut still works:

```bash
setup-mixxx-musicindex --fallback-address "$MUSICINDEX_FALLBACK_ADDRESS"
```

For a Lightning Address fallback recipient:

```bash
setup-mixxx-musicindex \
  --fallback-type lnaddress \
  --value-method lnaddress \
  --fallback-address station@example.com
```

## Build

```bash
cd ~/build/musicindex-live-publisher
cargo build --release --workspace
```

## Install The Binaries

The only step needing root:

```bash
sudo install -m 0755 target/release/mixxx-now-playing \
  /usr/bin/mixxx-now-playing
sudo install -m 0755 target/release/musicindex-live-publisher \
  /usr/bin/musicindex-live-publisher
```

## Provision A Live Item

The relay returns the broadcaster token **exactly once** and stores only a
hash. It cannot be recovered. `provision` writes it straight to a `0600` file
and never prints it.

```bash
install -d -m 0700 ~/.config/musicindex-live-publisher/mixxx/tokens
musicindex-live-publisher provision \
  --endpoint https://api.musicindex.org \
  --target default \
  --token-file ~/.config/musicindex-live-publisher/mixxx/tokens/default.token
```

The command prints a `[[target]]` stanza. Keep the `event_id` — listeners
subscribe by it, so changing it later means republishing your RSS live value
block.

Use JSON mode when a control surface provisions the item:

```bash
musicindex-live-publisher provision \
  --endpoint https://api.musicindex.org \
  --target default \
  --token-file ~/.config/musicindex-live-publisher/mixxx/tokens/default.token \
  --json
```

The JSON object contains the token file path. It never contains the token
content. A caller that needs the token reads the token file.

## Configuration

`~/.config/musicindex-live-publisher/mixxx/config.toml`:

```toml
# watch_dir is supplied by the unit as --watch-dir, so it is not set here.
watch_dir = "/run/user/1000/musicindex-live-publisher/mixxx/nowplaying"
endpoint = "https://api.musicindex.org"

[[target]]
name = "default"
event_id = "replace-with-provisioned-event-guid"
token_file = "~/.config/musicindex-live-publisher/mixxx/tokens/default.token"

# Optional: add [target.fallback] to receive station fallback payments.
# If omitted, the publisher logs a warning and uses a dead fallback route.
```

Replace `event_id` with the value printed by `provision` before starting the
service. The placeholder will never exist on the relay.

Keep one token file per target under
`~/.config/musicindex-live-publisher/<instance>/tokens/`. The packaged unit
reads these files as the same desktop user and the provision command writes
them with mode `0600`.

Splits are **decimal strings**, not numbers: `"100"`, `"49.51"`. Every
configured fallback destination needs `name`, `type`, `address`, and `split`,
and startup fails with a specific message if one is missing.

See [configuration options](musicindex-live-publisher-configuration.md) for the
full publisher and producer option reference.

To verify that the binary is installed:

```bash
musicindex-live-publisher --version
```

To show the loaded config in a machine-readable form:

```bash
musicindex-live-publisher config show \
  --config ~/.config/musicindex-live-publisher/mixxx/config.toml \
  --json
```

The output shows target names, event IDs, token file paths, stream delays, and
whether each target has a fallback. It does not show token content or fallback
destination addresses.

## Future Player Instances

Run only one `mixxx-now-playing.service`; Mixxx has one active desktop history
source. Mixxx uses the `musicindex-live-publisher@mixxx.service` publisher
instance. If a future non-Mixxx producer needs a separate pipeline, run a
separate publisher instance instead:

```bash
install -d -m 0700 ~/.config/musicindex-live-publisher/other-player/tokens
systemctl --user enable --now musicindex-live-publisher@other-player.service
```

That instance reads
`~/.config/musicindex-live-publisher/<instance>/config.toml` and watches
`$XDG_RUNTIME_DIR/musicindex-live-publisher/<instance>/nowplaying`. The future
producer must write its drop file there.

## Install The Units

```bash
install -d -m 0755 ~/.config/systemd/user
install -m 0644 systemd/musicindex-live-publisher.service \
  ~/.config/systemd/user/
install -m 0644 systemd/musicindex-live-publisher@.service \
  ~/.config/systemd/user/
install -m 0644 systemd/mixxx-now-playing.service \
  ~/.config/systemd/user/
systemctl --user daemon-reload
systemd-analyze --user verify ~/.config/systemd/user/musicindex-live-publisher@.service
```

Optional, so the services run without an active login session:

```bash
loginctl enable-linger "$USER"
```

## Rehearse Against A Local Relay First

Strongly recommended before pointing at production, because a payload mistake in
production routes real boosts to real destinations.

```bash
BIND=127.0.0.1:8018 ~/build/splitkit/target/release/musicindex-live-relay &

musicindex-live-publisher provision \
  --endpoint http://127.0.0.1:8018 \
  --target default \
  --token-file /tmp/local.token
```

Point a copy of the config at `http://127.0.0.1:8018` with the local
`event_id`, run the publisher in the foreground, play a V4V track, and inspect
what the relay is serving:

```bash
curl -s http://127.0.0.1:8018/v1/liveitems/<event_id>/remoteValue | python3 -m json.tool
```

Check, in order:

1. With no drop file present, the relay serves your **fallback** destinations.
2. Playing a V4V track replaces them with the **track's** destinations, and
   `duration` is in seconds.
3. Every destination has a non-empty `address`. A destination without one cannot
   be paid.
4. Stopping the track restores the fallback.
5. Playing a second track changes `blockGuid` but not `eventGuid`.

## Start

```bash
systemctl --user enable --now musicindex-live-publisher@mixxx.service
systemctl --user enable --now mixxx-now-playing.service
systemctl --user status musicindex-live-publisher@mixxx.service
journalctl --user -u musicindex-live-publisher@mixxx.service -f
```

## Observe

```bash
# What the relay is currently serving:
curl -s https://api.musicindex.org/v1/liveitems/<event_id>/remoteValue | python3 -m json.tool

# What the producer wrote:
cat "$XDG_RUNTIME_DIR/musicindex-live-publisher/mixxx/nowplaying/default.json"

# What OBS reads:
cat "$XDG_RUNTIME_DIR/musicindex-live-publisher/mixxx/nowplaying/now-playing.txt"

# Publishes, live:
journalctl --user -u musicindex-live-publisher@mixxx.service -f | grep published
```

A healthy track change logs `published live value payload` with a rising `seq`.

## Rollback

```bash
systemctl --user disable --now mixxx-now-playing.service
systemctl --user disable --now musicindex-live-publisher@mixxx.service
```

The relay keeps serving the last payload it accepted. If that was a track rather
than your fallback, publish the fallback once more before stopping, or the
ended track keeps collecting boosts.

## Failure Modes

- **`token_file "%d/<name>" requires CREDENTIALS_DIRECTORY`** — started outside
  a systemd unit that provides credentials. Use `~/.../tokens/<target>.token` or
  an absolute `token_file` for foreground runs.
- **Unit is `failed` with `start-limit-hit`** — five failures in five minutes.
  Almost always a fatal publish result. Check
  `journalctl --user -u musicindex-live-publisher@mixxx -n 50` for the HTTP
  status.
- **HTTP 401 or 403** — the token is wrong or revoked. The service now exits
  non-zero rather than idling silently, so the unit goes `failed`. Re-provision.
- **HTTP 404** — the live item no longer exists. Provision a new one and update
  `event_id`, then republish your RSS live value block.
- **Drop files appear but nothing publishes** — confirm `target` in the drop file
  matches a `[[target]]` `name`. A mismatch is logged as
  `skipping drop file for unknown target`.
- **Producer restarts every 10 seconds** — expected when Mixxx is not running.
  The producer exits cleanly and `Restart=always` retries until Mixxx appears.
- **`config must define at least one target`, or a fallback validation error** —
  missing fallback destinations no longer block startup. A fallback validation
  error means a configured route is malformed; fix or remove that fallback
  block.

## Known Gaps

Tracked in the [audit review](../reviews/nowplaying-publisher-audit-review.md).
Fixes 002, 003, and 005 are applied. Still open at deployment time:

- **Fix 004** — the publisher does not verify drop-directory ownership. The
  `0700` runtime directory in these units makes that unreachable in practice.
- `image` is always null in the drop file. No artwork reaches listening apps.
- Value routes fetched from the MusicIndex API are cached for the life of the
  process; restart the publisher to pick up changed splits.
