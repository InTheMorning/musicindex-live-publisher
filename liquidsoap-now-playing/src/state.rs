//! Parse the versioned Liquidsoap state file.

use serde::Deserialize;

/// The supported Liquidsoap state format.
pub const STATE_FORMAT: &str = "musicindex-liquidsoap-state/1";
/// The format marker for the Liquidsoap state file with chapter data.
pub const STATE_FORMAT_2: &str = "musicindex-liquidsoap-state/2";

/// The format marker read before the full state record.
#[derive(Debug, Deserialize)]
pub struct StateHeader {
    /// The state format identifier.
    pub format: String,
}

/// The fields in the Liquidsoap state file.
#[derive(Debug, Clone, Deserialize)]
pub struct StateFile {
    pub format: String,
    pub seq: i64,
    pub written: f64,
    pub live: bool,
    #[serde(default)]
    pub jingle: bool,
    pub track_started: f64,
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub artist: String,
    /// The ICY title of the file (ADR 0041 §The State File Contract). A missing field is the same as an empty field.
    #[serde(default)]
    pub song: String,
    #[serde(default)]
    pub album: Option<String>,
    #[serde(default)]
    pub pub_date: Option<String>,
    #[serde(default)]
    pub track_guid: Option<String>,
    #[serde(default)]
    pub feed_guid: Option<String>,
    #[serde(default)]
    pub image: Option<String>,
    /// The embedded picture file name in the state folder (ADR 0041 §The Producer Reads The Picture File).
    #[serde(default)]
    pub picture: String,
    #[serde(default)]
    pub value_routes: Option<String>,
    #[serde(default)]
    pub elapsed: Option<f64>,
    #[serde(default)]
    pub cue_in: Option<f64>,
    #[serde(default)]
    pub chapters: Option<String>,
}
