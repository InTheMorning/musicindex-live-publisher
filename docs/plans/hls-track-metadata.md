# Track Metadata In The HLS Stream: Phase Plan

Status: Proposed 2026-10-06. This plan does not make rules. ADR 0009 owns
them. No task packet exists yet. Each packet waits for the gate that it
needs, because a packet must not make an architecture decision.

## Goal

A tagger writes the MusicIndex frame of ADR 0009 into each HLS segment, at
the ICY title that releases it. A player that reads the frame gets the track,
the artwork and the value identity at the time that the listener hears the
track.

## Non-Goals

- No change to the ICY stream, the Socket.IO path or the live value payload.
- No change to the private app in this repository. `citizenradio`
  `docs/plans/upstream-live-metadata-plan.md` lists the app work.

## Gates

Each gate is an item of ADR 0009 §Before Acceptance. A packet that needs a
gate starts after the gate closes.

| Gate | Item | Kind | Closes when |
|---|---|---|---|
| G1 | 1, the `TXXX` check | A test on the test stack and an iPhone | Liquidsoap writes `TXXX:musicindex` from a value with a NUL, and AVPlayer gives the description and the text |
| G2 | 2, the ID3v2.4 check | A test on the test stack and an iPhone | The segments have ID3v2.4 tags, and AVPlayer still gives `TIT2` as `commonKeyTitle` |
| G3 | 3, the tagger home | An operator decision | A repository holds the tagger code and its package (§Where The Tagger Runs) |
| G4 | 4, the pairing | An operator decision, then ADRs in two repositories | The operator accepts §The Value Identity. Relay ADR 0003 and the display schema of ADR 0008 then change. |
| G5 | 5, the instant routes | Implementation | Relay ADR 0004 and ADR 0011 are implemented and deployed |

## Packets After The Gates

The planner writes these packets when their gates close. Their names are
provisional.

| Packet | Needs | Content |
|---|---|---|
| Tagger 001: the state list | G3 | Read `/display/events` with `Last-Event-ID`. Keep the last 4 states. Select by the ICY title with the ADR 0011 rules of `citizenradio`. |
| Tagger 002: the frame | G1, G2, G3 | Build the `musicindex.hls/1` JSON, at most 4,096 bytes, with `value` as `null`. Give it to liquidsoap for the metadata group of the ICY title. |
| Tagger 003: the package | G3, tagger 002 | Ship liquidsoap and the tagger as one package with two settings: the mount URL and the relay event. |
| Pairing 001: the display schema | G4 | The producer adds the drop-file identity and the exact song line to a new display schema. |
| Pairing 002: the publisher | G4, pairing 001, G5 | The publisher attaches the `eventGuid` and the `blockGuid` to the display state. |
| Pairing 003: the tagger | G4, pairing 002, tagger 002 | The tagger matches the song line by equality and copies `value`. |

The relay change of G4 is a packet in `musicindex-live-relay`, under its own
ADR change.

## Assumptions

- Liquidsoap can take metadata from the tagger and insert it into the group
  of the ICY title. G1 confirms the mechanism, for example `insert_metadata`
  or a `metadata.map` function that reads a value from the tagger.
- The relay gives each display state before its ICY title reaches the
  tagger. G5 gives that.

## Risk Areas

- **The text match before G4.** Until the pairing exists, the tagger uses
  the ADR 0011 text rules, and `value` stays `null`. The format rules of
  `now-playing.txt` can make the song line differ from the display key.
- **A late state.** A state that arrives after its ICY title goes in late, by
  the time that it was late.
- **Capacity.** Each tagger is one more SSE stream on the relay. See
  stophammer `docs/operations.md`, §Capacity.

## Test Strategy

The tagger tests use a recorded ICY title sequence and a recorded SSE
sequence. No test needs a live stream. The G1 and G2 checks need the test
stack and an iPhone, and the packets record them as visual checks.

## Rollback Strategy

Stop the tagger. Liquidsoap then writes only `TIT2`, and the private app
uses its `TIT2` fallback.
