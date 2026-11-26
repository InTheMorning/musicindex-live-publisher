//! Fetch payment routes for a track identified by its feed and item GUIDs.

use std::time::Duration;

use anyhow::{Context, Result, anyhow};
use display_keys::{FundingLink, funding_links};
use reqwest::StatusCode;
use reqwest::blocking::Client;
use serde::Deserialize;
use serde_json::Value;

use crate::PaymentRoute;

/// The default MusicIndex route service endpoint.
pub const DEFAULT_ROUTE_SERVICE: &str = "https://api.musicindex.org";

/// The result of a track route lookup.
#[derive(Debug, Clone, PartialEq)]
pub enum RouteLookup {
    /// The track has payment routes.
    Routes(Vec<PaymentRoute>),
    /// The track has no payment routes.
    NoRoutes,
}

/// Routes and funding links returned for one track.
#[derive(Debug, Clone, PartialEq)]
pub struct TrackDetails {
    pub routes: RouteLookup,
    pub funding: Vec<FundingLink>,
}

/// Sends track route requests to the MusicIndex API.
#[derive(Debug, Clone)]
pub struct RouteServiceClient {
    base_url: reqwest::Url,
    client: Client,
}

impl RouteServiceClient {
    /// Creates a client for one route service endpoint.
    ///
    /// # Errors
    ///
    /// Returns an error when the URL or HTTP client is invalid.
    pub fn new(base_url: impl Into<String>, timeout: Duration) -> Result<Self> {
        let base_url = base_url.into().trim_end_matches('/').to_owned();
        let mut url = reqwest::Url::parse(&base_url)
            .with_context(|| format!("parse route service URL {base_url:?}"))?;
        if !matches!(url.scheme(), "http" | "https") || url.host_str().is_none() {
            return Err(anyhow!(
                "route service URL must use HTTP or HTTPS and include a host"
            ));
        }
        let path = url.path().trim_end_matches('/').to_owned();
        url.set_path(&path);
        let client = Client::builder()
            .timeout(timeout)
            .build()
            .context("build route service HTTP client")?;
        Ok(Self {
            base_url: url,
            client,
        })
    }

    /// Gets routes for one track identified by its feed and item GUIDs.
    ///
    /// # Errors
    ///
    /// Returns an error when either GUID is empty or the request fails.
    pub fn track_routes(&self, feed_guid: &str, item_guid: &str) -> Result<RouteLookup> {
        Ok(self.track_details(feed_guid, item_guid)?.routes)
    }

    /// Gets routes and funding links for one track.
    pub fn track_details(&self, feed_guid: &str, item_guid: &str) -> Result<TrackDetails> {
        let feed_guid = sanitize_api_path_segment(feed_guid)?;
        let item_guid = sanitize_api_path_segment(item_guid)?;
        let mut url = self.base_url.clone();
        {
            let mut segments = url
                .path_segments_mut()
                .map_err(|_| anyhow!("route service URL cannot accept path segments"))?;
            segments.pop_if_empty();
            segments.push("v1");
            segments.push("feeds");
            segments.push(&feed_guid);
            segments.push("tracks");
            segments.push(&item_guid);
        }
        url.set_query(Some("include=payment_routes,source_links"));
        let response = self
            .client
            .get(url)
            .send()
            .context("send route service request")?;
        let status = response.status();
        if status == StatusCode::NOT_FOUND {
            return Ok(TrackDetails {
                routes: RouteLookup::NoRoutes,
                funding: Vec::new(),
            });
        }
        if !status.is_success() {
            return Err(anyhow!("route service returned HTTP {status}"));
        }
        let detail = response
            .json::<DetailResponse>()
            .context("decode route service response")?;
        // The routes keep their typed parse, so a bad route list is an error
        // and gets a retry. The links are parsed alone (ADR 0043 §The Source).
        let routes = detail
            .data
            .payment_routes
            .filter(|routes| !routes.is_empty());
        let funding = parse_funding(detail.data.source_links.as_ref());
        Ok(TrackDetails {
            routes: match routes {
                Some(routes) => RouteLookup::Routes(routes),
                None => RouteLookup::NoRoutes,
            },
            funding,
        })
    }

