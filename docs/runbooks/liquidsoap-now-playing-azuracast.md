# Liquidsoap Producer On An AzuraCast Host

## Purpose

This runbook installs `liquidsoap-now-playing` and the publisher on a radio host with AzuraCast. One user account
with lingering owns the two units (ADR 0022). Lingering is a systemd function: the user services of the account
operate with no login.

The station side comes first. The tutorial in `musicindex-research`,
`research_notes/azuracast/musicindex-producer-tutorial.md`, Part 1 steps 1 to 7, gives it. The slot text of step 5 is
`liquidsoap/musicindex-state.liq` in this repository. The package installs it as
`/usr/share/doc/musicindex-live-publisher/liquidsoap/musicindex-state.liq`.

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
- The `event_id` and the broadcaster token file of the station event. Each event has one owner instance (ADR 0016).

## Make The Account

Do these steps as root:

```sh
useradd --create-home --shell /bin/bash musicindex
loginctl enable-linger musicindex
apt install ./musicindex-live-publisher_VERSION_amd64.deb
```

Replace `VERSION` with the version in the file name of the package.

On a host without `apt`, install the files of the package as root:

```sh
install -m 0755 musicindex-live-publisher liquidsoap-now-playing /usr/bin/
install -m 0644 musicindex-live-publisher@.service liquidsoap-now-playing@.service /usr/lib/systemd/user/
```

Do not make a folder in the home folder of the account as root. A folder that root owns, for example `~/.config`,
stops `systemctl --user enable`.

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

Replace `EVENT_ID` with the `event_id` of the station event. Examine the result:

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
  `state.json`. A value of `format` that is not `musicindex-liquidsoap-state/1` gives an `error` line in the log.
- **The producer stops with an ADR 0016 error.** A different producer holds `.producer.lock` in the drop directory.
  Stop the other producer, or give each producer its own instance.
- **A track with routes pays nobody.** The track has no feed GUID or no track GUID (ADR 0024). The publisher log has a
  warning that names the missing field.
- **The publisher log names a second sender on the event.** A different publisher sends a payable block on the same
  event (ADR 0023). Stop one of the two.
