//! Now-playing drop-file parsing.

use anyhow::{Context, Result, anyhow};
use serde::{Deserialize, Serialize};

/// Supported now-playing drop-file schema version.
pub const SCHEMA_VERSION: &str = "musicindex.nowplaying/1";

/// A MusicIndex now-playing drop file.
///
/// Producers write this JSON shape into the watched drop directory. Presence of
/// a file means the track is playing; removing it means the track stopped.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DropFile {
    pub schema: String,
    pub target: String,
    pub artist: String,
    pub title: String,
    pub duration_secs: Option<f64>,
    pub image: Option<String>,
    pub feed_guid: Option<String>,
    pub track_guid: Option<String>,
    pub value_routes: Vec<PaymentRoute>,
    pub value_routes_source: Option<String>,
}

/// A payment route using the producer-facing MusicIndex field names.
///
/// The field names intentionally match `v4vmm::api::PaymentRoute` so producers
/// can serialize that type without a translation layer.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PaymentRoute {
    pub recipient_name: Option<String>,
    pub route_type: Option<String>,
    pub split: Option<f64>,
    pub fee: Option<bool>,
    pub address: Option<String>,
    pub custom_key: Option<String>,
    pub custom_value: Option<String>,
}

/// Parses a now-playing drop file.
///
/// Unknown schema versions are ignored with `Ok(None)`. Valid version 1 files
/// are returned as `Ok(Some(_))`.
///
/// # Errors
///
/// Returns an error when the bytes are not valid JSON, the `schema` field is
/// missing or not a string, or a version 1 file does not match the contract.
pub fn parse(bytes: &[u8]) -> Result<Option<DropFile>> {
    let value: serde_json::Value =
        serde_json::from_slice(bytes).context("failed to parse drop file JSON")?;

    let schema = value
        .get("schema")
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| anyhow!("drop file is missing a string schema field"))?;

    if schema != SCHEMA_VERSION {
        tracing::warn!(schema, "ignoring unknown now-playing drop-file schema");
        return Ok(None);
    }

    serde_json::from_value(value)
        .map(Some)
        .context("drop file does not match musicindex.nowplaying/1")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn valid_dropfile_json() -> &'static str {
        r#"{
          "schema": "musicindex.nowplaying/1",
          "target": "default",
          "artist": "Alice",
          "title": "Some Track",
          "duration_secs": 187.326,
          "image": "https://example.com/art.png",
          "feed_guid": "1c7a-feed",
          "track_guid": "9f3e-track",
          "value_routes": [
            {
              "recipient_name": "Alice",
              "route_type": "node",
              "address": "03ab",
              "split": 90.0,
              "fee": false,
              "custom_key": null,
              "custom_value": null
            }
          ],
          "value_routes_source": "musicindex-api"
        }"#
    }

    #[test]
    fn dropfile_well_formed_file_round_trips_unchanged() -> Result<()> {
        let original: serde_json::Value = serde_json::from_str(valid_dropfile_json())?;
        let parsed = parse(valid_dropfile_json().as_bytes())?
            .ok_or_else(|| anyhow!("expected known schema to parse"))?;
        let serialized = serde_json::to_value(parsed)?;

        assert_eq!(serialized, original);
        Ok(())
    }

    #[test]
    fn dropfile_unknown_schema_returns_none() -> Result<()> {
        let input =
            valid_dropfile_json().replace("musicindex.nowplaying/1", "musicindex.nowplaying/2");

        assert_eq!(parse(input.as_bytes())?, None);
        Ok(())
    }

    #[test]
    fn dropfile_malformed_json_returns_error() {
        let err = parse(br#"{"schema": "musicindex.nowplaying/1", "target": "#);

        assert!(err.is_err());
    }

    #[test]
    fn dropfile_missing_optional_fields_parse() -> Result<()> {
        let input = r#"{
          "schema": "musicindex.nowplaying/1",
          "target": "default",
          "artist": "Alice",
          "title": "Some Track",
          "value_routes": [],
          "value_routes_source": null
        }"#;

        let parsed =
            parse(input.as_bytes())?.ok_or_else(|| anyhow!("expected known schema to parse"))?;

        assert_eq!(parsed.duration_secs, None);
        assert_eq!(parsed.image, None);
        assert_eq!(parsed.feed_guid, None);
        assert_eq!(parsed.track_guid, None);
        assert!(parsed.value_routes.is_empty());
        Ok(())
    }

    #[test]
    fn dropfile_empty_value_routes_array_parse() -> Result<()> {
        let input = valid_dropfile_json().replace(
            r#"[
            {
              "recipient_name": "Alice",
              "route_type": "node",
              "address": "03ab",
              "split": 90.0,
              "fee": false,
              "custom_key": null,
              "custom_value": null
            }
          ]"#,
            "[]",
        );

        let parsed =
            parse(input.as_bytes())?.ok_or_else(|| anyhow!("expected known schema to parse"))?;

        assert!(parsed.value_routes.is_empty());
        Ok(())
    }
}
