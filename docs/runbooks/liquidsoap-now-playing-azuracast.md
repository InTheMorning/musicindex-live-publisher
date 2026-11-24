# Liquidsoap Producer On An AzuraCast Host

## Purpose

This runbook installs `liquidsoap-now-playing` and the publisher on a radio host with AzuraCast. One user account
with lingering owns the two units (ADR 0022). Lingering is a systemd function: the user services of the account
operate with no login.

The station side comes first. The tutorial in `musicindex-research`,
`research_notes/azuracast/musicindex-producer-tutorial.md`, Part 1 steps 1 to 7, gives it. The slot text of step 5 is
`liquidsoap/musicindex-state.liq` in this repository. The package installs it as
`/usr/share/doc/musicindex-live-publisher/liquidsoap/musicindex-state.liq`.

This slot text writes the format `musicindex-liquidsoap-state/2` (ADR 0025). It reads only the `Podcast` frames of
musicindex ADR 0086, for example `TXXX:Podcast Item Guid`. It does not read the old `MusicIndex` frames. A file with
only old frames pays nobody. Install this slot text only after HitIt wrote the new frames in all the files of the
station. The operator installs it in the same maintenance window as the Mixxx reader with the new frames.

The steps use two names:

- `INSTANCE` is the instance name of ADR 0022 §The Template Unit. It is the AzuraCast short name with each `_` changed
  to `-`, for example `heycitizens-test-radio`.
- `musicindex` is the user account. You can use a different name.

Replace the two names in each command.

## Prerequisites

- Steps 1 to 7 of the tutorial are complete. `/var/lib/musicindex/azuracast/INSTANCE/state.json` exists, and its
  `written` value increases each second.
- A `.deb` package of this repository. Make it on a different computer (ADR 0019). A small radio host does not have
  the CPU and the memory to make it.
- `liquidsoap-now-playing` operates on the radio host, as the user account, not in the AzuraCast containers. It sends
  HTTPS requests to the route service (ADR 0025 §The Chapter Route Lookup), and it uses the OpenSSL library of the
  host. On Debian and Ubuntu this is `libssl3`. The `.deb` package declares it, so `apt install` installs it. On a
  host without `apt`, install the OpenSSL library before the first start.
- The host can open `https://api.musicindex.org`, or the URL of `--route-service`.
- The `event_id` and the broadcaster token file of the station event. Each event has one owner instance (ADR 0016).
- Free memory for AzuraCast at its highest use, and approximately 20 MiB for the two units. On 2026-11-22, a test host
  used all its memory and swap. The kernel then stopped the user manager of the account.
  The cause was 300 `docker-proxy` processes with 960 MiB: AzuraCast publishes 150 ports, and the default Docker
  userland proxy starts two processes for each port. Count them as root:

  ```sh
  ps -C docker-proxy --no-headers | wc -l
  ```

  If the count is large, set `"userland-proxy": false` in `/etc/docker/daemon.json` and restart Docker. The tutorial
  gives this step. §Restart The User Manager gives the recovery after a stop.

## Make The Account

Do these steps as root:

```sh
useradd --create-home --shell /bin/bash musicindex
loginctl enable-linger musicindex
apt install ./musicindex-live-publisher_VERSION_amd64.deb
```

Replace `VERSION` with the version in the file name of the package.

On a host without `apt`, use the station bundle. Make it on a different computer with
`scripts/make-station-bundle.sh`. The script puts the bundle in `target/station-bundle-REVISION/`. Its binaries are
static, so the host needs no OpenSSL library. Copy the folder to the host, and examine it with `sha256sum -c SHA256SUMS`.
Then install the files as root in the bundle folder:

```sh
install -m 0755 musicindex-live-publisher liquidsoap-now-playing /usr/bin/
install -m 0644 musicindex-live-publisher@.service liquidsoap-now-playing@.service /usr/lib/systemd/user/
```

Do not make a folder in the home folder of the account as root. A folder that root owns, for example `~/.config`,
stops `systemctl --user enable`.

## Restart The User Manager

The user manager is the service `user@UID.service`, where `UID` is the user ID of the account. If the kernel stops it
because the host has no free memory, systemd also stops the two units. Lingering does not start the user manager again,
and the publisher stops its keepalive. The relay then removes the value at the end of the lease. This failure pays
nobody incorrectly, but MusicIndex stays off until a person starts the user manager.

Do these steps as root. Replace `UID` with the output of `id -u musicindex`.

```sh
install -d -m 0755 /etc/systemd/system/user@UID.service.d
printf '[Unit]\nStartLimitIntervalSec=0\n\n[Service]\nRestart=on-failure\nRestartSec=10s\n' \
  > /etc/systemd/system/user@UID.service.d/restart.conf
systemctl daemon-reload
systemctl show -p Restart -p StartLimitIntervalUSec user@UID.service
```

