//! Resolve the routes of chapter songs and song files in one ordered worker.

use std::collections::{HashMap, HashSet};
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread;
use std::time::{Duration, Instant};

use display_keys::FundingLink;
use now_playing_drop::route_service::{RouteLookup, RouteServiceClient};

use crate::SongRoutes;

type Pair = (String, String);
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
enum LookupKey {
    Track(Pair),
    Feed(String),
}
type Request = (LookupKey, u64);
type Answer = (LookupKey, u64, anyhow::Result<LookupValue>);
enum LookupValue {
    Track(now_playing_drop::route_service::TrackDetails),
    Feed(Vec<FundingLink>),
}

/// Holds route results and schedules route service requests.
#[derive(Debug)]
pub struct RouteLookups {
    client: Option<RouteServiceClient>,
    tx: Option<Sender<Request>>,
    rx: Option<Receiver<Answer>>,
    results: SongRoutes,
    track_funding: HashMap<Pair, Vec<FundingLink>>,
    feed_funding: HashMap<String, Vec<FundingLink>>,
    retries: HashMap<LookupKey, Instant>,
    outstanding: HashMap<LookupKey, u64>,
    current: HashMap<LookupKey, u64>,
    warned: HashSet<LookupKey>,
    next_id: u64,
    seq: Option<i64>,
}

impl RouteLookups {
    /// Creates a lookup cache with an optional route service client.
    pub fn new(client: Option<RouteServiceClient>) -> Self {
        let (tx, rx) = if client.is_some() {
            let (request_tx, request_rx) = mpsc::channel::<Request>();
            let (answer_tx, answer_rx) = mpsc::channel::<Answer>();
            let worker_client = client.clone();
            thread::spawn(move || {
                let Some(client) = worker_client else {
                    return;
                };
                while let Ok((pair, id)) = request_rx.recv() {
                    let answer = match &pair {
                        LookupKey::Track(pair) => client
                            .track_details(&pair.0, &pair.1)
                            .map(LookupValue::Track),
                        LookupKey::Feed(feed) => client.feed_funding(feed).map(LookupValue::Feed),
                    };
                    if answer_tx.send((pair, id, answer)).is_err() {
                        break;
                    }
                }
            });
            (Some(request_tx), Some(answer_rx))
        } else {
            (None, None)
        };
        Self {
            client,
            tx,
            rx,
            results: SongRoutes::new(),
            track_funding: HashMap::new(),
            feed_funding: HashMap::new(),
            retries: HashMap::new(),
            outstanding: HashMap::new(),
            current: HashMap::new(),
            warned: HashSet::new(),
            next_id: 0,
            seq: None,
        }
    }

    /// Takes completed requests and sends due requests in chapter order.
    pub fn update(&mut self, seq: i64, pairs_in_start_order: &[Pair], now: Instant) {
        if self.seq != Some(seq) {
            self.warned.clear();
            let next: HashSet<_> = pairs_in_start_order.iter().cloned().collect();
            self.results.retain(|pair, _| next.contains(pair));
            self.track_funding.retain(|pair, _| next.contains(pair));
            self.feed_funding
                .retain(|feed, _| next.iter().any(|pair| &pair.0 == feed));
            self.retries.retain(|key, _| match key {
                LookupKey::Track(pair) => next.contains(pair),
                LookupKey::Feed(feed) => next.iter().any(|pair| &pair.0 == feed),
            });
            self.outstanding.retain(|key, _| match key {
                LookupKey::Track(pair) => next.contains(pair),
                LookupKey::Feed(feed) => next.iter().any(|pair| &pair.0 == feed),
            });
            self.current.retain(|key, _| match key {
                LookupKey::Track(pair) => next.contains(pair),
                LookupKey::Feed(feed) => next.iter().any(|pair| &pair.0 == feed),
            });
            for pair in pairs_in_start_order {
                let key = LookupKey::Track(pair.clone());
                if !self.current.contains_key(&key) {
                    self.next_id = self.next_id.wrapping_add(1);
                    self.current.insert(key, self.next_id);
                }
            }
            self.seq = Some(seq);
        }
        for pair in pairs_in_start_order {
            let key = LookupKey::Track(pair.clone());
            if self.results.contains_key(pair) || self.outstanding.contains_key(&key) {
                continue;
            }
            let due = self.retries.get(&key).is_none_or(|retry| *retry <= now);
            if due
                && self.client.is_some()
                && let Some(tx) = &self.tx
            {
                let Some(id) = self.current.get(&key).copied() else {
                    continue;
                };
                if tx.send((key.clone(), id)).is_ok() {
                    self.outstanding.insert(key, id);
                }
            }
        }
        for pair in pairs_in_start_order {
            if self.results.contains_key(pair)
                && self.track_funding.get(pair).is_some_and(Vec::is_empty)
                && !self.feed_funding.contains_key(&pair.0)
            {
                let key = LookupKey::Feed(pair.0.clone());
                if !self.current.contains_key(&key) {
                    self.next_id = self.next_id.wrapping_add(1);
                    self.current.insert(key.clone(), self.next_id);
                }
                if self.outstanding.contains_key(&key) {
                    continue;
                }
                let due = self.retries.get(&key).is_none_or(|retry| *retry <= now);
                if due && let Some(tx) = &self.tx {
                    let id = self.current[&key];
                    if tx.send((key.clone(), id)).is_ok() {
                        self.outstanding.insert(key, id);
                    }
                }
            }
        }
    }

