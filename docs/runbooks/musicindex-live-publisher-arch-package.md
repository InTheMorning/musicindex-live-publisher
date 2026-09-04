# MusicIndex Live Publisher Arch Package

## Purpose

Build and install a local Arch package for testing the two binaries:

- `musicindex-live-publisher`
- `mixxx-now-playing`

The package also installs the systemd user units and example configuration.

## Package Type

`packaging/arch/PKGBUILD` is a local working-tree package:

- Package name: `musicindex-live-publisher-git`
- Source: the local checkout at `/home/citizen/build/musicindex-live-publisher`, or
  `$MUSICINDEX_LIVE_PUBLISHER_REPO` when set
- Installed binaries: `/usr/bin/musicindex-live-publisher` and
  `/usr/bin/mixxx-now-playing`
- Installed user units:
  `/usr/lib/systemd/user/musicindex-live-publisher.service` and
  `/usr/lib/systemd/user/mixxx-now-playing.service`
- Installed template user unit:
  `/usr/lib/systemd/user/musicindex-live-publisher@.service`
- Installed setup helper: `/usr/bin/setup-mixxx-musicindex`

The package builds the working tree on disk. If the tree is dirty, the generated
package version ends in `.local`. Commit local edits first when you need a
traceable package version.

## Prerequisites

```bash
sudo pacman -S --needed base-devel git rust
```

The package itself declares `openssl`, `ca-certificates`, and `pkgconf`; the
`makepkg -s` step installs any of those that are missing.

For runtime testing, install Mixxx and make sure it has created
`~/.mixxx/mixxxdb.sqlite`.

## Build And Install

```bash
cd /home/citizen/build/musicindex-live-publisher/packaging/arch
makepkg -Csi
```

`makepkg -Csi` cleans the previous build directory, builds the package, installs
missing build dependencies with pacman, and installs the resulting package.

If you keep the package directory somewhere else, point it at this checkout:

```bash
MUSICINDEX_LIVE_PUBLISHER_REPO=/home/citizen/build/musicindex-live-publisher makepkg -Csi
```

## Configure

For Mixxx, use the setup helper. It provisions the live item, writes the token
and config, generates `musicindex-live-publisher@mixxx.service`, and starts the
pipeline. With no fallback route options, idle/non-V4V playback uses a dead
fallback route:

```bash
setup-mixxx-musicindex
```

To receive station payments when no V4V track is playing, pass a fallback value
block:

```bash
install -d -m 0700 ~/.config/musicindex-live-publisher
cp /usr/share/doc/musicindex-live-publisher/examples/mixxx-fallback-value-block.toml \
  ~/.config/musicindex-live-publisher/mixxx-fallback.toml
$EDITOR ~/.config/musicindex-live-publisher/mixxx-fallback.toml
setup-mixxx-musicindex \
  --fallback-value-block ~/.config/musicindex-live-publisher/mixxx-fallback.toml
```

Use temporary mode for a current-login rehearsal:

```bash
setup-mixxx-musicindex --temporary
```

The script refuses placeholder fallback destinations, so set `address` to the
real recipient address before provisioning. For `type = "node"`, that is the
Lightning node pubkey.

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
- Add a fallback value block only if idle/non-V4V time should receive station
  payments.

Optional producer config:

```bash
install -d -m 0755 ~/.config/v4vmm
cp /usr/share/doc/musicindex-live-publisher/examples/v4vmm-config.toml \
  ~/.config/v4vmm/config.toml
```

Edit `~/.config/v4vmm/config.toml` if your V4V music directory is not
`~/V4Vmusic`.

See [configuration options](musicindex-live-publisher-configuration.md) for all
settings.

## Start

```bash
systemctl --user daemon-reload
systemctl --user enable --now musicindex-live-publisher@mixxx.service
systemctl --user enable --now mixxx-now-playing.service
```

Only run one `mixxx-now-playing.service`; Mixxx has one active desktop history
source. Future non-Mixxx producers can use separate
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

```bash
cd /home/citizen/build/musicindex-live-publisher/packaging/arch
makepkg -Csi
systemctl --user restart musicindex-live-publisher@mixxx.service
systemctl --user restart mixxx-now-playing.service
```

## Remove

```bash
systemctl --user disable --now mixxx-now-playing.service
systemctl --user disable --now musicindex-live-publisher@mixxx.service
sudo pacman -R musicindex-live-publisher-git
```

Pacman removes package-owned files only. User config and token files under
`~/.config/` remain in place.

## Before Publishing To AUR

Do not publish this package as-is. First:

- Add an upstream `LICENSE` file.
- Replace `license=('LicenseRef-UNKNOWN')` with the real SPDX identifier.
- Replace the local working-tree source with a public release or VCS URL.
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
