//! The link rules of the connector mode (ADR 0006 §How `mixxx-now-playing`
//! Uses The Deck State, §When The Connector Is Not Available, §Entering And
//! Leaving The Connector Mode and §Relink After An Outage).
//!
//! `Coordinator` makes each decision about the drop file. It reads no clock,
//! no file and no device. The caller gives it the connector events, the
//! history rows and the times. It gives back the actions that the caller must
//! do.
//!
//! `Coordinator` also keeps the display link and the display state of ADR 0008
//! §The Display Link and §The Display State. The display link applies to each
//! history row. The payment link applies only to a V4V row. The two links use
//! the same rule code: `link_at_row`, `RowLink` and `relink_refusal`.

use std::time::{Duration, Instant};

use super::midi::ControlChange;
use super::state::{
    ConnectorEvent, ConnectorState, DECKS, DeckChange, DeckChangeKind, HistoryOnlyReason, Mode,
};
use crate::expiry::Expiry;

/// The longest time at startup before the producer knows the mode with no
/// heartbeat and no device result.
pub const STARTUP_TIMEOUT: Duration = Duration::from_secs(3);

/// An action that the caller must do for the `Coordinator`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    /// Write `STATE_REQUEST` to the connector device.
    SendStateRequest,
    /// Remove the drop file.
    RemoveFile,
    /// Write the drop file for the present row, with this `duration_secs`.
    WriteFile {
        /// The duration for `duration_secs`. `None` gives no duration.
        duration: Option<Duration>,
    },
}

/// A new history row, as the `Coordinator` needs it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Row {
    /// A V4V track with tags that the producer could read.
    V4v {
        /// The duration from the stream headers.
        header_duration: Option<Duration>,
        /// The ADR 0005 expiry, which starts at the time of the row.
        expiry: Expiry,
    },
    /// A track that is not V4V, or a V4V track with tags that the producer
    /// could not read.
    Other,
}

/// The artist and the title of a history row, as Mixxx gives them.
///
/// The `Coordinator` does not change them. The format rules of
/// `now-playing.txt` do not apply here (ADR 0008 §The Display State).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DisplayTrack {
    /// The artist of the history row.
    pub artist: String,
    /// The title of the history row.
    pub title: String,
}

/// The display state of ADR 0008 §The Display State.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub enum DisplayState {
    /// No track shows. This state applies at startup, while no display link
    /// exists, and while the linked deck does not play.
    #[default]
    Null,
    /// The track of the present history row shows.
    Track {
        /// The artist of the history row.
        artist: String,
        /// The title of the history row.
        title: String,
        /// True when the row is `Row::V4v`.
        v4v: bool,
    },
}

/// The mode that the `Coordinator` uses now.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum KnownMode {
    /// The producer started and does not know the mode yet.
    Unknown,
    Connector,
    HistoryOnly,
}

/// The deck of a link and its sample count at the time of the link.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Link {
    deck: u8,
    samples: Option<u64>,
}

/// The cause when a new history row gets no link in the connector mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum NoLink {
    /// The row existed before the entry into the connector mode.
    BeforeEntry,
    /// The loudest deck is 0, or that deck does not play.
    NoPlayingLoudestDeck,
}

/// The result of a deck change on the linked deck (ADR 0006 §How
/// `mixxx-now-playing` Uses The Deck State).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LinkEffect {
    /// The linked deck stopped. The link stays.
    Stop,
    /// The linked deck plays again.
    Resume,
    /// A new load on the linked deck. The link ended.
    End,
}

/// The link of one row and its relink candidate. The payment link and the
/// display link use this type, so their rules cannot become different.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
struct RowLink {
    link: Option<Link>,
    /// The link from before an outage of the connector. The first state end
    /// after the next entry tests it (ADR 0006 §Relink After An Outage).
    relink: Option<Link>,
}

impl RowLink {
    /// The link of a new row. A new row has no relink candidate.
    fn at_row(link: Option<Link>) -> Self {
        Self { link, relink: None }
    }

    /// Ends the link at an entry into the connector mode. The relink
    /// candidate stays for the first state end after the entry.
    fn enter_connector(&mut self) {
        self.link = None;
    }

    /// Keeps the link as the relink candidate at an exit from the connector
    /// mode. A candidate from an earlier outage that did not relink goes
    /// away.
    fn leave_connector(&mut self) {
        self.relink = self.link.take();
    }

    /// Applies a deck change. Gives `None` when the change is not on the
    /// linked deck. A new load ends the link.
    fn deck_change(&mut self, change: DeckChange) -> Option<LinkEffect> {
        if self.link.map(|link| link.deck) != Some(change.deck) {
            return None;
        }
        let effect = match change.kind {
            DeckChangeKind::Play(false) => LinkEffect::Stop,
            DeckChangeKind::Play(true) => LinkEffect::Resume,
            DeckChangeKind::TrackLoaded(_)
            | DeckChangeKind::Duration(_)
            | DeckChangeKind::Samples(_) => LinkEffect::End,
        };
        if effect == LinkEffect::End {
            self.link = None;
        }
        Some(effect)
    }

    /// Gives the relink candidate. The candidate goes away after this call.
    fn take_candidate(&mut self) -> Option<Link> {
        self.relink.take()
    }
}

#[derive(Debug, Clone, Copy)]
struct CurrentRow {
    expiry: Expiry,
    /// True when the expiry ended and the file was removed for it.
    expired: bool,
    links: RowLink,
    file_duration: Option<Duration>,
}

/// The present history row for the display state, V4V or not.
#[derive(Debug, Clone)]
struct DisplayRow {
    track: DisplayTrack,
    v4v: bool,
    links: RowLink,
    /// True when the track shows.
    shown: bool,
}

/// Makes each decision about the drop file and the display state of the
/// producer.
#[derive(Debug)]
pub struct Coordinator {
    enabled: bool,
    started: Instant,
    state: ConnectorState,
    device_open: bool,
    heartbeat_seen: bool,
    unavailable_seen: bool,
    mode: KnownMode,
    pending: Vec<ConnectorEvent>,
    row: Option<CurrentRow>,
    present: bool,
    skip_next_row: bool,
    display_row: Option<DisplayRow>,
    /// The display state that `take_display_change` gave last.
    display_given: DisplayState,
}

impl Coordinator {
    /// Makes a `Coordinator` that started at `started`.
    ///
    /// With `connector` false, the producer always uses the history-only
    /// mode and has no startup gate.
    pub fn new(connector: bool, started: Instant) -> Self {
        Self {
            enabled: connector,
            started,
            state: ConnectorState::new(),
            device_open: false,
            heartbeat_seen: false,
            unavailable_seen: false,
            mode: if connector {
                KnownMode::Unknown
            } else {
                KnownMode::HistoryOnly
            },
            pending: Vec::new(),
            row: None,
            present: false,
            skip_next_row: false,
            display_row: None,
            display_given: DisplayState::Null,
        }
    }

    /// Gives true when the connector is on.
    pub fn connector_enabled(&self) -> bool {
        self.enabled
    }

    /// Records that the reader opened the device.
    pub fn device_opened(&mut self) {
        self.device_open = true;
    }

    /// Records that the device closed. The deck state is cleared, and the
    /// mode changes at the next `update`.
    pub fn device_closed(&mut self) {
        self.device_open = false;
        self.state.clear();
        self.pending.clear();
    }

    /// Records that the reader could not find or open the device.
    pub fn device_unavailable(&mut self) {
        self.device_open = false;
        self.unavailable_seen = true;
        self.state.clear();
        self.pending.clear();
    }

    /// Applies one control change that arrived at `time`.
    ///
    /// A deck change or a state end waits for the next `update`.
    pub fn control_change(&mut self, cc: ControlChange, time: Instant) {
        if let Some(event) = self.state.apply(cc, time) {
            self.pending.push(event);
        }
        // Only a heartbeat can give a mode other than `NoHeartbeat` at the
        // time of the message.
        if self.state.mode(time, true) != Mode::HistoryOnly(HistoryOnlyReason::NoHeartbeat) {
            self.heartbeat_seen = true;
        }
    }

