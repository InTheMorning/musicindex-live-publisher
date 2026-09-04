//! Stream-delay scheduling for outgoing live value payloads.
//!
//! The publisher sees a track change the moment the producer writes the drop
//! file, but listeners hear that track several seconds later, after the
//! encoder, the icecast queue, and the player's own buffer. The icecast title
//! inherits that delay because it travels in band with the audio; a relay
//! payload travels out of band and does not. Publishing on sight therefore
//! flips the value block while listeners still hear the previous track, and a
//! boost in that window pays the wrong destination.
//!
//! This module holds each payload for its target's configured delay so the
//! block a listener boosts is the block they are hearing.

use std::collections::HashMap;
use std::time::{Duration, Instant};

use crate::LiveValuePayload;

/// Holds outgoing payloads until their target's stream delay has elapsed.
///
/// Delays are keyed by `event_guid`, the same key `RelayPublisher` routes on. A
/// payload for an unknown key is scheduled with no delay; the relay publisher
/// rejects it by name a moment later.
///
/// The clock is a parameter on every operation rather than read internally, so
/// callers can drive the queue from one `Instant` per loop iteration and tests
/// can drive it without sleeping.
#[derive(Debug)]
pub struct PublishSchedule {
    delays: HashMap<String, Duration>,
    pending: Vec<Scheduled>,
}

#[derive(Debug)]
struct Scheduled {
    due_at: Instant,
    queued_at: Instant,
    payload: LiveValuePayload,
}

impl PublishSchedule {
    /// Creates a schedule from a map of event GUID to stream delay.
    pub fn new(delays: HashMap<String, Duration>) -> Self {
        Self {
            delays,
            pending: Vec::new(),
        }
    }

    /// Returns true when no payload is waiting to be released.
    pub fn is_empty(&self) -> bool {
        self.pending.is_empty()
    }

    /// Queues one payload for release after its target's stream delay.
    ///
    /// A payload that repeats a pending payload's `(eventGuid, blockGuid)` pair
    /// replaces it in place and keeps the original deadline. That is the
    /// MusicIndex value-route upgrade, which rewrites one track inside its own
    /// block: it must publish once, and it must not push the block later.
    ///
    /// Anything else is appended. Two tracks changing inside one delay window
    /// are two blocks a listener will hear in sequence, so both are released,
    /// each at its own deadline.
    pub fn schedule(&mut self, payload: LiveValuePayload, now: Instant) {
        let delay = self.delay_for(&payload.event_guid);

        if let Some(pending) = self.pending.iter_mut().find(|pending| {
            pending.payload.event_guid == payload.event_guid
                && pending.payload.block_guid == payload.block_guid
        }) {
            tracing::debug!(
                event_id = %payload.event_guid,
                block_guid = %payload.block_guid,
                title = %payload.title,
                "replacing pending live value payload in the same block"
            );
            pending.payload = payload;
            return;
        }

        if !delay.is_zero() {
            tracing::debug!(
                event_id = %payload.event_guid,
                block_guid = %payload.block_guid,
                title = %payload.title,
                delay_ms = delay.as_millis(),
                "holding live value payload for stream delay"
            );
        }

        self.pending.push(Scheduled {
            due_at: now.checked_add(delay).unwrap_or(now),
            queued_at: now,
            payload,
        });
    }

    /// Removes and returns every payload whose deadline has been reached.
    ///
    /// Due payloads are extracted in the order they were scheduled, and the
    /// rest are retained. Scanning the whole queue rather than popping from the
    /// front is deliberate: a target with no delay must not wait behind a
    /// target with a long one.
    pub fn take_due(&mut self, now: Instant) -> Vec<LiveValuePayload> {
        let mut due = Vec::new();
        let mut waiting = Vec::with_capacity(self.pending.len());

        for entry in self.pending.drain(..) {
            if entry.due_at > now {
                waiting.push(entry);
                continue;
            }
            let held = now.saturating_duration_since(entry.queued_at);
            if !held.is_zero() {
                tracing::info!(
                    event_id = %entry.payload.event_guid,
                    block_guid = %entry.payload.block_guid,
                    title = %entry.payload.title,
                    held_ms = held.as_millis(),
                    "releasing live value payload after stream delay"
                );
            }
            due.push(entry.payload);
        }

        self.pending = waiting;
        due
    }

    /// Returns the earliest pending deadline, if anything is waiting.
    pub fn next_deadline(&self) -> Option<Instant> {
        self.pending.iter().map(|entry| entry.due_at).min()
    }

    fn delay_for(&self, event_guid: &str) -> Duration {
        self.delays
            .get(event_guid)
            .copied()
            .unwrap_or(Duration::ZERO)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{LiveValue, LiveValueModel};

    fn payload(event_guid: &str, block_guid: &str, title: &str) -> LiveValuePayload {
        LiveValuePayload {
            title: title.to_owned(),
            image: None,
            description: String::new(),
            kind: "music".to_owned(),
            start_time: 0,
            duration: None,
            event_guid: event_guid.to_owned(),
            block_guid: block_guid.to_owned(),
            feed_guid: None,
            item_guid: None,
            value: LiveValue {
                model: LiveValueModel {
                    kind: "lightning".to_owned(),
                    method: "keysend".to_owned(),
                    suggested: None,
                },
                destinations: Vec::new(),
            },
        }
    }

    fn schedule_with(event_guid: &str, delay: Duration) -> PublishSchedule {
        PublishSchedule::new(HashMap::from([(event_guid.to_owned(), delay)]))
    }

    #[test]
    fn schedule_unknown_event_guid_is_released_immediately() {
        let now = Instant::now();
        let mut schedule = PublishSchedule::new(HashMap::new());

        schedule.schedule(payload("unknown", "block", "Track"), now);

        assert_eq!(schedule.take_due(now).len(), 1);
        assert!(schedule.is_empty());
    }

    #[test]
    fn schedule_next_deadline_is_none_when_empty() {
        let mut schedule = schedule_with("event", Duration::from_secs(10));
        assert_eq!(schedule.next_deadline(), None);

        let now = Instant::now();
        schedule.schedule(payload("event", "block", "Track"), now);

        assert_eq!(
            schedule.next_deadline(),
            Some(now + Duration::from_secs(10))
        );
    }

    #[test]
    fn schedule_next_deadline_is_the_earliest_of_several() {
        let now = Instant::now();
        let mut schedule = PublishSchedule::new(HashMap::from([
            ("slow".to_owned(), Duration::from_secs(30)),
            ("fast".to_owned(), Duration::from_secs(5)),
        ]));

        schedule.schedule(payload("slow", "block-slow", "Slow"), now);
        schedule.schedule(payload("fast", "block-fast", "Fast"), now);

        assert_eq!(schedule.next_deadline(), Some(now + Duration::from_secs(5)));
    }
}
