# Relay Lease Keepalive

Date: 2026-09-27. This plan states no rule. ADR 0005 owns the rules for this
work.

`musicindex-live-relay` plans to make "on air" a lease that only the
broadcaster renews. The proposal is
`docs/plans/live-lease-heartbeat-proposal.md` in that repository.

## Present Behavior

This service sends no stop message today. When a drop file is removed, the
publisher sends the target's fallback block to the metadata route. That block
pays a dead route by default, but configuration can make it pay the station.

A removed drop file means a non-V4V track, unreadable tags, an expired timer
or a stopped producer. The publisher cannot tell these conditions apart. ADR
0005 records why that blocks the lease design.

## Order Of Work

1. The relay accepts its lease ADR. This service needs the keepalive route and
   the interval from it.
2. An operator completes the Mixxx duration check in ADR 0005 §Verification.
3. ADR 0005 becomes Accepted.
4. Task: the producer takes the lock, uses the stream duration only, and
   applies the maximum.
5. Task: the publisher replaces the configured fallback with the dead block,
   tests the producer lock, and sends the keepalive.
6. Task: `v4vmm` stops reading `fallback_configured`. This is a commit in
   `v4vmm`.

Steps 4 and 5 go in one commit with the ADR 0002, `AGENTS.md` and runbook
changes that ADR 0005 lists.

## Later Decisions

- The talk break block from `v4vmm`, while `mixxx-now-playing` owns the drop
  directory.
- The MIDI link to Mixxx for the deck play state. It supersedes the payment
  timing rule in ADR 0005.
