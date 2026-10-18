# ADR 0016: Instances, Events And The Producer Lock

Status: Proposed
Date: 2026-10-18

Class: durable. These are ownership and liveness rules.

## Context

ADR 0014 makes the publisher the only assembler. That rule holds only if each
event has one publisher and each publisher has one producer.

- The publisher runs as a template unit,
  `musicindex-live-publisher@<instance>.service`. Each instance has its own
  configuration, tokens and drop directory.
- The Mixxx producer is one unit, `mixxx-now-playing.service`. It pairs with
  the instance `mixxx`. Mixxx runs one time in a user session, with one
  library and one V4V card, so its producer also runs one time.
- The `event_id` of a target is in the configuration file of an instance.
  systemd cannot see it. Two instances can name the same event.
- The producer lock of ADR 0005 is a kernel lock on `.producer.lock`. The
  producer waits for it with no time limit (`mixxx-now-playing/src/lock.rs`,
  `ProducerLock::acquire`).

The operator asked if lock files leave stale locks after a crash. A test on
2026-10-18 gave these results:

- After `kill -9` of the holder, the file stays, but the kernel lock is free
  immediately.
- When the holder gives the lock descriptor to a child process, the child
  keeps the lock after the holder ends. This is the only condition in which
  the lock stays held after a crash.

The producer and the publisher start no child process. Rust opens
each file with `O_CLOEXEC`.

## Decision

### One Instance For Each Show

- One show with one audio source uses one instance.
- Two shows at the same time, with different audio, use two instances. A
  bad or revoked token stops its own instance only (mandate 7).
- Two or more targets in one instance send the same tracks to two or more
  events. That is a simulcast of one audio source.

### Producer Units

- **The Mixxx producer** stays one unit, `mixxx-now-playing.service`, paired
  with the instance `mixxx`.
- **A producer that can run more than one time** uses a template unit,
  `<producer>@<instance>.service`. It pairs with the publisher instance of the
  same name:

```ini
[Unit]
Wants=musicindex-live-publisher@%i.service
After=musicindex-live-publisher@%i.service
```

systemd runs one unit of each name, so each instance has one producer. Two
shows at the same time on one host can have one Mixxx show only. The other
show needs a different producer.

### One Instance For Each Event

An event belongs to one instance. Two instances with the same `event_id` are
two writers.

- `target add` refuses an `event_id` that the configuration of a different
  instance holds. It reads each
  `~/.config/musicindex-live-publisher/*/config.toml`.
- At startup, the publisher does the same check. If a different instance
  holds one of its events, the publisher exits with an error that names the
  two instances. This check also finds a configuration that an operator
  edited by hand.
- These checks read configuration files only. They keep no state, so a crash
  leaves nothing stale.
- The checks cover one host. On two hosts, only the custody of the token
  prevents a second writer.

### The Producer Lock

The producer lock of ADR 0005 stays. These rules apply to each producer:

1. The lock is a kernel lock (`flock`) on `.producer.lock`. It is never the
   existence of a file. The kernel releases it when the process ends, also
   after `SIGKILL`.
2. The lock file is in the drop directory of the instance, in
   `$XDG_RUNTIME_DIR`. That is a local file system. A kernel lock on a network
   file system is not reliable.
3. A producer never gives the lock descriptor to a child process. A child that
   holds the descriptor keeps the lock after the producer ends.
4. A producer waits at most 2 seconds for the lock. The publisher holds a
   shared lock for a shorter time during each health check. After 2 seconds,
   the producer exits with an error that names the drop directory. It does not
   wait without a limit.

### Deferred: A Drop File Bound To The Lock Holder

The lock is advisory. A program that writes a drop file without the lock is
published while a producer holds the lock. A binding can stop that:

- After it gets the lock, the producer writes a random session identifier
  into `.producer.lock`.
- Each drop file holds the same value in `producer_session`.
- The publisher publishes a drop file only when its `producer_session` is
  equal to the session of the lock holder.

This record does not decide the binding. No incident has occurred, and the
binding adds a drop-file field, a second fact that must agree, and tests.
Decide it when one of these incidents occurs:

- A program wrote a drop file without the lock, and the publisher published
  it.
- Two producers wrote to one drop directory.

## Invariants

- Each instance has one producer, and each event has one instance.
- The producer lock is a kernel lock on a local file system.
- No producer gives the lock descriptor to a child process.
- A producer waits at most 2 seconds for the lock.

## Open Questions

1. **A producer that hangs.** A hung producer keeps its lock, because it is
   alive. The publisher cannot tell it from a quiet producer. A systemd
   watchdog (`WatchdogSec` with `sd_notify`) can find a hang.

## Alternatives Considered

### A Lock File For Each Event

Rejected. The configuration check finds the same error earlier, at setup and
at startup, and it keeps no state.

### A Template Unit For The Mixxx Producer

Rejected. Mixxx runs one time in a user session, so a template suggests a
choice that does not exist.

## Consequences

- The publisher gets the two checks of §One Instance For Each Event.
- The producer gets a wait limit of 2 seconds for its lock.
- On acceptance, ADR 0005 gets a dated amendment for the wait limit.

## References

- ADR 0004, ADR 0005, ADR 0014 of this repository
- `systemd/musicindex-live-publisher@.service`,
  `systemd/mixxx-now-playing.service`
