use std::collections::HashMap;
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use anyhow::Result;
use now_playing_drop::route_service::{RouteLookup, RouteServiceClient};

use crate::tags::TrackTags;

/// Resolves value routes from the route service or embedded track metadata.
pub fn resolve_value_routes(
    client: &RouteServiceClient,
    request: &RouteRequest,
    verbose: bool,
) -> RouteResolution {
    let (Some(feed_guid), Some(track_guid)) =
        (request.feed_guid.as_deref(), request.track_guid.as_deref())
    else {
        return request.embedded_resolution();
    };

    match client.track_routes(feed_guid, track_guid) {
        Ok(RouteLookup::Routes(routes)) => serde_json::to_string(&routes)
            .map(RouteResolution::api)
            .unwrap_or_else(|_| request.embedded_resolution()),
        Ok(RouteLookup::NoRoutes) => request.embedded_resolution(),
        Err(error) => {
            if verbose {
                eprintln!(
                    "warning: MusicIndex track route lookup failed for {track_guid}: {error:#}"
                );
            }
            request.embedded_resolution()
        }
    }
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
        self.track_guid
            .as_deref()
            .is_some_and(|guid| !guid.trim().is_empty())
            && self
                .feed_guid
                .as_deref()
                .is_some_and(|guid| !guid.trim().is_empty())
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

#[derive(Debug)]
pub struct ValueRouteResolver {
    enabled: bool,
    client: RouteServiceClient,
    cache: Arc<Mutex<HashMap<(String, String), RouteResolution>>>,
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
            client: RouteServiceClient::new(base_url, timeout)?,
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

        let (Some(feed_guid), Some(track_guid)) =
            (request.feed_guid.as_deref(), request.track_guid.as_deref())
        else {
            return RouteRequestStatus::NoLookupKey;
        };
        if let Some(resolution) = self.cached(feed_guid, track_guid) {
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
            let resolution = resolve_value_routes(&client, &request, verbose);
            if resolution.source == ValueRoutesSource::MusicIndexApi {
                cache_resolution(&cache, &request, &resolution);
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

    fn cached(&self, feed_guid: &str, track_guid: &str) -> Option<RouteResolution> {
        self.cache.lock().ok().and_then(|cache| {
            cache
                .get(&(feed_guid.to_owned(), track_guid.to_owned()))
                .cloned()
        })
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
    cache: &Arc<Mutex<HashMap<(String, String), RouteResolution>>>,
    request: &RouteRequest,
    resolution: &RouteResolution,
) {
    if let (Some(feed), Some(track)) = (request.feed_guid.as_deref(), request.track_guid.as_deref())
        && let Ok(mut cache) = cache.lock()
    {
        cache.insert((feed.to_owned(), track.to_owned()), resolution.clone());
    }
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
    fn musicindex_does_not_request_routes_with_one_guid() -> Result<()> {
        let tags = TrackTags {
            musicindex: vec![
                TagText::new("Track Guid", "track-guid"),
                TagText::new("Value Routes", "[]"),
            ],
            tags: Vec::new(),
            duration: None,
        };
        let mut resolver =
            ValueRouteResolver::new(true, "http://127.0.0.1:1", Duration::from_secs(1), false)?;

        assert_eq!(resolver.request(1, &tags), RouteRequestStatus::NoLookupKey);
        assert!(resolver.drain().is_empty());
        Ok(())
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
