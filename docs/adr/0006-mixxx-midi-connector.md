# ADR 0006: Mixxx MIDI Connector

Status: Accepted
Date: 2026-09-27

Accepted 2026-09-28 by the operator. The four checks in §Verification Before
Acceptance are done.

Amended 2026-10-06: a resume of the linked deck takes effect only after the
deck plays for 2 seconds. §A Resume Settles gives the rule. A load into a
playing deck sent the old track live again for some milliseconds. The rule
adds a time to the resume and reverses no decision.

Amended 2026-09-28: the producer links a history row to the loudest deck that
the mapping reports, not to a deck with the same duration. Check 1 showed that
the Mixxx library keeps the stream header duration of a new track, so a
duration link fails for exactly the tracks whose header is wrong. The ADR was
Proposed, so no accepted decision was reversed.

Amended 2026-09-28 in `docs/plans/mixxx-midi-connector.md`: §Entering And
Leaving The Connector Mode adds rules that this ADR did not state. They add to
the decision and reverse none. The runbook change moves from acceptance to the
packaging task, because the setup that it describes does not exist before that
task.

Amended 2026-09-29 in the review of connector task 003: §Entering And Leaving
The Connector Mode names the row that existed before the entry, not the row
that the producer read. At startup the producer reads the latest row only
after the entry, and that row can come from an earlier Mixxx session. The
rule does not change.

Amended 2026-09-30: ADR 0007 is accepted. It adds commands from a consumer to
Mixxx, and the protocol becomes version 3. §Protocol points to ADR 0007 for
the command messages. The non-goal about commands no longer applies to the
commands of ADR 0007. No other decision here changes.

Amended 2026-09-30: §Context no longer names the global PortMidi error flag
as the cause of the output stop. `Pm_Write` clears that flag at its start. The
probable cause is the shared ALSA sequencer handle. The tested behavior and
the decision do not change.

Amended 2026-09-29 by the operator: this amendment changes one decision.
Before it, a row that existed before the entry into the connector mode never
linked. Now the producer links such a row again after an outage, when the
same track still plays on the same deck (§Relink After An Outage). The deck
sample count identifies the track, so the protocol becomes version 2. It adds
the sample count and an end marker for the complete state. The manual check
of 2026-09-29 showed the cost of the old rule: after a short outage, the
artist of the present track was paid nothing until the next track.

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
- A device that disappears while Mixxx sends to it can stop MIDI output for
  all controllers. The probable cause is the one ALSA sequencer handle that
  all PortMidi output ports share.
- A SysEx send that fails halfway leaves PortMidi in the middle of a message.
  The next messages then fail with `Invalid MIDI message Data`.
- The Mixxx deck duration comes from the decoder, and it is correct. The
  history row for a new track appears about 4.5 seconds after AutoDJ starts
  it.
- The Mixxx library keeps the stream header duration of a new track. Tested
  on 2026-09-28: a 200.04-second MP3 with no VBR header had
  `library.duration = 617.807` in its history row, after the load and after
  the eject, while the deck showed 200.020 s. A track that Mixxx loaded in an
  earlier session had the same value in the library and on the deck.
- Mixxx writes a history row when its "loudest deck" changes
  (`src/mixer/playerinfo.cpp:136-193`). A mapping can read each control that
  this rule uses.
- A hardware controller and the connector operate at the same time.
- An unplugged controller whose mapping sends feedback stops MIDI output for
  all controllers, also the connector. Tested on 2026-09-28: the heartbeat
  stopped, and Mixxx logged send errors for the connector port. When the
  operator disabled the dead controller in Preferences, the output came back
  with no Mixxx restart. The messages sent during the outage were lost.

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
| 1 | Protocol version, now 3 | Heartbeat, sent each second |
| 2 | 0 to 4 | The loudest deck that plays, or 0 for none. See §The Loudest Deck. |
| 3 | 1 | The end of the complete state |
| 10 + N | 0 or 127 | Deck N `play` |
| 20 + N | 0 or 127 | Deck N `track_loaded` |
| 30 + N | Bits 7 to 13 | Deck N `duration` in whole seconds, high part |
| 40 + N | Bits 0 to 6 | Deck N `duration` in whole seconds, low part. The value applies when this part arrives. |
| 50 + N | Bits 28 to 34 | Deck N `track_samples`, part 1 |
| 60 + N | Bits 21 to 27 | Deck N `track_samples`, part 2 |
| 70 + N | Bits 14 to 20 | Deck N `track_samples`, part 3 |
| 80 + N | Bits 7 to 13 | Deck N `track_samples`, part 4 |
| 90 + N | Bits 0 to 6 | Deck N `track_samples`, part 5. The value applies when this part arrives. |