    /// Gets funding links for one feed.
    pub fn feed_funding(&self, feed_guid: &str) -> Result<Vec<FundingLink>> {
        let feed_guid = sanitize_api_path_segment(feed_guid)?;
        let mut url = self.base_url.clone();
        {
            let mut segments = url
                .path_segments_mut()
                .map_err(|_| anyhow!("route service URL cannot accept path segments"))?;
            segments.pop_if_empty();
            segments.push("v1");
            segments.push("feeds");
            segments.push(&feed_guid);
        }
        url.set_query(Some("include=source_links"));
        let response = self
            .client
            .get(url)
            .send()
            .context("send route service feed request")?;
        if response.status() == StatusCode::NOT_FOUND {
            return Ok(Vec::new());
        }
        if !response.status().is_success() {
            return Err(anyhow!("route service returned HTTP {}", response.status()));
        }
        let detail = response
            .json::<Value>()
            .context("decode route service response")?;
        Ok(parse_funding(
            detail.get("data").and_then(|data| data.get("source_links")),
        ))
    }
}

#[derive(Debug, Deserialize)]
struct DetailResponse {
    data: EntityWithRoutes,
}

#[derive(Debug, Deserialize, Default)]
#[serde(default)]
struct EntityWithRoutes {
    payment_routes: Option<Vec<PaymentRoute>>,
    source_links: Option<Value>,
}

fn parse_funding(value: Option<&Value>) -> Vec<FundingLink> {
    let Some(entries) = value.and_then(Value::as_array) else {
        return Vec::new();
    };
    let mut entries: Vec<_> = entries
        .iter()
        .filter_map(|entry| {
            let object = entry.as_object()?;
            Some((
                object.get("position").and_then(Value::as_i64),
                object.get("link_type").and_then(Value::as_str) == Some("funding"),
                object.get("url").and_then(Value::as_str)?,
                object.get("label").and_then(Value::as_str),
            ))
        })
        .filter(|(_, funding, _, _)| *funding)
        .collect();
    entries.sort_by_key(|(position, _, _, _)| position.unwrap_or(i64::MAX));
    funding_links(entries.into_iter().map(|(_, _, url, label)| (url, label)))
}

