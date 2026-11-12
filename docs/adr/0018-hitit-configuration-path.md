# ADR 0018: The HitIt Configuration Path

Status: Proposed
Date: 2026-11-11

The code change for this ADR is in the same commit. The operator must accept
this ADR.

Class: situational. The rule exists because of HitIt ADR 0096. It stays until
HitIt removes the move code of HitIt ADR 0096 Decision 5.

## Context

`mixxx-now-playing` reads `music_dir` and `musicindex_endpoint` from the
configuration file of the app HitIt. Before HitIt ADR 0096, the app had the
name v4vmm. The producer read `~/.config/v4vmm/config.toml` through
`ProjectDirs::from("xyz", "HeyCitizen", "v4vmm")`.

HitIt ADR 0096 Decision 3 changes the project directories to
`ProjectDirs::from("xyz", "HeyCitizen", "hitit")`. HitIt ADR 0096 Decision 5
moves the `v4vmm` configuration directory to the `hitit` path when HitIt
starts.

After that move, the producer did not find a file. It used `~/V4Vmusic` and
the default MusicIndex endpoint, and it gave no message. The orchestrator
found this on 2026-11-11, before a release of HitIt with the move.

## Decision

1. The producer reads the configuration file at the `hitit` path of HitIt
   ADR 0096 Decision 3 first. On Linux, this is
   `$XDG_CONFIG_HOME/hitit/config.toml`.
2. If there is no file at that path, the producer reads the file at the
   `v4vmm` path. This covers an install where HitIt has not started since the
   update, so the move of HitIt ADR 0096 Decision 5 has not occurred.
3. If there is no file at the two paths, the producer writes a warning to
   standard error. The warning gives the two paths. Then the producer uses
   the defaults, as before. It must not use the defaults with no message.
4. The packaging example and the runbooks give the `hitit` path.

The sequence for the V4V root does not change: the flag, then the
environment value, then the file.

## Invariants

- The function `select_config_path` in `mixxx-now-playing/src/config.rs`
  selects the `hitit` file before the `v4vmm` file. Unit tests enforce
  Decisions 1 and 2.
- The integration test `musicindex_once_mode_waits_for_api_routes_before_exit`
  writes the file at the `hitit` path and gets the endpoint from it.
- No test enforces the warning of Decision 3. A person examines the standard
  error of the producer with no configuration file.

## Alternatives Considered

### Read Only The `hitit` Path

This is simpler. But until HitIt starts one time after the update, the
producer finds no file. A producer that starts before HitIt then uses the
incorrect music directory.

### Move The Directory In The Producer

The producer would do the move of HitIt ADR 0096 Decision 5. HitIt owns its
user data, so a second writer is not correct. The producer only reads the
file.

## Consequences

- When HitIt removes its move code, a subsequent ADR removes the `v4vmm`
  fallback of Decision 2, and this ADR becomes superseded.
- The Arch package installs the example as `examples/hitit-config.toml`.

## References

- HitIt ADR 0096 (`~/build/HitIt/docs/adr/0096-the-app-name-is-hitit.md`),
  Decision 3, Decision 5 and Consequences
- ADR 0001 of this repository, the V4V root from the configuration of the app
