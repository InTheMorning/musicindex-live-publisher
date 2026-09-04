use std::path::Path;
use std::time::Duration;

use anyhow::{Context, Result};
use lofty::picture::Picture;
use lofty::prelude::*;
use lofty::tag::{ItemKey, ItemValue, Tag};
use serde::Serialize;

pub const MUSICINDEX_VOCABULARY: &[MusicIndexFrame] = &[
    MusicIndexFrame::new("Publisher", "TXXX:V4V_PUBLISHER"),
    MusicIndexFrame::new("Feed Guid", "TXXX:MusicIndex Feed Guid"),
    MusicIndexFrame::new("Track Guid", "TXXX:MusicIndex Track Guid"),
    MusicIndexFrame::new("Nostr Handle", "TXXX:RSS Nostr Handle"),
    MusicIndexFrame::new("Website", "WOAR"),
    MusicIndexFrame::new("Description", "COMM:MusicIndex Description"),
    MusicIndexFrame::new("Contributors", "TXXX:MusicIndex Contributors"),
    MusicIndexFrame::new("Value Routes", "TXXX:MusicIndex Value Routes"),
    MusicIndexFrame::new("MusicBrainz Recording", "UFID:http://musicbrainz.org"),
    MusicIndexFrame::new("MusicBrainz Release", "TXXX:MusicBrainz Album Id"),
    MusicIndexFrame::new(
        "MusicBrainz Release Group",
        "TXXX:MusicBrainz Release Group Id",
    ),
    MusicIndexFrame::new("Release Country", "TXXX:MusicBrainz Album Release Country"),
    MusicIndexFrame::new("Release Status", "TXXX:MusicBrainz Album Status"),
    MusicIndexFrame::new("Barcode", "TXXX:BARCODE"),
    MusicIndexFrame::new("Release Type", "TXXX:MusicBrainz Album Type"),
    MusicIndexFrame::new("Disc Subtitle", "TSST"),
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MusicIndexFrame {
    pub canonical: &'static str,
    pub frame_label: &'static str,
}

impl MusicIndexFrame {
    const fn new(canonical: &'static str, frame_label: &'static str) -> Self {
        Self {
            canonical,
            frame_label,
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TrackTags {
    pub musicindex: Vec<TagText>,
    pub tags: Vec<TagText>,
    pub duration: Option<Duration>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct TagText {
    pub key: String,
    pub value: String,
}

impl TagText {
    pub fn new(key: impl Into<String>, value: impl Into<String>) -> Self {
        Self {
            key: key.into(),
            value: value.into(),
        }
    }
}

impl TrackTags {
    pub fn musicindex_value(&self, key: &str) -> Option<&str> {
        self.musicindex
            .iter()
            .find(|item| item.key == key)
            .map(|item| item.value.as_str())
    }

    pub fn set_musicindex_value(&mut self, key: &str, value: impl Into<String>) {
        let value = value.into();
        if let Some(item) = self.musicindex.iter_mut().find(|item| item.key == key) {
            item.value = value;
            return;
        }
        self.musicindex.push(TagText::new(key, value));
    }

    pub fn remove_musicindex_value(&mut self, key: &str) {
        self.musicindex.retain(|item| item.key != key);
    }
}

pub fn read_tags(path: &Path) -> Result<TrackTags> {
    let tagged_file = lofty::read_from_path(path)
        .with_context(|| format!("read tags from {}", path.display()))?;
    let duration = nonzero_duration(tagged_file.properties().duration());
    let mut track_tags = TrackTags {
        musicindex: Vec::new(),
        tags: Vec::new(),
        duration,
    };

    for tag in tagged_file.tags() {
        collect_tag_items(tag, &mut track_tags);
        collect_pictures(tag, &mut track_tags);
    }

    Ok(track_tags)
}

fn nonzero_duration(duration: Duration) -> Option<Duration> {
    if duration.is_zero() {
        None
    } else {
        Some(duration)
    }
}

fn collect_tag_items(tag: &Tag, track_tags: &mut TrackTags) {
    for item in tag.items() {
        let key = item_key(tag, item.key(), item.description());
        if is_transcript_key(&key) {
            continue;
        }

        let value = render_item_value(&key, item.value());
        push_tag_text(track_tags, key, value);
    }
}

fn collect_pictures(tag: &Tag, track_tags: &mut TrackTags) {
    for picture in tag.pictures() {
        track_tags
            .tags
            .push(TagText::new("APIC", render_picture_summary(picture)));
    }
}

fn item_key(tag: &Tag, key: &ItemKey, description: &str) -> String {
    match key {
        ItemKey::Unknown(unknown) => unknown.clone(),
        ItemKey::Comment if !description.is_empty() => format!("COMM:{description}"),
        _ => key
            .map_key(tag.tag_type(), true)
            .map(str::to_string)
            .unwrap_or_else(|| format!("{key:?}")),
    }
}

fn push_tag_text(track_tags: &mut TrackTags, key: String, value: String) {
    if let Some(canonical) = canonical_musicindex_key(&key) {
        track_tags.musicindex.push(TagText::new(canonical, value));
    } else {
        track_tags.tags.push(TagText::new(key, value));
    }
}

fn canonical_musicindex_key(key: &str) -> Option<&'static str> {
    let normalized = normalize_key(key);
    MUSICINDEX_VOCABULARY
        .iter()
        .find(|entry| normalize_key(frame_match_key(entry.frame_label)) == normalized)
        .map(|entry| entry.canonical)
}

fn frame_match_key(frame_label: &str) -> &str {
    frame_label
        .rsplit_once(':')
        .map(|(_, key)| key)
        .unwrap_or(frame_label)
}

fn normalize_key(key: &str) -> String {
    key.split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_uppercase()
}

fn is_transcript_key(key: &str) -> bool {
    let normalized = normalize_key(frame_match_key(key));
    normalized == "USLT" || normalized == "MUSICINDEX TRANSCRIPT"
}

fn render_item_value(key: &str, value: &ItemValue) -> String {
    if is_binary_summary_key(key) {
        return binary_summary(key, value);
    }

    match value {
        ItemValue::Text(text) | ItemValue::Locator(text) => text.clone(),
        ItemValue::Binary(binary) => format!("{} bytes", binary.len()),
    }
}

fn is_binary_summary_key(key: &str) -> bool {
    let normalized = normalize_key(key);
    normalized == "APIC" || normalized == "PRIV" || normalized == "UFID"
}

fn binary_summary(key: &str, value: &ItemValue) -> String {
    match value {
        ItemValue::Binary(binary) => format!("{} bytes", binary.len()),
        ItemValue::Text(text) | ItemValue::Locator(text) => {
            if text.is_empty() {
                "0 bytes".to_string()
            } else {
                format!("{key} text, {} bytes", text.len())
            }
        }
    }
}

fn render_picture_summary(picture: &Picture) -> String {
    let mime_type = picture
        .mime_type()
        .map(ToString::to_string)
        .unwrap_or_else(|| "unknown".to_string());
    format!("{mime_type}, {}", render_kib(picture.data().len()))
}

fn render_kib(bytes: usize) -> String {
    format!("{:.1} KB", bytes as f64 / 1024.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tags_normalizes_musicindex_keys() {
        assert_eq!(
            canonical_musicindex_key("MusicIndex Feed Guid"),
            Some("Feed Guid")
        );
        assert_eq!(
            canonical_musicindex_key("MUSICINDEX   FEED   GUID"),
            Some("Feed Guid")
        );
        assert_eq!(
            canonical_musicindex_key("MusicIndex Value Routes"),
            Some("Value Routes")
        );
    }

    #[test]
    fn tags_identifies_transcript_keys() {
        assert!(is_transcript_key("USLT"));
        assert!(is_transcript_key("SYLT:MusicIndex Transcript"));
        assert!(is_transcript_key("MusicIndex Transcript"));
        assert!(!is_transcript_key("MusicIndex Description"));
    }

    #[test]
    fn tags_summarizes_binary_payload_keys() {
        let binary = ItemValue::Binary(vec![0, 1, 2, 3]);

        assert_eq!(render_item_value("PRIV", &binary), "4 bytes");
        assert_eq!(render_item_value("UFID", &binary), "4 bytes");
        assert_eq!(render_item_value("APIC", &binary), "4 bytes");
    }
}
