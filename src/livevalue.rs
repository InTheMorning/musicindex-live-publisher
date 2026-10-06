//! Podcasting 2.0 live value payload assembly.

use serde::{Deserialize, Serialize};

use crate::{DropFile, PaymentRoute};

/// A direct live value payload for the MusicIndex relay.
///
/// This is the raw payload shape consumed by remote value listeners. It is not
/// the relay's wrapped `{event_id, metadata}` form.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct LiveValuePayload {
    pub title: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub image: Option<String>,
    pub description: String,
    #[serde(rename = "type")]
    pub kind: String,
    #[serde(rename = "startTime")]
    pub start_time: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub duration: Option<f64>,
    #[serde(rename = "eventGuid")]
    pub event_guid: String,
    #[serde(rename = "blockGuid")]
    pub block_guid: String,
    #[serde(rename = "feedGuid", skip_serializing_if = "Option::is_none")]
    pub feed_guid: Option<String>,
    #[serde(rename = "itemGuid", skip_serializing_if = "Option::is_none")]
    pub item_guid: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub line: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub author: Option<String>,
    #[serde(rename = "podcastName", skip_serializing_if = "Option::is_none")]
    pub podcast_name: Option<String>,
    pub value: LiveValue,
    /// The ID of the Mixxx history row for this play (ADR 0010, ADR 0012).
    /// This field is not serialized; it is used only for pairing with display state.
    #[serde(skip)]
    pub play_id: Option<String>,
}

/// Live value payment routing information.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LiveValue {
    pub model: LiveValueModel,
    pub destinations: Vec<LiveValueDestination>,
}

/// Live value payment model metadata.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LiveValueModel {
    #[serde(rename = "type")]
    pub kind: String,
    pub method: String,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub suggested: Option<String>,
}

/// A live value payment destination.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LiveValueDestination {
    #[serde(rename = "type", skip_serializing_if = "Option::is_none")]
    pub kind: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub address: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub split: Option<String>,
    #[serde(rename = "customKey", skip_serializing_if = "Option::is_none")]
    pub custom_key: Option<String>,
    #[serde(rename = "customValue", skip_serializing_if = "Option::is_none")]
    pub custom_value: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fee: Option<bool>,
}

/// Builds a track live value payload from a parsed drop file.
///
/// The caller supplies `event_guid` for the live item and `block_guid` for the
/// current track block. GUID generation is intentionally outside this pure
/// transform so retries can reuse the same block identity.
pub fn payload_from_dropfile(
    dropfile: &DropFile,
    event_guid: &str,
    block_guid: &str,
) -> LiveValuePayload {
    // ADR 0010: `line` is `[album, artist]`, as the model server sends it.
    // With no album, the title takes the place of the album.
    let album = dropfile.album.as_deref().filter(|album| !album.is_empty());
    let line = vec![
        album.unwrap_or(&dropfile.title).to_owned(),
        dropfile.artist.clone(),
    ];

    LiveValuePayload {
        title: dropfile.title.clone(),
        image: dropfile.image.clone(),
        description: String::new(),
        kind: "music".to_owned(),
        start_time: 0,
        duration: dropfile.duration_secs,
        event_guid: event_guid.to_owned(),
        block_guid: block_guid.to_owned(),
        feed_guid: dropfile.feed_guid.clone(),
        item_guid: dropfile.track_guid.clone(),
        line: Some(line),
        author: Some(dropfile.artist.clone()),
        podcast_name: album.map(str::to_owned),
        value: live_value_from_routes(&dropfile.value_routes),
        play_id: dropfile.play_id.clone(),
    }
}

/// Fixed title for the dead block.
///
/// ADR 0005 states this value as a constant. No configuration can change it.
const DEAD_BLOCK_TITLE: &str = "No V4V track playing";

/// Fixed payment-destination name for the dead block.
const DEAD_BLOCK_RECIPIENT_NAME: &str = "No V4V payment route";

