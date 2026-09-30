# Mixxx Interfaces

Date: 2026-09-27. Mixxx 2.5.6, PortMidi 2.0.7, Linux 7.2.6.

This document is advisory. It states no rule. It records what Mixxx gives to
an external program, for `mixxx-now-playing` and for a future MIDI connector
between Mixxx and `v4vmm`. A decision that uses these facts goes into an ADR.

Each fact has an evidence class:

- **Tested:** seen on a Linux computer with Mixxx on 2026-09-27, with a
  probe program and a probe mapping. The test kit is not in version control.
- **Source:** read in the Mixxx source at tag `2.5.6`, in the PortMidi source
  at tag `v2.0.7`, or in the Linux kernel source.
- **Documentation:** from the Mixxx controller mapping documentation.
- **Not verified:** a probable fact that no test or source read confirms.

## Summary

- A hardware DJ controller and a MIDI connector operate at the same time. Each
  device has its own mapping.
- A kernel `snd-virmidi` port is a stable entry for a connector. Mixxx opens
  the sequencer side at startup. The connector opens the raw MIDI device as a
  file. Mixxx and the connector can start, stop and restart in any order.
- A port that a program makes is found only if it exists when Mixxx starts. If
  the program restarts, the input to Mixxx stops until Mixxx restarts.
- One MIDI device that disappears while Mixxx sends to it can stop MIDI output
  for all controllers. The probable cause is the one ALSA sequencer handle that
  all PortMidi output ports share. The cause is not confirmed.
- A mapping script can monitor the deck play state. It cannot identify the
  track. The track identity must come from a different source.
- The deck duration comes from the decoder, and it is correct. The duration
  that lofty reads from the headers can be very wrong.
- Mixxx writes a history row about 4.5 seconds after AutoDJ starts the next
  track. It writes no row when a deck stops.

## Controllers

| Fact | Evidence |
|---|---|
| Mixxx can have more than one controller open, each with its own mapping and script engine. | Tested: the probe mapping on a virtual MIDI card and a USB control surface operated at the same time. Source: `src/controllers/controllermanager.h:84`, `src/controllers/controller.h` |
| One device has one active mapping. | Documentation |
| Mixxx builds the controller list only at startup. | Tested: a probe port made after startup was not in the list. A USB control surface that was plugged in again did not operate until Mixxx restarted. Source: `src/controllers/controllermanager.cpp:190` |
| Mixxx hides the `Midi Through` ports, except in `--developer` mode. | Tested: not in the list. Source: `src/controllers/midi/portmidienumerator.cpp:13-20` |
| Mixxx joins a MIDI input and a MIDI output into one controller only when their names match. | Tested: two probe ports with different names gave no output. Source: `portmidienumerator.cpp:123-170` |
| PortMidi lists each ALSA sequencer port that accepts a subscription, except the system client. A port from the Rust crate `midir` is listed. | Tested. Source: PortMidi `pm_linux/pmlinuxalsa.c:867-887` |
| Mixxx makes one ALSA client, named `Client-N`, with one port for each open device. | Tested with `aconnect -l` |

### How PortMidi Addresses A Device

| Fact | Evidence |
|---|---|
| Output goes directly to the device `client:port` number. It uses no subscription. | Source: `pmlinuxalsa.c:265` |
| Input uses a subscription that Mixxx makes when it opens the device. | Source: `pmlinuxalsa.c:444` |
| When a program port restarts, the input subscription is lost. Output still arrives if the program gets the same client number again. | Tested: the probe restarted with the same numbers, received from Mixxx, and could not send to Mixxx. |
| A message that arrives at the Mixxx input port of a device goes to the mapping of that device, whatever program sent it. | Tested: messages from a different ALSA client, connected through a2j and JACK to the Mixxx input port of a virtual MIDI card, reached the mapping of that card. |
| PortMidi records an ALSA error in the global flag `pm_hosterror`. Mixxx never reads it with `Pm_GetHostErrorText`, so its log shows only "Host error". The flag alone does not stop other output ports, because `Pm_Write` sets it to false at its start. | Source: PortMidi `pm_common/portmidi.c:49`, `:504-520` and `:664`, `pmlinuxalsa.c:79-87`. Mixxx: `portmidicontroller.cpp:214-221`. Corrected 2026-09-30. |
| All PortMidi output ports on ALSA send through one sequencer handle, so they share one ALSA output buffer. An event for a destination that is gone can probably block that buffer for every port. Not confirmed. | Source: `pmlinuxalsa.c:48`, `:301` and `:513`. Mixxx opens outputs with latency 0 (`portmididevice.h:38-44`). |
| A controller whose port disappeared stopped the output of a different controller. | Tested: after the virtual probe stopped, sends to a virtual MIDI card failed with `Host error`, then with `Invalid MIDI message Data`. After the dead controller was disabled and Mixxx restarted, the errors stopped. |
| Unplugging a controller whose mapping sends nothing back caused no send error. | Tested: a USB control surface, 0 errors |
| Unplugging a controller whose mapping sends feedback stops the output of all controllers. Disabling the dead controller in Preferences restores the output with no restart. The messages sent during the outage are lost. | Tested on 2026-09-28: a mapping that sent to a USB control surface each 250 ms. After the unplug, the connector heartbeat stopped, and Mixxx logged 109 send errors for the connector port. |
| A mapping timer from `engine.beginTimer` with 1000 ms runs with no gap longer than 2 seconds. | Tested on 2026-09-28, more than 10 minutes |

