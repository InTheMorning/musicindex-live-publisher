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

    db.append_history_row_with_metadata(Some("Test-Artist"), Some("Test-Title"), None, &first_v4v)?;
    run_once(db.path(), &txt_file, &metadata_file, &v4v_root)?;
    assert_eq!(fs::read_to_string(&txt_file)?, "TestArtist - TestTitle");
    assert!(metadata_file.exists());
    assert!(fs::read_to_string(&metadata_file)?.contains("[MusicIndex]\n"));

    db.append_history_row_with_metadata(Some("Plain Artist"), Some("Plain Title"), None, &non_v4v)?;
    run_once(db.path(), &txt_file, &metadata_file, &v4v_root)?;
    assert_eq!(fs::read_to_string(&txt_file)?, "Plain Artist - Plain Title");
    assert!(!metadata_file.exists());

    db.append_history_row_with_metadata(
        Some("Next Artist"),
        Some("Next Title"),
        None,
        &second_v4v,
    )?;
    run_once(db.path(), &txt_file, &metadata_file, &v4v_root)?;
    assert_eq!(fs::read_to_string(&txt_file)?, "Next Artist - Next Title");
    assert!(metadata_file.exists());
    assert!(fs::read_to_string(&metadata_file)?.contains("Value Routes = embedded-id3"));

    Ok(())
}

fn read_json_drop_file(
    db_path: &Path,
    txt_file: &Path,
    metadata_file: &Path,
    v4v_root: &Path,
) -> Result<serde_json::Value> {
    let output = Command::new(binary())
        .arg("--once")
        .arg("--format")
        .arg("json")
        .arg("--db-file")
        .arg(db_path)
        .arg("--txt-file")
        .arg(txt_file)
        .arg("--id3-file")
        .arg(metadata_file)
        .arg("--v4v-root")
        .arg(v4v_root)
        .output()?;
    if !output.status.success() {
        return Err(anyhow!(
            "mixxx-now-playing failed: {}",
            String::from_utf8_lossy(&output.stderr)
        ));
    }
    Ok(serde_json::from_str(&fs::read_to_string(metadata_file)?)?)
}

#[test]
fn lifecycle_two_plays_of_one_track_give_two_play_ids() -> Result<()> {
    let temp = TempDir::new()?;
    let v4v_root = temp.path().join("V4Vmusic");
    fs::create_dir_all(&v4v_root)?;
    let track = v4v_root.join("track.mp3");
    fs::copy(fixture("musicindex-tagged.mp3"), &track)?;
    let txt_file = temp.path().join("now-playing.txt");
    let metadata_file = temp.path().join("metadata.json");
    let mut db = SyntheticMixxxDb::new()?;

    db.append_history_row_with_metadata(Some("Artist"), Some("Title"), Some("Album"), &track)?;
    let first = read_json_drop_file(db.path(), &txt_file, &metadata_file, &v4v_root)?;
    db.append_history_row_with_metadata(Some("Artist"), Some("Title"), Some("Album"), &track)?;
    let second = read_json_drop_file(db.path(), &txt_file, &metadata_file, &v4v_root)?;

    assert_eq!(first["schema"], "musicindex.nowplaying/2");
    assert_eq!(first["album"], "Album");
    assert_eq!(second["album"], "Album");
    let first_id = first["play_id"]
        .as_str()
        .ok_or_else(|| anyhow!("no play_id"))?;
    let second_id = second["play_id"]
        .as_str()
        .ok_or_else(|| anyhow!("no play_id"))?;
    assert!(first_id.parse::<i64>().is_ok(), "{first_id}");
    assert_ne!(first_id, second_id);
    Ok(())
}

