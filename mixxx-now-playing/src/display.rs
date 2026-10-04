//! The display output of ADR 0008 §The Producer Output.
//!
//! With `--display-dir DIR`, the producer writes `DIR/display.json` and the
//! image of the present track. This module holds the artwork source, the
//! embedded image rules, the writes and the image retention. The payment
//! path does not use this module.

use std::fs;
use std::io::{Cursor, ErrorKind};
use std::path::{Component, Path, PathBuf};

use anyhow::{Context, Result, anyhow};
use image::codecs::jpeg::JpegEncoder;
use image::{ImageFormat, ImageReader, Limits};
use serde::Serialize;
use sha2::{Digest, Sha256};

use crate::tags::read_tags_with_picture;

/// The schema of `display.json`.
pub const DISPLAY_SCHEMA: &str = "musicindex.display/1";
/// The file name of the display state in `DIR`.
pub const DISPLAY_FILE_NAME: &str = "display.json";
/// The producer rejects picture data larger than this value, in bytes.
pub const MAX_PICTURE_BYTES: usize = 64 * 1024 * 1024;
/// The producer rejects an image with a longer side, in pixels. It reads the
/// side from the header before a decode.
pub const MAX_SOURCE_SIDE: u32 = 4_000;
/// The longest side of an image that goes out, in pixels.
pub const MAX_OUTPUT_SIDE: u32 = 1_000;
/// The largest image that goes out, in bytes.
pub const MAX_OUTPUT_BYTES: usize = 524_288;
/// The longest image URL that the artwork can use, in characters.
pub const MAX_URL_CHARS: usize = 2_048;

/// The JPEG quality of a reduced image.
const JPEG_QUALITY: u8 = 85;
/// The decoder allocation limit. A 4,000 by 4,000 RGBA image fits in it.
const DECODE_ALLOC_LIMIT: u64 = 64 * 1024 * 1024;

/// The type of an image that goes out.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImageMime {
    /// A JPEG image.
    Jpeg,
    /// A PNG image.
    Png,
}

impl ImageMime {
    /// The MIME type text of `display.json`.
    pub fn mime(self) -> &'static str {
        match self {
            Self::Jpeg => "image/jpeg",
            Self::Png => "image/png",
        }
    }

    /// The file name extension of the image file.
    pub fn extension(self) -> &'static str {
        match self {
            Self::Jpeg => "jpg",
            Self::Png => "png",
        }
    }

    fn format(self) -> ImageFormat {
        match self {
            Self::Jpeg => ImageFormat::Jpeg,
            Self::Png => ImageFormat::Png,
        }
    }

    fn from_extension(extension: &str) -> Option<Self> {
        match extension {
            "jpg" => Some(Self::Jpeg),
            "png" => Some(Self::Png),
            _ => None,
        }
    }

    fn from_mime(mime: &str) -> Option<Self> {
        match mime {
            "image/jpeg" => Some(Self::Jpeg),
            "image/png" => Some(Self::Png),
            _ => None,
        }
    }
}

/// An image that goes out to `DIR`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImageFile {
    sha256: String,
    mime: ImageMime,
    bytes: Vec<u8>,
}

impl ImageFile {
    fn new(mime: ImageMime, bytes: Vec<u8>) -> Self {
        Self {
            sha256: sha256_hex(&bytes),
            mime,
            bytes,
        }
    }

    /// The SHA-256 of the bytes, as 64 lowercase hex characters.
    pub fn sha256(&self) -> &str {
        &self.sha256
    }

    /// The type of the image.
    pub fn mime(&self) -> ImageMime {
        self.mime
    }

    /// The bytes that go out.
    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }

    /// The file name in `DIR`: `<sha256>.jpg` or `<sha256>.png`.
    pub fn file_name(&self) -> String {
        format!("{}.{}", self.sha256, self.mime.extension())
    }
}

/// The artwork of a display track (ADR 0008 §The Producer Output). `None`
/// in an `Option<Artwork>` is the `null` artwork.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Artwork {
    /// An image URL that the app loads from its own host.
    Url(String),
    /// An embedded image that the producer writes into `DIR`.
    Image(ImageFile),
}

/// The cause when an embedded picture gives no image.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Rejection {
    /// The data is larger than `MAX_PICTURE_BYTES`.
    TooLarge,
    /// The first bytes are not JPEG or PNG.
    Format,
    /// The decoder cannot read the header.
    Header,
    /// A side in the header is longer than `MAX_SOURCE_SIDE`.
    SideTooLong,
    /// The decode failed.
    Decode,
    /// The JPEG encode failed.
    Encode,
    /// The reduced image is larger than `MAX_OUTPUT_BYTES`.
    ReducedTooLarge,
}

