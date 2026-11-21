# ADR 0021: A Template Unit For The Mixxx Producer

- Status: Proposed
- Date: 2026-11-21
- Changes: ADR 0016 items: §Producer Units, the Mixxx producer, and the rejected alternative "A Template Unit For The
  Mixxx Producer".
- Bar: crosses repositories. HitIt starts, stops and switches these units, and it calls the setup helper with the
  flags of this record.
- Operator choice, 2026-11-21: HitIt gets full control of the publisher and producer units. It can make an instance,
  start it, stop it, switch the Mixxx producer, and show the state. The orchestrator session relayed the choice.
- Operator choice, 2026-11-21: the package owns the content of each unit file, as template units. HitIt writes no unit
  file. HitIt runs only `systemctl --user` and the setup helper.
- Operator choice, 2026-11-21: the private stream does not change. It is instance `mixxx`, target `default`, with a
  reserved event. Its unit names can change only with a migration that keeps its operation the same.

## Context

ADR 0016 §Producer Units keeps the Mixxx producer as one unit, `mixxx-now-playing.service`, paired with the instance
`mixxx`. ADR 0016 rejected a template unit, because Mixxx runs one time in a user session.

A show now needs its own event on the same Mixxx computer as the private stream. The show tracks must not go to the
private event, and the private tracks must not go to the show event.

The code at commit f4408e4 gives these facts:

- The publisher sends a drop file only to the target with the name in its `target` field
  (`src/watcher.rs`, `payload_for_upsert`). The file name has no effect.
- One producer process writes one target name (`mixxx-now-playing --target`).
- Each producer reads the same Mixxx history. Two Mixxx producers that run at the same time each write each track.
  Two instances thus do not keep the tracks apart. Only one Mixxx producer can run at a time.
- `.producer.lock` is a lock for one drop directory (ADR 0016 §The Producer Lock). It does not stop a second producer
  in a different drop directory.
- `setup-mixxx-musicindex` sets `instance=mixxx` and `target=default`, and it has no flag to change them. It writes
  the producer unit `mixxx-now-playing.service` with no instance part. A run for a second instance writes over the
  producer unit of the private stream.
- The setup helper writes the two units into `~/.config/systemd/user/`. A unit file there replaces the
  template of the package with the same name.

Terms:

- A **template unit** is a systemd unit file with `@` in its name, for example `name@.service`. systemd makes one
  unit from it for each instance name, for example `name@show.service`. The text `%i` in the file is the instance
  name. This is an established systemd term.
- A **Mixxx producer** is a `mixxx-now-playing` process in watch mode. This is a project term.
- The **Mixxx lock** is the lock of §One Mixxx Producer At A Time. This is a project term.

## Decision

### The Unit Names

| Unit | File in the package |
| --- | --- |
| `musicindex-live-publisher@<instance>.service` | `/usr/lib/systemd/user/musicindex-live-publisher@.service` |
| `mixxx-now-playing@<instance>.service` | `/usr/lib/systemd/user/mixxx-now-playing@.service` |

- The package installs the two template files. No program writes a unit file for an instance.
- The package removes `mixxx-now-playing.service`. The setup helper writes no unit file.
- The producer unit pairs with the publisher unit of the same instance, as ADR 0016 §Producer Units shows:
  `Wants=musicindex-live-publisher@%i.service` and `After=musicindex-live-publisher@%i.service`.
- An instance name matches `^[a-z0-9][a-z0-9-]{0,31}$`. The name then needs no systemd escape and no TOML escape.

### The Producer Reads Its Instance

The producer template has this command:

```ini
ExecStart=/usr/bin/mixxx-now-playing --format json --instance %i
```

`mixxx-now-playing --instance NAME` finds each path from the instance name:

| Value | Source |
| --- | --- |
| Instance configuration | `${XDG_CONFIG_HOME:-$HOME/.config}/musicindex-live-publisher/NAME/config.toml` |
| Drop directory | `$XDG_RUNTIME_DIR/musicindex-live-publisher/NAME/nowplaying` |
| Drop file | `<drop directory>/<target>.json` |
| Song file for `butt` | `<drop directory>/now-playing.txt` |
| Target name | The `name` of the one `[[target]]` in the instance configuration |
| Display directory | The `display_dir` of that target, or no display output |

These rules apply:

1. The producer reads the instance configuration. It does not write it. The publisher owns that file (ADR 0004).
2. The instance configuration of a Mixxx producer has exactly one target. With zero or more than one target, the
   producer exits with an error that names this record.