#[test]
fn lifecycle_startup_writes_empty_now_playing_and_clears_stale_metadata() -> Result<()> {
    let temp = TempDir::new()?;
    let v4v_root = temp.path().join("V4Vmusic");
    fs::create_dir_all(&v4v_root)?;
    let txt_file = temp.path().join("now-playing.txt");
    let metadata_file = temp.path().join("metadata.txt");
    fs::write(&txt_file, "stale track")?;
    fs::write(&metadata_file, "stale metadata")?;
    let db = SyntheticMixxxDb::new()?;

    run_once(db.path(), &txt_file, &metadata_file, &v4v_root)?;

    // ADR 0008 §The Song File For `butt`: the producer never deletes the
    // song file. With no history row, startup leaves it with no text.
    assert!(txt_file.exists());
    assert_eq!(fs::read_to_string(&txt_file)?, "");
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
fn lifecycle_sigterm_clears_now_playing_and_removes_metadata_before_exit() -> Result<()> {
    let temp = TempDir::new()?;
    let v4v_root = temp.path().join("V4Vmusic");
    fs::create_dir_all(&v4v_root)?;
    let track = v4v_root.join("track.mp3");
    fs::copy(fixture("musicindex-tagged.mp3"), &track)?;

    let txt_file = temp.path().join("now-playing.txt");
    let metadata_file = temp.path().join("metadata.txt");
    let mut db = SyntheticMixxxDb::new()?;
    db.append_history_row_with_metadata(Some("Signal Artist"), Some("Signal Title"), None, &track)?;

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
        .arg("--no-connector")
        .spawn()?;

    wait_until(Duration::from_secs(5), || metadata_file.exists())?;
    // The daemon has written the track line by now. ADR 0008 §The Song File
    // For `butt` still applies here. A title line is correct while a track
    // plays. Only the exit must leave the file empty.
    assert_eq!(
        fs::read_to_string(&txt_file)?,
        "Signal Artist - Signal Title"
    );
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
    // ADR 0008 §The Song File For `butt`: the song file is never deleted.
    // A SIGTERM exit leaves it present with no text.
    assert!(txt_file.exists());
    assert_eq!(fs::read_to_string(&txt_file)?, "");
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

fn run_once_with_display(
    db_path: &Path,
    txt_file: &Path,
    metadata_file: &Path,
    v4v_root: &Path,
    display_dir: &Path,
) -> Result<std::process::Output> {
    Ok(Command::new(binary())
        .arg("--once")
        .arg("--db-file")
        .arg(db_path)
        .arg("--txt-file")
        .arg(txt_file)
        .arg("--id3-file")
        .arg(metadata_file)
        .arg("--v4v-root")
        .arg(v4v_root)
        .arg("--display-dir")
        .arg(display_dir)
        .output()?)
}

#[test]
fn lifecycle_display_pairs_with_the_song_file_and_the_drop_file() -> Result<()> {
    let temp = TempDir::new()?;
    let v4v_root = temp.path().join("V4Vmusic");
    let display_dir = temp.path().join("display");
    fs::create_dir_all(&v4v_root)?;
    fs::create_dir_all(&display_dir)?;
    let track = v4v_root.join("track.mp3");
    fs::copy(fixture("musicindex-tagged.mp3"), &track)?;
    let txt_file = temp.path().join("now-playing.txt");
    let metadata_file = temp.path().join("metadata.json");
    let mut db = SyntheticMixxxDb::new()?;

    // A hyphen in the artist and the title, so the format rules apply.
    db.append_history_row_with_metadata(Some("A-ha"), Some("Take-On Me"), None, &track)?;
    let output = Command::new(binary())
        .arg("--once")
        .arg("--format")
        .arg("json")
        .arg("--db-file")
        .arg(db.path())
        .arg("--txt-file")
        .arg(&txt_file)
        .arg("--id3-file")
        .arg(&metadata_file)
        .arg("--v4v-root")
        .arg(&v4v_root)
        .arg("--display-dir")
        .arg(&display_dir)
        .output()?;
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );

    let display = read_display(&display_dir)?;
    let drop_file: serde_json::Value = serde_json::from_str(&fs::read_to_string(&metadata_file)?)?;
    let song_file = fs::read_to_string(&txt_file)?;
    assert_eq!(display["schema"], "musicindex.display/3");
    assert_eq!(
        display["track"]["song_line"].as_str(),
        song_file.lines().next()
    );
    assert_ne!(display["track"]["song_line"], "A-ha - Take-On Me");
    assert!(display["track"]["play_id"].is_string());
    assert_eq!(display["track"]["play_id"], drop_file["play_id"]);
    Ok(())
}

fn read_display(display_dir: &Path) -> Result<serde_json::Value> {
    Ok(serde_json::from_str(&fs::read_to_string(
        display_dir.join("display.json"),
    )?)?)
}

#[test]
fn lifecycle_display_dir_equal_to_drop_dir_fails_at_startup() -> Result<()> {
    let temp = TempDir::new()?;
    let v4v_root = temp.path().join("V4Vmusic");
    let drop_dir = temp.path().join("drop");
    fs::create_dir_all(&v4v_root)?;
    fs::create_dir_all(&drop_dir)?;
    let db = SyntheticMixxxDb::new()?;

    let output = run_once_with_display(
        db.path(),
        &temp.path().join("now-playing.txt"),
        &drop_dir.join("metadata.txt"),
        &v4v_root,
        &drop_dir,
    )?;

    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("ADR 0008"), "{stderr}");
    assert!(!drop_dir.join("display.json").exists());
    Ok(())
}

