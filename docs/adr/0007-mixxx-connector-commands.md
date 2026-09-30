# ADR 0007: Commands To Mixxx Through The Connector

Status: Proposed
Date: 2026-09-30

This ADR extends the protocol of ADR 0006. It becomes Accepted when the
operator accepts it and the check in §Verification Before Acceptance passes.

## Context

ADR 0006 gives a consumer the deck state from Mixxx through the V4V card. The
only message from a consumer to Mixxx is the state request. ADR 0006 lists
other commands as a non-goal, and it says that a later ADR adds them.

An operator starts an AutoDJ fade to the next track from a script, as a skip
with a fade. Today that script sends CC 43 to a second virtual MIDI port. A
second Mixxx mapping maps CC 43 to `[AutoDJ],fade_now`. That setup needs a
second port and a second mapping. The script also needs the name of a card,
and that name can change.

The V4V card and the connector mapping already exist. The kernel sends each byte that a consumer writes to the raw device to
Mixxx.

These facts come from the Mixxx 2.5.6 source:

- `[AutoDJ],fade_now` starts the transition to the next track at once
  (`src/library/autodj/autodjprocessor.cpp:593-597`).
- Mixxx does nothing for `fade_now` when AutoDJ is disabled, when a
  transition is in progress, or when no deck plays
  (`autodjprocessor.cpp:203-230`). Mixxx gives no result for the request.
- A mapping script can read `[AutoDJ],enabled` before it sets a control.
- `[AutoDJ],skip_next` is a different control. It removes the next track from
  the AutoDJ queue, and the present track continues
  (`autodjprocessor.cpp:605-609`). This ADR does not use it.

## Decision

### Protocol Version 3

This ADR adds messages, so the protocol becomes version 3. The heartbeat
value is 3. A consumer that knows only version 2 uses the history-only mode,
as ADR 0006 says for an unknown version. All other messages of ADR 0006 do
not change.

### Messages

From a consumer to Mixxx:

| CC | Value | Meaning |
|---|---|---|
| 4 | Command code | Do this command |

From Mixxx to a consumer:

| CC | Value | Meaning |
|---|---|---|
| 4 | Command code | The mapping did the command |
| 5 | Command code | The mapping refused the command |

The command codes:

| Code | Command | The mapping does it when | Mixxx control |
|---|---|---|---|
| 1 | AutoDJ fade now | `[AutoDJ],enabled` is 1 | `[AutoDJ],fade_now` set to 1, then to 0 |

- The mapping refuses an unknown code, and a known code when its condition is
  false.
- "Did" means that the mapping set the control. It does not mean that the
  transition started. Mixxx can still ignore `fade_now`, for example during a
  transition. The consumer sees the result in the deck state.
- A new command needs an amendment to this table and a new protocol version.

### The Command Line

`mixxx-now-playing` gets a subcommand:

```text
mixxx-now-playing command fade-now [--connector-card ID] [--timeout SECS]
```

- The name `fade-now` agrees with the Mixxx control. The command skips the
  present track with a fade. It is not `skip_next`.
- It opens the raw device of the card, as ADR 0006 §Transport says. The
  default card ID is `V4V`. The default timeout is 2 seconds.
- It waits for one heartbeat of version 3. Then it sends the command, and it
  waits for CC 4 or CC 5 with the same code.
- It does not use the producer lock, and it does not change a drop file. It
  can run while the producer runs, because each open of the raw device gets
  its own copy of the messages from Mixxx.
- It writes one line to stderr for each result that is not success.

Exit codes:

| Code | Meaning |
|---|---|
| 0 | The mapping did the command |
| 2 | The command line is not correct |
| 3 | The mapping refused the command |
| 4 | No heartbeat of version 3 before the timeout. The command was not sent. |
| 5 | The command was sent, but no answer arrived before the timeout. The result is not known. |

A caller must not repeat a command after exit code 5 without a check of the
deck state. Mixxx can have done the command, and the answer can be lost. ADR
0006 §Context records that Mixxx output can stop while its input continues.

### Payment

A command changes no payment rule. A transition that a command starts gives
the same deck changes and history rows as any other transition. ADR 0006
applies to them.

## Invariants

- A command uses only 3-byte control change messages on channel 16.
- A command never changes a drop file or the producer lock.
- The command line sends a command only after a heartbeat of the version that
  knows the command.
- Exit code 0 means only that the mapping set the control.

## Verification Before Acceptance

Manual, with Mixxx, because it needs a running Mixxx:

1. In a test mapping on the V4V card, a script sets `[AutoDJ],fade_now` to 1
   and then to 0 while AutoDJ plays. Record that the transition starts.
   Passed on 2026-09-30. The transition started at once, and the mapping
   answered CC 4.
2. Do step 1 while a transition is in progress. Record that Mixxx ignores it.
   Passed on 2026-09-30. A second command 1 second after the first gave one
   normal transition and no second skip. The mapping answered CC 4 both
   times, as §Messages says.

Also tested on 2026-09-30: with AutoDJ disabled, the mapping answered CC 5,
and nothing changed in Mixxx.

## Verification After Implementation

Mechanical:

- A mapping test for each row of the command table: the control is set when
  the condition is true, and CC 4 answers. The control is not set when the
  condition is false, and CC 5 answers. An unknown code gives CC 5.
- A mapping test that the heartbeat value is 3.
- A producer test that version 3 gives the connector mode and version 2 gives
  `UnknownVersion`.
- Command-line tests with a byte stream in place of the device, for each exit
  code.

Visual:

- `mixxx-now-playing command fade-now` starts an AutoDJ transition, and it
  exits with 0.
- With AutoDJ disabled, it exits with 3, and nothing changes in Mixxx.

## Alternatives Considered

### Keep A Second Port And Mapping For Commands

Rejected. It needs a second virtual card, a second mapping and a card number
in a script. The connector card already carries messages in both directions.

### A Command With No Answer

Rejected. A script cannot know if the mapping received the command. The
mapping can be disabled, or it can be an older version.

### Report The Result Of The Transition

Deferred. The mapping cannot know if AutoDJ starts a transition after the
control is set. The deck state shows it later. A caller that needs the result
reads the deck state.

### A Separate Binary For Commands

Rejected. The producer already has the device code, the card lookup and the
protocol constants. A subcommand keeps one implementation of the protocol.

## Consequences

Positive:

- One card and one mapping give the deck state and take commands.
- A script gets a result: done, refused, not sent, or not known.
- `v4vmm` can use the same command line.

Negative and risks:

- A local program that can open the sound devices can start a transition.
  The old second port had the same risk.
- The protocol version changes again. The mapping and the producer must
  change together, as for version 2.
- If Mixxx output stops (ADR 0006 §Context), the command line gives exit code
  5 although the command can have run.

## Changes At Acceptance

- ADR 0006 §Protocol: the heartbeat value becomes 3, and the protocol points
  to this ADR for the command messages. Add a dated amendment.
- ADR 0006 §Non-Goals: the non-goal "Commands from `v4vmm` to Mixxx, other
  than the state request" no longer applies to the commands of this ADR. Add a
  dated amendment.
- `AGENTS.md` §Current State: remove the item for commands to Mixxx.

## Non-Goals

- The talk break block.
- Commands for transport, for example play or stop of a deck.
- A command to enable or disable AutoDJ.
- A report of the transition result.

## References

- `docs/adr/0006-mixxx-midi-connector.md`
- `docs/architecture/mixxx-interfaces.md`
- Mixxx 2.5.6 source: `src/library/autodj/autodjprocessor.cpp`