`track_samples` is the length of the loaded track in engine samples, which is
the frame count multiplied by 2 (`src/engine/enginebuffer.cpp:547`, Mixxx
2.5.6). The mapping sends it rounded down to a whole number. It sends 0 for a
value that is not valid or not more than 0. Mixxx sets an invalid value at the
start of a load and at an eject (`enginebuffer.cpp:521` and `:616`). The value
0 means "unknown", and it never identifies a track.

The mapping sends the `play` and `track_loaded` messages when the control
changes, the duration parts when `duration` changes, and the sample parts when
`track_samples` changes. It sends the complete state of all four decks when it
starts and when a consumer asks. For each deck, the complete state holds
`play`, `track_loaded`, the two duration parts and the five sample parts.
Then it holds CC 2, and then CC 3 as the last message.

### The Loudest Deck

The mapping computes the same rule as `PlayerInfo::updateCurrentPlayingDeck`
in Mixxx 2.5.6 (`src/mixer/playerinfo.cpp:136-193`). It sends CC 2 when the
result changes, and in the complete state.

- A deck counts only when `play` is not 0, `pregain` is more than 0.25 and
  `volume` is not 0.
- The crossfader gain comes from `[Master],crossfader` with the value x, from
  -1 to 1. `PlayerInfo` uses the additive curve with the transform 1.0
  (`src/engine/enginexfader.cpp:16-58`). So the left gain is `1 - x` when x
  is more than 0, else 1. The right gain is `1 + x` when x is less than 0,
  else 1. A gain is never less than 0.
- The deck `orientation` selects the gain: 0 is left, 1 is center with the
  gain 1, and 2 is right.
- The deck value is `volume` multiplied by that gain. The deck with the
  highest value that is more than 0 is the loudest deck. For equal values, the
  lower deck number is the loudest deck, because Mixxx accepts only a strictly
  higher value.
- The mapping computes the rule again each 250 ms and when one of these
  controls changes.

`PlayerInfo` computes its rule each 2 seconds. So the mapping reports a change
up to 2 seconds before Mixxx writes the history row. When the row appears, the
reported deck is already the deck of that row.

From a consumer to Mixxx:

| CC | Value | Meaning |
|---|---|---|
| 1 | 1 | Send the complete state of all decks |
| 4 | Command code | Do a command of ADR 0007 |

A consumer ignores a message that repeats the last value. Mixxx sends
`play = 0` more than one time at a stop.

ADR 0007 owns the command messages: CC 4 from a consumer, and CC 4 and CC 5
from Mixxx. A change to a message meaning needs a new protocol version.

### How `mixxx-now-playing` Uses The Deck State

The history row still gives the track identity. The deck state gives the
timing.

- **The link:** when a history row gives a new track, the producer links it to
  the loudest deck that the mapping reported last. If that report is 0, or if
  that deck does not play, the producer writes no drop file for that track.
  The producer does not compare durations. The library duration can be the
  wrong header value.
- **The duration:** in connector mode, `duration_secs` in the drop file comes
  from the linked deck. The deck value comes from the decoder. The lofty value
  comes from the stream headers and can be very wrong.
- **A stop or a pause:** when the linked deck `play` becomes 0, the producer
  removes the drop file. The publisher then publishes the dead block (ADR
  0005).
- **A resume:** when the linked deck `play` becomes 127 again, the producer
  writes the drop file again after the settle time of §A Resume Settles.
- **A new load:** when the linked deck `track_loaded`, `duration` or
  `track_samples` changes, the link ends, and the producer removes the drop
  file.
- **The switch time:** the payee changes when the history row appears. That
  is the Mixxx "loudest deck" rule. The stream metadata that Mixxx sends to
  Icecast uses the same rule (`src/engine/sidechain/shoutconnection.cpp:772`,
  Mixxx 2.5.6).