/// Fixed lnaddress for the dead block.
///
/// A payment to this address fails, so the dead block pays nobody.
const DEAD_BLOCK_ADDRESS: &str = "no-v4v-track@example.invalid";

/// Builds the dead block: the fixed live value payload for "no payable block
/// plays here now" (ADR 0005).
///
/// The dead block is a constant. No configuration can change its title, its
/// model, or its destination. It carries no `feedGuid` and no `itemGuid`,
/// because no producer supplied a track. The caller supplies `event_guid` for
/// the target and a fresh `block_guid` for each publish.
pub fn dead_payload(event_guid: &str, block_guid: &str) -> LiveValuePayload {
    LiveValuePayload {
        title: DEAD_BLOCK_TITLE.to_owned(),
        image: None,
        description: String::new(),
        kind: "music".to_owned(),
        start_time: 0,
        duration: None,
        event_guid: event_guid.to_owned(),
        block_guid: block_guid.to_owned(),
        feed_guid: None,
        item_guid: None,
        line: None,
        author: None,
        podcast_name: None,
        value: LiveValue {
            model: LiveValueModel {
                kind: "lightning".to_owned(),
                method: "lnaddress".to_owned(),
                suggested: None,
            },
            destinations: vec![LiveValueDestination {
                kind: Some("lnaddress".to_owned()),
                name: Some(DEAD_BLOCK_RECIPIENT_NAME.to_owned()),
                address: Some(DEAD_BLOCK_ADDRESS.to_owned()),
                split: Some("100".to_owned()),
                custom_key: None,
                custom_value: None,
                fee: None,
            }],
        },
        play_id: None,
    }
}

/// Maps a producer payment route to a live value destination.
pub fn destination_from_payment_route(route: &PaymentRoute) -> LiveValueDestination {
    LiveValueDestination {
        kind: route.route_type.clone(),
        name: route.recipient_name.clone(),
        address: route.address.clone(),
        split: route.split.map(format_split),
        custom_key: route.custom_key.clone(),
        custom_value: route.custom_value.clone(),
        fee: route.fee,
    }
}

/// Formats a split as the shortest decimal string Serde can round-trip.
pub fn format_split(split: f64) -> String {
    split.to_string()
}

fn live_value_from_routes(routes: &[PaymentRoute]) -> LiveValue {
    LiveValue {
        model: LiveValueModel {
            kind: "lightning".to_owned(),
            method: "keysend".to_owned(),
            suggested: None,
        },
        destinations: routes.iter().map(destination_from_payment_route).collect(),
    }
}

#[cfg(test)]
mod tests {
    use anyhow::{Result, anyhow};
    use serde_json::{Value, json};

    use super::*;
    use crate::SCHEMA_VERSION;

    fn route(split: Option<f64>) -> PaymentRoute {
        PaymentRoute {
            recipient_name: Some("Alice".to_owned()),
            route_type: Some("node".to_owned()),
            split,
            fee: None,
            address: Some("03ab".to_owned()),
            custom_key: None,
            custom_value: None,
        }
    }

    fn dropfile() -> DropFile {
        DropFile {
            schema: SCHEMA_VERSION.to_owned(),
            target: "default".to_owned(),
            artist: "Alice".to_owned(),
            title: "Some Track".to_owned(),
            duration_secs: Some(187.326),
            image: Some("https://example.com/art.png".to_owned()),
            feed_guid: Some("feed-guid".to_owned()),
            track_guid: Some("track-guid".to_owned()),
            album: None,
            play_id: None,
            value_routes: vec![route(Some(90.0))],
            value_routes_source: Some("musicindex-api".to_owned()),
        }
    }

    #[test]
    fn livevalue_split_formats_as_decimal_string() -> Result<()> {
        assert_eq!(format_split(90.0), "90");
        assert_eq!(format_split(0.49), "0.49");
        assert_eq!(format_split(49.51), "49.51");

        let payload = payload_from_dropfile(&dropfile(), "event-guid", "block-guid");
        let value = serde_json::to_value(payload)?;
        let split = &value["value"]["destinations"][0]["split"];

        assert_eq!(split, "90");
        assert!(!split.is_number());
        Ok(())
    }