    /// Gives true when the history poll can run. It is false until the mode
    /// is known.
    pub fn history_allowed(&self) -> bool {
        self.mode != KnownMode::Unknown
    }

    /// Gives true when the drop file is present.
    pub fn file_present(&self) -> bool {
        self.present
    }

    /// Gives the present display state (ADR 0008 §The Display State).
    pub fn display_state(&self) -> DisplayState {
        match &self.display_row {
            Some(display) if display.shown => DisplayState::Track {
                artist: display.track.artist.clone(),
                title: display.track.title.clone(),
                v4v: display.v4v,
            },
            _ => DisplayState::Null,
        }
    }

    /// Gives the display state when it is different from the state that
    /// this function gave last, or `None`. The first state is `Null`.
    pub fn take_display_change(&mut self) -> Option<DisplayState> {
        let state = self.display_state();
        if state == self.display_given {
            return None;
        }
        self.display_given = state.clone();
        Some(state)
    }

    /// Applies the mode change, the deck changes and the expiry at `now`,
    /// in this order.
    pub fn update(&mut self, now: Instant) -> Vec<Action> {
        let mut actions = Vec::new();
        self.apply_mode(now, &mut actions);
        self.apply_deck_changes(&mut actions);
        self.apply_expiry(now, &mut actions);
        actions
    }

    /// Applies a new history row and its track. It sets the display link of
    /// the row, then it applies the payment link of the row.
    pub fn history_row(&mut self, row: Row, track: DisplayTrack) -> Vec<Action> {
        // `row` takes `skip_next_row`, so the display link reads it first.
        let link = match self.mode {
            KnownMode::Connector => self.link_at_row(self.skip_next_row).ok(),
            KnownMode::HistoryOnly | KnownMode::Unknown => None,
        };
        // In the history-only mode, the track shows until the next row.
        let shown = self.mode == KnownMode::HistoryOnly || link.is_some();
        self.display_row = Some(DisplayRow {
            track,
            v4v: matches!(row, Row::V4v { .. }),
            links: RowLink::at_row(link),
            shown,
        });
        self.row(row)
    }

    /// Applies the payment link of a new history row.
    fn row(&mut self, row: Row) -> Vec<Action> {
        let skip = std::mem::take(&mut self.skip_next_row);
        let Row::V4v {
            header_duration,
            expiry,
        } = row
        else {
            self.row = None;
            self.present = false;
            return vec![Action::RemoveFile];
        };

        let (link, file_duration) = match self.mode {
            KnownMode::HistoryOnly => (None, header_duration),
            KnownMode::Connector => match self.link_at_row(skip) {
                Ok(link) => (Some(link), self.deck_duration(link.deck)),
                Err(NoLink::BeforeEntry) => {
                    tracing::info!(
                        loudest = self.state.loudest_deck(),
                        "history row existed before the connector mode; no link"
                    );
                    (None, None)
                }
                Err(NoLink::NoPlayingLoudestDeck) => {
                    let loudest = self.state.loudest_deck();
                    tracing::info!(loudest, "no playing loudest deck; history row has no link");
                    (None, None)
                }
            },
            KnownMode::Unknown => (None, None),
        };

        let write = self.mode == KnownMode::HistoryOnly || link.is_some();
        self.row = Some(CurrentRow {
            expiry,
            expired: false,
            links: RowLink::at_row(link),
            file_duration,
        });
        self.present = write;
        if write {
            vec![Action::WriteFile {
                duration: file_duration,
            }]
        } else {
            vec![Action::RemoveFile]
        }
    }

    /// Gives the link of a new history row in the connector mode: the
    /// loudest deck, when it plays. With `skip` true, the row existed before
    /// the entry and gets no link.
    fn link_at_row(&self, skip: bool) -> Result<Link, NoLink> {
        if skip {
            return Err(NoLink::BeforeEntry);
        }
        let loudest = self.state.loudest_deck();
        if (1..=DECKS).contains(&loudest) && self.state.play(loudest) {
            Ok(Link {
                deck: loudest,
                samples: self.state.samples(loudest),
            })
        } else {
            Err(NoLink::NoPlayingLoudestDeck)
        }
    }

    /// Gives the write for a MusicIndex API result, or `None`.
    ///
    /// A result changes the drop file only while the file is present. It
    /// never writes a file again that a stop removed.
    pub fn api_result(&self) -> Option<Action> {
        match self.row {
            Some(row) if self.present => Some(Action::WriteFile {
                duration: row.file_duration,
            }),
            _ => None,
        }
    }

    fn apply_mode(&mut self, now: Instant, actions: &mut Vec<Action>) {
        if !self.enabled {
            return;
        }
        let known = self.heartbeat_seen
            || self.unavailable_seen
            || now.saturating_duration_since(self.started) >= STARTUP_TIMEOUT;
        if !known {
            return;
        }
        match self.state.mode(now, self.device_open) {
            Mode::Connector if self.mode != KnownMode::Connector => {
                tracing::info!("connector mode");
                // At startup the history was not read. Its latest row existed
                // before the entry, so it must not link.
                self.skip_next_row = self.mode == KnownMode::Unknown;
                self.mode = KnownMode::Connector;
                // Keep the row and its relink candidate. The first state end
                // after this entry tests the candidate.
                if let Some(row) = self.row.as_mut() {
                    row.links.enter_connector();
                }
                if let Some(display) = self.display_row.as_mut() {
                    display.links.enter_connector();
                    display.shown = false;
                }
                self.present = false;
                self.pending.clear();
                actions.push(Action::SendStateRequest);
                actions.push(Action::RemoveFile);
            }
            Mode::HistoryOnly(reason) if self.mode != KnownMode::HistoryOnly => {
                tracing::warn!(reason = ?reason, "history-only mode");
                self.mode = KnownMode::HistoryOnly;
                // No write here. The expiry of the row applies from the time
                // of the row, in `apply_expiry`. Only a linked row keeps a
                // relink candidate. The display state does not change: the
                // connector mode removed what does not show.
                if let Some(row) = self.row.as_mut() {
                    row.links.leave_connector();
                }
                if let Some(display) = self.display_row.as_mut() {
                    display.links.leave_connector();
                }
                self.pending.clear();
            }
            _ => {}
        }
    }

    fn apply_deck_changes(&mut self, actions: &mut Vec<Action>) {
        let events = std::mem::take(&mut self.pending);
        if self.mode != KnownMode::Connector {
            return;
        }
        for event in events {
            match event {
                ConnectorEvent::Deck(change) => {
                    self.apply_display_deck_change(change);
                    self.apply_deck_change(change, actions);
                }
                ConnectorEvent::StateEnd => {
                    self.apply_display_state_end();
                    self.apply_state_end(actions);
                }
            }
        }
    }

    fn apply_deck_change(&mut self, change: DeckChange, actions: &mut Vec<Action>) {
        let Some(row) = self.row.as_mut() else {
            return;
        };
        match row.links.deck_change(change) {
            None => {}
            Some(LinkEffect::Stop | LinkEffect::End) => {
                self.present = false;
                actions.push(Action::RemoveFile);
            }
            Some(LinkEffect::Resume) => {
                if !self.present {
                    self.present = true;
                    actions.push(Action::WriteFile {
                        duration: row.file_duration,
                    });
                }
            }
        }
    }

    fn apply_display_deck_change(&mut self, change: DeckChange) {
        let Some(display) = self.display_row.as_mut() else {
            return;
        };
        match display.links.deck_change(change) {
            None => {}
            Some(LinkEffect::Stop | LinkEffect::End) => display.shown = false,
            Some(LinkEffect::Resume) => display.shown = true,
        }
    }

