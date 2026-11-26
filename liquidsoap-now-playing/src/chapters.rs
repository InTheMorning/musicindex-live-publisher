//! Parse chapter entries and select the segment at one position.

use std::collections::HashSet;

use now_playing_drop::{
    PaymentRoute, display::image_url, merge::has_percentage as routes_have_percentage,
};
use serde_json::Value;

use crate::state::StateFile;

/// One valid payment chapter from the state file.
#[derive(Debug, Clone)]
pub struct Chapter {
    /// Start time in milliseconds.
    pub start_ms: i64,
    /// End time in milliseconds.
    pub end_ms: i64,
    /// Optional chapter title.
    pub title: Option<String>,
    /// Optional chapter artist.
    pub artist: Option<String>,
    /// Optional feed GUID.
    pub feed_guid: Option<String>,
    /// Optional item GUID.
    pub item_guid: Option<String>,
    /// The part of the payment for the remote song, 0 to 100.
    pub remote_percentage: f64,
    /// The local recipients of a local split, or the song routes from the
    /// file for a remote split. Empty when the file has no routes.
    pub value_routes: Vec<PaymentRoute>,
    /// The image URL of the song of a remote split, when `image_url` of
    /// `now-playing-drop` accepts it (ADR 0041 §The Chapter List).
    pub image_url: Option<String>,
}

impl Chapter {
    /// An entry with no feed GUID and no item GUID is a local split.
    #[must_use]
    pub fn is_local(&self) -> bool {
        self.feed_guid.is_none() && self.item_guid.is_none()
    }
}

/// One segment start and its active chapter, if any.
#[derive(Debug, Clone, Copy)]
pub struct Segment<'a> {
    /// Chapter active in this segment.
    pub chapter: Option<&'a Chapter>,
    /// Start time of this segment in milliseconds.
    pub start_ms: i64,
}

/// Parses valid chapter entries and logs at most one error for each sequence.
pub fn parse_chapters(state: &StateFile, bad_chapters_seq: &mut Option<i64>) -> Vec<Chapter> {
    // ADR 0041 §The State File Contract: an empty `chapters` value means that the file
    // has no chapter list. Liquidsoap gives an empty value for a missing tag.
    let Some(raw) = state
        .chapters
        .as_deref()
        .filter(|raw| !raw.trim().is_empty())
    else {
        return Vec::new();
    };
    let parsed = serde_json::from_str::<Value>(raw);
    let Ok(Value::Array(entries)) = parsed else {
        log_bad_chapters(state.seq, bad_chapters_seq, "chapters is not a JSON array");
        return Vec::new();
    };

    let mut chapters = Vec::new();
    for entry in entries {
        match parse_entry(entry) {
            Some(chapter) => chapters.push(chapter),
            None => log_bad_chapters(state.seq, bad_chapters_seq, "chapter entry is not valid"),
        }
    }
    chapters.sort_by_key(|chapter| chapter.start_ms);
    let mut starts = HashSet::new();
    chapters.retain(|chapter| starts.insert(chapter.start_ms));
    chapters
}

/// Selects the chapter or gap segment at a non-negative position.
#[must_use]
pub fn segment_at(chapters: &[Chapter], position_ms: i64) -> Option<Segment<'_>> {
    if chapters.is_empty() || position_ms < 0 {
        return None;
    }
    for (index, chapter) in chapters.iter().enumerate() {
        let end_ms = chapters
            .get(index + 1)
            .map_or(chapter.end_ms, |next| chapter.end_ms.min(next.start_ms));
        if position_ms >= chapter.start_ms && position_ms < end_ms {
            return Some(Segment {
                chapter: Some(chapter),
                start_ms: chapter.start_ms,
            });
        }
        if position_ms < chapter.start_ms {
            let start_ms = index.checked_sub(1).map_or(0, |previous| {
                chapters[previous].end_ms.min(chapter.start_ms)
            });
            return Some(Segment {
                chapter: None,
                start_ms,
            });
        }
    }
    chapters.last().map(|chapter| Segment {
        chapter: None,
        start_ms: chapter.end_ms,
    })
}

