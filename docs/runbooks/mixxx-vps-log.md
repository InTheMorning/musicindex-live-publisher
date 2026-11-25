# Mixxx VPS Log: The Stream Test Stack

Started: 2026-11-25.

This log records the changes on the VPS `bastone.heycitizen.xyz` that serves the Mixxx stream. The host runs the stack
of `~/build/musicindex-stream-test`: Icecast and pure Liquidsoap in Docker, with no AzuraCast. It is a different host
from the AzuraCast test station `radio.heycitizen.xyz`, which has its own [log](test-station-log.md).

`musicindex-stream-test` has no Git repository, so this log is the record of its changes. Add a line to
[Pending Changes](#pending-changes) when a decision needs a change on the VPS. Move it to [Steps](#steps) when the
change is on the VPS. Make each change in the same commit as the record that needs it.

This log is advisory. The publisher ADRs own the rules. The stack README (`musicindex-stream-test/README.md`) gives the
signal path and the deploy procedure.

## Starting Point

These facts come from the files in `~/build/musicindex-stream-test` on 2026-11-25. No command ran on the VPS.

- `docker-compose.yml` uses the image `savonet/liquidsoap:v2.4.1`.
- `liquidsoap/radio.liq` reads the Icecast mount `/live` with `input.http` and serves HLS with
  `output.harbor.hls` on port 8080. It has three AAC variants in the `adts` format, with `id3 = true` and
  `id3_version = 4`. Segments are 4 seconds.
- `radio.liq` changes no `settings.encoder.metadata` value.

## Pending Changes

| # | Change | Owner of the rule | Status |
| --- | --- | --- | --- |
| V1 | Change the Liquidsoap image to `savonet/liquidsoap:v2.4.6`, the version of the test station. The facts of ADR 0036 §Context come from the 2.4.6 source. The research repository has no 2.4.1 source, so the cover rule of 2.4.1 is not verified. | Publisher ADR 0036 | Open |
| V2 | In `radio.liq`, before the output: remove `apic` from `settings.encoder.metadata.cover`, and add `apic` to `settings.request.metadata_decoders.recode.exclude`. | Publisher ADR 0036 §The Liquidsoap Settings | Open |
| V3 | In `radio.liq`, a test only: at each ICY title change, put in the metadata with the key `APIC` and a fixed JPEG. The JPEG is less than 512 KiB. After 1 second, put in the same metadata with no `APIC`. | Publisher ADR 0036 §Before Acceptance item 1 | Open. Remove the fixed picture after the test. |
| V4 | The tagger of ADR 0009 gives the picture of each Mixxx song from the relay display state. It replaces the fixed picture of V3. | Publisher ADR 0009, ADR 0036 §The VPS Stack | Open. It needs the tagger and its record. |

## Steps

| # | Date (UTC) | Step | Source | Result |
| --- | --- | --- | --- | --- |
