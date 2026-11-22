//! Fetch payment routes for a track identified by its feed and item GUIDs.

use std::time::Duration;

use anyhow::{Context, Result, anyhow};
use reqwest::StatusCode;
use reqwest::blocking::Client;
use serde::Deserialize;

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
        url.query_pairs_mut()
            .append_pair("include", "payment_routes");
        let response = self
            .client
            .get(url)
            .send()
            .context("send route service request")?;
        let status = response.status();
        if status == StatusCode::NOT_FOUND {
            return Ok(RouteLookup::NoRoutes);
        }
        if !status.is_success() {
            return Err(anyhow!("route service returned HTTP {status}"));
        }
        let detail = response
            .json::<DetailResponse>()
            .context("decode route service response")?;
        Ok(
            match detail
                .data
                .payment_routes
                .filter(|routes| !routes.is_empty())
            {
                Some(routes) => RouteLookup::Routes(routes),
                None => RouteLookup::NoRoutes,
            },
        )
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
                vec!["/v1/feeds/F/tracks/I?include=payment_routes"]
            );
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
