//! Podcasting 2.0 live value payload assembly.

use std::time::SystemTime;

use serde::{Deserialize, Serialize};

use crate::{DropFile, PaymentRoute, ShowLogEntryKind};

/// A direct live value payload for the MusicIndex relay.
///
/// This is the raw payload shape consumed by remote value listeners. It is not
/// the relay's wrapped `{event_id, metadata}` form.
#[derive(Debug, Clone, Serialize)]
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
    /// The ID of the Mixxx history row for this play (ADR 0010, ADR 0039).
    /// This field is not serialized; it is used only for pairing with display state.
    #[serde(skip)]
    pub play_id: Option<String>,
    /// The show log facts of this payload (ADR 0017, show log task 002).
    ///
    /// `DropWatcher` fills this field when it builds the payload. The relay
    /// worker reads it to write a `track` or `clear` show log entry after
    /// each publish attempt. Not serialized: a listener app never sees it.
    #[serde(skip)]
    pub log: Option<LogFacts>,
}

/// Equality ignores `log` (ADR 0017, show log task 002).
///
/// The watcher compares a freshly built payload against the last one it
/// built for a path, to skip an unchanged payload
/// (`DropWatcher::payload_for_upsert`). `log` carries the read time of the
/// drop file, so comparing it would make every read of an unchanged file
/// look like a change.
///
/// The destructuring of `self` names every field. A new field is then a
/// compile error here, so it cannot silently drop out of the comparison. A
/// missing field would make two different blocks look equal, and the
/// watcher would not publish the new one.
impl PartialEq for LiveValuePayload {
    fn eq(&self, other: &Self) -> bool {
        let Self {
            title,
            image,
            description,
            kind,
            start_time,
            duration,
            event_guid,
            block_guid,
            feed_guid,
            item_guid,
            line,
            author,
            podcast_name,
            value,
            play_id,
            log: _,
        } = self;
        *title == other.title
            && *image == other.image
            && *description == other.description
            && *kind == other.kind
            && *start_time == other.start_time
            && *duration == other.duration
            && *event_guid == other.event_guid
            && *block_guid == other.block_guid
            && *feed_guid == other.feed_guid
            && *item_guid == other.item_guid
            && *line == other.line
            && *author == other.author
            && *podcast_name == other.podcast_name
            && *value == other.value
            && *play_id == other.play_id
    }
}

/// The show log facts of one payload, carried from the watcher to the relay
/// worker (ADR 0017, show log task 002).
///
/// `DropWatcher` fills this when it reads a drop file, or finds it gone.
/// `observed_at` is the read time of that moment, not the time of a later
/// publish outcome: a retried payload keeps the facts it was built with, so
/// each of its log entries shares the same `observed_at` and the same
/// `kind`.
#[derive(Debug, Clone, PartialEq)]
pub struct LogFacts {
    /// The read time of the drop file, or the time the drop file was found
    /// gone.
    pub observed_at: SystemTime,
    /// `track` or `clear` (ADR 0017 §Entries). A `display` entry never
    /// carries `LogFacts`; a later task writes it on its own path.
    pub kind: ShowLogEntryKind,
    /// `true` when the drop file has value routes. `None` for a `clear`
    /// entry.
    pub payable: Option<bool>,
    /// From the drop file, when present.
    pub artist: Option<String>,
    /// From the drop file, when present.
    pub title: Option<String>,
    /// From the drop file, when present.
    pub album: Option<String>,
    /// From the drop file, when present.
    pub play_id: Option<String>,
    /// From the drop file, when present.
    pub duration_secs: Option<f64>,
    /// From the drop file, when present.
    pub feed_guid: Option<String>,
    /// From the drop file, when present.
    pub track_guid: Option<String>,
    /// The `image` of the drop file, when present.
    pub image_url: Option<String>,
}

impl LogFacts {
    /// Builds the facts of a `clear` entry: the drop file is gone, and no
    /// track plays (ADR 0017 §Entries).
    pub fn clear(observed_at: SystemTime) -> Self {
        Self {
            observed_at,
            kind: ShowLogEntryKind::Clear,
            payable: None,
            artist: None,
            title: None,
            album: None,
            play_id: None,
            duration_secs: None,
            feed_guid: None,
            track_guid: None,
            image_url: None,
        }
    }

