# MusicIndex Live Publisher Plan

Status: Proposed
Date: 2026-09-03

## Goal

A standalone systemd service that watches a directory for now-playing metadata,
transforms it into a Podcasting 2.0 live value payload, and publishes it to a
MusicIndex live relay so listening apps route boosts to the correct artists.

It depends on no other project at build time. Its only interfaces are a
documented drop-file contract on the input side and the relay's HTTP API on the
output side.

## Naming

The working name for this was "splitkit", but `~/build/splitkit` is already the
**relay** — package `musicindex-live-relay`, the server this publishes *to*.
Reusing the name for the client would be actively confusing.

This plan uses **`musicindex-live-publisher`**: the symmetric client to
`musicindex-live-relay`. Rename freely, but keep it distinct from the relay.

## Non-Goals

- Replacing or re-implementing the relay. It exists and is unchanged by this work.
- Removing the client from `v4vmm`. That is agreed but belongs to a separate plan
  in that repo — see Cutover.
- Chapter blocks, or any block type other than `type: "music"`.
- Deriving splits from anything but supplied metadata. No fuzzy matching, no
  title or filename inference.
- A UI. This is a headless service.

## Current State

### What already exists

| Component | Location | Role |
|---|---|---|
| `musicindex-live-relay` | `~/build/splitkit` | Server. Socket.IO + SSE + HTTP, memory-only. No v4vmm dependency. |
| Live client | `v4vmm/src/api.rs`, `v4vmm/src/cli.rs` | Creates live items, publishes metadata. ADR 0018, ADR 0019. ~350 lines. |
| Payload examples | `~/build/splitkit/hgh-example-{1,2,3}.json` | Real Curiohoster live value payloads. |
| Metadata producer | `mixxx-now-playing` (in flight) | Emits V4V track metadata with `Value Routes`. Presence means a track is playing. |

### The gap this project closes

The relay accepts two body forms, distinguished by exact key match: a body with
exactly the keys `event_id` and `metadata` is treated as **wrapped**; anything
else is treated as a **direct** live value payload and passed through opaquely.

`v4vmm` publishes the wrapped form containing a `NowPlayingUpdate`. Listening
apps consuming `remoteValue` expect the direct form, with splits under
`value.destinations`. Nothing currently emits that shape. Producing it is the
substance of this project — extraction is the small part.

### Verified payload semantics

Confirmed by reading all three example payloads, not assumed:

- `type` is `"music"` for track blocks.
- `startTime` is `0` for music blocks. No stream-elapsed clock is needed.
- `duration` is **seconds as a float** for music blocks (`187.326`, `213.875`).
  The `316800` in example 1 is a `chapter` block and does not apply.
- `eventGuid` is identical across all examples; `blockGuid` differs per block.
  `eventGuid` identifies the live item, `blockGuid` identifies the track.
- `split` values are **decimal strings**: `"49"`, `"0.49"`, `"49.51"`.
- `eventTimestamp` is present only on the chapter example. Optional for music.
- The relay does not validate direct payloads. Correctness is entirely this
  service's responsibility.

## Target State

### Repository layout

A workspace member in this repository, alongside `mixxx-now-playing`:

```text
musicindex-live-publisher/
  Cargo.toml                # publisher package and workspace root
  src/
  mixxx-now-playing/
```

The repository is now a Cargo workspace. The root `Cargo.lock` owns dependency
resolution for both binaries, and `mixxx-now-playing` depends on the root
publisher crate in tests for drop-file contract coverage.

### Dependencies

```toml
notify        = "8"      # inotify on the drop directory
reqwest       = { version = "0.13", features = ["blocking", "json"] }
serde         = { version = "1", features = ["derive"] }
serde_json    = "1"
uuid          = { version = "1", features = ["v4"] }   # blockGuid per track
toml          = "1.0"
anyhow        = "1"
signal-hook   = "0.3"
tracing            = "0.1"
tracing-subscriber = { version = "0.3", features = ["env-filter"] }
```

Blocking `reqwest` on the main loop is adequate: this service handles one event
per track change, not a request stream. No async runtime.

### Input: the drop-file contract

A directory watched with `notify`. Any producer writes JSON files there. The
contract is owned by this project and documented in an ADR, so `mixxx-now-playing`
and `v4vmm` can both target it without either depending on the other.

**Presence is the signal**, matching what `mixxx-now-playing` already does: a file
present means that track is playing; its removal means it stopped.

