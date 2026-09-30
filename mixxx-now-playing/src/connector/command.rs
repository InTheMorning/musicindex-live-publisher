//! Commands to Mixxx through the connector (ADR 0007).
//!
//! `send_command` holds all the rules of one command. It reads the events of
//! the device reader from a channel and writes the command to a writer. The
//! command line gives it the raw MIDI device. The tests give it a channel and
//! a `Vec<u8>`.

use std::io::Write;
use std::sync::mpsc::{Receiver, RecvTimeoutError};
use std::time::Instant;

use super::device::DeviceEvent;
use super::midi::ControlChange;
use super::state::{CC_COMMAND_DONE, CC_COMMAND_REFUSED, CHANNEL, PROTOCOL_VERSION};

/// The command code of "AutoDJ fade now" (ADR 0007 §Messages).
pub const FADE_NOW: u8 = 1;

/// The status byte of a control change on channel 16.
const STATUS: u8 = 0xB0 | CHANNEL;

const CC_HEARTBEAT: u8 = 1;

/// The controller of a command from a consumer to Mixxx.
const CC_COMMAND: u8 = 4;

/// The result of one command (ADR 0007 §The Command Line).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    /// The mapping did the command. It set the control.
    Done,
    /// The mapping refused the command.
    Refused,
    /// The command was not sent.
    NotSent,
    /// The command was sent, but no answer arrived. The command can have run.
    Unknown,
}

impl Outcome {
    /// Gives the exit code of the command line for this outcome.
    pub fn exit_code(self) -> i32 {
        match self {
            Self::Done => 0,
            Self::Refused => 3,
            Self::NotSent => 4,
            Self::Unknown => 5,
        }
    }
}

/// Gives the three bytes of the command with this code.
pub fn command_message(code: u8) -> [u8; 3] {
    [STATUS, CC_COMMAND, code]
}

/// Sends one command and waits for its answer until `deadline`.
///
/// The function does these steps:
///
/// 1. It waits for a heartbeat of `PROTOCOL_VERSION` on channel 16. A
///    heartbeat of a different version does not count.
/// 2. It writes the command in one `write_all` call.
/// 3. It waits for CC 4 or CC 5 on channel 16 with the value `code`. Other
///    messages do not count.
///
/// Gives `NotSent` when the deadline comes or the reader stops before step 2.
/// Gives `Unknown` when the deadline comes or the reader stops after step 2.
/// A failed write also gives `Unknown`, because a part of the message can
/// have gone to Mixxx.
pub fn send_command(
    events: &Receiver<DeviceEvent>,
    writer: &mut impl Write,
    code: u8,
    deadline: Instant,
) -> Outcome {
    let heartbeat = |cc: ControlChange| {
        cc.channel == CHANNEL && cc.controller == CC_HEARTBEAT && cc.value == PROTOCOL_VERSION
    };
    if next_match(events, deadline, heartbeat).is_none() {
        return Outcome::NotSent;
    }

    if writer.write_all(&command_message(code)).is_err() {
        return Outcome::Unknown;
    }

    let answer = |cc: ControlChange| {
        cc.channel == CHANNEL
            && cc.value == code
            && (cc.controller == CC_COMMAND_DONE || cc.controller == CC_COMMAND_REFUSED)
    };
    match next_match(events, deadline, answer) {
        Some(cc) if cc.controller == CC_COMMAND_DONE => Outcome::Done,
        Some(_) => Outcome::Refused,
        None => Outcome::Unknown,
    }
}

/// Gives the first control change that `wanted` accepts. Gives `None` when
/// the deadline comes or the reader stops first.
fn next_match(
    events: &Receiver<DeviceEvent>,
    deadline: Instant,
    wanted: impl Fn(ControlChange) -> bool,
) -> Option<ControlChange> {
    loop {
        let remaining = deadline.checked_duration_since(Instant::now())?;
        if remaining.is_zero() {
            return None;
        }
        match events.recv_timeout(remaining) {
            Ok(DeviceEvent::ControlChange { cc, .. }) if wanted(cc) => return Some(cc),
            Ok(_) => {}
            Err(RecvTimeoutError::Timeout | RecvTimeoutError::Disconnected) => return None,
        }
    }
}

#[cfg(test)]
mod tests {
    use std::io;
    use std::sync::mpsc::{self, Sender};
    use std::time::Duration;

    use super::*;

    /// A deadline for a test that must end before it.
    fn far() -> Instant {
        Instant::now() + Duration::from_secs(10)
    }

    /// A short deadline for a test that must wait for it.
    fn near() -> Instant {
        Instant::now() + Duration::from_millis(50)
    }

    fn send(sender: &Sender<DeviceEvent>, channel: u8, controller: u8, value: u8) {
        let cc = ControlChange {
            channel,
            controller,
            value,
        };
        sender
            .send(DeviceEvent::ControlChange {
                cc,
                time: Instant::now(),
            })
            .expect("send event");
    }

    fn heartbeat(sender: &Sender<DeviceEvent>, version: u8) {
        send(sender, 15, 1, version);
    }

    #[test]
    fn heartbeat_and_done_answer_give_done_and_one_message() {
        let (sender, receiver) = mpsc::channel();
        heartbeat(&sender, 3);
        send(&sender, 15, 4, 1);
        let mut written = Vec::new();

        let outcome = send_command(&receiver, &mut written, FADE_NOW, far());

        assert_eq!(outcome, Outcome::Done);
        assert_eq!(written, [0xBF, 0x04, 0x01]);
    }

