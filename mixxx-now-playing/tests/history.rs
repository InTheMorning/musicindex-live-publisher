mod common;

use std::path::Path;

use anyhow::Result;
use mixxx_now_playing::history::HistoryWatcher;

use common::SyntheticMixxxDb;

#[test]
fn history_empty_table_returns_none() -> Result<()> {
    let db = SyntheticMixxxDb::new()?;
    let mut watcher = HistoryWatcher::open(db.path())?;

    assert_eq!(watcher.poll()?, None);
    Ok(())
}

#[test]
fn history_repeated_poll_returns_none_until_hist_id_changes() -> Result<()> {
    let mut db = SyntheticMixxxDb::new()?;
    let first_path = Path::new("/music/first.mp3");
    db.append_history_row_with_metadata(Some("Artist"), Some("Title"), first_path)?;
    let mut watcher = HistoryWatcher::open(db.path())?;

    let first = watcher.poll()?;
    assert_eq!(
        first.as_ref().map(|row| row.path.as_path()),
        Some(first_path)
    );
    assert_eq!(watcher.poll()?, None);

    let second_path = Path::new("/music/second.mp3");
    db.append_history_row_with_metadata(Some("Artist"), Some("Title"), second_path)?;

    let second = watcher.poll()?;
    assert_eq!(
        second.as_ref().map(|row| row.path.as_path()),
        Some(second_path)
    );
    assert_eq!(watcher.poll()?, None);
    Ok(())
}

#[test]
fn history_null_artist_and_title_are_empty_strings() -> Result<()> {
    let mut db = SyntheticMixxxDb::new()?;
    db.append_history_row_with_metadata(None, None, Path::new("/music/nulls.flac"))?;
    let mut watcher = HistoryWatcher::open(db.path())?;

    let row = watcher.poll()?.expect("history row should be emitted");

    assert_eq!(row.artist, "");
    assert_eq!(row.title, "");
    assert_eq!(row.path, Path::new("/music/nulls.flac"));
    Ok(())
}

#[test]
fn history_null_location_still_reports_the_newest_track() -> Result<()> {
    let mut db = SyntheticMixxxDb::new()?;
    db.append_history_row_with_metadata(
        Some("Artist"),
        Some("Good Song"),
        Path::new("/music/ok.mp3"),
    )?;
    let orphan_id =
        db.append_history_row_without_location(Some("Orphan"), Some("Orphan Song"), None)?;
    let mut watcher = HistoryWatcher::open(db.path())?;

    let row = watcher
        .poll()?
        .expect("newest history row should be emitted");

    assert_eq!(row.hist_id, orphan_id);
    assert_eq!(row.title, "Orphan Song");
    assert_eq!(row.path, Path::new(""));
    Ok(())
}

#[test]
fn history_dangling_location_behaves_like_a_null_location() -> Result<()> {
    let mut db = SyntheticMixxxDb::new()?;
    db.append_history_row_with_metadata(
        Some("Artist"),
        Some("Good Song"),
        Path::new("/music/ok.mp3"),
    )?;
    let orphan_id =
        db.append_history_row_without_location(Some("Orphan"), Some("Orphan Song"), Some(9_999))?;
    let mut watcher = HistoryWatcher::open(db.path())?;

    let row = watcher
        .poll()?
        .expect("newest history row should be emitted");

    assert_eq!(row.hist_id, orphan_id);
    assert_eq!(row.title, "Orphan Song");
    assert_eq!(row.path, Path::new(""));
    Ok(())
}

#[test]
fn history_unresolvable_location_is_not_a_v4v_track() -> Result<()> {
    let mut db = SyntheticMixxxDb::new()?;
    db.append_history_row_without_location(Some("Orphan"), Some("Orphan Song"), None)?;
    let mut watcher = HistoryWatcher::open(db.path())?;

    let row = watcher.poll()?.expect("history row should be emitted");

    assert!(!mixxx_now_playing::classify::is_v4v_track(
        &row.path,
        Path::new("/music")
    ));
    Ok(())
}
