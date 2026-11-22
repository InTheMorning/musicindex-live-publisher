use serde::{Deserialize, Serialize};

/// Holds one payment route from a MusicIndex track.
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
#[serde(default)]
pub struct PaymentRoute {
    /// The route recipient name.
    pub recipient_name: Option<String>,
    /// The route type.
    pub route_type: Option<String>,
    /// The route split.
    pub split: Option<f64>,
    /// Whether the route includes a fee.
    pub fee: Option<bool>,
    /// The route address.
    pub address: Option<String>,
    /// A custom route key.
    pub custom_key: Option<String>,
    /// A custom route value.
    pub custom_value: Option<String>,
}