#[test]
fn lifecycle_display_startup_writes_null_track() -> Result<()> {
    let temp = TempDir::new()?;
    let v4v_root = temp.path().join("V4Vmusic");
    let display_dir = temp.path().join("display");
    fs::create_dir_all(&v4v_root)?;
    fs::create_dir_all(&display_dir)?;
    fs::write(
        display_dir.join("display.json"),
        r#"{"schema": "musicindex.display/3", "track": {"artist": "Old", "title": "Old", "artwork": null, "song_line": "Old - Old", "play_id": "1", "album": null}}"#,
    )?;
    let db = SyntheticMixxxDb::new()?;

    let output = run_once_with_display(
        db.path(),
        &temp.path().join("now-playing.txt"),
        &temp.path().join("metadata.txt"),
        &v4v_root,
        &display_dir,
    )?;

    assert!(output.status.success());
    assert_eq!(
        read_display(&display_dir)?,
        serde_json::json!({"schema": "musicindex.display/3", "track": null})
    );
    Ok(())
}

#[test]
fn lifecycle_display_writes_the_track_and_its_embedded_image() -> Result<()> {
    let temp = TempDir::new()?;
    let v4v_root = temp.path().join("V4Vmusic");
    let other_root = temp.path().join("OtherMusic");
    let display_dir = temp.path().join("display");
    fs::create_dir_all(&v4v_root)?;
    fs::create_dir_all(&other_root)?;
    let v4v_track = v4v_root.join("track.mp3");
    let plain_track = other_root.join("plain.flac");
    fs::copy(fixture("musicindex-tagged.mp3"), &v4v_track)?;
    fs::copy(fixture("untagged.flac"), &plain_track)?;
    let txt_file = temp.path().join("now-playing.txt");
    let metadata_file = temp.path().join("metadata.txt");
    let mut db = SyntheticMixxxDb::new()?;

    // A V4V track with no image URL uses its embedded image.
    db.append_history_row_with_metadata(Some("Test-Artist"), Some("Test-Title"), None, &v4v_track)?;
    let output = run_once_with_display(
        db.path(),
        &txt_file,
        &metadata_file,
        &v4v_root,
        &display_dir,
    )?;
    assert!(output.status.success());
    let display = read_display(&display_dir)?;
    assert_eq!(display["schema"], "musicindex.display/3");
    // The display state holds the raw fields. The hyphen removal of the
    // song file does not apply.
    assert_eq!(display["track"]["artist"], "Test-Artist");
    assert_eq!(display["track"]["title"], "Test-Title");
    // Version 2 includes song_line and play_id. The song_line uses the
    // strip_hyphens setting (default true), which removes hyphens.
    assert_eq!(display["track"]["song_line"], "TestArtist - TestTitle");
    assert_eq!(display["track"]["play_id"], "1");
    assert_eq!(display["track"]["artwork"]["mime"], "image/jpeg");
    let sha256 = display["track"]["artwork"]["sha256"]
        .as_str()
        .ok_or_else(|| anyhow!("artwork has no sha256"))?;
    let image = fs::read(display_dir.join(format!("{sha256}.jpg")))?;
    assert_eq!(mixxx_now_playing::display::sha256_hex(&image), sha256);
    assert!(metadata_file.exists());

    // A track that is not V4V uses its embedded image too.
    let other_with_picture = other_root.join("other.mp3");
    fs::copy(fixture("musicindex-tagged.mp3"), &other_with_picture)?;
    db.append_history_row_with_metadata(Some("Other"), Some("Song"), None, &other_with_picture)?;
    let output = run_once_with_display(
        db.path(),
        &txt_file,
        &metadata_file,
        &v4v_root,
        &display_dir,
    )?;
    assert!(output.status.success());
    let display = read_display(&display_dir)?;
    assert_eq!(display["track"]["artist"], "Other");
    assert_eq!(display["track"]["artwork"]["sha256"], sha256);
    assert!(!metadata_file.exists());

    // A track with no picture gives the `null` artwork.
    db.append_history_row_with_metadata(Some("Plain"), Some("Track"), None, &plain_track)?;
    let output = run_once_with_display(
        db.path(),
        &txt_file,
        &metadata_file,
        &v4v_root,
        &display_dir,
    )?;
    assert!(output.status.success());
    let display = read_display(&display_dir)?;
    assert_eq!(display["track"]["artist"], "Plain");
    assert_eq!(display["track"]["artwork"], serde_json::Value::Null);
    Ok(())
}

