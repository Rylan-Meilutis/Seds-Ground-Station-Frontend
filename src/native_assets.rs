//! Bundled renderer assets, served from the native document's own origin.
#[cfg(not(target_arch = "wasm32"))]
pub(crate) fn use_station_assets() {
    dioxus_desktop::use_asset_handler("station-assets", |request, responder| {
        let asset = station_document_asset(request.uri().path());
        let (status, mime, bytes) = match asset {
            Some((mime, bytes)) => (200, mime, bytes),
            None => (404, "text/plain; charset=utf-8", &b"Asset not found"[..]),
        };
        let response = dioxus_desktop::wry::http::Response::builder()
            .status(status)
            .header("Content-Type", mime)
            .header("X-Content-Type-Options", "nosniff")
            .body(bytes.to_vec())
            .expect("static native asset response");
        responder.respond(response);
    });
}

#[cfg(not(target_arch = "wasm32"))]
fn station_document_asset(path: &str) -> Option<(&'static str, &'static [u8])> {
    let path = path.strip_prefix("/station-assets/")?;
    station_asset(&format!("/assets/{path}"))
}

#[cfg(not(target_arch = "wasm32"))]
pub(crate) fn station_asset(path: &str) -> Option<(&'static str, &'static [u8])> {
    match path {
        "/assets/three/BufferGeometryUtils.js" => Some((
            "text/javascript; charset=utf-8",
            include_bytes!("../station_ui/rendering/three/BufferGeometryUtils.js"),
        )),
        "/assets/three/GLTFLoader.js" => Some((
            "text/javascript; charset=utf-8",
            include_bytes!("../station_ui/rendering/three/GLTFLoader.js"),
        )),
        "/assets/three/replay-scene.js" => Some((
            "text/javascript; charset=utf-8",
            include_bytes!("../station_ui/rendering/three/replay-scene.js"),
        )),
        "/assets/three/saved-replay-player.js" => Some((
            "text/javascript; charset=utf-8",
            include_bytes!("../station_ui/rendering/three/saved-replay-player.js"),
        )),
        "/assets/three/software-renderer.js" => Some((
            "text/javascript; charset=utf-8",
            include_bytes!("../station_ui/rendering/three/software-renderer.js"),
        )),
        "/assets/three/three.core.js" => Some((
            "text/javascript; charset=utf-8",
            include_bytes!("../station_ui/rendering/three/three.core.js"),
        )),
        "/assets/three/three.module.js" => Some((
            "text/javascript; charset=utf-8",
            include_bytes!("../station_ui/rendering/three/three.module.js"),
        )),
        "/assets/three/vehicle-renderer.js" => Some((
            "text/javascript; charset=utf-8",
            include_bytes!("../station_ui/rendering/three/vehicle-renderer.js"),
        )),
        "/assets/models/gse-site.glb" => Some((
            "model/gltf-binary",
            include_bytes!("../station_ui/rendering/models/gse-site.glb"),
        )),
        "/assets/models/vehicle.glb" => Some((
            "model/gltf-binary",
            include_bytes!("../station_ui/rendering/models/vehicle.glb"),
        )),
        _ => None,
    }
}

pub(crate) fn station_asset_url(path: &str) -> Option<String> {
    #[cfg(not(target_arch = "wasm32"))]
    {
        station_asset(path)?;
        // Relative imports and GLTF requests stay on Dioxus's document origin.
        // A separate custom scheme requires WebKit-specific Fetch/CORS support
        // even when a protocol response carries permissive CORS headers.
        Some(format!("/station-assets/{}", path.strip_prefix("/assets/")?))
    }
    #[cfg(target_arch = "wasm32")]
    {
        let _ = path;
        None
    }
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    use super::*;
    #[test]
    fn native_renderer_and_relative_dependencies_are_bundled() {
        for path in [
            "/assets/three/vehicle-renderer.js",
            "/assets/three/three.module.js",
            "/assets/three/three.core.js",
            "/assets/three/GLTFLoader.js",
            "/assets/three/BufferGeometryUtils.js",
            "/assets/three/software-renderer.js",
            "/assets/three/replay-scene.js",
            "/assets/models/vehicle.glb",
            "/assets/models/gse-site.glb",
        ] {
            let (mime, bytes) = station_asset(path).unwrap();
            assert!(!bytes.is_empty());
            assert!(!mime.is_empty());
            let url = station_asset_url(path).unwrap();
            assert_eq!(url, path.replacen("/assets/", "/station-assets/", 1));
            assert_eq!(station_document_asset(&url), Some((mime, bytes)));
        }
        for path in [
            "/assets/three/../secret",
            "/api/auth",
            "https://outside.test/assets/models/vehicle.glb",
        ] {
            assert!(station_asset(path).is_none());
            assert!(station_asset_url(path).is_none());
        }
        for path in [
            "/station-assets/three/../secret",
            "/station-assets/three/%2e%2e/secret",
            "/station-assets/three/unknown.js",
            "/assets/three/vehicle-renderer.js",
            "https://outside.test/station-assets/models/vehicle.glb",
        ] {
            assert!(station_document_asset(path).is_none());
        }
    }
}
