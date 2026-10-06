//! MusicIndex relay HTTP client and retry workers.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::fmt;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, mpsc};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result, anyhow};
use reqwest::StatusCode;
use reqwest::blocking::Client;
use serde::Deserialize;
use serde_json::Value;

use crate::{
    ArtworkImage, DisplayEntry, DisplayState, LiveValuePayload, ProducerState, PublisherConfig,
    PublisherTarget,
};

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
    /// The stream delay of this target, rounded to the nearest second.
    ///
    /// `publish_value` sends this value in the `Listener-Delay-Secs`
    /// header of each live value publish. ADR 0011 owns this header.
    pub listener_delay_secs: u64,
}

impl fmt::Debug for RelayTarget {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("RelayTarget")
            .field("name", &self.name)
            .field("endpoint", &self.endpoint)
            .field("event_id", &self.event_id)
            .field("token", &"<redacted>")
            .field("listener_delay_secs", &self.listener_delay_secs)
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
            listener_delay_secs: round_listener_delay_secs(target.stream_delay),
        }
    }
}

/// The name of the header that carries the rounded stream delay.
///
/// ADR 0011 and `musicindex-live-relay` ADR 0004 own this header. Only a
/// live value publish carries it. A keepalive, a display request, and an
/// artwork upload carry no header.
const LISTENER_DELAY_HEADER: &str = "Listener-Delay-Secs";

