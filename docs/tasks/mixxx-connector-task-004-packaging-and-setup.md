# Mixxx Connector Task 004: Packaging And Setup

Status: Implemented - 2026-09-29. Two gates are open. See §Review Result.

The mechanical criteria and the visual criteria are in separate lists. The
visual criteria need a reboot and a running Mixxx.

## Goal

The package makes the V4V card at boot and installs the mapping. The producer
unit can open the raw MIDI device. The setup script checks the card. The
runbook tells the operator how to set up the card and the Mixxx controller.

## Files To Inspect

- `docs/adr/0006-mixxx-midi-connector.md` (§Card Setup, §Operator Rules and
  §Changes At Acceptance)
- `docs/plans/mixxx-midi-connector.md` (§Risk Areas, the unit sandbox)
- `packaging/arch/PKGBUILD`
- `packaging/arch/musicindex-live-publisher.install`
- `systemd/mixxx-now-playing.service`
- `scripts/setup-mixxx-musicindex.sh` (the generated producer unit and the
  Mixxx check)
- `docs/runbooks/musicindex-live-publisher-configuration.md`
- `docs/runbooks/musicindex-live-publisher-arch-package.md`
- `mixxx/` (the mapping files from task 001)

## Files Likely To Change

- `packaging/midi/musicindex-v4v-midi.modules-load.conf` (new)
- `packaging/midi/musicindex-v4v-midi.modprobe.conf` (new)
- `packaging/arch/PKGBUILD`
- `packaging/arch/musicindex-live-publisher.install`
- `systemd/mixxx-now-playing.service`
- `scripts/setup-mixxx-musicindex.sh`
- `docs/runbooks/musicindex-live-publisher-configuration.md`
- `docs/runbooks/musicindex-live-publisher-arch-package.md`

## Do Not Touch

- `src/**` and `mixxx-now-playing/**`
- `mixxx/**`
- `docs/adr/**`
- Any file in `/etc`. The package installs only in `/usr`.

## Constraints

- The modules file holds one line: `snd-virmidi`.
- The modprobe file holds one line:
  `options snd-virmidi enable=1 index=31 id=V4V midi_devs=1`.
- The package installs:
  - the modules file as `/usr/lib/modules-load.d/musicindex-v4v-midi.conf`,
  - the modprobe file as `/usr/lib/modprobe.d/musicindex-v4v-midi.conf`,
  - `mixxx/MusicIndex-V4V-Connector.midi.xml` and
    `mixxx/MusicIndex-V4V-Connector.js` in `/usr/share/mixxx/controllers/`.
- No install script loads, unloads or reloads a kernel module.
- The install message says:
  - the V4V card exists after the next reboot,
  - an operator who already uses `snd-virmidi` must merge the options, as the
    configuration runbook tells,
  - the operator enables "MusicIndex V4V Connector" on `VirMIDI 31-0` in
    Mixxx.
- The producer unit in `systemd/` and the unit that the setup script writes
  both set `PrivateDevices=false`. A comment above it cites ADR 0006 and says
  that the producer opens the raw MIDI device. Keep every other sandbox
  setting.
- The setup script checks the card before it writes a file:
  - `/proc/asound/V4V` is missing: stop with a message that says to reboot
    after the package install, and that names the configuration runbook.
  - The link is `card31`: print the port name `VirMIDI 31-0`.
  - The link is a different card: print a warning with the real port name,
    `VirMIDI N-0`, and continue.
  - An environment variable `MUSICINDEX_ASOUND_DIR` replaces `/proc/asound`,
    so a person can test the check without a real card.
- The configuration runbook gets a section "MIDI Connector". It restates ADR
  0006 and names it as the owner:
  - the card files and the reboot,
  - the merge for an operator who already uses `snd-virmidi`, with the
    example line from ADR 0006,
  - the controller setup in Mixxx Preferences,
  - the operator rules from ADR 0006 §Operator Rules,
  - the `--connector-card` and `--no-connector` options,
  - how to see the mode in `journalctl --user -u mixxx-now-playing`.
- The package runbook lists the new installed files.

## Implementation Steps

1. Add the two files in `packaging/midi/`.
2. Change `PKGBUILD` to install the two files and the two mapping files.
3. Change the install message.
4. Change the producer unit and the unit text in the setup script.
5. Add the card check to the setup script.
6. Add the runbook section and the package runbook list.

## Acceptance Criteria

Mechanical:

- `bash -n scripts/setup-mixxx-musicindex.sh` passes.
- With `MUSICINDEX_ASOUND_DIR` set to an empty `tempfile` directory, the setup
  script stops before it writes a file, and its message names the reboot.
- With `MUSICINDEX_ASOUND_DIR` set to a directory with the link
  `V4V -> card31`, the check prints `VirMIDI 31-0`.
