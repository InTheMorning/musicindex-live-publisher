use std::time::Duration;

use anyhow::{Result, anyhow};
use mixxx_now_playing::musicindex::ValueRoutesSource;
use mixxx_now_playing::render::{TrackDisplay, render_metadata_json_with_routes};
use mixxx_now_playing::tags::{TagText, TrackTags};
use musicindex_live_publisher::{parse, payload_from_dropfile};

#[test]
fn json_output_parses_as_live_publisher_dropfile_and_transforms() -> Result<()> {
    let tags = TrackTags {
        musicindex: vec![
            TagText::new("Feed Guid", "feed-guid"),
            TagText::new("Track Guid", "track-guid"),
            TagText::new(
                "Value Routes",
                r#"[{"recipient_name":"Alice","route_type":"node","address":"03alice","split":90.0,"fee":false,"custom_key":null,"custom_value":null}]"#,
            ),
        ],
        tags: Vec::new(),
        duration: Some(Duration::from_millis(187_326)),
    };
    let json = render_metadata_json_with_routes(
        TrackDisplay {
            artist: "Alice",
            title: "Some Track",
            album: Some("Test Album"),
            play_id: Some(42),
            tags: &tags,
        },
        "default",
        ValueRoutesSource::MusicIndexApi,
    )?;

    let dropfile =
        parse(json.as_bytes())?.ok_or_else(|| anyhow!("known now-playing schema should parse"))?;
    let payload = payload_from_dropfile(&dropfile, "event-guid", "block-guid");

    assert_eq!(dropfile.schema, "musicindex.nowplaying/3");
    assert_eq!(dropfile.target, "default");
    assert_eq!(dropfile.duration_secs, Some(187.326));
    assert_eq!(dropfile.album, Some("Test Album".to_string()));
    assert_eq!(dropfile.play_id, Some("42".to_string()));
    assert_eq!(payload.title, "Some Track");
    assert_eq!(payload.event_guid, "event-guid");
    assert_eq!(payload.feed_guid.as_deref(), Some("feed-guid"));
    assert_eq!(payload.item_guid.as_deref(), Some("track-guid"));
    assert_eq!(payload.value.destinations.len(), 1);
    Ok(())
}

#[test]
fn json_output_of_a_non_payable_row_parses_with_empty_routes() -> Result<()> {
    // ADR 0014: a row that is not payable gives a version 3 file with an
    // empty `value_routes`, and no image, feed guid, or track guid.
    let tags = TrackTags {
        musicindex: Vec::new(),
        tags: Vec::new(),
        duration: Some(Duration::from_secs(180)),
    };
    let json = render_metadata_json_with_routes(
        TrackDisplay {
            artist: "Alice",
            title: "Not V4V",
            album: Some("Test Album"),
            play_id: Some(7),
            tags: &tags,
        },
        "default",
        ValueRoutesSource::EmbeddedId3,
    )?;

    let dropfile =
        parse(json.as_bytes())?.ok_or_else(|| anyhow!("known now-playing schema should parse"))?;
    let payload = payload_from_dropfile(&dropfile, "event-guid", "block-guid");

    assert_eq!(dropfile.schema, "musicindex.nowplaying/3");
    assert_eq!(dropfile.artist, "Alice");
    assert_eq!(dropfile.title, "Not V4V");
    assert_eq!(dropfile.play_id, Some("7".to_string()));
    assert!(dropfile.value_routes.is_empty());
    assert_eq!(dropfile.value_routes_source, None);
    assert_eq!(dropfile.image, None);
    assert_eq!(dropfile.feed_guid, None);
    assert_eq!(dropfile.track_guid, None);
    assert!(payload.value.destinations.is_empty());
    Ok(())
}
