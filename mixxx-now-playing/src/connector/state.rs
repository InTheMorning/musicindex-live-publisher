//! Deck state and connector mode (ADR 0006 §Protocol and §When The Connector
//! Is Not Available).
//!
//! `ConnectorState` applies the control changes from the Mixxx mapping. It
//! reports a deck change only when a value changes, because Mixxx sends
//! `play = 0` more than one time at a stop.

use std::time::{Duration, Instant};

use super::midi::ControlChange;

/// The protocol version that this producer knows.
pub const PROTOCOL_VERSION: u8 = 1;

/// The longest time with no heartbeat before the producer uses the
/// history-only mode.
pub const HEARTBEAT_TIMEOUT: Duration = Duration::from_secs(3);

/// The number of decks in the protocol.
pub const DECKS: u8 = 4;

/// The MIDI channel of the protocol, channel 16, as the low nibble of the
/// status byte.
const CHANNEL: u8 = 15;

const CC_HEARTBEAT: u8 = 1;
const CC_LOUDEST: u8 = 2;
const CC_PLAY: u8 = 10;
const CC_TRACK_LOADED: u8 = 20;
const CC_DURATION_HIGH: u8 = 30;
const CC_DURATION_LOW: u8 = 40;

/// A changed value of one deck.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DeckChange {
    /// The deck number, 1 to 4.
    pub deck: u8,
    /// The value that changed.
    pub kind: DeckChangeKind,
}

/// The value that changed on a deck.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeckChangeKind {
    /// The deck `play` control.
    Play(bool),
    /// The deck `track_loaded` control.
    TrackLoaded(bool),
    /// The deck `duration` control, in whole seconds.
    Duration(u16),
}

/// The mode of the producer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    /// The connector gives the deck state.
    Connector,
    /// The producer uses the history-only mode of ADR 0005.
    HistoryOnly(HistoryOnlyReason),
}

/// The reason for the history-only mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HistoryOnlyReason {
    /// The raw MIDI device is not open.
    NoDevice,
    /// No heartbeat arrived in the last 3 seconds.
    NoHeartbeat,
    /// The heartbeat gives a protocol version that this producer does not
    /// know.
    UnknownVersion,
}

#[derive(Debug, Clone, Copy, Default)]
struct DeckState {
    play: Option<bool>,
    track_loaded: Option<bool>,
    duration_high: u8,
    duration_secs: Option<u16>,
}

/// The deck state and the heartbeat from the connector mapping.
#[derive(Debug, Default)]
pub struct ConnectorState {
    decks: [DeckState; DECKS as usize],
    loudest: u8,
    heartbeat: Option<(Instant, u8)>,
}

impl ConnectorState {
    /// Makes an empty state. No deck plays, and the loudest deck is 0.
    pub fn new() -> Self {
        Self::default()
    }

    /// Applies one control change that arrived at `now`.
    ///
    /// Gives a `DeckChange` only when a deck value differs from its last
    /// value. A message on a different channel changes nothing.
    pub fn apply(&mut self, cc: ControlChange, now: Instant) -> Option<DeckChange> {
        if cc.channel != CHANNEL {
            return None;
        }
        match cc.controller {
            CC_HEARTBEAT => {
                self.heartbeat = Some((now, cc.value));
                None
            }
            CC_LOUDEST => {
                if cc.value <= DECKS {
                    self.loudest = cc.value;
                }
                None
            }
            controller => self.apply_deck(controller, cc.value),
        }
    }

    fn apply_deck(&mut self, controller: u8, value: u8) -> Option<DeckChange> {
        let base = controller - controller % 10;
        let deck = controller % 10;
        if !(1..=DECKS).contains(&deck) {
            return None;
        }
        let state = &mut self.decks[usize::from(deck - 1)];
        let kind = match base {
            CC_PLAY => {
                let playing = value >= 64;
                if state.play == Some(playing) {
                    return None;
                }
                state.play = Some(playing);
                DeckChangeKind::Play(playing)
            }
            CC_TRACK_LOADED => {
                let loaded = value >= 64;
                if state.track_loaded == Some(loaded) {
                    return None;
                }
                state.track_loaded = Some(loaded);
                DeckChangeKind::TrackLoaded(loaded)
            }
            CC_DURATION_HIGH => {
                state.duration_high = value & 0x7F;
                return None;
            }
            // The duration applies when its low part arrives.
            CC_DURATION_LOW => {
                let secs = (u16::from(state.duration_high) << 7) | u16::from(value & 0x7F);
                if state.duration_secs == Some(secs) {
                    return None;
                }
                state.duration_secs = Some(secs);
                DeckChangeKind::Duration(secs)
            }
            _ => return None,
        };
        Some(DeckChange { deck, kind })
    }

