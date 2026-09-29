//! The link rules of the connector mode (ADR 0006 §How `mixxx-now-playing`
//! Uses The Deck State, §When The Connector Is Not Available and §Entering And
//! Leaving The Connector Mode).
//!
//! `Coordinator` makes each decision about the drop file. It reads no clock,
//! no file and no device. The caller gives it the connector events, the
//! history rows and the times. It gives back the actions that the caller must
//! do.

use std::time::{Duration, Instant};

use super::midi::ControlChange;
use super::state::{ConnectorState, DECKS, DeckChange, DeckChangeKind, HistoryOnlyReason, Mode};
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

/// The mode that the `Coordinator` uses now.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum KnownMode {
    /// The producer started and does not know the mode yet.
    Unknown,
    Connector,
    HistoryOnly,
}

#[derive(Debug, Clone, Copy)]
struct CurrentRow {
    expiry: Expiry,
    link: Option<u8>,
    file_duration: Option<Duration>,
}

/// Makes each decision about the drop file of the producer.
#[derive(Debug)]
pub struct Coordinator {
    enabled: bool,
    started: Instant,
    state: ConnectorState,
    device_open: bool,
    heartbeat_seen: bool,
    unavailable_seen: bool,
    mode: KnownMode,
    pending: Vec<DeckChange>,
    row: Option<CurrentRow>,
    present: bool,
    skip_next_row: bool,
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
    /// A deck change waits for the next `update`.
    pub fn control_change(&mut self, cc: ControlChange, time: Instant) {
        if let Some(change) = self.state.apply(cc, time) {
            self.pending.push(change);
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

    /// Applies the mode change, the deck changes and the expiry at `now`,
    /// in this order.
    pub fn update(&mut self, now: Instant) -> Vec<Action> {
        let mut actions = Vec::new();
        self.apply_mode(now, &mut actions);
        self.apply_deck_changes(&mut actions);
        self.apply_expiry(now, &mut actions);
        actions
    }

    /// Applies a new history row.
    pub fn row(&mut self, row: Row) -> Vec<Action> {
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
            KnownMode::Connector if skip => {
                tracing::info!(
                    loudest = self.state.loudest_deck(),
                    "history row existed before the connector mode; no link"
                );
                (None, None)
            }
            KnownMode::Connector => {
                let loudest = self.state.loudest_deck();
                if (1..=DECKS).contains(&loudest) && self.state.play(loudest) {
                    let duration = self
                        .state
                        .duration_secs(loudest)
                        .map(|secs| Duration::from_secs(u64::from(secs)));
                    (Some(loudest), duration)
                } else {
                    tracing::info!(loudest, "no playing loudest deck; history row has no link");
                    (None, None)
                }
            }
            KnownMode::Unknown => (None, None),
        };

        let write = self.mode == KnownMode::HistoryOnly || link.is_some();
        self.row = Some(CurrentRow {
            expiry,
            link,
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
                self.row = None;
                self.present = false;
                self.pending.clear();
                actions.push(Action::SendStateRequest);
                actions.push(Action::RemoveFile);
            }
            Mode::HistoryOnly(reason) if self.mode != KnownMode::HistoryOnly => {
                tracing::warn!(reason = ?reason, "history-only mode");
                self.mode = KnownMode::HistoryOnly;
                // No write here. The expiry of the row applies from the time
                // of the row, in `apply_expiry`.
                if let Some(row) = self.row.as_mut() {
                    row.link = None;
                }
                self.pending.clear();
            }
            _ => {}
        }
    }

    fn apply_deck_changes(&mut self, actions: &mut Vec<Action>) {
        let changes = std::mem::take(&mut self.pending);
        if self.mode != KnownMode::Connector {
            return;
        }
        for change in changes {
            let Some(row) = self.row.as_mut() else {
                return;
            };
            if row.link != Some(change.deck) {
                continue;
            }
            match change.kind {
                DeckChangeKind::Play(false) => {
                    self.present = false;
                    actions.push(Action::RemoveFile);
                }
                DeckChangeKind::Play(true) => {
                    if !self.present {
                        self.present = true;
                        actions.push(Action::WriteFile {
                            duration: row.file_duration,
                        });
                    }
                }
                DeckChangeKind::TrackLoaded(_) | DeckChangeKind::Duration(_) => {
                    row.link = None;
                    self.present = false;
                    actions.push(Action::RemoveFile);
                }
            }
        }
    }

    fn apply_expiry(&mut self, now: Instant, actions: &mut Vec<Action>) {
        if self.mode != KnownMode::HistoryOnly {
            return;
        }
        let Some(row) = self.row.as_mut() else {
            return;
        };
        if row.expiry.expired_at(now) {
            row.expiry = Expiry::none();
            self.present = false;
            actions.push(Action::RemoveFile);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const CHANNEL_16: u8 = 15;
    const MAX: Duration = Duration::from_secs(600);

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
        coordinator.control_change(cc(1, 1), time);
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

    /// A connector coordinator with deck 2 loudest, playing, 200 seconds.
    fn deck_2_plays(start: Instant) -> Coordinator {
        let mut coordinator = connector(start);
        set_duration(&mut coordinator, 2, 200, start);
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
}
