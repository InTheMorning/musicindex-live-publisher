# MusicIndex Live Publisher Arch Package

## Purpose

Build and install the Arch package of the two binaries:

- `musicindex-live-publisher`
- `mixxx-now-playing`

The package also installs the systemd user units and example configuration.

## Package Type

`packaging/arch/PKGBUILD` is a VCS package. It builds the newest commit of
the `master` branch from GitHub:

- Package name: `musicindex-live-publisher-git`
- Provides: `musicindex-live-publisher` and `mixxx-now-playing`. HitIt names
  `musicindex-live-publisher` as an optional dependency.
- Source: `git+https://github.com/InTheMorning/musicindex-live-publisher.git`
- Installed binaries: `/usr/bin/musicindex-live-publisher` and
  `/usr/bin/mixxx-now-playing`
- Installed user unit:
  `/usr/lib/systemd/user/musicindex-live-publisher.service`
- Installed template user units:
  `/usr/lib/systemd/user/musicindex-live-publisher@.service` and
  `/usr/lib/systemd/user/mixxx-now-playing@.service`
- Installed setup helper: `/usr/bin/setup-mixxx-musicindex`
- Installed MIDI card files (ADR 0006):
  `/usr/lib/modules-load.d/musicindex-v4v-midi.conf` and
  `/usr/lib/modprobe.d/musicindex-v4v-midi.conf`
- Installed Mixxx mapping files (ADR 0006):
  `/usr/share/mixxx/controllers/MusicIndex-V4V-Connector.midi.xml` and
  `/usr/share/mixxx/controllers/MusicIndex-V4V-Connector.js`