fn sanitize_api_path_segment(value: &str) -> Result<String> {
    let sanitized = value
        .chars()
        .map(|ch| {
            if ch == '\0' || ch == '\\' || ch.is_control() {
                ' '
            } else {
                ch
            }
        })
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    if sanitized.is_empty() {
        return Err(anyhow!("route service GUID is empty after sanitization"));
    }
    Ok(sanitized)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_server::{Reply, TestServer};

    #[test]
    fn requests_pair_path_and_maps_empty_results() -> Result<()> {
        for (status, body, expected) in [
            (200, r#"{"data":{"payment_routes":[{"split":100}]}}"#, true),
            (200, r#"{"data":{"payment_routes":[]}}"#, false),
            (200, r#"{"data":{"payment_routes":null}}"#, false),
            (200, r#"{"data":{}}"#, false),
            (404, "{}", false),
        ] {
            let server = TestServer::start(move |_| Reply::Answer {
                status,
                body: body.to_owned(),
            })?;
            let client = RouteServiceClient::new(server.base_url(), Duration::from_secs(1))?;
            let got = client.track_routes("F", "I")?;
            assert_eq!(matches!(got, RouteLookup::Routes(_)), expected);
            assert_eq!(
                server.requests(),
                vec!["/v1/feeds/F/tracks/I?include=payment_routes,source_links"]
            );
        }
        Ok(())
    }

    fn answer_server(status: u16, body: &str) -> Result<TestServer> {
        let body = body.to_owned();
        TestServer::start(move |_| Reply::Answer {
            status,
            body: body.clone(),
        })
    }

    #[test]
    fn a_track_answer_gives_the_same_routes_and_the_funding_links() -> Result<()> {
        let body = r#"{"data":{
            "payment_routes":[{"address":"alice","split":100}],
            "source_links":[
                {"link_type":"funding","url":"https://b.example","label":"B","position":2},
                {"link_type":"website","url":"https://site.example","position":0},
                {"link_type":"funding","url":"https://a.example","position":1}
            ]}}"#;
        let server = answer_server(200, body)?;
        let client = RouteServiceClient::new(server.base_url(), Duration::from_secs(1))?;
        let details = client.track_details("F", "I")?;
        assert_eq!(
            server.requests(),
            vec!["/v1/feeds/F/tracks/I?include=payment_routes,source_links"]
        );
        let RouteLookup::Routes(routes) = &details.routes else {
            anyhow::bail!("the answer must give routes");
        };
        assert_eq!(routes.len(), 1);
        assert_eq!(routes[0].address.as_deref(), Some("alice"));
        assert_eq!(
            client.track_routes("F", "I")?,
            details.routes,
            "the routes call gives the routes of the details"
        );
        // Only funding links stay, in the sequence of `position`.
        let urls: Vec<_> = details
            .funding
            .iter()
            .map(|link| link.url.as_str())
            .collect();
        assert_eq!(urls, ["https://a.example", "https://b.example"]);
        assert_eq!(details.funding[1].label.as_deref(), Some("B"));
        Ok(())
    }

    #[test]
    fn bad_source_links_never_change_the_routes() -> Result<()> {
        let routes = r#""payment_routes":[{"address":"alice","split":100}]"#;
        for source_links in [
            r#""source_links":"text""#,
            r#""source_links":{"url":"https://a.example"}"#,
            r#""source_links":null"#,
        ] {
            let server = answer_server(200, &format!(r#"{{"data":{{{routes},{source_links}}}}}"#))?;
            let client = RouteServiceClient::new(server.base_url(), Duration::from_secs(1))?;
            let details = client.track_details("F", "I")?;
            assert!(
                matches!(details.routes, RouteLookup::Routes(_)),
                "{source_links}"
            );
            assert!(details.funding.is_empty(), "{source_links}");
        }
        // An entry that is not an object, or has no string `url`, is ignored.
        // The other entries stay.
        let source_links = r#""source_links":[
            "text", 7, null, [],
            {"link_type":"funding"},
            {"link_type":"funding","url":5},
            {"link_type":"funding","url":"https://a.example","position":1},
            {"link_type":"funding","url":"https://b.example","position":2}
        ]"#;
        let server = answer_server(200, &format!(r#"{{"data":{{{routes},{source_links}}}}}"#))?;
        let client = RouteServiceClient::new(server.base_url(), Duration::from_secs(1))?;
        let details = client.track_details("F", "I")?;
        assert!(matches!(details.routes, RouteLookup::Routes(_)));
        let urls: Vec<_> = details
            .funding
            .iter()
            .map(|link| link.url.as_str())
            .collect();
        assert_eq!(urls, ["https://a.example", "https://b.example"]);
        Ok(())
    }

    #[test]
    fn funding_links_use_a_stable_sort_and_put_a_missing_position_last() -> Result<()> {
        let body = r#"{"data":{"source_links":[
            {"link_type":"funding","url":"https://nopos.example"},
            {"link_type":"funding","url":"https://text.example","position":"1"},
            {"link_type":"funding","url":"https://first.example","position":1},
            {"link_type":"funding","url":"https://same.example","position":1}
        ]}}"#;
        let server = answer_server(200, body)?;
        let client = RouteServiceClient::new(server.base_url(), Duration::from_secs(1))?;
        let urls: Vec<_> = client
            .track_details("F", "I")?
            .funding
            .into_iter()
            .map(|link| link.url)
            .collect();
        // The crate `display-keys` keeps three links at most. An entry with
        // no integer `position` goes after the others, in answer order.
        assert_eq!(
            urls,
            [
                "https://first.example",
                "https://same.example",
                "https://nopos.example",
            ]
        );
        Ok(())
    }

    #[test]
    fn a_track_404_gives_no_routes_and_no_links() -> Result<()> {
        let server = answer_server(404, "{}")?;
        let client = RouteServiceClient::new(server.base_url(), Duration::from_secs(1))?;
        let details = client.track_details("F", "I")?;
        assert_eq!(details.routes, RouteLookup::NoRoutes);
        assert!(details.funding.is_empty());
        Ok(())
    }

    #[test]
    fn the_feed_request_has_the_feed_path_and_gives_the_funding_links() -> Result<()> {
        let body = r#"{"data":{"source_links":[
            {"link_type":"funding","url":"https://b.example","position":2},
            {"link_type":"funding","url":"https://a.example","position":1},
            {"link_type":"website","url":"https://site.example","position":0},
            "text"
        ]}}"#;
        let server = answer_server(200, body)?;
        let client = RouteServiceClient::new(server.base_url(), Duration::from_secs(1))?;
        let links = client.feed_funding("F")?;
        assert_eq!(server.requests(), vec!["/v1/feeds/F?include=source_links"]);
        let urls: Vec<_> = links.iter().map(|link| link.url.as_str()).collect();
        assert_eq!(urls, ["https://a.example", "https://b.example"]);
        Ok(())
    }

    #[test]
    fn the_feed_request_gives_no_links_for_a_404_or_a_bad_list() -> Result<()> {
        for (status, body) in [
            (404, "{}"),
            (200, r#"{"data":{"source_links":"text"}}"#),
            (200, r#"{"data":{}}"#),
            (200, "[]"),
        ] {
            let server = answer_server(status, body)?;
            let client = RouteServiceClient::new(server.base_url(), Duration::from_secs(1))?;
            assert!(client.feed_funding("F")?.is_empty(), "{status} {body}");
        }
        Ok(())
    }

    #[test]
    fn the_feed_request_uses_the_path_segment_check_and_reports_failures() -> Result<()> {
        let server = answer_server(200, "{}")?;
        let client = RouteServiceClient::new(server.base_url(), Duration::from_secs(1))?;
        assert!(client.feed_funding(" ").is_err());
        assert!(server.requests().is_empty());
        for (status, body) in [(500, "failure"), (200, "not json")] {
            let server = answer_server(status, body)?;
            let client = RouteServiceClient::new(server.base_url(), Duration::from_secs(1))?;
            assert!(client.feed_funding("F").is_err(), "{status}");
        }
        Ok(())
    }

    #[test]
    fn rejects_empty_guid_without_request() -> Result<()> {
        let server = TestServer::start(|_| Reply::Answer {
            status: 200,
            body: "{}".into(),
        })?;
        let client = RouteServiceClient::new(server.base_url(), Duration::from_secs(1))?;
        assert!(client.track_routes(" ", "I").is_err());
        assert!(server.requests().is_empty());
        Ok(())
    }

    #[test]
    fn rejects_server_errors_invalid_json_and_timeouts() -> Result<()> {
        for (reply, timeout) in [
            (
                Reply::Answer {
                    status: 500,
                    body: "secret response".into(),
                },
                Duration::from_secs(1),
            ),
            (
                Reply::Answer {
                    status: 200,
                    body: "not json".into(),
                },
                Duration::from_secs(1),
            ),
            (
                Reply::Hang(Duration::from_millis(100)),
                Duration::from_millis(10),
            ),
        ] {
            let server = TestServer::start(move |_| reply.clone())?;
            let client = RouteServiceClient::new(server.base_url(), timeout)?;
            let error = client
                .track_routes("F", "I")
                .expect_err("the response must fail");
            assert!(!format!("{error:#}").contains("secret response"));
        }
        Ok(())
    }
}
