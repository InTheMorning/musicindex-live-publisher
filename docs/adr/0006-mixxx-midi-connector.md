# ADR 0006: Mixxx MIDI Connector

Status: Proposed
Date: 2026-09-27

This ADR becomes Accepted when the operator accepts it and the checks in
§Verification Before Acceptance pass.

## Context

`mixxx-now-playing` reads the Mixxx history database. The history cannot show
a stop or a pause, and ADR 0001 lists pause detection as a non-goal. So when
the DJ stops a V4V track to talk, its artist stays payable until the expiry
timer ends. ADR 0005 limits that time with `--expiry-max`, and it says that a
MIDI link to Mixxx supersedes that limit.

Tests with Mixxx 2.5.6 on Linux on 2026-09-27 give these facts. The evidence
is in `docs/architecture/mixxx-interfaces.md`.

- A Mixxx mapping script can monitor the deck `play`, `track_loaded` and
  `duration` controls, and can send MIDI.
- A script cannot identify the track on a deck. The track identity must come
  from a different source.
- Mixxx finds MIDI devices only at startup. A sequencer port that a program
  makes loses its input to Mixxx when the program restarts.
- A kernel `snd-virmidi` port always exists. Mixxx opens its sequencer side.
  A program can open its raw MIDI device as a file at any time, and can
  restart while Mixxx runs.
- PortMidi keeps its error state in one global flag. A device that disappears
  while Mixxx sends to it can stop MIDI output for all controllers.
- A SysEx send that fails halfway leaves PortMidi in the middle of a message.
  The next messages then fail with `Invalid MIDI message Data`.
- The Mixxx deck duration comes from the decoder, and it is correct. The
  history row for a new track appears about 4.5 seconds after AutoDJ starts
  it.
- A hardware controller and the connector operate at the same time.

## Decision

### Ownership

This repository owns the connector protocol, the Mixxx mapping that speaks it,
and the setup rules. `mixxx-now-playing` is the first consumer. `v4vmm` can be
a second consumer. It cites this ADR and has no build dependency on this
repository.

### Transport: A Kernel Virtual MIDI Port

One `snd-virmidi` card is for the connector only:

| Property | Value |
|---|---|
| Card ID | `V4V` |
| Card number | 31 |
| Port that Mixxx shows | `VirMIDI 31-0` |
| Raw device for a consumer | `hw:V4V,0`, which is `/dev/snd/midiC31D0` |

- Mixxx opens the sequencer port `VirMIDI 31-0` as a controller with the
  connector mapping.
- A consumer opens the raw MIDI device as a file. It finds the device from the
  card ID, not from the number: `/proc/asound/V4V` gives the card number N,
  and the device is `/dev/snd/midiCND0`. A configuration value can name a
  different card ID.
- A consumer never makes an ALSA sequencer port, a JACK port or a PortMidi
  device. So no connector port can disappear from Mixxx.
- No other source is patched into the connector port, in ALSA or in JACK.

The kernel gives each open file its own copy of the messages from Mixxx. So
`mixxx-now-playing` and `v4vmm` can both open the device.

#### Why The Card Number Is Pinned

The port name that Mixxx shows always contains the card number. The kernel
sets it with `sprintf(pinfo->name, "VirMIDI %d-%d", ...)`
(`sound/core/seq/seq_virmidi.c:388`), and no module option changes it. The
module has only the options `enable`, `index`, `id` and `midi_devs`
(`sound/drivers/virmidi.c:54-61`). Mixxx stores a controller by that name. If
the number changes, Mixxx no longer opens the connector port.

Number 31 is the highest card number in the Arch kernel, which has
`CONFIG_SND_MAX_CARDS=32`. The kernel gives a USB sound card the lowest free
number, so a USB card does not take number 31.

#### Card Setup

The package installs two files. They make the V4V card at boot:

```text
# /usr/lib/modules-load.d/musicindex-v4v-midi.conf
snd-virmidi

# /usr/lib/modprobe.d/musicindex-v4v-midi.conf
options snd-virmidi enable=1 index=31 id=V4V midi_devs=1
```

The change applies at the next boot. A package install script does not load
or reload the module, because a reload fails while a program has a virtual
MIDI port open.

The module has one set of options for all its cards. If the computer already
uses `snd-virmidi` for other cards, two `options snd-virmidi` lines set the
same parameters and conflict. The operator then does these steps:

