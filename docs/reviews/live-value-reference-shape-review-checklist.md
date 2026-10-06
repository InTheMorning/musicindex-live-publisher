# The Live Value Payload Follows The Model Server: Review Checklist

Status: open - 2026-10-06. Tasks 001 and 002 are done, and each mechanical
item passes. The app check and the visual items are open. ADR 0010 becomes
`Implemented` only when each item passes.

## Invariants Of ADR 0010

- [x] Each field that the publisher sent before ADR 0010 keeps its name, its
  form and its meaning. The golden test shows it.
- [x] `line` is present in each payload of a V4V track and has two strings.
- [x] A missing album gives no `podcastName`. No payload has `link`.
- [x] `value` keeps the nested `model` form.

## Code

- [x] The dead block JSON did not change.
- [x] The history query is still one query with the `LIMIT 1` subquery.
- [x] The producer and the publisher use the same schema version. The
  contract test covers it.
- [x] The payload never has exactly the keys `event_id` and `metadata`.

## Documents

- [x] `README.md` and the configuration runbook describe the drop file
  version 2 and name ADR 0010 as its owner.
- [x] The payload example in `README.md` has the new fields.
- [x] ADR 0002 has a dated sentence that version 2 exists and that ADR 0010
  owns it.
- [x] `AGENTS.md` §Current State and
  `docs/architecture/broadcast-chain-boundaries.md` describe the present.

## Open Decisions Of ADR 0010

- [x] The link source: the operator decided on 2026-10-06 that `link` stays
  absent.
- [ ] The app check with the CurioCaster test feed is recorded.

## Visual

A person must complete these items. Report each one as open until then.

- [ ] An app that shows `line` shows the album and the artist of this stream.
- [ ] The artwork from `image` changes with the track, as before.