#[test]
fn lifecycle_sigterm_writes_null_display_before_exit() -> Result<()> {
    let temp = TempDir::new()?;
    let v4v_root = temp.path().join("V4Vmusic");
    let display_dir = temp.path().join("display");
    fs::create_dir_all(&v4v_root)?;
    let track = v4v_root.join("track.mp3");
    fs::copy(fixture("musicindex-tagged.mp3"), &track)?;
    let txt_file = temp.path().join("now-playing.txt");
    let metadata_file = temp.path().join("metadata.txt");
    let mut db = SyntheticMixxxDb::new()?;
    db.append_history_row_with_metadata(Some("Signal Artist"), Some("Signal Title"), None, &track)?;

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
        .arg("--display-dir")
        .arg(&display_dir)
        .arg("--poll-secs")
        .arg("0.05")
        .arg("--no-connector")
        .spawn()?;

    let shows_track = || {
        read_display(&display_dir)
            .map(|display| display["track"]["artist"] == "Signal Artist")
            .unwrap_or(false)
    };
    let waited = wait_until(Duration::from_secs(5), shows_track);
    let status = Command::new("kill")
        .arg("-TERM")
        .arg(child.id().to_string())
        .status()?;
    let exit = child.wait()?;
    let _ = fake_mixxx.kill();
    let _ = fake_mixxx.wait();
    waited?;

    assert!(status.success());
    assert!(exit.success());
    assert_eq!(
        read_display(&display_dir)?,
        serde_json::json!({"schema": "musicindex.display/3", "track": null})
    );
    Ok(())
}

#[test]
fn lifecycle_display_v4v_track_with_image_tag_gives_the_url() -> Result<()> {
    use lofty::config::WriteOptions;
    use lofty::prelude::*;
    use lofty::tag::{ItemKey, ItemValue, TagItem};

    let temp = TempDir::new()?;
    let v4v_root = temp.path().join("V4Vmusic");
    let display_dir = temp.path().join("display");
    fs::create_dir_all(&v4v_root)?;
    let track = v4v_root.join("track.mp3");
    fs::copy(fixture("musicindex-tagged.mp3"), &track)?;
    // `Tag::insert` drops a new `TXXX` frame that has no key for the tag
    // type, so the test uses `insert_unchecked`.
    let mut tagged = lofty::read_from_path(&track)?;
    tagged
        .primary_tag_mut()
        .ok_or_else(|| anyhow!("the MP3 fixture has a primary tag"))?
        .insert_unchecked(TagItem::new(
            ItemKey::Unknown("MusicIndex Image".to_owned()),
            ItemValue::Text("https://example.test/cover.gif".to_owned()),
        ));
    tagged.save_to_path(&track, WriteOptions::default())?;
    let mut db = SyntheticMixxxDb::new()?;
    db.append_history_row_with_metadata(Some("Url Artist"), Some("Url Title"), None, &track)?;

    let output = run_once_with_display(
        db.path(),
        &temp.path().join("now-playing.txt"),
        &temp.path().join("metadata.txt"),
        &v4v_root,
        &display_dir,
    )?;

    assert!(output.status.success());
    let display = read_display(&display_dir)?;
    assert_eq!(display["track"]["artist"], "Url Artist");
    assert_eq!(
        display["track"]["artwork"],
        serde_json::json!({"url": "https://example.test/cover.gif"})
    );
    let names: Vec<String> = fs::read_dir(&display_dir)?
        .filter_map(|entry| entry.ok()?.file_name().into_string().ok())
        .collect();
    assert_eq!(names, vec!["display.json".to_owned()]);
    Ok(())
}

