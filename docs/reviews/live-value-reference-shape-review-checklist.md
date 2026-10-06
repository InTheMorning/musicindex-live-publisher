# The Live Value Payload Follows The Model Server: Review Checklist

Status: open. Use this checklist after reference shape task 002. ADR 0010
becomes `Implemented` only when each item passes.

## Invariants Of ADR 0010

- [ ] Each field that the publisher sent before ADR 0010 keeps its name, its
  form and its meaning. The golden test shows it.
- [ ] `line` is present in each payload of a V4V track and has two strings.
- [ ] A missing album gives no `podcastName`. A missing or invalid link gives
  no `link`.
- [ ] `value` keeps the nested `model` form.

## Code

- [ ] The dead block JSON did not change.
- [ ] The history query is still one query with the `LIMIT 1` subquery.
- [ ] The producer and the publisher use the same schema version. The
  contract test covers it.
- [ ] The payload never has exactly the keys `event_id` and `metadata`.

## Documents

- [ ] `README.md` and the configuration runbook describe the drop file
  version 2 and name ADR 0010 as its owner.
- [ ] The payload example in `README.md` has the new fields.
- [ ] ADR 0002 has a dated sentence that version 2 exists and that ADR 0010
  owns it.
- [ ] `AGENTS.md` §Current State and
  `docs/architecture/broadcast-chain-boundaries.md` describe the present.

## Open Decisions Of ADR 0010

- [ ] The link source is selected, or the operator accepts that `link` stays
  absent.
- [ ] The app check with the CurioCaster test feed is recorded.

## Visual

A person must complete these items. Report each one as open until then.

- [ ] An app that shows `line` shows the album and the artist of this stream.
- [ ] The artwork from `image` changes with the track, as before.
