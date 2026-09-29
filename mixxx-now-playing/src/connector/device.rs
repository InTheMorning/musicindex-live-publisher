//! Raw MIDI device reader and management.
//!
//! Handles opening and reading from the raw MIDI device, parsing messages,
//! and coordinating with the poll loop through channels.

use std::fs::{File, OpenOptions, read_link};
use std::io::{ErrorKind, Read};
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

use anyhow::{Context, Result};

use super::midi::{ControlChange, MidiParser};

/// Request sent to Mixxx to get the complete state of all decks.
pub const STATE_REQUEST: [u8; 3] = [0xBF, 0x01, 0x01];

/// Events sent from the device reader thread to the poll loop.
#[derive(Debug)]
pub enum DeviceEvent {
    /// The device was successfully opened and is ready to read.
    DeviceOpened(File),
    /// A control change was received from the device.
    ControlChange { cc: ControlChange, time: Instant },
    /// The device was closed or an error occurred.
    DeviceClosed,
    /// The device could not be opened or resolved.
    DeviceUnavailable(String),
}

/// Resolves the raw MIDI device path for a given card ID.
///
/// Reads the symbolic link at `proc_asound/<card_id>`, which should point to
/// a card directory like `card31`. Returns the path `dev_snd/midiC{N}D0` where
/// N is the card number extracted from the link target.
///
/// # Errors
///
/// Returns an error if the link cannot be read or its format is unexpected.
pub fn resolve_raw_device(proc_asound: &Path, dev_snd: &Path, card_id: &str) -> Result<PathBuf> {
    let link_path = proc_asound.join(card_id);
    let link_target =
        read_link(&link_path).with_context(|| format!("read /proc/asound/{} link", card_id))?;

    let target_name = link_target
        .file_name()
        .and_then(|n| n.to_str())
        .ok_or_else(|| anyhow::anyhow!("invalid link target for /proc/asound/{}", card_id))?;

    // Extract card number from "cardN".
    if !target_name.starts_with("card") {
        return Err(anyhow::anyhow!(
            "unexpected link target format for /proc/asound/{}: {}",
            card_id,
            target_name
        ));
    }

    let card_num = target_name[4..]
        .parse::<u32>()
        .with_context(|| format!("parse card number from {}", target_name))?;

    Ok(dev_snd.join(format!("midiC{}D0", card_num)))
}

/// Parses MIDI bytes from a reader and sends each control change.
///
/// Each control change goes to `events` with the time of its read. After each
/// read that gave a control change, a wake-up goes to `wake`, so the poll loop
/// does not wait for its next pass. Returns at end of file.
///
/// # Errors
///
/// Returns an error when a read fails.
pub fn pump(
    mut reader: impl Read,
    events: &mpsc::Sender<DeviceEvent>,
    wake: &mpsc::Sender<()>,
) -> Result<()> {
    let mut parser = MidiParser::new();
    let mut buffer = [0u8; 256];
    loop {
        let count = match reader.read(&mut buffer) {
            Ok(0) => return Ok(()),
            Ok(count) => count,
            Err(error) if error.kind() == ErrorKind::Interrupted => continue,
            Err(error) => return Err(error).context("read from MIDI device"),
        };
        let time = Instant::now();
        let mut received = false;
        parser.parse(&buffer[..count], |cc| {
            received = true;
            // A dropped receiver means that the producer stops.
            let _ = events.send(DeviceEvent::ControlChange { cc, time });
        });
        if received {
            let _ = wake.send(());
        }
    }
}

/// Where the reader thread finds the raw MIDI device.
#[derive(Debug, Clone)]
pub struct DeviceLocation {
    /// The `/proc/asound` directory.
    pub proc_asound: PathBuf,
    /// The `/dev/snd` directory.
    pub dev_snd: PathBuf,
    /// The card ID, for example `V4V`.
    pub card_id: String,
}

impl DeviceLocation {
    /// The system location of the card with this ID.
    pub fn system(card_id: impl Into<String>) -> Self {
        Self {
            proc_asound: PathBuf::from("/proc/asound"),
            dev_snd: PathBuf::from("/dev/snd"),
            card_id: card_id.into(),
        }
    }
}

/// The time between two attempts to open the device.
const REOPEN_DELAY: Duration = Duration::from_secs(5);