/// Gives the image URL when `value` is an `http` or `https` URL of at most
/// `MAX_URL_CHARS` characters (ADR 0008 §The Artwork Source).
///
/// The function does not change `value`. Thus the URL is the same value as
/// `image` in the drop file. An empty value is not a URL.
pub fn image_url(value: &str) -> Option<&str> {
    if value.chars().count() > MAX_URL_CHARS {
        return None;
    }
    let (scheme, rest) = value.split_once("://")?;
    let scheme_ok = scheme.eq_ignore_ascii_case("http") || scheme.eq_ignore_ascii_case("https");
    (scheme_ok && !rest.is_empty()).then_some(value)
}

/// Gives the image type from the first bytes of `data`. The type text in
/// the tag is not used.
pub fn sniff_image(data: &[u8]) -> Option<ImageMime> {
    const JPEG: &[u8] = &[0xFF, 0xD8, 0xFF];
    const PNG: &[u8] = &[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];
    if data.starts_with(JPEG) {
        Some(ImageMime::Jpeg)
    } else if data.starts_with(PNG) {
        Some(ImageMime::Png)
    } else {
        None
    }
}

/// Applies the rules of ADR 0008 §The Embedded Image to the picture data.
///
/// The function reads the pixel size from the header before it decodes the
/// image. An image that goes out unchanged is not decoded.
///
/// # Errors
///
/// Returns the `Rejection` when the picture gives no image.
pub fn prepare_image(data: &[u8]) -> Result<ImageFile, Rejection> {
    if data.len() > MAX_PICTURE_BYTES {
        return Err(Rejection::TooLarge);
    }
    let mime = sniff_image(data).ok_or(Rejection::Format)?;
    let (width, height) = reader(data, mime)
        .into_dimensions()
        .map_err(|_| Rejection::Header)?;
    let longest = width.max(height);
    if longest > MAX_SOURCE_SIDE {
        return Err(Rejection::SideTooLong);
    }
    if data.len() <= MAX_OUTPUT_BYTES && longest <= MAX_OUTPUT_SIDE {
        return Ok(ImageFile::new(mime, data.to_vec()));
    }
    reduce(data, mime, longest)
}

/// Decodes the image, reduces it to `MAX_OUTPUT_SIDE` on its longest side
/// and encodes it as JPEG.
fn reduce(data: &[u8], mime: ImageMime, longest: u32) -> Result<ImageFile, Rejection> {
    let mut reader = reader(data, mime);
    let mut limits = Limits::default();
    limits.max_image_width = Some(MAX_SOURCE_SIDE);
    limits.max_image_height = Some(MAX_SOURCE_SIDE);
    limits.max_alloc = Some(DECODE_ALLOC_LIMIT);
    reader.limits(limits);
    let decoded = reader.decode().map_err(|_| Rejection::Decode)?;
    let reduced = if longest > MAX_OUTPUT_SIDE {
        decoded.thumbnail(MAX_OUTPUT_SIDE, MAX_OUTPUT_SIDE)
    } else {
        decoded
    };
    let rgb = reduced.to_rgb8();
    let mut bytes = Vec::new();
    JpegEncoder::new_with_quality(&mut bytes, JPEG_QUALITY)
        .encode_image(&rgb)
        .map_err(|_| Rejection::Encode)?;
    if bytes.len() > MAX_OUTPUT_BYTES {
        return Err(Rejection::ReducedTooLarge);
    }
    Ok(ImageFile::new(ImageMime::Jpeg, bytes))
}

fn reader(data: &[u8], mime: ImageMime) -> ImageReader<Cursor<&[u8]>> {
    ImageReader::with_format(Cursor::new(data), mime.format())
}

/// Gives the artwork of an embedded picture. No picture, or a picture that
/// the rules reject, gives `None`.
pub fn embedded_artwork(picture: Option<&[u8]>) -> Option<Artwork> {
    match prepare_image(picture?) {
        Ok(image) => Some(Artwork::Image(image)),
        Err(rejection) => {
            tracing::debug!(?rejection, "embedded picture gives no artwork");
            None
        }
    }
}

/// Gives the artwork of a V4V track. An image URL in `image_tag` wins. Else
/// the track uses its embedded picture (ADR 0008 §The Artwork Source).
pub fn v4v_artwork(image_tag: Option<&str>, picture: Option<&[u8]>) -> Option<Artwork> {
    match image_tag.and_then(image_url) {
        Some(url) => Some(Artwork::Url(url.to_owned())),
        None => embedded_artwork(picture),
    }
}

