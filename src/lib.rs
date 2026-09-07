//! MusicIndex live publisher primitives.
//!
//! This crate starts with the now-playing drop-file contract. Later tasks add
//! transforms, watching, configuration, and publishing.

pub mod config;
pub mod dropfile;
pub mod livevalue;
pub mod relay;
pub mod schedule;
pub mod watcher;

pub use config::{
    ConfigEditError, ConfigOverrides, DEFAULT_CONFIG_PATH, PublisherConfig, PublisherTarget,
    TargetConfigEdit, TargetConfigSummary, add_target_to_config, list_config_targets,
    list_config_targets_from_str, load_config, load_config_bytes, remove_target_from_config,
};
pub use dropfile::{DropFile, PaymentRoute, SCHEMA_VERSION, parse};
pub use livevalue::{
    LiveValue, LiveValueDestination, LiveValueModel, LiveValuePayload,
    destination_from_payment_route, fallback_payload, format_split, payload_from_dropfile,
};
pub use relay::{
    DEFAULT_INITIAL_BACKOFF, DEFAULT_MAX_BACKOFF, DEFAULT_REQUEST_TIMEOUT, ProvisionedLiveItem,
    PublishOutcome, RelayClient, RelayPublisher, RelayTarget, write_token_file,
};
pub use schedule::PublishSchedule;
pub use watcher::{
    DEFAULT_DEBOUNCE_WINDOW, DropEvent, DropEventKind, DropWatcher, FallbackConfig, WatchTarget,
    is_final_drop_file,
};
