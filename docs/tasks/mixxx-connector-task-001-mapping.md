# Mixxx Connector Task 001: The Mapping

Status: Proposed - 2026-09-28.

The acceptance criteria are mechanical. The visual criteria are in a separate
list.

## Goal

Add the Mixxx mapping that speaks the ADR 0006 protocol, and tests that run
its script outside Mixxx.

## Files To Inspect

- `docs/adr/0006-mixxx-midi-connector.md` (§Protocol and §The Loudest Deck)
- `docs/architecture/mixxx-interfaces.md` (the mapping script facts)
- Mixxx 2.5.6 source, if you have it: `src/mixer/playerinfo.cpp:136-193` and
  `src/engine/enginexfader.cpp:16-58`
- `AGENTS.md` (§Build / Lint / Test Commands)

## Files Likely To Change

- `mixxx/MusicIndex-V4V-Connector.midi.xml` (new)
- `mixxx/MusicIndex-V4V-Connector.js` (new)
- `mixxx/tests/connector.test.js` (new)
- `AGENTS.md`

## Do Not Touch

- `src/**` and `mixxx-now-playing/**`
- `packaging/**`, `systemd/**`, `scripts/**`
- `docs/adr/**`

## Constraints

- The mapping sends only 3-byte control change messages on channel 16, with
  status `0xBF`. It sends no SysEx.
- The script defines one global object, `var V4VConnector = {};`. The XML
  names it with `functionprefix="V4VConnector"`.
- The XML maps one input: status `0xBF`, control 1, to the script function
  `V4VConnector.request`. A value of 1 sends the complete state. Any other
  value does nothing.
- The script monitors these controls for `[Channel1]` to `[Channel4]`: `play`,
  `track_loaded`, `duration`, `volume`, `pregain` and `orientation`. It also
  monitors `[Master],crossfader`. Use `engine.makeConnection`.
- Heartbeat: CC 1 with the value 1, each 1000 ms, from `engine.beginTimer`.
- Loudest deck: CC 2 with the value 0 to 4. Compute it each 250 ms from a
  timer and when a monitored control changes. Send it only when the value
  changes, and in the complete state.
- Deck N `play`: CC 10+N, value 0 or 127. Deck N `track_loaded`: CC 20+N,
  value 0 or 127. Send when the control changes.
- Deck N `duration`: whole seconds, rounded down, limited to 16383. Send CC
  30+N with bits 7 to 13, then CC 40+N with bits 0 to 6. Send when `duration`
  changes.
- The complete state: for each deck, `play`, `track_loaded`, then the two
  duration parts. Then CC 2. Send it in `init` and on a request.
- `shutdown` stops the timers and disconnects each connection.
- Put the rule for the loudest deck in a pure function,
  `V4VConnector.loudestDeck(decks, crossfader)`. `decks` is an array of four
  objects with `play`, `pregain`, `volume` and `orientation`. The function
  returns 0 to 4. It follows ADR 0006 §The Loudest Deck exactly:
  - a deck counts only if `play` is not 0, `pregain` is more than 0.25, and
    `volume` is not 0,
  - the left gain is `1 - x` if x is more than 0, else 1,
  - the right gain is `1 + x` if x is less than 0, else 1,
  - no gain is less than 0,
  - orientation 0 uses the left gain, 1 uses the gain 1, and 2 uses the right
    gain,
  - only a strictly higher value replaces the present best, so equal values
    give the lower deck number,
  - a best value of 0 gives the result 0.
- Put the duration split in a pure function,
  `V4VConnector.durationParts(seconds)`, that returns `[high, low]`.
- The script must run in the Mixxx 2.5 script engine and in the Node.js `vm`
  module. Do not use `require`, `module` or `import` in the script.
- The tests use only the Node.js standard library: `node:test`,
  `node:assert` and `node:vm`. The test loads the script text into a `vm`
  context with a stub `engine`, a stub `midi` and a stub `print`. The stub
  `midi.sendShortMsg` records each message.
- The XML `info` block names the mapping "MusicIndex V4V Connector" and cites
  ADR 0006 in its description.

## Implementation Steps

1. Write `mixxx/MusicIndex-V4V-Connector.js` with `init`, `shutdown`,
   `request`, `loudestDeck`, `durationParts` and the send functions.
