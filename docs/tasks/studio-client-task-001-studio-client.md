# Studio Client Task 001: Start And End A Show Through Studio

Status: Not started - 2026-11-01. It waits for Studio task 073, which makes the crate `studio-app-client`. Packet a writes ADR 0020 first. Do not write code before ADR 0020 is `Accepted`.

Each criterion states if it is mechanical or visual.

## Goal

The publisher starts and ends a live show in its Studio feed, and sets the `podcast:liveValue` of that show. It uses the Studio client crate and a show grant that the user gives in Studio. The relay payloads do not depend on Studio.

## Authority

- Studio ADR 0110 (`~/build/musicindex-studio/docs/adr/0110-a-connected-app-signs-each-request-with-its-own-key.md`) owns the interface between Studio and an app. This repository cites it and does not restate it.
- The Studio handoff note (`~/build/musicindex-studio/docs/notes/2026-11-01-live-publisher-studio-client.md`) describes the interface, the rules for an app, the errors and the open points. It is advisory.
- `v4vmm` ADR 0059 task 019 gives the form of the live value address: `https://<relay host>/event?event_id=<EVENT_ID>` with `protocol="socket.io"`.
- AGENTS.md §5 Secret Handling and §7 Failure Behavior apply to the key file and to the Studio failures.
- `v4vmm` task `adr-0059-task-020-studio-connection.md` uses the result of this task.

## Recorded Facts - 2026-11-01

- The publisher has no Studio client, no feed writer and no host connector. `src/relay.rs` sends the payloads to the relay `endpoint` of a target (`src/config.rs`).
- Studio is the only writer of the feed. A granted write goes through the Studio API. The publisher never gets a host secret.
- The crate `studio-app-client` does the pairing, the signed requests, the grant vault key and the four operations: go live, end, set the live value and remove the live value. It corrects a clock difference of 5 minutes or less, and gives a warning above 30 seconds.
- The crate reads a pairing text only from the clipboard or from a prompt with no echo. It refuses a command argument and an environment variable.
- A Studio grant names one copy of the publisher, one feed and the `guid` of one live item. It fixes the live value origin that the publisher gave at the pairing.

## Packets

### a: ADR 0020, The Publisher Writes The Live Item Through Studio

Write ADR 0020 with the status `Proposed`, and get the operator's decision on each point. The recommendation of this task is in parentheses.

1. The dependency: the publisher depends on `studio-app-client` through a Git tag of the Studio repository. AGENTS.md §Neighbors forbids a build dependency on `v4vmm` and the relay only. ADR 0020 records the new dependency and why. (Accept: the crate is the owner of the Studio contract.)
2. The key storage: which `AppSigner` the publisher uses. (`FileSigner`, with the key in the state folder of the instance, mode 0600, in a folder with mode 0700, as the token files.)
3. The configuration: where the publisher keeps the Studio origin and the path of the key for each target. (A `studio` table in the target. The publisher owns its configuration.)
4. The pairing command: its name and its options. (`studio connect --target NAME`. It reads the clipboard by default, and `--prompt` reads a prompt with no echo. `--json` prints one result object for `v4vmm`. The live value origin comes from the relay `endpoint` of the target.)
5. The show of a grant: how the publisher finds the grant of its event. (The target gets `studio_live_item_guid`. `v4vmm` sets it through a publisher command when it attaches an event. The publisher acts only on the grant with that `guid`.)
6. The time of each write. (Go live and set the live value when the target starts its event. End when `v4vmm` ends the event, or when the service stops at the end of the event.)
7. A refused write. (Retry `Network` and `TooManyAttempts` with backoff, for a maximum of 10 minutes. Stop at once for `NotGranted`, `VaultLocked`, `LiveValueInvalid`, `ClockWrong`, `KeyRequired` and `SignatureInvalid`. A Studio failure never stops or delays a relay payload.)
8. The status for `v4vmm`. (The publisher writes a status file in the state folder of the instance, with a temporary file and a rename. It holds the Studio state, the last error code, the time and the clock warning. It holds no secret, no pairing text and no event.)
9. Studio does not answer at the start or the end of a show. (The relay payloads continue. The publisher retries as item 7 gives, and the status file shows the failure.)
10. A restart. (The publisher reads the shows again at each start, and at each attach of an event.)

