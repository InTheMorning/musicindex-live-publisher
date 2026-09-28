//! MusicIndex live publisher primitives.
//!
//! This crate starts with the now-playing drop-file contract. Later tasks add
//! transforms, watching, configuration, and publishing.

pub mod config;
pub mod dropfile;
pub mod liveness;
pub mod livevalue;
pub mod relay;
pub mod schedule;
pub mod watcher;

pub use config::{
    ConfigEditError, ConfigOverrides, DEFAULT_CONFIG_PATH, PublisherConfig, PublisherTarget,
    RedactedPublisherConfig, RedactedPublisherTarget, TargetConfigEdit, TargetConfigSummary,
    add_target_to_config, list_config_targets, list_config_targets_from_str, load_config,
    load_config_bytes, remove_target_from_config, show_config, show_config_from_str,
};
pub use dropfile::{DropFile, PaymentRoute, SCHEMA_VERSION, parse};
pub use liveness::{LOCK_FILE_NAME, ProducerState, probe_producer};
pub use livevalue::{
    LiveValue, LiveValueDestination, LiveValueModel, LiveValuePayload, dead_payload,
    destination_from_payment_route, format_split, payload_from_dropfile,
};
pub use relay::{
    DEFAULT_INITIAL_BACKOFF, DEFAULT_MAX_BACKOFF, DEFAULT_REQUEST_TIMEOUT, ProvisionedLiveItem,
    PublishOutcome, RelayClient, RelayPublisher, RelayTarget, write_token_file,
};
pub use schedule::PublishSchedule;
pub use watcher::{
    DEFAULT_DEBOUNCE_WINDOW, DropEvent, DropEventKind, DropWatcher, WatchTarget, is_final_drop_file,
};
