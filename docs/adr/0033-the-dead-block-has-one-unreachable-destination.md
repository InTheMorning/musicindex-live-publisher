# ADR 0033: The Dead Block Has One Destination That A Payment Cannot Reach

- Status: Accepted on 2026-11-25 by the operator.
- Date: 2026-11-25
- Supersedes: ADR 0031 (`docs/archive/adr/0031-the-dead-block-has-no-destination.md`).
- Bar: crosses repositories. The relay passes the dead block to the apps, and each app has its own check of the block.
  An agent can also make the opposite choice by mistake: ADR 0031 removed the destination because of a known problem.
- Operator choice, 2026-11-25: the dead block gets the destination of ADR 0029 again. The operator said: "it will fix
  the incorrect metadata in other apps and citizenradio will still disable its boost button".

## Context

ADR 0031 gave the dead block `value.destinations` as `[]`, because a dead block with a destination looks payable.

On 2026-11-25 the operator reported that the value block and the metadata do not change at a jingle. The apps were
CitizenRadio and Podcast Guru. Station log step 36 shows the relay output at each jingle. The relay sent the dead block
with `destinations: []` on the Socket.IO stream and on the SSE stream.

The publisher session ran the v4vmusic.com live value client (chunk `6832.5c303e27eb6db5df.js`, fetched on
2026-11-25) with the payloads of step 36. The parse function throws "Invalid event: missing value.destinations" when
the array is missing or empty. The client then keeps the last block. Thus during a jingle the app shows the last song,
and a boost pays the last artist. ADR 0029 made the dead block to stop that result.

The same client applied the dead block of ADR 0029 with one destination. Podcast Guru is closed code. It shows the
same fault, and the inference is that it has the same rule.

CitizenRadio reads a destination with the reserved host `example.invalid` as not payable. It shows the station lines
and turns off its boost button for that block (CitizenRadio ADR 0028).

## Decision

1. The dead block has one destination: `type` `lnaddress`, `name` "No V4V payment route", `address`
   `no-v4v-track@example.invalid`, `split` `"100"`. A payment to that address fails, so the dead block pays nobody.
2. These items of ADR 0029 do not change: `value.model` `{"type": "lightning", "method": "lnaddress"}`, the settings
   `dead_block_title` and `dead_block_image`, no `feedGuid`, no `itemGuid`, and a new `blockGuid` for each change.
3. No setting changes the destination. A `[target.fallback]` table stays a configuration error.
4. The keepalive check of the publisher reads a block as a dead block in two conditions: each destination has the dead
   block address, or the block has no destination. The relay can keep a dead block with no destination from a publisher of the ADR 0031
   version.

### Durable And Situational Rules

Each rule of this record is situational. It describes the dead block for the apps of 2026-11-25. Remove it when a
record about the dead block supersedes this record.

## Invariants

- The dead block has one destination and no other destination, at `no-v4v-track@example.invalid`.
- No publish path makes a track block with zero destinations.

## Verification

Mechanical:

- `livevalue_dead_payload_has_the_fixed_dead_block_values` (`src/livevalue.rs`): the default dead block has the one
  destination of rule 1.
- `dead_block_of_a_configured_target_has_the_unreachable_destination` (`tests/config.rs`): a target with
  `dead_block_title` and `dead_block_image` gives the same destination.
- `watcher_empty_value_routes_publishes_dead_block` and the other watcher tests of the dead block (`tests/watcher.rs`).
- `livevalue_dead_block_is_the_dead_address_or_no_destination` (`src/livevalue.rs`): rule 4.

Manual, by a person, because a test cannot operate the apps. On the test station, play a jingle. v4vmusic.com and
Podcast Guru must show the dead block title. CitizenRadio must show the station lines and a gray boost button.

## Alternatives Considered

- **Keep ADR 0031.** Rejected by the operator. v4vmusic.com rejects the block and keeps the last song payable.
- **Give the dead block the lightning address of the station.** Not selected. Each app accepts it, but the rule "the
  dead block pays nobody" changes, and the station needs an address.

## Consequences

- The apps that read only the destinations show a boost button during a jingle. A boost to the dead block fails. The
  operator accepts this.
- v4vmusic.com shows the dead block title during a jingle. Log step 30 showed that it keeps the dead block until a
  reload. The manual check examines if it changes at the next song.

## References

- `docs/archive/adr/0031-the-dead-block-has-no-destination.md`
- `docs/adr/0029-producer-liveness-dead-block-and-remote-value-title.md`
- `docs/runbooks/test-station-log.md`, steps 30 and 36