1. Write `/etc/modprobe.d/musicindex-v4v-midi.conf`. A file in `/etc` with
   the same name replaces the package file. It holds one line with all
   cards. For two other cards, the line is:

   ```text
   options snd-virmidi enable=1,1,1 index=-1,-1,31 id=Synth1,Synth2,V4V midi_devs=1,1,1
   ```

2. Remove the other `options snd-virmidi` line from the computer.
3. Reboot.

Only the V4V card gets a fixed number. The value `-1` gives a card the lowest
free number, which is the default. A fixed low number is not safe: if a USB
card takes that number first at boot, the kernel cannot make the virtual
card.

An ID that the option gives keeps only letters and digits. Tested on
2026-09-27: an ID with an underscore lost the underscore. The kernel gives an
underscore only in an ID that it makes itself. So when the merged line names
the other cards, their IDs can change, and a script that opens their raw
devices by the old ID must change. The port names that Mixxx shows do not
change, because they use the card number.

The setup script checks that `/proc/asound/V4V` exists and is card 31. It
gives the port name that Mixxx shows, and it stops with a clear message if the
card is missing.

### Protocol

All messages are short control change messages on MIDI channel 16 (status
`0xBF`). The protocol uses no SysEx.

From Mixxx to a consumer. N is the deck number, 1 to 4:

| CC | Value | Meaning |
|---|---|---|
| 1 | Protocol version, now 1 | Heartbeat, sent each second |
| 10 + N | 0 or 127 | Deck N `play` |
| 20 + N | 0 or 127 | Deck N `track_loaded` |
| 30 + N | Bits 7 to 13 | Deck N `duration` in whole seconds, high part |
| 40 + N | Bits 0 to 6 | Deck N `duration` in whole seconds, low part. The value applies when this part arrives. |

The mapping sends the `play` and `track_loaded` messages when the control
changes, and the duration parts when `duration` changes. It sends the complete
state of all four decks when it starts and when a consumer asks.

From a consumer to Mixxx:

| CC | Value | Meaning |
|---|---|---|
| 1 | 1 | Send the complete state of all decks |

A consumer ignores a message that repeats the last value. Mixxx sends
`play = 0` more than one time at a stop.

A later ADR adds commands, for example a talk break from `v4vmm`. A change to
a message meaning needs a new protocol version.

### How `mixxx-now-playing` Uses The Deck State

The history row still gives the track identity. The deck state gives the
timing.

- **The link:** when a history row gives a new track, the producer links it to
  the deck that plays and has the same duration. It compares the Mixxx library
  duration of the track, rounded to whole seconds, with the deck duration. If
  no deck or more than one deck matches, the producer writes no drop file for
  that track.
- **The duration:** in connector mode, `duration_secs` in the drop file comes
  from the linked deck. The deck value comes from the decoder. The lofty value
  comes from the stream headers and can be very wrong.
- **A stop or a pause:** when the linked deck `play` becomes 0, the producer
  removes the drop file. The publisher then publishes the dead block (ADR
  0005).
- **A resume:** when the linked deck `play` becomes 127 again, the producer
  writes the drop file again.
- **A new load:** when the linked deck `track_loaded` or `duration` changes,
  the link ends, and the producer removes the drop file.
- **The switch time:** the payee changes when the history row appears. That
  is the Mixxx "loudest deck" rule. The stream metadata that Mixxx sends to
  Icecast uses the same rule (`src/engine/sidechain/shoutconnection.cpp:772`,
  Mixxx 2.5.6).
- **A track in the last six tracks:** Mixxx writes no history row for it. The
  linked deck of the previous track stops at the end of the crossfade, so the
  producer removes the file. The replayed track then pays nobody. That result
  is safe.

### When The Connector Is Not Available

The producer uses the history-only mode of ADR 0005, with `--expiry-max`, in
these conditions:

- the raw device cannot be opened,
- no heartbeat arrived in the last 3 seconds,
- the heartbeat gives a protocol version that the producer does not know.

The producer logs the change of mode with `tracing::warn!`. When heartbeats
arrive again, it asks for the complete state and returns to the connector
mode.

### Operator Rules

- Do not leave a Mixxx controller enabled whose device can disappear while
  Mixxx sends to it. Disable a controller that is not connected.
- Enable only one Mixxx controller with the connector mapping.

## Invariants

- A consumer never makes a sequencer port, a JACK port or a PortMidi device.
- The protocol uses only 3-byte control change messages on channel 16.
- The producer never keeps a drop file while its linked deck does not play,
  when the connector is available.