/// Gives the artwork of a track that is not V4V, from the embedded picture
/// of `path`. The producer reads the file one time. A file that the producer
/// cannot read gives `None`.
pub fn file_artwork(path: &Path) -> Option<Artwork> {
    match read_tags_with_picture(path) {
        Ok((_, picture)) => embedded_artwork(picture.as_deref()),
        Err(error) => {
            tracing::debug!(path = %path.display(), error = %format!("{error:#}"), "no picture read");
            None
        }
    }
}

/// The track that `display.json` shows.
#[derive(Debug, Clone, Copy)]
pub struct ShownTrack<'a> {
    /// The artist of the history row, with no change.
    pub artist: &'a str,
    /// The title of the history row, with no change.
    pub title: &'a str,
    /// The artwork, or `None` for the `null` artwork.
    pub artwork: Option<&'a Artwork>,
}

#[derive(Serialize)]
struct DisplayJson<'a> {
    schema: &'static str,
    track: Option<TrackJson<'a>>,
}

#[derive(Serialize)]
struct TrackJson<'a> {
    artist: &'a str,
    title: &'a str,
    artwork: Option<ArtworkJson<'a>>,
}

#[derive(Serialize)]
#[serde(untagged)]
enum ArtworkJson<'a> {
    Url { url: &'a str },
    Image { sha256: &'a str, mime: &'static str },
}

/// Renders the content of `display.json`.
///
/// # Errors
///
/// Returns an error when the JSON encode fails.
pub fn render_display_json(track: Option<ShownTrack<'_>>) -> Result<String> {
    let document = DisplayJson {
        schema: DISPLAY_SCHEMA,
        track: track.map(|track| TrackJson {
            artist: track.artist,
            title: track.title,
            artwork: track.artwork.map(|artwork| match artwork {
                Artwork::Url(url) => ArtworkJson::Url { url },
                Artwork::Image(image) => ArtworkJson::Image {
                    sha256: image.sha256(),
                    mime: image.mime().mime(),
                },
            }),
        }),
    };
    let mut json = serde_json::to_string_pretty(&document).context("render display.json")?;
    json.push('\n');
    Ok(json)
}

/// The writer of `DIR/display.json` and of the images in `DIR`.
#[derive(Debug)]
pub struct DisplayOutput {
    dir: PathBuf,
    last: Option<String>,
    present: Option<String>,
    previous: Option<String>,
}

impl DisplayOutput {
    /// Makes a writer for `dir`. This function does no I/O.
    pub fn new(dir: impl Into<PathBuf>) -> Self {
        Self {
            dir: dir.into(),
            last: None,
            present: None,
            previous: None,
        }
    }

    /// The display directory.
    pub fn dir(&self) -> &Path {
        &self.dir
    }

    /// Writes the display state `null` at startup.
    ///
    /// The function makes `dir` when it is missing. It reads the present
    /// `display.json`, so it does not rewrite a file with the same content,
    /// and so the image of that state stays as the previous image.
    ///
    /// # Errors
    ///
    /// Returns an error when the directory or a write fails.
    pub fn start(&mut self) -> Result<()> {
        fs::create_dir_all(&self.dir)
            .with_context(|| format!("create display directory {}", self.dir.display()))?;
        if let Ok(content) = fs::read_to_string(self.dir.join(DISPLAY_FILE_NAME)) {
            self.present = image_in_display_json(&content);
            self.last = Some(content);
        }
        self.write(None)
    }

    /// Writes the display state. `None` is the display state `null`.
    ///
    /// The image goes into `DIR` before `display.json`. Each file goes to a
    /// temporary file and then a rename. When the content of `display.json`
    /// does not change, the function writes nothing. After a write, only the
    /// image of the present state and of the state before it stay in `DIR`.
    ///
    /// # Errors
    ///
    /// Returns an error when a write, a rename or a removal fails.
    pub fn write(&mut self, track: Option<ShownTrack<'_>>) -> Result<()> {
        let json = render_display_json(track)?;
        if self.last.as_deref() == Some(json.as_str()) {
            return Ok(());
        }
        let image = match track.and_then(|track| track.artwork) {
            Some(Artwork::Image(image)) => {
                let name = image.file_name();
                let path = self.dir.join(&name);
                if !path.is_file() {
                    write_atomic(&path, image.bytes())?;
                }
                Some(name)
            }
            Some(Artwork::Url(_)) | None => None,
        };
        write_atomic(&self.dir.join(DISPLAY_FILE_NAME), json.as_bytes())?;
        self.last = Some(json);
        self.previous = std::mem::replace(&mut self.present, image);
        self.retain()
    }

