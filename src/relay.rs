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

use crate::{LiveValuePayload, ProducerState, PublisherConfig, PublisherTarget};

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
    Accepted {
        seq: u64,
        /// The relay's keepalive interval, present only when the relay's
        /// lease feature is enabled (`musicindex-live-relay` ADR 0002). A
        /// relay with no lease support omits this field, and no keepalive
        /// is ever sent for this target.
        keepalive_interval_secs: Option<u64>,
    },
    Retryable {
        reason: String,
    },
    Dropped {
        reason: String,
    },
    Fatal {
        reason: String,
    },
}

/// Result of one keepalive attempt.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum KeepaliveOutcome {
    /// The relay renewed the lease. Carries its current keepalive interval.
    Renewed {
        keepalive_interval_secs: Option<u64>,
    },
    /// The event holds no snapshot. Only a publish brings it back.
    LeaseExpired,
    Retryable {
        reason: String,
    },
    Fatal {
        reason: String,
    },
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
    #[serde(default)]
    keepalive_interval_secs: Option<u64>,
}

/// Body of a `200` keepalive response.
///
/// `event_id` and `lease_expires_at` are part of the wire contract
/// (`musicindex-live-relay` ADR 0002) but this client needs neither.
#[derive(Debug, Deserialize)]
struct KeepaliveResponse {
    #[serde(default)]
    keepalive_interval_secs: Option<u64>,
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
                Ok(PublishOutcome::Accepted {
                    seq: body.seq,
                    keepalive_interval_secs: body.keepalive_interval_secs,
                })
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

