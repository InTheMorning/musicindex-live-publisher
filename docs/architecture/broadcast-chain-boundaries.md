# Broadcast Chain Boundaries

This document says what this repository consumes, what it produces, and which
neighbor owns each contract. It is the local half of a map that four
repositories share.

The full chain map lives in `v4vmm`:
`docs/architecture/broadcast-chain.md`.

## Neighbors

| Neighbor | Repository | Relation |
|---|---|---|
| `v4vmm` | `v4vmm` | Writes the MusicIndex tags this chain reads. Controls this service. Reads relay snapshots. |
| `musicindex-live-relay` | `splitkit` | Receives the payloads this service sends. |
| Mixxx | external | The player that `mixxx-now-playing` observes. |
| Liquidsoap | external | A future player. It needs a producer that writes the same drop file. |

This repository has no build dependency on `v4vmm` and no build dependency on
`splitkit`. Each contract below is a file format or an HTTP call.

## What This Repository Consumes

### MusicIndex tags in the audio file

`mixxx-now-playing` reads these `TXXX` frames with `lofty`:

- `MusicIndex Feed Guid`
- `MusicIndex Track Guid`
- `MusicIndex Image`
- `MusicIndex Value Routes`

`v4vmm` writes those frames at download time. `Value Routes` holds a JSON array
that uses the `PaymentRoute` field names.

If a file carries no `Value Routes` tag, the producer has no splits to report.
The publisher then sends a payload with no destinations, and boosts do not reach
the artist. `v4vmm` owns that defect, not this repository.

### The `v4vmm` configuration file

`mixxx-now-playing` reads `music_dir` and `musicindex_endpoint` from
`~/.config/v4vmm/config.toml`. The read is optional and read-only. The producer
falls back to flags, environment variables, and `~/V4Vmusic`.

## What This Repository Produces

### The drop file

`musicindex.nowplaying/1`. ADR 0002 defines it. This repository owns the
contract.

Every producer targets this contract. `mixxx-now-playing` writes it today.
`v4vmm` writes it later for its built-in `mpv` player. A liquidsoap producer
writes it after that.

A change to the fields needs a new schema version. Unknown versions are ignored,
never guessed.

### The show log

`musicindex.showlog/1`, an append-only JSON Lines file. ADR 0003 defines it.
This repository owns the contract.

This service records what played and when. `v4vmm` reads the log and generates
the recorded episode: the RSS item, the chapters, and the value time split
blocks. This service generates nothing.

Two fields matter more than the rest:

- `observed_at` is the producer time. An episode built from a local encoder
  recording aligns to it, because that recording is made before every delay
  that `stream_delay_secs` covers.
- `aired_at` is the time this service sent the payload. It serves the live path
  only and must not drive a recorded episode. A recorder that pulls the stream
  after icecast sits closer to this time instead.
- A later entry with the same `event_guid` and `block_guid` supersedes an
  earlier one. That is a route revision, and only the last entry is correct.

### The live value payload

This service sends the direct Podcasting 2.0 live value payload to the relay. It
is the only sender in the chain.

The relay separates two body forms by exact key match. A body with exactly the
keys `event_id` and `metadata` is the wrapped form. Any other body is a direct
payload. Listener apps read the direct form, so this service must never produce
a body with exactly those two keys.

## What Other Components Control Here

### Service control from `v4vmm`

`v4vmm` starts and stops `musicindex-live-publisher@<instance>.service` and
`mixxx-now-playing.service`. It uses `systemctl --user` on the local host, and
the same commands through `ssh` on a remote host.

The unit files in `systemd/` stay in this repository. `v4vmm` does not write
them.

`StartLimitBurst=5` in the publisher unit means a wrong token drives the unit to
the `failed` state. A control surface must show that state and offer a reset.
A plain start command does nothing while a unit is failed.

### Configuration

This repository owns `config.toml` and the setup rules in
`scripts/setup-mixxx-musicindex.sh`. No other component writes that file.

`v4vmm` reads the file to display its content. Every change from `v4vmm` runs
`musicindex-live-publisher provision` or `setup-mixxx-musicindex`.

Keep the command-line interface stable, because a control surface now depends on
the exit codes and the output.

## Known Limits

- The relay keeps state in memory. A relay restart discards the live item, the
  token, and the snapshot. The event then dies.
- The relay also removes an event after an idle TTL. The default is 24 hours.
  A configured `event_id` can therefore stop working with no restart and no
  change on this side.
- The relay returns a broadcaster token one time only.
- The drop-file contract has no pause state. A producer reports play or stop.
- This service has no remote control API. Remote control uses `ssh` today.

## Future Work

- A remote control API for this service. The liquidsoap work needs more than
  start and stop, and that is the correct moment to design it.
- A liquidsoap producer that writes the same drop file.

## References

- `docs/adr/0001-rust-now-playing-lifecycle.md`
- `docs/adr/0002-nowplaying-drop-file-contract.md`
- `docs/runbooks/musicindex-live-publisher-configuration.md`
- `v4vmm`: `docs/architecture/broadcast-chain.md`
- `v4vmm`: `docs/adr/0059-broadcast-control-surface.md`
- `splitkit`: `README.md` and `docs/interoperability.md`