    #[test]
    fn livevalue_null_custom_fields_and_fee_are_omitted() -> Result<()> {
        let payload = payload_from_dropfile(&dropfile(), "event-guid", "block-guid");
        let value = serde_json::to_value(payload)?;
        let destination = value["value"]["destinations"][0]
            .as_object()
            .ok_or_else(|| anyhow!("destination should be an object"))?;

        assert!(!destination.contains_key("customKey"));
        assert!(!destination.contains_key("customValue"));
        assert!(!destination.contains_key("fee"));
        Ok(())
    }

    #[test]
    fn livevalue_empty_value_routes_keep_empty_destinations() -> Result<()> {
        let mut dropfile = dropfile();
        dropfile.value_routes.clear();

        let payload = payload_from_dropfile(&dropfile, "event-guid", "block-guid");
        let value = serde_json::to_value(payload)?;

        assert_eq!(value["value"]["destinations"], json!([]));
        assert!(value.get("value").is_some());
        Ok(())
    }

    #[test]
    fn livevalue_top_level_payload_is_never_wrapped_shape() -> Result<()> {
        let payload = payload_from_dropfile(&dropfile(), "event-guid", "block-guid");
        let value = serde_json::to_value(payload)?;
        let keys: Vec<&str> = value
            .as_object()
            .ok_or_else(|| anyhow!("payload should be an object"))?
            .keys()
            .map(String::as_str)
            .collect();

        assert_ne!(keys, vec!["event_id", "metadata"]);
        assert!(value.get("event_id").is_none());
        assert!(value.get("metadata").is_none());
        Ok(())
    }

    #[test]
    fn livevalue_duration_is_omitted_when_absent() -> Result<()> {
        let mut dropfile = dropfile();
        dropfile.duration_secs = None;

        let payload = payload_from_dropfile(&dropfile, "event-guid", "block-guid");
        let value = serde_json::to_value(payload)?;

        assert!(value.get("duration").is_none());
        Ok(())
    }

    #[test]
    fn livevalue_carries_sourceable_musicindex_identity_fields() -> Result<()> {
        let payload = payload_from_dropfile(&dropfile(), "event-guid", "block-guid");
        let value = serde_json::to_value(payload)?;

        assert_eq!(value["feedGuid"], "feed-guid");
        assert_eq!(value["itemGuid"], "track-guid");
        Ok(())
    }

    #[test]
    fn livevalue_different_block_guids_only_change_block_guid() -> Result<()> {
        let dropfile = dropfile();
        let first = serde_json::to_value(payload_from_dropfile(
            &dropfile,
            "event-guid",
            "block-guid-1",
        ))?;
        let second = serde_json::to_value(payload_from_dropfile(
            &dropfile,
            "event-guid",
            "block-guid-2",
        ))?;

        let mut first_without_block = first;
        let mut second_without_block = second;
        first_without_block["blockGuid"] = Value::Null;
        second_without_block["blockGuid"] = Value::Null;

        assert_eq!(first_without_block, second_without_block);
        Ok(())
    }

    #[test]
    fn livevalue_dead_payload_has_the_fixed_dead_block_values() -> Result<()> {
        let payload = dead_payload("event-guid", "block-guid");
        let value = serde_json::to_value(&payload)?;

        assert_eq!(value["title"], "No V4V track playing");
        assert!(value.get("image").is_none());
        assert!(value.get("duration").is_none());
        assert!(value.get("feedGuid").is_none());
        assert!(value.get("itemGuid").is_none());
        assert_eq!(value["value"]["model"]["type"], "lightning");
        assert_eq!(value["value"]["model"]["method"], "lnaddress");
        assert_eq!(
            value["value"]["destinations"].as_array().map(Vec::len),
            Some(1)
        );
        assert_eq!(value["value"]["destinations"][0]["type"], "lnaddress");
        assert_eq!(
            value["value"]["destinations"][0]["name"],
            "No V4V payment route"
        );
        assert_eq!(
            value["value"]["destinations"][0]["address"],
            "no-v4v-track@example.invalid"
        );
        assert_eq!(value["value"]["destinations"][0]["split"], "100");
        assert!(!value["value"]["destinations"][0]["split"].is_number());
        Ok(())
    }

