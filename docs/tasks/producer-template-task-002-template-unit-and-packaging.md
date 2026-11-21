# Producer Template Task 002: The Template Unit And The Packages

Status: Planned 2026-11-21.

Every criterion is mechanical.

## Goal

The package installs `mixxx-now-playing@.service` as ADR 0021 §The Unit Names and §One Mixxx Producer At A Time say.
It no longer installs `mixxx-now-playing.service`.

## Base components

| Component | Status | Location | Reason |
| --- | --- | --- | --- |
| Producer unit | extend | systemd/mixxx-now-playing.service | Rename to the template and add the instance lines. Keep the hardening lines. |
| Publisher template unit | reuse | systemd/musicindex-live-publisher@.service | Change only the configuration path to `%E`. |
| Arch package | extend | packaging/arch/PKGBUILD | Install the new file name. |
| Debian package | extend | Cargo.toml | Change `[package.metadata.deb]` assets. |

## Files To Inspect

- `docs/adr/0021-mixxx-producer-template-unit.md`
- `systemd/mixxx-now-playing.service`, `systemd/musicindex-live-publisher@.service`
- `packaging/arch/PKGBUILD`, `Cargo.toml` `[package.metadata.deb]`
- `scripts/setup-mixxx-musicindex.sh` lines 505-580, the units that it writes at this time

## Files Likely To Change

- `git mv systemd/mixxx-now-playing.service systemd/mixxx-now-playing@.service`
- `systemd/musicindex-live-publisher@.service`
- `packaging/arch/PKGBUILD`
- `Cargo.toml`
- A test that reads the unit files, in `tests/` (new file `tests/units.rs`)

## Do Not Touch

- `scripts/**`. Task 003 changes the setup helper.
- `src/**`, `mixxx-now-playing/src/**`.
- `systemd/musicindex-live-publisher.service`.

## Constraints

- The producer template has:
  - `Wants=musicindex-live-publisher@%i.service` and `After=musicindex-live-publisher@%i.service`
  - `ExecStart=/usr/bin/mixxx-now-playing --format json --instance %i`
  - `Restart=always`, `RestartSec=10s`, `RestartPreventExitStatus=3`
  - `RuntimeDirectory=musicindex-live-publisher/%i/nowplaying musicindex-live-publisher/%i/display mixxx-now-playing`
  - `RuntimeDirectoryMode=0700`, `RuntimeDirectoryPreserve=yes`
  - the hardening lines of the producer unit that the setup helper writes at this time, with `PrivateDevices=false`
    for the MIDI device (ADR 0006)
  - `[Install]` `WantedBy=default.target`
- The publisher template reads `--config %E/musicindex-live-publisher/%i/config.toml`. Keep each other line.
- Use only systemd specifiers (`%i`, `%E`, `%t`, `%S`, `%h`) for paths. Put no home path, no `/run/user/` number and
  no host name in a unit file.
- Comments in the units are in Simplified Technical English (`~/.agents/skills/asd-ste100/SKILL.md`).

## Acceptance Criteria

All are mechanical.

1. A test in `tests/units.rs` reads `systemd/mixxx-now-playing@.service`. It asserts the `ExecStart=` line, the
   `Wants=` and `After=` lines, `RestartPreventExitStatus=3`, and the three runtime directories.
2. The same test asserts that `systemd/mixxx-now-playing.service` does not exist.
3. The same test asserts that no file in `systemd/` contains `/home/`, `/run/user/` or a digit after `/run/user`.
4. `Cargo.toml` and `packaging/arch/PKGBUILD` name `systemd/mixxx-now-playing@.service` and do not name
   `systemd/mixxx-now-playing.service`. A test asserts this.
5. `systemd-analyze --user verify` gives no error for the two templates with an instance, when the command is
   available. When it is not available, the report says so and the criterion stays open.
6. The gate of `AGENTS.md` passes.

## Delegate Rules

A delegate does not commit. It leaves the change in the working tree and reports the files and the gate result.
