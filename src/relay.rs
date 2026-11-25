//! MusicIndex relay HTTP client and retry workers.

use std::collections::{BTreeMap, HashMap, HashSet, VecDeque};
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

use crate::livevalue::is_dead_block;
use crate::{
    Artwork, ArtworkImage, DisplayEntry, DisplayState, LiveValuePayload, Pairing, ProducerState,
    PublishResult, PublisherConfig, PublisherTarget, ShowLog, ShowLogEntry,
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

#[derive(Debug, Deserialize)]
struct MetadataSnapshot {
    metadata: SnapshotPayload,
}

#[derive(Debug, Deserialize)]
struct SnapshotPayload {
    #[serde(rename = "blockGuid")]
    block_guid: String,
    value: crate::LiveValue,
}

enum MetadataRead {
    Snapshot(MetadataSnapshot),
    NotFound,
    OtherStatus(StatusCode),
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

    fn read_metadata(&self, target: &RelayTarget) -> Result<MetadataRead> {
        let url = build_url(
            &target.endpoint,
            &["v1", "liveitems", &target.event_id, "metadata"],
        )?;
        let response = self.client.get(url).send().context("read relay metadata")?;
        let status = response.status();
        if status == StatusCode::NOT_FOUND {
            return Ok(MetadataRead::NotFound);
        }
        if status != StatusCode::OK {
            return Ok(MetadataRead::OtherStatus(status));
        }
        response
            .json::<MetadataSnapshot>()
            .context("parse relay metadata snapshot")
            .map(MetadataRead::Snapshot)
    }

    /// Publishes one display state (relay ADR 0003).
    ///
    /// `POST {endpoint}/v1/liveitems/{event_id}/display` with the bearer
    /// token. The body includes `songLine` and optionally `value` when the
    /// pairing information matches (ADR 0012). The request carries no
    /// `Listener-Delay-Secs` header (ADR 0011).
    ///
    /// # Errors
    ///
    /// Returns an error when the target's token is invalid or the endpoint
    /// cannot be built.
    pub fn publish_display(
        &self,
        target: &RelayTarget,
        state: &DisplayState,
        pairing: Option<&Pairing>,
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
            .json(&state.body_with_pairing(pairing));
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
/// (ADR 0029) on the same channel a new payload arrives on.
#[derive(Debug)]
enum WorkerCommand {
    /// A live value payload to publish.
    ///
    /// Boxed so this variant does not dwarf [`WorkerCommand::Producer`] in
    /// size: `LiveValuePayload` carries several owned strings.
    Publish(Box<LiveValuePayload>),
    /// A producer liveness change.
    Producer(ProducerState),
    /// The display worker has an image URL for one play (ADR 0034
    /// §The Payload Image Sequence, rules 2 and 3). The payload worker
    /// sends the payload of that play one more time with this `image`.
    PayloadImage { play_id: String, image: String },
}

/// Gives the payload image of a display state that the relay accepted
/// (ADR 0034 §The Payload Image Sequence, rules 2 and 3).
///
/// Rule 2 is the artwork URL of the state, when it is an `http` or `https`
/// URL. Rule 3 is the relay URL of the embedded image. `send_display` only
/// returns a state with an embedded image after the relay accepted its
/// upload. The relay URL is the form that relay ADR 0011 requires: no text
/// follows the hash. A state with no `play_id` gives no image.
fn payload_image(target: &RelayTarget, state: &DisplayState) -> Option<(String, String)> {
    let track = state.track.as_ref()?;
    let play_id = track.play_id.clone()?;
    let image = match track.artwork.as_ref()? {
        Artwork::Url(url) => reqwest::Url::parse(url)
            .ok()
            .filter(|parsed| matches!(parsed.scheme(), "http" | "https"))
            .map(|_| url.clone())?,
        Artwork::Image(image) => build_url(
            &target.endpoint,
            &[
                "v1",
                "liveitems",
                &target.event_id,
                "artwork",
                image.sha256(),
            ],
        )
        .ok()?
        .to_string(),
    };
    Some((play_id, image))
}

/// Tells if `image` is a relay URL of the event of `target` (ADR 0034
/// rule 3).
fn is_relay_copy(target: &RelayTarget, image: &str) -> bool {
    build_url(
        &target.endpoint,
        &["v1", "liveitems", &target.event_id, "artwork"],
    )
    .is_ok_and(|prefix| image.starts_with(&format!("{prefix}/")))
}

/// Tells if the payload worker sends `last` one more time with the image of
/// `play_id` (ADR 0034 §The Second Send).
///
/// `last` must be the payload of that play, with no drop file image, and
/// not the dead block. `sent_for` is the play of the last second send: the
/// second send occurs one time for each play.
fn second_send_applies(last: &LiveValuePayload, play_id: &str, sent_for: Option<&str>) -> bool {
    last.play_id.as_deref() == Some(play_id)
        && last.image.is_none()
        && !is_dead_block(&last.value)
        && sent_for != Some(play_id)
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
    // ADR 0034 rule 3: an upload is accepted with `200` or `201`.
    if status == StatusCode::OK || status == StatusCode::CREATED {
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
    /// A live value payload was published with pairing information. If a
    /// display state with matching play_id is waiting, it should be resent
    /// with the value (ADR 0012).
    UpdatePayloadPairing {
        event_id: String,
        play_id: Option<String>,
        event_guid: String,
        block_guid: String,
    },
    /// The `RelayPublisher` is dropped. The display worker stops and drops
    /// its payload senders. Each payload worker holds a display sender, so
    /// without this command no worker sees a closed channel.
    Stop,
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

    /// Starts relay workers with explicit backoff settings and no show log
    /// (ADR 0017, show log task 002).
    ///
    /// Equivalent to [`RelayPublisher::start_with_show_log`] with
    /// `show_log` of `None`: no worker ever writes a show log entry.
    ///
    /// # Errors
    ///
    /// Returns an error when the relay HTTP client cannot be initialized.
    pub fn start_with_backoff(
        config: &PublisherConfig,
        initial_backoff: Duration,
        max_backoff: Duration,
    ) -> Result<Self> {
        Self::start_with_show_log(config, initial_backoff, max_backoff, None)
    }

    /// Starts relay workers with explicit backoff settings and a show log
    /// (ADR 0017, show log task 002).
    ///
    /// Each worker gets its own clone of `show_log` and writes a `track` or
    /// `clear` entry after each publish outcome of its own payload. `None`
    /// writes no entry anywhere: this is what a dry run, or a service
    /// started with no `--state-dir`, uses.
    ///
    /// # Errors
    ///
    /// Returns an error when the relay HTTP client cannot be initialized.
    pub fn start_with_show_log(
        config: &PublisherConfig,
        initial_backoff: Duration,
        max_backoff: Duration,
        show_log: Option<Arc<ShowLog>>,
    ) -> Result<Self> {
        let client = RelayClient::new(DEFAULT_REQUEST_TIMEOUT)?;
        let mut senders = HashMap::new();
        let fatal = Arc::new(Mutex::new(None));
        // The display worker sends a payload image to the payload worker of
        // its target (ADR 0034), so the payload channels exist first.
        let mut receivers = Vec::new();
        for target in &config.targets {
            let (sender, receiver) = mpsc::channel();
            senders.insert(target.event_id.clone(), sender);
            receivers.push((target, receiver));
        }
        let display = start_display_worker(config, &client, &senders, initial_backoff, max_backoff);

        for (target, receiver) in receivers {
            let relay_target = RelayTarget::from_config(&config.endpoint, target);
            // Only a target with a display path tells the display worker
            // about a republish after a keepalive `409`.
            let display = display
                .as_ref()
                .filter(|_| target.display_dir.is_some())
                .cloned();
            let show_log = show_log.clone();
            thread::Builder::new()
                .name(format!("relay-publisher-{}", relay_target.name))
                .spawn({
                    let client = client.clone();
                    let fatal = Arc::clone(&fatal);
                    move || {
                        publish_worker(
                            PublishWorkerInputs {
                                client,
                                target: relay_target,
                                receiver,
                                initial_backoff,
                                max_backoff,
                                display,
                                show_log,
                            },
                            &fatal,
                        )
                    }
                })
                .context("spawn relay publish worker")?;
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
    /// ADR 0029 and relay-lease-task-004: `Producer(Running)` reaches every
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
    /// ADR 0029, ADR 0011 and relay-lease-task-004: the main loop sends each
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

impl Drop for RelayPublisher {
    fn drop(&mut self) {
        if let Some(display) = self.display.as_ref() {
            let _ignored = display.send(DisplayCommand::Stop);
        }
    }
}

/// Shared, read-mostly state one worker's retry helpers need.
///
/// This groups the parameters `run_publish`, `run_keepalive`, and
/// `republish_last_accepted` all take, so passing them through does not
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
    /// The show log of this worker's instance, when the service runs with
    /// `--state-dir` (ADR 0017, show log task 002). `None` writes no entry.
    show_log: Option<&'a ShowLog>,
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
///
/// Writes one show log entry after each outcome of `ctx.client.publish`
/// (ADR 0017, show log task 002): see [`record_show_log_entry`]. A
/// retryable outcome writes its own entry and loops, so a payload retried
/// twice writes two entries, one for each attempt, both with the facts of
/// this same payload: the same `observed_at` and the same `block_guid`.
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
                record_show_log_entry(
                    ctx.show_log,
                    target,
                    &payload,
                    PublishResult::Accepted,
                    Some(seq),
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
                record_show_log_entry(ctx.show_log, target, &payload, PublishResult::Dropped, None);
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
                record_show_log_entry(ctx.show_log, target, &payload, PublishResult::Fatal, None);
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
                record_show_log_entry(
                    ctx.show_log,
                    target,
                    &payload,
                    PublishResult::Retryable,
                    None,
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
                    // The payload of this play is not accepted yet, so the
                    // image goes into the next attempt (ADR 0034).
                    Ok(WorkerCommand::PayloadImage { play_id, image }) => {
                        if second_send_applies(&payload, &play_id, None) {
                            payload.image = Some(image);
                        }
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

/// Writes one show log `track` or `clear` entry after one publish attempt
/// (ADR 0017, show log task 002).
///
/// Does nothing when `show_log` is `None` (a dry run, or no `--state-dir`),
/// or when `payload` carries no [`LogFacts`](crate::LogFacts): a payload
/// that the watcher did not build from a drop file, for example a payload a
/// test builds directly, logs nothing.
///
/// `target`, `block_guid` and `value` come from `payload` and `target`.
/// `listener_delay_secs` comes from `target`, the value this worker sends
/// in `Listener-Delay-Secs`. `result` and `seq` come from the publish
/// outcome. Every other field comes from the payload's log facts.
fn record_show_log_entry(
    show_log: Option<&ShowLog>,
    target: &RelayTarget,
    payload: &LiveValuePayload,
    result: PublishResult,
    seq: Option<u64>,
) {
    let Some(show_log) = show_log else {
        return;
    };
    let Some(facts) = payload.log.as_ref() else {
        return;
    };
    let mut entry = ShowLogEntry::new(
        facts.kind,
        facts.observed_at,
        target.name.clone(),
        payload.event_guid.clone(),
    );
    entry.block_guid = Some(payload.block_guid.clone());
    entry.payable = facts.payable;
    entry.play_id = facts.play_id.clone();
    entry.artist = facts.artist.clone();
    entry.title = facts.title.clone();
    entry.album = facts.album.clone();
    entry.duration_secs = facts.duration_secs;
    entry.feed_guid = facts.feed_guid.clone();
    entry.track_guid = facts.track_guid.clone();
    entry.image_url = facts.image_url.clone();
    entry.value = Some(payload.value.clone());
    entry.listener_delay_secs = Some(target.listener_delay_secs);
    entry.publish_result = Some(result);
    entry.seq = seq;
    show_log.append(&entry);
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
    waiting: &mut VecDeque<WorkerCommand>,
) -> KeepaliveAttempt {
    let target = ctx.target;
    let mut backoff = ctx.initial_backoff;
    if *producer_state == ProducerState::Running
        && let Some(payload) = last_accepted.filter(|payload| !is_dead_block(&payload.value))
    {
        match ctx.client.read_metadata(target) {
            Ok(MetadataRead::Snapshot(snapshot)) => {
                if is_dead_block(&snapshot.metadata.value)
                    && snapshot.metadata.block_guid != payload.block_guid
                {
                    tracing::warn!(
                        target = %target.name,
                        event_id = %target.event_id,
                        relay_block_guid = %snapshot.metadata.block_guid,
                        "relay holds a dead block from another sender; restoring the last block"
                    );
                    return republish_last_accepted(
                        ctx,
                        producer_state,
                        last_accepted,
                        "a foreign dead block replaced the last block",
                    );
                }
                if !is_dead_block(&snapshot.metadata.value)
                    && snapshot.metadata.block_guid != payload.block_guid
                {
                    tracing::warn!(
                        target = %target.name,
                        event_id = %target.event_id,
                        relay_block_guid = %snapshot.metadata.block_guid,
                        "another sender has a payable block on this event"
                    );
                }
            }
            Ok(MetadataRead::NotFound) => {}
            Ok(MetadataRead::OtherStatus(status)) => tracing::debug!(
                target = %target.name,
                event_id = %target.event_id,
                %status,
                "relay metadata read returned an unexpected status"
            ),
            Err(error) => tracing::debug!(
                target = %target.name,
                event_id = %target.event_id,
                %error,
                "relay metadata read failed"
            ),
        }
    }
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
                return republish_last_accepted(
                    ctx,
                    producer_state,
                    last_accepted,
                    "the relay lease expired",
                );
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
                        // ADR 0029: no keepalive goes out while the producer is
                        // missing. A retry must not keep the event on air.
                        if state == ProducerState::Missing {
                            return KeepaliveAttempt::Paused;
                        }
                        backoff = next_backoff(backoff, ctx.max_backoff);
                    }
                    // The worker loop handles the image after the keepalive
                    // (ADR 0034 §The Second Send).
                    Ok(command @ WorkerCommand::PayloadImage { .. }) => {
                        waiting.push_back(command);
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

fn republish_last_accepted(
    ctx: &WorkerContext<'_>,
    producer_state: &mut ProducerState,
    last_accepted: Option<&LiveValuePayload>,
    reason: &'static str,
) -> KeepaliveAttempt {
    let target = ctx.target;
    let Some(mut payload) = last_accepted.cloned() else {
        tracing::warn!(
            target = %target.name,
            event_id = %target.event_id,
            reason,
            "no accepted payload to republish"
        );
        return KeepaliveAttempt::Idle;
    };
    tracing::warn!(
        target = %target.name,
        event_id = %target.event_id,
        block_guid = %payload.block_guid,
        reason,
        "republishing the last accepted payload"
    );
    // A lease end, or a foreign block, removed the images of the event in
    // the relay. The relay URL names an image that the relay no longer
    // holds, until the display resend uploads it again (ADR 0034 §The
    // Second Send).
    if payload
        .image
        .as_deref()
        .is_some_and(|image| is_relay_copy(target, image))
    {
        payload.image = None;
    }
    match run_publish(ctx, payload, producer_state) {
        PublishAttempt::Accepted {
            payload,
            keepalive_interval,
        } => {
            // A lease end clears the display state and the images in the
            // relay (relay ADR 0003). A foreign block can also replace them.
            // The send never blocks.
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

/// The owned inputs of one [`publish_worker`] thread, bundled into one
/// parameter.
///
/// `show_log` (ADR 0017, show log task 002) pushed the parameter count of
/// `publish_worker` past clippy's argument-count lint; a struct carries the
/// per-target inputs instead, next to `fatal`, which stays a separate
/// parameter because it is shared by every target's worker, not owned by
/// one of them.
struct PublishWorkerInputs {
    client: RelayClient,
    target: RelayTarget,
    receiver: mpsc::Receiver<WorkerCommand>,
    initial_backoff: Duration,
    max_backoff: Duration,
    display: Option<mpsc::Sender<DisplayCommand>>,
    show_log: Option<Arc<ShowLog>>,
}

fn publish_worker(inputs: PublishWorkerInputs, fatal: &Mutex<Option<String>>) {
    let PublishWorkerInputs {
        client,
        target,
        receiver,
        initial_backoff,
        max_backoff,
        display,
        show_log,
    } = inputs;
    let ctx = WorkerContext {
        client: &client,
        target: &target,
        receiver: &receiver,
        initial_backoff,
        max_backoff,
        fatal,
        display: display.as_ref(),
        show_log: show_log.as_deref(),
    };
    let mut last_accepted: Option<LiveValuePayload> = None;
    let mut keepalive_interval: Option<Duration> = None;
    // `Producer(Running)` reaches every worker once at startup, with the
    // first probe result, before any payload (ADR 0029). Missing is the
    // safe default until that first command arrives: it holds off any
    // keepalive rather than send one with no producer confirmed live.
    let mut producer_state = ProducerState::Missing;
    // The play of the last second send (ADR 0034 §The Second Send).
    let mut image_sent_for: Option<String> = None;
    // Commands that the second send check took from the channel. They go
    // before the channel, in their order.
    let mut waiting: VecDeque<WorkerCommand> = VecDeque::new();

    loop {
        // A new payload always goes before a keepalive: waiting here with
        // `recv_timeout` lets a `Publish` or `Producer` command preempt the
        // keepalive deadline, and handling it resets that deadline, because
        // a publish renews the lease on its own.
        let command = if let Some(command) = waiting.pop_front() {
            command
        } else {
            match keepalive_wait(keepalive_interval, producer_state) {
                Some(wait) => match receiver.recv_timeout(wait) {
                    Ok(command) => command,
                    Err(mpsc::RecvTimeoutError::Timeout) => {
                        match run_keepalive(
                            &ctx,
                            &mut producer_state,
                            last_accepted.as_ref(),
                            &mut waiting,
                        ) {
                            KeepaliveAttempt::Renewed { interval } => {
                                keepalive_interval = interval;
                            }
                            KeepaliveAttempt::Republished { payload, interval } => {
                                // A republish with no relay URL lets the
                                // display resend give a new second send.
                                if payload.image.is_none()
                                    && payload.play_id.is_some()
                                    && payload.play_id == image_sent_for
                                {
                                    image_sent_for = None;
                                }
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
            }
        };

        let attempt = match command {
            WorkerCommand::Producer(state) => {
                producer_state = state;
                continue;
            }
            WorkerCommand::Publish(payload) => run_publish(&ctx, *payload, &mut producer_state),
            WorkerCommand::PayloadImage { play_id, image } => {
                // Only while this play is the newest payload of the target:
                // a newer payload in the channel cancels the second send.
                while let Ok(command) = receiver.try_recv() {
                    waiting.push_back(command);
                }
                let newer = waiting
                    .iter()
                    .any(|command| matches!(command, WorkerCommand::Publish(_)));
                let Some(last) = last_accepted.as_ref().filter(|last| {
                    !newer && second_send_applies(last, &play_id, image_sent_for.as_deref())
                }) else {
                    continue;
                };
                tracing::info!(
                    target = %target.name,
                    event_id = %target.event_id,
                    %play_id,
                    "sending the payload of this play again with its image"
                );
                let mut second = last.clone();
                second.image = Some(image);
                // The second send adds no show log line (ADR 0034). The
                // accepted payload gets its facts back for a republish.
                let facts = second.log.take();
                image_sent_for = Some(play_id);
                match run_publish(&ctx, second, &mut producer_state) {
                    PublishAttempt::Accepted {
                        mut payload,
                        keepalive_interval,
                    } => {
                        if payload.log.is_none() && payload.play_id == last.play_id {
                            payload.log = facts;
                        }
                        PublishAttempt::Accepted {
                            payload,
                            keepalive_interval,
                        }
                    }
                    other => other,
                }
            }
        };
        match attempt {
            PublishAttempt::Accepted {
                payload,
                keepalive_interval: interval,
            } => {
                // Notify the display worker about the new payload's pairing info
                // so it can pair display states with their payloads (ADR 0012).
                if let Some(display) = ctx.display {
                    let _ignored = display.send(DisplayCommand::UpdatePayloadPairing {
                        event_id: target.event_id.clone(),
                        play_id: payload.play_id.clone(),
                        event_guid: payload.event_guid.clone(),
                        block_guid: payload.block_guid.clone(),
                    });
                }
                last_accepted = Some(*payload);
                keepalive_interval = interval;
            }
            PublishAttempt::Dropped => {}
            PublishAttempt::Stop => return,
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
    payload_senders: &HashMap<String, mpsc::Sender<WorkerCommand>>,
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
                    payload_senders.get(&target.event_id).cloned(),
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
    /// True if last_sent included value with its pairing (ADR 0012).
    ///
    /// When a payload arrives after a display state, the state is resent with
    /// value only one time. This flag ensures it happens only once.
    last_sent_paired: bool,
    /// The SHA-256 of each image that this process uploaded to this target.
    uploaded: HashSet<String>,
    /// True after `404` or `409 event_not_reserved`, until the next start.
    disabled: bool,
    backoff: Duration,
    /// The earliest time of the next attempt after a retryable failure.
    retry_at: Option<Instant>,
    /// The pairing of the newest track payload (ADR 0012): play_id, event_guid,
    /// and block_guid. None for dead blocks or tracks with no drop file.
    current_pairing: Option<Pairing>,
    /// The payload worker of this target. It gets the image of each
    /// accepted display state (ADR 0034 §The Second Send).
    payload: Option<mpsc::Sender<WorkerCommand>>,
}

impl DisplaySlot {
    fn new(
        target: RelayTarget,
        payload: Option<mpsc::Sender<WorkerCommand>>,
        initial_backoff: Duration,
    ) -> Self {
        Self {
            target,
            pending: None,
            last_sent: None,
            last_sent_paired: false,
            uploaded: HashSet::new(),
            disabled: false,
            backoff: initial_backoff,
            retry_at: None,
            current_pairing: None,
            payload,
        }
    }

    fn ready(&self, now: Instant) -> bool {
        !self.disabled && self.pending.is_some() && self.retry_at.is_none_or(|at| at <= now)
    }
}

/// How one display attempt ended.
enum DisplayStep {
    /// The relay accepted this state. It has an embedded image only when
    /// the relay also accepted the upload of that image.
    Done(DisplayState),
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
                    Ok(DisplayCommand::Stop) => return,
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
                    Ok(DisplayCommand::Stop) => return,
                    Ok(command) => command,
                    Err(mpsc::RecvTimeoutError::Timeout) => continue,
                    Err(mpsc::RecvTimeoutError::Disconnected) => return,
                },
                None => match receiver.recv() {
                    Ok(DisplayCommand::Stop) | Err(_) => return,
                    Ok(command) => command,
                },
            };
            self.apply(command);
        }
    }

    fn apply(&mut self, command: DisplayCommand) {
        match command {
            // `run` returns at `Stop` before it calls `apply`.
            DisplayCommand::Stop => {}
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
            DisplayCommand::UpdatePayloadPairing {
                event_id,
                play_id,
                event_guid,
                block_guid,
            } => {
                let Some(slot) = self.slots.get_mut(&event_id) else {
                    return;
                };

                // Update the current pairing only for payloads with play_id.
                // A dead block (play_id None) clears the pairing.
                slot.current_pairing = play_id.as_ref().map(|id| Pairing {
                    play_id: id.clone(),
                    event_guid,
                    block_guid,
                });

                // If pending is waiting (not sent yet), it will go out with the
                // new pairing on the next attempt. Do nothing.
                //
                // If pending is None but last_sent has a track with matching
                // play_id and last_sent was not already paired, resend it with
                // value. This handles the case where a display state went out
                // before its payload arrived.
                if slot.pending.is_none()
                    && !slot.last_sent_paired
                    && let Some(last_sent) = slot.last_sent.as_ref()
                    && let Some(pairing) = slot.current_pairing.as_ref()
                    && last_sent.pairs_with(pairing)
                {
                    // Resend the last state with value, one time only
                    slot.pending = Some(last_sent.clone());
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
            DisplayStep::Done(sent) => {
                tracing::info!(
                    target = %target.name,
                    event_id = %target.event_id,
                    "published display state"
                );
                // ADR 0034: the payload worker decides if the play gets a
                // second send. The send never blocks.
                if let Some(payload) = slot.payload.as_ref()
                    && let Some((play_id, image)) = payload_image(&target, &sent)
                {
                    let _ignored = payload.send(WorkerCommand::PayloadImage { play_id, image });
                }
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

        match client.publish_display(&slot.target, &state, slot.current_pairing.as_ref()) {
            Ok(DisplayOutcome::Accepted) => {
                // True only when the sent body had `value`.
                slot.last_sent_paired = slot
                    .current_pairing
                    .as_ref()
                    .is_some_and(|pairing| state.pairs_with(pairing));
                return DisplayStep::Done(state);
            }
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