    #[test]
    fn livevalue_two_dead_payloads_only_differ_by_block_guid() -> Result<()> {
        let first = serde_json::to_value(dead_payload("event-guid", "block-guid-1"))?;
        let second = serde_json::to_value(dead_payload("event-guid", "block-guid-2"))?;

        assert_ne!(first["blockGuid"], second["blockGuid"]);

        let mut first_without_block = first;
        let mut second_without_block = second;
        first_without_block["blockGuid"] = Value::Null;
        second_without_block["blockGuid"] = Value::Null;

        assert_eq!(first_without_block, second_without_block);
        Ok(())
    }

    #[test]
    fn livevalue_with_album_gives_line_album_artist_and_podcast_name() -> Result<()> {
        let mut dropfile = dropfile();
        dropfile.album = Some("Test Album".to_owned());

        let payload = payload_from_dropfile(&dropfile, "event-guid", "block-guid");
        let value = serde_json::to_value(payload)?;

        assert_eq!(value["line"], json!(["Test Album", "Alice"]));
        assert_eq!(value["author"], "Alice");
        assert_eq!(value["podcastName"], "Test Album");
        Ok(())
    }

    #[test]
    fn livevalue_without_album_gives_line_title_artist_and_no_podcast_name() -> Result<()> {
        let dropfile = dropfile();

        let payload = payload_from_dropfile(&dropfile, "event-guid", "block-guid");
        let value = serde_json::to_value(payload)?;

        assert_eq!(value["line"], json!(["Some Track", "Alice"]));
        assert_eq!(value["author"], "Alice");
        assert!(value.get("podcastName").is_none());
        Ok(())
    }

    #[test]
    fn livevalue_with_empty_album_gives_line_title_artist_and_no_podcast_name() -> Result<()> {
        let mut dropfile = dropfile();
        dropfile.album = Some(String::new());

        let payload = payload_from_dropfile(&dropfile, "event-guid", "block-guid");
        let value = serde_json::to_value(payload)?;

        assert_eq!(value["line"], json!(["Some Track", "Alice"]));
        assert_eq!(value["author"], "Alice");
        assert!(value.get("podcastName").is_none());
        Ok(())
    }

    #[test]
    fn livevalue_payload_never_has_link_key() -> Result<()> {
        let dropfile = dropfile();
        let payload = payload_from_dropfile(&dropfile, "event-guid", "block-guid");
        let value = serde_json::to_value(payload)?;

        assert!(value.get("link").is_none());
        Ok(())
    }

    #[test]
    fn livevalue_dead_payload_never_has_link_key() -> Result<()> {
        let payload = dead_payload("event-guid", "block-guid");
        let value = serde_json::to_value(payload)?;

        assert!(value.get("link").is_none());
        Ok(())
    }

    #[test]
    fn livevalue_dead_payload_has_no_new_fields() -> Result<()> {
        let payload = dead_payload("event-guid", "block-guid");
        let value = serde_json::to_value(&payload)?;

        let mut keys: Vec<&str> = value
            .as_object()
            .map(|object| object.keys().map(String::as_str).collect())
            .unwrap_or_default();
        keys.sort_unstable();
        // The key set of the dead block before ADR 0010.
        assert_eq!(
            keys,
            [
                "blockGuid",
                "description",
                "eventGuid",
                "startTime",
                "title",
                "type",
                "value"
            ]
        );
        Ok(())
    }
}