    #[test]
    fn refused_answer_gives_refused() {
        let (sender, receiver) = mpsc::channel();
        heartbeat(&sender, 3);
        send(&sender, 15, 5, 1);
        let mut written = Vec::new();

        let outcome = send_command(&receiver, &mut written, FADE_NOW, far());

        assert_eq!(outcome, Outcome::Refused);
        assert_eq!(written, [0xBF, 0x04, 0x01]);
    }

    #[test]
    fn no_heartbeat_gives_not_sent_and_writes_nothing() {
        let (_sender, receiver) = mpsc::channel();
        let mut written = Vec::new();

        let outcome = send_command(&receiver, &mut written, FADE_NOW, near());

        assert_eq!(outcome, Outcome::NotSent);
        assert!(written.is_empty());
    }

    #[test]
    fn heartbeats_of_version_2_give_not_sent_and_write_nothing() {
        let (sender, receiver) = mpsc::channel();
        for _ in 0..3 {
            heartbeat(&sender, 2);
        }
        // A heartbeat of version 3 on a different channel does not count.
        send(&sender, 0, 1, 3);
        let mut written = Vec::new();

        let outcome = send_command(&receiver, &mut written, FADE_NOW, near());

        assert_eq!(outcome, Outcome::NotSent);
        assert!(written.is_empty());
    }

    #[test]
    fn a_stopped_reader_before_the_heartbeat_gives_not_sent() {
        let (sender, receiver) = mpsc::channel();
        drop(sender);
        let mut written = Vec::new();

        let outcome = send_command(&receiver, &mut written, FADE_NOW, far());

        assert_eq!(outcome, Outcome::NotSent);
        assert!(written.is_empty());
    }

    #[test]
    fn heartbeat_and_no_answer_gives_unknown() {
        let (sender, receiver) = mpsc::channel();
        heartbeat(&sender, 3);
        let mut written = Vec::new();

        let outcome = send_command(&receiver, &mut written, FADE_NOW, near());

        assert_eq!(outcome, Outcome::Unknown);
        assert_eq!(written, [0xBF, 0x04, 0x01]);
    }

    #[test]
    fn a_stopped_reader_after_the_write_gives_unknown() {
        let (sender, receiver) = mpsc::channel();
        heartbeat(&sender, 3);
        drop(sender);
        let mut written = Vec::new();

        let outcome = send_command(&receiver, &mut written, FADE_NOW, far());

        assert_eq!(outcome, Outcome::Unknown);
        assert_eq!(written, [0xBF, 0x04, 0x01]);
    }

    #[test]
    fn answers_with_a_different_value_or_channel_do_not_count() {
        let (sender, receiver) = mpsc::channel();
        heartbeat(&sender, 3);
        send(&sender, 15, 4, 2);
        send(&sender, 15, 5, 2);
        send(&sender, 0, 4, 1);
        send(&sender, 14, 5, 1);
        let mut written = Vec::new();

        let outcome = send_command(&receiver, &mut written, FADE_NOW, near());

        assert_eq!(outcome, Outcome::Unknown);
    }

    #[test]
    fn the_correct_answer_after_wrong_answers_gives_done() {
        let (sender, receiver) = mpsc::channel();
        heartbeat(&sender, 3);
        send(&sender, 15, 4, 2);
        send(&sender, 0, 4, 1);
        send(&sender, 15, 4, 1);
        let mut written = Vec::new();

        let outcome = send_command(&receiver, &mut written, FADE_NOW, far());

        assert_eq!(outcome, Outcome::Done);
    }

    #[test]
    fn more_heartbeats_give_one_message() {
        let (sender, receiver) = mpsc::channel();
        heartbeat(&sender, 3);
        heartbeat(&sender, 3);
        heartbeat(&sender, 3);
        send(&sender, 15, 4, 1);
        let mut written = Vec::new();

        let outcome = send_command(&receiver, &mut written, FADE_NOW, far());

        assert_eq!(outcome, Outcome::Done);
        assert_eq!(written, [0xBF, 0x04, 0x01]);
    }

    /// A writer that accepts `limit` bytes for each call, and so needs more
    /// than one call for a message.
    struct ShortWriter {
        limit: usize,
        calls: usize,
        written: Vec<u8>,
    }

    impl Write for ShortWriter {
        fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
            self.calls += 1;
            let count = buf.len().min(self.limit);
            self.written.extend_from_slice(&buf[..count]);
            Ok(count)
        }

        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    #[test]
    fn the_message_goes_out_in_one_write_all_call() {
        let (sender, receiver) = mpsc::channel();
        heartbeat(&sender, 3);
        send(&sender, 15, 4, 1);
        let mut writer = ShortWriter {
            limit: 3,
            calls: 0,
            written: Vec::new(),
        };

        let outcome = send_command(&receiver, &mut writer, FADE_NOW, far());

        assert_eq!(outcome, Outcome::Done);
        assert_eq!(writer.calls, 1);
        assert_eq!(writer.written, [0xBF, 0x04, 0x01]);
    }

    struct FailingWriter;

    impl Write for FailingWriter {
        fn write(&mut self, _buf: &[u8]) -> io::Result<usize> {
            Err(io::Error::other("device gone"))
        }

        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    #[test]
    fn a_failed_write_gives_unknown() {
        let (sender, receiver) = mpsc::channel();
        heartbeat(&sender, 3);

        let outcome = send_command(&receiver, &mut FailingWriter, FADE_NOW, far());

        assert_eq!(outcome, Outcome::Unknown);
    }

    #[test]
    fn exit_codes() {
        assert_eq!(Outcome::Done.exit_code(), 0);
        assert_eq!(Outcome::Refused.exit_code(), 3);
        assert_eq!(Outcome::NotSent.exit_code(), 4);
        assert_eq!(Outcome::Unknown.exit_code(), 5);
    }
}
