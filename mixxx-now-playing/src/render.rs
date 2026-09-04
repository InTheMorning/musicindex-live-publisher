use anyhow::Result;
use serde::Serialize;

use crate::musicindex::{PaymentRoute, ValueRoutesSource};
use crate::tags::{TagText, TrackTags};

#[derive(Debug, Clone, Copy)]
pub struct TrackDisplay<'a> {
    pub artist: &'a str,
    pub title: &'a str,
    pub tags: &'a TrackTags,
}

pub fn render_metadata_text(display: TrackDisplay<'_>) -> String {
    render_metadata_text_with_routes(display, ValueRoutesSource::EmbeddedId3)
}

pub fn render_metadata_text_with_routes(
    display: TrackDisplay<'_>,
    value_routes_source: ValueRoutesSource,
) -> String {
    let mut output = String::new();
    output.push_str(display.artist);
    output.push_str(" - ");
    output.push_str(display.title);
    output.push_str("\n\n");

    if !display.tags.musicindex.is_empty() {
        output.push_str("[MusicIndex]\n");
        render_items(
            &mut output,
            &display.tags.musicindex,
            Some(value_routes_source),
        );
        output.push('\n');
    }

    if !display.tags.tags.is_empty() {
        output.push_str("[Tags]\n");
        render_items(&mut output, &display.tags.tags, None);
    }

    output
}

pub fn render_metadata_json(display: TrackDisplay<'_>) -> Result<String> {
    render_metadata_json_with_routes(display, "default", ValueRoutesSource::EmbeddedId3)
}

pub fn render_metadata_json_with_routes(
    display: TrackDisplay<'_>,
    target: &str,
    value_routes_source: ValueRoutesSource,
) -> Result<String> {
    #[derive(Serialize)]
    struct DropFile<'a> {
        schema: &'static str,
        target: &'a str,
        artist: &'a str,
        title: &'a str,
        duration_secs: Option<f64>,
        image: Option<&'a str>,
        feed_guid: Option<&'a str>,
        track_guid: Option<&'a str>,
        value_routes: Vec<PaymentRoute>,
        value_routes_source: Option<&'static str>,
    }

    let value_routes_json = display.tags.musicindex_value("Value Routes");
    let value_routes = value_routes_json
        .map(serde_json::from_str::<Vec<PaymentRoute>>)
        .transpose()?
        .unwrap_or_default();

    Ok(serde_json::to_string_pretty(&DropFile {
        schema: "musicindex.nowplaying/1",
        target,
        artist: display.artist,
        title: display.title,
        duration_secs: display.tags.duration.map(|duration| duration.as_secs_f64()),
        image: display.tags.musicindex_value("Image"),
        feed_guid: display.tags.musicindex_value("Feed Guid"),
        track_guid: display.tags.musicindex_value("Track Guid"),
        value_routes,
        value_routes_source: value_routes_json.map(|_| value_routes_source.as_str()),
    })?)
}

pub fn render_now_playing_line(artist: &str, title: &str, strip_hyphens: bool) -> String {
    let mut line = format!("{artist}|{title}");
    if strip_hyphens {
        line = line.replace('-', "");
    }
    line.replace('|', " - ")
}

