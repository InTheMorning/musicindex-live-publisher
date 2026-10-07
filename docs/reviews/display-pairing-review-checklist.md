# Pair The Display State With Its Value Block: Review Checklist

Status: closed - 2026-10-06. Each item passes. This checklist is the named
artifact for the `Implemented` status of ADR 0012. `README.md` does not
describe `display.json`, so the configuration runbook holds version 2. The
producer calls `render_now_playing_line` for the song file and for
`display.json` with the same arguments from the same display state.

## Invariants Of ADR 0012

- [x] A display state names only a block that the publisher published for the
  same `play_id`.
- [x] The publisher never builds `value` from a text match.
- [x] `songLine` is the line that `butt` reads, without a change. A test with
  a hyphen in the artist or the title shows it.

## Code

- [x] The payload JSON did not change.
- [x] An image is not uploaded again for the second send.
- [x] The producer computes the song line in one place only.

## Documents

- [x] `README.md` and the configuration runbook describe `display.json`
  version 2 and name ADR 0012 as its owner.
- [x] ADR 0008 has a dated sentence that version 2 exists and that ADR 0012
  owns it.
- [x] `AGENTS.md` §Current State gives the present display body.

## Cross-Repository

- [x] Relay ADR 0005 is deployed before this publisher. Done 2026-10-06 on
  `api.musicindex.org` with publisher `r83`. A display state that went out
  before its payload went out again with `value` in the next poll.
- [x] `citizenradio` is told that `songLine` is available for its ICY sync.
  Done 2026-10-06 in `citizenradio`
  `docs/plans/upstream-live-metadata-plan.md`, §Available Now.
