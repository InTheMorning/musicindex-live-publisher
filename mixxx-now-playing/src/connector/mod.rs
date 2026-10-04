//! The Mixxx MIDI connector (ADR 0006 and ADR 0007).
//!
//! `midi` parses the bytes from the raw MIDI device, `state` keeps the deck
//! state and gives the mode, `device` reads the device in its own thread, and
//! `link` makes each decision about the drop file. `command` sends one
//! command to Mixxx and gives its result.

pub mod command;
pub mod device;
pub mod link;
pub mod midi;
pub mod state;

pub use command::{FADE_NOW, Outcome, command_message, send_command};
pub use device::{
    DeviceEvent, DeviceLocation, STATE_REQUEST, open_device, pump, resolve_raw_device, spawn_reader,
};
pub use link::{Action, Coordinator, DisplayState, DisplayTrack, Row, STARTUP_TIMEOUT};
pub use midi::{ControlChange, MidiParser};
pub use state::{
    ConnectorEvent, ConnectorState, DECKS, DeckChange, DeckChangeKind, HEARTBEAT_TIMEOUT,
    HistoryOnlyReason, Mode, PROTOCOL_VERSION,
};