### The Kernel Virtual MIDI Port

| Fact | Evidence |
|---|---|
| Each `snd-virmidi` card has a sequencer port, for example `VirMIDI 31-0`, and a raw MIDI device, for example `hw:V4V,0`. Both exist from boot. | Tested. Source: kernel `sound/drivers/virmidi.c` |
| Bytes that a program writes to the raw device go to each subscriber of the sequencer port. | Tested: the probe started and stopped deck 3. Source: kernel `sound/core/seq/seq_virmidi.c:50-51` and `:134-160` |
| Messages that Mixxx sends to the sequencer port go to each program that reads the raw device. Each open file gets its own copy. | Tested: the probe and `amidi` both received. Source: `seq_virmidi.c:72-107` |
| The raw device path comes from the card ID: `/proc/asound/<ID>` names the card number N, and the device is `/dev/snd/midiCND0`. | Tested |
| Mixxx stores a controller by its port name, which contains the card number. The card number can change if the module load order changes. | Tested: a Mixxx configuration held entries for virtual card numbers that no longer existed. |
| The port name is always `VirMIDI N-M`, with the card number N. No module option changes it. The options are `enable`, `index`, `id` and `midi_devs`. The `id` option names the card, so the raw device can be `hw:<ID>,0`. | Source: kernel `sound/core/seq/seq_virmidi.c:388`, `sound/drivers/virmidi.c:54-61` |
| The highest card number is `CONFIG_SND_MAX_CARDS - 1`. The Arch kernel `7.2.6-arch2-1` has `CONFIG_SND_MAX_CARDS=32`, so the highest number is 31. | Source: the kernel build configuration |
| The connector received a reply 3 ms after a command, also after a connector restart while Mixxx ran. | Tested |

### What A Mapping Script Can Do

| Capability | Evidence |
|---|---|
| Read and set a control with `engine.getValue` and `engine.setValue`. | Tested. Source: `src/controllers/scripting/legacy/controllerscriptinterfacelegacy.h` |
| Get a callback when a control changes, with `engine.makeConnection`. | Tested |
| Send MIDI with `midi.sendShortMsg` and `midi.sendSysexMsg`. | Tested |
| Receive MIDI input and call a script function. | Tested |
| Start and stop a deck, and start an AutoDJ fade or skip. | Tested: `play` on a deck, and a mapping that sets `[AutoDJ],fade_now`. |
| A script that sets `[AutoDJ],fade_now` to 1 and then to 0 starts the AutoDJ transition at once. During a transition, Mixxx ignores it. With AutoDJ disabled, it does nothing. | Tested on 2026-09-30 with a script on the V4V card. Source: `src/library/autodj/autodjprocessor.cpp:203-230` |
| Read the file path, title, artist or library identifier of the loaded track. | Tested: not possible. The controls `track_location`, `track_title`, `track_artist`, `track_id`, `location`, `title` and `artist` do not exist. `engine` and `midi` have no call that returns a track. |

The `engine` object has these members: `getSetting`, `getValue`, `setValue`,
`getParameter`, `setParameter`, `getParameterForValue`, `reset`,
`getDefaultValue`, `getDefaultParameter`, `makeConnection`,
`makeUnbufferedConnection`, `connectControl`, `trigger`, `log`, `beginTimer`,
`stopTimer`, the scratch functions, `softTakeover`,
`softTakeoverIgnoreNextValue`, `brake`, `spinback` and `softStart`. The `midi`
object has `send`, `sendShortMsg`, `sendSysexMsg` and `makeInputHandler`.

### Deck Controls That Matter

Each control is in the group `[ChannelN]`, where N is the deck number.

| Control | Meaning | Evidence |
|---|---|---|
| `play` | The deck plays | Tested |
| `play_indicator` | The play light state. It changes 2 ms before `play`. | Tested |
| `playposition` | The position, from 0 to 1 | Tested |
| `track_loaded` | A track is loaded | Tested |
| `duration` | The track duration in seconds | Tested |
| `track_samples` | The track length in samples, for all channels together | Tested: `track_samples / 2 / track_samplerate` equals `duration` for a stereo track |
| `track_samplerate` | The sample rate | Tested |
| `volume`, `pregain` | The deck gain | Source: `src/mixer/playerinfo.h:41-42` |

