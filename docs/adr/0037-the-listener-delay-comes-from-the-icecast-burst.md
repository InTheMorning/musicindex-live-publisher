# ADR 0037: The Listener Delay Comes From The Icecast Burst

- Status: Accepted on 2026-11-26 by the operator.
- Date: 2026-11-26
- Changes: ADR 0011 §Send At Once, the item about the value of `Listener-Delay-Secs`, and §The Setting. A target can
  give the Icecast burst in place of `stream_delay_secs` (§The Two Fields).
- Uses: `musicindex-live-relay` ADR 0004 §The Delay, without a change. The header, its whole seconds and its limit
  stay as that record states.
- Bar: opposite choice by mistake. Without this record, an agent can round the calculated delay up with the rule of
  ADR 0011, or add the Icecast `queue-size` to the delay.
- Operator choice, 2026-11-26: "i want the producer to send the relay a calculated delay amount based on icecast
  buffer/burst settings."
- Operator choice, 2026-11-26: the two Icecast values come from the producer configuration. The producer does not
  read `icecast.xml`, and it does not ask the AzuraCast API.

## Context

The relay holds each Socket.IO update for the delay in the header `Listener-Delay-Secs` (relay ADR 0004). The
publisher sends the `stream_delay_secs` of the target (ADR 0011). The test station has `0.0`.

On 2026-11-26, v4vmusic.com showed the picture 1 to 2 seconds before the audio (station log step 46). The ICY title
changed from 0.4 seconds before to 0.2 seconds after the publish (station log step 47). Thus the publish and the
stream change occur at almost the same time on the server. The listener hears the change later by the Icecast burst
and the player buffer.

Icecast sends a new client a burst of audio from its queue. The client then plays that much audio behind the live
point, for the full connection. The station has these values (station log step 47):

- `icecast.xml` `<limits>`: `<burst-size>65535</burst-size>` and `<queue-size>524288</queue-size>`. Both are global.
  The mount `/radio.mp3` has no burst value of its own.
- The mount `/radio.mp3` is MP3 at 192 kbit/s. The encoder line, the frames of the stream and the AzuraCast API agree.
- 65535 bytes at 192 kbit/s (24000 bytes each second) is 2.73 seconds of audio.

`queue-size` is the largest backlog that Icecast keeps for a slow client before it disconnects that client. It does
not set the delay of a client that plays at the stream rate.

The publisher and the station producer read one instance configuration file,
`musicindex-live-publisher/<instance>/config.toml` (`systemd/musicindex-live-publisher@.service`,
`now-playing-drop/src/instance.rs`). The publisher is the program that sends the header. Thus the publisher
calculates the delay from that file. No drop file field and no state field is necessary.

## Decision

### The Two Fields

A `[[target]]` stanza can have these two fields:

- `icecast_burst_bytes`: the Icecast `burst-size` of the mount that the listeners use, in bytes. An integer from 1.
- `stream_bitrate_kbps`: the bitrate of that mount, in kilobits each second (1000 bits). An integer from 1.

Rules:

- A target has both fields or neither field. One field alone is a configuration error.
- A target with the two fields has no `stream_delay_secs`. Both forms together are a configuration error.
- A target with neither form has a delay of 0, as before.
- The `config` edit command of ADR 0004 refuses an edit that gives a target both forms.

### The Calculation

The delay in seconds is `icecast_burst_bytes * 8 / (stream_bitrate_kbps * 1000)`.

- The header value is that delay rounded down to a whole second. On the test station, 2.73 seconds gives `2`.
- The rounding down applies only to the calculated delay. `stream_delay_secs` keeps the rounding of ADR 0011.
- A calculated delay above 300 seconds is a configuration error, as for `stream_delay_secs`.

The cause of the rounding down: the player adds its own buffer, which the publisher cannot know. A picture that
comes a little before the audio is acceptable. A payment block that comes after the audio pays the last track for that
time. The observed lead of 1 to 2 seconds also shows that the full 2.73 seconds is too much.

### The Test Station

The station target gets `icecast_burst_bytes = 65535` and `stream_bitrate_kbps = 192`, and loses
`stream_delay_secs = 0.0`. An operator who changes the burst or the bitrate in AzuraCast must change these two fields.

## Invariants

These rules apply while this decision is current.

- A target has at most one delay form: `stream_delay_secs`, or the two Icecast fields.
- The calculated header value is never more than the calculated delay.
- The Icecast `queue-size` has no effect on the delay.

## Verification After Implementation

Mechanical. The tests are in `tests/config.rs` and `tests/relay.rs`.

- A test that 65535 bytes and 192 kbit/s give the header `2`.
- A test that a calculated delay of 2.99 seconds gives `2`, and that `stream_delay_secs = 2.5` still gives `3`.
- A test that one Icecast field alone is a configuration error.
- A test that the two Icecast fields and `stream_delay_secs` together are a configuration error.
- A test that a value of 0 in either Icecast field is a configuration error.
- A test that a calculated delay above 300 seconds is a configuration error.
- A test that the `config` edit command refuses `--stream-delay-secs` for a target with the two Icecast fields.

Visual. A person must examine these items in an app. Report each one as open until a person completes the check.

- On the test station, v4vmusic.com shows the picture of the next song at the same time as the audio, or less than
  1 second before it.
- A boost at the start of a song pays the block of that song in v4vmusic.com.

## Alternatives Considered

### Read `burst-size` From `icecast.xml` In The Liquidsoap Slot Text

Not selected by the operator. The values would follow a change in AzuraCast. The file also holds the source and
admin passwords, so a read of it is a risk.

### The Bitrate From The AzuraCast API

Not selected by the operator. It needs the API URL and an available API at each start.

### Round To The Nearest Second

Rejected. On the test station it gives 3 seconds. With the player buffer, the payment block can then come after the
audio.

### A Drop File Field From The Producer

Rejected. The producer and the publisher read the same configuration file. A drop file field adds a format change
and gives no new information.

## Consequences

- Socket.IO apps get each block about 2 seconds later than before on the test station.
- A change of the burst or the bitrate in AzuraCast needs a manual change of the two fields.
- A player with a large buffer still gets the picture before the audio. The publisher cannot correct that.
- The SSE apps do not change. Their routes stay instant (relay ADR 0004).

## References

- `docs/adr/0011-relay-applies-stream-delay.md`
- `docs/runbooks/test-station-log.md`, steps 46 and 47
- `musicindex-live-relay/docs/adr/0004-listener-timeline-delay.md`