    /// Deletes each image file in `DIR` other than the present image and
    /// the previous image. Other files in `DIR` stay.
    fn retain(&self) -> Result<()> {
        let entries = fs::read_dir(&self.dir)
            .with_context(|| format!("list display directory {}", self.dir.display()))?;
        for entry in entries {
            let entry =
                entry.with_context(|| format!("list display directory {}", self.dir.display()))?;
            let name = entry.file_name();
            let Some(name) = name.to_str() else {
                continue;
            };
            if !is_image_file_name(name)
                || self.present.as_deref() == Some(name)
                || self.previous.as_deref() == Some(name)
            {
                continue;
            }
            match fs::remove_file(entry.path()) {
                Ok(()) => {}
                Err(error) if error.kind() == ErrorKind::NotFound => {}
                Err(error) => {
                    return Err(error)
                        .with_context(|| format!("remove old image {}", entry.path().display()));
                }
            }
        }
        Ok(())
    }
}

/// Writes the display state `null` before the producer exits. A shutdown
/// guard uses this function, because it holds only the path.
///
/// # Errors
///
/// Returns an error when a write fails.
pub fn write_null_at_exit(dir: &Path) -> Result<()> {
    DisplayOutput::new(dir).start()
}

/// Gives the image file name that a `display.json` content names.
fn image_in_display_json(content: &str) -> Option<String> {
    let value: serde_json::Value = serde_json::from_str(content).ok()?;
    let artwork = value.get("track")?.get("artwork")?;
    let sha256 = artwork.get("sha256")?.as_str()?;
    let mime = ImageMime::from_mime(artwork.get("mime")?.as_str()?)?;
    let name = format!("{sha256}.{}", mime.extension());
    is_image_file_name(&name).then_some(name)
}

/// True for `<64 lowercase hex characters>.jpg` and `.png`.
fn is_image_file_name(name: &str) -> bool {
    let Some((stem, extension)) = name.split_once('.') else {
        return false;
    };
    stem.len() == 64
        && stem
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
        && ImageMime::from_extension(extension).is_some()
}

/// Writes `bytes` to a temporary file in the same directory, then renames it
/// to `path` (AGENTS.md §6).
fn write_atomic(path: &Path, bytes: &[u8]) -> Result<()> {
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    let name = path
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| anyhow!("output path has no UTF-8 file name: {}", path.display()))?;
    let temp_path = parent.join(format!(".{name}.{}.tmp", std::process::id()));
    fs::write(&temp_path, bytes)
        .with_context(|| format!("write temporary output {}", temp_path.display()))?;
    if let Err(error) = fs::rename(&temp_path, path) {
        let _ = fs::remove_file(&temp_path);
        return Err(error).with_context(|| {
            format!(
                "rename temporary output {} to {}",
                temp_path.display(),
                path.display()
            )
        });
    }
    Ok(())
}

/// True when `a` and `b` name the same directory. The function resolves
/// each path when it exists. Else it compares the absolute paths, with `.`
/// and `..` removed.
pub fn same_directory(a: &Path, b: &Path) -> bool {
    normalize(a) == normalize(b)
}

fn normalize(path: &Path) -> PathBuf {
    if let Ok(resolved) = path.canonicalize() {
        return resolved;
    }
    let absolute = std::path::absolute(path).unwrap_or_else(|_| path.to_path_buf());
    let mut clean = PathBuf::new();
    for component in absolute.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                clean.pop();
            }
            other => clean.push(other.as_os_str()),
        }
    }
    clean
}

/// Gives the SHA-256 of `data` as 64 lowercase hex characters.
pub fn sha256_hex(data: &[u8]) -> String {
    format!("{:x}", Sha256::digest(data))
}

#[cfg(test)]
mod tests {
    use std::time::SystemTime;

    use image::{DynamicImage, ImageBuffer, Rgb, RgbImage};
    use tempfile::TempDir;

    use super::*;

    fn encode(image: &RgbImage, format: ImageFormat) -> Vec<u8> {
        let mut bytes = Cursor::new(Vec::new());
        DynamicImage::ImageRgb8(image.clone())
            .write_to(&mut bytes, format)
            .expect("encode test image");
        bytes.into_inner()
    }

