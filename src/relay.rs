//! MusicIndex relay HTTP client and retry workers.

use std::collections::HashMap;
use std::fmt;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::Path;
use std::sync::{Arc, Mutex, mpsc};
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result, anyhow};
use reqwest::StatusCode;
use reqwest::blocking::Client;
use serde::Deserialize;
use serde_json::Value;

use crate::{LiveValuePayload, PublisherConfig, PublisherTarget};

/// Default per-request timeout for relay calls.
pub const DEFAULT_REQUEST_TIMEOUT: Duration = Duration::from_secs(10);
/// First retry delay after a retryable relay failure.
pub const DEFAULT_INITIAL_BACKOFF: Duration = Duration::from_millis(250);
/// Maximum retry delay after repeated relay failures.
pub const DEFAULT_MAX_BACKOFF: Duration = Duration::from_secs(30);

/// A configured relay publish target.
#[derive(Clone, PartialEq)]
pub struct RelayTarget {
    pub name: String,
    pub endpoint: String,
    pub event_id: String,
    pub token: String,
}

impl fmt::Debug for RelayTarget {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("RelayTarget")
            .field("name", &self.name)
            .field("endpoint", &self.endpoint)
            .field("event_id", &self.event_id)
            .field("token", &"<redacted>")
            .finish()
    }
}

impl RelayTarget {
    /// Builds relay target settings from service configuration.
    pub fn from_config(endpoint: &str, target: &PublisherTarget) -> Self {
        Self {
            name: target.name.clone(),
            endpoint: endpoint.to_owned(),
            event_id: target.event_id.clone(),
            token: target.token.clone(),
        }
    }
}

/// Result of one publish attempt.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PublishOutcome {
    Accepted { seq: u64 },
    Retryable { reason: String },
    Dropped { reason: String },
    Fatal { reason: String },
}

/// Response from `POST /v1/liveitems`.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct ProvisionedLiveItem {
    pub event_id: String,
    pub broadcaster_token: String,
    pub metadata_url: String,
    pub remote_value_url: String,
    pub events_url: String,
    pub socket_io_url: String,
}

#[derive(Debug, Deserialize)]
struct PublishResponse {
    seq: u64,
}

/// Blocking HTTP client for the MusicIndex relay.
#[derive(Debug, Clone)]
pub struct RelayClient {
    client: Client,
}

impl RelayClient {
    /// Creates a relay client with the supplied timeout.
    ///
    /// # Errors
    ///
    /// Returns an error when the underlying HTTP client cannot be built.
    pub fn new(timeout: Duration) -> Result<Self> {
        let client = Client::builder()
            .timeout(timeout)
            .build()
            .context("build relay HTTP client")?;
        Ok(Self { client })
    }

    /// Publishes one direct live value payload to the relay.
    ///
    /// # Errors
    ///
    /// Returns an error when the target settings or payload shape are invalid.
    pub fn publish(
        &self,
        target: &RelayTarget,
        payload: &LiveValuePayload,
    ) -> Result<PublishOutcome> {
        let value = serde_json::to_value(payload).context("serialize live value payload")?;
        self.publish_value(target, &value)
    }

    /// Publishes one direct JSON payload to the relay.
    ///
    /// This helper exists so tests can exercise the pre-send wrapped-shape
    /// rejection without changing task 002's live value structs.
    ///
    /// # Errors
    ///
    /// Returns an error when the target settings or payload shape are invalid.
    pub fn publish_value(&self, target: &RelayTarget, payload: &Value) -> Result<PublishOutcome> {
        reject_wrapped_payload(payload)?;
        validate_bearer_token(&target.token)?;
        let url = build_url(
            &target.endpoint,
            &["v1", "liveitems", &target.event_id, "metadata"],
        )?;

        let response = match self
            .client
            .post(url)
            .bearer_auth(&target.token)
            .json(payload)
            .send()
        {
            Ok(response) => response,
            Err(error) => {
                return Ok(PublishOutcome::Retryable {
                    reason: format!("network error: {error}"),
                });
            }
        };

        let status = response.status();
        match status {
            StatusCode::OK => {
                let body: PublishResponse = response
                    .json()
                    .context("parse relay publish success response")?;
                Ok(PublishOutcome::Accepted { seq: body.seq })
            }
            StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN | StatusCode::NOT_FOUND => {
                Ok(PublishOutcome::Fatal {
                    reason: format!("relay returned HTTP {status}"),
                })
            }
            StatusCode::PAYLOAD_TOO_LARGE => Ok(PublishOutcome::Dropped {
                reason: format!("relay returned HTTP {status}"),
            }),
            StatusCode::TOO_MANY_REQUESTS => Ok(PublishOutcome::Retryable {
                reason: format!("relay returned HTTP {status}"),
            }),
            status if status.is_server_error() => Ok(PublishOutcome::Retryable {
                reason: format!("relay returned HTTP {status}"),
            }),
            status => Ok(PublishOutcome::Fatal {
                reason: format!("relay returned unexpected HTTP {status}"),
            }),
        }
    }

