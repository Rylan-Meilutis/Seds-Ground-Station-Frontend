mod app;
mod auth;
mod native_storage;
mod telemetry_dashboard;

use dioxus::prelude::*;
#[cfg(not(target_arch = "wasm32"))]
use dioxus_desktop::tao::window::WindowBuilder;
#[cfg(not(target_arch = "wasm32"))]
use dioxus_desktop::wry::http::{Request as HttpRequest, Response as HttpResponse};
#[cfg(not(target_arch = "wasm32"))]
use dioxus_desktop::RequestAsyncResponder;
#[cfg(not(target_arch = "wasm32"))]
use image::ImageFormat;
#[cfg(not(target_arch = "wasm32"))]
use std::backtrace::Backtrace;
#[cfg(not(target_arch = "wasm32"))]
use std::borrow::Cow;
#[cfg(not(target_arch = "wasm32"))]
use std::fs::{create_dir_all, OpenOptions};
#[cfg(not(target_arch = "wasm32"))]
use std::hash::{Hash, Hasher};
#[cfg(not(target_arch = "wasm32"))]
use std::io::Write;
#[cfg(not(target_arch = "wasm32"))]
use std::panic::{self, AssertUnwindSafe};
#[cfg(not(target_arch = "wasm32"))]
use std::path::PathBuf;
#[cfg(not(target_arch = "wasm32"))]
use std::time::{SystemTime, UNIX_EPOCH};
#[cfg(not(target_arch = "wasm32"))]
use std::{collections::hash_map::DefaultHasher, fs};

#[cfg(target_arch = "wasm32")]
/// Installs a browser panic hook so Rust panics appear in the JS console.
fn init_panic_hook() {
    console_error_panic_hook::set_once();
}

#[cfg(not(target_arch = "wasm32"))]
/// Installs a native panic hook that appends panic details to the frontend log file.
fn init_panic_hook() {
    panic::set_hook(Box::new(|panic_info| {
        let payload = if let Some(s) = panic_info.payload().downcast_ref::<&str>() {
            (*s).to_string()
        } else if let Some(s) = panic_info.payload().downcast_ref::<String>() {
            s.clone()
        } else {
            "non-string panic payload".to_string()
        };
        let location = panic_info
            .location()
            .map(|loc| format!("{}:{}", loc.file(), loc.line()))
            .unwrap_or_else(|| "unknown".to_string());
        let bt = Backtrace::force_capture();
        append_native_log(&format!(
            "[panic] location={location} payload={payload}\n[panic] backtrace={bt:?}"
        ));
    }));
}

#[cfg(not(target_arch = "wasm32"))]
/// Resolves the native frontend log file path, honoring the override environment variable.
fn log_file_path() -> PathBuf {
    if let Ok(p) = std::env::var("GS26_FRONTEND_LOG")
        && !p.trim().is_empty()
    {
        return PathBuf::from(p);
    }
    std::env::temp_dir().join("groundstation_frontend.log")
}