/// Rounds a stream delay to the nearest second for the header value.
///
/// A half second rounds up (ADR 0011 §Send At Once).
fn round_listener_delay_secs(stream_delay: Duration) -> u64 {
    let half_up_millis = stream_delay.as_millis().saturating_add(500);
    u64::try_from(half_up_millis / 1000).unwrap_or(u64::MAX)
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

/// Result of one display publish or one image upload (relay ADR 0003).
///
/// No outcome is fatal (ADR 0008). A display failure never changes the
/// payment path.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DisplayOutcome {
    /// The relay accepted the request.
    Accepted,
    /// `409 artwork_missing`: the relay does not hold the named image.
    ArtworkMissing,
    /// `404` or `409 event_not_reserved`: the event has no display path.
    Disabled { reason: String },
    /// A network error, a `5xx` or a `429`.
    Retryable { reason: String },
    /// Any other answer. The worker logs it and drops the state.
    Refused { reason: String },
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
    /// The request carries the header `Listener-Delay-Secs` with the
    /// rounded stream delay of `target` (ADR 0011).
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
    /// rejection without changing task 002's live value structs. The request
    /// carries the header `Listener-Delay-Secs` with the rounded stream
    /// delay of `target` (ADR 0011).
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
            .header(
                LISTENER_DELAY_HEADER,
                target.listener_delay_secs.to_string(),
            )
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
    /// bearer token and no request body. The request carries no
    /// `Listener-Delay-Secs` header (ADR 0011).
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

    /// Publishes one display state (relay ADR 0003).
    ///
    /// `POST {endpoint}/v1/liveitems/{event_id}/display` with the bearer
    /// token. The body is [`DisplayState::body`], which holds only `track`.
    /// The request carries no `Listener-Delay-Secs` header (ADR 0011).
    ///
    /// # Errors
    ///
    /// Returns an error when the target's token is invalid or the endpoint
    /// cannot be built.
    pub fn publish_display(
        &self,
        target: &RelayTarget,
        state: &DisplayState,
    ) -> Result<DisplayOutcome> {
        validate_bearer_token(&target.token)?;
        let url = build_url(
            &target.endpoint,
            &["v1", "liveitems", &target.event_id, "display"],
        )?;
        let request = self
            .client
            .post(url)
            .bearer_auth(&target.token)
            .json(&state.body());
        Ok(display_outcome(request.send()))
    }

    /// Uploads one image (relay ADR 0003).
    ///
    /// `PUT {endpoint}/v1/liveitems/{event_id}/artwork/{sha256}` with the
    /// bearer token. The body is the image bytes. The request carries no
    /// `Listener-Delay-Secs` header (ADR 0011).
    ///
    /// # Errors
    ///
    /// Returns an error when the target's token is invalid or the endpoint
    /// cannot be built.
    pub fn upload_artwork(
        &self,
        target: &RelayTarget,
        image: &ArtworkImage,
    ) -> Result<DisplayOutcome> {
        validate_bearer_token(&target.token)?;
        let url = build_url(
            &target.endpoint,
            &[
                "v1",
                "liveitems",
                &target.event_id,
                "artwork",
                image.sha256(),
            ],
        )?;
        let request = self
            .client
            .put(url)
            .bearer_auth(&target.token)
            .header(reqwest::header::CONTENT_TYPE, image.mime().as_str())
            .body(image.bytes().to_vec());
        Ok(display_outcome(request.send()))
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

/// Counts temporary token files so that each one in a process has its own name.
static TOKEN_TEMP_COUNTER: AtomicU64 = AtomicU64::new(0);

/// Writes a new broadcaster token file and never replaces an existing path.
///
/// The token goes to a new temporary file with mode `0600` in the same
/// directory. The function then hard links that file to `path` and removes the
/// temporary name. A hard link fails when `path` exists, so an existing token
/// file, directory or symbolic link stays as it is.
///
/// # Errors
///
/// Returns an error when the temporary file cannot be created or written, or
/// when the link to `path` fails. After the temporary file holds the token, the
/// function keeps it and the error names it, because it can be the only copy
/// of a one-time token.
pub fn write_token_file(path: &Path, token: &str) -> Result<()> {
    let file_name = path
        .file_name()
        .ok_or_else(|| anyhow!("token file {} has no file name", path.display()))?;
    let directory = match path.parent() {
        Some(parent) if !parent.as_os_str().is_empty() => parent,
        _ => Path::new("."),
    };
    let counter = TOKEN_TEMP_COUNTER.fetch_add(1, Ordering::Relaxed);
    let mut temp_name = std::ffi::OsString::from(".");
    temp_name.push(file_name);
    temp_name.push(format!(".tmp.{}.{counter}", std::process::id()));
    let temp_path = directory.join(temp_name);

    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options
        .open(&temp_path)
        .with_context(|| format!("create temporary token file {}", temp_path.display()))?;

    let keep_temp = |action: &str| {
        format!(
            "{action}; the token stays in the temporary token file {}",
            temp_path.display()
        )
    };
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        file.set_permissions(fs::Permissions::from_mode(0o600))
            .with_context(|| keep_temp("set temporary token file permissions"))?;
    }
    file.write_all(token.as_bytes())
        .and_then(|()| file.write_all(b"\n"))
        .with_context(|| keep_temp("write temporary token file"))?;
    file.sync_all()
        .with_context(|| keep_temp("sync temporary token file"))?;
    drop(file);

    fs::hard_link(&temp_path, path).with_context(|| {
        keep_temp(&format!(
            "link token file {} without replacing an existing path",
            path.display()
        ))
    })?;

    if let Err(error) = fs::remove_file(&temp_path) {
        tracing::warn!(
            temp_file = %temp_path.display(),
            token_file = %path.display(),
            %error,
            "token file written, but the temporary token file was not removed"
        );
    }
    if let Ok(directory) = fs::File::open(directory) {
        let _ignored = directory.sync_all();
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

/// Maps one display answer to its outcome.
fn display_outcome(
    response: std::result::Result<reqwest::blocking::Response, reqwest::Error>,
) -> DisplayOutcome {
    let response = match response {
        Ok(response) => response,
        Err(error) => {
            return DisplayOutcome::Retryable {
                reason: format!("network error: {error}"),
            };
        }
    };
    let status = response.status();
    if status == StatusCode::OK {
        return DisplayOutcome::Accepted;
    }
    let reason = format!("relay returned HTTP {status}");
    if status == StatusCode::NOT_FOUND {
        return DisplayOutcome::Disabled { reason };
    }
    if status == StatusCode::TOO_MANY_REQUESTS || status.is_server_error() {
        return DisplayOutcome::Retryable { reason };
    }
    if status == StatusCode::CONFLICT {
        let code = response
            .json::<Value>()
            .ok()
            .and_then(|body| body.get("error").and_then(Value::as_str).map(str::to_owned));
        return match code.as_deref() {
            Some("artwork_missing") => DisplayOutcome::ArtworkMissing,
            Some("event_not_reserved") => DisplayOutcome::Disabled {
                reason: format!("{reason} event_not_reserved"),
            },
            _ => DisplayOutcome::Refused { reason },
        };
    }
    DisplayOutcome::Refused { reason }
}

/// A command for the display worker (ADR 0008).
#[derive(Debug)]
enum DisplayCommand {
    /// A display state, ready to publish at once (ADR 0011).
    Publish(DisplayEntry),
    /// A keepalive got `409` and the payload worker sent its last payload
    /// again. The lease end cleared the display state and the images in the
    /// relay, so the display worker sends its latest state again.
    Resend { event_id: String },
}

/// Coordinates per-target relay workers.
#[derive(Debug)]
pub struct RelayPublisher {
    senders: HashMap<String, mpsc::Sender<WorkerCommand>>,
    fatal: Arc<Mutex<Option<String>>>,
    /// The channel of the display worker. `None` when no target has a
    /// display path.
    display: Option<mpsc::Sender<DisplayCommand>>,
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
        let display = start_display_worker(config, &client, initial_backoff, max_backoff);

        for target in &config.targets {
            let relay_target = RelayTarget::from_config(&config.endpoint, target);
            let (sender, receiver) = mpsc::channel();
            // Only a target with a display path tells the display worker
            // about a republish after a keepalive `409`.
            let display = display
                .as_ref()
                .filter(|_| target.display_dir.is_some())
                .cloned();
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
                            display,
                        )
                    }
                })
                .context("spawn relay publish worker")?;
            senders.insert(target.event_id.clone(), sender);
        }

        Ok(Self {
            senders,
            fatal,
            display,
        })
    }

    /// Queues one display state for the display worker (ADR 0008).
    ///
    /// This call never blocks and never fails. A display state for a target
    /// with no display path, or a display worker that stopped, gives a
    /// warning. No display failure is fatal.
    pub fn publish_display(&self, entry: DisplayEntry) {
        let Some(display) = self.display.as_ref() else {
            tracing::warn!(
                event_id = %entry.event_id,
                "no display worker runs; dropping the display state"
            );
            return;
        };
        if display.send(DisplayCommand::Publish(entry)).is_err() {
            tracing::warn!("the display worker stopped; dropping the display state");
        }
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
    /// ADR 0005, ADR 0011 and relay-lease-task-004: the main loop sends each
    /// target's dead block on its own, so `Producer(Missing)` for a missing
    /// producer goes to one target at a time, only after that target's dead
    /// block is sent.
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
    /// The display worker, for a target with a display path. The payload
    /// worker only sends it a command. It never sends a display request.
    display: Option<&'a mpsc::Sender<DisplayCommand>>,
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
        } => {
            // The lease end cleared the display state and the images in the
            // relay (relay ADR 0003). The send never blocks.
            if let Some(display) = ctx.display {
                let _ignored = display.send(DisplayCommand::Resend {
                    event_id: target.event_id.clone(),
                });
            }
            KeepaliveAttempt::Republished {
                payload,
                interval: keepalive_interval,
            }
        }
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
    display: Option<mpsc::Sender<DisplayCommand>>,
) {
    let ctx = WorkerContext {
        client: &client,
        target: &target,
        receiver: &receiver,
        initial_backoff,
        max_backoff,
        fatal,
        display: display.as_ref(),
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

/// Starts the one display worker when a target has a display path.
///
/// A spawn failure gives a warning and no display path. It does not stop the
/// payment path.
fn start_display_worker(
    config: &PublisherConfig,
    client: &RelayClient,
    initial_backoff: Duration,
    max_backoff: Duration,
) -> Option<mpsc::Sender<DisplayCommand>> {
    let slots: BTreeMap<String, DisplaySlot> = config
        .targets
        .iter()
        .filter(|target| target.display_dir.is_some())
        .map(|target| {
            (
                target.event_id.clone(),
                DisplaySlot::new(
                    RelayTarget::from_config(&config.endpoint, target),
                    initial_backoff,
                ),
            )
        })
        .collect();
    if slots.is_empty() {
        return None;
    }
    let (sender, receiver) = mpsc::channel();
    let worker = DisplayWorker {
        client: client.clone(),
        slots,
        initial_backoff,
        max_backoff,
    };
    match thread::Builder::new()
        .name("relay-display".to_owned())
        .spawn(move || worker.run(&receiver))
    {
        Ok(_handle) => Some(sender),
        Err(error) => {
            tracing::warn!(%error, "cannot start the display worker; the display path is off");
            None
        }
    }
}

/// The display state of one target in the display worker.
#[derive(Debug)]
struct DisplaySlot {
    /// The relay target. Its `Debug` output redacts the token.
    target: RelayTarget,
    /// The latest state that waits. A newer state replaces it.
    pending: Option<DisplayState>,
    /// The last state that the relay accepted, for a resend.
    last_sent: Option<DisplayState>,
    /// The SHA-256 of each image that this process uploaded to this target.
    uploaded: HashSet<String>,
    /// True after `404` or `409 event_not_reserved`, until the next start.
    disabled: bool,
    backoff: Duration,
    /// The earliest time of the next attempt after a retryable failure.
    retry_at: Option<Instant>,
}

impl DisplaySlot {
    fn new(target: RelayTarget, initial_backoff: Duration) -> Self {
        Self {
            target,
            pending: None,
            last_sent: None,
            uploaded: HashSet::new(),
            disabled: false,
            backoff: initial_backoff,
            retry_at: None,
        }
    }

    fn ready(&self, now: Instant) -> bool {
        !self.disabled && self.pending.is_some() && self.retry_at.is_none_or(|at| at <= now)
    }
}

/// How one display attempt ended.
enum DisplayStep {
    Done,
    Retry(String),
    Disable(String),
    Drop(String),
}

/// The one display worker thread for every target (ADR 0008).
///
/// It sends display requests only. A payload worker never waits for it, and
/// it never waits for a payload worker.
struct DisplayWorker {
    client: RelayClient,
    slots: BTreeMap<String, DisplaySlot>,
    initial_backoff: Duration,
    max_backoff: Duration,
}

impl DisplayWorker {
    fn run(mut self, receiver: &mpsc::Receiver<DisplayCommand>) {
        loop {
            // Apply every waiting command first, so an attempt always sends
            // the latest state of its target.
            loop {
                match receiver.try_recv() {
                    Ok(command) => self.apply(command),
                    Err(mpsc::TryRecvError::Empty) => break,
                    Err(mpsc::TryRecvError::Disconnected) => return,
                }
            }

            let now = Instant::now();
            let ready = self
                .slots
                .iter()
                .find(|(_, slot)| slot.ready(now))
                .map(|(event_id, _)| event_id.clone());
            if let Some(event_id) = ready {
                self.attempt(&event_id);
                continue;
            }

            let next_retry = self
                .slots
                .values()
                .filter(|slot| !slot.disabled && slot.pending.is_some())
                .filter_map(|slot| slot.retry_at)
                .min();
            let command = match next_retry {
                Some(at) => match receiver.recv_timeout(at.saturating_duration_since(now)) {
                    Ok(command) => command,
                    Err(mpsc::RecvTimeoutError::Timeout) => continue,
                    Err(mpsc::RecvTimeoutError::Disconnected) => return,
                },
                None => match receiver.recv() {
                    Ok(command) => command,
                    Err(_) => return,
                },
            };
            self.apply(command);
        }
    }

    fn apply(&mut self, command: DisplayCommand) {
        match command {
            DisplayCommand::Publish(entry) => {
                let Some(slot) = self.slots.get_mut(&entry.event_id) else {
                    tracing::warn!(
                        event_id = %entry.event_id,
                        "no display path for this event; dropping the display state"
                    );
                    return;
                };
                if slot.disabled {
                    tracing::debug!(
                        target = %slot.target.name,
                        "display path is off; dropping the display state"
                    );
                    return;
                }
                // Only the latest state waits. A retry time stays, so a new
                // state does not make the worker send faster during an
                // outage.
                slot.pending = Some(entry.state);
            }
            DisplayCommand::Resend { event_id } => {
                let Some(slot) = self.slots.get_mut(&event_id) else {
                    return;
                };
                if slot.disabled {
                    return;
                }
                tracing::info!(
                    target = %slot.target.name,
                    event_id = %slot.target.event_id,
                    "relay lease was renewed by a republish; sending the display state again"
                );
                // The lease end removed every image of the event.
                slot.uploaded.clear();
                if slot.pending.is_none() {
                    slot.pending = slot.last_sent.clone();
                }
            }
        }
    }

    fn attempt(&mut self, event_id: &str) {
        let initial_backoff = self.initial_backoff;
        let max_backoff = self.max_backoff;
        let Some(slot) = self.slots.get_mut(event_id) else {
            return;
        };
        let Some(state) = slot.pending.clone() else {
            return;
        };
        let target = slot.target.clone();
        match send_display(&self.client, slot, &state) {
            DisplayStep::Done => {
                tracing::info!(
                    target = %target.name,
                    event_id = %target.event_id,
                    "published display state"
                );
                slot.pending = None;
                slot.last_sent = Some(state);
                slot.retry_at = None;
                slot.backoff = initial_backoff;
            }
            DisplayStep::Retry(reason) => {
                let delay = jittered_delay(slot.backoff).min(max_backoff);
                tracing::warn!(
                    target = %target.name,
                    event_id = %target.event_id,
                    %reason,
                    delay_ms = delay.as_millis(),
                    "display request failed; backing off"
                );
                slot.retry_at = Some(Instant::now() + delay);
                slot.backoff = next_backoff(slot.backoff, max_backoff);
            }
            DisplayStep::Disable(reason) => {
                tracing::warn!(
                    target = %target.name,
                    event_id = %target.event_id,
                    %reason,
                    "the relay has no display path for this event; display path off until the next start"
                );
                slot.disabled = true;
                slot.pending = None;
                slot.retry_at = None;
            }
            DisplayStep::Drop(reason) => {
                tracing::warn!(
                    target = %target.name,
                    event_id = %target.event_id,
                    %reason,
                    "relay refused the display state; dropping it"
                );
                slot.pending = None;
                slot.retry_at = None;
                slot.backoff = initial_backoff;
            }
        }
    }
}

/// Uploads the image of `state` when needed, then publishes `state`.
///
/// After `409 artwork_missing`, it uploads the image again and publishes
/// again, one time.
fn send_display(client: &RelayClient, slot: &mut DisplaySlot, state: &DisplayState) -> DisplayStep {
    let mut state = state.clone();
    let mut missing_retried = false;
    loop {
        if let Some(image) = state.image().cloned()
            && !slot.uploaded.contains(image.sha256())
        {
            match client.upload_artwork(&slot.target, &image) {
                Ok(DisplayOutcome::Accepted) => {
                    slot.uploaded.insert(image.sha256().to_owned());
                }
                Ok(DisplayOutcome::Retryable { reason }) => return DisplayStep::Retry(reason),
                Ok(DisplayOutcome::Disabled { reason }) => return DisplayStep::Disable(reason),
                Ok(DisplayOutcome::ArtworkMissing) => {
                    return DisplayStep::Drop("unexpected artwork_missing on upload".to_owned());
                }
                Ok(DisplayOutcome::Refused { reason }) => {
                    tracing::warn!(
                        target = %slot.target.name,
                        event_id = %slot.target.event_id,
                        %reason,
                        "relay refused the image; publishing the display state with artwork null"
                    );
                    state = state.without_image();
                }
                Err(error) => return DisplayStep::Drop(format!("{error:#}")),
            }
        }

        match client.publish_display(&slot.target, &state) {
            Ok(DisplayOutcome::Accepted) => return DisplayStep::Done,
            Ok(DisplayOutcome::ArtworkMissing) => {
                let Some(image) = state.image() else {
                    return DisplayStep::Drop(
                        "artwork_missing for a state with no image".to_owned(),
                    );
                };
                if missing_retried {
                    return DisplayStep::Drop("artwork_missing after a new upload".to_owned());
                }
                missing_retried = true;
                slot.uploaded.remove(image.sha256());
            }
            Ok(DisplayOutcome::Retryable { reason }) => return DisplayStep::Retry(reason),
            Ok(DisplayOutcome::Disabled { reason }) => return DisplayStep::Disable(reason),
            Ok(DisplayOutcome::Refused { reason }) => return DisplayStep::Drop(reason),
            Err(error) => return DisplayStep::Drop(format!("{error:#}")),
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
