use std::fs::File;

use anyhow::{Context, Result};
use musicindex_live_publisher::{LOCK_FILE_NAME, ProducerState, probe_producer};
use tempfile::TempDir;

#[test]
fn probe_with_no_lock_file_gives_missing() -> Result<()> {
    let temp = TempDir::new()?;

    assert_eq!(probe_producer(temp.path())?, ProducerState::Missing);
    Ok(())
}

#[test]
fn probe_with_a_lock_file_with_no_holder_gives_missing() -> Result<()> {
    let temp = TempDir::new()?;
    File::create(temp.path().join(LOCK_FILE_NAME))?;

    assert_eq!(probe_producer(temp.path())?, ProducerState::Missing);
    Ok(())
}

#[test]
fn probe_with_an_exclusive_lock_from_a_second_open_file_gives_running() -> Result<()> {
    let temp = TempDir::new()?;
    let path = temp.path().join(LOCK_FILE_NAME);
    let holder = File::create(&path)?;
    holder
        .lock()
        .context("hold the exclusive lock in the test")?;

    assert_eq!(probe_producer(temp.path())?, ProducerState::Running);

    drop(holder);
    Ok(())
}

#[test]
fn probe_never_creates_the_lock_file() -> Result<()> {
    let temp = TempDir::new()?;

    probe_producer(temp.path())?;

    assert!(!temp.path().join(LOCK_FILE_NAME).exists());
    Ok(())
}

/// Gives the CPU time of a process in clock ticks, from `/proc/<pid>/stat`.
fn cpu_ticks(pid: u32) -> Result<u64> {
    let stat = std::fs::read_to_string(format!("/proc/{pid}/stat"))?;
    // The fields after the process name start after its closing parenthesis.
    let fields: Vec<&str> = stat
        .rsplit_once(')')
        .context("parse /proc stat")?
        .1
        .split_whitespace()
        .collect();
    // utime and stime are fields 14 and 15 of the stat line.
    let utime: u64 = fields.get(11).context("utime")?.parse()?;
    let stime: u64 = fields.get(12).context("stime")?.parse()?;
    Ok(utime + stime)
}

/// Incident of 2026-09-30: the probe opened `.producer.lock`, the watcher
/// reported that open as an event, and each event started a new probe. The
/// watch loop then used a full CPU core while a producer held the lock.
#[test]
fn the_watch_loop_is_idle_while_a_producer_holds_the_lock() -> Result<()> {
    use std::io::Read;
    use std::process::{Command, Stdio};
    use std::time::Duration;

    let temp = TempDir::new()?;
    let watch_dir = temp.path().join("drop");
    std::fs::create_dir(&watch_dir)?;
    let token = temp.path().join("test.token");
    std::fs::write(&token, "test-token")?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&token, std::fs::Permissions::from_mode(0o600))?;
    }
    let config = temp.path().join("config.toml");
    std::fs::write(
        &config,
        format!(
            "watch_dir = \"{}\"\nendpoint = \"https://api.example.test\"\n\n[[target]]\nname = \"default\"\nevent_id = \"event-default\"\ntoken_file = \"{}\"\n",
            watch_dir.display(),
            token.display()
        ),
    )?;
    let holder = File::create(watch_dir.join(LOCK_FILE_NAME))?;
    holder
        .lock()
        .context("hold the exclusive lock in the test")?;

    let mut child = Command::new(env!("CARGO_BIN_EXE_musicindex-live-publisher"))
        .arg("--dry-run")
        .arg("--config")
        .arg(&config)
        .env_remove("RUST_LOG")
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()?;
    std::thread::sleep(Duration::from_secs(3));

    let exited = child.try_wait()?;
    let ticks = match exited {
        None => cpu_ticks(child.id()),
        Some(_) => Ok(0),
    };
    let _ = child.kill();
    let status = child.wait()?;
    if exited.is_some() {
        let mut stderr = String::new();
        if let Some(mut pipe) = child.stderr.take() {
            pipe.read_to_string(&mut stderr)?;
        }
        anyhow::bail!("the publisher exited early with {status}: {stderr}");
    }
    let ticks = ticks?;

    // A busy loop uses about 100 ticks each second. An idle loop uses almost
    // none after its startup.
    assert!(
        ticks < 50,
        "the watch loop used {ticks} clock ticks of CPU in 3 seconds"
    );
    drop(holder);
    Ok(())
}