    /// Builds the facts of a `track` entry from a parsed drop file
    /// (ADR 0017 §Entries).
    ///
    /// `payable` is `true` when the drop file carries at least one value
    /// route. These facts come from the drop file itself, so a non-payable
    /// track keeps its artist, title and other facts even though its
    /// payload is the dead block.
    pub fn track(observed_at: SystemTime, dropfile: &DropFile) -> Self {
        Self {
            observed_at,
            kind: ShowLogEntryKind::Track,
            payable: Some(has_payable_routes(dropfile)),
            artist: Some(dropfile.artist.clone()),
            title: Some(dropfile.title.clone()),
            album: dropfile.album.clone(),
            play_id: dropfile.play_id.clone(),
            duration_secs: dropfile.duration_secs,
            feed_guid: dropfile.feed_guid.clone(),
            track_guid: dropfile.track_guid.clone(),
            image_url: dropfile.image.clone(),
        }
    }
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
        title: track_payload_title(&dropfile.artist, &dropfile.title),
        // ADR 0034 rule 1: an empty `image` is no image.
        image: dropfile.image.clone().filter(|image| !image.is_empty()),
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
        log: None,
    }
}

/// Returns whether a drop file can pay its value routes.
pub(crate) fn has_payable_routes(dropfile: &DropFile) -> bool {
    !dropfile.value_routes.is_empty()
        && dropfile
            .feed_guid
            .as_deref()
            .is_some_and(|guid| !guid.trim().is_empty())
        && dropfile
            .track_guid
            .as_deref()
            .is_some_and(|guid| !guid.trim().is_empty())
}

/// Default title when a target has no `dead_block_title` (ADR 0029).
pub const DEFAULT_DEAD_BLOCK_TITLE: &str = "No V4V track playing";

/// The settings of a target for its dead block (ADR 0029 §The Dead Block).
///
/// The settings change only `title` and `image`. The model and the
/// destination of the dead block stay constants.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DeadBlockSettings {
    /// The `title` of the dead block.
    pub title: String,
    /// The `image` URL of the dead block. `None` gives no `image`.
    pub image: Option<String>,
}

impl Default for DeadBlockSettings {
    /// Gives the title "No V4V track playing" and no image.
    fn default() -> Self {
        Self {
            title: DEFAULT_DEAD_BLOCK_TITLE.to_owned(),
            image: None,
        }
    }
}

/// Builds the track title from the trimmed artist and title (ADR 0029 §The Title Of A Track Block).
fn track_payload_title(artist: &str, title: &str) -> String {
    match (artist.trim(), title.trim()) {
        ("", title) => title.to_owned(),
        (artist, "") => artist.to_owned(),
        (artist, title) => format!("{artist} - {title}"),
    }
}

/// Fixed payment-destination name for the dead block (ADR 0033).
const DEAD_BLOCK_RECIPIENT_NAME: &str = "No V4V payment route";

/// Fixed lnaddress for the dead block (ADR 0033).
///
/// A payment to this address fails, so the dead block pays nobody.
const DEAD_BLOCK_ADDRESS: &str = "no-v4v-track@example.invalid";

/// Builds the dead block with the title and the image of the target (ADR 0029).
///
/// The model and the destination are constants (ADR 0033). The block carries
/// no `feedGuid` or `itemGuid`. The caller supplies a fresh `block_guid` for
/// each publish.
pub fn dead_payload(
    event_guid: &str,
    block_guid: &str,
    settings: &DeadBlockSettings,
) -> LiveValuePayload {
    LiveValuePayload {
        title: settings.title.clone(),
        image: settings.image.clone(),
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
        log: None,
    }
}

