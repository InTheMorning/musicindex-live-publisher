# The Album In The Display State: Review Checklist

Status: open - 2026-10-06. Task 001 is done, and each item above
§Cross-Repository passes. The deployment item and the visual item are open.
ADR 0013 becomes `Implemented` only when each item passes.

## Invariants Of ADR 0013

- [x] The album of a display state comes from the same history row as its
  artist and title.
- [x] A track that is not V4V gets its album also.
- [x] The publisher never sends an empty `album`.

## Code

- [x] The live value payload and the drop file did not change.
- [x] The pairing tests pass with no change.

## Documents

- [x] The configuration runbook gives `display.json` version 3 and names
  ADR 0013 as its owner.
- [x] ADR 0012 has a dated sentence that version 3 exists and that ADR 0013
  owns it.
- [x] `AGENTS.md` §Current State gives the present display body.

## Cross-Repository

- [ ] Relay ADR 0006 is deployed before this publisher.

## Visual

A person must complete this item. Report it as open until then.

- [ ] The private app shows the album for a V4V track and for a track that is
  not V4V.