    /// Gives the mode at `now`.
    ///
    /// The connector mode needs an open device, a heartbeat that is not
    /// older than `HEARTBEAT_TIMEOUT`, and `PROTOCOL_VERSION`.
    pub fn mode(&self, now: Instant, device_open: bool) -> Mode {
        if !device_open {
            return Mode::HistoryOnly(HistoryOnlyReason::NoDevice);
        }
        match self.heartbeat {
            Some((at, _)) if now.saturating_duration_since(at) > HEARTBEAT_TIMEOUT => {
                Mode::HistoryOnly(HistoryOnlyReason::NoHeartbeat)
            }
            Some((_, PROTOCOL_VERSION)) => Mode::Connector,
            Some(_) => Mode::HistoryOnly(HistoryOnlyReason::UnknownVersion),
            None => Mode::HistoryOnly(HistoryOnlyReason::NoHeartbeat),
        }
    }

    /// Clears the deck state, the loudest deck and the heartbeat.
    ///
    /// Call it when the device closes. The next device gives the connector
    /// mode only after its first heartbeat.
    pub fn clear(&mut self) {
        *self = Self::default();
    }

    /// Gives the loudest deck that the mapping reported last, 1 to 4, or 0
    /// for none.
    pub fn loudest_deck(&self) -> u8 {
        self.loudest
    }

    /// Gives true when the deck plays. An unknown deck or value gives false.
    pub fn play(&self, deck: u8) -> bool {
        self.deck(deck).and_then(|d| d.play).unwrap_or(false)
    }

    /// Gives true when the deck has a track. An unknown deck or value gives
    /// false.
    pub fn track_loaded(&self, deck: u8) -> bool {
        self.deck(deck)
            .and_then(|d| d.track_loaded)
            .unwrap_or(false)
    }

    /// Gives the deck duration in whole seconds, or `None` when the mapping
    /// did not send it.
    pub fn duration_secs(&self, deck: u8) -> Option<u16> {
        self.deck(deck).and_then(|d| d.duration_secs)
    }

