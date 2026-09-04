use std::collections::HashMap;
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use anyhow::{Context, Result, anyhow};
use reqwest::StatusCode;
use reqwest::blocking::Client;
use serde::{Deserialize, Serialize};

use crate::tags::TrackTags;

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
#[serde(default)]
pub struct PaymentRoute {
    pub recipient_name: Option<String>,
    pub route_type: Option<String>,
    pub split: Option<f64>,
    pub fee: Option<bool>,
    pub address: Option<String>,
    pub custom_key: Option<String>,
    pub custom_value: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ValueRoutesSource {
    EmbeddedId3,
    MusicIndexApi,
}

impl ValueRoutesSource {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::EmbeddedId3 => "embedded-id3",
            Self::MusicIndexApi => "musicindex-api",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RouteResolution {
    pub source: ValueRoutesSource,
    pub routes_json: Option<String>,
}

impl RouteResolution {
    pub fn embedded(tags: &TrackTags) -> Self {
        Self {
            source: ValueRoutesSource::EmbeddedId3,
            routes_json: embedded_value_routes(tags).map(str::to_string),
        }
    }

    pub fn api(routes_json: String) -> Self {
        Self {
            source: ValueRoutesSource::MusicIndexApi,
            routes_json: Some(routes_json),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RouteRequest {
    pub track_guid: Option<String>,
    pub feed_guid: Option<String>,
    pub embedded_routes_json: Option<String>,
}

impl RouteRequest {
    pub fn from_tags(tags: &TrackTags) -> Self {
        Self {
            track_guid: tags.musicindex_value("Track Guid").map(str::to_string),
            feed_guid: tags.musicindex_value("Feed Guid").map(str::to_string),
            embedded_routes_json: embedded_value_routes(tags).map(str::to_string),
        }
    }

    fn has_lookup_key(&self) -> bool {
        self.track_guid.is_some() || self.feed_guid.is_some()
    }

    fn embedded_resolution(&self) -> RouteResolution {
        RouteResolution {
            source: ValueRoutesSource::EmbeddedId3,
            routes_json: self.embedded_routes_json.clone(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedRouteResult {
    pub hist_id: i64,
    pub resolution: RouteResolution,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RouteRequestStatus {
    Disabled,
    NoLookupKey,
    Spawned,
    Cached(ResolvedRouteResult),
}

#[derive(Debug, Clone)]
pub struct MusicIndexClient {
    base_url: String,
    client: Client,
}

impl MusicIndexClient {
    pub fn new(base_url: impl Into<String>, timeout: Duration) -> Result<Self> {
        let base_url = base_url.into().trim_end_matches('/').to_string();
        reqwest::Url::parse(&format!("{base_url}/"))
            .with_context(|| format!("parse MusicIndex base URL {base_url:?}"))?;
        let client = Client::builder()
            .timeout(timeout)
            .build()
            .context("build MusicIndex HTTP client")?;
        Ok(Self { base_url, client })
    }

    pub fn resolve_value_routes(&self, request: &RouteRequest, verbose: bool) -> RouteResolution {
        if !request.has_lookup_key() {
            return request.embedded_resolution();
        }

        if let Some(track_guid) = request.track_guid.as_deref() {
            match self.fetch_routes("tracks", track_guid) {
                Ok(routes) => return routes,
                Err(LookupError::NotFound) => {}
                Err(error) => {
                    if verbose {
                        eprintln!(
                            "warning: MusicIndex track route lookup failed for {track_guid}: {error}"
                        );
                    }
                    return request.embedded_resolution();
                }
            }
        }

        if let Some(feed_guid) = request.feed_guid.as_deref() {
            match self.fetch_routes("feeds", feed_guid) {
                Ok(routes) => return routes,
                Err(error) => {
                    if verbose {
                        eprintln!(
                            "warning: MusicIndex feed route lookup failed for {feed_guid}: {error}"
                        );
                    }
                }
            }
        }

        request.embedded_resolution()
    }

    fn fetch_routes(&self, entity_path: &str, guid: &str) -> Result<RouteResolution, LookupError> {
        let mut url = reqwest::Url::parse(&format!("{}/", self.base_url.trim_end_matches('/')))
            .map_err(|error| LookupError::Other(anyhow!(error)))?;
        {
            let mut segments = url
                .path_segments_mut()
                .map_err(|_| LookupError::Other(anyhow!("base URL cannot be a base")))?;
            segments.push("v1");
            segments.push(entity_path);
            segments.push(&sanitize_api_path_segment(guid)?);
        }
        url.query_pairs_mut()
            .append_pair("include", "payment_routes");

        let response = self.client.get(url).send().map_err(LookupError::from)?;
        let status = response.status();
        if status == StatusCode::NOT_FOUND {
            return Err(LookupError::NotFound);
        }
        if !status.is_success() {
            let body = response.text().unwrap_or_default();
            return Err(LookupError::Other(anyhow!(
                "GET failed with HTTP {status}: {body}"
            )));
        }

        let body = response.text().map_err(LookupError::from)?;
        let detail = serde_json::from_str::<DetailResponse<EntityWithRoutes>>(&body)
            .with_context(|| "decode MusicIndex route response")
            .map_err(LookupError::Other)?;
        let Some(routes) = detail
            .data
            .payment_routes
            .filter(|routes| !routes.is_empty())
        else {
            return Err(LookupError::NoRoutes);
        };
        let routes_json = serde_json::to_string(&routes).map_err(LookupError::from)?;
        Ok(RouteResolution::api(routes_json))
    }
}

#[derive(Debug, Deserialize)]
struct DetailResponse<T> {
    data: T,
}

#[derive(Debug, Deserialize, Default)]
#[serde(default)]
struct EntityWithRoutes {
    payment_routes: Option<Vec<PaymentRoute>>,
}

#[derive(Debug)]
enum LookupError {
    NotFound,
    NoRoutes,
    Other(anyhow::Error),
}

impl std::fmt::Display for LookupError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NotFound => write!(f, "not found"),
            Self::NoRoutes => write!(f, "no payment routes"),
            Self::Other(error) => write!(f, "{error:#}"),
        }
    }
}

impl std::error::Error for LookupError {}

impl From<anyhow::Error> for LookupError {
    fn from(error: anyhow::Error) -> Self {
        Self::Other(error)
    }
}

impl From<reqwest::Error> for LookupError {
    fn from(error: reqwest::Error) -> Self {
        Self::Other(error.into())
    }
}

impl From<serde_json::Error> for LookupError {
    fn from(error: serde_json::Error) -> Self {
        Self::Other(error.into())
    }
}

#[derive(Debug)]
pub struct ValueRouteResolver {
    enabled: bool,
    client: MusicIndexClient,
    cache: Arc<Mutex<HashMap<String, RouteResolution>>>,
    tx: Sender<ResolvedRouteResult>,
    rx: Receiver<ResolvedRouteResult>,
    verbose: bool,
}

impl ValueRouteResolver {
    pub fn new(
        enabled: bool,
        base_url: impl Into<String>,
        timeout: Duration,
        verbose: bool,
    ) -> Result<Self> {
        let (tx, rx) = mpsc::channel();
        Ok(Self {
            enabled,
            client: MusicIndexClient::new(base_url, timeout)?,
            cache: Arc::new(Mutex::new(HashMap::new())),
            tx,
            rx,
            verbose,
        })
    }

    pub fn request(&mut self, hist_id: i64, tags: &TrackTags) -> RouteRequestStatus {
        if !self.enabled {
            return RouteRequestStatus::Disabled;
        }

        let request = RouteRequest::from_tags(tags);
        if !request.has_lookup_key() {
            return RouteRequestStatus::NoLookupKey;
        }

        if let Some(track_guid) = request.track_guid.as_deref()
            && let Some(resolution) = self.cached(track_guid)
        {
            return RouteRequestStatus::Cached(ResolvedRouteResult {
                hist_id,
                resolution,
            });
        }

        let client = self.client.clone();
        let cache = Arc::clone(&self.cache);
        let tx = self.tx.clone();
        let verbose = self.verbose;
        thread::spawn(move || {
            let resolution = client.resolve_value_routes(&request, verbose);
            if resolution.source == ValueRoutesSource::MusicIndexApi
                && let Some(track_guid) = request.track_guid.as_deref()
            {
                cache_resolution(&cache, track_guid, &resolution);
            }
            let _ = tx.send(ResolvedRouteResult {
                hist_id,
                resolution,
            });
        });

        RouteRequestStatus::Spawned
    }

    pub fn drain(&mut self) -> Vec<ResolvedRouteResult> {
        self.rx.try_iter().collect()
    }

    fn cached(&self, track_guid: &str) -> Option<RouteResolution> {
        self.cache
            .lock()
            .ok()
            .and_then(|cache| cache.get(track_guid).cloned())
    }
}

pub fn apply_resolution_to_tags(tags: &TrackTags, resolution: &RouteResolution) -> TrackTags {
    let mut tags = tags.clone();
    match &resolution.routes_json {
        Some(routes_json) => tags.set_musicindex_value("Value Routes", routes_json.clone()),
        None => tags.remove_musicindex_value("Value Routes"),
    }
    tags
}

pub fn embedded_value_routes(tags: &TrackTags) -> Option<&str> {
    tags.musicindex_value("Value Routes")
        .filter(|value| !value.trim().is_empty())
}

pub fn result_matches_current(result: &ResolvedRouteResult, current_hist_id: i64) -> bool {
    result.hist_id == current_hist_id
}

fn cache_resolution(
    cache: &Arc<Mutex<HashMap<String, RouteResolution>>>,
    track_guid: &str,
    resolution: &RouteResolution,
) {
    if let Ok(mut cache) = cache.lock() {
        cache.insert(track_guid.to_string(), resolution.clone());
    }
}

fn sanitize_api_path_segment(value: &str) -> std::result::Result<String, LookupError> {
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
        return Err(LookupError::Other(anyhow!("API path segment is empty")));
    }
    Ok(sanitized)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tags::{TagText, TrackTags};

    #[test]
    fn musicindex_extracts_route_request_from_tags() {
        let tags = TrackTags {
            musicindex: vec![
                TagText::new("Track Guid", "track-guid"),
                TagText::new("Feed Guid", "feed-guid"),
                TagText::new("Value Routes", "[]"),
            ],
            tags: Vec::new(),
            duration: None,
        };

        let request = RouteRequest::from_tags(&tags);

        assert_eq!(request.track_guid.as_deref(), Some("track-guid"));
        assert_eq!(request.feed_guid.as_deref(), Some("feed-guid"));
        assert_eq!(request.embedded_routes_json.as_deref(), Some("[]"));
    }

    #[test]
    fn musicindex_applies_absent_routes_by_removing_value_routes() {
        let tags = TrackTags {
            musicindex: vec![TagText::new("Value Routes", "[]")],
            tags: Vec::new(),
            duration: None,
        };
        let resolution = RouteResolution {
            source: ValueRoutesSource::EmbeddedId3,
            routes_json: None,
        };

        let tags = apply_resolution_to_tags(&tags, &resolution);

        assert!(tags.musicindex_value("Value Routes").is_none());
    }

    #[test]
    fn musicindex_result_match_checks_hist_id() {
        let result = ResolvedRouteResult {
            hist_id: 10,
            resolution: RouteResolution::api("[]".to_string()),
        };

        assert!(result_matches_current(&result, 10));
        assert!(!result_matches_current(&result, 11));
    }
}