The `[Master],crossfader` control and the deck orientation decide how loud
each deck is in the main mix.

### AutoDJ Deck Events

Tested with two AutoDJ track changes:

- AutoDJ starts the next deck 6.0 seconds before the old deck stops. Both
  decks play during that time.
- The old deck stops at the end of its track.
- AutoDJ loads the next track into the stopped deck about 20 ms after the
  stop. That deck then has a loaded track that does not play.
- Mixxx sends `play = 0` three times at each stop.

## The History Database

`mixxx-now-playing` reads the most recent row of the history playlist
(`Playlists.hidden = 2`, ordered by `pl_datetime_added` and `id`). See
`mixxx-now-playing/src/history.rs:16-24`.

| Fact | Evidence |
|---|---|
| Each 2 seconds, `PlayerInfo` finds the loudest deck that plays. | Source: `src/mixer/playerinfo.cpp:13` and `:151-183` |
| A deck counts only when `play` is on, `pregain` is more than 0.25 and `volume` is more than 0. The crossfader and the orientation change the result. | Source: `src/mixer/playerinfo.cpp:155-178` |
| The history feature adds the track when the loudest deck changes. A load alone adds no row. | Source: `src/library/trackset/setlogfeature.cpp:128-130` and `:590-663` |
| In an AutoDJ change, the history row appeared 4.49 s and 4.65 s after the new deck started, with a poll each 0.5 s. | Tested |
| A track that is in the last six played tracks gets no new row and no play count. | Source: `setlogfeature.cpp:596-626`, `src/library/library_prefs.h:41` |
| When no deck plays, the history feature does nothing. | Source: `setlogfeature.cpp:591-593` |

### Effects On The Present Producer

- The producer sees a new track about 4.5 seconds after it starts. During that
  time, the previous artist stays payable. The stream delay comes on top of
  that time.
- The producer cannot see a stop or a pause. ADR 0005 limits the payable time
  with `--expiry-max` for this reason.
- If the DJ plays a track that is in the last six tracks, the producer does
  not see it. The previous drop file stays until its expiry.

## Track Duration

| Fact | Evidence |
|---|---|
| When Mixxx loads a track, it replaces the duration from the file metadata with the duration from the decoder. | Source: `src/track/trackmetadata.cpp:16-70`, `src/track/trackrecord.cpp:205` |
| For a 200.04-second VBR MP3 with no VBR header, the Mixxx deck showed 200.02 s. lofty showed 617.81 s, and so did `ffprobe`. | Tested |
| For the same audio with a Xing header, Mixxx and lofty both showed 200.05 s. | Tested |
| The library does not store the decoder duration of a new track at once. A new 200.04-second MP3 with no VBR header kept `library.duration = 617.807`, the header estimate, after the load, in its history row, and after the eject. The deck showed 200.020 s. | Tested on 2026-09-28 |
| A track that Mixxx loaded in an earlier session had the same duration in the library and on the deck, to the millisecond. | Tested on 2026-09-28, three tracks |
| No code in `src/` reads the ID3 `TLEN` frame. | Source |
| The deck control `track_samples` is the track length in engine samples: the frame count multiplied by 2. Mixxx sets it when a track loads. At the start of a load and at an eject, it sets an invalid end position. | Source: `src/engine/enginebuffer.cpp:521`, `:547` and `:616`, `src/audio/frame.h:45-47` |

## Other Interfaces

| Interface | Result in 2.5.6 | Evidence |
|---|---|---|
| OSC | None found | Source: no OSC library or receiver in `src/` or `CMakeLists.txt` |
| MPRIS or D-Bus | None found | Source: no match for `mpris` or `QDBus` in `src/` |
| HTTP API | None found | Source: no HTTP or TCP server class in `src/` |
| Icecast and Shoutcast broadcast | Present | Source: `src/broadcast/broadcastmanager.cpp`, `src/engine/sidechain/shoutconnection.cpp` |
| AutoDJ | It uses the same deck controls as manual play. | Tested |

## Open Questions

- Two mappings that set the same control: does the last write win?

## References

- `docs/adr/0001-rust-now-playing-lifecycle.md`
- `docs/adr/0005-producer-liveness-and-dead-block.md`
- `mixxx-now-playing/src/history.rs`
- Mixxx source: https://github.com/mixxxdj/mixxx, tag `2.5.6`
- PortMidi source: https://github.com/PortMidi/portmidi, tag `v2.0.7`
- Linux source: `sound/core/seq/seq_virmidi.c`, `sound/core/seq/seq_ports.c`,
  `sound/core/rawmidi.c`
