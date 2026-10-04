use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use lofty::config::WriteOptions;
use lofty::prelude::*;
use lofty::tag::{ItemValue, TagItem};
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

fn musicindex_value<'a>(
    tags: &'a mixxx_now_playing::tags::TrackTags,
    key: &str,
) -> Option<&'a str> {
    tags.musicindex
        .iter()
        .find(|item| item.key == key)
        .map(|item| item.value.as_str())
}

#[test]
fn tags_mp3_fixture_reads_musicindex_fields_and_duration() -> Result<()> {
    let tags = read_tags(&fixture("musicindex-tagged.mp3"))?;

    assert_eq!(
        musicindex_value(&tags, "Feed Guid"),
        Some("feed-guid-fixture")
    );
    assert_eq!(
        musicindex_value(&tags, "Track Guid"),
        Some("track-guid-fixture")
    );
    assert_eq!(
        musicindex_value(&tags, "Publisher"),
        Some("Fixture Publisher")
    );
    assert_eq!(
        musicindex_value(&tags, "Contributors"),
        Some("Alice: vocals")
    );
    assert_eq!(
        musicindex_value(&tags, "Value Routes"),
        Some(r#"[{"recipient_name":"Alice","route_type":"node","split":90}]"#)
    );
    assert_eq!(
        musicindex_value(&tags, "Nostr Handle"),
        Some("artist@example.com")
    );
    assert!(tags.duration.is_some());

    Ok(())
}

#[test]
fn tags_flac_fixture_reads_same_canonical_musicindex_fields() -> Result<()> {
    let tags = read_tags(&fixture("musicindex-tagged.flac"))?;
    let keys = tags
        .musicindex
        .iter()
        .map(|item| item.key.as_str())
        .collect::<Vec<_>>();

    assert!(keys.contains(&"Feed Guid"));
    assert!(keys.contains(&"Track Guid"));
    assert!(keys.contains(&"Publisher"));
    assert!(keys.contains(&"Contributors"));
    assert!(keys.contains(&"Value Routes"));
    assert!(keys.contains(&"Nostr Handle"));
    assert!(tags.duration.is_some());

    Ok(())
}

#[test]
fn tags_unknown_non_transcript_keys_pass_through() -> Result<()> {
    let tags = read_tags(&fixture("musicindex-tagged.flac"))?;

    assert!(tags.tags.iter().any(|item| item.key == "TITLE"));
    assert!(tags.tags.iter().any(|item| item.key == "ARTIST"));
    assert!(tags.tags.iter().any(|item| item.key == "DESCRIPTION"));

    Ok(())
}

#[test]
fn tags_transcript_fields_are_excluded() -> Result<()> {
    let tags = read_tags(&fixture("musicindex-tagged.mp3"))?;
    let rendered_debug = format!("{tags:?}");

    assert!(!rendered_debug.contains("transcript should not render"));
    assert!(!rendered_debug.contains("MusicIndex Transcript"));

    Ok(())
}

#[test]
fn tags_file_without_musicindex_tags_returns_empty_fields() -> Result<()> {
    let tags = read_tags(&fixture("untagged.flac"))?;

    assert!(tags.musicindex.is_empty());
    assert!(tags.duration.is_some());

    Ok(())
}

#[test]
fn tags_artwork_renders_summary_without_binary_payload() -> Result<()> {
    let tags = read_tags(&fixture("musicindex-tagged.mp3"))?;
    let artwork = tags
        .tags
        .iter()
        .find(|item| item.key == "APIC")
        .expect("fixture should contain embedded artwork");

    assert!(artwork.value.starts_with("image/jpeg, "));
    assert!(artwork.value.ends_with(" KB"));
    assert!(!artwork.value.contains("/9j/"));

    Ok(())
}

#[test]
fn tags_file_lofty_cannot_open_returns_error() {
    let result = read_tags(Path::new("tests/fixtures/missing.mp3"));

    assert!(result.is_err());
}

#[test]
fn tags_image_tag_reads_the_musicindex_image_value() -> Result<()> {
    let temp = TempDir::new()?;
    let tagged = fixture_with_image_tag(temp.path(), "https://example.com/cover.jpg")?;

    let tags = read_tags(&tagged)?;

    assert_eq!(
        musicindex_value(&tags, "Image"),
        Some("https://example.com/cover.jpg")
    );

    Ok(())
}

#[test]
fn tags_image_tag_absent_gives_none() -> Result<()> {
    let tags = read_tags(&fixture("musicindex-tagged.mp3"))?;

    assert_eq!(musicindex_value(&tags, "Image"), None);

    Ok(())
}
