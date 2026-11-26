//! The display writer of ADR 0008 §The Producer Output.
//!
//! With a display directory `DIR`, a producer writes `DIR/display.json` and
//! the image of the present track. This module holds the output schema, the
//! artwork types, the writes and the image retention. Each producer uses
//! this one writer (ADR 0035 §One Display Writer And One Picture Component). The code that reads a
//! picture and makes an `ImageFile` from it stays in the producer.

use std::fs;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde::Serialize;
use sha2::{Digest, Sha256};

use crate::sink::write_atomic;

/// The schema of `display.json`.
pub const DISPLAY_SCHEMA: &str = "musicindex.display/3";
/// The file name of the display state in `DIR`.
pub const DISPLAY_FILE_NAME: &str = "display.json";
/// The longest image URL that the artwork can use, in characters.
pub const MAX_URL_CHARS: usize = 2_048;

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
    /// Makes an image file. It computes the SHA-256 of `bytes`.
    pub fn new(mime: ImageMime, bytes: Vec<u8>) -> Self {
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

/// The track that `display.json` shows.
#[derive(Debug, Clone, Copy)]
pub struct ShownTrack<'a> {
    /// The artist of the history row, with no change.
    pub artist: &'a str,
    /// The title of the history row, with no change.
    pub title: &'a str,
    /// The artwork, or `None` for the `null` artwork.
    pub artwork: Option<&'a Artwork>,
    /// The first line of `now-playing.txt` for this track (ADR 0009 §The Song Line).
    pub song_line: &'a str,
    /// The history row ID of this play (ADR 0010, ADR 0039).
    pub play_id: Option<&'a str>,
    /// The album of the history row (ADR 0013).
    pub album: Option<&'a str>,
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
    song_line: &'a str,
    play_id: Option<String>,
    album: Option<&'a str>,
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
            song_line: track.song_line,
            play_id: track.play_id.map(str::to_owned),
            album: track.album,
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

/// Gives the SHA-256 of `data` as 64 lowercase hex characters.
pub fn sha256_hex(data: &[u8]) -> String {
    format!("{:x}", Sha256::digest(data))
}

#[cfg(test)]
mod tests {
    use std::time::SystemTime;

    use tempfile::TempDir;

    use super::*;

    /// Fixed bytes that start as a JPEG file. `tag` makes the bytes differ.
    fn jpeg_bytes(tag: u8) -> Vec<u8> {
        vec![
            0xFF, 0xD8, 0xFF, 0xE0, 0x00, 0x10, b'J', b'F', b'I', b'F', tag,
        ]
    }

    /// Fixed bytes that start as a PNG file. `tag` makes the bytes differ.
    fn png_bytes(tag: u8) -> Vec<u8> {
        vec![0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A, tag]
    }

    fn small_jpeg() -> Vec<u8> {
        jpeg_bytes(0)
    }

    fn image_artwork(bytes: &[u8]) -> Artwork {
        Artwork::Image(ImageFile::new(ImageMime::Jpeg, bytes.to_vec()))
    }

    fn shown<'a>(name: &'a str, artwork: Option<&'a Artwork>) -> ShownTrack<'a> {
        ShownTrack {
            artist: name,
            title: name,
            artwork,
            song_line: name,
            play_id: None,
            album: None,
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
    fn display_json_has_the_schema_and_the_three_artwork_forms() -> Result<()> {
        let jpeg = small_jpeg();
        let image = image_artwork(&jpeg);
        let url = Artwork::Url("https://example.test/a.png".to_owned());

        let null: serde_json::Value = serde_json::from_str(&render_display_json(None)?)?;
        assert_eq!(
            null,
            serde_json::json!({"schema": "musicindex.display/3", "track": null})
        );
        let embedded: serde_json::Value =
            serde_json::from_str(&render_display_json(Some(ShownTrack {
                artist: "A - B",
                title: "T",
                artwork: Some(&image),
                song_line: "A - B - T",
                play_id: Some("42"),
                album: None,
            }))?)?;
        assert_eq!(
            embedded,
            serde_json::json!({
                "schema": "musicindex.display/3",
                "track": {
                    "artist": "A - B",
                    "title": "T",
                    "artwork": {"sha256": sha256_hex(&jpeg), "mime": "image/jpeg"},
                    "song_line": "A - B - T",
                    "play_id": "42",
                    "album": null
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
            .map(|tag| Artwork::Image(ImageFile::new(ImageMime::Png, png_bytes(tag))))
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
        let old = Artwork::Image(ImageFile::new(ImageMime::Png, png_bytes(10)));
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
            serde_json::json!({"schema": "musicindex.display/3", "track": null})
        );
        Ok(())
    }
}