- `grep -c PrivateDevices=false` gives 1 for `systemd/mixxx-now-playing.service`
  and 1 for the producer unit text in the setup script.
- The package from `makepkg` holds the four new files at the paths in
  §Constraints. Check with `bsdtar -tf`.
- The full cargo gate and `node --test mixxx/tests/` pass.

Visual. A person checks these after a reboot. The review records each result:

- `cat /proc/asound/cards` shows card 31 with the ID `V4V`.
- Mixxx lists "MusicIndex V4V Connector" in Preferences, Controllers, for
  `VirMIDI 31-0`.
- The producer unit log shows the connector mode.

## Test Commands

```bash
bash -n scripts/setup-mixxx-musicindex.sh
node --test mixxx/tests/
cargo fmt --all -- --check
cargo build --workspace
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
```

## Escalation Triggers

Stop and report if one of these occurs:

- `PrivateDevices=false` is not enough for the unit to open the device.
- The package cannot install into `/usr/share/mixxx/controllers/`.
- The setup script needs to write a file in `/etc`.
- The card check blocks a setup mode that needs no connector.

## Prompt for lower-context coding model

You are implementing one bounded task from a larger plan.

Implement only this task. Do not redesign the architecture.

Read:
- docs/adr/0006-mixxx-midi-connector.md
- docs/plans/mixxx-midi-connector.md
- docs/tasks/mixxx-connector-task-004-packaging-and-setup.md
- packaging/arch/PKGBUILD
- packaging/arch/musicindex-live-publisher.install
- systemd/mixxx-now-playing.service
- scripts/setup-mixxx-musicindex.sh
- docs/runbooks/musicindex-live-publisher-configuration.md
- docs/runbooks/musicindex-live-publisher-arch-package.md

Goal:
- The package makes the V4V card at boot and installs the Mixxx mapping. The producer unit can open /dev/snd. The setup script checks the card. The runbook tells the operator the setup.

Constraints:
- packaging/midi/musicindex-v4v-midi.modules-load.conf holds `snd-virmidi`. packaging/midi/musicindex-v4v-midi.modprobe.conf holds `options snd-virmidi enable=1 index=31 id=V4V midi_devs=1`.
- PKGBUILD installs them as /usr/lib/modules-load.d/musicindex-v4v-midi.conf and /usr/lib/modprobe.d/musicindex-v4v-midi.conf, and the two mixxx/ mapping files in /usr/share/mixxx/controllers/.
- No install script touches a kernel module. Nothing is written in /etc.
- The install message: reboot for the card, merge options if snd-virmidi is already used, enable "MusicIndex V4V Connector" on VirMIDI 31-0 in Mixxx.
- systemd/mixxx-now-playing.service and the setup script unit text set PrivateDevices=false with a comment that cites ADR 0006. Keep every other sandbox setting.
- The setup script checks /proc/asound/V4V before it writes a file. Missing: stop, tell the operator to reboot, name the runbook. card31: print VirMIDI 31-0. Other card N: warn with VirMIDI N-0 and continue. MUSICINDEX_ASOUND_DIR replaces /proc/asound.
- The configuration runbook gets a "MIDI Connector" section that restates ADR 0006 and names it as the owner. The package runbook lists the new files.

Do not touch:
- src/**, mixxx-now-playing/**, mixxx/**, docs/adr/**

Acceptance criteria:
- bash -n passes.
- An empty MUSICINDEX_ASOUND_DIR makes the setup stop before it writes a file, with a reboot message.
- A MUSICINDEX_ASOUND_DIR with V4V -> card31 prints VirMIDI 31-0.
- Both producer unit texts hold PrivateDevices=false one time.
- bsdtar -tf of the makepkg package lists the four new files.
- The cargo gate and node --test mixxx/tests/ pass.

Test commands:
- bash -n scripts/setup-mixxx-musicindex.sh
- node --test mixxx/tests/
- cargo fmt --all -- --check
- cargo build --workspace
- cargo test --workspace
- cargo clippy --workspace --all-targets -- -D warnings

At the end, report:
1. files changed
2. tests run
3. behavior changed
4. deviations from task
5. unresolved concerns

## Review Result

Reviewed 2026-09-29. The review changed no code.

- The card check ran with a scratch `HOME` and a test `MUSICINDEX_ASOUND_DIR`.
  An empty directory stopped the script before it wrote a file. A link to
  `card31` printed `VirMIDI 31-0`. A link to `card5` printed a warning with
  `VirMIDI 5-0`.
- `makepkg` could not run in the sandbox of the agent. A run of `package()`
  alone installed the four files at the correct paths. The `bsdtar -tf`
  criterion stays open until the operator builds the package.
- The setup script stops without the card also for an operator who wants
  `--no-connector`. ADR 0006 says that the script stops when the card is
  missing. The review accepts this.
- The visual criteria need a reboot and a running Mixxx. They stay open.