    fn gradient(width: u32, height: u32) -> RgbImage {
        ImageBuffer::from_fn(width, height, |x, y| {
            Rgb([(x % 256) as u8, (y % 256) as u8, ((x + y) % 256) as u8])
        })
    }

    /// Pixel values from a small linear congruential generator. The data
    /// does not compress well.
    fn noise(width: u32, height: u32, amplitude: u8) -> RgbImage {
        let mut seed: u32 = 0x1234_5678;
        ImageBuffer::from_fn(width, height, |_, _| {
            let mut channel = || {
                seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                ((seed >> 24) as u8) % amplitude.max(1)
            };
            Rgb([channel(), channel(), channel()])
        })
    }

    fn small_jpeg() -> Vec<u8> {
        encode(&gradient(120, 80), ImageFormat::Jpeg)
    }

    fn image_artwork(bytes: &[u8]) -> Artwork {
        Artwork::Image(prepare_image(bytes).expect("test image is accepted"))
    }

    fn shown<'a>(name: &'a str, artwork: Option<&'a Artwork>) -> ShownTrack<'a> {
        ShownTrack {
            artist: name,
            title: name,
            artwork,
        }
    }

    fn image_files(dir: &Path) -> Vec<String> {
        let mut names: Vec<String> = fs::read_dir(dir)
            .expect("list test dir")
            .filter_map(|entry| entry.ok()?.file_name().into_string().ok())
            .filter(|name| is_image_file_name(name))
            .collect();
        names.sort();
        names
    }

    fn display_json(dir: &Path) -> serde_json::Value {
        let content = fs::read_to_string(dir.join(DISPLAY_FILE_NAME)).expect("read display.json");
        serde_json::from_str(&content).expect("parse display.json")
    }

    fn modified(path: &Path) -> SystemTime {
        fs::metadata(path)
            .and_then(|metadata| metadata.modified())
            .expect("modified time")
    }

    #[test]
    fn file_name_is_the_sha256_of_the_bytes_that_go_out() {
        // The SHA-256 of "abc" (FIPS 180-4 example).
        let digest = "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad";
        assert_eq!(sha256_hex(b"abc"), digest);
        let image = ImageFile::new(ImageMime::Png, b"abc".to_vec());

        assert_eq!(image.sha256(), digest);
        assert_eq!(image.file_name(), format!("{digest}.png"));
    }

    #[test]
    fn image_url_accepts_only_http_and_https_up_to_the_limit() {
        assert_eq!(
            image_url("https://example.test/a.png"),
            Some("https://example.test/a.png")
        );
        assert_eq!(
            image_url("HTTP://example.test/a.gif"),
            Some("HTTP://example.test/a.gif")
        );
        for value in [
            "ftp://example.test/a.png",
            "file:///home/a.png",
            "data:image/png;base64,AAAA",
            "javascript:alert(1)",
            "example.test/a.png",
            "https://",
            "",
            " https://example.test/a.png",
        ] {
            assert_eq!(image_url(value), None, "{value:?} must not be used");
        }
        let prefix = "https://example.test/";
        let longest = format!("{prefix}{}", "a".repeat(MAX_URL_CHARS - prefix.len()));
        assert_eq!(image_url(&longest), Some(longest.as_str()));
        let too_long = format!("{longest}a");
        assert_eq!(image_url(&too_long), None);
    }

    #[test]
    fn v4v_track_with_url_gives_the_url() {
        let jpeg = small_jpeg();

        let artwork = v4v_artwork(Some("https://example.test/cover.gif"), Some(&jpeg));

        assert_eq!(
            artwork,
            Some(Artwork::Url("https://example.test/cover.gif".to_owned()))
        );
    }

    #[test]
    fn v4v_track_with_no_url_uses_its_embedded_image() {
        let jpeg = small_jpeg();

        assert_eq!(v4v_artwork(None, Some(&jpeg)), Some(image_artwork(&jpeg)));
    }

    #[test]
    fn v4v_track_with_a_url_of_another_scheme_uses_its_embedded_image() {
        let jpeg = small_jpeg();

        let artwork = v4v_artwork(Some("ftp://example.test/cover.jpg"), Some(&jpeg));

        assert_eq!(artwork, Some(image_artwork(&jpeg)));
        assert_eq!(
            v4v_artwork(Some("ftp://example.test/cover.jpg"), None),
            None
        );
    }

    #[test]
    fn v4v_track_with_an_empty_image_tag_uses_its_embedded_image() {
        let jpeg = small_jpeg();

        assert_eq!(
            v4v_artwork(Some(""), Some(&jpeg)),
            Some(image_artwork(&jpeg))
        );
        assert_eq!(v4v_artwork(Some(""), None), None);
    }