- **A track in the last six tracks:** Mixxx writes no history row for it. The
  linked deck of the previous track stops at the end of the crossfade, so the
  producer removes the file. The replayed track then pays nobody. That result
  is safe.

### A Resume Settles

When a load into a playing deck starts, Mixxx sets `play` to 0 and then to 1.
It sets 1 before it changes `duration` and `track_samples`
(`EngineBuffer::slotTrackLoading`, `src/engine/enginebuffer.cpp:520`, Mixxx
2.5.6). The mapping cannot see that a load is in progress. For that time, the
`play` change looks like a resume of the old track.

- A resume of the linked deck starts a settle time of 2 seconds. The producer
  writes the drop file when the settle time ends, if the link still exists.
- A stop or a new load in the settle time cancels the resume.
- The display link uses the same rule. The display state shows the track
  again when the settle time ends.
- `RESUME_SETTLE` in `mixxx-now-playing/src/connector/link.rs` holds the
  time.

On 2026-10-06, before this rule, each load into the playing linked deck sent
the old track live for some milliseconds. The test
`a_load_into_the_playing_linked_deck_gives_one_remove_and_no_write` guards
the rule.

### When The Connector Is Not Available

The producer uses the history-only mode of ADR 0005, with `--expiry-max`, in
these conditions:

- the raw device cannot be opened,
- no heartbeat arrived in the last 3 seconds,
- the heartbeat gives a protocol version that the producer does not know.

The producer logs the change of mode with `tracing::warn!`. When heartbeats
arrive again, it asks for the complete state and returns to the connector
mode.

### Entering And Leaving The Connector Mode

- At startup, the producer acts on no history row until it knows the mode.
  The mode is known at the first heartbeat, after 3 seconds with no heartbeat,
  or when the raw device cannot be opened.
- When the producer enters the connector mode, it links no history row that
  existed before the entry. At startup, that is the first row that the
  producer reads. It removes the drop file. The next history row links.
  So the present track pays nobody until the next track. A link to the present
  loudest deck is not safe: a replayed track has no history row, so that link
  could pay the artist of the previous row.
- A row that existed before the entry links only by §Relink After An Outage.
- When the producer leaves the connector mode, the ADR 0005 expiry of the
  present track applies. That expiry starts at the time of the history row,
  not at the change of mode. If it already ended, the producer removes the
  drop file at once.
- When the producer leaves the connector mode, it does not write a drop file
  again that the connector mode removed.
- A MusicIndex API result changes the drop file only while the file is
  present. It never writes a file again that a stop removed.

### Relink After An Outage

When the producer leaves the connector mode, it keeps the deck and the
sample count of the link. When it enters the connector mode again, it asks
for the complete state. At the end marker (CC 3) of the first complete state
after the entry, it links the row again when all these conditions are true:

1. The row was linked in the connector mode before the outage.
2. No new history row arrived during the outage.
3. The deck of that link is the loudest deck.
4. That deck plays.
5. That deck has the same `track_samples` as before the outage, and the
   value is not 0.

Then the producer writes the drop file, with `duration_secs` from the deck.
If a condition is false, or if no end marker arrives before the next change
of mode, the row does not link. The present track then pays nobody until the
next track.

At startup, the producer has no link from before. So the first row at startup
never links.

A relink also applies after the ADR 0005 expiry ended during the outage. The
expiry is a limit for the time with no deck state. The complete state shows
that the same track still plays.

The remaining risk is a different track with the same sample count on the
same deck. For two different tracks, an equal count to one sample is very
improbable. A replay of the same track pays the correct artist.

### Operator Rules

- Disable a controller in Mixxx before you unplug it, if its mapping sends
  feedback such as LED output. If such a controller was unplugged, disable it
  in Preferences. The connector output then comes back with no restart.
- Do not leave a controller enabled whose device is not connected, if its
  mapping sends feedback.
- Enable only one Mixxx controller with the connector mapping.

## Invariants

- A consumer never makes a sequencer port, a JACK port or a PortMidi device.
- The protocol uses only 3-byte control change messages on channel 16.
- The producer never keeps a drop file while its linked deck does not play,
  when the connector is available.
- A `play` change alone never makes the drop file present before the settle
  time ends.
