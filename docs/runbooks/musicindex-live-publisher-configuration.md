# MusicIndex Live Publisher Configuration

## Purpose

List the configuration files, service flags, producer flags, and TOML fields
used by the Mixxx now-playing to MusicIndex live value pipeline.

## Runtime Files

Publisher config:

```text
~/.config/musicindex-live-publisher/config.toml
```

Publisher tokens:

```text
~/.config/musicindex-live-publisher/tokens/<target>.token
```

Producer config:

```text
~/.config/v4vmm/config.toml
```

Drop directory under the packaged systemd user units:

```text
$XDG_RUNTIME_DIR/musicindex-live-publisher/nowplaying
```

OBS text output:

```text
/tmp/mixxx-now-playing.txt
```

## Publisher TOML

Example:

```toml
watch_dir = "/run/user/1000/musicindex-live-publisher/nowplaying"
endpoint = "https://api.musicindex.org"

[[target]]
name = "default"
event_id = "replace-with-provisioned-event-guid"
token_file = "~/.config/musicindex-live-publisher/tokens/default.token"

  [target.fallback]
  title = "Homegrown Hits"
  image = "https://example.com/station-art.png"
  destinations = [
    { name = "Station", type = "node", address = "03your-node-pubkey", split = "100" },
  ]
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
  usually under `~/.config/musicindex-live-publisher/tokens/`. `~/` expands to
  the service user's home directory. `%d/<name>` is also supported for custom
  systemd units that provide `CREDENTIALS_DIRECTORY`.
- `[target.fallback]`: station-owned fallback live value block. The publisher
  refuses to start without a fallback.

Fallback fields:

- `title`: fallback title shown when no track is playing.
- `image`: optional artwork URL.
- `destinations`: one or more fallback payment destinations.

Destination fields:

- `name`: recipient label.
- `type`: destination type, usually `node`.
- `address`: node pubkey or relay-supported destination address.
- `split`: decimal string, for example `"100"` or `"49.51"`.
- `customKey`: optional custom record key.
- `customValue`: optional custom record value.
- `fee`: optional boolean.

Validation rules:

- There must be at least one target.
- Target names must be unique and non-empty.
- `event_id` must be non-empty and must not be the example placeholder.
- Every target must define fallback destinations.
- Fallback destinations must include non-empty `name`, `type`, `address`, and
  `split`.
- `split` must parse as a finite decimal.
- Empty token files are rejected.

## Publisher Instances

The packaged `musicindex-live-publisher.service` is the Mixxx pipeline. It reads
`~/.config/musicindex-live-publisher/config.toml` and watches:

```text
$XDG_RUNTIME_DIR/musicindex-live-publisher/nowplaying
```

Run only one `mixxx-now-playing.service`; Mixxx has one active desktop history
source. If a future non-Mixxx producer needs its own pipeline, use a separate
publisher instance with its own config and drop directory:

```text
~/.config/musicindex-live-publisher/<instance>/config.toml
~/.config/musicindex-live-publisher/<instance>/tokens/default.token
$XDG_RUNTIME_DIR/musicindex-live-publisher/<instance>/nowplaying
```

The package installs `musicindex-live-publisher@.service` for that future
instance layout. The future producer must write its drop file into that
instance's watch directory.

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
  `/tmp/mixxx-now-playing.txt`.
- `--id3-file <path>`: metadata output. In JSON mode this is the publisher drop
  file. Default: `/tmp/mixxx-now-playing-metadata.txt`.
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

  [target.fallback]
  title = "Local Test Station"
  destinations = [
    { name = "Station", type = "node", address = "03localtest", split = "100" },
  ]
```

Create the directory, then run:

```bash
install -d -m 0700 /tmp/musicindex-live-publisher-nowplaying
musicindex-live-publisher --config /tmp/publisher.toml --dry-run --verbose
```
