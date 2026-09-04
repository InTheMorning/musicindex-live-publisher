use std::path::{Path, PathBuf};

use anyhow::Result;
use mixxx_now_playing::tags::read_tags;

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name)
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