    fn deck(&self, deck: u8) -> Option<&DeckState> {
        deck.checked_sub(1)
            .and_then(|index| self.decks.get(usize::from(index)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cc(controller: u8, value: u8) -> ControlChange {
        ControlChange {
            channel: CHANNEL,
            controller,
            value,
        }
    }

    fn change(deck: u8, kind: DeckChangeKind) -> Option<DeckChange> {
        Some(DeckChange { deck, kind })
    }

    #[test]
    fn repeated_play_zero_gives_one_change() {
        let mut state = ConnectorState::new();
        let now = Instant::now();
        assert_eq!(
            state.apply(cc(11, 0), now),
            change(1, DeckChangeKind::Play(false))
        );
        assert_eq!(state.apply(cc(11, 0), now), None);
        assert_eq!(
            state.apply(cc(11, 127), now),
            change(1, DeckChangeKind::Play(true))
        );
    }

    #[test]
    fn values_of_64_or_more_are_true() {
        let mut state = ConnectorState::new();
        let now = Instant::now();
        state.apply(cc(11, 63), now);
        assert!(!state.play(1));
        state.apply(cc(11, 64), now);
        assert!(state.play(1));
    }

    #[test]
    fn track_loaded_change() {
        let mut state = ConnectorState::new();
        let now = Instant::now();
        assert_eq!(
            state.apply(cc(24, 127), now),
            change(4, DeckChangeKind::TrackLoaded(true))
        );
        assert!(state.track_loaded(4));
        assert_eq!(state.apply(cc(24, 127), now), None);
    }

    #[test]
    fn duration_applies_when_the_low_part_arrives() {
        let mut state = ConnectorState::new();
        let now = Instant::now();
        assert_eq!(state.apply(cc(31, 1), now), None);
        assert_eq!(state.duration_secs(1), None);
        assert_eq!(
            state.apply(cc(41, 72), now),
            change(1, DeckChangeKind::Duration(200))
        );
        assert_eq!(state.duration_secs(1), Some(200));
    }

    #[test]
    fn a_new_duration_with_the_same_low_part_is_a_change() {
        let mut state = ConnectorState::new();
        let now = Instant::now();
        state.apply(cc(31, 1), now);
        state.apply(cc(41, 72), now);
        // 328 seconds is 2 * 128 + 72.
        assert_eq!(state.apply(cc(31, 2), now), None);
        assert_eq!(
            state.apply(cc(41, 72), now),
            change(1, DeckChangeKind::Duration(328))
        );
    }

    #[test]
    fn the_same_duration_again_is_no_change() {
        let mut state = ConnectorState::new();
        let now = Instant::now();
        state.apply(cc(32, 1), now);
        state.apply(cc(42, 72), now);
        state.apply(cc(32, 1), now);
        assert_eq!(state.apply(cc(42, 72), now), None);
    }

    #[test]
    fn a_duration_high_part_alone_gives_no_change() {
        let mut state = ConnectorState::new();
        assert_eq!(state.apply(cc(31, 2), Instant::now()), None);
    }

    #[test]
    fn channel_1_messages_change_nothing() {
        let mut state = ConnectorState::new();
        let now = Instant::now();
        let message = ControlChange {
            channel: 0,
            controller: 11,
            value: 127,
        };
        assert_eq!(state.apply(message, now), None);
        assert!(!state.play(1));
    }

    #[test]
    fn deck_numbers_outside_1_to_4_change_nothing() {
        let mut state = ConnectorState::new();
        let now = Instant::now();
        assert_eq!(state.apply(cc(10, 127), now), None);
        assert_eq!(state.apply(cc(15, 127), now), None);
        assert_eq!(state.apply(cc(19, 127), now), None);
    }

    #[test]
    fn loudest_value_above_4_is_ignored() {
        let mut state = ConnectorState::new();
        let now = Instant::now();
        state.apply(cc(2, 3), now);
        assert_eq!(state.apply(cc(2, 5), now), None);
        assert_eq!(state.loudest_deck(), 3);
        state.apply(cc(2, 0), now);
        assert_eq!(state.loudest_deck(), 0);
    }

    #[test]
    fn mode_without_a_device_is_no_device() {
        let mut state = ConnectorState::new();
        let now = Instant::now();
        state.apply(cc(1, PROTOCOL_VERSION), now);
        assert_eq!(
            state.mode(now, false),
            Mode::HistoryOnly(HistoryOnlyReason::NoDevice)
        );
    }

    #[test]
    fn mode_without_a_heartbeat_is_no_heartbeat() {
        let state = ConnectorState::new();
        assert_eq!(
            state.mode(Instant::now(), true),
            Mode::HistoryOnly(HistoryOnlyReason::NoHeartbeat)
        );
    }

    #[test]
    fn mode_follows_the_heartbeat_age() {
        let mut state = ConnectorState::new();
        let start = Instant::now();
        state.apply(cc(1, PROTOCOL_VERSION), start);
        assert_eq!(
            state.mode(start + Duration::from_millis(2900), true),
            Mode::Connector
        );
        assert_eq!(state.mode(start + HEARTBEAT_TIMEOUT, true), Mode::Connector);
        assert_eq!(
            state.mode(start + Duration::from_millis(3100), true),
            Mode::HistoryOnly(HistoryOnlyReason::NoHeartbeat)
        );
    }

    #[test]
    fn mode_with_an_unknown_version_is_unknown_version() {
        let mut state = ConnectorState::new();
        let now = Instant::now();
        state.apply(cc(1, 2), now);
        assert_eq!(
            state.mode(now, true),
            Mode::HistoryOnly(HistoryOnlyReason::UnknownVersion)
        );
    }

    #[test]
    fn clear_resets_the_decks_the_loudest_deck_and_the_heartbeat() {
        let mut state = ConnectorState::new();
        let now = Instant::now();
        state.apply(cc(1, PROTOCOL_VERSION), now);
        state.apply(cc(11, 127), now);
        state.apply(cc(31, 1), now);
        state.apply(cc(41, 72), now);
        state.apply(cc(2, 1), now);

        state.clear();

        assert!(!state.play(1));
        assert_eq!(state.duration_secs(1), None);
        assert_eq!(state.loudest_deck(), 0);
        assert_eq!(
            state.mode(now, true),
            Mode::HistoryOnly(HistoryOnlyReason::NoHeartbeat)
        );
        // After a clear, the first play = 0 is a change again.
        assert_eq!(
            state.apply(cc(11, 0), now),
            change(1, DeckChangeKind::Play(false))
        );
    }
}