- The producer never links a track to a deck without a duration match.
- In connector mode, `duration_secs` comes from the linked deck, never from
  the stream headers.
- Without a heartbeat, the producer uses the ADR 0005 expiry. It never assumes
  that a deck plays.

## Verification Before Acceptance

Manual, on a computer with Mixxx, because each check needs a running Mixxx:

1. **The link key.** Load three tracks. For each, compare the Mixxx library
   `duration` column with the deck `duration` control. They must agree within
   1 second. If they do not, the link rule in this ADR changes before
   acceptance.
2. **A controller with feedback.** Enable a controller whose mapping sends
   LED output. Unplug it while the connector runs. Record if connector
   messages still arrive. The result goes into the operator rules.
3. **`engine.beginTimer`.** Confirm that a 1-second timer in the mapping sends
   the heartbeat for 10 minutes with no gap longer than 2 seconds.

## Verification After Implementation

Mechanical:

- A unit test for the MIDI byte parser, with running status and a real-time
  byte between messages.
- A unit test for each producer rule in §How `mixxx-now-playing` Uses The Deck
  State, with a sequence of deck events and history rows.
- A unit test for each condition in §When The Connector Is Not Available.
- A unit test that the producer never writes a drop file for a track with no
  duration match.
- A unit test that the drop file in connector mode has the deck duration, for
  a track whose header duration is different.

## Alternatives Considered

### A Sequencer Port That The Connector Makes

Rejected. Tested: Mixxx finds it only at startup, and a connector restart
loses the input to Mixxx until Mixxx restarts. A missing port can also stop
Mixxx MIDI output for all controllers.

### A JACK MIDI Port

Rejected. Mixxx uses ALSA through PortMidi, not JACK. A JACK port depends on
the a2j bridge and on a connection that a script must make again after each
start.

### SysEx Messages

Rejected. Tested: one failed SysEx send makes the next sends fail. Short
messages cannot fail halfway, and 14 bits of whole seconds hold a duration up
to 4.5 hours.

### Switch The Payee When The New Deck Starts

Deferred. The producer does not know the track identity until the history row
appears. A switch at the deck start would need the identity from a different
source.

### A Different Track Identity Source

Deferred. The script API cannot give it. The Icecast metadata comes from the
same "loudest deck" rule as the history, so it is not earlier. The history
database is the only source found that gives a file path.

## Consequences

Positive:

- A stop or a pause ends the payment at once, not after the expiry.
- Mixxx and the connector can start and restart in any order.
- A hardware controller keeps working next to the connector.
- `v4vmm` gets the deck state from the same device, with no second setup.

Negative and risks:

- The package makes the V4V card, but the operator must enable the mapping
  on `VirMIDI 31-0` in Mixxx. On a computer that already uses `snd-virmidi`,
  the operator must also merge the options. A missing card makes the producer
  fall back to the expiry.
- The duration link can fail for two decks with the same track length. The
  producer then writes no drop file, and the track pays nobody.
- The payee still changes about 4.5 seconds after the new deck starts.
- A mapping bug can stop the heartbeat. The producer then falls back to the
  expiry, which is safe.

## Changes At Acceptance

- ADR 0001: the non-goal "Detecting pause or exact audio output state" no
  longer applies. Add a dated amendment that cites this ADR.
- ADR 0005: §Payment Timing Until MIDI stays in force as the history-only
  mode. Add a dated amendment that cites this ADR.
- `docs/architecture/mixxx-interfaces.md`: add the results of §Verification
  Before Acceptance.
- `docs/runbooks/musicindex-live-publisher-configuration.md`: add the operator
  setup for the V4V card and the Mixxx controller.

## Non-Goals

- Commands from `v4vmm` to Mixxx, other than the state request.
- The talk break block.
- Support for more than four decks.
- A connector for a player other than Mixxx.

## References

- `docs/architecture/mixxx-interfaces.md`
- `docs/adr/0001-rust-now-playing-lifecycle.md`
- `docs/adr/0002-nowplaying-drop-file-contract.md`
- `docs/adr/0005-producer-liveness-and-dead-block.md`
- Linux source: `sound/core/seq/seq_virmidi.c`
- PortMidi source, tag `v2.0.7`: `pm_linux/pmlinuxalsa.c`,
  `pm_common/portmidi.c`
