use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use lofty::config::WriteOptions;
use lofty::prelude::*;
use lofty::tag::{ItemValue, TagItem};
use mixxx_now_playing::render::{TrackDisplay, render_metadata_json, render_metadata_text};
use mixxx_now_playing::tags::read_tags;
use tempfile::TempDir;

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name)
}

/// Copies the mp3 fixture into `dir` and writes a `TXXX:MusicIndex Image`
/// frame with `value` onto the copy, returning the copy's path.
///
/// `insert_unchecked` is required here, not `insert_text`: the checked
/// `Tag::insert` rejects an `ItemKey::Unknown` that has no mapping for the
/// target `TagType`, so it silently drops a brand new MusicIndex frame.
fn fixture_with_image_tag(dir: &Path, value: &str) -> Result<PathBuf> {
    let copy = dir.join("musicindex-tagged-with-image.mp3");
    std::fs::copy(fixture("musicindex-tagged.mp3"), &copy).context("copy mp3 fixture")?;

    let mut tagged_file =
        lofty::read_from_path(&copy).context("read mp3 fixture copy for tagging")?;
    let tag = tagged_file
        .primary_tag_mut()
        .context("mp3 fixture copy must carry a primary tag")?;
    tag.insert_unchecked(TagItem::new(
        ItemKey::Unknown("MusicIndex Image".to_string()),
        ItemValue::Text(value.to_string()),
    ));
    tagged_file
        .save_to_path(&copy, WriteOptions::default())
        .context("save mp3 fixture copy with image tag")?;

    Ok(copy)
}

#[test]
fn render_mp3_fixture_excludes_transcripts_and_binary_payloads() -> Result<()> {
    let tags = read_tags(&fixture("musicindex-tagged.mp3"))?;
    let rendered = render_metadata_text(TrackDisplay {
        artist: "Fixture Artist",
        title: "Fixture Title",
        album: None,
        play_id: None,
        tags: &tags,
    });

    assert!(rendered.contains("[MusicIndex]\n"));
    assert!(rendered.contains("Feed Guid"));
    assert!(rendered.contains("Value Routes = embedded-id3\n"));
    assert!(rendered.contains(r#"[{"recipient_name":"Alice","route_type":"node","split":90}]"#));
    assert!(rendered.contains("[Tags]\n"));
    assert!(rendered.contains("APIC"));
    assert!(!rendered.contains("transcript should not render"));
    assert!(!rendered.contains("/9j/"));

    Ok(())
}

#[test]
fn render_flac_fixture_uses_same_musicindex_labels() -> Result<()> {
    let tags = read_tags(&fixture("musicindex-tagged.flac"))?;
    let rendered = render_metadata_text(TrackDisplay {
        artist: "Fixture Artist",
        title: "Fixture Title",
        album: None,
        play_id: None,
        tags: &tags,
    });

    assert!(rendered.contains("Feed Guid"));
    assert!(rendered.contains("Track Guid"));
    assert!(rendered.contains("Publisher"));
    assert!(rendered.contains("Contributors"));
    assert!(rendered.contains("Value Routes = embedded-id3\n"));
    assert!(rendered.contains("Nostr Handle"));

    Ok(())
}

#[test]
fn render_drop_file_holds_the_image_tag_value() -> Result<()> {
    let temp = TempDir::new()?;
    let tagged = fixture_with_image_tag(temp.path(), "https://example.com/cover.jpg")?;
    let tags = read_tags(&tagged)?;

    let rendered = render_metadata_json(TrackDisplay {
        artist: "Fixture Artist",
        title: "Fixture Title",
        album: None,
        play_id: None,
        tags: &tags,
    })?;
    let value: serde_json::Value = serde_json::from_str(&rendered)?;

    assert_eq!(value["image"], "https://example.com/cover.jpg");

    Ok(())
}

#[test]
fn render_drop_file_gives_null_image_when_no_such_tag() -> Result<()> {
    let tags = read_tags(&fixture("musicindex-tagged.mp3"))?;

    let rendered = render_metadata_json(TrackDisplay {
        artist: "Fixture Artist",
        title: "Fixture Title",
        album: None,
        play_id: None,
        tags: &tags,
    })?;
    let value: serde_json::Value = serde_json::from_str(&rendered)?;

    assert_eq!(value["image"], serde_json::Value::Null);

    Ok(())
}

#[test]
fn render_drop_file_gives_null_image_when_the_tag_is_empty() -> Result<()> {
    let temp = TempDir::new()?;
    let tagged = fixture_with_image_tag(temp.path(), "")?;
    let tags = read_tags(&tagged)?;

    let rendered = render_metadata_json(TrackDisplay {
        artist: "Fixture Artist",
        title: "Fixture Title",
        album: None,
        play_id: None,
        tags: &tags,
    })?;
    let value: serde_json::Value = serde_json::from_str(&rendered)?;

    assert_eq!(value["image"], serde_json::Value::Null);

    Ok(())
}
