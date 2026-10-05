# Packaging Pass Plan

Date: 2026-09-29. This plan states no rule. It collects the intended package
behavior and the deferred packaging work in one place. A later pass turns it
into task packets. The owners of the rules are ADR 0005, ADR 0006 and
`AGENTS.md` §1.

## Status

Deferred. The package files for ADR 0006 are merged, but nobody built or
installed that package. Connector task 004 left two gates open. They move to
this plan:

- The `makepkg` build and the `bsdtar -tf` listing of the four new files.
- Manual check 5 of `docs/plans/mixxx-midi-connector.md`: the installed
  producer unit opens the V4V raw MIDI device.

Until this pass is complete, the operator runs the connector from a
development setup. See `docs/plans/mixxx-midi-connector.md` §Development
Setup For The Manual Checks.

## Goal

One pass makes the package correct, builds it, installs it on a test
computer, and verifies each item below. After the pass, an operator can
install the package, reboot, run the setup helper, and enable one Mixxx
controller.

## Intended Behavior

### What The Package Installs

| Path | Source | Owner |
|---|---|---|
| `/usr/bin/musicindex-live-publisher` | publisher crate | this repository |
| `/usr/bin/mixxx-now-playing` | producer crate | this repository |
| `/usr/bin/setup-mixxx-musicindex` | `scripts/setup-mixxx-musicindex.sh` | this repository |
| `/usr/lib/systemd/user/*.service` | `systemd/` | `AGENTS.md` §1 |
| `/usr/lib/modules-load.d/musicindex-v4v-midi.conf` | `packaging/midi/` | ADR 0006 §Card Setup |
| `/usr/lib/modprobe.d/musicindex-v4v-midi.conf` | `packaging/midi/` | ADR 0006 §Card Setup |
| `/usr/share/mixxx/controllers/MusicIndex-V4V-Connector.*` | `mixxx/` | ADR 0006 §Ownership |
| `/usr/share/doc/musicindex-live-publisher/` | runbooks and examples | this repository |

### What The Package Does Not Do

- It does not load, unload or reload a kernel module. The V4V card exists
  after the next reboot (ADR 0006 §Card Setup).
- It writes nothing in `/etc`. An operator who already uses `snd-virmidi`
  merges the options in `/etc/modprobe.d/musicindex-v4v-midi.conf`.
- It does not start, enable or change a user unit. The setup helper does
  that.
- It does not enable the Mixxx controller. The operator does that in Mixxx
  Preferences.

### What The Setup Helper Does

- It checks `/proc/asound/V4V` before it writes a file. A missing card stops
  it. A card with a number other than 31 gives a warning with the real port
  name.
- It writes the publisher and producer units. The producer unit sets
  `PrivateDevices=false`, so the producer can open `/dev/snd`.

## Open Items

Each item needs a decision or a check in the pass.

1. **The build gate.** Build with `makepkg -f` on the host. List the package
   with `bsdtar -tf` and confirm the four ADR 0006 files.
2. **A second options line.** The package file in `/usr/lib/modprobe.d` and
   an operator file in `/etc/modprobe.d` with a different name give two
   `options snd-virmidi` lines. Check what `modprobe` does with two lines for
   one module. Then decide if the install message is enough, or if
   `post_install` must look for other `snd-virmidi` options and warn. A check
   of that kind only reads files.
3. **The mapping path.** `/usr/share/mixxx/controllers/` is a directory of the
   Mixxx package. Check it with `namcap`. Decide if the package keeps that
   path, or if the setup helper copies the mapping to
   `~/.mixxx/controllers/`. A Mixxx from Flatpak does not read either path.
4. **Two copies of the producer unit.** `systemd/mixxx-now-playing.service`
   and the unit text in the setup helper must change together. Decide if one
   source is possible.
5. **The upgrade path.** A unit that an older setup helper wrote keeps
   `PrivateDevices=true`, and that producer cannot open the device. Decide if
   the operator runs the setup helper again, and say so in the install
   message and the runbook. Seen on 2026-10-04: the operator's generated unit
   in `~/.config/systemd/user/` kept `PrivateDevices=true`, and the producer
   logged `history-only mode reason=NoDevice`. A run of the setup helper also
   provisions a new relay event, so it is not a safe upgrade step. The pass
   needs an upgrade path that changes only the unit.

   Closed on 2026-10-05 by reserved safety task 003. The operator runs
   `setup-mixxx-musicindex --units-only`. That run writes only the two units.
   The install message and the package runbook give that step.
6. **The setup helper and `--no-connector`.** The helper stops without the
   card, also for an operator who does not want the connector. The helper
   has no `--connector-card` or `--no-connector` option. ADR 0006 says that
   the helper stops. A change needs an ADR amendment.
7. **The mapping tests in the package build.** `check()` runs only
   `cargo test`. Decide if `node --test mixxx/tests/` runs in `check()`, with
   `nodejs` in `checkdepends`.
8. **Removal.** `pacman -R` leaves the V4V card until the next reboot. The
   operator file in `/etc/modprobe.d` stays. Say so in the package runbook.
9. **The display directory of the producer (ADR 0008).** The producer unit
   sets `ProtectSystem=strict`. The producer can write only in the paths of
   `RuntimeDirectory=`. A unit with `--display-dir` must also name the display
   directory in `RuntimeDirectory=`. Decide if the packaged unit and the setup
   helper turn on the display output, or if the operator adds it. Found on
   2026-10-04.
10. **Before publishing to AUR.** The list in
   `docs/runbooks/musicindex-live-publisher-arch-package.md` §Before
   Publishing To AUR still applies: a license, a public source, a release
   without `-git`, and `namcap`.

## Acceptance Criteria For The Pass

Mechanical:

- `makepkg -f` builds the package on the host with no error.
- `bsdtar -tf` lists each path in §What The Package Installs.
- `namcap` gives no error for the PKGBUILD and the package.
- Each decision in §Open Items is in a task packet, or in an ADR amendment
  when it changes a rule.

Visual. A person checks these on a test computer:

- After the install and a reboot, `cat /proc/asound/cards` shows card 31
  with the ID `V4V`.
- Mixxx lists "MusicIndex V4V Connector" for `VirMIDI 31-0`.
- The producer unit from the setup helper logs `connector mode`.

## References

- `docs/adr/0006-mixxx-midi-connector.md`
- `docs/plans/mixxx-midi-connector.md`
- `docs/tasks/mixxx-connector-task-004-packaging-and-setup.md`
- `docs/runbooks/musicindex-live-publisher-arch-package.md`
- `docs/runbooks/musicindex-live-publisher-configuration.md` §MIDI Connector
