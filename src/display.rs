//! The display state of ADR 0008, as the publisher reads and sends it.
//!
//! The producer writes `DIR/display.json` with the schema
//! `musicindex.display/1`. For an embedded image, it writes the image to
//! `DIR/<sha256>.jpg` or `DIR/<sha256>.png` before `display.json`. The
//! publisher reads `display.json` and the image bytes at once, because the
//! producer can delete the image before the stream delay ends.
//!
//! The relay owns the wire format (`musicindex-live-relay` ADR 0003). The
//! display publish body holds only the key `track`. The `schema` key of
//! `display.json` never goes to the relay, because the relay refuses an
//! unknown key with `400 invalid_display`.

use std::fmt;
use std::fs::File;
use std::io::{self, Read};
use std::path::Path;
use std::sync::Arc;

use serde_json::{Value, json};
use sha2::{Digest, Sha256};

/// The schema of `display.json` that this publisher reads.
pub const DISPLAY_SCHEMA: &str = "musicindex.display/1";

/// The file name of the display state in a display directory.
pub const DISPLAY_FILE_NAME: &str = "display.json";

/// The largest image, in bytes, that the publisher reads and uploads.
///
/// This is the default `ARTWORK_MAX_BYTES` of the relay (relay ADR 0003). The
/// producer sends no larger image (ADR 0008 §The Embedded Image).
pub const MAX_IMAGE_BYTES: u64 = 524_288;

/// The type of an embedded image.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImageMime {
    Jpeg,
    Png,
}

impl ImageMime {
    /// The MIME type text of the display state.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Jpeg => "image/jpeg",
            Self::Png => "image/png",
        }
    }

    /// The file name extension that the producer uses for this type.
    pub fn extension(self) -> &'static str {
        match self {
            Self::Jpeg => "jpg",
            Self::Png => "png",
        }
    }

    fn from_mime(mime: &str) -> Option<Self> {
        match mime {
            "image/jpeg" => Some(Self::Jpeg),
            "image/png" => Some(Self::Png),
            _ => None,
        }
    }

    /// True when `bytes` start with the signature of this type. The relay
    /// gets the stored type from these bytes only.
    fn matches(self, bytes: &[u8]) -> bool {
        match self {
            Self::Jpeg => bytes.starts_with(&[0xFF, 0xD8, 0xFF]),
            Self::Png => bytes.starts_with(&[0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A]),
        }
    }
}

/// An embedded image and its bytes, read from the display directory.
///
/// The bytes are shared, so one image costs its size one time, also when
/// several targets read the same display directory.
#[derive(Clone)]
pub struct ArtworkImage {
    sha256: String,
    mime: ImageMime,
    bytes: Arc<[u8]>,
}

impl ArtworkImage {
    /// Makes an image from its bytes. The SHA-256 comes from the bytes.
    pub fn from_bytes(mime: ImageMime, bytes: impl Into<Arc<[u8]>>) -> Self {
        let bytes = bytes.into();
        Self {
            sha256: sha256_hex(&bytes),
            mime,
            bytes,
        }
    }

    /// The SHA-256 of the bytes, as 64 lowercase hexadecimal characters.
    pub fn sha256(&self) -> &str {
        &self.sha256
    }

    /// The image type.
    pub fn mime(&self) -> ImageMime {
        self.mime
    }

    /// The image bytes.
    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }
}

/// Two images are equal when their SHA-256 and their type are equal. The
/// SHA-256 identifies the bytes.
impl PartialEq for ArtworkImage {
    fn eq(&self, other: &Self) -> bool {
        self.sha256 == other.sha256 && self.mime == other.mime
    }
}

impl Eq for ArtworkImage {}

/// The `Debug` output gives the length, not the bytes.
impl fmt::Debug for ArtworkImage {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ArtworkImage")
            .field("sha256", &self.sha256)
            .field("mime", &self.mime)
            .field("len", &self.bytes.len())
            .finish()
    }
}

/// The artwork of a display track.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Artwork {
    /// An embedded image that the publisher uploads to the relay.
    Image(ArtworkImage),
    /// An image that a client loads from its own host. No upload.
    Url(String),
}

/// One display track.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DisplayTrack {
    pub artist: String,
    pub title: String,
    pub artwork: Option<Artwork>,
}

/// The display state of one target: `null` or one track.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct DisplayState {
    pub track: Option<DisplayTrack>,
}

impl DisplayState {
    /// The `null` display state.
    pub fn null() -> Self {
        Self { track: None }
    }

    /// The body of `POST /v1/liveitems/{event_id}/display`.
    ///
    /// The body holds only the key `track` (relay ADR 0003). It never holds
    /// the `schema` key of `display.json`.
    pub fn body(&self) -> Value {
        let track = self.track.as_ref().map(|track| {
            let artwork = match &track.artwork {
                None => Value::Null,
                Some(Artwork::Image(image)) => {
                    json!({ "sha256": image.sha256(), "mime": image.mime().as_str() })
                }
                Some(Artwork::Url(url)) => json!({ "url": url }),
            };
            json!({ "artist": track.artist, "title": track.title, "artwork": artwork })
        });
        json!({ "track": track })
    }

    /// The embedded image of this state, if it has one.
    pub fn image(&self) -> Option<&ArtworkImage> {
        match self.track.as_ref()?.artwork.as_ref()? {
            Artwork::Image(image) => Some(image),
            Artwork::Url(_) => None,
        }
    }

    /// The same state with `artwork: null` in place of an embedded image.
    pub fn without_image(&self) -> Self {
        let mut state = self.clone();
        if let Some(track) = state.track.as_mut()
            && matches!(track.artwork, Some(Artwork::Image(_)))
        {
            track.artwork = None;
        }
        state
    }
}

