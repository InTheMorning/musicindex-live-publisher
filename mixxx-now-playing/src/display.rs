//! The picture rules of the Mixxx producer (ADR 0008 §The Embedded Image).
//!
//! The shared picture rules live in `now-playing-drop`.

use std::path::Path;

pub use now_playing_drop::display::{
    Artwork, DISPLAY_FILE_NAME, DISPLAY_SCHEMA, DisplayOutput, ImageFile, ImageMime, MAX_URL_CHARS,
    ShownTrack, image_url, render_display_json, sha256_hex, write_null_at_exit,
};
pub use now_playing_drop::picture::{
    MAX_OUTPUT_BYTES, MAX_OUTPUT_SIDE, MAX_PICTURE_BYTES, MAX_SOURCE_SIDE, Rejection, artwork_from,
    embedded_artwork, prepare_image, sniff_image,
};

use crate::tags::read_tags_with_picture;

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn play_id_goes_out_as_a_string() {
        let json = render_display_json(Some(ShownTrack {
            artist: "A",
            title: "T",
            artwork: None,
            song_line: "A - T",
            play_id: Some("42"),
            album: None,
        }))
        .expect("render display.json");
        let value: serde_json::Value = serde_json::from_str(&json).expect("parse display.json");

        assert_eq!(value["track"]["play_id"], "42");
    }
}
