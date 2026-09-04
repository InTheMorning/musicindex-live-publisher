use std::path::{Path, PathBuf};

use anyhow::Result;
use mixxx_now_playing::render::{TrackDisplay, render_metadata_text};
use mixxx_now_playing::tags::read_tags;

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name)
}

#[test]
fn render_mp3_fixture_excludes_transcripts_and_binary_payloads() -> Result<()> {
    let tags = read_tags(&fixture("musicindex-tagged.mp3"))?;
    let rendered = render_metadata_text(TrackDisplay {
        artist: "Fixture Artist",
        title: "Fixture Title",
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
