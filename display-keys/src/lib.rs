//! Shared validation rules for optional display keys.

use serde::{Deserialize, Serialize};

pub const MAX_FUNDING_LINKS: usize = 3;
pub const MAX_FUNDING_URL_CHARS: usize = 512;
pub const MAX_FUNDING_LABEL_CHARS: usize = 128;

/// Returns the valid Gregorian day at the start of a date-time value.
pub fn pub_day(value: &str) -> Option<&str> {
    let day = value.get(..10)?;
    let bytes = day.as_bytes();
    if bytes[4] != b'-' || bytes[7] != b'-' {
        return None;
    }
    if !bytes.iter().enumerate().all(|(index, byte)| {
        matches!(index, 4 | 7) && *byte == b'-' || !matches!(index, 4 | 7) && byte.is_ascii_digit()
    }) {
        return None;
    }
    let year = day[0..4].parse::<u32>().ok()?;
    let month = day[5..7].parse::<u32>().ok()?;
    let date = day[8..10].parse::<u32>().ok()?;
    let leap = year % 4 == 0 && (year % 100 != 0 || year % 400 == 0);
    let month_days = match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if leap => 29,
        2 => 28,
        _ => return None,
    };
    (date > 0 && date <= month_days).then_some(day)
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FundingLink {
    pub url: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
}

/// Returns one valid funding link from a file's `WPAY` value.
pub fn file_funding(raw: &str) -> Vec<FundingLink> {
    let value = raw.trim_end_matches('\0');
    funding_links([(value, None)])
}

/// Returns true when `url` follows the relay funding URL rules.
pub fn funding_url_ok(url: &str) -> bool {
    let count = url.chars().count();
    if count == 0 || count > MAX_FUNDING_URL_CHARS {
        return false;
    }
    if url.chars().any(|ch| ch.is_whitespace() || ch.is_control()) {
        return false;
    }
    let Some((scheme, remainder)) = url.split_once("://") else {
        return false;
    };
    if !scheme.eq_ignore_ascii_case("http") && !scheme.eq_ignore_ascii_case("https") {
        return false;
    }
    let host = remainder.split(['/', '?', '#']).next().unwrap_or_default();
    !host.is_empty()
}

/// Trims and limits a funding label.
pub fn funding_label(raw: &str) -> Option<String> {
    let trimmed = raw.trim();
    let limited: String = trimmed.chars().take(MAX_FUNDING_LABEL_CHARS).collect();
    let result = limited.trim_end();
    (!result.is_empty()).then(|| result.to_owned())
}

/// Filters, normalizes, and limits funding links while preserving their order.
pub fn funding_links<'a, I>(entries: I) -> Vec<FundingLink>
where
    I: IntoIterator<Item = (&'a str, Option<&'a str>)>,
{
    let mut links = Vec::new();
    for (url, label) in entries {
        if !funding_url_ok(url) || links.iter().any(|link: &FundingLink| link.url == url) {
            continue;
        }
        links.push(FundingLink {
            url: url.to_owned(),
            label: label.and_then(funding_label),
        });
        if links.len() == MAX_FUNDING_LINKS {
            break;
        }
    }
    links
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validates_gregorian_days_and_takes_the_date_prefix() {
        assert_eq!(pub_day("2026-11-12"), Some("2026-11-12"));
        assert_eq!(pub_day("2024-02-29"), Some("2024-02-29"));
        assert_eq!(pub_day("2026-11-12T08:00:00"), Some("2026-11-12"));
        for day in [
            "2026-02-29",
            "2026-02-30",
            "2026-13-01",
            "2026-00-10",
            "2026-11-31",
            "12 Nov 2026",
            "",
        ] {
            assert_eq!(pub_day(day), None, "{day}");
        }
    }

    #[test]
    fn validates_funding_urls() {
        assert!(funding_url_ok("HTTPS://example.com"));
        assert!(!funding_url_ok("https:///support"));
        assert!(!funding_url_ok("ftp://x"));
        assert!(!funding_url_ok("https://x y"));
        assert!(!funding_url_ok("https://x\ty"));
        assert!(funding_url_ok(&format!("https://{}", "a".repeat(504))));
        assert!(!funding_url_ok(&format!("https://{}", "a".repeat(505))));
    }

    #[test]
    fn trims_and_limits_labels() {
        assert_eq!(
            funding_label(&"a".repeat(200)).unwrap().chars().count(),
            128
        );
        assert_eq!(funding_label("label\u{a0}"), Some("label".to_owned()));
        assert_eq!(funding_label("label\u{3000}"), Some("label".to_owned()));
        assert_eq!(funding_label(" \u{3000}"), None);
    }

    #[test]
    fn keeps_unique_valid_links_in_order_and_omits_empty_labels() {
        let links = funding_links([
            ("https://one", Some(" Support ")),
            ("https://one", Some("duplicate")),
            ("https://two", None),
            ("https://three", None),
            ("https://four", None),
        ]);
        assert_eq!(links.len(), 3);
        assert_eq!(links[0].label.as_deref(), Some("Support"));
        assert_eq!(links[1].label, None);
    }

    #[test]
    fn reads_file_funding_values() {
        assert_eq!(
            file_funding("https://example.com/pay\0")[0].url,
            "https://example.com/pay"
        );
        assert!(file_funding("").is_empty());
        assert!(file_funding("\0").is_empty());
        assert!(file_funding("ftp://x").is_empty());
    }
}
