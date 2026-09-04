use std::time::{Duration, Instant};

use anyhow::Result;

use crate::sink::{OutputFile, Presence};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Expiry {
    deadline: Option<Instant>,
}

impl Expiry {
    pub fn none() -> Self {
        Self { deadline: None }
    }

    pub fn duration(
        started_at: Instant,
        track_duration: Option<Duration>,
        slack: Duration,
        fallback: Duration,
    ) -> Self {
        let ttl = track_duration.map_or(fallback, |duration| duration.saturating_add(slack));
        Self {
            deadline: started_at.checked_add(ttl),
        }
    }

    pub fn deadline(&self) -> Option<Instant> {
        self.deadline
    }

    pub fn expired_at(&self, now: Instant) -> bool {
        self.deadline.is_some_and(|deadline| now >= deadline)
    }
}

pub fn expire_metadata_at(
    expiry: &mut Expiry,
    metadata: &mut OutputFile,
    now: Instant,
) -> Result<()> {
    if expiry.expired_at(now) {
        metadata.set(Presence::Absent)?;
        *expiry = Expiry::none();
    }
    Ok(())
}
