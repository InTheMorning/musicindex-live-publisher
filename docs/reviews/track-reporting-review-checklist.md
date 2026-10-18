# Each Producer Reports Each Track: Review Checklist

Status: open - 2026-10-18. Tasks 001 and 002 are done, and each mechanical
item passes. The manual items are open. ADR 0014 becomes `Implemented` only
when each item passes.

## Invariants Of ADR 0014

- [x] Each producer reports each track that plays, payable or not. A
  non-V4V track gives a present file with an empty `value_routes` in the
  two modes.
- [x] An absent file means that no track plays.
- [x] The publisher publishes the dead block for a present file with no
  routes, as before.

## Code

- [x] The producer never takes a route from a non-V4V file, and starts no
  route lookup for it. A test covers a non-V4V file that holds MusicIndex
  tags.
- [x] The producer and the publisher use `musicindex.nowplaying/3`. The
  contract test covers a payable and a non-payable file.
- [x] Each test of a V4V row in `connector/link.rs` kept its result.
- [x] The payload and the dead block JSON did not change. The golden test
  shows it.
- [x] `Cargo.lock` did not change, or the review records why.

## Documents

- [x] `README.md` and the configuration runbook describe version 3 and name
  ADR 0014 as its owner.
- [x] The doc comment of `DropFile` states the meaning of presence of
  ADR 0014.
- [ ] ADR 0014 gets the status `Implemented`, with a dated sentence that
  names this checklist. Its acceptance sentence about version 2 is removed.
- [x] The ADR 0002 amendment of 2026-10-18 says that the producer writes
  version 3. Its sentence about version 2 until the implementation is
  removed.
- [x] `AGENTS.md` §Current State describes the present.

## Manual

A person must complete these items on the Mixxx host. Report each one as
open until then.

- [ ] With the connector, a non-V4V track that plays gives a drop file with
  an empty `value_routes`. A stop of the deck removes the file.
- [ ] Without the connector, a non-V4V track gives a drop file with an empty
  `value_routes`, and the next history row replaces it.
- [ ] During a non-V4V track, the relay shows the dead block.