2. Write `mixxx/MusicIndex-V4V-Connector.midi.xml` with the script file and
   the one input.
3. Write `mixxx/tests/connector.test.js` with the tests in §Acceptance
   Criteria.
4. Add `node --test mixxx/tests/` to the commands in `AGENTS.md`, with one
   line that says it tests the Mixxx mapping script.

## Acceptance Criteria

Mechanical. Each item is a test in `mixxx/tests/connector.test.js`:

- `loudestDeck` with each crossfader value -1, 0 and 1, and each orientation
  0, 1 and 2, gives the deck that the ADR rule gives.
- `pregain` of exactly 0.25 does not count. `pregain` of 0.26 counts.
- `volume` of 0 does not count.
- Two decks with equal values give the lower deck number.
- No deck that plays gives 0.
- `durationParts` gives `[0, 0]` for 0, `[0, 127]` for 127, `[1, 0]` for 128,
  `[127, 127]` for 16383, `[127, 127]` for 20000, and `[1, 72]` for 200.9.
- `init` sends the complete state in the order in §Constraints, and starts
  two timers.
- `request` with the value 1 sends the complete state. `request` with the
  value 0 sends nothing.
- A timer pass with no change sends no CC 2 message.
- Each message has the status `0xBF`.

Also:

- `node --test mixxx/tests/` passes.
- The full cargo gate still passes.

Visual. A person checks these in Mixxx. The review records the result:

- Mixxx lists "MusicIndex V4V Connector" for `VirMIDI 31-0` and loads it with
  no script error in the log.
- `amidi -p hw:V4V,0 -d` shows `BF 01 01` each second.

## Test Commands

```bash
node --test mixxx/tests/
cargo fmt --all -- --check
cargo build --workspace
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
```

## Escalation Triggers

Stop and report if one of these occurs:

- The Mixxx script engine cannot run a construct that the Node.js test needs.
- The rule in `playerinfo.cpp` differs from ADR 0006 §The Loudest Deck.
- The mapping needs an output or input that §Constraints does not name.

## Prompt for lower-context coding model

You are implementing one bounded task from a larger plan.

Implement only this task. Do not redesign the architecture.

Read:
- docs/adr/0006-mixxx-midi-connector.md
- docs/tasks/mixxx-connector-task-001-mapping.md
- docs/architecture/mixxx-interfaces.md
- AGENTS.md

Goal:
- Add a Mixxx mapping in mixxx/ that sends the ADR 0006 protocol version 1, and Node.js tests that run its script outside Mixxx.

Constraints:
- Only 3-byte CC messages on status 0xBF. No SysEx.
- One global `var V4VConnector = {};`. XML functionprefix V4VConnector. One input: 0xBF control 1 to V4VConnector.request. Value 1 sends the complete state.
- Monitor play, track_loaded, duration, volume, pregain, orientation for [Channel1] to [Channel4], and [Master],crossfader, with engine.makeConnection.
- Heartbeat CC1 = 1 each 1000 ms. Loudest deck CC2 each 250 ms and on control change, sent only on change and in the complete state.
- CC 10+N play, CC 20+N track_loaded (0 or 127). CC 30+N then CC 40+N: duration in whole seconds, rounded down, limited to 16383, high 7 bits then low 7 bits.
- Complete state: per deck play, track_loaded, duration high, duration low; then CC2. Sent in init and on request.
- Pure functions loudestDeck(decks, crossfader) and durationParts(seconds). Follow ADR 0006 §The Loudest Deck exactly. Strictly higher value wins, so a tie gives the lower deck.
- No require, module or import in the script. Tests use only node:test, node:assert and node:vm with stub engine, midi and print.

Do not touch:
- src/**, mixxx-now-playing/**, packaging/**, systemd/**, scripts/**, docs/adr/**

Acceptance criteria:
- The tests listed in the task §Acceptance Criteria exist and pass with `node --test mixxx/tests/`.
- AGENTS.md lists `node --test mixxx/tests/`.
- The cargo gate passes.

Test commands:
- node --test mixxx/tests/
- cargo fmt --all -- --check
- cargo build --workspace
- cargo test --workspace
- cargo clippy --workspace --all-targets -- -D warnings

At the end, report:
1. files changed
2. tests run
3. behavior changed
4. deviations from task
5. unresolved concerns