/// Returns whether `value` is a dead block: only the dead block address, or no
/// destination. A publisher of the ADR 0031 version can leave a dead block with
/// no destination on the relay (ADR 0033).
pub(crate) fn is_dead_block(value: &LiveValue) -> bool {
    value
        .destinations
        .iter()
        .all(|destination| destination.address.as_deref() == Some(DEAD_BLOCK_ADDRESS))
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
    fn an_empty_drop_file_image_gives_no_payload_image() {
        let mut track = dropfile();
        track.image = Some(String::new());
        let payload = payload_from_dropfile(&track, "event", "block");
        assert_eq!(payload.image, None);
    }

    #[test]
    fn track_payload_title_uses_artist_and_trimmed_title() {
        let mut track = dropfile();
        track.artist = " Anni Powell ".to_owned();
        track.title = " Beyond the Hills ".to_owned();
        let payload = payload_from_dropfile(&track, "event", "block");
        assert_eq!(payload.title, "Anni Powell - Beyond the Hills");
        assert_eq!(
            payload.line,
            Some(vec![
                " Beyond the Hills ".to_owned(),
                " Anni Powell ".to_owned()
            ])
        );
        assert_eq!(payload.author.as_deref(), Some(" Anni Powell "));

        track.artist.clear();
        let payload = payload_from_dropfile(&track, "event", "block");
        assert_eq!(payload.title, "Beyond the Hills");
    }

    #[test]
    fn dead_block_title_does_not_change_its_value() {
        let settings = DeadBlockSettings {
            title: "Station".to_owned(),
            image: None,
        };
        let station = dead_payload("e", "b", &settings);
        let default = dead_payload("e", "b", &DeadBlockSettings::default());
        assert_eq!(station.title, "Station");
        assert_eq!(station.value, default.value);
        assert!(station.image.is_none());
        assert!(station.feed_guid.is_none());
        assert!(station.item_guid.is_none());
    }

    #[test]
    fn dead_block_image_does_not_change_its_value() -> Result<()> {
        let settings = DeadBlockSettings {
            title: "Station".to_owned(),
            image: Some("https://example.com/station.png".to_owned()),
        };
        let station = dead_payload("e", "b", &settings);
        let default = dead_payload("e", "b", &DeadBlockSettings::default());
        assert_eq!(station.value, default.value);
        assert!(is_dead_block(&station.value));
        let json = serde_json::to_value(&station)?;
        assert_eq!(json["image"], "https://example.com/station.png");
        assert!(json.get("feedGuid").is_none());
        assert!(json.get("itemGuid").is_none());
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
        let payload = dead_payload("event-guid", "block-guid", &DeadBlockSettings::default());
        let value = serde_json::to_value(&payload)?;

        assert_eq!(value["title"], "No V4V track playing");
        assert!(value.get("image").is_none());
        assert!(value.get("duration").is_none());
        assert!(value.get("feedGuid").is_none());
        assert!(value.get("itemGuid").is_none());
        assert_eq!(value["value"]["model"]["type"], "lightning");
        assert_eq!(value["value"]["model"]["method"], "lnaddress");
        // ADR 0033: one destination that a payment cannot reach. The
        // v4vmusic.com client rejects a block with no destination.
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
    fn livevalue_dead_block_is_the_dead_address_or_no_destination() {
        let dead = dead_payload("event-guid", "block-guid", &DeadBlockSettings::default());
        assert!(is_dead_block(&dead.value));

        let mut empty = dead.clone();
        empty.value.destinations.clear();
        assert!(is_dead_block(&empty.value));

        let payable = payload_from_dropfile(&dropfile(), "event-guid", "block-guid");
        assert!(!is_dead_block(&payable.value));
    }

    #[test]
    fn livevalue_two_dead_payloads_only_differ_by_block_guid() -> Result<()> {
        let first = serde_json::to_value(dead_payload(
            "event-guid",
            "block-guid-1",
            &DeadBlockSettings::default(),
        ))?;
        let second = serde_json::to_value(dead_payload(
            "event-guid",
            "block-guid-2",
            &DeadBlockSettings::default(),
        ))?;

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
        let payload = dead_payload("event-guid", "block-guid", &DeadBlockSettings::default());
        let value = serde_json::to_value(payload)?;

        assert!(value.get("link").is_none());
        Ok(())
    }

    #[test]
    fn livevalue_dead_payload_has_no_new_fields() -> Result<()> {
        let payload = dead_payload("event-guid", "block-guid", &DeadBlockSettings::default());
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