3. A `display_dir` must be `$XDG_RUNTIME_DIR/musicindex-live-publisher/NAME/display`. The template unit names that
   directory in `RuntimeDirectory=`, and `ProtectSystem=strict` permits writes only there. A different value stops
   the producer with an error that names this record.
4. `--instance` cannot be used with `--target`, `--id3-file`, `--txt-file` or `--display-dir`. The producer refuses
   the combination.
5. Without `--instance`, the producer keeps its current flags for a run by hand.

The instance configuration thus holds the target name and the display directory one time. The unit file holds no
value of an instance.

### One Mixxx Producer At A Time

Only one Mixxx producer runs at a time in a user session. The package enforces the rule with a kernel lock. A
`Conflicts=` line cannot do it, because systemd cannot name each instance of a template in it.

1. The Mixxx lock is an exclusive `flock` on `$XDG_RUNTIME_DIR/mixxx-now-playing/mixxx.lock`. The rules 1 to 4 of
   ADR 0016 §The Producer Lock also apply to it: a kernel lock on a local file system, no descriptor to a child
   process, and a wait of at most 2 seconds.
2. A Mixxx producer takes the Mixxx lock before `.producer.lock`. It holds the two locks for the life of the
   process.
3. After it gets the Mixxx lock, the producer writes its instance name into the lock file. When the lock is not
   free after 2 seconds, the producer reads that name. It then exits with exit status 3 and an error that names the
   instance that holds the lock and this record. Without `--instance`, the producer writes the name of its drop
   directory.
4. The producer template has `RestartPreventExitStatus=3`. A refused producer thus stops with the state `failed`.
   It does not restart and take the lock later without a command.
5. `--once` reads one time and writes no drop file. It takes no lock.
6. The template has `RuntimeDirectory=mixxx-now-playing` with `RuntimeDirectoryPreserve=yes`, so the lock directory
   stays when one instance stops.

### The State Of The Private Event During A Show

While the producer of a different instance runs, the producer of `mixxx` does not run. Its `.producer.lock` is free.
The publisher of `mixxx` continues to run, and it publishes the dead block of ADR 0005 to the private event. The
private event gets no track of the show.

### The Switch

A switch from instance A to instance B is two commands, in this order:

```bash
systemctl --user stop mixxx-now-playing@A.service
systemctl --user start mixxx-now-playing@B.service
```

- The start of the producer of B also starts the publisher of B (`Wants=`).
- A start of B while A runs fails after 2 seconds, with exit status 3. A continues to run with no change.
- HitIt stops the publisher of a show instance after the show. That publisher then stops the keepalive for the show
  event.

### Enabled Units

- The setup helper enables units only for the instance `mixxx`. The private stream thus starts at login as it does
  now.
- The setup helper never enables or starts a unit of a different instance. HitIt starts and stops those units.
- When two producer units are enabled, the first to start takes the Mixxx lock. The second stops with exit status 3.
  The lock thus stops the tracks of one stream from going to the other stream, also after an operator error.

### The Setup Helper

```text
setup-mixxx-musicindex [--instance NAME] [--target NAME] [--endpoint URL]
                       [--event-id ID --token-file PATH]
                       [--display] [--force] [--no-start] [--json]
setup-mixxx-musicindex --migrate-units [--force] [--no-start] [--json]
```

| Flag | Meaning |
| --- | --- |
| `--instance NAME` | The instance. Default: `mixxx`. |
| `--target NAME` | The name of the one target. Default: `default`. |
| `--endpoint URL` | The relay endpoint. Default: `https://api.musicindex.org`. |
| `--event-id ID --token-file PATH` | Use an event that already exists, with its token file. The helper sends no relay request. The token file must exist with mode `0600`. The helper does not copy or move it. |
| `--display` | Write `display_dir` into the configuration (ADR 0008). |
| `--force` | Replace a configuration that names a real event, as at commit f4408e4. |
| `--no-start` | Run no `systemctl` command. |
| `--json` | Write one JSON object on standard output as the last line. Write the text lines on standard error. |
| `--migrate-units` | Do the migration of §Migration. |

- Without `--event-id`, the helper provisions a new live item, as at commit f4408e4.
- The helper reads no input from the terminal. HitIt can call each form with no question to the user.
- For each instance, the helper writes the configuration
  `~/.config/musicindex-live-publisher/<instance>/config.toml` with mode `0600`, in a directory with mode `0700`.
  When it provisions, it also writes the token file `~/.config/musicindex-live-publisher/<instance>/tokens/<target>.token`.
  It writes no other file.
