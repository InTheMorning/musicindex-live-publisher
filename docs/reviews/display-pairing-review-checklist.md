# Pair The Display State With Its Value Block: Review Checklist

Status: open. Use this checklist after display pairing task 002. ADR 0012
becomes `Implemented` only when each item passes.

## Invariants Of ADR 0012

- [ ] A display state names only a block that the publisher published for the
  same `play_id`.
- [ ] The publisher never builds `value` from a text match.
- [ ] `songLine` is the line that `butt` reads, without a change. A test with
  a hyphen in the artist or the title shows it.

## Code

- [ ] The payload JSON did not change.
- [ ] An image is not uploaded again for the second send.
- [ ] The producer computes the song line in one place only.

## Documents

- [ ] `README.md` and the configuration runbook describe `display.json`
  version 2 and name ADR 0012 as its owner.
- [ ] ADR 0008 has a dated sentence that version 2 exists and that ADR 0012
  owns it.
- [ ] `AGENTS.md` §Current State gives the present display body.

## Cross-Repository

- [ ] Relay ADR 0005 is deployed before this publisher.
- [ ] `citizenradio` is told that `songLine` is available for its ICY sync.
