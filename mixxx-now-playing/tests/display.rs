use std::fs;
use std::io::Cursor;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use image::{DynamicImage, ImageBuffer, ImageFormat, Rgb, RgbImage};
use lofty::config::WriteOptions;
use lofty::picture::{MimeType, Picture, PictureType};
use lofty::prelude::*;
use lofty::tag::Tag;
use lofty::tag::{ItemKey, ItemValue, TagItem};
use mixxx_now_playing::display::{Artwork, ImageMime, file_artwork, sha256_hex, v4v_artwork};
use mixxx_now_playing::tags::{read_tags, read_tags_with_picture};
use tempfile::TempDir;

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name)
}

fn encode(side: u32, format: ImageFormat) -> Vec<u8> {
    let image: RgbImage =
        ImageBuffer::from_fn(side, side, |x, y| Rgb([x as u8, y as u8, (x ^ y) as u8]));
    let mut bytes = Cursor::new(Vec::new());
    DynamicImage::ImageRgb8(image)
        .write_to(&mut bytes, format)
        .expect("encode test image");
    bytes.into_inner()
}

/// Copies `untagged.flac` and adds the pictures in their order.
fn flac_with_pictures(temp: &TempDir, pictures: Vec<(PictureType, Vec<u8>)>) -> Result<PathBuf> {
    let path = temp.path().join("track.flac");
    fs::copy(fixture("untagged.flac"), &path)?;
    let mut tagged = lofty::read_from_path(&path)?;
    let tag_type = tagged.primary_tag_type();
    if tagged.primary_tag().is_none() {
        tagged.insert_tag(Tag::new(tag_type));
    }
    let tag = tagged.primary_tag_mut().context("primary tag")?;
    for (pic_type, data) in pictures {
        // The tag type text says JPEG for each picture. The producer must
        // use the first bytes.
        tag.push_picture(Picture::new_unchecked(
            pic_type,
            Some(MimeType::Jpeg),
            None,
            data,
        ));
    }
    tagged.save_to_path(&path, WriteOptions::default())?;
    Ok(path)
}

/// Gives the picture of the one tag read of `path`.
fn read_picture(path: &Path) -> Result<Option<Vec<u8>>> {
    Ok(read_tags_with_picture(path)?.1)
}

/// Copies `musicindex-tagged.mp3` and writes `value` into its
/// `TXXX:MusicIndex Image` frame. `Tag::insert` drops a new `TXXX` frame that
/// has no key for the tag type, so the helper uses `insert_unchecked`.
fn mp3_with_image_tag(temp: &TempDir, value: &str) -> Result<PathBuf> {
    let path = temp.path().join("tagged-with-image.mp3");
    fs::copy(fixture("musicindex-tagged.mp3"), &path)?;
    let mut tagged = lofty::read_from_path(&path)?;
    let tag = tagged.primary_tag_mut().context("primary tag")?;
    tag.insert_unchecked(TagItem::new(
        ItemKey::Unknown("MusicIndex Image".to_owned()),
        ItemValue::Text(value.to_owned()),
    ));
    tagged.save_to_path(&path, WriteOptions::default())?;
    Ok(path)
}

/// Gives the artwork of a V4V file through the tag read of the producer.
fn v4v_file_artwork(path: &Path) -> Result<Option<Artwork>> {
    let (tags, picture) = read_tags_with_picture(path)?;
    Ok(v4v_artwork(
        tags.musicindex_value("Image"),
        picture.as_deref(),
    ))
}

fn image_of(artwork: Option<Artwork>) -> (String, ImageMime) {
    match artwork {
        Some(Artwork::Image(image)) => (image.sha256().to_owned(), image.mime()),
        other => panic!("expected an embedded image, got {other:?}"),
    }
}

