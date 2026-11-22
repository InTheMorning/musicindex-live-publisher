//! Shared drop-file and producer-lock code.

pub mod instance;
pub mod lock;
pub mod merge;
pub mod payment_route;
pub mod sink;
pub use instance::InstanceSettings;
pub use lock::{LOCK_FILE_NAME, LOCK_WAIT_LIMIT, ProducerLock, open_lock_file, wait_for_lock};
pub use payment_route::PaymentRoute;

use serde::Serialize;

/// Version 3 drop-file fields written by now-playing producers.
#[derive(Debug, Clone, Serialize)]
pub struct Version3DropFile<'a> {
    /// The drop-file schema identifier.
    pub schema: &'static str,
    /// The target name.
    pub target: &'a str,
    /// The track artist.
    pub artist: &'a str,
    /// The track title.
    pub title: &'a str,
    /// The track duration in seconds.
    pub duration_secs: Option<f64>,
    /// The track image reference.
    pub image: Option<&'a str>,
    /// The feed GUID.
    pub feed_guid: Option<&'a str>,
    /// The track GUID.
    pub track_guid: Option<&'a str>,
    /// The track album.
    pub album: Option<&'a str>,
    /// The play identifier.
    pub play_id: Option<String>,
    /// The payment routes for this track.
    pub value_routes: Vec<PaymentRoute>,
    /// The source of the payment routes.
    pub value_routes_source: Option<&'static str>,
}
