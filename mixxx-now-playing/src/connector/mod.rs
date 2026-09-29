//! The Mixxx MIDI connector (ADR 0006).
//!
//! `midi` parses the bytes from the raw MIDI device, `state` keeps the deck
//! state and gives the mode, and `device` reads the device in its own thread.

pub mod device;
pub mod midi;
pub mod state;

pub use device::{
    DeviceEvent, DeviceLocation, STATE_REQUEST, pump, resolve_raw_device, spawn_reader,
};
pub use midi::{ControlChange, MidiParser};
pub use state::{
    ConnectorState, DECKS, DeckChange, DeckChangeKind, HEARTBEAT_TIMEOUT, HistoryOnlyReason, Mode,
    PROTOCOL_VERSION,
};