    /// Stores completed answers and schedules retries after failures.
    pub fn drain(&mut self, now: Instant) {
        let Some(rx) = &self.rx else {
            return;
        };
        for (key, id, answer) in rx.try_iter() {
            if self.outstanding.get(&key) == Some(&id) {
                self.outstanding.remove(&key);
            }
            if self.current.get(&key) != Some(&id) {
                continue;
            }
            match answer {
                Ok(LookupValue::Track(details)) => {
                    let LookupKey::Track(pair) = key else {
                        continue;
                    };
                    self.results.insert(
                        pair.clone(),
                        match details.routes {
                            RouteLookup::Routes(routes) => routes,
                            RouteLookup::NoRoutes => Vec::new(),
                        },
                    );
                    self.track_funding.insert(pair.clone(), details.funding);
                    self.retries.remove(&LookupKey::Track(pair));
                }
                Ok(LookupValue::Feed(links)) => {
                    let LookupKey::Feed(feed) = key else { continue };
                    self.feed_funding.insert(feed.clone(), links);
                    self.retries.remove(&LookupKey::Feed(feed));
                }
                Err(error) => {
                    if self.warned.insert(key.clone()) {
                        match &key {
                            LookupKey::Track(pair) => {
                                tracing::warn!(feed_guid = %pair.0, item_guid = %pair.1, error = %format!("{error:#}"), "route lookup failed")
                            }
                            LookupKey::Feed(feed) => {
                                tracing::warn!(feed_guid = %feed, error = %format!("{error:#}"), "feed funding lookup failed")
                            }
                        }
                    }
                    self.retries.insert(key, now + Duration::from_secs(30));
                }
            }
        }
    }

    /// Returns the resolved routes for chapter rendering.
    pub fn results(&self) -> &SongRoutes {
        &self.results
    }

    /// Returns item funding links, or the cached feed links when item links are empty.
    pub fn funding(&self, pair: &(String, String)) -> &[FundingLink] {
        self.track_funding
            .get(pair)
            .filter(|links| !links.is_empty())
            .map(Vec::as_slice)
            .or_else(|| self.feed_funding.get(&pair.0).map(Vec::as_slice))
            .unwrap_or(&[])
    }

