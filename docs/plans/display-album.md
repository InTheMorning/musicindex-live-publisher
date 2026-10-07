# The Album In The Display State: Phase Plan

Status: Ready 2026-10-06. This plan does not make rules. ADR 0013 owns them.

## Goal

The producer writes `display.json` version 3 with the album of the history
row. The publisher sends it as `album` in the display state.

## Non-Goals

- No change to the live value payload or the drop file.
- No album in the HLS frame of ADR 0009.

## Sequence

1. [Task 001](../tasks/display-album-task-001-display-json-v3.md): the
   producer, the publisher and their tests, in one release.

## Schema And API

`display.json` goes to `musicindex.display/3`. The display body adds `album`
(relay ADR 0006). The configuration runbook gives version 3.

## Risk Areas

- The deployment order. Relay ADR 0006 must run first, or the relay refuses
  each display state with `album`.
- The source of the album. It must come from the history row of the display
  state, not from the V4V state. The producer clears the V4V state for a
  track that is not V4V.

## Test Strategy

A lifecycle test runs the producer for a V4V track and for a track that is
not V4V, and reads `display.json`. The publisher tests drive the display
worker to the stub relay.

## Rollback Strategy

Revert the commit and build the package again. The producer and the
publisher go back to version 2 together.

The review uses the
[display album review checklist](../reviews/display-album-review-checklist.md).
