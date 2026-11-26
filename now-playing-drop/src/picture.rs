//! Shared embedded picture preparation rules (ADR 0008 and ADR 0041 §The Producer Reads The Picture File).

use std::io::Cursor;

use crate::display::{Artwork, ImageFile, ImageMime, image_url};
use image::codecs::jpeg::JpegEncoder;
use image::{ImageFormat, ImageReader, Limits};

/// The producer rejects picture data larger than this value, in bytes.
pub const MAX_PICTURE_BYTES: usize = 64 * 1024 * 1024;
/// The producer rejects an image with a longer side, in pixels. It reads the
/// side from the header before a decode.
pub const MAX_SOURCE_SIDE: u32 = 4_000;
/// The longest side of an image that goes out, in pixels.
pub const MAX_OUTPUT_SIDE: u32 = 1_000;
/// The largest image that goes out, in bytes.
pub const MAX_OUTPUT_BYTES: usize = 524_288;

/// The JPEG quality of a reduced image.
const JPEG_QUALITY: u8 = 85;
/// The decoder allocation limit. A 4,000 by 4,000 RGBA image fits in it.
const DECODE_ALLOC_LIMIT: u64 = 64 * 1024 * 1024;

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

fn image_format(mime: ImageMime) -> ImageFormat {
    match mime {
        ImageMime::Jpeg => ImageFormat::Jpeg,
        ImageMime::Png => ImageFormat::Png,
    }
}

fn reader(data: &[u8], mime: ImageMime) -> ImageReader<Cursor<&[u8]>> {
    ImageReader::with_format(Cursor::new(data), image_format(mime))
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

/// Gives artwork from a valid image URL, or from embedded picture bytes (ADR 0008).
pub fn artwork_from(image_tag: Option<&str>, picture: Option<&[u8]>) -> Option<Artwork> {
    match image_tag.and_then(image_url) {
        Some(url) => Some(Artwork::Url(url.to_owned())),
        None => embedded_artwork(picture),
    }
}

#[cfg(test)]
mod tests {
    use image::{DynamicImage, ImageBuffer, Rgb, RgbImage};

    use crate::display::sha256_hex;

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

    #[test]
    fn v4v_track_with_url_gives_the_url() {
        let jpeg = small_jpeg();

        let artwork = artwork_from(Some("https://example.test/cover.gif"), Some(&jpeg));

        assert_eq!(
            artwork,
            Some(Artwork::Url("https://example.test/cover.gif".to_owned()))
        );
    }

    #[test]
    fn v4v_track_with_no_url_uses_its_embedded_image() {
        let jpeg = small_jpeg();

        assert_eq!(artwork_from(None, Some(&jpeg)), Some(image_artwork(&jpeg)));
    }

    #[test]
    fn v4v_track_with_a_url_of_another_scheme_uses_its_embedded_image() {
        let jpeg = small_jpeg();

        let artwork = artwork_from(Some("ftp://example.test/cover.jpg"), Some(&jpeg));

        assert_eq!(artwork, Some(image_artwork(&jpeg)));
        assert_eq!(
            artwork_from(Some("ftp://example.test/cover.jpg"), None),
            None
        );
    }

    #[test]
    fn v4v_track_with_an_empty_image_tag_uses_its_embedded_image() {
        let jpeg = small_jpeg();

        assert_eq!(
            artwork_from(Some(""), Some(&jpeg)),
            Some(image_artwork(&jpeg))
        );
        assert_eq!(artwork_from(Some(""), None), None);
    }

    #[test]
    fn track_with_no_picture_gives_null_artwork() {
        assert_eq!(embedded_artwork(None), None);
        assert_eq!(artwork_from(None, None), None);
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
}
