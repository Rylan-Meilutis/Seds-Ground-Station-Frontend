//! Wording for a failed request for this device's own location.
/// Why a request for this device's own location failed. Every platform maps
/// to these so the operator sees the same explanation everywhere.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LocationFailure {
    /// The operator or the operating system refused location access.
    Denied,
    /// Location access is allowed but the device has no position yet.
    Timeout,
    /// Location services are off or the device has no location source.
    Unavailable,
    /// Browsers only share location with https:// pages.
    Insecure,
    /// The browser or WebView has no geolocation API.
    Unsupported,
}
impl LocationFailure {
    pub fn message(self) -> &'static str {
        match self {
            Self::Denied => {
                "Location permission for this device was denied. Allow location for this page or app in its settings, or enter coordinates."
            }
            Self::Timeout => {
                "This device has no location fix of its own (common on desktops and indoors). The map's station markers come from the station's GPS: use “Use live station GPS”, or enter coordinates."
            }
            Self::Unavailable => {
                "This device's location service is off or has no location source. Turn on location services, use “Use live station GPS”, or enter coordinates."
            }
            Self::Insecure => {
                "This device's location needs a secure page. Open this station over https://, use “Use live station GPS”, or enter coordinates."
            }
            Self::Unsupported => {
                "This device cannot share its location. Use “Use live station GPS” or enter coordinates."
            }
        }
    }
}