    /// Provisions a live item and returns its one-time token response.
    ///
    /// # Errors
    ///
    /// Returns an error if the relay request fails or the response is invalid.
    pub fn provision(&self, endpoint: &str) -> Result<ProvisionedLiveItem> {
        let url = build_url(endpoint, &["v1", "liveitems"])?;
        let response = self.client.post(url).send().context("POST /v1/liveitems")?;
        let status = response.status();
        if !status.is_success() {
            let body = response
                .text()
                .unwrap_or_else(|error| format!("failed to read response body: {error}"));
            return Err(anyhow!(
                "POST /v1/liveitems failed with HTTP {status}: {body}"
            ));
        }
        response
            .json()
            .context("parse relay live item provision response")
    }
}

/// Writes a broadcaster token file with private permissions where supported.
///
/// # Errors
///
/// Returns an error when the token file cannot be created or written.
pub fn write_token_file(path: &Path, token: &str) -> Result<()> {
    let mut options = OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }

    let mut file = options
        .open(path)
        .with_context(|| format!("create token file {}", path.display()))?;
    file.write_all(token.as_bytes())
        .with_context(|| format!("write token file {}", path.display()))?;
    file.write_all(b"\n")
        .with_context(|| format!("finish token file {}", path.display()))?;

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o600))
            .with_context(|| format!("set token file permissions {}", path.display()))?;
    }

    Ok(())
}

/// Coordinates per-target relay workers.
#[derive(Debug)]
pub struct RelayPublisher {
    senders: HashMap<String, mpsc::Sender<LiveValuePayload>>,
    fatal: Arc<Mutex<Option<String>>>,
}

impl RelayPublisher {
    /// Starts a relay worker for each configured target.
    ///
    /// # Errors
    ///
    /// Returns an error when the relay HTTP client cannot be initialized.
    pub fn start(config: &PublisherConfig) -> Result<Self> {
        Self::start_with_backoff(config, DEFAULT_INITIAL_BACKOFF, DEFAULT_MAX_BACKOFF)
    }

    /// Starts relay workers with explicit backoff settings.
    ///
    /// # Errors
    ///
    /// Returns an error when the relay HTTP client cannot be initialized.
    pub fn start_with_backoff(
        config: &PublisherConfig,
        initial_backoff: Duration,
        max_backoff: Duration,
    ) -> Result<Self> {
        let client = RelayClient::new(DEFAULT_REQUEST_TIMEOUT)?;
        let mut senders = HashMap::new();
        let fatal = Arc::new(Mutex::new(None));

        for target in &config.targets {
            let relay_target = RelayTarget::from_config(&config.endpoint, target);
            let (sender, receiver) = mpsc::channel();
            thread::Builder::new()
                .name(format!("relay-publisher-{}", relay_target.name))
                .spawn({
                    let client = client.clone();
                    let fatal = Arc::clone(&fatal);
                    move || {
                        publish_worker(
                            client,
                            relay_target,
                            receiver,
                            initial_backoff,
                            max_backoff,
                            &fatal,
                        )
                    }
                })
                .context("spawn relay publish worker")?;
            senders.insert(target.event_id.clone(), sender);
        }

        Ok(Self { senders, fatal })
    }

    /// Returns an error once any target has failed fatally.
    ///
    /// The watch loop calls this while idle so a revoked token surfaces within
    /// seconds instead of waiting for the next track change. A fatal target
    /// means the relay is serving a payload nobody can replace, so the process
    /// must stop rather than look healthy.
    ///
    /// # Errors
    ///
    /// Returns the recorded failure reason if a relay worker stopped fatally.
    pub fn check_health(&self) -> Result<()> {
        let fatal = self
            .fatal
            .lock()
            .map_err(|_| anyhow!("relay failure state poisoned"))?;
        match fatal.as_deref() {
            Some(reason) => Err(anyhow!("relay publishing stopped: {reason}")),
            None => Ok(()),
        }
    }