- The producer links a history row only to the loudest deck that the mapping
  reported. It never links a row to a deck that does not play.
- In connector mode, `duration_secs` comes from the linked deck, never from
  the stream headers.
- Without a heartbeat, the producer uses the ADR 0005 expiry. It never assumes
  that a deck plays.
- A row that existed before the entry into the connector mode links only when
  its deck, its play state and its sample count agree with the link from
  before the outage.

## Verification Before Acceptance

Manual, on a computer with Mixxx, because each check needs a running Mixxx:

1. **The link key.** Done on 2026-09-28, and it failed for a new track with a
   wrong header. That result changed the link rule. See §Context.
2. **A controller with feedback.** Done on 2026-09-28. The unplug stopped the
   connector output, and disabling the dead controller restored it. See
   §Context and §Operator Rules.
3. **`engine.beginTimer`.** Passed on 2026-09-28. The heartbeat ran for more
   than 10 minutes with no gap longer than 2 seconds. The one longer gap, 9.5
   seconds, was a Mixxx restart, and the new mapping sent the complete state
   after it.
4. **The loudest deck.** Passed on 2026-09-28. The test had seven history
   rows. Six came from AutoDJ and one from a manual change with the crossfader.
   Each row appeared 1.0 to 6.8 seconds after the mapping reported the deck of
   that track.

## Verification After Implementation

Mechanical:

- A unit test for the MIDI byte parser, with running status and a real-time
  byte between messages.
- A unit test for each producer rule in §How `mixxx-now-playing` Uses The Deck
  State, with a sequence of deck events and history rows.
- A unit test for each condition in §When The Connector Is Not Available.
- A unit test that the producer writes no drop file when the reported loudest
  deck is 0 or does not play.
- A test of the mapping rule for the loudest deck, with a JavaScript runtime
  outside Mixxx. It covers the crossfader at -1, 0 and 1, each orientation,
  `pregain` at 0.25, `volume` at 0, and equal values.
- A unit test that the drop file in connector mode has the deck duration, for
  a track whose header duration is different.
- A unit test for each condition in §Relink After An Outage, one test for an
  end marker that does not arrive, and one test for startup.
- A mapping test for the sample parts of 0, of an invalid value, and of a
  value above 2^32.

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

### Link By Duration

Rejected. Tested on 2026-09-28: the Mixxx library keeps the stream header
duration of a new track. A track with a wrong header then matches no deck and
pays nobody. Two decks with the same length also match no single deck.

### Link To The Deck That Started Last

Rejected. A DJ can start a deck with the volume down to cue it, while a
different deck becomes the loudest. The last deck to start is then not the
deck of the history row.

### Switch The Payee When The New Deck Starts

Deferred. The producer does not know the track identity until the history row
appears. A switch at the deck start would need the identity from a different
source.

### Relink By The Whole-Second Duration

Rejected 2026-09-29. Many tracks have the same length in whole seconds. The
sample count separates them. It needs a protocol version, and the producer
has no users, so the new version costs nothing.

### No Relink After An Outage

Replaced 2026-09-29. This was the rule before the amendment. It paid nothing
for the rest of the track after an outage of a few seconds.

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
- The mapping copies a Mixxx rule. A Mixxx release can change that rule, and
  the copy must then change too. Check 4 detects a difference for the Mixxx
  version in use.
- The payee still changes about 4.5 seconds after the new deck starts.
- A mapping bug can stop the heartbeat. The producer then falls back to the
  expiry, which is safe.

## Changes At Acceptance

- ADR 0001: the non-goal "Detecting pause or exact audio output state" no
  longer applies. Add a dated amendment that cites this ADR.
- ADR 0005: §Payment Timing Without The MIDI Connector stays in force as the history-only
  mode. Add a dated amendment that cites this ADR.
- `docs/architecture/mixxx-interfaces.md`: add the results of §Verification
  Before Acceptance.
- `docs/runbooks/musicindex-live-publisher-configuration.md`: add the operator
  setup for the V4V card and the Mixxx controller. The packaging task
  `docs/tasks/mixxx-connector-task-004-packaging-and-setup.md` owns this
  change.

## Non-Goals

- Commands from a consumer to Mixxx, other than the state request and the
  commands of ADR 0007.
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
