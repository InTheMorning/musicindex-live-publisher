//! MIDI byte parser for the connector protocol.
//!
//! Parses raw MIDI bytes into control change messages. The parser supports
//! running status, ignores real-time bytes inside messages, and skips SysEx
//! and other message kinds.

/// A control change message received from the MIDI device.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ControlChange {
    /// MIDI channel (0 to 15 for channels 1 to 16).
    pub channel: u8,
    /// MIDI controller number.
    pub controller: u8,
    /// MIDI controller value.
    pub value: u8,
}

/// Parses MIDI bytes into control change messages.
///
/// The parser accepts bytes in any split. It supports running status, ignores
/// real-time bytes (0xF8 to 0xFF) inside messages, and skips SysEx (0xF0 to
/// 0xF7) and other message kinds. It calls the callback for each control
/// change message it completes.
pub struct MidiParser {
    running_status: Option<u8>,
    buffer: Vec<u8>,
}

impl MidiParser {
    /// Creates a new MIDI parser.
    pub fn new() -> Self {
        Self {
            running_status: None,
            buffer: Vec::with_capacity(3),
        }
    }

    /// Parses a block of bytes and calls the callback for each control change.
    pub fn parse<F>(&mut self, data: &[u8], mut callback: F)
    where
        F: FnMut(ControlChange),
    {
        for &byte in data {
            // Real-time byte: ignore it and continue the message.
            if byte >= 0xF8 {
                continue;
            }

            // Status byte: starts a new message.
            if byte & 0x80 != 0 {
                // A system message, SysEx included, clears the running
                // status. Its data bytes then have no status and are skipped.
                if byte >= 0xF0 {
                    self.running_status = None;
                    self.buffer.clear();
                    continue;
                }

                // Regular channel message: save as running status.
                self.running_status = Some(byte);
                self.buffer.clear();
            } else if let Some(status) = self.running_status {
                // Data byte with running status: add to buffer.
                self.buffer.push(byte);

                // Program change and channel pressure have one data byte.
                let status_type = status & 0xF0;
                let required_len = if matches!(status_type, 0xC0 | 0xD0) {
                    1
                } else {
                    2
                };

                if self.buffer.len() == required_len {
                    // Complete message: process it.
                    if status_type == 0xB0 {
                        // Control change message.
                        let channel = status & 0x0F;
                        let controller = self.buffer[0];
                        let value = self.buffer[1];
                        callback(ControlChange {
                            channel,
                            controller,
                            value,
                        });
                    }
                    self.buffer.clear();
                }
            }
        }
    }
}

impl Default for MidiParser {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_complete_message() {
        let mut parser = MidiParser::new();
        let mut messages = Vec::new();

        // Control change on channel 16, controller 1, value 1 (0xBF = 0xB0 | 0x0F).
        let bytes = [0xBF, 0x01, 0x01];
        parser.parse(&bytes, |cc| messages.push(cc));

        assert_eq!(messages.len(), 1);
        assert_eq!(messages[0].channel, 15);
        assert_eq!(messages[0].controller, 1);
        assert_eq!(messages[0].value, 1);
    }

    #[test]
    fn parse_message_split_over_three_reads() {
        let mut parser = MidiParser::new();
        let mut messages = Vec::new();

        parser.parse(&[0xBF], |cc| messages.push(cc));
        assert_eq!(messages.len(), 0);

        parser.parse(&[0x01], |cc| messages.push(cc));
        assert_eq!(messages.len(), 0);

        parser.parse(&[0x01], |cc| messages.push(cc));
        assert_eq!(messages.len(), 1);
        assert_eq!(messages[0].channel, 15);
        assert_eq!(messages[0].controller, 1);
        assert_eq!(messages[0].value, 1);
    }

    #[test]
    fn parse_running_status() {
        let mut parser = MidiParser::new();
        let mut messages = Vec::new();

        // Send status once, then two messages with running status.
        let bytes = [0xBF, 0x01, 0x01, 0x02, 0x02];
        parser.parse(&bytes, |cc| messages.push(cc));

        assert_eq!(messages.len(), 2);
        assert_eq!(messages[0].controller, 1);
        assert_eq!(messages[0].value, 1);
        assert_eq!(messages[1].controller, 2);
        assert_eq!(messages[1].value, 2);
    }

    #[test]
    fn ignore_real_time_byte_inside_message() {
        let mut parser = MidiParser::new();
        let mut messages = Vec::new();

        // Status, data byte, real-time (0xF8), data byte.
        let bytes = [0xBF, 0x01, 0xF8, 0x01];
        parser.parse(&bytes, |cc| messages.push(cc));

        assert_eq!(messages.len(), 1);
        assert_eq!(messages[0].controller, 1);
        assert_eq!(messages[0].value, 1);
    }

    #[test]
    fn skip_sysex_block() {
        let mut parser = MidiParser::new();
        let mut messages = Vec::new();

        // SysEx start, data bytes, SysEx end, then a control change.
        let bytes = [0xF0, 0x7E, 0x00, 0x09, 0x01, 0xF7, 0xBF, 0x01, 0x01];
        parser.parse(&bytes, |cc| messages.push(cc));

        assert_eq!(messages.len(), 1);
        assert_eq!(messages[0].controller, 1);
    }

    #[test]
    fn ignore_note_on_message() {
        let mut parser = MidiParser::new();
        let mut messages = Vec::new();

        // Note-on on channel 1 (0x90), note 60, velocity 100.
        let bytes = [0x90, 0x3C, 0x64];
        parser.parse(&bytes, |cc| messages.push(cc));

        assert_eq!(messages.len(), 0);
    }
}