```json
{
  "schema": "musicindex.nowplaying/1",
  "target": "default",
  "artist": "Alice",
  "title": "Some Track",
  "duration_secs": 187.326,
  "image": "https://...",
  "feed_guid": "1c7a...",
  "track_guid": "9f3e...",
  "value_routes": [
    {"recipient_name": "Alice", "route_type": "node",
     "address": "03ab...", "split": 90.0, "fee": false,
     "custom_key": null, "custom_value": null}
  ],
  "value_routes_source": "musicindex-api"
}
```

- `schema` is mandatory and versioned. An unknown version is logged and ignored,
  never guessed at.
- `target` names which publish target this belongs to. Today there is one; see
  Configuration.
- `value_routes` uses the `PaymentRoute` field names from `v4vmm/src/api.rs:238`,
  so producers can serialise their existing type without a translation layer.
- Files are read only after the writer has finished. Producers must write to a
  temp file in the same directory and `rename` — which `mixxx-now-playing`
  already does.

`mixxx-now-playing` needs no code change to feed this: point `--id3-file` at the
drop directory and run `--format json`. Its JSON output must match this schema,
which is a small alignment task in that project, tracked separately.

### Transform: PaymentRoute to live value

The core of the service. The two shapes do not line up, so each mapping is
explicit:

| Drop file (`PaymentRoute`) | Live value destination | Note |
|---|---|---|
| `recipient_name` | `name` | |
| `route_type` | `type` | `"node"` in practice |
| `address` | `address` | |
| `split` (f64) | `split` (**string**) | Decimal string. `90.0` renders `"90"`, `0.49` renders `"0.49"` |
| `custom_key` | `customKey` | Omit when null |
| `custom_value` | `customValue` | Omit when null |
| `fee` | `fee` | Omit when null |

Assembled block:

```json
{
  "title": "<title>",
  "image": "<image, omitted when absent>",
  "description": "",
  "type": "music",
  "startTime": 0,
  "duration": 187.326,
  "eventGuid": "<live item event_id>",
  "blockGuid": "<fresh uuid v4 per track>",
  "feedGuid": "<feed_guid, omitted when absent>",
  "itemGuid": "<track_guid, omitted when absent>",
  "value": {
    "model": {
      "type": "lightning",
      "method": "keysend"
    },
    "destinations": [ ... ]
  }
}
```

`blockGuid` is generated once per drop file and reused for any republish of that
same track, so a retry does not read as a new block.

### Output: publishing

```http
POST {endpoint}/v1/liveitems/{event_id}/metadata
Authorization: Bearer <broadcaster_token>
Content-Type: application/json
```

The body is the direct payload above. It must **not** have exactly the two keys
`event_id` and `metadata`, or the relay will treat it as wrapped.

Relay status codes to handle distinctly, from its README:

- `400` path and body event ID differ — not applicable to direct form
- `401` / `403` token missing, malformed, or wrong — fatal, stop retrying
- `404` event does not exist — fatal, the live item was lost
- `413` body over 64 KiB — drop the payload, log, do not retry
- `429` rate limited — back off and retry

### Clearing: what happens when the track stops

When the drop file is removed, the last published payload stays live at the
relay. Listeners would keep routing boosts to the previous track's destinations.
**That is misrouted money, and it is the most important correctness requirement
in this plan.**

On clear, the service publishes a fallback payload with a fresh `blockGuid` and
`type: "music"`. If no station fallback is configured, the service warns and
publishes a deliberately dead fallback route instead, so a stop does not leave
stale track destinations live.

### Configuration

TOML at `/etc/musicindex-live-publisher/config.toml`, or `--config`.

Modelled as a **list of targets from the start**, even though there is one today.
The immediate goal is a single private stream; the longer-term goal is a tool
podcasters can run for public indexed music podcasts, and a single hardcoded
event would have to be unpicked to get there.

```toml
watch_dir = "/run/musicindex-live-publisher/nowplaying"
endpoint  = "https://api.musicindex.org"

[[target]]
name       = "default"
event_id   = "1873a383-..."
token_file = "/etc/musicindex-live-publisher/default.token"   # mode 0600

  [target.fallback]
  title = "Homegrown Hits"
  destinations = [
    { name = "Station", type = "node", address = "03...", split = "100" },
  ]
```

Live items are **provisioned out of band**. The relay returns the broadcaster
token exactly once and stores only a SHA-256 hash, so it cannot be recovered.
The service reads the token, never creates a live item, and never writes a token
to disk. `event_id` stays stable so it can be advertised in an RSS live value
block.

A `provision` subcommand wraps `POST /v1/liveitems` and prints the config stanza
to paste, so the one-time step is not a manual curl.

### Service hardening

