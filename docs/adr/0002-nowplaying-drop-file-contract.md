# ADR 0002: Now-playing drop-file contract

Status: Accepted
Date: 2026-09-04

Amended 2026-09-06: `v4vmm` accepted its ADR 0059 and became the control
surface for this chain. Two statements below now have a known outcome. The
`v4vmm` live client loses its publish half, and `v4vmm` becomes a producer for
its built-in `mpv` player. This contract does not change. See
`docs/architecture/broadcast-chain-boundaries.md`.

## Context

`musicindex-live-publisher` watches for track metadata from producers such as
`mixxx-now-playing` and publishes live value payloads to a MusicIndex relay.
Those producers should not depend on each other or on relay internals.

The relay accepts live value payloads over HTTP, but this service needs a stable
input contract that can be produced by any local process.

## Decision

The input is a JSON drop file owned by `musicindex-live-publisher`, with schema
string `musicindex.nowplaying/1`.

Presence is the signal: a file present in the watched directory means that track
is playing. Removing the file means the track stopped.

Producers must write to a temporary file in the same directory and then `rename`
it into place. Consumers read only renamed files.

The version 1 object fields are:

| Field | Type | Required | Description |
|---|---:|---:|---|
| `schema` | string | yes | Must be `musicindex.nowplaying/1`. Unknown versions are ignored. |
| `target` | string | yes | Publish target name, `default` for the initial single-target deployment. |
| `artist` | string | yes | Display artist for the playing track. |
| `title` | string | yes | Display title for the playing track. |
| `duration_secs` | number or null | no | Track duration in seconds. |
| `image` | string or null | no | Artwork URL. |
| `feed_guid` | string or null | no | MusicIndex feed GUID, when known. |
| `track_guid` | string or null | no | MusicIndex track GUID, when known. |
| `value_routes` | array | yes | Payment routes using the field names from `v4vmm`'s `PaymentRoute`. Empty is valid. |
| `value_routes_source` | string or null | no | Source of the value-route data, for diagnostics. |

Each `value_routes` item uses these exact field names:

| Field | Type | Required | Description |
|---|---:|---:|---|
| `recipient_name` | string or null | no | Destination display name. |
| `route_type` | string or null | no | Payment route type, commonly `node`. |
| `address` | string or null | no | Destination address. |
| `split` | number or null | no | Numeric split supplied by the producer. |
| `fee` | boolean or null | no | Whether the route is a fee destination. |
| `custom_key` | string or null | no | Custom keysend record key. |
| `custom_value` | string or null | no | Custom keysend record value. |

Example:

```json
{
  "schema": "musicindex.nowplaying/1",
  "target": "default",
  "artist": "Alice",
  "title": "Some Track",
  "duration_secs": 187.326,
  "image": "https://example.com/art.png",
  "feed_guid": "1c7a...",
  "track_guid": "9f3e...",
  "value_routes": [
    {
      "recipient_name": "Alice",
      "route_type": "node",
      "address": "03ab...",
      "split": 90.0,
      "fee": false,
      "custom_key": null,
      "custom_value": null
    }
  ],
  "value_routes_source": "musicindex-api"
}
```

## Alternatives Considered

Directly depending on `v4vmm` was rejected because this publisher must build
independently and because the publish half of the `v4vmm` live client is
withdrawn. `v4vmm` ADR 0059 records that decision. `v4vmm` keeps the live item
create and read operations for event registration, and this publisher stays the
only sender of payloads.

Reading relay payloads directly from producers was rejected because the publisher
owns transformation and clear semantics. Producers supply facts about the
playing track and value routes; the publisher owns the relay-facing shape.

Guessing unknown schema versions was rejected. Unknown versions are ignored so a
producer can roll forward without an older publisher misrouting payments.

## Consequences

The contract is a stable boundary between producers and the publisher.
`mixxx-now-playing`, `v4vmm`, or any other producer targets this contract rather
than another producer's internal model.

The temp-file-plus-rename rule prevents the watcher from treating a partially
written file as a track update.

Schema changes require a new version string and parser support.

## Invariants

- Unknown schema versions are ignored.
- Malformed JSON is an input error for the parser.
- A present file means playing.
- Removing a file means stopped.
- `value_routes` uses the exact `PaymentRoute` field names:
  `recipient_name`, `route_type`, `address`, `split`, `fee`, `custom_key`,
  `custom_value`.

## Follow-up Work

Task 002 transforms this contract into direct live value payloads. Later tasks
add directory watching, clearing, relay publishing, configuration, and systemd
deployment.