The last command must show `Restart=on-failure` and `StartLimitIntervalUSec=0`. After a restart, the user manager
starts the two enabled units. While the host has no free memory, the kernel can stop the user manager again during the
start. `StartLimitIntervalSec=0` removes the start limit, so systemd tries again each 10 seconds until the user
manager operates.

Do the test of this step on a test host only. The test stops MusicIndex for 10 seconds or more:

```sh
systemctl kill --kill-whom=main --signal=SIGKILL user@UID.service
systemctl is-active user@UID.service
```

Run the second command 60 seconds after the first. It must show `active`. On a host with almost no free memory, the start
can use more than one try. On 2026-11-22 the test host used two tries and 32 seconds. If the command shows
`activating`, run it again after 30 seconds. Then do the steps of §Examine The Result.

## Make Sure That The User Manager Operates

From a different computer, log in as the account with SSH:

```sh
ssh musicindex@HOST systemctl --user is-system-running
```

The result must be `running` or `degraded`. With lingering, systemd starts the user manager of the account at boot.
On an SSH login with `UsePAM yes`, `pam_systemd` sets `XDG_RUNTIME_DIR`. This is an inference from the systemd
documents. This step is the test.

If the command gives `Failed to connect to bus`, set the variable and do the step again:

```sh
export XDG_RUNTIME_DIR=/run/user/$(id -u)
systemctl --user is-system-running
```

Do not use `systemctl --user -M` from a different account. Do each step below as the account, in an SSH login.

## Put The Token File On The Host

The token file of the station stays on the radio host, with mode `0600`. Copy it with a method that does not show its
content, for example `scp`. Do not put the token in a command line or in a log.

```sh
install -d -m 0700 ~/.config/musicindex-live-publisher/INSTANCE/tokens
chmod 0600 ~/.config/musicindex-live-publisher/INSTANCE/tokens/default.token
```

The DJ computer needs its own copy of the token file for the DJ publisher (ADR 0023).

## Write The Configuration

Write `~/.config/musicindex-live-publisher/INSTANCE/config.toml` (ADR 0016) with the endpoint and the drop
directory:

```toml
endpoint = "https://api.musicindex.org"
# The unit gives --watch-dir with the same path. The commands of
# musicindex-live-publisher read this value.
watch_dir = "/run/user/UID/musicindex-live-publisher/INSTANCE/nowplaying"
```

Replace `UID` with the output of `id -u`.

Then add the target. The producer writes the drop file for the target `default`.

```sh
musicindex-live-publisher target add \
  --config ~/.config/musicindex-live-publisher/INSTANCE/config.toml \
  --name default \
  --event-id EVENT_ID \
  --token-file ~/.config/musicindex-live-publisher/INSTANCE/tokens/default.token
```

Replace `EVENT_ID` with the `event_id` of the station event.

Open `~/.config/musicindex-live-publisher/INSTANCE/config.toml`. In the `[[target]]` table with the name `default`,
add this line:

```toml
display_dir = "/run/user/UID/musicindex-live-publisher/INSTANCE/display"
```

The producer writes `display.json` in this folder (ADR 0026). The unit makes the folder. Without this line, the relay
gets no display state. Replace `UID` as in the step before.

Examine the result:

```sh
musicindex-live-publisher config show \
  --config ~/.config/musicindex-live-publisher/INSTANCE/config.toml \
  --json
```

The output shows the target `default` and the token file path. It does not show the token.

## Make The State Directory

Make the state directory of the publisher before the first start:

```sh
install -d -m 0700 ~/.local/state/musicindex-live-publisher/INSTANCE
```

Why: the publisher unit sets `StateDirectory=musicindex-live-publisher/%i`. If that directory is missing and
`~/.config/musicindex-live-publisher/INSTANCE` exists, systemd 254 or later makes the state directory a symbolic link
to the configuration directory. The journal then shows "Unit state directory ... missing but matching configuration
directory ... exists". The show log then goes into the folder of the token file.

If the journal showed this message, stop the publisher, remove the link and make the directory:

```sh
systemctl --user stop musicindex-live-publisher@INSTANCE.service
ls -l ~/.local/state/musicindex-live-publisher/
rm ~/.local/state/musicindex-live-publisher/INSTANCE
install -d -m 0700 ~/.local/state/musicindex-live-publisher/INSTANCE
```

Use `rm` only when `ls -l` shows `INSTANCE ->`, which is a link. The show log files are
`showlog/showlog-YYYY-MM-DD.jsonl` in the state directory (ADR 0017). Through the link, they went into the folder
`~/.config/musicindex-live-publisher/INSTANCE/showlog`. Move that folder into the new state directory, if you want to
keep the show log:

