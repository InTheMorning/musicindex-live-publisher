# Listener Delay Task 001: The Delay From The Icecast Burst

Status: Implemented - 2026-11-26.

Every criterion is mechanical. ADR 0037 has the visual items.

## Goal

A target can give `icecast_burst_bytes` and `stream_bitrate_kbps` in place of `stream_delay_secs`. The publisher
calculates the delay from the two fields, rounds it down to a whole second, and sends it as `Listener-Delay-Secs`.
ADR 0037 owns each rule. Read it first.

## Files To Read

- `docs/adr/0037-the-listener-delay-comes-from-the-icecast-burst.md`
- `docs/adr/0011-relay-applies-stream-delay.md` §Send At Once and §The Setting
- `src/config.rs`: `PublisherTarget`, `TargetConfigSummary`, `RawTarget`, `RawTargetSummary`, `resolve_stream_delay`,
  `validate_stream_delay_secs`, the target edit functions near lines 940 to 1110
- `src/relay.rs`: `RelayTarget::from_config`, `round_listener_delay_secs`
- `src/main.rs`: the target list output near lines 890 to 925
- `tests/config.rs` near line 1453, `tests/relay.rs` near line 971

## Files To Change

- `src/config.rs`, `src/relay.rs`, `src/main.rs`
- `tests/config.rs`, `tests/relay.rs`
- `packaging/arch/musicindex-live-publisher.example.toml`: add the two fields as comments below
  `# stream_delay_secs = 0`, with one comment line that names ADR 0037 and says that a target has one form only.

## Do Not Touch

- The relay repository, the producers, the drop file and the display path.
- `docs/adr/`, `AGENTS.md`, `docs/README.md`.

## Base components

| Component | Status | Location | Reason |
| --- | --- | --- | --- |
| Target config parse and validation | extend | src/config.rs | Add two optional fields to `RawTarget` and `RawTargetSummary`, and validate them next to `resolve_stream_delay`. |
| Header value | extend | src/relay.rs | `RelayTarget::from_config` takes the resolved whole seconds. Keep `round_listener_delay_secs` for `stream_delay_secs`. |
| Target list output | extend | src/main.rs | Add `listener_delay_secs` to the text and the JSON output. |

## Design

- Add to `PublisherTarget` a field `listener_delay_secs: u64`. Config resolution sets it:
  - with the two Icecast fields: `icecast_burst_bytes * 8 / (stream_bitrate_kbps * 1000)`, rounded down. Use integer
    arithmetic: `burst * 8 / (kbps * 1000)` in `u64`, with checked or saturating multiplication.
  - else: the present half-up rounding of `stream_delay_secs`.
- Keep `stream_delay: Duration` on `PublisherTarget`. For an Icecast target, set it to the exact calculated delay,
  so that the start log line at `src/main.rs:89` shows it.
- `RelayTarget::from_config` uses `target.listener_delay_secs`. Move the half-up rounding into config resolution, or
  call it from there. Do not keep two copies of the rounding.
- Add the same `listener_delay_secs: u64` to `TargetConfigSummary`, with the same validation.
- Errors. Each message names the target and ADR 0037:
  - one Icecast field without the other;
  - the two Icecast fields and `stream_delay_secs` together;
  - an Icecast field with the value 0, or a negative value (TOML integers; parse as `i64` or `u64`, and give a clear
    error for a negative value);
  - a calculated delay above 300 seconds (`MAX_STREAM_DELAY_SECS`).
- The `target add --replace` edit keeps the lines that it does not replace. When the result has both forms, the edit
  must fail with the error above, and must not write the file. Find where the edit validates the new text, and make
  sure that the full validation runs there.

## Acceptance Criteria (Mechanical)

1. A target with `icecast_burst_bytes = 65535` and `stream_bitrate_kbps = 192` sends the header `2` (`tests/relay.rs`).
2. A calculated delay of 2.99 seconds gives `2`. Example: burst 71760, bitrate 192 gives 2.99 s.
3. `stream_delay_secs = 2.5` still gives the header `3`.
4. One Icecast field alone is a config error that names ADR 0037.
5. The two Icecast fields and `stream_delay_secs` together are a config error.
6. A value of 0 in either Icecast field is a config error.
7. A calculated delay above 300 seconds is a config error.
8. `target add --replace --stream-delay-secs 1` on a target with the two Icecast fields fails and leaves the file
   unchanged.
9. The target list text and JSON output have `listener_delay_secs`.
10. The gate passes: `cargo build && cargo test`, `cargo clippy --all-targets -- -D warnings`,
    `cargo fmt -- --check`, at the workspace root.

## Rules For The Delegate

- Write comments and messages in Simplified Technical English. The skill is
  `/home/citizen/.agents/skills/asd-ste100/SKILL.md`.
- Do not commit. Leave the change in the working tree and report the changed files and the gate output.
- Do not make an architecture decision. If a rule of ADR 0037 does not fit the code, stop and report.
