# Task 005 — Relay client and publish loop

Part of [MusicIndex live publisher plan](../plans/musicindex-live-publisher-plan.md), Phase 3.

## Goal

Publish assembled payloads to the relay, handle its documented status codes
distinctly, and provide the one-time `provision` subcommand for creating a live
item.

## Files To Inspect

- `docs/plans/musicindex-live-publisher-plan.md` — Output: publishing
- `~/build/splitkit/README.md` — status codes, body-form discrimination, token issuance
- `~/build/v4vmm/src/api.rs:581` — `publish_live_metadata_with_token`, the existing client for reference
- Tasks 002, 003 and 004 output

## Files Likely To Change

- `src/relay.rs` (new)
- `src/main.rs`
- `tests/relay.rs` (new)

## Do Not Touch

- Task 002's payload assembly — this task transports it, it does not reshape it
- `~/build/splitkit/src/` — the relay is not modified by this project

## Constraints

- `POST {endpoint}/v1/liveitems/{event_id}/metadata` with
  `Authorization: Bearer <token>` and `Content-Type: application/json`.
- The body is the **direct** payload from task 002. Assert before sending that its
  top-level key set is not exactly `{event_id, metadata}` — that exact set makes
  the relay treat it as the wrapped form.
- Handle status codes distinctly, per the relay README:

  | Code | Meaning | Behaviour |
  |---|---|---|
  | `401` | token missing or malformed | fatal, stop retrying, log loudly |
  | `403` | token wrong | fatal, stop retrying, log loudly |
  | `404` | event does not exist | fatal, the live item was lost |
  | `413` | body over 64 KiB | drop payload, log, do **not** retry |
  | `429` | rate limited | back off and retry |
  | `5xx` / network | transient | back off and retry |

- Retries use bounded exponential backoff with a cap. A publish failure must never
  wedge the watcher — a new track event supersedes a pending retry.
- Set a request timeout. Blocking `reqwest` is fine; this is one request per track
  change, not a stream.
- Never log the token.

## Implementation Steps

1. Implement `publish(target, payload) -> Result<PublishOutcome>` where the
   outcome distinguishes accepted, retryable, and fatal.
2. Wire it into task 003's watcher, replacing `--dry-run` printing when the flag
   is absent. `--dry-run` must keep working unchanged.
3. Implement backoff: capped exponential, jittered, abandoned when a newer payload
   arrives for the same target.
4. Implement `provision --endpoint <url>`: `POST /v1/liveitems`, then print the
   ready-to-paste config stanza including `event_id`, and write the token to a
   `0600` file at a path given by `--token-file`. Print a clear warning that the
   token is shown exactly once and cannot be recovered.
5. On a fatal code, keep the process alive but stop publishing to that target, and
   log at error level. Other targets continue.

## Acceptance Criteria

- A successful publish returns accepted and logs the relay's `seq`.
- Each of 401, 403, 404, 413, 429 produces its documented behaviour, verified
  against a stub server.
- A 429 followed by a 200 succeeds after backoff.
- A newer payload arriving mid-retry abandons the older retry.
- The pre-send assertion rejects a payload with exactly `{event_id, metadata}`.
- `provision` creates a live item against a locally built relay and writes a
  `0600` token file.
- Integration test: build `~/build/splitkit`, run it on a loopback port, publish,
  and read the payload back from
  `GET /v1/liveitems/{event_id}/remoteValue`.
- No test contacts `api.musicindex.org`.
- No log line at any verbosity contains a token.

## Test Commands

```bash
cd musicindex-live-publisher
cargo clippy -- -D warnings
cargo test relay
cargo test --test relay -- --ignored   # relay integration, requires local build
```

## Expected Final Report

- Files created or changed
- The status-code matrix and observed behaviour for each
- Backoff parameters chosen
- Round-trip evidence: payload published, and the same payload read back from the
  relay's `remoteValue` endpoint
- Confirmation no real network calls were made in tests

## Escalation Triggers

Stop and ask before proceeding if:

- The relay rejects a payload that the task 002 golden tests accept — that is a
  contract disagreement, not something to patch around in transport.
- `~/build/splitkit` does not build, blocking the integration test.
- A fatal code appears recoverable in practice and the classification above looks
  wrong.
