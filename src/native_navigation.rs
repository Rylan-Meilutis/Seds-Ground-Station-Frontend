//! Explicit station documents may navigate in native iframes, never arbitrary sites.
use std::sync::Mutex;

/// The station the dashboard is currently showing. A station selected for this
/// session only (not saved) is never persisted, so the WebView navigation
/// handler cannot learn it from storage; the media surface records it here
/// before it mounts its iframes.
static LIVE_STATION: Mutex<String> = Mutex::new(String::new());

pub(crate) fn remember_live_station(base: &str) {
    if let Ok(mut live) = LIVE_STATION.lock()
        && *live != base
    {
        *live = base.to_string();
    }
}

/// Allows a station document for the saved station or the live station.
pub(crate) fn station_document_allowed_for_known_stations(target: &str, persisted: &str) -> bool {
    if station_document_allowed(target, persisted) {
        return true;
    }
    let live = LIVE_STATION.lock().map(|live| live.clone()).unwrap_or_default();
    !live.is_empty() && station_document_allowed(target, &live)
}

pub(crate) fn station_document_allowed(target: &str, station: &str) -> bool {
    // URL parsing normalizes dot segments and backslashes. Accept canonical
    // document paths only, so a rejected API path cannot normalize into one.
    // Queries (including opaque media tickets) are not part of this check.
    let document = target.split(['?', '#']).next().unwrap_or(target);
    if document.contains('\\')
        || document.split('/').any(|segment| {
            segment == "." || segment == ".." || segment.contains('%')
        })
    {
        return false;
    }
    let (Ok(target), Ok(station)) = (url::Url::parse(target), url::Url::parse(station)) else {
        return false;
    };
    matches!(target.scheme(), "http" | "https")
        && target.origin() == station.origin()
        && target.username().is_empty()
        && target.password().is_none()
        && (matches!(
            target.path(),
            "/program" | "/media" | "/radio" | "/player" | "/studio-audio" | "/recorded-vehicle"
                | "/api/media-assets/program" | "/api/media-assets/recorded-vehicle"
        ) || target.path().strip_prefix("/api/media-assets/streams/").is_some_and(|id| {
            // Same grammar as the backend's media::valid_id. No API subtree,
            // extra path segment or percent-encoded separator is a document.
            !id.is_empty() && id.len() <= 64
                && id.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
        }))
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn unsaved_live_station_media_documents_stay_in_app() {
        // The saved address falls back to localhost when the operator connected
        // without saving; the live station's voice and camera pages must load.
        let persisted = "http://localhost:3000";
        let live = "http://gs-live-session.invalid:3000";
        let radio = format!("{live}/radio?embedded=1");
        remember_live_station(live);
        for path in ["/radio?embedded=1", "/media?embedded=1", "/program", "/player"] {
            assert!(station_document_allowed_for_known_stations(&format!("{live}{path}"), persisted), "{path}");
        }
        assert!(station_document_allowed_for_known_stations("http://localhost:3000/media", persisted));
        assert!(!station_document_allowed_for_known_stations("http://other.invalid:3000/radio", persisted));
        assert!(!station_document_allowed_for_known_stations(&format!("{live}/api/auth"), persisted));
        remember_live_station("");
        assert!(!station_document_allowed_for_known_stations(&radio, persisted));
    }
    #[test]
    fn media_document_routes_reject_api_subpaths_and_normalization_bypasses() {
        let base = "https://gs.local:3443";
        for path in [
            "/api/media-assets/program/state", "/api/media-assets/program/audio",
            "/api/media-assets/recorded-vehicle/control", "/api/media-assets/recorded-vehicle/window",
            "/api/media-assets/streams/", "/api/media-assets/streams/pad/state",
            "/api/media-assets/streams/pad%2fother", "/api/media-assets/streams/pad%5cother",
            "/api/control/../../program", "/api/%2e%2e/media-assets/program",
            "/api/media-assets/streams/../program", "/api/media-assets/streams/pad.name",
            "/api/media-assets/streams/pad\\name",
        ] {
            assert!(!station_document_allowed(&format!("{base}{path}"), base), "{path}");
        }
        assert!(!station_document_allowed(&format!("{base}/api/media-assets/streams/{}", "x".repeat(65)), base));
        assert!(station_document_allowed(&format!("{base}/api/media-assets/streams/{}?ticket=EXAMPLE", "x".repeat(64)), base));
        assert!(!station_document_allowed("https://other.local:3443/api/media-assets/program", base));
    }

    #[test]
    fn only_exact_station_media_documents_stay_in_app() {
        for path in [
            "/program?ticket=opaque",
            "/media?embedded=1",
            "/radio?embedded=1",
            "/player",
        ] {
            assert!(station_document_allowed(
                &format!("https://gs.local:3443{path}"),
                "https://gs.local:3443"
            ));
        }
        for url in [
            "https://gs.local.evil.test:3443/program",
            "http://gs.local:3443/program",
            "https://gs.local/program",
            "https://evil@gs.local:3443/program",
            "https://gs.local:3443/api/auth",
            "https://gs.local:3443/program/other",
            "javascript:alert(1)",
        ] {
            assert!(
                !station_document_allowed(url, "https://gs.local:3443"),
                "{url}"
            );
        }
    }
}
