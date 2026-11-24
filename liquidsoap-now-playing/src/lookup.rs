//! Resolve the routes of chapter songs and song files in one ordered worker.

use std::collections::{HashMap, HashSet};
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread;
use std::time::{Duration, Instant};

use now_playing_drop::route_service::{RouteLookup, RouteServiceClient};

use crate::SongRoutes;

type Pair = (String, String);
type Request = (Pair, u64);
type Answer = (Pair, u64, anyhow::Result<RouteLookup>);

/// Holds route results and schedules route service requests.
#[derive(Debug)]
pub struct RouteLookups {
    client: Option<RouteServiceClient>,
    tx: Option<Sender<Request>>,
    rx: Option<Receiver<Answer>>,
    results: SongRoutes,
    retries: HashMap<Pair, Instant>,
    outstanding: HashMap<Pair, u64>,
    current: HashMap<Pair, u64>,
    warned: HashSet<Pair>,
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
                    let answer = client.track_routes(&pair.0, &pair.1);
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
            self.retries.retain(|pair, _| next.contains(pair));
            self.outstanding.retain(|pair, _| next.contains(pair));
            self.current.retain(|pair, _| next.contains(pair));
            for pair in pairs_in_start_order {
                if !self.current.contains_key(pair) {
                    self.next_id = self.next_id.wrapping_add(1);
                    self.current.insert(pair.clone(), self.next_id);
                }
            }
            self.seq = Some(seq);
        }
        for pair in pairs_in_start_order {
            if self.results.contains_key(pair) || self.outstanding.contains_key(pair) {
                continue;
            }
            let due = self.retries.get(pair).is_none_or(|retry| *retry <= now);
            if due
                && self.client.is_some()
                && let Some(tx) = &self.tx
            {
                let Some(id) = self.current.get(pair).copied() else {
                    continue;
                };
                if tx.send((pair.clone(), id)).is_ok() {
                    self.outstanding.insert(pair.clone(), id);
                }
            }
        }
    }

    /// Stores completed answers and schedules retries after failures.
    pub fn drain(&mut self, now: Instant) {
        let Some(rx) = &self.rx else {
            return;
        };
        for (pair, id, answer) in rx.try_iter() {
            if self.outstanding.get(&pair) == Some(&id) {
                self.outstanding.remove(&pair);
            }
            if self.current.get(&pair) != Some(&id) {
                continue;
            }
            match answer {
                Ok(RouteLookup::Routes(routes)) => {
                    self.results.insert(pair.clone(), routes);
                    self.retries.remove(&pair);
                }
                Ok(RouteLookup::NoRoutes) => {
                    self.results.insert(pair.clone(), Vec::new());
                    self.retries.remove(&pair);
                }
                Err(error) => {
                    if self.warned.insert(pair.clone()) {
                        tracing::warn!(feed_guid = %pair.0, item_guid = %pair.1, error = %format!("{error:#}"), "route lookup failed");
                    }
                    self.retries.insert(pair, now + Duration::from_secs(30));
                }
            }
        }
    }

    /// Returns the resolved routes for chapter rendering.
    pub fn results(&self) -> &SongRoutes {
        &self.results
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
        let server = TestServer::start(|_| Reply::Answer {
            status: 200,
            body: r#"{"data":{"payment_routes":[{"address":"route","split":100}]}}"#.into(),
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
                "/v1/feeds/feed-b/tracks/item-b?include=payment_routes",
                "/v1/feeds/feed-a/tracks/item-a?include=payment_routes",
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
        assert!(lookups.warned.contains(&pair));

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