    #[cfg(test)]
    pub(crate) fn insert_for_test(&mut self, routes: SongRoutes) {
        self.results = routes;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use anyhow::Result;
    use now_playing_drop::test_server::{Reply, TestServer};

    fn wait_for_routes(lookups: &mut RouteLookups, now: Instant, count: usize) -> bool {
        let started = Instant::now();
        while started.elapsed() < Duration::from_secs(5) {
            lookups.drain(now);
            if lookups.results().len() >= count {
                return true;
            }
            thread::sleep(Duration::from_millis(5));
        }
        false
    }

    #[test]
    fn requests_pairs_in_order_and_keeps_results_for_shared_pairs() -> Result<()> {
        // Each answer has a funding link, so the lookup sends no feed request.
        let server = TestServer::start(|_| {
            Reply::Answer {
            status: 200,
            body: r#"{"data":{"payment_routes":[{"address":"route","split":100}],"source_links":[{"link_type":"funding","url":"https://example.com/pay","position":1}]}}"#.into(),
        }
        })?;
        let client = RouteServiceClient::new(server.base_url(), Duration::from_secs(1))?;
        let mut lookups = RouteLookups::new(Some(client));
        let now = Instant::now();
        let pairs = vec![
            ("feed-b".into(), "item-b".into()),
            ("feed-a".into(), "item-a".into()),
        ];
        lookups.update(1, &pairs, now);
        assert!(wait_for_routes(&mut lookups, now, 2));
        assert_eq!(
            server.requests(),
            vec![
                "/v1/feeds/feed-b/tracks/item-b?include=payment_routes,source_links",
                "/v1/feeds/feed-a/tracks/item-a?include=payment_routes,source_links",
            ]
        );
        lookups.update(2, &pairs[..1], now + Duration::from_secs(1));
        assert_eq!(lookups.results().len(), 1);
        lookups.update(3, &[], now + Duration::from_secs(2));
        lookups.update(4, &pairs[..1], now + Duration::from_secs(3));
        assert!(wait_for_routes(
            &mut lookups,
            now + Duration::from_secs(3),
            1
        ));
        assert_eq!(server.requests().len(), 3);
        Ok(())
    }

    #[test]
    fn keeps_feed_results_for_feeds_in_the_new_list_of_pairs() -> Result<()> {
        // A track answer has no funding link, and a feed answer has one.
        let server = TestServer::start(|target| Reply::Answer {
            status: 200,
            body: if target.contains("/tracks/") {
                r#"{"data":{}}"#.into()
            } else {
                r#"{"data":{"source_links":[{"link_type":"funding","url":"https://example.com/feed","position":1}]}}"#.into()
            },
        })?;
        let client = RouteServiceClient::new(server.base_url(), Duration::from_secs(1))?;
        let mut lookups = RouteLookups::new(Some(client));
        let now = Instant::now();
        let first = ("feed".to_owned(), "item-1".to_owned());
        let second = ("feed".to_owned(), "item-2".to_owned());

        let started = Instant::now();
        while lookups.funding(&first).is_empty() && started.elapsed() < Duration::from_secs(5) {
            lookups.drain(now);
            lookups.update(1, std::slice::from_ref(&first), now);
            thread::sleep(Duration::from_millis(5));
        }
        assert_eq!(lookups.funding(&first)[0].url, "https://example.com/feed");
        assert_eq!(server.requests().len(), 2);

        // The new list has another item of the same feed. The feed result stays,
        // and no second feed request follows.
        lookups.update(2, std::slice::from_ref(&second), now);
        assert!(lookups.feed_funding.contains_key("feed"));
        let started = Instant::now();
        while !lookups.results().contains_key(&second) && started.elapsed() < Duration::from_secs(5)
        {
            lookups.drain(now);
            thread::sleep(Duration::from_millis(5));
        }
        lookups.update(2, std::slice::from_ref(&second), now);
        thread::sleep(Duration::from_millis(100));
        assert_eq!(lookups.funding(&second)[0].url, "https://example.com/feed");
        assert_eq!(server.requests().len(), 3);

        // A list with no pair of the feed removes the feed result.
        lookups.update(3, &[], now);
        assert!(lookups.feed_funding.is_empty());
        Ok(())
    }

    #[test]
    fn warns_once_per_pair_and_clears_warnings_for_a_new_seq() -> Result<()> {
        let server = TestServer::start(|_| Reply::Answer {
            status: 500,
            body: "failure".into(),
        })?;
        let client = RouteServiceClient::new(server.base_url(), Duration::from_secs(1))?;
        let mut lookups = RouteLookups::new(Some(client));
        let pair = ("feed".to_owned(), "item".to_owned());
        let now = Instant::now();

        lookups.update(1, std::slice::from_ref(&pair), now);
        let started = Instant::now();
        while lookups.warned.is_empty() && started.elapsed() < Duration::from_secs(5) {
            lookups.drain(now);
            thread::sleep(Duration::from_millis(5));
        }
        assert_eq!(lookups.warned.len(), 1);
        assert!(lookups.warned.contains(&LookupKey::Track(pair.clone())));

        lookups.update(
            1,
            std::slice::from_ref(&pair),
            now + Duration::from_secs(30),
        );
        let started = Instant::now();
        while server.requests().len() < 2 && started.elapsed() < Duration::from_secs(5) {
            lookups.drain(now + Duration::from_secs(30));
            thread::sleep(Duration::from_millis(5));
        }
        lookups.drain(now + Duration::from_secs(30));
        assert_eq!(lookups.warned.len(), 1);

        lookups.update(
            2,
            std::slice::from_ref(&pair),
            now + Duration::from_secs(31),
        );
        assert!(lookups.warned.is_empty());
        Ok(())
    }
}