/// A display state for one target, in the stream-delay schedule.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DisplayEntry {
    /// The event GUID of the target.
    pub event_id: String,
    pub state: DisplayState,
}

/// Reads `DIR/display.json` and the image that it names.
///
/// Gives `None` when the file is absent, cannot be read, is not JSON, has an
/// unknown schema, or does not have the shape of `musicindex.display/1`. The
/// caller then ignores the file. Each case other than an absent file gives a
/// warning.
///
/// An embedded image that is missing, larger than [`MAX_IMAGE_BYTES`], has a
/// SHA-256 different from its file name, or does not start with the bytes of
/// its type gives the display state with `artwork: null` and a warning.
pub fn read_display_state(dir: &Path) -> Option<DisplayState> {
    let path = dir.join(DISPLAY_FILE_NAME);
    let text = match std::fs::read_to_string(&path) {
        Ok(text) => text,
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            tracing::debug!(path = %path.display(), "no display.json in the display directory");
            return None;
        }
        Err(error) => {
            tracing::warn!(path = %path.display(), %error, "cannot read display.json; ignoring it");
            return None;
        }
    };
    let value: Value = match serde_json::from_str(&text) {
        Ok(value) => value,
        Err(error) => {
            tracing::warn!(path = %path.display(), %error, "display.json is not JSON; ignoring it");
            return None;
        }
    };
    let schema = value.get("schema").and_then(Value::as_str);
    if schema != Some(DISPLAY_SCHEMA) {
        tracing::warn!(
            path = %path.display(),
            schema = schema.unwrap_or("<none>"),
            "display.json has an unknown schema; ignoring it"
        );
        return None;
    }
    let track = match value.get("track") {
        None => {
            tracing::warn!(path = %path.display(), "display.json has no track key; ignoring it");
            return None;
        }
        Some(Value::Null) => None,
        Some(track) => Some(parse_track(dir, &path, track)?),
    };
    Some(DisplayState { track })
}

fn parse_track(dir: &Path, path: &Path, track: &Value) -> Option<DisplayTrack> {
    let artist = track.get("artist").and_then(Value::as_str);
    let title = track.get("title").and_then(Value::as_str);
    let (Some(artist), Some(title)) = (artist, title) else {
        tracing::warn!(path = %path.display(), "display.json track has no artist or title; ignoring it");
        return None;
    };
    let artwork = match track.get("artwork") {
        None | Some(Value::Null) => None,
        Some(artwork) => parse_artwork(dir, artwork),
    };
    Some(DisplayTrack {
        artist: artist.to_owned(),
        title: title.to_owned(),
        artwork,
    })
}

fn parse_artwork(dir: &Path, artwork: &Value) -> Option<Artwork> {
    if let Some(url) = artwork.get("url").and_then(Value::as_str) {
        return Some(Artwork::Url(url.to_owned()));
    }
    let sha256 = artwork.get("sha256").and_then(Value::as_str);
    let mime = artwork.get("mime").and_then(Value::as_str);
    let (Some(sha256), Some(mime)) = (sha256, mime) else {
        tracing::warn!(dir = %dir.display(), "display.json artwork has an unknown form; using artwork null");
        return None;
    };
    load_image(dir, sha256, mime).map(Artwork::Image)
}

/// Reads one embedded image and checks it before an upload.
fn load_image(dir: &Path, sha256: &str, mime: &str) -> Option<ArtworkImage> {
    if !is_sha256_hex(sha256) {
        tracing::warn!(dir = %dir.display(), "display.json artwork sha256 is not 64 lowercase hex characters; using artwork null");
        return None;
    }
    let Some(mime) = ImageMime::from_mime(mime) else {
        tracing::warn!(dir = %dir.display(), mime, "display.json artwork has an unknown mime; using artwork null");
        return None;
    };
    let path = dir.join(format!("{sha256}.{}", mime.extension()));
    let bytes = match read_bounded(&path, MAX_IMAGE_BYTES) {
        Ok(Some(bytes)) => bytes,
        Ok(None) => {
            tracing::warn!(
                path = %path.display(),
                limit = MAX_IMAGE_BYTES,
                "image is larger than the limit; using artwork null"
            );
            return None;
        }
        Err(error) => {
            tracing::warn!(path = %path.display(), %error, "cannot read image; using artwork null");
            return None;
        }
    };
    let image = ArtworkImage::from_bytes(mime, bytes);
    if image.sha256() != sha256 {
        tracing::warn!(
            path = %path.display(),
            actual = image.sha256(),
            "image SHA-256 is not its file name; using artwork null"
        );
        return None;
    }
    if !mime.matches(image.bytes()) {
        tracing::warn!(
            path = %path.display(),
            mime = mime.as_str(),
            "image bytes are not of their type; using artwork null"
        );
        return None;
    }
    Some(image)
}

/// Reads at most `limit` bytes. Gives `None` for a larger file, with no
/// further read.
fn read_bounded(path: &Path, limit: u64) -> io::Result<Option<Vec<u8>>> {
    let file = File::open(path)?;
    let mut bytes = Vec::new();
    file.take(limit.saturating_add(1)).read_to_end(&mut bytes)?;
    if u64::try_from(bytes.len()).unwrap_or(u64::MAX) > limit {
        return Ok(None);
    }
    Ok(Some(bytes))
}

fn is_sha256_hex(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn sha256_hex(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sha256_of_empty_input_is_the_known_value() {
        assert_eq!(
            sha256_hex(b""),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
    }

    #[test]
    fn the_body_of_a_null_state_is_a_null_track() {
        assert_eq!(DisplayState::null().body(), json!({ "track": null }));
    }
}
