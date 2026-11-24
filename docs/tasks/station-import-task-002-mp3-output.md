# Station Import Task 002: The Output Format `mp3`

Status: Done - 2026-11-24, with musicindex-lib `44cdc65`. The station check is open.

The criteria are mechanical. §Station Criteria has one check for a person.

## Goal

`station-import` gets the flags `--format keep|mp3` and `--mp3-bitrate KBPS` (musicindex ADR 0095). With
`--format mp3`, the import writes only MP3 files for songs and episodes. It downloads an MP3 file of the feed when the
feed gives one. Else it converts the enclosure to MP3 through musicindex-convert. The default `keep` does not change
the output.

musicindex ADR 0095 R-MP3-1 to R-MP3-6 own the rules. This task does not write them again.

## Base components

| Component | Status | Location | Reason |
| --- | --- | --- | --- |
| MP3 file of an item | reuse | ~/build/musicindex-lib/fetch/src/resolve.rs | `ResolvedItem::mp3_url` and `track_mp3_url` of musicindex-lib `44cdc65` select the file of R-MP3-2 item 1. The import keeps no second copy of the selection. |
| Conversion to MP3 | reuse | ~/build/musicindex-lib/convert/src/lib.rs | `convert_to_mp3_with` and `DEFAULT_MP3_BIT_RATE_KBPS` of musicindex-convert `44cdc65` (R-MP3-3 item 1). |
| Converter probe of one run | extend | station-import/src/lib.rs | The `Converter` of task 001 gives the `ffmpeg` probe of its one observation. |
| Enclosure download | reuse | ~/build/musicindex-lib/fetch/src/download.rs | `download_enclosure`, with no change. |
| Tag write and read | reuse | ~/build/musicindex-lib/tags/src/podcast.rs | `write_song_tags`, `write_episode_tags` and `read_podcast_tags` write and read ID3v2.4 in an MP3 file (R-MP3-5). |
| Argument parser | extend | station-import/src/main.rs | Two new flags and two new usage errors. |
| Test HTTP server | extend | station-import/tests/import.rs | It records the request paths, so that a test can show that the FLAC file was not requested. |

## Files To Change

- `station-import/Cargo.toml` and `Cargo.lock`: musicindex-fetch, musicindex-tags and musicindex-convert at
  musicindex-lib `44cdc65`.
- `station-import/src/lib.rs`, `station-import/src/main.rs`, `station-import/tests/import.rs`.
- `docs/runbooks/liquidsoap-now-playing-azuracast.md` §Import Songs And Episodes, and `AGENTS.md`.

## Mechanical Criteria

Each one is a test in `station-import/` (musicindex ADR 0095 §Checks, the items of the publisher):

1. `format_keep_is_the_default` (unit test in `src/main.rs`): with no `--format`, the format is `keep`. The tests of
   task 001 pass with no change, so the output of `keep` does not change (R-MP3-1 item 2).
2. `mp3_format_takes_the_feed_mp3_file_with_no_conversion`: an item with an MP3 enclosure gives that file. The run
   has no encoder, and the item has no error (R-MP3-2 items 1 and 2).
3. `mp3_format_takes_the_mp3_alternate_and_not_the_flac_file`: an item with a FLAC enclosure and two `audio/mpeg`
   alternate enclosures gives the first alternate file. The server gets no request for the FLAC file or for the
   second alternate file (R-MP3-2 item 1).
4. `converted_file_has_the_bit_rate_of_the_flag`: the encoder gets `-b:a 256k` with no `--mp3-bitrate`, and
   `-b:a 192k` with `--mp3-bitrate 192` (R-MP3-3 item 6). The tests use a script in place of `ffmpeg`.
5. `rejects_usage_errors` (unit test in `src/main.rs`): `--mp3-bitrate` with `--format keep` or with no `--format`,
   a bit rate out of 32 to 320, and a format that is not `keep` or `mp3` are usage errors (R-MP3-3 item 6).
6. `mp3_format_converts_a_wav_song` and `mp3_format_with_no_encoder_is_an_error_and_the_run_continues`: a WAV song
   becomes an MP3 file. With no `ffmpeg`, that song has an error, and the run writes the other song
   (R-MP3-2 item 4, R-MP3-4).
7. `mp3_format_episode_has_its_chapters`: an episode with a FLAC enclosure becomes an MP3 file. `read_podcast_tags`
   gives its GUIDs, its chapters and two `CHAP` frames in one `CTOC` frame. For each MP3 file, the manifest gives the `.mp3` path and the SHA-256 of that
   file (R-MP3-5, R-MP3-6).

Gate:

8. `cargo build --workspace`, `cargo test --workspace`, `cargo clippy --workspace --all-targets -- -D warnings` and
   `cargo fmt --all -- --check` pass.

## Station Criteria

- The orchestrator session runs `station-import --format mp3` on the station. A person examines the files. The task
  does not do this check.

## Choices

Implementer choice (main session, 2026-11-24): with `--format mp3`, only the selection of musicindex-fetch decides
that a file needs no conversion. The import does not examine the first bytes of a download for this decision, because
`AudioFormat::detect` of musicindex-tags at `44cdc65` also gives `Mp3` for an AAC ADTS file. With `--format keep`, a
WAV episode does not become a FLAC file, as before (musicindex ADR 0094 R-CONVERT-7 applies to songs).

Implementer choice (main session, 2026-11-24): a dry run with `--format mp3` gives the `.mp3` path and the selected
URL in the manifest.