/// Starts the thread `connector-midi` that reads the raw MIDI device.
///
/// The thread opens the device for reading and writing. It sends
/// `DeviceOpened` with a write handle, sends each control change, and sends
/// `DeviceClosed` at end of file or on a read error. A failed lookup or open
/// sends `DeviceUnavailable`. After a close or a failure, the thread waits
/// 5 seconds and tries again. Each event also sends a wake-up. The thread
/// stops when the event receiver is dropped.
///
/// # Errors
///
/// Returns an error when the thread cannot start.
pub fn spawn_reader(
    location: DeviceLocation,
    events: mpsc::Sender<DeviceEvent>,
    wake: mpsc::Sender<()>,
) -> Result<thread::JoinHandle<()>> {
    thread::Builder::new()
        .name("connector-midi".to_owned())
        .spawn(move || {
            loop {
                let closed = match open_device(&location) {
                    Ok((reader, writer)) => {
                        if events.send(DeviceEvent::DeviceOpened(writer)).is_err() {
                            return;
                        }
                        let _ = wake.send(());
                        let _ = pump(reader, &events, &wake);
                        DeviceEvent::DeviceClosed
                    }
                    Err(error) => DeviceEvent::DeviceUnavailable(format!("{error:#}")),
                };
                if events.send(closed).is_err() {
                    return;
                }
                let _ = wake.send(());
                thread::sleep(REOPEN_DELAY);
            }
        })
        .context("spawn connector-midi thread")
}

fn open_device(location: &DeviceLocation) -> Result<(File, File)> {
    let path = resolve_raw_device(&location.proc_asound, &location.dev_snd, &location.card_id)?;
    let reader = OpenOptions::new()
        .read(true)
        .write(true)
        .open(&path)
        .with_context(|| format!("open MIDI device {}", path.display()))?;
    let writer = reader
        .try_clone()
        .with_context(|| format!("clone MIDI device handle {}", path.display()))?;
    Ok((reader, writer))
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn resolve_raw_device_follows_symlink() -> Result<()> {
        let temp = TempDir::new()?;
        let proc_asound = temp.path().join("proc/asound");
        let dev_snd = temp.path().join("dev/snd");
        std::fs::create_dir_all(&proc_asound)?;
        std::fs::create_dir_all(&dev_snd)?;

        // Create a symlink from V4V to card31.
        #[cfg(unix)]
        {
            use std::os::unix::fs::symlink;
            symlink("card31", proc_asound.join("V4V"))?;
        }

        let result = resolve_raw_device(&proc_asound, &dev_snd, "V4V")?;
        assert_eq!(result, dev_snd.join("midiC31D0"));
        Ok(())
    }

    #[test]
    fn resolve_raw_device_missing_link_is_error() {
        let temp = TempDir::new().expect("create temp dir");
        let proc_asound = temp.path().join("proc/asound");
        let dev_snd = temp.path().join("dev/snd");
        std::fs::create_dir_all(&proc_asound).expect("create proc_asound");
        std::fs::create_dir_all(&dev_snd).expect("create dev_snd");

        let result = resolve_raw_device(&proc_asound, &dev_snd, "NonExistent");
        assert!(result.is_err());
        let error_msg = format!("{}", result.unwrap_err());
        assert!(error_msg.contains("NonExistent"));
    }

    #[test]
    fn pump_sends_control_changes() -> Result<()> {
        let (sender, receiver) = mpsc::channel();
        let (wake, woken) = mpsc::channel();
        let data = [0xBF, 0x01, 0x01, 0xBF, 0x02, 0x02];

        pump(&data[..], &sender, &wake)?;
        assert!(
            woken.try_recv().is_ok(),
            "a read with messages wakes the loop"
        );

        let event1 = receiver.recv().context("receive first event")?;
        match event1 {
            DeviceEvent::ControlChange { cc, time: _ } => {
                assert_eq!(cc.channel, 15);
                assert_eq!(cc.controller, 1);
                assert_eq!(cc.value, 1);
            }
            _ => panic!("expected ControlChange"),
        }

        let event2 = receiver.recv().context("receive second event")?;
        match event2 {
            DeviceEvent::ControlChange { cc, time: _ } => {
                assert_eq!(cc.controller, 2);
                assert_eq!(cc.value, 2);
            }
            _ => panic!("expected ControlChange"),
        }

        Ok(())
    }

    #[test]
    fn pump_returns_at_end_of_file() -> Result<()> {
        let (sender, _receiver) = mpsc::channel();
        let (wake, _woken) = mpsc::channel();
        let data = [0xBF, 0x01, 0x01];

        pump(&data[..], &sender, &wake)?;
        Ok(())
    }
}
