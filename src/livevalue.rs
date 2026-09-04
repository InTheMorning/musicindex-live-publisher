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
    pub value: LiveValue,
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
        value: live_value_from_routes(&dropfile.value_routes),
    }
}

/// Builds a fallback live value payload from a configured station value block.
///
/// The fallback payload uses the same music block shape as track payloads. The
/// caller supplies title, optional image, and GUIDs because configuration and
/// block identity are owned by later tasks.
pub fn fallback_payload(
    title: &str,
    image: Option<&str>,
    event_guid: &str,
    block_guid: &str,
    value: &LiveValue,
) -> LiveValuePayload {
    LiveValuePayload {
        title: title.to_owned(),
        image: image.map(str::to_owned),
        description: String::new(),
        kind: "music".to_owned(),
        start_time: 0,
        duration: None,
        event_guid: event_guid.to_owned(),
        block_guid: block_guid.to_owned(),
        feed_guid: None,
        item_guid: None,
        value: value.clone(),
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

    fn fallback_destination(split: &str) -> LiveValueDestination {
        LiveValueDestination {
            kind: Some("node".to_owned()),
            name: Some("Station".to_owned()),
            address: Some("03ab".to_owned()),
            split: Some(split.to_owned()),
            custom_key: None,
            custom_value: None,
            fee: None,
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
    fn livevalue_fallback_payload_preserves_configured_value_block() -> Result<()> {
        let payload = fallback_payload(
            "Station",
            None,
            "event-guid",
            "block-guid",
            &LiveValue {
                model: LiveValueModel {
                    kind: "custom-model".to_owned(),
                    method: "custom-method".to_owned(),
                    suggested: Some("0.0000100000".to_owned()),
                },
                destinations: vec![fallback_destination("100")],
            },
        );
        let value = serde_json::to_value(payload)?;

        assert_eq!(value["title"], "Station");
        assert!(value.get("image").is_none());
        assert!(value.get("duration").is_none());
        assert_eq!(value["value"]["model"]["type"], "custom-model");
        assert_eq!(value["value"]["model"]["method"], "custom-method");
        assert_eq!(value["value"]["model"]["suggested"], "0.0000100000");
        assert_eq!(value["value"]["destinations"][0]["type"], "node");
        assert_eq!(value["value"]["destinations"][0]["split"], "100");
        Ok(())
    }
}
