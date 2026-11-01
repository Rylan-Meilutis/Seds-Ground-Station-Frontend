//! Explicit station documents may navigate in native iframes, never arbitrary sites.
pub(crate) fn station_document_allowed(target: &str, station: &str) -> bool {
    let (Ok(target), Ok(station)) = (url::Url::parse(target), url::Url::parse(station)) else {
        return false;
    };
    matches!(target.scheme(), "http" | "https")
        && target.origin() == station.origin()
        && target.username().is_empty()
        && target.password().is_none()
        && matches!(
            target.path(),
            "/program" | "/media" | "/radio" | "/player" | "/studio-audio" | "/recorded-vehicle"
        )
}
#[cfg(test)]
mod tests {
    use super::*;
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
