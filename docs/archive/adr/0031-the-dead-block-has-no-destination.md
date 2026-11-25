# ADR 0031: The Dead Block Has No Destination

> Archived: superseded by [ADR 0033](../../adr/0033-the-dead-block-has-one-unreachable-destination.md) on
> 2026-11-25. The v4vmusic.com client rejects a block with no destination.

- Status: Superseded by ADR 0033 on 2026-11-25. Accepted on 2026-11-24 by the operator. The orchestrator session relayed the decision.
- Date: 2026-11-24
- Changes: ADR 0029 §The Dead Block (the destination item), §Publisher Behavior (the item "The publisher never
  publishes a block with zero destinations") and §Invariants (the same invariant).
- Bar: crosses repositories. The relay passes the dead block to the apps. citizenradio reads it to find a block that
  is not payable. An agent can also make the opposite choice by mistake: ADR 0029 and the tests of relay lease task 002
  required one destination.
- Operator choice, 2026-11-24: the dead block has zero destinations. The operator said: "dead blocks are not ideal
  because they look payable".

## Context

The dead block of ADR 0029 had one destination: the name "No V4V payment route" and the address
`no-v4v-track@example.invalid`. A payment to that address fails. But an app reads a block with a destination as a
payable block. On the test station on 2026-11-24, the apps kept the boost button on during the dead block.

The orchestrator read the bundle of the v4vmusic.com client earlier. That client throws when `value.destinations` is
missing. So the field must stay.

The relay does not examine `value.destinations` (`musicindex-live-relay` `src/`). It keeps and sends the payload as the
publisher sends it.

## Decision

- The dead block has `value.destinations` as an empty array `[]`. The field is always present.
- These items of ADR 0029 do not change: `value.model` `{"type": "lightning", "method": "lnaddress"}`, the title
  setting `dead_block_title`, the image setting `dead_block_image`, no `feedGuid`, no `itemGuid`, and a new
  `blockGuid` for each change.
- No setting adds a destination to the dead block. A `[target.fallback]` table stays a configuration error.
- A track block always has one or more destinations. An empty `value_routes` list gives the dead block (ADR 0029).
- The dead block pays nobody. This invariant of ADR 0029 does not change.
- The keepalive check of the publisher reads a block as a dead block when it has no destination. A publisher of an earlier version
  can keep a dead block with the address `no-v4v-track@example.invalid` on the relay. The check also reads that block
  as a dead block.

## Invariants

- The dead block has `value.destinations` equal to `[]`, and the field is present.
- No publish path makes a track block with zero destinations.

## Verification

Mechanical:

- `livevalue_dead_payload_has_the_fixed_dead_block_values` (`src/livevalue.rs`): the default dead block has
  `destinations: []`.
- `dead_block_of_a_configured_target_has_no_destination` (`tests/config.rs`): a target with `dead_block_title` and
  `dead_block_image` gives a dead block with `destinations: []` and the same model.
- `watcher_empty_value_routes_publishes_dead_block` and the other watcher tests of the dead block (`tests/watcher.rs`):
  each dead block has `destinations: []`.
- `livevalue_dead_block_is_no_destination_or_the_old_dead_address` (`src/livevalue.rs`): the keepalive check reads the
  new and the old dead block as a dead block, and a track block as payable.

Manual, by a person, because a test cannot reach the apps. On the test station, play a jingle. v4vmusic.com and
Podcast Guru must show the dead block title with no error. They must show no boost for the block.

## Alternatives Considered

- **Keep the destination at `example.invalid` (ADR 0029).** Rejected by the operator. The apps show the block as
  payable.
- **Remove the field `destinations`.** Rejected. The v4vmusic.com client throws when the field is missing.
- **Send no block, so that the relay keeps an empty snapshot `{}`.** Rejected in ADR 0029. A client can keep the last
  block and pay the last artist.

## Consequences

- An app that reads the destinations finds no payee. Inference: the app turns off the boost button. The manual check
  confirms or rejects this.
- citizenradio reads the address host `example.invalid` to find a block that is not payable. A dead block with no
  destination does not have that host. The citizenradio session owns that code.
- If an app fails with an empty `destinations` array, the operator puts back the binary of ea50623, and this record
  needs a new decision.

## References

- `docs/adr/0029-producer-liveness-dead-block-and-remote-value-title.md`
- `docs/runbooks/musicindex-live-publisher-configuration.md` §Dead Block