Model the unit on the relay's own at `~/build/splitkit/systemd/`:
`NoNewPrivileges`, `ProtectSystem=strict`, `ProtectHome`, `PrivateTmp`,
`Restart=on-failure`. Add `ReadWritePaths` for the watch directory and
`LoadCredential=` for the token so it is not readable process-wide.

## Phases

### Phase 1 — contract and transform

Write the drop-file ADR. Implement drop-file parsing, the `PaymentRoute` to
destinations transform, and payload assembly. Pure functions, no I/O, no network.
Golden-file tests against `hgh-example-2.json` and `hgh-example-3.json` — the two
music blocks — proving the assembled shape matches real payloads.

### Phase 2 — watcher and lifecycle

`notify` on the watch directory. Debounce, ignore partial writes, handle the
directory not existing at startup. Emit a transform on create or modify, and the
fallback payload on remove. Still no network: `--dry-run` prints the payload.

### Phase 3 — publish

The relay client, config, token loading, status-code handling, and backoff. The
`provision` subcommand. Integration tests against a local relay instance built
from `~/build/splitkit`.

### Phase 4 — deploy

Systemd unit, hardening, `ReadWritePaths` and `LoadCredential`. Align
`mixxx-now-playing --format json` with the drop-file schema. Run the private
stream end to end.

## Cutover

Removing the client from `v4vmm` is agreed, but it is a change to a different
repository and needs its own plan and commit there. Sequence:

1. This service runs the private stream successfully for a full show.
2. Open a plan in `v4vmm` to remove `v4vmm liveitem *`, superseding ADR 0018 and
   ADR 0019 with a pointer to this project.
3. Remove the live surface from `v4vmm/src/api.rs` and `v4vmm/src/cli.rs`.

Do not remove anything from `v4vmm` before step 1. It is the only working
publisher today and the fallback if this service has a problem.

## Risks

| Risk | Mitigation |
|------|------------|
| **Stale destinations after a track stops route boosts to the wrong artist** | Fallback payload published on clear; missing station fallback uses a warned dead fallback route |
| Relay does not validate direct payloads, so a malformed block fails silently | Golden-file tests against the real examples; validate before send, never after |
| Payload accidentally has exactly `event_id` and `metadata` keys and is read as wrapped | Assert against that key set in a unit test |
| Broadcaster token cannot be recovered if lost | Out-of-band provisioning, `LoadCredential`, documented re-provision path |
| Producer writes a partial file and it is read mid-write | Contract requires temp-file-plus-rename; watcher ignores files failing schema parse |
| `split` emitted as a number rather than a string | Explicit string conversion with a test asserting `"0.49"` not `0.49` |
| Drop-file schema drifts from `mixxx-now-playing` output | Versioned `schema` field; unknown versions ignored and logged, never guessed |
| Single-event assumptions block the multi-podcaster goal | Config is a target list from day one |

## Test Strategy

- Golden-file: assembled payload matches `hgh-example-2.json` and
  `hgh-example-3.json` structure for a given drop file.
- Transform units: split formatting (`90.0`→`"90"`, `0.49`→`"0.49"`), null
  `custom_key` omitted, empty `value_routes` handled.
- Key-collision: assembled payload never has exactly `{event_id, metadata}`.
- Watcher: create, modify, remove, partial write, unknown schema version,
  malformed JSON. Driven with `tempfile`, no real inotify races.
- Clear semantics: removing the drop file publishes the fallback, with a
  different `blockGuid` than the track it replaced.
- Relay integration: build `~/build/splitkit`, run on a loopback port, assert
  published payloads come back over the `remoteValue` HTTP snapshot endpoint.
- Status codes: 401, 403, 404, 413, 429 each produce the documented behaviour.
  Stubbed, no real API calls.

## Rollback Strategy

The service is additive. Nothing existing depends on it until cutover.

- Stop the unit. The relay keeps serving its last snapshot; `v4vmm liveitem
  publish` remains available to correct it manually.
- `--dry-run` at any phase prints payloads without publishing.
- Because `v4vmm`'s client is untouched until after a successful show, rollback
  before cutover is stopping one systemd unit.

## Decisions

Resolved 2026-09-03.

- **Input is a generic watched directory**, not a direct Mixxx or v4vmm coupling.
  Any producer can write the documented drop file, so the service depends on
  neither project.
- **Publishes the direct live value payload**, not the wrapped
  `{event_id, metadata}` form, so listening apps receive splits in the shape they
  read.
- **Live items are provisioned out of band with a stable `event_id`.** Immediate
  need is one private stream; config is a target list so the multi-podcaster goal
  does not require reworking the data model.
- **The v4vmm client is removed only after cutover**, under a separate plan in
  that repository.
