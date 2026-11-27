//! Parse chapter entries and select the segment at one position.

use now_playing_drop::{
    PaymentRoute, display::image_url, merge::has_percentage as routes_have_percentage,
};
use serde_json::Value;

use crate::state::StateFile;

/// Largest time value accepted by the segment rule (ADR 0045 §The Segment Rule).
const MAX_TIME_MS: i64 = 4_611_686_018_427_387_903;

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

/// Holds timed entries, including entries with invalid chapter content.
#[derive(Debug, Clone, Default)]
pub struct ChapterList {
    spans: Vec<Span>,
}

#[derive(Debug, Clone)]
struct Span {
    start_ms: i64,
    end_ms: i64,
    chapter: Option<Chapter>,
}

impl ChapterList {
    /// Returns true when no timed entry has valid chapter content.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        !self.spans.iter().any(|span| span.chapter.is_some())
    }

    /// Returns true when at least one timed entry defines a segment.
    #[must_use]
    pub fn has_segments(&self) -> bool {
        !self.spans.is_empty()
    }

    /// Returns the entries with valid content in segment order.
    pub fn chapters(&self) -> impl Iterator<Item = &Chapter> {
        self.spans.iter().filter_map(|span| span.chapter.as_ref())
    }
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
pub fn parse_chapters(state: &StateFile, bad_chapters_seq: &mut Option<i64>) -> ChapterList {
    // ADR 0041 §The State File Contract: an empty `chapters` value means that the file
    // has no chapter list. Liquidsoap gives an empty value for a missing tag.
    let Some(raw) = state
        .chapters
        .as_deref()
        .filter(|raw| !raw.trim().is_empty())
    else {
        return ChapterList { spans: Vec::new() };
    };
    let parsed = serde_json::from_str::<Value>(raw);
    let Ok(Value::Array(entries)) = parsed else {
        log_bad_chapters(state.seq, bad_chapters_seq, "chapters is not a JSON array");
        return ChapterList { spans: Vec::new() };
    };

    let mut spans = Vec::new();
    for entry in entries {
        let Some((start_ms, end_ms)) = parse_times(&entry) else {
            log_bad_chapters(state.seq, bad_chapters_seq, "chapter entry is not valid");
            continue;
        };
        let chapter = parse_content(entry, start_ms, end_ms);
        if chapter.is_none() {
            log_bad_chapters(state.seq, bad_chapters_seq, "chapter entry is not valid");
        }
        spans.push(Span {
            start_ms,
            end_ms,
            chapter,
        });
    }
    spans.sort_by_key(|span| span.start_ms);
    spans.dedup_by_key(|span| span.start_ms);
    ChapterList { spans }
}

/// Selects the chapter or gap segment at a non-negative position.
#[must_use]
pub fn segment_at(chapters: &ChapterList, position_ms: i64) -> Option<Segment<'_>> {
    if !chapters.has_segments() || position_ms < 0 {
        return None;
    }
    for (index, span) in chapters.spans.iter().enumerate() {
        let end_ms = chapters
            .spans
            .get(index + 1)
            .map_or(span.end_ms, |next| span.end_ms.min(next.start_ms));
        if position_ms >= span.start_ms && position_ms < end_ms {
            return Some(Segment {
                chapter: span.chapter.as_ref(),
                start_ms: span.start_ms,
            });
        }
        if position_ms < span.start_ms {
            let start_ms = index.checked_sub(1).map_or(0, |previous| {
                chapters.spans[previous].end_ms.min(span.start_ms)
            });
            return Some(Segment {
                chapter: None,
                start_ms,
            });
        }
    }
    chapters.spans.last().map(|span| Segment {
        chapter: None,
        start_ms: span.end_ms,
    })
}

fn parse_times(value: &Value) -> Option<(i64, i64)> {
    let object = value.as_object()?;
    let start_ms = valid_time(object.get("start_ms")?)?;
    let end_ms = valid_time(object.get("end_ms")?)?;
    if end_ms <= start_ms {
        return None;
    }
    Some((start_ms, end_ms))
}

fn valid_time(value: &Value) -> Option<i64> {
    let number = value.as_i64()?;
    (0..=MAX_TIME_MS).contains(&number).then_some(number)
}

fn parse_content(value: Value, start_ms: i64, end_ms: i64) -> Option<Chapter> {
    let object = value.as_object()?;
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
            pub_date: None,
            wpay: None,
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
        assert_eq!(chapters.chapters().count(), 1);
        assert_eq!(
            chapters
                .chapters()
                .next()
                .and_then(|chapter| chapter.title.as_deref()),
            Some("first")
        );
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

    #[test]
    fn shared_segment_cases_match_adr_0045() -> Result<(), Box<dyn std::error::Error>> {
        #[derive(serde::Deserialize)]
        struct Case {
            name: String,
            chapters: String,
            boundaries: Vec<i64>,
            suffix: bool,
        }

        let cases: Vec<Case> =
            serde_json::from_str(include_str!("../tests/segment-boundaries.json"))?;
        for case in cases {
            let mut candidates = vec![0];
            if let Ok(Value::Array(entries)) = serde_json::from_str::<Value>(&case.chapters) {
                for entry in entries {
                    if let Some(object) = entry.as_object() {
                        for key in ["start_ms", "end_ms"] {
                            if let Some(value) = object.get(key).and_then(Value::as_i64)
                                && (0..=MAX_TIME_MS).contains(&value)
                            {
                                candidates.push(value);
                            }
                        }
                    }
                }
            }
            let mut bad = None;
            let chapters = parse_chapters(&state(&case.chapters), &mut bad);
            let mut actual = vec![0];
            actual.extend(
                candidates
                    .into_iter()
                    .filter_map(|position| segment_at(&chapters, position))
                    .map(|segment| segment.start_ms),
            );
            actual.sort_unstable();
            actual.dedup();
            let mut expected = case.boundaries;
            expected.sort_unstable();
            assert_eq!(
                actual, expected,
                "{}: ADR 0045 §The Segment Rule",
                case.name
            );
            assert_eq!(
                chapters.has_segments(),
                case.suffix,
                "{}: ADR 0045 §The Segment Rule",
                case.name
            );
        }
        Ok(())
    }
}