#[cfg(not(target_arch = "wasm32"))]
/// Appends a timestamped line to the native frontend log file.
fn append_native_log(message: &str) {
    let path = log_file_path();
    if let Some(parent) = path.parent() {
        let _ = create_dir_all(parent);
    }
    let ts_ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0);
    let line = format!("[{ts_ms}] {message}\n");
    if let Ok(mut file) = OpenOptions::new().create(true).append(true).open(path) {
        let _ = file.write_all(line.as_bytes());
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn tile_cache_root() -> PathBuf {
    std::env::temp_dir().join("gs26-tile-cache")
}

#[cfg(not(target_arch = "wasm32"))]
fn tile_cache_path(base: &str, z: u32, x: u32, y: u32) -> PathBuf {
    let mut hasher = DefaultHasher::new();
    base.hash(&mut hasher);
    let base_key = format!("{:016x}", hasher.finish());
    tile_cache_root()
        .join(base_key)
        .join(z.to_string())
        .join(x.to_string())
        .join(format!("{y}.jpg"))
}

#[cfg(not(target_arch = "wasm32"))]
fn read_cached_tile(base: &str, z: u32, x: u32, y: u32) -> Option<Vec<u8>> {
    let path = tile_cache_path(base, z, x, y);
    fs::read(path).ok()
}

#[cfg(not(target_arch = "wasm32"))]
fn write_cached_tile(base: &str, z: u32, x: u32, y: u32, bytes: &[u8]) {
    let path = tile_cache_path(base, z, x, y);
    if let Some(parent) = path.parent() {
        let _ = create_dir_all(parent);
    }
    let _ = fs::write(path, bytes);
}

#[cfg(target_os = "android")]
/// Initializes rustls-platform-verifier with Android JVM/context handles.
fn init_android_platform_tls_verifier() {
    use ::jni::JavaVM;
    use ::jni::objects::JObject;
    use ::jni021::JavaVM as JavaVM021;
    use ::jni021::objects::JObject as JObject021;

    let ctx = ndk_context::android_context();
    let vm = match unsafe { JavaVM::from_raw(ctx.vm().cast()) } {
        vm => vm,
    };

    match vm.attach_current_thread(|env| -> ::jni::errors::Result<()> {
        let context = unsafe { JObject::from_raw(env, ctx.context().cast()) };
        rustls_platform_verifier::android::init_with_env(env, context)?;
        Ok(())
    }) {
        Ok(_) => append_native_log("[startup] android TLS verifier initialized"),
        Err(e) => append_native_log(&format!("[startup] android TLS verifier init failed: {e}")),
    }

    match (|| -> ::jni021::errors::Result<()> {
        let vm = unsafe { JavaVM021::from_raw(ctx.vm().cast()) }?;
        let mut env = vm.attach_current_thread()?;
        let context = unsafe { JObject021::from_raw(ctx.context().cast()) };
        rustls_platform_verifier_reqwest::android::init_with_env(&mut env, context)?;
        Ok(())
    })() {
        Ok(_) => append_native_log("[startup] android reqwest TLS verifier initialized"),
        Err(e) => append_native_log(&format!(
            "[startup] android reqwest TLS verifier init failed: {e}"
        )),
    }
}

#[cfg(target_arch = "wasm32")]
/// Launches the web build of the frontend.
fn main() {
    init_panic_hook();

    // Web launch (wasm)
    // You can add assets config here if you want; default is fine.
    launch(app::App);
}

#[cfg(not(target_arch = "wasm32"))]
/// Launches the desktop build and wires in the custom tile proxy protocol.
fn main() {
    init_panic_hook();
    append_native_log("[startup] native main entered");
    #[cfg(target_os = "android")]
    init_android_platform_tls_verifier();
    let mut cfg = dioxus_desktop::Config::new();
    #[cfg(target_os = "android")]
    {
        cfg = cfg.with_custom_protocol("gs26", |_id, request| {
            append_native_log("[startup] protocol request dispatched");
            handle_gs26_protocol_safely(request)
        });
    }
    #[cfg(not(target_os = "android"))]
    {
        cfg = cfg.with_asynchronous_custom_protocol("gs26", |_id, request, responder| {
            append_native_log("[startup] protocol request dispatched");
            _handle_gs26_protocol_async(request, responder);
        });
    }
    #[cfg(target_os = "android")]
    {
        cfg = cfg.with_custom_head(android_custom_head());
    }
    cfg = cfg.with_window(WindowBuilder::new().with_title(app::APP_DISPLAY_NAME));
    if let Some(icon) = load_desktop_window_icon() {
        cfg = cfg.with_icon(icon);
    }
    append_native_log("[startup] launching desktop app");
    LaunchBuilder::desktop().with_cfg(cfg).launch(app::App);
    append_native_log("[startup] desktop launch returned");
}

#[cfg(not(target_arch = "wasm32"))]
fn handle_gs26_protocol_safely(request: HttpRequest<Vec<u8>>) -> HttpResponse<Cow<'static, [u8]>> {
    match panic::catch_unwind(AssertUnwindSafe(|| handle_gs26_protocol(request))) {
        Ok(resp) => resp,
        Err(_) => {
            append_native_log("[protocol] panic while handling request");
            HttpResponse::builder()
                .status(500)
                .body(Cow::Owned(Vec::new()))
                .unwrap_or_else(|_| HttpResponse::new(Cow::Owned(Vec::new())))
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
/// Loads the desktop window icon from the bundled PNG asset.
fn load_desktop_window_icon() -> Option<dioxus_desktop::tao::window::Icon> {
    let image =
        image::load_from_memory_with_format(include_bytes!("../assets/icon.png"), ImageFormat::Png)
            .ok()?
            .into_rgba8();
    let (width, height) = image.dimensions();
    dioxus_desktop::tao::window::Icon::from_rgba(image.into_raw(), width, height).ok()
}

#[cfg(target_os = "android")]
fn android_custom_head() -> String {
    r#"
<script>
(() => {
    const normalizeInternalUrl = (value) => {
        if (typeof value !== "string") {
            return value;
        }
        return value
            .replace("https://dioxus.index.html//__events", "https://dioxus.index.html/__events")
            .replace("http://dioxus.index.html//__events", "http://dioxus.index.html/__events");
    };

    const originalXhrOpen = XMLHttpRequest.prototype.open;
    XMLHttpRequest.prototype.open = function(method, url, ...rest) {
        return originalXhrOpen.call(this, method, normalizeInternalUrl(url), ...rest);
    };
})();
</script>
"#
    .to_string()
}

#[cfg(not(target_arch = "wasm32"))]
/// Handles `gs26://` requests by proxying tile image reads to the configured backend.
fn handle_gs26_protocol(request: HttpRequest<Vec<u8>>) -> HttpResponse<Cow<'static, [u8]>> {
    /// Builds a protocol response while falling back to an empty 500 on builder failure.
    fn build_response(
        status: u16,
        content_type: Option<&str>,
        body: Vec<u8>,
    ) -> HttpResponse<Cow<'static, [u8]>> {
        let mut builder = HttpResponse::builder().status(status);
        builder = builder
            .header("Access-Control-Allow-Origin", "*")
            .header("Access-Control-Allow-Methods", "GET, OPTIONS")
            .header("Access-Control-Allow-Headers", "*")
            .header("Cross-Origin-Resource-Policy", "cross-origin");
        if let Some(ct) = content_type {
            builder = builder.header("Content-Type", ct);
        }
        builder.body(Cow::Owned(body)).unwrap_or_else(|_| {
            HttpResponse::builder()
                .status(500)
                .header("Access-Control-Allow-Origin", "*")
                .body(Cow::Owned(Vec::new()))
                .unwrap()
        })
    }

    let uri = request.uri().to_string();
    append_native_log(&format!("[protocol] request uri={uri}"));
    let path = request.uri().path();
    let segs: Vec<&str> = path
        .trim_start_matches('/')
        .split('/')
        .filter(|s| !s.is_empty())
        .collect();

    // Accept either:
    // - /tiles/{z}/{x}/{y}.jpg
    // - /{host}/tiles/{z}/{x}/{y}.jpg
    let parts: &[&str] = if segs.len() >= 4 && segs[segs.len() - 4] == "tiles" {
        &segs[segs.len() - 4..]
    } else {
        &[]
    };

    if parts.len() != 4 || !parts[3].ends_with(".jpg") {
        return build_response(404, None, Vec::new());
    }

    let z = match parts[1].parse::<u32>() {
        Ok(v) => v,
        Err(_) => return build_response(400, None, Vec::new()),
    };
    let x = match parts[2].parse::<u32>() {
        Ok(v) => v,
        Err(_) => return build_response(400, None, Vec::new()),
    };
    let y = match parts[3].trim_end_matches(".jpg").parse::<u32>() {
        Ok(v) => v,
        Err(_) => return build_response(400, None, Vec::new()),
    };

    let base = telemetry_dashboard::persisted_base_http_for_native_io();
    let skip_tls = telemetry_dashboard::persisted_skip_tls_for_base_for_native_io(&base);
    let tile_url = format!("{}/tiles/{z}/{x}/{y}.jpg", base.trim_end_matches('/'));
    append_native_log(&format!(
        "[protocol] tile fetch base={} skip_tls={} url={}",
        base, skip_tls, tile_url
    ));

    if let Some(cached) = read_cached_tile(&base, z, x, y) {
        append_native_log("[protocol] cache hit, serving tile without upstream fetch");
        return build_response(200, Some("image/jpeg"), cached);
    }

    let client = match reqwest::blocking::Client::builder()
        .danger_accept_invalid_certs(skip_tls)
        .build()
    {
        Ok(c) => c,
        Err(_) => return build_response(500, None, Vec::new()),
    };

    let upstream = match client.get(tile_url).send() {
        Ok(r) => r,
        Err(err) => {
            append_native_log(&format!(
                "[protocol] upstream fetch failed, attempting cache fallback: {err}"
            ));
            if let Some(cached) = read_cached_tile(&base, z, x, y) {
                return build_response(200, Some("image/jpeg"), cached);
            }
            return build_response(502, None, Vec::new());
        }
    };

    let status = upstream.status().as_u16();
    let content_type = upstream
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string());
    let bytes = match upstream.bytes() {
        Ok(b) => b.to_vec(),
        Err(err) => {
            append_native_log(&format!(
                "[protocol] upstream body read failed, attempting cache fallback: {err}"
            ));
            if let Some(cached) = read_cached_tile(&base, z, x, y) {
                return build_response(200, content_type.as_deref().or(Some("image/jpeg")), cached);
            }
            return build_response(502, None, Vec::new());
        }
    };

    if status == 404 {
        return build_response(204, content_type.as_deref(), Vec::new());
    }

    if (200..300).contains(&status) && !bytes.is_empty() {
        write_cached_tile(&base, z, x, y, &bytes);
    } else if let Some(cached) = read_cached_tile(&base, z, x, y) {
        return build_response(200, content_type.as_deref().or(Some("image/jpeg")), cached);
    }

    build_response(status, content_type.as_deref(), bytes)
}

#[cfg(not(target_arch = "wasm32"))]
/// Runs the custom protocol handler on a dedicated thread so blocking tile fetches do not stall the UI.
fn _handle_gs26_protocol_async(request: HttpRequest<Vec<u8>>, responder: RequestAsyncResponder) {
    let _ = std::thread::Builder::new()
        .name("gs26-proto-req".to_string())
        .spawn(move || {
            let response = handle_gs26_protocol_safely(request);
            if panic::catch_unwind(AssertUnwindSafe(|| responder.respond(response))).is_err() {
                append_native_log("[protocol] panic while responding to request");
            }
        });
}
