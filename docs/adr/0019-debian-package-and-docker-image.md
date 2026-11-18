# ADR 0019: The Debian Package And The Docker Image

- Status: Accepted on 2026-11-18 by the operator. The orchestrator session relayed the decision.
- Date: 2026-11-18
- Operator choice, 2026-11-18: the Docker image builds only on GitHub through `release.yml`. No host build is
  necessary.
- Bar: crosses repositories. musicindex ADR 0084 R-PACKAGE-5 asks each repository for its packaging record. The
  Docker image also changes where the producer and the publisher run.

This record applies musicindex ADR 0084 to this repository. It does not write the rules of that record again.
The orchestrator session sent the task on 2026-11-18.

## Context

musicindex ADR 0084 R-PACKAGE-2 asks this repository for an Arch PKGBUILD, a Debian `.deb` and a Docker image.
The PKGBUILD is in `packaging/arch/`. No `.deb` and no Docker image existed.

The repository has two programs:

- `musicindex-live-publisher` reads the drop files and sends each block to the relay. It needs the network, its
  configuration, its token files and the drop directory.
- `mixxx-now-playing` reads the Mixxx history database in the home folder. It opens the raw MIDI device of the V4V
  card in `/dev/snd` (ADR 0006). It writes the drop files.

Terms:

- **cargo-deb** is a Cargo subcommand that makes a `.deb` from the `Cargo.toml` of a package. This is an established
  tool.
- **cargo-chef** is a tool that keeps the compiled dependencies in a Docker layer. This is an established tool.

## Decision

### 1. cargo-deb makes the .deb

1. The section `[package.metadata.deb]` of the root `Cargo.toml` gives the assets of the `.deb`.
2. The assets are the files of `package()` in `packaging/arch/PKGBUILD`, without the Arch runbook. A change to one
   list changes the other list in the same commit.
3. The `.deb` installs the three systemd user units of `systemd/` in `/usr/lib/systemd/user/`. It does not turn on
   or start a unit. The setup helper does that, as in the PKGBUILD.
4. The `.deb` lists `mixxx` in `Recommends`. The PKGBUILD lists it in `optdepends`.
5. The build command is `cargo build --release --locked --workspace`, then `cargo deb --no-build --locked`.
   One workspace build makes the two binaries with the same flags.

### 2. The Docker image holds the publisher only

1. `Dockerfile` builds `musicindex-live-publisher` on `rust:alpine` with cargo-chef. The runtime image is Alpine
   with `ca-certificates`. `../stophammer/Dockerfile` is the model.
2. The image does not hold `mixxx-now-playing`. The producer needs the Mixxx database, the music files and the MIDI
   device of the host. The producer runs on the host from the `.deb` or the PKGBUILD.
3. The configuration of the setup helper holds absolute host paths in `token_file`, `watch_dir` and `display_dir`.
   Thus the operator mounts each host folder at the same path in the container:
   - The configuration root `~/.config/musicindex-live-publisher`. The instance check of ADR 0016 then reads the
     other instances.
   - The runtime folder of the instance, which holds the drop directory and the display directory of ADR 0008.
   - The state directory of the instance, for the show log of ADR 0017.
4. The operator starts the container with the user ID and the `HOME` of the host user. The drop directory and the
   token files have mode `0700` and `0600`.
5. The image has no default command. The operator gives the arguments of `systemd/musicindex-live-publisher@.service`.
   For the instance `mixxx`:

```bash
docker run -d --name musicindex-live-publisher-mixxx \
  --user "$(id -u):$(id -g)" -e HOME="$HOME" \
  -v "$HOME/.config/musicindex-live-publisher:$HOME/.config/musicindex-live-publisher" \
  -v "$XDG_RUNTIME_DIR/musicindex-live-publisher/mixxx:$XDG_RUNTIME_DIR/musicindex-live-publisher/mixxx" \
  -v "$HOME/.local/state/musicindex-live-publisher/mixxx:$HOME/.local/state/musicindex-live-publisher/mixxx" \
  ghcr.io/inthemorning/musicindex-live-publisher:0.1.0 \
  --config "$HOME/.config/musicindex-live-publisher/mixxx/config.toml" \
  --watch-dir "$XDG_RUNTIME_DIR/musicindex-live-publisher/mixxx/nowplaying" \
  --state-dir "$HOME/.local/state/musicindex-live-publisher/mixxx"
```

### 3. A tag starts the release workflow

`.github/workflows/release.yml` runs for each tag `v*`.

1. The job `deb` builds the `.deb` on `ubuntu-24.04` with cargo-deb 3.8.0. It examines the binaries, the units and
   `Recommends` in the `.deb`. It attaches the `.deb` to the GitHub release of the tag.
2. The job `image` builds the target `publisher` of `Dockerfile` for `linux/amd64`. It pushes the image to
   `ghcr.io/<owner>/musicindex-live-publisher:<version>`.
3. Each job gives the git revision and the build time to `build.rs` (musicindex ADR 0083 R-VERSION-8).

The workflow does not build the PKGBUILD. musicindex ADR 0084 Open item 2 gives no decision for that point.

## Checks

Mechanical:

- The job `deb` fails when the `.deb` has no binary, no systemd unit or no `mixxx` in `Recommends`.

Manual, because the CI has no test computer:

- A person installs the `.deb` on a Debian or Ubuntu computer and runs the setup helper.
- A person runs the image with the three mounts and sees a published block.

## Open

1. The image has no `linux/arm64` build.
2. The image tag `latest` and a candidate scheme, as in stophammer, are not decided.

These checks stay open until the first run of `release.yml`:

1. The job `image` builds the Docker image.
2. `dpkg-shlibdeps` on the Ubuntu runner adds the library entries to `Depends` of the `.deb`. A local build on
   2026-11-18 at commit `cf3e146` had no `dpkg-shlibdeps`. Its `Depends` had only `bash, ca-certificates, systemd`.

## Consequences

- The asset list of the `.deb` and `package()` of the PKGBUILD are two copies. No test compares them.
- The `.deb` and the image use the version of `Cargo.toml`. A release changes that version first (musicindex ADR
  0083 R-VERSION-7).
