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

# Optional: add [target.fallback] to receive station fallback payments.
# If omitted, the publisher logs a warning and uses a dead fallback route.
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
- `[target.fallback]`: optional station-owned fallback live value block.

Fallback fields:

- `title`: fallback title shown when no track is playing. Defaults to
  `No V4V track playing` when the fallback table is omitted.
- `image`: optional artwork URL.
- `value.model`: fallback value model. `type` and `method` must be non-empty.
  `suggested` is optional when the value model needs it.
- `value.destinations`: one or more fallback payment destinations. The older
  direct `destinations = [...]` form is still accepted for existing configs; if
  no model is configured, those legacy destinations use the default
  `lightning`/`keysend` model.

Destination fields:

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

The publisher is metadata-only: it publishes the configured value block to the
MusicIndex relay and does not resolve or pay Lightning routes. Recipient
compatibility therefore depends on the app or wallet that consumes the live
value payload.

For maximum Podcasting 2.0 compatibility, use a Lightning node recipient:

```toml
[target.fallback.value.model]
type = "lightning"
method = "keysend"

[[target.fallback.value.destinations]]
name = "Station"
type = "node"
address = "03..."
split = "100"
```

Lightning Address is supported as pass-through metadata by using the
Podcasting `lnaddress` recipient type. The current Podcasting docs describe
Lightning Address recipients as email-like addresses that consuming apps
resolve through well-known LNURL/keysend endpoints before payment. The
publisher does not perform that resolution:

```toml
[target.fallback.value.model]
type = "lightning"
method = "lnaddress"

[[target.fallback.value.destinations]]
name = "Station"
type = "lnaddress"
address = "station@example.com"
split = "100"
```

The setup shortcut can generate the same shape:

```bash
setup-mixxx-musicindex \
  --fallback-type lnaddress \
  --value-method lnaddress \
  --fallback-address station@example.com
```

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

## Fallback Default

Every target gets a fallback payload. This is an implementation safety measure,
not a Podcasting namespace requirement.

The publisher posts fallback metadata at startup when the watched directory has
no current track, and again when a producer removes the drop file. Without
publishing some fallback payload, the relay would keep serving the last
successfully published track payload unless the software gained a separate,
relay-supported clear operation. That stale-track state can route boosts to the
previous song after playback has moved on, stopped, or switched to non-V4V
audio.

If no fallback payment route is configured, the publisher logs a warning and
uses this deliberately dead value block:

```toml
[target.fallback.value.model]
type = "lightning"
method = "lnaddress"

[[target.fallback.value.destinations]]
name = "No V4V payment route"
type = "lnaddress"
address = "no-v4v-track@example.invalid"
split = "100"
```

That default gives the stream no usable payment route when no V4V track is
playing. Configure a station-owned fallback value block only when idle/non-V4V
time should receive payments.

Validation rules:

- There must be at least one target.
- Target names must be unique and non-empty.
- `event_id` must be non-empty and must not be the example placeholder.
- Configured fallback destinations must include non-empty `name`, `type`,
  `address`, and `split`.
- Configured fallback destination `split` values must parse as finite decimals.
- Empty token files are rejected.
- Example placeholder destination addresses such as `YOUR_LIGHTNING_NODE_PUBKEY`
  are rejected.

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
- `--expiry-fallback <seconds>`: expiry for tracks with unknown duration.
  Default: `600`.
- `--no-api`: use embedded tag value routes only; do not query MusicIndex.
- `--api-timeout <seconds>`: MusicIndex route lookup timeout. Default: `5`.
- `--strip-hyphens`: strip hyphens in the plain text now-playing line. Default.
- `--no-strip-hyphens`: preserve hyphens in the text output.
- `--verbose`: print resolved paths and API status.

## Drop File Contract

The producer writes final `*.json` files by temp-file-plus-rename. The publisher
ignores non-JSON files and publishes fallback when the final file disappears.

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