    /// Sends a keepalive for one target's live event.
    ///
    /// `musicindex-live-relay` ADR 0002 owns this route:
    /// `POST {endpoint}/v1/liveitems/{event_id}/keepalive`, with the
    /// bearer token and no request body.
    ///
    /// # Errors
    ///
    /// Returns an error when the target's token is invalid or the
    /// endpoint cannot be built, or when a `200` response body cannot be
    /// parsed.
    pub fn keepalive(&self, target: &RelayTarget) -> Result<KeepaliveOutcome> {
        validate_bearer_token(&target.token)?;
        let url = build_url(
            &target.endpoint,
            &["v1", "liveitems", &target.event_id, "keepalive"],
        )?;

        let response = match self.client.post(url).bearer_auth(&target.token).send() {
            Ok(response) => response,
            Err(error) => {
                return Ok(KeepaliveOutcome::Retryable {
                    reason: format!("network error: {error}"),
                });
            }
        };

        let status = response.status();
        match status {
            StatusCode::OK => {
                let body: KeepaliveResponse = response
                    .json()
                    .context("parse relay keepalive success response")?;
                Ok(KeepaliveOutcome::Renewed {
                    keepalive_interval_secs: body.keepalive_interval_secs,
                })
            }
            StatusCode::CONFLICT => Ok(KeepaliveOutcome::LeaseExpired),
            StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN | StatusCode::NOT_FOUND => {
                Ok(KeepaliveOutcome::Fatal {
                    reason: format!("relay returned HTTP {status}"),
                })
            }
            StatusCode::TOO_MANY_REQUESTS => Ok(KeepaliveOutcome::Retryable {
                reason: format!("relay returned HTTP {status}"),
            }),
            status if status.is_server_error() => Ok(KeepaliveOutcome::Retryable {
                reason: format!("relay returned HTTP {status}"),
            }),
            status => Ok(KeepaliveOutcome::Fatal {
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

/// A command sent from the main loop to one relay worker thread.
///
/// The channel that once carried only [`LiveValuePayload`] now carries this
/// enum, so the main loop can tell a worker about a producer liveness change
/// (ADR 0005) on the same channel a new payload arrives on.
#[derive(Debug)]
enum WorkerCommand {
    /// A live value payload to publish.
    ///
    /// Boxed so this variant does not dwarf [`WorkerCommand::Producer`] in
    /// size: `LiveValuePayload` carries several owned strings.
    Publish(Box<LiveValuePayload>),
    /// A producer liveness change.
    Producer(ProducerState),
}

/// Coordinates per-target relay workers.
#[derive(Debug)]
pub struct RelayPublisher {
    senders: HashMap<String, mpsc::Sender<WorkerCommand>>,
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
            .send(WorkerCommand::Publish(Box::new(payload)))
            .map_err(|_| anyhow!("relay publish worker for event_id {event_id} stopped"))
    }

    /// Tells every relay worker about a producer liveness change.
    ///
    /// ADR 0005 and relay-lease-task-004: `Producer(Running)` reaches every
    /// worker at once, because a producer that returns affects the whole
    /// service, not one target. The startup probe result also goes through
    /// this method, one time, before the first payload.
    ///
    /// A worker that already stopped after a fatal relay failure drops the
    /// command. `check_health` reports that failure. This method does not
    /// repeat it.
    pub fn set_producer(&self, state: ProducerState) {
        for sender in self.senders.values() {
            let _ignored = sender.send(WorkerCommand::Producer(state));
        }
    }

    /// Tells one target's relay worker about a producer liveness change.
    ///
    /// ADR 0005 and relay-lease-task-004: each target's dead block leaves
    /// `PublishSchedule` at its own time, so `Producer(Missing)` for a
    /// missing producer goes to one target at a time, only after that
    /// target's dead block is released.
    ///
    /// # Errors
    ///
    /// Returns an error when no worker exists for `event_id`, or when that
    /// worker already stopped.
    pub fn set_producer_for_target(&self, event_id: &str, state: ProducerState) -> Result<()> {
        let sender = self
            .senders
            .get(event_id)
            .ok_or_else(|| anyhow!("no relay target configured for event_id {event_id}"))?;
        sender
            .send(WorkerCommand::Producer(state))
            .map_err(|_| anyhow!("relay publish worker for event_id {event_id} stopped"))
    }
}

/// Shared, read-mostly state one worker's retry helpers need.
///
/// This groups the parameters `run_publish`, `run_keepalive`, and
/// `republish_after_lease_expiry` all take, so passing them through does not
/// grow into a long, repeated argument list.
struct WorkerContext<'a> {
    client: &'a RelayClient,
    target: &'a RelayTarget,
    receiver: &'a mpsc::Receiver<WorkerCommand>,
    initial_backoff: Duration,
    max_backoff: Duration,
    fatal: &'a Mutex<Option<String>>,
}

/// Outcome of retrying one payload until it settles.
enum PublishAttempt {
    /// The relay accepted the payload.
    ///
    /// Boxed for the same reason as [`WorkerCommand::Publish`]: this is the
    /// large variant next to `Dropped` and `Stop`, which carry no payload.
    Accepted {
        payload: Box<LiveValuePayload>,
        keepalive_interval: Option<Duration>,
    },
    /// The relay refused the payload outright. The worker keeps running.
    Dropped,
    /// A fatal relay failure, or the command channel closed. The caller
    /// must stop the worker. A relay-reported fatal outcome has already
    /// recorded the reason in `fatal` before this variant is returned.
    Stop,
}

/// Publishes one payload, retrying with backoff until it is accepted,
/// dropped, or found fatal.
///
/// A `Publish` command received while retrying supersedes the payload being
/// retried and restarts the backoff, the same as before this task. A
/// `Producer` command received while retrying updates `*producer_state` in
/// place and does not affect the retry.
fn run_publish(
    ctx: &WorkerContext<'_>,
    mut payload: LiveValuePayload,
    producer_state: &mut ProducerState,
) -> PublishAttempt {
    let target = ctx.target;
    let mut backoff = ctx.initial_backoff;
    loop {
        match ctx.client.publish(target, &payload) {
            Ok(PublishOutcome::Accepted {
                seq,
                keepalive_interval_secs,
            }) => {
                tracing::info!(
                    target = %target.name,
                    event_id = %target.event_id,
                    seq,
                    "published live value payload"
                );
                return PublishAttempt::Accepted {
                    payload: Box::new(payload),
                    keepalive_interval: keepalive_interval_secs.map(Duration::from_secs),
                };
            }
            Ok(PublishOutcome::Dropped { reason }) => {
                tracing::error!(
                    target = %target.name,
                    event_id = %target.event_id,
                    %reason,
                    "dropping live value payload"
                );
                return PublishAttempt::Dropped;
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
                if let Ok(mut fatal) = ctx.fatal.lock() {
                    *fatal = Some(format!("target {}: {reason}", target.name));
                }
                return PublishAttempt::Stop;
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
                match ctx.receiver.recv_timeout(retry_delay) {
                    Ok(WorkerCommand::Publish(newer_payload)) => {
                        payload = *newer_payload;
                        backoff = ctx.initial_backoff;
                    }
                    Ok(WorkerCommand::Producer(state)) => {
                        *producer_state = state;
                    }
                    Err(mpsc::RecvTimeoutError::Timeout) => {
                        backoff = next_backoff(backoff, ctx.max_backoff);
                    }
                    Err(mpsc::RecvTimeoutError::Disconnected) => return PublishAttempt::Stop,
                }
            }
            Err(error) => {
                tracing::error!(
                    target = %target.name,
                    event_id = %target.event_id,
                    %error,
                    "invalid relay publish request; dropping payload"
                );
                return PublishAttempt::Dropped;
            }
        }
    }
}

/// Outcome of one keepalive cycle, including a 409 republish when needed.
enum KeepaliveAttempt {
    /// The relay renewed the lease. Carries its current interval.
    Renewed { interval: Option<Duration> },
    /// The lease had expired and the republish was accepted.
    Republished {
        payload: Box<LiveValuePayload>,
        interval: Option<Duration>,
    },
    /// The lease had expired and no republish took effect: the relay
    /// dropped it, or the worker held no accepted payload to resend. No
    /// keepalive can continue until a fresh publish arrives.
    Idle,
    /// The producer went missing during a retry. The keepalive stops, and the
    /// worker keeps its interval for the time when the producer returns.
    Paused,
    /// A fatal relay failure, or the command channel closed. The caller
    /// must stop the worker.
    Stop,
}

/// Sends one keepalive, and republishes the last accepted payload on a
/// `409` (an expired lease), retrying either call with backoff.
///
/// `last_accepted` is the payload to resend on a `409`. This case should
/// never arise with an empty `last_accepted`, because a worker only learns
/// a keepalive interval from a publish response that also gives it an
/// accepted payload (see [`PublishAttempt::Accepted`] and its use in
/// `publish_worker`). If it ever does, this function only logs it and
/// leaves the worker idle. No payload exists that is safe to republish.
fn run_keepalive(
    ctx: &WorkerContext<'_>,
    producer_state: &mut ProducerState,
    last_accepted: Option<&LiveValuePayload>,
) -> KeepaliveAttempt {
    let target = ctx.target;
    let mut backoff = ctx.initial_backoff;
    loop {
        match ctx.client.keepalive(target) {
            Ok(KeepaliveOutcome::Renewed {
                keepalive_interval_secs,
            }) => {
                tracing::debug!(
                    target = %target.name,
                    event_id = %target.event_id,
                    "renewed relay lease with a keepalive"
                );
                return KeepaliveAttempt::Renewed {
                    interval: keepalive_interval_secs.map(Duration::from_secs),
                };
            }
            Ok(KeepaliveOutcome::LeaseExpired) => {
                return republish_after_lease_expiry(ctx, producer_state, last_accepted);
            }
            Ok(KeepaliveOutcome::Fatal { reason }) => {
                tracing::error!(
                    target = %target.name,
                    event_id = %target.event_id,
                    %reason,
                    "fatal relay keepalive failure; stopping publisher"
                );
                if let Ok(mut fatal) = ctx.fatal.lock() {
                    *fatal = Some(format!("target {}: {reason}", target.name));
                }
                return KeepaliveAttempt::Stop;
            }
            Ok(KeepaliveOutcome::Retryable { reason }) => {
                tracing::warn!(
                    target = %target.name,
                    event_id = %target.event_id,
                    %reason,
                    delay_ms = backoff.as_millis(),
                    "relay keepalive failed; backing off"
                );
                let retry_delay = jittered_delay(backoff);
                match ctx.receiver.recv_timeout(retry_delay) {
                    Ok(WorkerCommand::Publish(payload)) => {
                        // A new payload takes priority over a stalled
                        // keepalive retry.
                        return match run_publish(ctx, *payload, producer_state) {
                            PublishAttempt::Accepted {
                                payload,
                                keepalive_interval,
                            } => KeepaliveAttempt::Republished {
                                payload,
                                interval: keepalive_interval,
                            },
                            PublishAttempt::Dropped => KeepaliveAttempt::Idle,
                            PublishAttempt::Stop => KeepaliveAttempt::Stop,
                        };
                    }
                    Ok(WorkerCommand::Producer(state)) => {
                        *producer_state = state;
                        // ADR 0005: no keepalive goes out while the producer is
                        // missing. A retry must not keep the event on air.
                        if state == ProducerState::Missing {
                            return KeepaliveAttempt::Paused;
                        }
                        backoff = next_backoff(backoff, ctx.max_backoff);
                    }
                    Err(mpsc::RecvTimeoutError::Timeout) => {
                        backoff = next_backoff(backoff, ctx.max_backoff);
                    }
                    Err(mpsc::RecvTimeoutError::Disconnected) => return KeepaliveAttempt::Stop,
                }
            }
            Err(error) => {
                tracing::error!(
                    target = %target.name,
                    event_id = %target.event_id,
                    %error,
                    "invalid relay keepalive request"
                );
                return KeepaliveAttempt::Idle;
            }
        }
    }
}

fn republish_after_lease_expiry(
    ctx: &WorkerContext<'_>,
    producer_state: &mut ProducerState,
    last_accepted: Option<&LiveValuePayload>,
) -> KeepaliveAttempt {
    let target = ctx.target;
    let Some(payload) = last_accepted.cloned() else {
        tracing::warn!(
            target = %target.name,
            event_id = %target.event_id,
            "keepalive reported an expired lease with no accepted payload to republish"
        );
        return KeepaliveAttempt::Idle;
    };
    tracing::warn!(
        target = %target.name,
        event_id = %target.event_id,
        block_guid = %payload.block_guid,
        "relay lease expired; republishing the last accepted payload"
    );
    match run_publish(ctx, payload, producer_state) {
        PublishAttempt::Accepted {
            payload,
            keepalive_interval,
        } => KeepaliveAttempt::Republished {
            payload,
            interval: keepalive_interval,
        },
        PublishAttempt::Dropped => KeepaliveAttempt::Idle,
        PublishAttempt::Stop => KeepaliveAttempt::Stop,
    }
}

/// How long the worker may wait before it must send the next keepalive.
///
/// `None` means no timer is armed: no relay interval is known yet, or the
/// producer is not running. A relay with no lease support therefore never
/// receives a keepalive, and neither does a target whose producer is
/// missing.
fn keepalive_wait(interval: Option<Duration>, producer_state: ProducerState) -> Option<Duration> {
    match producer_state {
        ProducerState::Running => interval,
        ProducerState::Missing => None,
    }
}

fn publish_worker(
    client: RelayClient,
    target: RelayTarget,
    receiver: mpsc::Receiver<WorkerCommand>,
    initial_backoff: Duration,
    max_backoff: Duration,
    fatal: &Mutex<Option<String>>,
) {
    let ctx = WorkerContext {
        client: &client,
        target: &target,
        receiver: &receiver,
        initial_backoff,
        max_backoff,
        fatal,
    };
    let mut last_accepted: Option<LiveValuePayload> = None;
    let mut keepalive_interval: Option<Duration> = None;
    // `Producer(Running)` reaches every worker once at startup, with the
    // first probe result, before any payload (ADR 0005). Missing is the
    // safe default until that first command arrives: it holds off any
    // keepalive rather than send one with no producer confirmed live.
    let mut producer_state = ProducerState::Missing;

    loop {
        // A new payload always goes before a keepalive: waiting here with
        // `recv_timeout` lets a `Publish` or `Producer` command preempt the
        // keepalive deadline, and handling it resets that deadline, because
        // a publish renews the lease on its own.
        let command = match keepalive_wait(keepalive_interval, producer_state) {
            Some(wait) => match receiver.recv_timeout(wait) {
                Ok(command) => command,
                Err(mpsc::RecvTimeoutError::Timeout) => {
                    match run_keepalive(&ctx, &mut producer_state, last_accepted.as_ref()) {
                        KeepaliveAttempt::Renewed { interval } => {
                            keepalive_interval = interval;
                        }
                        KeepaliveAttempt::Republished { payload, interval } => {
                            last_accepted = Some(*payload);
                            keepalive_interval = interval;
                        }
                        KeepaliveAttempt::Idle => {
                            keepalive_interval = None;
                        }
                        KeepaliveAttempt::Paused => {}
                        KeepaliveAttempt::Stop => return,
                    }
                    continue;
                }
                Err(mpsc::RecvTimeoutError::Disconnected) => return,
            },
            None => match receiver.recv() {
                Ok(command) => command,
                Err(_) => return,
            },
        };

        match command {
            WorkerCommand::Producer(state) => producer_state = state,
            WorkerCommand::Publish(payload) => {
                match run_publish(&ctx, *payload, &mut producer_state) {
                    PublishAttempt::Accepted {
                        payload,
                        keepalive_interval: interval,
                    } => {
                        last_accepted = Some(*payload);
                        keepalive_interval = interval;
                    }
                    PublishAttempt::Dropped => {}
                    PublishAttempt::Stop => return,
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