fn render_items(
    output: &mut String,
    items: &[TagText],
    value_routes_source: Option<ValueRoutesSource>,
) {
    let width = items.iter().map(|item| item.key.len()).max().unwrap_or(0);
    for item in items {
        if let (Some(source), "Value Routes") = (value_routes_source, item.key.as_str()) {
            output.push_str(&format!(
                "{:<width$} = {}\n{}\n",
                item.key,
                source.as_str(),
                item.value
            ));
        } else {
            output.push_str(&format!("{:<width$} = {}\n", item.key, item.value));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tags::{TagText, TrackTags};

    #[test]
    fn render_metadata_text_includes_musicindex_then_tags() {
        let tags = TrackTags {
            musicindex: vec![
                TagText::new("Feed Guid", "feed-guid"),
                TagText::new(
                    "Value Routes",
                    r#"[{"recipient_name":"Alice","route_type":"node","split":90}]"#,
                ),
            ],
            tags: vec![TagText::new("TIT2", "Test Title")],
            duration: None,
        };

        let rendered = render_metadata_text(TrackDisplay {
            artist: "Artist",
            title: "Title",
            tags: &tags,
        });

        assert_eq!(
            rendered,
            concat!(
                "Artist - Title\n\n",
                "[MusicIndex]\n",
                "Feed Guid    = feed-guid\n",
                "Value Routes = embedded-id3\n",
                r#"[{"recipient_name":"Alice","route_type":"node","split":90}]"#,
                "\n\n",
                "[Tags]\n",
                "TIT2 = Test Title\n",
            )
        );
    }

    #[test]
    fn render_metadata_text_omits_empty_sections() {
        let tags = TrackTags::default();

        let rendered = render_metadata_text(TrackDisplay {
            artist: "Artist",
            title: "Title",
            tags: &tags,
        });

        assert_eq!(rendered, "Artist - Title\n\n");
    }

    #[test]
    fn render_metadata_text_can_mark_api_value_routes() {
        let tags = TrackTags {
            musicindex: vec![TagText::new("Value Routes", "[]")],
            tags: Vec::new(),
            duration: None,
        };

        let rendered = render_metadata_text_with_routes(
            TrackDisplay {
                artist: "Artist",
                title: "Title",
                tags: &tags,
            },
            ValueRoutesSource::MusicIndexApi,
        );

        assert!(rendered.contains("Value Routes = musicindex-api\n[]\n"));
    }

    #[test]
    fn render_metadata_json_emits_drop_file_schema() -> Result<()> {
        let tags = TrackTags {
            musicindex: vec![
                TagText::new("Feed Guid", "feed-guid"),
                TagText::new("Track Guid", "track-guid"),
                TagText::new(
                    "Value Routes",
                    r#"[{"recipient_name":"Alice","route_type":"node","address":"03ab","split":90.0,"fee":false,"custom_key":null,"custom_value":null}]"#,
                ),
            ],
            tags: vec![TagText::new("TIT2", "Test Title")],
            duration: Some(std::time::Duration::from_millis(187_326)),
        };

        let rendered = render_metadata_json_with_routes(
            TrackDisplay {
                artist: "Artist",
                title: "Title",
                tags: &tags,
            },
            "stream-a",
            ValueRoutesSource::MusicIndexApi,
        )?;
        let value: serde_json::Value = serde_json::from_str(&rendered)?;
        let object = value
            .as_object()
            .ok_or_else(|| anyhow::anyhow!("rendered JSON must be an object"))?;

        let mut keys = object.keys().map(String::as_str).collect::<Vec<_>>();
        keys.sort_unstable();
        assert_eq!(
            keys,
            vec![
                "artist",
                "duration_secs",
                "feed_guid",
                "image",
                "schema",
                "target",
                "title",
                "track_guid",
                "value_routes",
                "value_routes_source",
            ]
        );
        assert_eq!(value["schema"], "musicindex.nowplaying/1");
        assert_eq!(value["target"], "stream-a");
        assert_eq!(value["duration_secs"], 187.326);
        assert_eq!(value["image"], serde_json::Value::Null);
        assert_eq!(value["feed_guid"], "feed-guid");
        assert_eq!(value["track_guid"], "track-guid");
        assert_eq!(value["value_routes"][0]["recipient_name"], "Alice");
        assert_eq!(value["value_routes_source"], "musicindex-api");
        Ok(())
    }

    #[test]
    fn render_metadata_json_uses_empty_routes_without_source_when_absent() -> Result<()> {
        let tags = TrackTags::default();

        let rendered = render_metadata_json(TrackDisplay {
            artist: "Artist",
            title: "Title",
            tags: &tags,
        })?;
        let value: serde_json::Value = serde_json::from_str(&rendered)?;

        assert_eq!(value["target"], "default");
        assert_eq!(value["value_routes"], serde_json::json!([]));
        assert_eq!(value["value_routes_source"], serde_json::Value::Null);
        Ok(())
    }

    #[test]
    fn render_now_playing_line_preserves_shell_quirk() {
        assert_eq!(
            render_now_playing_line("Test-Artist", "Test-Title", true),
            "TestArtist - TestTitle"
        );
        assert_eq!(
            render_now_playing_line("Test-Artist", "Test-Title", false),
            "Test-Artist - Test-Title"
        );
    }
}