    #[test]
    fn track_with_no_picture_gives_null_artwork() {
        assert_eq!(embedded_artwork(None), None);
        assert_eq!(v4v_artwork(None, None), None);
    }

    #[test]
    fn gif_picture_gives_null_artwork() {
        let gif = b"GIF89a\x01\x00\x01\x00\x80\x00\x00\x00\x00\x00\xff\xff\xff!\xf9\x04\x01\x00\x00\x00\x00,\x00\x00\x00\x00\x01\x00\x01\x00\x00\x02\x02D\x01\x00;";

        assert_eq!(prepare_image(gif), Err(Rejection::Format));
        assert_eq!(embedded_artwork(Some(gif)), None);
    }

    #[test]
    fn type_comes_from_the_first_bytes() {
        let png = encode(&gradient(10, 10), ImageFormat::Png);
        let jpeg = small_jpeg();

        assert_eq!(sniff_image(&png), Some(ImageMime::Png));
        assert_eq!(sniff_image(&jpeg), Some(ImageMime::Jpeg));
        assert_eq!(sniff_image(b"RIFF....WEBP"), None);
    }

    #[test]
    fn header_that_cannot_be_read_gives_null_artwork() {
        let jpeg = [0xFF, 0xD8, 0xFF, 0x00, 0x01, 0x02, 0x03];
        let png = [0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A, 0, 0, 0];

        assert_eq!(prepare_image(&jpeg), Err(Rejection::Header));
        assert_eq!(prepare_image(&png), Err(Rejection::Header));
        assert_eq!(embedded_artwork(Some(&jpeg)), None);
    }

    /// Changes the size in the SOF0 header of a baseline JPEG. The pixel data
    /// stays the data of the small image, so a decode cannot give the large
    /// image.
    fn jpeg_with_header_size(width: u16, height: u16) -> Vec<u8> {
        let mut jpeg = small_jpeg();
        let sof = jpeg
            .windows(2)
            .position(|marker| marker == [0xFF, 0xC0])
            .expect("baseline JPEG has SOF0");
        jpeg[sof + 5..sof + 7].copy_from_slice(&height.to_be_bytes());
        jpeg[sof + 7..sof + 9].copy_from_slice(&width.to_be_bytes());
        jpeg
    }

    #[test]
    fn side_over_4000_gives_null_artwork_with_no_decode() {
        let header_only = jpeg_with_header_size(5_000, 300);
        let decodable = encode(&gradient(MAX_SOURCE_SIDE + 1, 1), ImageFormat::Png);
        let at_limit = encode(&gradient(MAX_SOURCE_SIDE, 1), ImageFormat::Png);

        // `SideTooLong` comes before a decode. A decode of the header-only
        // image gives `Decode` or an image, never `SideTooLong`.
        assert_eq!(prepare_image(&header_only), Err(Rejection::SideTooLong));
        assert_eq!(prepare_image(&decodable), Err(Rejection::SideTooLong));
        assert_eq!(embedded_artwork(Some(&header_only)), None);
        assert!(prepare_image(&at_limit).is_ok());
    }

    #[test]
    fn data_over_64_mib_is_rejected() {
        let mut data = small_jpeg();
        data.resize(MAX_PICTURE_BYTES + 1, 0);

        assert_eq!(prepare_image(&data), Err(Rejection::TooLarge));
    }

    #[test]
    fn small_jpeg_goes_out_with_the_same_bytes() {
        let jpeg = small_jpeg();

        let image = prepare_image(&jpeg).expect("small JPEG is accepted");

        assert_eq!(image.bytes(), jpeg.as_slice());
        assert_eq!(image.mime(), ImageMime::Jpeg);
        assert_eq!(image.sha256(), sha256_hex(&jpeg));
        assert_eq!(image.file_name(), format!("{}.jpg", sha256_hex(&jpeg)));
    }

    #[test]
    fn small_png_goes_out_with_the_same_bytes() {
        let png = encode(&gradient(MAX_OUTPUT_SIDE, 20), ImageFormat::Png);

        let image = prepare_image(&png).expect("small PNG is accepted");

        assert_eq!(image.bytes(), png.as_slice());
        assert_eq!(image.mime(), ImageMime::Png);
        assert_eq!(image.file_name(), format!("{}.png", sha256_hex(&png)));
    }