Add ADR 0020 to `docs/README.md` and to the Current State of AGENTS.md in the same change.

### b: The Pairing Command And The Configuration

1. Add `studio-app-client` to `Cargo.toml` at the Git tag that ADR 0020 names.
2. Add the configuration of ADR 0020 items 2 and 3. `config show` prints the Studio origin and the short key, never the key file content.
3. Add the pairing command of ADR 0020 item 4. It uses the crate module `input`. It prints the result and the short key only.
4. On `CodeUsed`, the command prints that a different program possibly used the pairing text, and that the user must remove that app in Studio.
5. Add the command that sets `studio_live_item_guid` of a target, for `v4vmm` (ADR 0020 item 5).

### c: The Show Writes And The Status File

1. When the target starts its event, the publisher reads its grants and finds the grant of ADR 0020 item 5. Then it goes live and sets the live value. The live value `uri` is the form of `v4vmm` ADR 0059 task 019.
2. When the event ends, the publisher ends the show.
3. The publisher handles each error as ADR 0020 item 7 gives.
4. The publisher writes the status file of ADR 0020 item 8 at each change of the Studio state.
5. The Studio work runs apart from the relay worker. A slow or failed Studio request does not delay a payload.

### d: The Documentation

1. `README.md` gets a section "Studio": what the user does in Studio, the pairing command, and what the publisher writes.
2. `docs/runbooks/musicindex-live-publisher-configuration.md` gets the `studio` table, the pairing, the errors and what to do for each one, and the clock rule.
3. The runbook tells the user to keep the automatic time of the computer on. It also tells the user to write or discard each Studio draft of the feed before a show.

## Files To Inspect

- `src/config.rs`, `src/main.rs`, `src/relay.rs`, `src/watcher.rs`, `src/liveness.rs`
- `tests/config.rs`, `tests/relay.rs`
- `docs/architecture/broadcast-chain-boundaries.md`
- The Studio handoff note and Studio ADR 0110 (read only)

## Do Not Touch

- The drop-file contract and the payload assembly. The payment rules of AGENTS.md §4 do not change.
- `mixxx-now-playing/**`.
- The relay protocol.

## Constraints

- No key, pairing text, signed event or grant vault key goes into a log line, an error message or a command argument. The same applies to a `Debug` output and the status file (AGENTS.md §5, Studio ADR 0110).
- The key file uses mode 0600, and its folder uses mode 0700.
- No test sends a request to a public Studio. Tests use a local stub of the Studio API.
- A Studio failure never changes, stops or delays a relay payload or the dead block.

## Acceptance Criteria

Mechanical:

- A config test: the `studio` table loads, and `config show` prints the short key and no key content.
- A command test with a stub Studio: the pairing command reads a pairing text from a stub clipboard, and prints the short key. It refuses a pairing text in an argument. `CodeUsed` prints the warning.
- A test with a stub Studio: the start of an event sends go live and the live value with the `uri` form of `v4vmm` ADR 0059 task 019. The end of the event sends end.
- A test: `NotGranted` stops the Studio writes, and the relay payloads continue with no delay.
- A test: `Network` retries with backoff and stops after 10 minutes.
- A test: the status file holds the state and the last error code, and no secret.
- A test: a slow stub Studio does not delay a payload.
- The gate of AGENTS.md passes in the workspace.

Visual: none. This task has no screen. The `v4vmm` task owns the screens.

Human: the operator pairs the publisher with a real Studio, gives a grant, runs a show, and sees the feed go live and end. This check needs a running Studio with task 073.