#[test]
fn lifecycle_display_v4v_track_with_album() -> Result<()> {
    let temp = TempDir::new()?;
    let v4v_root = temp.path().join("V4Vmusic");
    let display_dir = temp.path().join("display");
    fs::create_dir_all(&v4v_root)?;
    fs::create_dir_all(&display_dir)?;
    let track = v4v_root.join("track.mp3");
    fs::copy(fixture("musicindex-tagged.mp3"), &track)?;
    let mut db = SyntheticMixxxDb::new()?;

    db.append_history_row_with_metadata(
        Some("Album Artist"),
        Some("Album Track"),
        Some("Test Album"),
        &track,
    )?;
    let output = Command::new(binary())
        .arg("--once")
        .arg("--db-file")
        .arg(db.path())
        .arg("--txt-file")
        .arg(temp.path().join("now-playing.txt"))
        .arg("--id3-file")
        .arg(temp.path().join("metadata.txt"))
        .arg("--v4v-root")
        .arg(&v4v_root)
        .arg("--display-dir")
        .arg(&display_dir)
        .output()?;

    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let display = read_display(&display_dir)?;
    assert_eq!(display["track"]["album"], "Test Album");
    Ok(())
}

#[test]
fn lifecycle_display_non_v4v_track_with_album() -> Result<()> {
    let temp = TempDir::new()?;
    let other_root = temp.path().join("OtherMusic");
    let display_dir = temp.path().join("display");
    fs::create_dir_all(&other_root)?;
    fs::create_dir_all(&display_dir)?;
    let track = other_root.join("track.mp3");
    fs::copy(fixture("untagged.flac"), &track)?;
    let mut db = SyntheticMixxxDb::new()?;

    db.append_history_row_with_metadata(
        Some("Non-V4V Artist"),
        Some("Non-V4V Track"),
        Some("Non-V4V Album"),
        &track,
    )?;
    let output = Command::new(binary())
        .arg("--once")
        .arg("--db-file")
        .arg(db.path())
        .arg("--txt-file")
        .arg(temp.path().join("now-playing.txt"))
        .arg("--id3-file")
        .arg(temp.path().join("metadata.txt"))
        .arg("--display-dir")
        .arg(&display_dir)
        .output()?;

    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let display = read_display(&display_dir)?;
    assert_eq!(display["track"]["album"], "Non-V4V Album");
    Ok(())
}

#[test]
fn lifecycle_display_track_with_null_album() -> Result<()> {
    let temp = TempDir::new()?;
    let v4v_root = temp.path().join("V4Vmusic");
    let display_dir = temp.path().join("display");
    fs::create_dir_all(&v4v_root)?;
    fs::create_dir_all(&display_dir)?;
    let track = v4v_root.join("track.mp3");
    fs::copy(fixture("musicindex-tagged.mp3"), &track)?;
    let mut db = SyntheticMixxxDb::new()?;

    db.append_history_row_with_metadata(
        Some("No Album Artist"),
        Some("No Album Track"),
        None,
        &track,
    )?;
    let output = Command::new(binary())
        .arg("--once")
        .arg("--db-file")
        .arg(db.path())
        .arg("--txt-file")
        .arg(temp.path().join("now-playing.txt"))
        .arg("--id3-file")
        .arg(temp.path().join("metadata.txt"))
        .arg("--v4v-root")
        .arg(&v4v_root)
        .arg("--display-dir")
        .arg(&display_dir)
        .output()?;

    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let display = read_display(&display_dir)?;
    assert!(display["track"]["album"].is_null());
    Ok(())
}