```sh
mv ~/.config/musicindex-live-publisher/INSTANCE/showlog ~/.local/state/musicindex-live-publisher/INSTANCE/
systemctl --user start musicindex-live-publisher@INSTANCE.service
```

## Start The Units

The package installs `musicindex-live-publisher@.service` and `liquidsoap-now-playing@.service` in
`/usr/lib/systemd/user/`.

```sh
systemctl --user daemon-reload
systemctl --user enable --now musicindex-live-publisher@INSTANCE.service
systemctl --user enable --now liquidsoap-now-playing@INSTANCE.service
```

The producer unit reads `/var/lib/musicindex/azuracast/INSTANCE/state.json`. For a station without AzuraCast, change
the path with a drop-in file:

```sh
systemctl --user edit liquidsoap-now-playing@INSTANCE.service
```

## Examine The Result

The drop file of the producer:

```sh
cat "$XDG_RUNTIME_DIR/musicindex-live-publisher/INSTANCE/nowplaying/default.json"
```

At a track with the two GUIDs and value routes, the file has `feed_guid`, `track_guid` and `value_routes`. While a
live DJ is on air, the file is not there.

The value on the relay:

```sh
curl -s https://api.musicindex.org/v1/liveitems/EVENT_ID/metadata
```

The log of the two units:

```sh
journalctl --user -u liquidsoap-now-playing@INSTANCE.service -u musicindex-live-publisher@INSTANCE.service -n 50
```

## Import Songs And Episodes

Run `station-import` on your computer, not on the station host. Do not install it on
the station host. The station needs only the files that you send with SFTP.

The station bundle has a `station-import` binary for x86-64 Linux. Run it from the
bundle folder, for example `./station-import`. On a different computer, build it from
this repository with `cargo build --release -p station-import`.

First, run a dry run. Replace the feed URLs and selectors with your values:

```sh
station-import --out station-import-output --songs https://example.org/music.xml --episode-feed https://example.org/podcast.xml --episode episode-guid-1 --episode 7 --dry-run
```

Read `manifest.json` first. Examine each item with an `error` value. Examine each entry in `unresolved_splits`.

`flac` is an optional but recommended dependency of `station-import` on your computer (musicindex ADR 0094 §The
Packages). `ffmpeg` is also optional. With `flac` or `ffmpeg` on `PATH`, `station-import` converts a WAV song to
FLAC and tags the FLAC file. The manifest gives the path and the SHA-256 hash of the FLAC file. The WAV file does not
stay in the output folder. Without the two programs, the manifest gives an `error` value for each WAV song that
starts with `cannot convert the WAV song to FLAC`, and the run continues with the other items. For example, on Arch
Linux install `flac` with this command:

```sh
sudo pacman -S flac
```

An item GUID with characters that are not safe in a file name, for example a URL, gives the file name
`sha256-` and the SHA-256 hex of the GUID. The tags and the manifest keep the item GUID.

Then write the files:

```sh
station-import --out station-import-output --songs https://example.org/music.xml --episode-feed https://example.org/podcast.xml --episode episode-guid-1 --episode 7
```

Send the `songs/` and `episodes/` folders to the station with its AzuraCast SFTP account. The command does not upload files (musicindex ADR 0092 R-STATION-5).

## Rollback

```sh
systemctl --user disable --now liquidsoap-now-playing@INSTANCE.service
systemctl --user disable --now musicindex-live-publisher@INSTANCE.service
```

When the producer stops, the publisher sends the dead block one time (ADR 0005). Make sure that the relay shows the
destination `no-v4v-track@example.invalid` before you stop the publisher. To remove the station side, use the section
"Remove The Changes" of the tutorial.

## Failure Modes

- **The producer writes no drop file.** `written` in `state.json` does not change, or `live` is `true`. Read
  `state.json`. A value of `format` that is not `musicindex-liquidsoap-state/1` or `musicindex-liquidsoap-state/2`
  gives an `error` line in the log.
- **The producer stops with an ADR 0016 error.** A different producer holds `.producer.lock` in the drop directory.
  Stop the other producer, or give each producer its own instance.
- **A track with routes pays nobody.** The track has no feed GUID or no track GUID (ADR 0024). The publisher log has a
  warning that names the missing field. A file with only the old `MusicIndex` frames has no GUID for this slot text.
  Tag the file again with HitIt.
- **The two units stopped together, and the relay has no value.** The system journal shows `user@UID.service` with
  `Failed with result 'signal'`. Read `journalctl -u user@UID.service` as root. Without the step of §Restart The User
  Manager, start it as root with `systemctl start user@UID.service`. Then find what used the memory.
- **The publisher log names a second sender on the event.** A different publisher sends a payable block on the same
  event (ADR 0023). Stop one of the two.