    fn assert_reduced(image: &ImageFile, expected: (u32, u32)) {
        assert_eq!(image.mime(), ImageMime::Jpeg);
        assert!(image.bytes().len() <= MAX_OUTPUT_BYTES);
        let size = ImageReader::with_format(Cursor::new(image.bytes()), ImageFormat::Jpeg)
            .into_dimensions()
            .expect("reduced image has a header");
        assert_eq!(size, expected);
        assert_eq!(image.sha256(), sha256_hex(image.bytes()));
    }

    #[test]
    fn large_image_comes_out_as_jpeg_of_at_most_1000_pixels() {
        let png = encode(&gradient(2_000, 1_500), ImageFormat::Png);
        let jpeg = encode(&gradient(1_500, 3_000), ImageFormat::Jpeg);

        assert_reduced(&prepare_image(&png).expect("reduced"), (1_000, 750));
        assert_reduced(&prepare_image(&jpeg).expect("reduced"), (500, 1_000));
    }

    #[test]
    fn image_over_512_kib_is_reduced_even_at_1000_pixels() {
        let png = encode(&noise(700, 700, 16), ImageFormat::Png);
        assert!(png.len() > MAX_OUTPUT_BYTES, "test data is {}", png.len());

        assert_reduced(&prepare_image(&png).expect("reduced"), (700, 700));
    }

    #[test]
    fn reduced_image_over_512_kib_gives_null_artwork() {
        let png = encode(&noise(1_000, 1_000, 255), ImageFormat::Png);

        assert_eq!(prepare_image(&png), Err(Rejection::ReducedTooLarge));
        assert_eq!(embedded_artwork(Some(&png)), None);
    }

    #[test]
    fn display_json_has_the_schema_and_the_three_artwork_forms() -> Result<()> {
        let jpeg = small_jpeg();
        let image = image_artwork(&jpeg);
        let url = Artwork::Url("https://example.test/a.png".to_owned());

        let null: serde_json::Value = serde_json::from_str(&render_display_json(None)?)?;
        assert_eq!(
            null,
            serde_json::json!({"schema": "musicindex.display/1", "track": null})
        );
        let embedded: serde_json::Value =
            serde_json::from_str(&render_display_json(Some(ShownTrack {
                artist: "A - B",
                title: "T",
                artwork: Some(&image),
            }))?)?;
        assert_eq!(
            embedded,
            serde_json::json!({
                "schema": "musicindex.display/1",
                "track": {
                    "artist": "A - B",
                    "title": "T",
                    "artwork": {"sha256": sha256_hex(&jpeg), "mime": "image/jpeg"}
                }
            })
        );
        let linked: serde_json::Value =
            serde_json::from_str(&render_display_json(Some(shown("A", Some(&url))))?)?;
        assert_eq!(
            linked["track"]["artwork"],
            serde_json::json!({"url": "https://example.test/a.png"})
        );
        let none: serde_json::Value =
            serde_json::from_str(&render_display_json(Some(shown("A", None)))?)?;
        assert_eq!(none["track"]["artwork"], serde_json::Value::Null);
        Ok(())
    }

    #[test]
    fn url_track_writes_no_image_file() -> Result<()> {
        let temp = TempDir::new()?;
        let mut output = DisplayOutput::new(temp.path());
        let url = Artwork::Url("https://example.test/a.png".to_owned());

        output.write(Some(shown("A", Some(&url))))?;

        assert_eq!(
            display_json(temp.path())["track"]["artwork"]["url"],
            "https://example.test/a.png"
        );
        let names: Vec<_> = fs::read_dir(temp.path())?
            .filter_map(|entry| entry.ok()?.file_name().into_string().ok())
            .collect();
        assert_eq!(names, vec![DISPLAY_FILE_NAME.to_owned()]);
        Ok(())
    }

    #[test]
    fn display_json_is_written_after_its_image() -> Result<()> {
        let temp = TempDir::new()?;
        let mut output = DisplayOutput::new(temp.path());
        output.write(Some(shown("A", None)))?;
        let before = fs::read_to_string(temp.path().join(DISPLAY_FILE_NAME))?;
        let jpeg = small_jpeg();
        let artwork = image_artwork(&jpeg);
        let Artwork::Image(image) = &artwork else {
            unreachable!("image_artwork gives an image");
        };
        // A directory at the image path makes the image rename fail.
        let blocker = temp.path().join(image.file_name());
        fs::create_dir(&blocker)?;

        assert!(output.write(Some(shown("B", Some(&artwork)))).is_err());
        assert_eq!(
            fs::read_to_string(temp.path().join(DISPLAY_FILE_NAME))?,
            before
        );

        fs::remove_dir(&blocker)?;
        output.write(Some(shown("B", Some(&artwork))))?;
        assert_eq!(fs::read(&blocker)?, jpeg);
        assert_eq!(
            display_json(temp.path())["track"]["artwork"]["sha256"],
            image.sha256()
        );
        Ok(())
    }