fn parse_entry(value: Value) -> Option<Chapter> {
    let object = value.as_object()?;
    let start_ms = nonnegative_integer(object.get("start_ms")?)?;
    let end_ms = nonnegative_integer(object.get("end_ms")?)?;
    if end_ms <= start_ms {
        return None;
    }
    let remote_percentage = match object.get("remote_percentage") {
        None | Some(Value::Null) => 100.0,
        Some(Value::Number(number)) => number.as_f64()?.clamp(0.0, 100.0),
        Some(_) => return None,
    };
    let title = optional_string(object.get("title"))?;
    let artist = optional_string(object.get("artist"))?;
    let feed_guid = optional_guid(object.get("feed_guid"))?;
    let item_guid = optional_guid(object.get("item_guid"))?;
    let local_split = feed_guid.is_none() && item_guid.is_none();
    let value_routes = if local_split {
        let routes =
            serde_json::from_value::<Vec<PaymentRoute>>(object.get("value_routes")?.clone())
                .ok()?;
        // A local split with no percentage on its routes cannot pay.
        if !routes_have_percentage(&routes) {
            return None;
        }
        routes
    } else {
        // ADR 0041 §The Route Lookup: the routes of a remote entry are
        // the second source of the song routes (musicindex ADR 0089 R-TAG-6).
        match object.get("value_routes") {
            None | Some(Value::Null) => Vec::new(),
            Some(routes) => serde_json::from_value::<Vec<PaymentRoute>>(routes.clone()).ok()?,
        }
    };
    // ADR 0041 §The Chapter List: a bad `image_url` is the same as null,
    // and it does not make the entry not valid.
    let image_url = match object.get("image_url") {
        Some(Value::String(url)) if !local_split => image_url(url).map(str::to_owned),
        _ => None,
    };
    Some(Chapter {
        start_ms,
        end_ms,
        title,
        artist,
        feed_guid,
        item_guid,
        remote_percentage,
        value_routes,
        image_url,
    })
}

fn nonnegative_integer(value: &Value) -> Option<i64> {
    let number = value.as_i64()?;
    (number >= 0).then_some(number)
}

fn optional_string(value: Option<&Value>) -> Option<Option<String>> {
    match value {
        None | Some(Value::Null) => Some(None),
        Some(Value::String(text)) => Some(Some(text.clone())),
        Some(_) => None,
    }
}

fn optional_guid(value: Option<&Value>) -> Option<Option<String>> {
    optional_string(value).map(|value| value.filter(|guid| !guid.trim().is_empty()))
}

fn log_bad_chapters(seq: i64, last_seq: &mut Option<i64>, message: &str) {
    if *last_seq != Some(seq) {
        tracing::error!(
            seq,
            reason = message,
            "invalid chapter data in the state file"
        );
        *last_seq = Some(seq);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn state(chapters: &str) -> StateFile {
        StateFile {
            format: crate::state::STATE_FORMAT_2.to_owned(),
            seq: 1,
            written: 0.0,
            live: false,
            jingle: false,
            track_started: 1.0,
            title: "State title".to_owned(),
            artist: "State artist".to_owned(),
            song: String::new(),
            album: None,
            track_guid: None,
            feed_guid: None,
            image: None,
            picture: String::new(),
            value_routes: None,
            elapsed: Some(0.0),
            cue_in: Some(0.0),
            chapters: Some(chapters.to_owned()),
        }
    }

    #[test]
    fn parses_valid_chapters_and_uses_stable_duplicate_starts() {
        let text = r#"[
          {"start_ms":100,"end_ms":200,"title":"first","value_routes":[{"split":1}]},
          {"start_ms":100,"end_ms":300,"title":"second","value_routes":[{"split":1}]}
        ]"#;
        let mut bad = None;
        let chapters = parse_chapters(&state(text), &mut bad);
        assert_eq!(chapters.len(), 1);
        assert_eq!(chapters[0].title.as_deref(), Some("first"));
        assert_eq!(
            segment_at(&chapters, 100).map(|segment| segment.start_ms),
            Some(100)
        );
    }

    #[test]
    fn empty_chapters_give_no_chapters_and_no_error() {
        for text in ["", "  "] {
            let mut bad = None;
            assert!(parse_chapters(&state(text), &mut bad).is_empty());
            assert_eq!(bad, None, "the value {text:?} must not log an error");
        }
    }

    #[test]
    fn invalid_json_gives_no_chapters() {
        let mut bad = None;
        assert!(parse_chapters(&state("invalid"), &mut bad).is_empty());
        assert_eq!(bad, Some(1));
    }
}