    /// Tests the relink candidate at the first state end after an entry into
    /// the connector mode (ADR 0006 §Relink After An Outage). The candidate
    /// goes away after this test.
    fn apply_state_end(&mut self, actions: &mut Vec<Action>) {
        let Some(candidate) = self.row.as_mut().and_then(|row| row.links.take_candidate()) else {
            return;
        };
        if let Some(reason) = self.relink_refusal(candidate) {
            tracing::info!(
                deck = candidate.deck,
                loudest = self.state.loudest_deck(),
                reason,
                "history row does not link again"
            );
            return;
        }

        let duration = self.deck_duration(candidate.deck);
        let Some(row) = self.row.as_mut() else {
            return;
        };
        row.links.link = Some(candidate);
        row.file_duration = duration;
        // The ADR 0005 expiry applies again at the next outage. If it already
        // ended, that outage removes the file at once.
        row.expired = false;
        self.present = true;
        tracing::info!(
            deck = candidate.deck,
            relink = true,
            "history row linked again after an outage"
        );
        actions.push(Action::WriteFile { duration });
    }

    /// Tests the display relink candidate by the same conditions as the
    /// payment candidate (ADR 0008 §The Display Link).
    fn apply_display_state_end(&mut self) {
        let Some(candidate) = self
            .display_row
            .as_mut()
            .and_then(|display| display.links.take_candidate())
        else {
            return;
        };
        if self.relink_refusal(candidate).is_some() {
            return;
        }
        if let Some(display) = self.display_row.as_mut() {
            display.links.link = Some(candidate);
            display.shown = true;
        }
    }