- The rules of the reserved safety tasks for the configuration stay: a configuration with a real event changes only
  with `--force`, and the helper makes a backup first.
- For `mixxx`, without `--no-start`, the helper runs `systemctl --user enable --now` for the two units.
- The JSON object has these keys: `instance`, `target`, `event_id`, `config_file`, `publisher_unit`, `producer_unit`,
  `started`. It holds no token.
- The helper removes `--permanent`, `--temporary` and `--units-only`. A template unit has one configuration path, so
  a configuration in `$XDG_RUNTIME_DIR` cannot work. `--migrate-units` replaces `--units-only`.

### The State

HitIt reads the state with these commands. This record adds no state command:

- `systemctl --user show` or `systemctl --user is-active` for each unit.
- `systemctl --user list-units 'mixxx-now-playing@*'` for the producer that runs.
- `musicindex-live-publisher config show --config <path> --json` for the targets of an instance (ADR 0004).

### Migration

`setup-mixxx-musicindex --migrate-units` moves the private stream from the units of the earlier helper to the
templates. It makes these steps:

1. It requires `~/.config/musicindex-live-publisher/mixxx/config.toml` with one target. It does not change that file.
2. It reads if `mixxx-now-playing.service` and `musicindex-live-publisher@mixxx.service` are enabled and active.
3. It refuses a unit file in `~/.config/systemd/user/` with no generated marker, unless `--force` is given.
4. It runs `systemctl --user disable --now mixxx-now-playing.service`, and it removes a link to that unit that has
   no target.
5. It moves `~/.config/systemd/user/mixxx-now-playing.service` and
   `~/.config/systemd/user/musicindex-live-publisher@mixxx.service` to a backup with a time stamp.
6. It runs `systemctl --user daemon-reload`.
7. It enables `musicindex-live-publisher@mixxx.service` and `mixxx-now-playing@mixxx.service` when the earlier units
   were enabled. It starts or restarts them when the earlier units were active.

After the migration, the private stream keeps its configuration, token, event and target. It also keeps its drop
directory, its drop file `default.json`, its song file, its display directory and its state directory. Only the unit
names and the unit files change.

A second run finds no earlier unit file and makes no change.

## Invariants

- At most one Mixxx producer runs in a user session. The Mixxx lock enforces it.
- The package owns each unit file. The setup helper and HitIt write no unit file.
- The instance configuration is the only source of the target name and the display directory of a Mixxx producer.
- The setup helper enables and starts units only for the instance `mixxx`.

## Open Questions

1. **A simulcast with one Mixxx producer.** ADR 0016 §One Instance For Each Show says that two targets in one
   instance send the same tracks to two events. The code sends a drop file to one target only, and a producer writes
   one target name. Rule 2 of §The Producer Reads Its Instance thus allows one target. A simulcast needs a
   different record.
2. **The removal of an instance.** This record gives no command to remove an instance. HitIt does not need it for a
   show.

## Alternatives Considered

### `Conflicts=` In The Unit

Rejected. `Conflicts=` names units one by one. A template cannot name each other instance.

### A Unit File Written By Hand Or By HitIt

Rejected by the operator choice. The package owns each unit file.

### An Environment File For Each Instance

Rejected. A file with the target name would be a second copy of a fact in the instance configuration. The two
copies can disagree.

### A Restart Loop For A Refused Producer

Rejected. A refused producer that restarts takes the lock when the other producer stops, with no command. The
stream would then change without an operator action.

## Consequences

- The producer gets `--instance` and the Mixxx lock.
- The package replaces `systemd/mixxx-now-playing.service` with `systemd/mixxx-now-playing@.service`, in the
  PKGBUILD and in `[package.metadata.deb]`.
- The setup helper gets the interface of §The Setup Helper and the migration.
- HitIt changes its producer unit name from `mixxx-now-playing.service` to `mixxx-now-playing@mixxx.service`.
- ADR 0016 gets a "Changed by" line for this record.

## References

- ADR 0004, ADR 0005, ADR 0008, ADR 0016 of this repository
- `scripts/setup-mixxx-musicindex.sh`, `mixxx-now-playing/src/lock.rs`, `src/watcher.rs`
- HitIt `docs/plans/live-rehearsal-goal-request.md`, item 2
- `~/build/musicindex-research/research_notes/live-rehearsal/README.md`, gap G8
