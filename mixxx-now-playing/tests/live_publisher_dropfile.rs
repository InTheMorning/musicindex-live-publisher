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
            tags: &tags,
        },
        "default",
        ValueRoutesSource::MusicIndexApi,
    )?;

    let dropfile =
        parse(json.as_bytes())?.ok_or_else(|| anyhow!("known now-playing schema should parse"))?;
    let payload = payload_from_dropfile(&dropfile, "event-guid", "block-guid");

    assert_eq!(dropfile.schema, "musicindex.nowplaying/1");
    assert_eq!(dropfile.target, "default");
    assert_eq!(dropfile.duration_secs, Some(187.326));
    assert_eq!(payload.title, "Some Track");
    assert_eq!(payload.event_guid, "event-guid");
    assert_eq!(payload.feed_guid.as_deref(), Some("feed-guid"));
    assert_eq!(payload.item_guid.as_deref(), Some("track-guid"));
    assert_eq!(payload.value.destinations.len(), 1);
    Ok(())
}
