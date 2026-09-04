mod common;

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Child, Command};
use std::thread;
use std::time::{Duration, Instant};

use anyhow::{Result, anyhow};
use mixxx_now_playing::render::render_now_playing_line;
use mixxx_now_playing::sink::{OutputFile, Presence};
use tempfile::TempDir;

use common::SyntheticMixxxDb;

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name)
}

fn binary() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_mixxx-now-playing"))
}

fn run_once(db_path: &Path, txt_file: &Path, metadata_file: &Path, v4v_root: &Path) -> Result<()> {
    let output = Command::new(binary())
        .arg("--once")
        .arg("--db-file")
        .arg(db_path)
        .arg("--txt-file")
        .arg(txt_file)
        .arg("--id3-file")
        .arg(metadata_file)
        .arg("--v4v-root")
        .arg(v4v_root)
        .output()?;

    if output.status.success() {
        return Ok(());
    }

    Err(anyhow!(
        "mixxx-now-playing failed: {}",
        String::from_utf8_lossy(&output.stderr)
    ))
}

#[test]
fn lifecycle_toggles_metadata_presence_for_v4v_and_non_v4v_tracks() -> Result<()> {
    let temp = TempDir::new()?;
    let v4v_root = temp.path().join("V4Vmusic");
    let other_root = temp.path().join("OtherMusic");
    fs::create_dir_all(&v4v_root)?;
    fs::create_dir_all(&other_root)?;

    let first_v4v = v4v_root.join("first.mp3");
    let second_v4v = v4v_root.join("second.flac");
    let non_v4v = other_root.join("plain.flac");
    fs::copy(fixture("musicindex-tagged.mp3"), &first_v4v)?;
    fs::copy(fixture("musicindex-tagged.flac"), &second_v4v)?;
    fs::copy(fixture("untagged.flac"), &non_v4v)?;

    let txt_file = temp.path().join("now-playing.txt");
    let metadata_file = temp.path().join("metadata.txt");
    let mut db = SyntheticMixxxDb::new()?;

    db.append_history_row_with_metadata(Some("Test-Artist"), Some("Test-Title"), &first_v4v)?;
    run_once(db.path(), &txt_file, &metadata_file, &v4v_root)?;
    assert_eq!(fs::read_to_string(&txt_file)?, "TestArtist - TestTitle");
    assert!(metadata_file.exists());
    assert!(fs::read_to_string(&metadata_file)?.contains("[MusicIndex]\n"));

    db.append_history_row_with_metadata(Some("Plain Artist"), Some("Plain Title"), &non_v4v)?;
    run_once(db.path(), &txt_file, &metadata_file, &v4v_root)?;
    assert_eq!(fs::read_to_string(&txt_file)?, "Plain Artist - Plain Title");
    assert!(!metadata_file.exists());

    db.append_history_row_with_metadata(Some("Next Artist"), Some("Next Title"), &second_v4v)?;
    run_once(db.path(), &txt_file, &metadata_file, &v4v_root)?;
    assert_eq!(fs::read_to_string(&txt_file)?, "Next Artist - Next Title");
    assert!(metadata_file.exists());
    assert!(fs::read_to_string(&metadata_file)?.contains("Value Routes = embedded-id3"));

    Ok(())
}

#[test]
fn lifecycle_startup_clears_stale_output_before_first_poll() -> Result<()> {
    let temp = TempDir::new()?;
    let v4v_root = temp.path().join("V4Vmusic");
    fs::create_dir_all(&v4v_root)?;
    let txt_file = temp.path().join("now-playing.txt");
    let metadata_file = temp.path().join("metadata.txt");
    fs::write(&txt_file, "stale track")?;
    fs::write(&metadata_file, "stale metadata")?;
    let db = SyntheticMixxxDb::new()?;

    run_once(db.path(), &txt_file, &metadata_file, &v4v_root)?;

    assert!(!txt_file.exists());
    assert!(!metadata_file.exists());
    Ok(())
}

#[test]
fn lifecycle_same_content_is_not_rewritten() -> Result<()> {
    let temp = TempDir::new()?;
    let path = temp.path().join("metadata.txt");
    let mut output = OutputFile::new(&path);

    output.set(Presence::Present("same".to_string()))?;
    let first_mtime = fs::metadata(&path)?.modified()?;
    output.set(Presence::Present("same".to_string()))?;

    assert_eq!(fs::metadata(&path)?.modified()?, first_mtime);
    Ok(())
}

#[test]
fn lifecycle_now_playing_line_matches_shell_script_bytes() -> Result<()> {
    let temp = TempDir::new()?;
    let actual_path = temp.path().join("actual.txt");
    let expected_path = temp.path().join("expected.txt");
    fs::write(
        &actual_path,
        render_now_playing_line("Test-Artist", "Test-Title", true),
    )?;
    let output = Command::new("bash")
        .arg("-lc")
        .arg(
            "LINE=\"$(printf '%s|%s\\n' 'Test-Artist' 'Test-Title' | sed 's/-//g' | sed 's/|/ - /g')\"; printf '%s' \"$LINE\"",
        )
        .output()?;
    assert!(output.status.success());
    fs::write(&expected_path, output.stdout)?;

    let status = Command::new("cmp")
        .arg("-s")
        .arg(&actual_path)
        .arg(&expected_path)
        .status()?;

    assert!(status.success());
    Ok(())
}

#[test]
fn lifecycle_sigterm_removes_metadata_before_exit() -> Result<()> {
    let temp = TempDir::new()?;
    let v4v_root = temp.path().join("V4Vmusic");
    fs::create_dir_all(&v4v_root)?;
    let track = v4v_root.join("track.mp3");
    fs::copy(fixture("musicindex-tagged.mp3"), &track)?;

    let txt_file = temp.path().join("now-playing.txt");
    let metadata_file = temp.path().join("metadata.txt");
    let mut db = SyntheticMixxxDb::new()?;
    db.append_history_row_with_metadata(Some("Signal Artist"), Some("Signal Title"), &track)?;

    let mut fake_mixxx = spawn_fake_mixxx(&temp)?;
    let mut child = Command::new(binary())
        .arg("--db-file")
        .arg(db.path())
        .arg("--txt-file")
        .arg(&txt_file)
        .arg("--id3-file")
        .arg(&metadata_file)
        .arg("--v4v-root")
        .arg(&v4v_root)
        .arg("--poll-secs")
        .arg("0.05")
        .spawn()?;

    wait_until(Duration::from_secs(5), || metadata_file.exists())?;
    let status = Command::new("kill")
        .arg("-TERM")
        .arg(child.id().to_string())
        .status()?;
    assert!(status.success());
    let exit = child.wait()?;
    let _ = fake_mixxx.kill();
    let _ = fake_mixxx.wait();

    assert!(exit.success());
    assert!(!metadata_file.exists());
    Ok(())
}

fn spawn_fake_mixxx(temp: &TempDir) -> Result<Child> {
    let fake_bin = temp.path().join("mixxx");
    std::os::unix::fs::symlink("/bin/sleep", &fake_bin)?;
    Ok(Command::new(fake_bin).arg("30").spawn()?)
}

fn wait_until(timeout: Duration, predicate: impl Fn() -> bool) -> Result<()> {
    let started = Instant::now();
    while started.elapsed() < timeout {
        if predicate() {
            return Ok(());
        }
        thread::sleep(Duration::from_millis(10));
    }
    Err(anyhow!("timed out waiting for condition"))
}