    /// Queues a payload for relay publishing.
    ///
    /// # Errors
    ///
    /// Returns an error when no worker exists for the payload's event.
    pub fn publish(&self, payload: LiveValuePayload) -> Result<()> {
        let event_id = payload.event_guid.clone();
        let sender = self
            .senders
            .get(&event_id)
            .ok_or_else(|| anyhow!("no relay target configured for event_id {event_id}"))?;
        sender
            .send(payload)
            .map_err(|_| anyhow!("relay publish worker for event_id {event_id} stopped"))
    }
}

fn publish_worker(
    client: RelayClient,
    target: RelayTarget,
    receiver: mpsc::Receiver<LiveValuePayload>,
    initial_backoff: Duration,
    max_backoff: Duration,
    fatal: &Mutex<Option<String>>,
) {
    while let Ok(mut payload) = receiver.recv() {
        let mut backoff = initial_backoff;
        loop {
            match client.publish(&target, &payload) {
                Ok(PublishOutcome::Accepted { seq }) => {
                    tracing::info!(
                        target = %target.name,
                        event_id = %target.event_id,
                        seq,
                        "published live value payload"
                    );
                    break;
                }
                Ok(PublishOutcome::Dropped { reason }) => {
                    tracing::error!(
                        target = %target.name,
                        event_id = %target.event_id,
                        %reason,
                        "dropping live value payload"
                    );
                    break;
                }
                // A fatal outcome must never leave a live-looking daemon that
                // publishes nothing: the relay would keep serving the last
                // payload, so a track that has already ended would keep
                // collecting boosts for its destinations. Stop the worker,
                // which closes the channel and makes the next publish fail the
                // process so systemd reports it.
                Ok(PublishOutcome::Fatal { reason }) => {
                    tracing::error!(
                        target = %target.name,
                        event_id = %target.event_id,
                        %reason,
                        "fatal relay publish failure; stopping publisher"
                    );
                    if let Ok(mut fatal) = fatal.lock() {
                        *fatal = Some(format!("target {}: {reason}", target.name));
                    }
                    return;
                }
                Ok(PublishOutcome::Retryable { reason }) => {
                    tracing::warn!(
                        target = %target.name,
                        event_id = %target.event_id,
                        %reason,
                        delay_ms = backoff.as_millis(),
                        "relay publish failed; backing off"
                    );
                    let retry_delay = jittered_delay(backoff);
                    match receiver.recv_timeout(retry_delay) {
                        Ok(newer_payload) => {
                            payload = newer_payload;
                            backoff = initial_backoff;
                        }
                        Err(mpsc::RecvTimeoutError::Timeout) => {
                            backoff = next_backoff(backoff, max_backoff);
                        }
                        Err(mpsc::RecvTimeoutError::Disconnected) => return,
                    }
                }
                Err(error) => {
                    tracing::error!(
                        target = %target.name,
                        event_id = %target.event_id,
                        %error,
                        "invalid relay publish request; dropping payload"
                    );
                    break;
                }
            }
        }
    }
}

fn next_backoff(current: Duration, max: Duration) -> Duration {
    current.saturating_mul(2).min(max)
}

fn jittered_delay(base: Duration) -> Duration {
    let millis = base.as_millis();
    if millis == 0 {
        return base;
    }

    let jitter_span = (millis / 10).max(1);
    let seed = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or(0);
    let jitter = seed % (jitter_span + 1);
    let total = millis.saturating_add(jitter);
    Duration::from_millis(total.try_into().unwrap_or(u64::MAX))
}

fn reject_wrapped_payload(payload: &Value) -> Result<()> {
    if let Some(map) = payload.as_object()
        && map.len() == 2
        && map.contains_key("event_id")
        && map.contains_key("metadata")
    {
        return Err(anyhow!(
            "refusing to publish wrapped {{event_id, metadata}} relay payload"
        ));
    }
    Ok(())
}

fn validate_bearer_token(token: &str) -> Result<()> {
    if token.trim().is_empty() {
        return Err(anyhow!("relay bearer token is empty"));
    }
    if token.chars().any(|ch| ch == '\0' || ch.is_control()) {
        return Err(anyhow!("relay bearer token contains control bytes"));
    }
    Ok(())
}

fn build_url(endpoint: &str, path_segments: &[&str]) -> Result<reqwest::Url> {
    let mut url = reqwest::Url::parse(&format!("{}/", endpoint.trim_end_matches('/')))
        .with_context(|| format!("parse relay endpoint {endpoint}"))?;
    {
        let mut segments = url
            .path_segments_mut()
            .map_err(|_| anyhow!("relay endpoint cannot be a base URL: {endpoint}"))?;
        for segment in path_segments {
            if segment.trim().is_empty() {
                return Err(anyhow!("relay URL path segment must not be empty"));
            }
            segments.push(segment);
        }
    }
    Ok(url)
}
