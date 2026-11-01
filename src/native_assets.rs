//! Bundled renderer assets: native module imports must not depend on backend TLS/CORS.
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
        #[cfg(any(target_os = "windows", target_os = "android"))]
        let base = "http://gs26.station";
        #[cfg(not(any(target_os = "windows", target_os = "android")))]
        let base = "gs26://station";
        Some(format!("{base}{path}"))
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
            assert!(station_asset_url(path).unwrap().ends_with(path));
        }
        for path in [
            "/assets/three/../secret",
            "/api/auth",
            "https://outside.test/assets/models/vehicle.glb",
        ] {
            assert!(station_asset(path).is_none());
            assert!(station_asset_url(path).is_none());
        }
    }
}