The package does not load the kernel module. The V4V card exists after the
next reboot. The setup helper stops if the card is missing. See
[MIDI Connector](musicindex-live-publisher-configuration.md#midi-connector)
for the card setup and the Mixxx controller setup.

In Mixxx, also set Preferences, then Library, "Track duplicate distance" to
0. Without it, a track that plays again within six tracks pays nobody. See
[Mixxx History Setup](musicindex-live-publisher-configuration.md#mixxx-history-setup).

The package does not build the working tree on disk. A change that is not
on GitHub is not in the package. To test a local change, use the manual
build path in the
[deploy runbook](musicindex-live-publisher-deploy.md).

## Prerequisites

```bash
sudo pacman -S --needed base-devel git rust
```

The package itself declares `openssl`, `ca-certificates`, and `pkgconf`; the
`makepkg -s` step installs any of those that are missing.

For runtime testing, install Mixxx and make sure it has created
`~/.mixxx/mixxxdb.sqlite`.

## Build And Install

The package needs two files: `PKGBUILD` and
`musicindex-live-publisher.install`. Download the two files into an empty
directory:

```bash
mkdir -p ~/build/musicindex-live-publisher-pkg
cd ~/build/musicindex-live-publisher-pkg
curl -fLO https://raw.githubusercontent.com/InTheMorning/musicindex-live-publisher/master/packaging/arch/PKGBUILD
curl -fLO https://raw.githubusercontent.com/InTheMorning/musicindex-live-publisher/master/packaging/arch/musicindex-live-publisher.install
makepkg -Csi
```

In a checkout, you can also use the two files in `packaging/arch`:

```bash
cd path/to/musicindex-live-publisher/packaging/arch
makepkg -Csi
```

`makepkg -Csi` cleans the earlier build directory and gets the source from
GitHub. It installs missing build dependencies with pacman. Then it builds
the package and installs it.

## Configure

For Mixxx, use the setup helper. It provisions the live item, writes the token
and config, and starts the pipeline with the packaged units
`musicindex-live-publisher@mixxx.service` and `mixxx-now-playing@mixxx.service`.
The helper writes no unit file. The publisher uses a fixed dead block for idle or non-V4V playback
(ADR 0005). No setup flag changes it:

```bash
setup-mixxx-musicindex
```

The helper stops when the config names a real event ID. It then sends no
relay request and changes no file. Only `--force` replaces that config. See
[Upgrade](#upgrade).

To use an event that already exists, give its ID and its token file. The
token file must have mode `0600`. The helper sends no relay request:

```bash
setup-mixxx-musicindex --event-id EVENT_ID --token-file TOKEN_FILE
```

Use `--instance NAME` for a different instance. The helper then runs no
`systemctl` command. See ADR 0021.

For manual setup, create the user config directory:

```bash
install -d -m 0700 ~/.config/musicindex-live-publisher/mixxx/tokens
```

Copy the example publisher config:

```bash
install -d -m 0700 ~/.config/musicindex-live-publisher/mixxx
cp /usr/share/doc/musicindex-live-publisher/examples/config.toml \
  ~/.config/musicindex-live-publisher/mixxx/config.toml
```

Provision a live item and token:

```bash
musicindex-live-publisher provision \
  --endpoint https://api.musicindex.org \
  --target default \
  --token-file ~/.config/musicindex-live-publisher/mixxx/tokens/default.token
```

Edit `~/.config/musicindex-live-publisher/mixxx/config.toml`:

- Replace `event_id` with the provisioned value.
- Keep
  `token_file = "~/.config/musicindex-live-publisher/mixxx/tokens/default.token"`.
- Do not add a `[target.fallback]` table. The publisher uses the fixed dead
  block (ADR 0005). No configuration changes it.

Optional producer config:

```bash
install -d -m 0755 ~/.config/hitit
cp /usr/share/doc/musicindex-live-publisher/examples/hitit-config.toml \
  ~/.config/hitit/config.toml
```

Edit `~/.config/hitit/config.toml` if your V4V music directory is not
`~/V4Vmusic`.

See [configuration options](musicindex-live-publisher-configuration.md) for all
settings.

## Start

```bash
systemctl --user daemon-reload
systemctl --user enable --now musicindex-live-publisher@mixxx.service
systemctl --user enable --now mixxx-now-playing@mixxx.service
```

Run only one Mixxx producer. Mixxx has one active desktop history
source. The second producer stops with exit status 3 (ADR 0021). Future non-Mixxx producers can use separate
`musicindex-live-publisher@<instance>.service` instances with their own configs
and watch directories.

Watch logs:

```bash
journalctl --user -u musicindex-live-publisher@mixxx.service -f
```

Check the producer drop file:

```bash
cat "$XDG_RUNTIME_DIR/musicindex-live-publisher/mixxx/nowplaying/default.json"
```

Check the plain now-playing text file:

```bash
cat "$XDG_RUNTIME_DIR/musicindex-live-publisher/mixxx/nowplaying/now-playing.txt"
```

## Foreground Dry Run

This prints payloads instead of publishing them:

```bash
install -d -m 0700 "$XDG_RUNTIME_DIR/musicindex-live-publisher/mixxx/nowplaying"
musicindex-live-publisher \
  --config ~/.config/musicindex-live-publisher/mixxx/config.toml \
  --watch-dir "$XDG_RUNTIME_DIR/musicindex-live-publisher/mixxx/nowplaying" \
  --dry-run \
  --verbose
```

In another terminal, run the producer once:

```bash
mixxx-now-playing \
  --format json \
  --target default \
  --id3-file "$XDG_RUNTIME_DIR/musicindex-live-publisher/mixxx/nowplaying/default.json" \
  --once \
  --verbose
```

## Upgrade

Build and install the new package in the directory of the two package files.
`makepkg` gets the newest commit of `master`:

```bash
cd ~/build/musicindex-live-publisher-pkg
makepkg -Csi
```

The package installs the template units, and a package upgrade updates them.
The units that the earlier setup helper wrote in `~/.config/systemd/user/`
stay in place and hide the package units. Move the private stream to the
templates one time:

```bash
setup-mixxx-musicindex --migrate-units
```

This command stops the earlier producer. It moves `mixxx-now-playing.service`
and `musicindex-live-publisher@mixxx.service` in `~/.config/systemd/user/` to a
backup, and it removes their enable links. It then enables
`musicindex-live-publisher@mixxx.service` and `mixxx-now-playing@mixxx.service`
when the earlier units were enabled. It starts each one when the earlier unit
was active.

The command does not change the config or a token, and it sends no relay
request. It stops at a unit file that has no
generated marker, unless you give `--force`. A second run changes nothing.

With `--no-start`, the helper runs no `systemctl` command. It prints the
commands instead.

Do not run the helper without `--migrate-units` for an upgrade. When the config
names a real event ID, the helper stops before the relay request. With
`--force`, it backs up the config and provisions a new live item. The old
event then leaves the config.

If you did not use the setup helper, restart the services:

```bash
systemctl --user restart musicindex-live-publisher@mixxx.service
systemctl --user restart mixxx-now-playing@mixxx.service
```

## Remove

```bash
systemctl --user disable --now mixxx-now-playing@mixxx.service
systemctl --user disable --now musicindex-live-publisher@mixxx.service
sudo pacman -R musicindex-live-publisher-git
```

Pacman removes package-owned files only. User config and token files under
`~/.config/` remain in place.

## Before Publishing To AUR

Do not publish this package as-is. First:

- For a stable package, drop the `-git` suffix and use a fixed release tarball
  with real checksums.
- Run `namcap PKGBUILD` and `namcap *.pkg.tar.zst`.

## Failure Modes

### `undefined symbol: sqlite3_open_v2` when linking `mixxx-now-playing`

Every `sqlite3_*` symbol comes back undefined from `rust-lld`, while a plain
`cargo build --release` in the repo succeeds.

Cause: makepkg's default `options=(lto)` adds `-flto=auto` to `CFLAGS`.
`mixxx-now-playing` builds `rusqlite` with the `bundled` feature, so the `cc`
crate compiles `sqlite3.c` with those `CFLAGS` and emits an archive of GCC LTO
bitcode instead of native objects. `rust-lld` cannot read GIMPLE bitcode without
the GCC LTO plugin, so it sees an archive with no symbols in it.

Fix: the `PKGBUILD` sets `options=(!lto)`. Rust still applies its own LTO through
the release profile, so nothing is lost. If you copy this `PKGBUILD` elsewhere,
carry that line with it.

Reproduce the failure, and its fix, without makepkg:

```bash
# fails
CFLAGS="-O2 -flto=auto" CARGO_TARGET_DIR=/tmp/lto-check \
  cargo build --release --manifest-path mixxx-now-playing/Cargo.toml

# succeeds
CFLAGS="-O2" CARGO_TARGET_DIR=/tmp/nolto-check \
  cargo build --release --manifest-path mixxx-now-playing/Cargo.toml
```

An alternative fix, not taken here: drop the `bundled` feature and add `sqlite`
to `depends`, linking the system library instead. That is closer to Arch
convention, but it unpins the SQLite version the crate is tested against and
changes behaviour for non-Arch builds, so it is a repo-wide decision rather than
a packaging one.

### `makepkg` fails part-way and leaves an unreadable `pkg/`

A partial run can leave `packaging/arch/pkg` with restrictive permissions, which
makes later runs and even `ls` fail with `Permission denied`. Clean before
rebuilding:

```bash
cd packaging/arch
chmod -R u+rwX pkg src 2>/dev/null || true
makepkg -C
```

### `ERROR: /etc/makepkg.conf not found`

`makepkg` is being run somewhere without an Arch build configuration, such as a
container or sandbox. Build on the host instead.

### `cargo fetch --locked` fails in `prepare()`

`Cargo.lock` is out of step with one of the workspace manifests. Run
`cargo update -p <crate>` or plain `cargo build` in the repo to refresh the root
lockfile, commit it, then rebuild the package.

## References

- ArchWiki: [Rust package guidelines](https://wiki.archlinux.org/title/Rust_package_guidelines)
- ArchWiki: [Arch package guidelines](https://wiki.archlinux.org/title/Arch_package_guidelines)
- ArchWiki: [PKGBUILD](https://wiki.archlinux.org/title/PKGBUILD)
- ArchWiki: [Creating packages](https://wiki.archlinux.org/title/Creating_packages)