#[test]
fn front_cover_wins_over_an_earlier_picture() -> Result<()> {
    let temp = TempDir::new()?;
    let other = encode(16, ImageFormat::Png);
    let front = encode(24, ImageFormat::Jpeg);
    let path = flac_with_pictures(
        &temp,
        vec![
            (PictureType::Other, other),
            (PictureType::CoverFront, front.clone()),
        ],
    )?;

    assert_eq!(read_picture(&path)?, Some(front.clone()));
    assert_eq!(
        image_of(file_artwork(&path)),
        (sha256_hex(&front), ImageMime::Jpeg)
    );
    Ok(())
}

#[test]
fn first_picture_is_used_with_no_front_cover() -> Result<()> {
    let temp = TempDir::new()?;
    let first = encode(16, ImageFormat::Png);
    let second = encode(24, ImageFormat::Jpeg);
    let path = flac_with_pictures(
        &temp,
        vec![
            (PictureType::Artist, first.clone()),
            (PictureType::Other, second),
        ],
    )?;

    assert_eq!(read_picture(&path)?, Some(first.clone()));
    // The tag says JPEG. The first bytes say PNG, and PNG wins.
    assert_eq!(
        image_of(file_artwork(&path)),
        (sha256_hex(&first), ImageMime::Png)
    );
    Ok(())
}

#[test]
fn file_with_no_picture_gives_null_artwork() -> Result<()> {
    assert_eq!(read_picture(&fixture("untagged.flac"))?, None);
    assert_eq!(file_artwork(&fixture("untagged.flac")), None);
    Ok(())
}

#[test]
fn file_that_cannot_be_read_gives_null_artwork() -> Result<()> {
    let temp = TempDir::new()?;
    let missing = temp.path().join("unmounted").join("track.flac");
    let not_audio = temp.path().join("track.mp3");
    fs::write(&not_audio, "not audio")?;

    assert!(read_picture(&missing).is_err());
    assert_eq!(file_artwork(&missing), None);
    assert_eq!(file_artwork(&not_audio), None);
    Ok(())
}

#[test]
fn tag_read_with_picture_gives_the_same_tags() -> Result<()> {
    for name in ["musicindex-tagged.mp3", "musicindex-tagged.flac"] {
        let path = fixture(name);
        let (tags, _picture) = read_tags_with_picture(&path)?;

        assert_eq!(tags, read_tags(&path)?, "{name}");
    }
    let (_, picture) = read_tags_with_picture(&fixture("musicindex-tagged.mp3"))?;
    let picture = picture.context("the MP3 fixture has a picture")?;
    assert!(picture.starts_with(&[0xFF, 0xD8, 0xFF]));
    Ok(())
}

#[test]
fn v4v_file_with_an_image_tag_gives_the_url() -> Result<()> {
    let temp = TempDir::new()?;
    let path = mp3_with_image_tag(&temp, "https://example.test/cover.gif")?;

    assert_eq!(
        v4v_file_artwork(&path)?,
        Some(Artwork::Url("https://example.test/cover.gif".to_owned()))
    );
    Ok(())
}

#[test]
fn v4v_file_with_an_empty_image_tag_uses_its_embedded_image() -> Result<()> {
    let temp = TempDir::new()?;
    let path = mp3_with_image_tag(&temp, "")?;
    let (tags, picture) = read_tags_with_picture(&path)?;
    let picture = picture.context("the MP3 fixture has a picture")?;

    assert_eq!(tags.musicindex_value("Image"), Some(""));
    assert_eq!(
        image_of(v4v_file_artwork(&path)?),
        (sha256_hex(&picture), ImageMime::Jpeg)
    );
    Ok(())
}

#[test]
fn file_that_is_not_v4v_ignores_its_image_tag() -> Result<()> {
    let temp = TempDir::new()?;
    let path = mp3_with_image_tag(&temp, "https://example.test/cover.gif")?;
    let picture = read_picture(&path)?.context("the MP3 fixture has a picture")?;

    assert_eq!(
        image_of(file_artwork(&path)),
        (sha256_hex(&picture), ImageMime::Jpeg)
    );
    Ok(())
}