    /// Gives the cause that stops the candidate link, or `None`
    /// when each condition of ADR 0006 §Relink After An Outage is true.
    fn relink_refusal(&self, candidate: Link) -> Option<&'static str> {
        let deck = candidate.deck;
        let samples = self.state.samples(deck);
        if self.state.loudest_deck() != deck {
            Some("the deck is not the loudest deck")
        } else if !self.state.play(deck) {
            Some("the deck does not play")
        } else if samples != candidate.samples {
            Some("the deck sample count changed")
        } else if samples.unwrap_or(0) == 0 {
            Some("the deck sample count is unknown")
        } else {
            None
        }
    }

    fn deck_duration(&self, deck: u8) -> Option<Duration> {
        self.state
            .duration_secs(deck)
            .map(|secs| Duration::from_secs(u64::from(secs)))
    }

    fn apply_expiry(&mut self, now: Instant, actions: &mut Vec<Action>) {
        if self.mode != KnownMode::HistoryOnly {
            return;
        }
        let Some(row) = self.row.as_mut() else {
            return;
        };
        if !row.expired && row.expiry.expired_at(now) {
            row.expired = true;
            self.present = false;
            actions.push(Action::RemoveFile);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::state::PROTOCOL_VERSION;
    use super::*;

    const CHANNEL_16: u8 = 15;
    const MAX: Duration = Duration::from_secs(600);
    /// The deck 2 sample count in the tests. It is above 2^32.
    const SAMPLES: u64 = (1 << 32) + 5;

    fn cc(controller: u8, value: u8) -> ControlChange {
        ControlChange {
            channel: CHANNEL_16,
            controller,
            value,
        }
    }

    fn at(start: Instant, millis: u64) -> Instant {
        start + Duration::from_millis(millis)
    }

    fn heartbeat(coordinator: &mut Coordinator, time: Instant) {
        coordinator.control_change(cc(1, PROTOCOL_VERSION), time);
    }

    fn set_loudest(coordinator: &mut Coordinator, deck: u8, time: Instant) {
        coordinator.control_change(cc(2, deck), time);
    }

    fn set_play(coordinator: &mut Coordinator, deck: u8, play: bool, time: Instant) {
        coordinator.control_change(cc(10 + deck, if play { 127 } else { 0 }), time);
    }

    fn set_duration(coordinator: &mut Coordinator, deck: u8, secs: u16, time: Instant) {
        let high = u8::try_from(secs >> 7).expect("duration high part");
        let low = u8::try_from(secs & 0x7F).expect("duration low part");
        coordinator.control_change(cc(30 + deck, high), time);
        coordinator.control_change(cc(40 + deck, low), time);
    }

    fn set_samples(coordinator: &mut Coordinator, deck: u8, samples: u64, time: Instant) {
        for (index, base) in [50, 60, 70, 80, 90].into_iter().enumerate() {
            let shift = 7 * (4 - index);
            let part = u8::try_from((samples >> shift) & 0x7F).expect("7-bit part");
            coordinator.control_change(cc(base + deck, part), time);
        }
    }

    fn state_end(coordinator: &mut Coordinator, time: Instant) {
        coordinator.control_change(cc(3, 1), time);
    }

    fn v4v_row(time: Instant, header_secs: u64) -> Row {
        let header = Duration::from_secs(header_secs);
        Row::V4v {
            header_duration: Some(header),
            expiry: Expiry::duration(time, Some(header), Duration::ZERO, MAX),
        }
    }

    fn write(secs: u64) -> Action {
        Action::WriteFile {
            duration: Some(Duration::from_secs(secs)),
        }
    }

    fn has_write(actions: &[Action]) -> bool {
        actions
            .iter()
            .any(|action| matches!(action, Action::WriteFile { .. }))
    }

    /// A coordinator in the history-only mode after `DeviceUnavailable`.
    fn history_only(start: Instant) -> Coordinator {
        let mut coordinator = Coordinator::new(true, start);
        coordinator.device_unavailable();
        coordinator.update(start);
        assert!(coordinator.history_allowed());
        coordinator
    }

    /// A coordinator that entered the connector mode from the history-only
    /// mode, so that the next row can link.
    fn connector(start: Instant) -> Coordinator {
        let mut coordinator = history_only(start);
        coordinator.device_opened();
        heartbeat(&mut coordinator, start);
        let actions = coordinator.update(start);
        assert_eq!(actions, vec![Action::SendStateRequest, Action::RemoveFile]);
        coordinator
    }

    /// A connector coordinator with deck 2 loudest, playing, 200 seconds and
    /// `SAMPLES`.
    fn deck_2_plays(start: Instant) -> Coordinator {
        let mut coordinator = connector(start);
        set_duration(&mut coordinator, 2, 200, start);
        set_samples(&mut coordinator, 2, SAMPLES, start);
        set_play(&mut coordinator, 2, true, start);
        set_loudest(&mut coordinator, 2, start);
        assert_eq!(coordinator.update(start), vec![]);
        coordinator
    }

    #[test]
    fn startup_gate_blocks_history_before_the_mode_is_known() {
        let start = Instant::now();
        let mut coordinator = Coordinator::new(true, start);
        assert!(!coordinator.history_allowed());
        coordinator.device_opened();
        assert_eq!(coordinator.update(at(start, 2900)), vec![]);
        assert!(!coordinator.history_allowed());
    }

    #[test]
    fn startup_gate_opens_at_the_first_heartbeat() {
        let start = Instant::now();
        let mut coordinator = Coordinator::new(true, start);
        coordinator.device_opened();
        heartbeat(&mut coordinator, at(start, 500));
        let actions = coordinator.update(at(start, 500));
        assert!(coordinator.history_allowed());
        assert_eq!(actions, vec![Action::SendStateRequest, Action::RemoveFile]);
    }

    #[test]
    fn startup_gate_opens_at_device_unavailable() {
        let start = Instant::now();
        let mut coordinator = Coordinator::new(true, start);
        coordinator.device_unavailable();
        assert_eq!(coordinator.update(at(start, 100)), vec![]);
        assert!(coordinator.history_allowed());
    }

    #[test]
    fn startup_gate_opens_after_3_seconds() {
        let start = Instant::now();
        let mut coordinator = Coordinator::new(true, start);
        coordinator.device_opened();
        coordinator.update(at(start, 2999));
        assert!(!coordinator.history_allowed());
        coordinator.update(start + STARTUP_TIMEOUT);
        assert!(coordinator.history_allowed());
    }

    #[test]
    fn startup_row_before_the_entry_does_not_link_and_the_next_row_links() {
        let start = Instant::now();
        let mut coordinator = Coordinator::new(true, start);
        coordinator.device_opened();
        heartbeat(&mut coordinator, start);
        set_duration(&mut coordinator, 2, 200, start);
        set_play(&mut coordinator, 2, true, start);
        set_loudest(&mut coordinator, 2, start);
        coordinator.update(start);
        assert!(coordinator.history_allowed());

        // The first row after startup existed before the entry.
        assert_eq!(
            coordinator.row(v4v_row(start, 617)),
            vec![Action::RemoveFile]
        );
        assert_eq!(coordinator.row(v4v_row(start, 617)), vec![write(200)]);
    }

    #[test]
    fn row_with_playing_loudest_deck_2_writes_with_the_deck_2_duration() {
        let start = Instant::now();
        let mut coordinator = deck_2_plays(start);
        assert_eq!(coordinator.row(v4v_row(start, 200)), vec![write(200)]);
        assert!(coordinator.file_present());
    }

    #[test]
    fn header_duration_617_and_deck_duration_200_gives_200() {
        let start = Instant::now();
        let mut coordinator = deck_2_plays(start);
        assert_eq!(coordinator.row(v4v_row(start, 617)), vec![write(200)]);
    }

    #[test]
    fn deck_with_no_known_duration_gives_no_duration() {
        let start = Instant::now();
        let mut coordinator = connector(start);
        set_play(&mut coordinator, 3, true, start);
        set_loudest(&mut coordinator, 3, start);
        coordinator.update(start);
        assert_eq!(
            coordinator.row(v4v_row(start, 617)),
            vec![Action::WriteFile { duration: None }]
        );
    }

    #[test]
    fn row_with_loudest_deck_0_gives_no_write() {
        let start = Instant::now();
        let mut coordinator = deck_2_plays(start);
        set_loudest(&mut coordinator, 0, start);
        coordinator.update(start);
        assert_eq!(
            coordinator.row(v4v_row(start, 200)),
            vec![Action::RemoveFile]
        );
        assert!(!coordinator.file_present());
    }

    #[test]
    fn row_with_loudest_deck_2_that_does_not_play_gives_no_write() {
        let start = Instant::now();
        let mut coordinator = connector(start);
        set_duration(&mut coordinator, 2, 200, start);
        set_play(&mut coordinator, 2, false, start);
        set_loudest(&mut coordinator, 2, start);
        coordinator.update(start);
        assert_eq!(
            coordinator.row(v4v_row(start, 200)),
            vec![Action::RemoveFile]
        );
    }

    #[test]
    fn linked_deck_stop_removes_and_start_writes() {
        let start = Instant::now();
        let mut coordinator = deck_2_plays(start);
        coordinator.row(v4v_row(start, 200));

        set_play(&mut coordinator, 2, false, start);
        assert_eq!(coordinator.update(start), vec![Action::RemoveFile]);
        assert!(!coordinator.file_present());

        set_play(&mut coordinator, 2, true, start);
        assert_eq!(coordinator.update(start), vec![write(200)]);
        assert!(coordinator.file_present());
    }

    #[test]
    fn track_loaded_on_the_linked_deck_ends_the_link() {
        let start = Instant::now();
        let mut coordinator = deck_2_plays(start);
        coordinator.row(v4v_row(start, 200));

        set_play(&mut coordinator, 2, false, start);
        coordinator.control_change(cc(22, 127), start);
        assert_eq!(
            coordinator.update(start),
            vec![Action::RemoveFile, Action::RemoveFile]
        );

        set_play(&mut coordinator, 2, true, start);
        assert_eq!(coordinator.update(start), vec![]);
        assert!(!coordinator.file_present());
    }

    #[test]
    fn duration_change_on_the_linked_deck_ends_the_link() {
        let start = Instant::now();
        let mut coordinator = deck_2_plays(start);
        coordinator.row(v4v_row(start, 200));

        set_duration(&mut coordinator, 2, 328, start);
        assert_eq!(coordinator.update(start), vec![Action::RemoveFile]);

        set_play(&mut coordinator, 2, false, start);
        set_play(&mut coordinator, 2, true, start);
        assert_eq!(coordinator.update(start), vec![]);
    }

    #[test]
    fn deck_change_on_a_deck_that_is_not_linked_gives_no_action() {
        let start = Instant::now();
        let mut coordinator = deck_2_plays(start);
        coordinator.row(v4v_row(start, 200));

        set_play(&mut coordinator, 1, true, start);
        set_play(&mut coordinator, 1, false, start);
        coordinator.control_change(cc(21, 127), start);
        set_duration(&mut coordinator, 1, 300, start);
        set_samples(&mut coordinator, 1, 1000, start);
        assert_eq!(coordinator.update(start), vec![]);
        assert!(coordinator.file_present());
    }

    #[test]
    fn non_v4v_row_ends_the_link() {
        let start = Instant::now();
        let mut coordinator = deck_2_plays(start);
        coordinator.row(v4v_row(start, 200));
        assert_eq!(coordinator.row(Row::Other), vec![Action::RemoveFile]);

        set_play(&mut coordinator, 2, false, start);
        set_play(&mut coordinator, 2, true, start);
        assert_eq!(coordinator.update(start), vec![]);
        assert_eq!(coordinator.api_result(), None);
    }

    #[test]
    fn entering_the_connector_mode_removes_and_the_prior_row_never_links() {
        let start = Instant::now();
        let mut coordinator = history_only(start);
        assert_eq!(coordinator.row(v4v_row(start, 200)), vec![write(200)]);
        assert!(coordinator.file_present());

        coordinator.device_opened();
        heartbeat(&mut coordinator, at(start, 1000));
        assert_eq!(
            coordinator.update(at(start, 1000)),
            vec![Action::SendStateRequest, Action::RemoveFile]
        );
        assert!(!coordinator.file_present());

        // The complete state arrives after the request.
        set_duration(&mut coordinator, 2, 200, at(start, 1100));
        set_play(&mut coordinator, 2, true, at(start, 1100));
        set_loudest(&mut coordinator, 2, at(start, 1100));
        assert_eq!(coordinator.update(at(start, 1100)), vec![]);
        assert_eq!(coordinator.api_result(), None);
        assert!(!coordinator.file_present());
    }

    #[test]
    fn leaving_the_connector_mode_after_a_stop_gives_no_write() {
        let start = Instant::now();
        let mut coordinator = deck_2_plays(start);
        coordinator.row(v4v_row(start, 200));
        set_play(&mut coordinator, 2, false, at(start, 1000));
        assert_eq!(
            coordinator.update(at(start, 1000)),
            vec![Action::RemoveFile]
        );

        // The heartbeat stops. The device stays open.
        set_play(&mut coordinator, 2, true, at(start, 3500));
        let actions = coordinator.update(at(start, 3500));
        assert!(!has_write(&actions), "{actions:?}");
        assert!(!coordinator.file_present());
        assert_eq!(coordinator.api_result(), None);
        assert!(!has_write(&coordinator.update(at(start, 4000))));
    }

    #[test]
    fn leaving_the_connector_mode_after_the_expiry_removes_at_once() {
        let start = Instant::now();
        let mut coordinator = deck_2_plays(start);
        coordinator.row(v4v_row(start, 10));
        for second in 1..=12 {
            heartbeat(&mut coordinator, at(start, second * 1000));
            assert_eq!(coordinator.update(at(start, second * 1000)), vec![]);
        }

        // No heartbeat after 12 seconds.
        assert_eq!(
            coordinator.update(at(start, 15_500)),
            vec![Action::RemoveFile]
        );
        assert!(!coordinator.file_present());
    }

    #[test]
    fn leaving_the_connector_mode_keeps_the_expiry_from_the_row_time() {
        let start = Instant::now();
        let mut coordinator = deck_2_plays(start);
        coordinator.row(v4v_row(start, 10));

        // The heartbeat stops at once. The mode changes at 3.5 seconds.
        assert_eq!(coordinator.update(at(start, 3500)), vec![]);
        assert!(coordinator.file_present());
        assert_eq!(coordinator.update(at(start, 9999)), vec![]);
        assert_eq!(
            coordinator.update(at(start, 10_000)),
            vec![Action::RemoveFile]
        );
    }

    #[test]
    fn connector_mode_expiry_does_not_remove_the_file_of_a_playing_deck() {
        let start = Instant::now();
        let mut coordinator = deck_2_plays(start);
        coordinator.row(v4v_row(start, 10));
        for second in 1..=700 {
            heartbeat(&mut coordinator, at(start, second * 1000));
            assert_eq!(coordinator.update(at(start, second * 1000)), vec![]);
        }
        assert!(coordinator.file_present());
    }

    #[test]
    fn api_result_writes_while_the_file_is_present() {
        let start = Instant::now();
        let mut coordinator = deck_2_plays(start);
        coordinator.row(v4v_row(start, 617));
        assert_eq!(coordinator.api_result(), Some(write(200)));
    }

    #[test]
    fn api_result_after_a_stop_does_not_write() {
        let start = Instant::now();
        let mut coordinator = deck_2_plays(start);
        coordinator.row(v4v_row(start, 200));
        set_play(&mut coordinator, 2, false, start);
        coordinator.update(start);
        assert_eq!(coordinator.api_result(), None);
    }

    #[test]
    fn api_result_after_the_expiry_does_not_write() {
        let start = Instant::now();
        let mut coordinator = history_only(start);
        coordinator.row(v4v_row(start, 10));
        assert_eq!(
            coordinator.update(at(start, 10_000)),
            vec![Action::RemoveFile]
        );
        assert_eq!(coordinator.api_result(), None);
    }

    #[test]
    fn device_closed_changes_the_mode_on_the_next_pass() {
        let start = Instant::now();
        let mut coordinator = deck_2_plays(start);
        coordinator.row(v4v_row(start, 200));
        coordinator.device_closed();
        let actions = coordinator.update(at(start, 100));
        assert!(!has_write(&actions));
        assert!(coordinator.file_present());

        // The deck state is clear, so a new row in a new connector mode
        // links to nothing.
        coordinator.device_opened();
        heartbeat(&mut coordinator, at(start, 200));
        assert_eq!(
            coordinator.update(at(start, 200)),
            vec![Action::SendStateRequest, Action::RemoveFile]
        );
        assert_eq!(
            coordinator.row(v4v_row(at(start, 300), 200)),
            vec![Action::RemoveFile]
        );
    }

    #[test]
    fn no_connector_has_no_gate_and_the_expiry_applies() {
        let start = Instant::now();
        let mut coordinator = Coordinator::new(false, start);
        assert!(coordinator.history_allowed());
        assert_eq!(coordinator.update(start), vec![]);
        assert_eq!(coordinator.row(v4v_row(start, 617)), vec![write(617)]);
        assert_eq!(coordinator.api_result(), Some(write(617)));
        // The expiry uses the header duration, limited by the maximum.
        assert_eq!(coordinator.update(at(start, 599_999)), vec![]);
        assert_eq!(
            coordinator.update(at(start, 600_000)),
            vec![Action::RemoveFile]
        );
    }

    #[test]
    fn history_only_row_writes_with_the_header_duration() {
        let start = Instant::now();
        let mut coordinator = history_only(start);
        assert_eq!(coordinator.row(v4v_row(start, 187)), vec![write(187)]);
        assert_eq!(coordinator.row(Row::Other), vec![Action::RemoveFile]);
    }

    #[test]
    fn samples_change_on_the_linked_deck_ends_the_link() {
        let start = Instant::now();
        let mut coordinator = deck_2_plays(start);
        coordinator.row(v4v_row(start, 200));

        set_samples(&mut coordinator, 2, SAMPLES + 2, start);
        assert_eq!(coordinator.update(start), vec![Action::RemoveFile]);
        assert!(!coordinator.file_present());

        set_play(&mut coordinator, 2, false, start);
        set_play(&mut coordinator, 2, true, start);
        assert_eq!(coordinator.update(start), vec![]);
        assert_eq!(coordinator.api_result(), None);
    }

    /// Deck 2 plays and is linked to a row with the header duration
    /// `header_secs`. The last heartbeat is at `start`.
    fn linked_deck_2(start: Instant, header_secs: u64) -> Coordinator {
        let mut coordinator = deck_2_plays(start);
        assert_eq!(
            coordinator.row(v4v_row(start, header_secs)),
            vec![write(200)]
        );
        coordinator
    }

    /// The heartbeat stopped after `start`. The mode changes at 3.5 seconds.
    fn outage(coordinator: &mut Coordinator, start: Instant) {
        let actions = coordinator.update(at(start, 3500));
        assert!(!has_write(&actions), "{actions:?}");
    }

    /// The heartbeat comes back at `time`. The producer enters the connector
    /// mode, removes the file and asks for the complete state.
    fn heartbeat_returns(coordinator: &mut Coordinator, time: Instant) {
        heartbeat(coordinator, time);
        assert_eq!(
            coordinator.update(time),
            vec![Action::SendStateRequest, Action::RemoveFile]
        );
        assert!(!coordinator.file_present());
        assert_eq!(coordinator.api_result(), None);
    }

    /// The complete state after the request: deck 2 has 200 seconds and
    /// `samples`, `play` for deck 2 is `play`, and `loudest` is the loudest
    /// deck. The state end follows.
    fn complete_state(
        coordinator: &mut Coordinator,
        loudest: u8,
        play: bool,
        samples: u64,
        time: Instant,
    ) -> Vec<Action> {
        set_play(coordinator, 2, play, time);
        coordinator.control_change(cc(22, 127), time);
        set_duration(coordinator, 2, 200, time);
        set_samples(coordinator, 2, samples, time);
        set_loudest(coordinator, loudest, time);
        state_end(coordinator, time);
        coordinator.update(time)
    }

    #[test]
    fn relink_after_an_outage_writes_with_the_deck_duration() {
        let start = Instant::now();
        let mut coordinator = linked_deck_2(start, 617);
        outage(&mut coordinator, start);
        heartbeat_returns(&mut coordinator, at(start, 5000));

        assert_eq!(
            complete_state(&mut coordinator, 2, true, SAMPLES, at(start, 5100)),
            vec![write(200)]
        );
        assert!(coordinator.file_present());
        assert_eq!(coordinator.api_result(), Some(write(200)));

        // The link works again: a stop removes the file.
        set_play(&mut coordinator, 2, false, at(start, 5200));
        assert_eq!(
            coordinator.update(at(start, 5200)),
            vec![Action::RemoveFile]
        );
    }

    #[test]
    fn relink_after_a_device_close_writes() {
        let start = Instant::now();
        let mut coordinator = linked_deck_2(start, 617);
        coordinator.device_closed();
        assert!(!has_write(&coordinator.update(at(start, 100))));

        coordinator.device_opened();
        heartbeat_returns(&mut coordinator, at(start, 2000));
        assert_eq!(
            complete_state(&mut coordinator, 2, true, SAMPLES, at(start, 2100)),
            vec![write(200)]
        );
    }

    #[test]
    fn no_relink_when_a_different_deck_is_loudest() {
        let start = Instant::now();
        let mut coordinator = linked_deck_2(start, 200);
        outage(&mut coordinator, start);
        heartbeat_returns(&mut coordinator, at(start, 5000));
        assert_eq!(
            complete_state(&mut coordinator, 1, true, SAMPLES, at(start, 5100)),
            vec![]
        );
        assert!(!coordinator.file_present());
    }

    #[test]
    fn no_relink_when_the_deck_does_not_play() {
        let start = Instant::now();
        let mut coordinator = linked_deck_2(start, 200);
        outage(&mut coordinator, start);
        heartbeat_returns(&mut coordinator, at(start, 5000));
        assert_eq!(
            complete_state(&mut coordinator, 2, false, SAMPLES, at(start, 5100)),
            vec![]
        );

        // The candidate went away. A start after the state end does not
        // write.
        set_play(&mut coordinator, 2, true, at(start, 5200));
        assert_eq!(coordinator.update(at(start, 5200)), vec![]);
        assert!(!coordinator.file_present());
    }

    #[test]
    fn no_relink_when_the_sample_count_differs() {
        let start = Instant::now();
        let mut coordinator = linked_deck_2(start, 200);
        outage(&mut coordinator, start);
        heartbeat_returns(&mut coordinator, at(start, 5000));
        assert_eq!(
            complete_state(&mut coordinator, 2, true, SAMPLES + 2, at(start, 5100)),
            vec![]
        );
        assert!(!coordinator.file_present());
    }

    #[test]
    fn no_relink_when_the_sample_count_is_0() {
        let start = Instant::now();
        let mut coordinator = connector(start);
        set_duration(&mut coordinator, 2, 200, start);
        set_samples(&mut coordinator, 2, 0, start);
        set_play(&mut coordinator, 2, true, start);
        set_loudest(&mut coordinator, 2, start);
        coordinator.update(start);
        assert_eq!(coordinator.row(v4v_row(start, 200)), vec![write(200)]);

        outage(&mut coordinator, start);
        heartbeat_returns(&mut coordinator, at(start, 5000));
        assert_eq!(
            complete_state(&mut coordinator, 2, true, 0, at(start, 5100)),
            vec![]
        );
        assert!(!coordinator.file_present());
    }

    #[test]
    fn no_relink_when_the_sample_count_is_unknown() {
        let start = Instant::now();
        let mut coordinator = connector(start);
        set_duration(&mut coordinator, 2, 200, start);
        set_play(&mut coordinator, 2, true, start);
        set_loudest(&mut coordinator, 2, start);
        coordinator.update(start);
        assert_eq!(coordinator.row(v4v_row(start, 200)), vec![write(200)]);

        outage(&mut coordinator, start);
        heartbeat_returns(&mut coordinator, at(start, 5000));
        // The mapping sends no sample parts.
        set_loudest(&mut coordinator, 2, at(start, 5100));
        state_end(&mut coordinator, at(start, 5100));
        assert_eq!(coordinator.update(at(start, 5100)), vec![]);
    }

    #[test]
    fn no_relink_after_a_new_history_row_during_the_outage() {
        let start = Instant::now();
        let mut coordinator = linked_deck_2(start, 200);
        outage(&mut coordinator, start);
        // The history-only mode writes the new row with its header duration.
        assert_eq!(
            coordinator.row(v4v_row(at(start, 4000), 187)),
            vec![write(187)]
        );

        heartbeat_returns(&mut coordinator, at(start, 5000));
        assert_eq!(
            complete_state(&mut coordinator, 2, true, SAMPLES, at(start, 5100)),
            vec![]
        );
        assert!(!coordinator.file_present());
    }

    #[test]
    fn no_relink_for_a_row_that_had_no_link_before_the_outage() {
        let start = Instant::now();
        let mut coordinator = deck_2_plays(start);
        // The loudest deck is 0, so the row has no link.
        set_loudest(&mut coordinator, 0, start);
        coordinator.update(start);
        assert_eq!(
            coordinator.row(v4v_row(start, 200)),
            vec![Action::RemoveFile]
        );

        outage(&mut coordinator, start);
        heartbeat_returns(&mut coordinator, at(start, 5000));
        assert_eq!(
            complete_state(&mut coordinator, 2, true, SAMPLES, at(start, 5100)),
            vec![]
        );
        assert!(!coordinator.file_present());
    }

    #[test]
    fn no_relink_for_a_history_only_row() {
        let start = Instant::now();
        let mut coordinator = history_only(start);
        assert_eq!(coordinator.row(v4v_row(start, 200)), vec![write(200)]);

        coordinator.device_opened();
        heartbeat_returns(&mut coordinator, at(start, 1000));
        assert_eq!(
            complete_state(&mut coordinator, 2, true, SAMPLES, at(start, 1100)),
            vec![]
        );
        assert!(!coordinator.file_present());
    }

    #[test]
    fn no_relink_without_a_state_end_before_the_next_mode_change() {
        let start = Instant::now();
        let mut coordinator = linked_deck_2(start, 200);
        outage(&mut coordinator, start);
        heartbeat_returns(&mut coordinator, at(start, 5000));

        // The complete state arrives without its end marker.
        set_samples(&mut coordinator, 2, SAMPLES, at(start, 5100));
        set_loudest(&mut coordinator, 2, at(start, 5100));
        assert_eq!(coordinator.update(at(start, 5100)), vec![]);

        // A second outage, then a complete state with its end marker.
        assert!(!has_write(&coordinator.update(at(start, 8500))));
        heartbeat_returns(&mut coordinator, at(start, 10_000));
        assert_eq!(
            complete_state(&mut coordinator, 2, true, SAMPLES, at(start, 10_100)),
            vec![]
        );
        assert!(!coordinator.file_present());
    }

    #[test]
    fn deck_change_before_the_state_end_does_not_relink() {
        let start = Instant::now();
        let mut coordinator = linked_deck_2(start, 200);
        outage(&mut coordinator, start);
        heartbeat_returns(&mut coordinator, at(start, 5000));

        set_play(&mut coordinator, 2, false, at(start, 5050));
        set_play(&mut coordinator, 2, true, at(start, 5060));
        set_loudest(&mut coordinator, 0, at(start, 5060));
        set_loudest(&mut coordinator, 2, at(start, 5070));
        assert_eq!(coordinator.update(at(start, 5070)), vec![]);
        assert!(!coordinator.file_present());

        // Only the state end links the row again.
        state_end(&mut coordinator, at(start, 5100));
        assert_eq!(coordinator.update(at(start, 5100)), vec![write(200)]);
    }

    #[test]
    fn only_the_first_state_end_after_the_entry_tests_the_candidate() {
        let start = Instant::now();
        let mut coordinator = linked_deck_2(start, 200);
        outage(&mut coordinator, start);
        heartbeat_returns(&mut coordinator, at(start, 5000));
        assert_eq!(
            complete_state(&mut coordinator, 1, true, SAMPLES, at(start, 5100)),
            vec![]
        );
        assert_eq!(
            complete_state(&mut coordinator, 2, true, SAMPLES, at(start, 5200)),
            vec![]
        );
        assert!(!coordinator.file_present());
    }

    #[test]
    fn startup_entry_never_relinks() {
        let start = Instant::now();
        let mut coordinator = Coordinator::new(true, start);
        coordinator.device_opened();
        heartbeat(&mut coordinator, start);
        assert_eq!(
            coordinator.update(start),
            vec![Action::SendStateRequest, Action::RemoveFile]
        );
        assert_eq!(
            complete_state(&mut coordinator, 2, true, SAMPLES, at(start, 100)),
            vec![]
        );

        // The first row after startup existed before the entry.
        assert_eq!(
            coordinator.row(v4v_row(at(start, 200), 200)),
            vec![Action::RemoveFile]
        );
        assert_eq!(
            complete_state(&mut coordinator, 2, true, SAMPLES, at(start, 300)),
            vec![]
        );
        assert!(!coordinator.file_present());
    }

    #[test]
    fn relink_after_the_expiry_ended_during_the_outage_writes() {
        let start = Instant::now();
        let mut coordinator = linked_deck_2(start, 10);
        outage(&mut coordinator, start);
        assert_eq!(
            coordinator.update(at(start, 10_000)),
            vec![Action::RemoveFile]
        );

        heartbeat_returns(&mut coordinator, at(start, 12_000));
        assert_eq!(
            complete_state(&mut coordinator, 2, true, SAMPLES, at(start, 12_100)),
            vec![write(200)]
        );
        assert!(coordinator.file_present());
    }

    #[test]
    fn a_second_outage_after_a_relink_past_the_expiry_removes_at_once() {
        let start = Instant::now();
        let mut coordinator = linked_deck_2(start, 10);
        outage(&mut coordinator, start);
        assert_eq!(
            coordinator.update(at(start, 10_000)),
            vec![Action::RemoveFile]
        );
        heartbeat_returns(&mut coordinator, at(start, 12_000));
        assert_eq!(
            complete_state(&mut coordinator, 2, true, SAMPLES, at(start, 12_100)),
            vec![write(200)]
        );

        // No heartbeat after 12 seconds. The expiry ended at 10 seconds.
        assert_eq!(
            coordinator.update(at(start, 15_500)),
            vec![Action::RemoveFile]
        );
        assert!(!coordinator.file_present());
    }

    // The display link and the display state (ADR 0008 §The Display Link and
    // §The Display State).

    fn track(name: &str) -> DisplayTrack {
        DisplayTrack {
            artist: format!("{name} Artist"),
            title: format!("{name} Title"),
        }
    }

    fn shown(name: &str, v4v: bool) -> DisplayState {
        DisplayState::Track {
            artist: format!("{name} Artist"),
            title: format!("{name} Title"),
            v4v,
        }
    }

    fn other_row(coordinator: &mut Coordinator, name: &str) -> Vec<Action> {
        coordinator.history_row(Row::Other, track(name))
    }

    fn v4v_display_row(
        coordinator: &mut Coordinator,
        name: &str,
        time: Instant,
        header_secs: u64,
    ) -> Vec<Action> {
        coordinator.history_row(v4v_row(time, header_secs), track(name))
    }

    /// Deck 2 plays and is display-linked to the non-V4V row "A".
    fn other_linked_deck_2(start: Instant) -> Coordinator {
        let mut coordinator = deck_2_plays(start);
        assert_eq!(other_row(&mut coordinator, "A"), vec![Action::RemoveFile]);
        assert_eq!(coordinator.display_state(), shown("A", false));
        coordinator
    }

    #[test]
    fn display_is_null_at_startup() {
        let start = Instant::now();
        let mut coordinator = Coordinator::new(true, start);
        assert_eq!(coordinator.display_state(), DisplayState::Null);
        assert_eq!(coordinator.take_display_change(), None);
    }

    #[test]
    fn non_v4v_row_with_playing_loudest_deck_gives_display_track_and_no_payment_write() {
        let start = Instant::now();
        let mut coordinator = deck_2_plays(start);
        let actions = other_row(&mut coordinator, "A");
        assert_eq!(actions, vec![Action::RemoveFile]);
        assert!(!has_write(&actions));
        assert!(!coordinator.file_present());
        assert_eq!(coordinator.api_result(), None);
        assert_eq!(coordinator.take_display_change(), Some(shown("A", false)));
        assert_eq!(coordinator.take_display_change(), None);
    }

    #[test]
    fn non_v4v_row_with_loudest_deck_0_gives_display_null() {
        let start = Instant::now();
        let mut coordinator = deck_2_plays(start);
        set_loudest(&mut coordinator, 0, start);
        coordinator.update(start);
        other_row(&mut coordinator, "A");
        assert_eq!(coordinator.display_state(), DisplayState::Null);
    }

    #[test]
    fn non_v4v_row_with_a_loudest_deck_that_does_not_play_gives_display_null() {
        let start = Instant::now();
        let mut coordinator = connector(start);
        set_play(&mut coordinator, 2, false, start);
        set_loudest(&mut coordinator, 2, start);
        coordinator.update(start);
        other_row(&mut coordinator, "A");
        assert_eq!(coordinator.display_state(), DisplayState::Null);
    }

    #[test]
    fn non_v4v_linked_deck_stop_gives_null_and_start_gives_the_track_again() {
        let start = Instant::now();
        let mut coordinator = other_linked_deck_2(start);

        set_play(&mut coordinator, 2, false, start);
        assert_eq!(coordinator.update(start), vec![]);
        assert_eq!(coordinator.display_state(), DisplayState::Null);

        set_play(&mut coordinator, 2, true, start);
        assert_eq!(coordinator.update(start), vec![]);
        assert_eq!(coordinator.display_state(), shown("A", false));
        assert!(!coordinator.file_present());
    }

    /// A new load on the display-linked deck gives `Null`. A start of that
    /// deck after the load does not show the track again.
    fn assert_new_load_ends_the_display_link(load: impl Fn(&mut Coordinator, Instant)) {
        let start = Instant::now();
        let mut coordinator = other_linked_deck_2(start);
        load(&mut coordinator, start);
        assert_eq!(coordinator.update(start), vec![]);
        assert_eq!(coordinator.display_state(), DisplayState::Null);

        set_play(&mut coordinator, 2, false, start);
        set_play(&mut coordinator, 2, true, start);
        coordinator.update(start);
        assert_eq!(coordinator.display_state(), DisplayState::Null);
    }

    #[test]
    fn track_loaded_on_the_display_linked_deck_gives_null() {
        assert_new_load_ends_the_display_link(|coordinator, time| {
            coordinator.control_change(cc(22, 127), time);
        });
    }

    #[test]
    fn duration_change_on_the_display_linked_deck_gives_null() {
        assert_new_load_ends_the_display_link(|coordinator, time| {
            set_duration(coordinator, 2, 328, time);
        });
    }

    #[test]
    fn samples_change_on_the_display_linked_deck_gives_null() {
        assert_new_load_ends_the_display_link(|coordinator, time| {
            set_samples(coordinator, 2, SAMPLES + 2, time);
        });
    }

    #[test]
    fn deck_change_on_a_deck_that_is_not_display_linked_keeps_the_track() {
        let start = Instant::now();
        let mut coordinator = other_linked_deck_2(start);
        set_play(&mut coordinator, 1, true, start);
        set_play(&mut coordinator, 1, false, start);
        coordinator.control_change(cc(21, 127), start);
        set_samples(&mut coordinator, 1, 1000, start);
        coordinator.update(start);
        assert_eq!(coordinator.display_state(), shown("A", false));
    }

    #[test]
    fn entering_the_connector_mode_gives_null_and_the_prior_row_never_display_links() {
        let start = Instant::now();
        let mut coordinator = history_only(start);
        other_row(&mut coordinator, "A");
        assert_eq!(coordinator.display_state(), shown("A", false));

        coordinator.device_opened();
        heartbeat(&mut coordinator, at(start, 1000));
        coordinator.update(at(start, 1000));
        assert_eq!(coordinator.display_state(), DisplayState::Null);

        // The complete state shows deck 2 loudest and playing.
        assert_eq!(
            complete_state(&mut coordinator, 2, true, SAMPLES, at(start, 1100)),
            vec![]
        );
        assert_eq!(coordinator.display_state(), DisplayState::Null);
        set_play(&mut coordinator, 2, false, at(start, 1200));
        set_play(&mut coordinator, 2, true, at(start, 1300));
        coordinator.update(at(start, 1300));
        assert_eq!(coordinator.display_state(), DisplayState::Null);
    }

    #[test]
    fn startup_row_before_the_entry_has_no_display_link_and_the_next_row_has_one() {
        let start = Instant::now();
        let mut coordinator = Coordinator::new(true, start);
        coordinator.device_opened();
        heartbeat(&mut coordinator, start);
        set_play(&mut coordinator, 2, true, start);
        set_loudest(&mut coordinator, 2, start);
        coordinator.update(start);

        other_row(&mut coordinator, "A");
        assert_eq!(coordinator.display_state(), DisplayState::Null);
        other_row(&mut coordinator, "B");
        assert_eq!(coordinator.display_state(), shown("B", false));
    }

    #[test]
    fn relink_after_an_outage_restores_the_display_track() {
        let start = Instant::now();
        let mut coordinator = other_linked_deck_2(start);
        outage(&mut coordinator, start);
        // The history-only mode does not change the display state.
        assert_eq!(coordinator.display_state(), shown("A", false));
        heartbeat_returns(&mut coordinator, at(start, 5000));
        assert_eq!(coordinator.display_state(), DisplayState::Null);

        assert_eq!(
            complete_state(&mut coordinator, 2, true, SAMPLES, at(start, 5100)),
            vec![]
        );
        assert_eq!(coordinator.display_state(), shown("A", false));

        // The display link works again: a stop gives `Null`.
        set_play(&mut coordinator, 2, false, at(start, 5200));
        coordinator.update(at(start, 5200));
        assert_eq!(coordinator.display_state(), DisplayState::Null);
    }

    #[test]
    fn v4v_relink_after_an_outage_restores_both_links() {
        let start = Instant::now();
        let mut coordinator = deck_2_plays(start);
        v4v_display_row(&mut coordinator, "V", start, 617);
        outage(&mut coordinator, start);
        heartbeat_returns(&mut coordinator, at(start, 5000));
        assert_eq!(coordinator.display_state(), DisplayState::Null);
        assert_eq!(
            complete_state(&mut coordinator, 2, true, SAMPLES, at(start, 5100)),
            vec![write(200)]
        );
        assert_eq!(coordinator.display_state(), shown("V", true));
    }

    #[test]
    fn no_display_relink_when_a_different_deck_is_loudest() {
        let start = Instant::now();
        let mut coordinator = other_linked_deck_2(start);
        outage(&mut coordinator, start);
        heartbeat_returns(&mut coordinator, at(start, 5000));
        complete_state(&mut coordinator, 1, true, SAMPLES, at(start, 5100));
        assert_eq!(coordinator.display_state(), DisplayState::Null);
        // Only the first state end tests the candidate.
        complete_state(&mut coordinator, 2, true, SAMPLES, at(start, 5200));
        assert_eq!(coordinator.display_state(), DisplayState::Null);
    }

    #[test]
    fn no_display_relink_when_the_sample_count_differs() {
        let start = Instant::now();
        let mut coordinator = other_linked_deck_2(start);
        outage(&mut coordinator, start);
        heartbeat_returns(&mut coordinator, at(start, 5000));
        complete_state(&mut coordinator, 2, true, SAMPLES + 2, at(start, 5100));
        assert_eq!(coordinator.display_state(), DisplayState::Null);
    }

    #[test]
    fn no_display_relink_when_the_deck_does_not_play() {
        let start = Instant::now();
        let mut coordinator = other_linked_deck_2(start);
        outage(&mut coordinator, start);
        heartbeat_returns(&mut coordinator, at(start, 5000));
        complete_state(&mut coordinator, 2, false, SAMPLES, at(start, 5100));
        assert_eq!(coordinator.display_state(), DisplayState::Null);
    }

    #[test]
    fn deck_change_before_the_state_end_does_not_display_relink() {
        let start = Instant::now();
        let mut coordinator = other_linked_deck_2(start);
        outage(&mut coordinator, start);
        heartbeat_returns(&mut coordinator, at(start, 5000));
        set_play(&mut coordinator, 2, false, at(start, 5050));
        set_play(&mut coordinator, 2, true, at(start, 5060));
        coordinator.update(at(start, 5060));
        assert_eq!(coordinator.display_state(), DisplayState::Null);

        state_end(&mut coordinator, at(start, 5100));
        coordinator.update(at(start, 5100));
        assert_eq!(coordinator.display_state(), shown("A", false));
    }

    #[test]
    fn no_display_relink_after_a_new_history_row_during_the_outage() {
        let start = Instant::now();
        let mut coordinator = other_linked_deck_2(start);
        outage(&mut coordinator, start);
        other_row(&mut coordinator, "B");
        assert_eq!(coordinator.display_state(), shown("B", false));

        heartbeat_returns(&mut coordinator, at(start, 5000));
        complete_state(&mut coordinator, 2, true, SAMPLES, at(start, 5100));
        assert_eq!(coordinator.display_state(), DisplayState::Null);
    }

    #[test]
    fn leaving_the_connector_mode_after_a_stop_keeps_display_null() {
        let start = Instant::now();
        let mut coordinator = other_linked_deck_2(start);
        set_play(&mut coordinator, 2, false, at(start, 1000));
        coordinator.update(at(start, 1000));
        assert_eq!(coordinator.display_state(), DisplayState::Null);

        coordinator.update(at(start, 3500));
        assert_eq!(coordinator.display_state(), DisplayState::Null);
    }

    #[test]
    fn v4v_row_gives_a_payment_write_and_a_display_track() {
        let start = Instant::now();
        let mut coordinator = deck_2_plays(start);
        assert_eq!(
            v4v_display_row(&mut coordinator, "V", start, 617),
            vec![write(200)]
        );
        assert_eq!(coordinator.display_state(), shown("V", true));

        set_play(&mut coordinator, 2, false, start);
        assert_eq!(coordinator.update(start), vec![Action::RemoveFile]);
        assert_eq!(coordinator.display_state(), DisplayState::Null);

        set_play(&mut coordinator, 2, true, start);
        assert_eq!(coordinator.update(start), vec![write(200)]);
        assert_eq!(coordinator.display_state(), shown("V", true));
    }

    #[test]
    fn v4v_row_new_load_gives_a_remove_and_display_null() {
        let start = Instant::now();
        let mut coordinator = deck_2_plays(start);
        v4v_display_row(&mut coordinator, "V", start, 200);
        set_samples(&mut coordinator, 2, SAMPLES + 2, start);
        assert_eq!(coordinator.update(start), vec![Action::RemoveFile]);
        assert_eq!(coordinator.display_state(), DisplayState::Null);
    }

    #[test]
    fn non_v4v_row_after_a_v4v_row_removes_the_file_and_shows_the_new_track() {
        let start = Instant::now();
        let mut coordinator = deck_2_plays(start);
        v4v_display_row(&mut coordinator, "V", start, 200);
        assert_eq!(other_row(&mut coordinator, "A"), vec![Action::RemoveFile]);
        assert_eq!(coordinator.display_state(), shown("A", false));
    }

    #[test]
    fn history_only_display_is_the_latest_row_until_the_next_row() {
        let start = Instant::now();
        let mut coordinator = history_only(start);
        assert_eq!(
            v4v_display_row(&mut coordinator, "V", start, 10),
            vec![write(10)]
        );
        assert_eq!(coordinator.display_state(), shown("V", true));

        // The ADR 0005 expiry removes the drop file. The display has no stop.
        assert_eq!(
            coordinator.update(at(start, 10_000)),
            vec![Action::RemoveFile]
        );
        assert_eq!(coordinator.display_state(), shown("V", true));

        assert_eq!(other_row(&mut coordinator, "A"), vec![Action::RemoveFile]);
        assert_eq!(coordinator.display_state(), shown("A", false));
    }

    #[test]
    fn no_connector_display_is_the_latest_row() {
        let start = Instant::now();
        let mut coordinator = Coordinator::new(false, start);
        assert_eq!(coordinator.display_state(), DisplayState::Null);
        other_row(&mut coordinator, "A");
        assert_eq!(coordinator.take_display_change(), Some(shown("A", false)));
        v4v_display_row(&mut coordinator, "V", start, 617);
        assert_eq!(coordinator.take_display_change(), Some(shown("V", true)));
    }
}