    #[test]
    fn third_image_deletes_the_first() -> Result<()> {
        let temp = TempDir::new()?;
        let other_files = [
            "notes.txt",
            "cover.jpg",
            "ABCDEF0123456789ABCDEF0123456789ABCDEF0123456789ABCDEF0123456789.jpg",
            ".display.json.1.tmp",
        ];
        for name in other_files {
            fs::write(temp.path().join(name), "keep")?;
        }
        let mut output = DisplayOutput::new(temp.path());
        let images: Vec<Artwork> = [10, 20, 30]
            .into_iter()
            .map(|side| image_artwork(&encode(&gradient(side, side), ImageFormat::Png)))
            .collect();
        let names: Vec<String> = images
            .iter()
            .map(|artwork| match artwork {
                Artwork::Image(image) => image.file_name(),
                Artwork::Url(_) => unreachable!("test images are embedded"),
            })
            .collect();

        output.write(Some(shown("A", Some(&images[0]))))?;
        output.write(Some(shown("B", Some(&images[1]))))?;
        let mut first_two = vec![names[0].clone(), names[1].clone()];
        first_two.sort();
        assert_eq!(image_files(temp.path()), first_two);

        output.write(Some(shown("C", Some(&images[2]))))?;
        let mut last_two = vec![names[1].clone(), names[2].clone()];
        last_two.sort();
        assert_eq!(image_files(temp.path()), last_two);
        for name in other_files {
            assert!(temp.path().join(name).exists(), "{name} must stay");
        }

        // The `null` state keeps the image of the state before it.
        output.write(None)?;
        assert_eq!(image_files(temp.path()), vec![names[2].clone()]);
        Ok(())
    }

    #[test]
    fn same_content_is_not_rewritten() -> Result<()> {
        let temp = TempDir::new()?;
        let mut output = DisplayOutput::new(temp.path());
        let artwork = image_artwork(&small_jpeg());
        output.write(Some(shown("A", Some(&artwork))))?;
        let path = temp.path().join(DISPLAY_FILE_NAME);
        let first = modified(&path);

        output.write(Some(shown("A", Some(&artwork))))?;

        assert_eq!(modified(&path), first);
        Ok(())
    }

    #[test]
    fn startup_writes_null_and_keeps_the_previous_image() -> Result<()> {
        let temp = TempDir::new()?;
        let dir = temp.path().join("display");
        let mut first_run = DisplayOutput::new(&dir);
        let old = image_artwork(&encode(&gradient(10, 10), ImageFormat::Png));
        let last = image_artwork(&small_jpeg());
        first_run.start()?;
        first_run.write(Some(shown("A", Some(&old))))?;
        first_run.write(Some(shown("B", Some(&last))))?;
        assert_eq!(image_files(&dir).len(), 2);

        let mut second_run = DisplayOutput::new(&dir);
        second_run.start()?;

        assert_eq!(display_json(&dir)["track"], serde_json::Value::Null);
        let Artwork::Image(last) = last else {
            unreachable!("test image is embedded");
        };
        assert_eq!(image_files(&dir), vec![last.file_name()]);

        // A second start with `null` already present does not rewrite it.
        let first = modified(&dir.join(DISPLAY_FILE_NAME));
        DisplayOutput::new(&dir).start()?;
        assert_eq!(modified(&dir.join(DISPLAY_FILE_NAME)), first);
        Ok(())
    }

    #[test]
    fn exit_writes_null() -> Result<()> {
        let temp = TempDir::new()?;
        let mut output = DisplayOutput::new(temp.path());
        let artwork = image_artwork(&small_jpeg());
        output.write(Some(shown("A", Some(&artwork))))?;

        write_null_at_exit(temp.path())?;

        assert_eq!(
            display_json(temp.path()),
            serde_json::json!({"schema": "musicindex.display/1", "track": null})
        );
        Ok(())
    }

    #[test]
    fn same_directory_compares_resolved_paths() -> Result<()> {
        let temp = TempDir::new()?;
        let missing = temp.path().join("missing");

        assert!(same_directory(temp.path(), &temp.path().join(".")));
        assert!(same_directory(&missing, &missing.join("x").join("..")));
        assert!(!same_directory(temp.path(), &missing));
        Ok(())
    }
}
