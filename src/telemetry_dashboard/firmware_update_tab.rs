use super::prelude::*;
use super::{UrlConfig, http_get_json};
use crate::auth;
use serde::Deserialize;

#[derive(Debug, Clone, Deserialize, PartialEq)]
struct FirmwareTarget {
    board: String,
    board_label: String,
    hostname: String,
    discovered: bool,
    supported: bool,
    ota_stream_port: u16,
    artifact_kind: String,
    workflow: String,
    unavailable_reason: Option<String>,
}

#[derive(Debug, Clone, Deserialize, PartialEq)]
struct FirmwareUpdateStatus {
    id: u64,
    board: String,
    board_label: String,
    filename: String,
    artifact_kind: String,
    phase: String,
    bytes_sent: usize,
    total_bytes: usize,
    progress_percent: f32,
    board_max_bytes: Option<u32>,
    message: String,
}

fn terminal_phase(phase: &str) -> bool {
    matches!(phase, "completed" | "cancelled" | "failed")
}

fn is_seds_filename(filename: &str) -> bool {
    filename.to_ascii_lowercase().ends_with(".seds")
}

fn human_bytes(bytes: usize) -> String {
    if bytes >= 1024 * 1024 {
        format!("{:.2} MiB", bytes as f64 / (1024.0 * 1024.0))
    } else if bytes >= 1024 {
        format!("{:.1} KiB", bytes as f64 / 1024.0)
    } else {
        format!("{bytes} B")
    }
}

fn firmware_url(path: &str) -> Result<String, String> {
    let path = if path.starts_with('/') {
        path.to_string()
    } else {
        format!("/{path}")
    };
    let base = UrlConfig::base_http();
    if !base.trim().is_empty() {
        return Ok(format!("{}{path}", base.trim_end_matches('/')));
    }
    #[cfg(target_arch = "wasm32")]
    {
        let window = web_sys::window().ok_or("no browser window".to_string())?;
        let origin = window
            .location()
            .origin()
            .map_err(|_| "failed to read browser origin".to_string())?;
        return Ok(format!("{origin}{path}"));
    }
    #[cfg(not(target_arch = "wasm32"))]
    Ok(format!("http://localhost:3000{path}"))
}

#[cfg(target_arch = "wasm32")]
async fn upload_firmware(
    board: &str,
    filename: &str,
    bytes: Vec<u8>,
) -> Result<FirmwareUpdateStatus, String> {
    use gloo_net::http::Request;

    let url = firmware_url(&format!("/api/firmware/flash/{board}"))?;
    let mut request = Request::post(&url)
        .header("Content-Type", "application/octet-stream")
        .header("X-Firmware-Filename", filename);
    if let Some(token) = auth::current_token() {
        request = request.header("Authorization", &format!("Bearer {token}"));
    }
    let response = request
        .body(bytes)
        .map_err(|err| err.to_string())?
        .send()
        .await
        .map_err(|err| err.to_string())?;
    let status = response.status();
    let body = response.text().await.map_err(|err| err.to_string())?;
    if status == 401 {
        auth::clear_current_session();
    }
    if !(200..300).contains(&status) {
        return Err(format!("HTTP {status}: {}", body.trim()));
    }
    serde_json::from_str(&body).map_err(|err| err.to_string())
}

#[cfg(not(target_arch = "wasm32"))]
async fn upload_firmware(
    board: &str,
    filename: &str,
    bytes: Vec<u8>,
) -> Result<FirmwareUpdateStatus, String> {
    let url = firmware_url(&format!("/api/firmware/flash/{board}"))?;
    let client = auth::build_native_http_client(
        UrlConfig::_skip_tls_verify(),
        std::time::Duration::from_secs(10),
        std::time::Duration::from_secs(120),
    )?;
    let mut request = client
        .post(url)
        .header("Content-Type", "application/octet-stream")
        .header("X-Firmware-Filename", filename)
        .body(bytes);
    if let Some(token) = auth::current_token() {
        request = request.bearer_auth(token);
    }
    let response = request.send().await.map_err(|err| err.to_string())?;
    let status = response.status();
    let body = response.text().await.map_err(|err| err.to_string())?;
    if status == reqwest::StatusCode::UNAUTHORIZED {
        auth::clear_current_session();
    }
    if !status.is_success() {
        return Err(format!("HTTP {status}: {}", body.trim()));
    }
    serde_json::from_str(&body).map_err(|err| err.to_string())
}

#[cfg(target_arch = "wasm32")]
async fn cancel_firmware(id: u64) -> Result<FirmwareUpdateStatus, String> {
    use gloo_net::http::Request;

    let url = firmware_url(&format!("/api/firmware/updates/{id}/cancel"))?;
    let mut request = Request::post(&url);
    if let Some(token) = auth::current_token() {
        request = request.header("Authorization", &format!("Bearer {token}"));
    }
    let response = request.send().await.map_err(|err| err.to_string())?;
    let status = response.status();
    let body = response.text().await.map_err(|err| err.to_string())?;
    if !(200..300).contains(&status) {
        return Err(format!("HTTP {status}: {}", body.trim()));
    }
    serde_json::from_str(&body).map_err(|err| err.to_string())
}

#[cfg(not(target_arch = "wasm32"))]
async fn cancel_firmware(id: u64) -> Result<FirmwareUpdateStatus, String> {
    let url = firmware_url(&format!("/api/firmware/updates/{id}/cancel"))?;
    let client = auth::build_native_http_client(
        UrlConfig::_skip_tls_verify(),
        std::time::Duration::from_secs(8),
        std::time::Duration::from_secs(15),
    )?;
    let mut request = client.post(url);
    if let Some(token) = auth::current_token() {
        request = request.bearer_auth(token);
    }
    let response = request.send().await.map_err(|err| err.to_string())?;
    let status = response.status();
    let body = response.text().await.map_err(|err| err.to_string())?;
    if !status.is_success() {
        return Err(format!("HTTP {status}: {}", body.trim()));
    }
    serde_json::from_str(&body).map_err(|err| err.to_string())
}

#[cfg(target_arch = "wasm32")]
async fn poll_delay() {
    gloo_timers::future::TimeoutFuture::new(500).await;
}

#[cfg(not(target_arch = "wasm32"))]
async fn poll_delay() {
    tokio::time::sleep(std::time::Duration::from_millis(500)).await;
}

async fn poll_update(id: u64, mut status_signal: Signal<Option<FirmwareUpdateStatus>>) {
    loop {
        match http_get_json::<FirmwareUpdateStatus>(&format!("/api/firmware/updates/{id}")).await {
            Ok(status) => {
                let done = terminal_phase(&status.phase);
                status_signal.set(Some(status));
                if done {
                    return;
                }
            }
            Err(err) => {
                let mut current = status_signal.read().clone();
                if let Some(status) = current.as_mut() {
                    status.message = format!("Progress refresh failed: {err}");
                }
                status_signal.set(current);
            }
        }
        poll_delay().await;
    }
}

#[component]
pub(crate) fn FirmwareUpdateTab(theme: ThemeConfig) -> Element {
    let mut targets = use_signal(Vec::<FirmwareTarget>::new);
    let mut selected_board = use_signal(String::new);
    let mut selected_filename = use_signal(String::new);
    let mut selected_bytes = use_signal(|| None::<Vec<u8>>);
    let mut acknowledged = use_signal(|| false);
    let mut loading = use_signal(|| true);
    let mut target_error = use_signal(|| None::<String>);
    let mut upload_error = use_signal(String::new);
    let mut update = use_signal(|| None::<FirmwareUpdateStatus>);

    // Discovery continues after this tab mounts. Keep the address-book snapshot
    // current, without switching an operator's selected firmware destination.
    use_future(move || async move {
        loop {
            match http_get_json::<Vec<FirmwareTarget>>("/api/firmware/targets").await {
                Ok(found) => {
                    if selected_board.read().is_empty()
                        && let Some(first) = found
                            .iter()
                            .find(|target| target.supported && target.discovered)
                    {
                        selected_board.set(first.board.clone());
                    }
                    targets.set(found);
                    target_error.set(None);
                }
                Err(err) => {
                    targets.set(Vec::new());
                    target_error.set(Some(format!("Unable to refresh firmware targets: {err}")));
                }
            }
            loading.set(false);
            for _ in 0..4 {
                poll_delay().await;
            }
        }
    });

    let can_flash = auth::can_send_command("FirmwareUpdate");
    let active = update
        .read()
        .as_ref()
        .is_some_and(|status| !terminal_phase(&status.phase));
    let selected_target = targets
        .read()
        .iter()
        .find(|target| target.board == *selected_board.read())
        .cloned();
    let ready = can_flash
        && acknowledged()
        && selected_bytes
            .read()
            .as_ref()
            .is_some_and(|bytes| !bytes.is_empty())
        && selected_target
            .as_ref()
            .is_some_and(|target| target.supported && target.discovered)
        && !active;

    let panel_style = format!(
        "padding:18px; border:1px solid {}; border-radius:12px; background:{}; color:{};",
        theme.border, theme.panel_background, theme.text_primary
    );
    let input_style = format!(
        "width:100%; padding:10px; border:1px solid {}; border-radius:9px; background:{}; color:{};",
        theme.button_border, theme.panel_background_alt, theme.text_primary
    );
    let primary_button = format!(
        "padding:10px 16px; border:1px solid {}; border-radius:9px; background:{}; color:{}; cursor:{}; opacity:{};",
        theme.info_accent,
        theme.info_background,
        theme.info_text,
        if ready { "pointer" } else { "not-allowed" },
        if ready { "1" } else { "0.55" }
    );
    let firmware_accent = theme
        .main_tab_accents
        .get("firmware-update")
        .map(String::as_str)
        .unwrap_or(theme.info_accent.as_str());

    rsx! {
        div { style: "height:100%; overflow-y:auto; padding:16px; display:flex; flex-direction:column; gap:14px; background:{theme.app_background};",
            div { style: "{panel_style}",
                h2 { style: "margin:0 0 8px; color:{firmware_accent};", "Firmware Update" }
                p { style: "margin:0; color:{theme.text_secondary}; line-height:1.45;",
                    "Upload a .seds firmware artifact. The backend sends it to the selected board over the routed SEDSnet stream, then the board validates, installs, and reboots."
                }
            }

            div { style: "{panel_style} display:grid; gap:14px;",
                label { style: "display:grid; gap:6px; font-weight:600;",
                    "Target board"
                    if let Some(error) = target_error.read().as_ref() {
                        span { style:"font-size:12px;font-weight:400;", "{error}" }
                    }
                    select {
                        style: "{input_style}",
                        value: "{selected_board}",
                        disabled: active || loading(),
                        onchange: move |event| selected_board.set(event.value()),
                        if selected_board.read().is_empty() {
                            option { value: "", disabled: true, "No supported online OTA targets" }
                        }
                        for target in targets.read().iter() {
                            option {
                                value: "{target.board}",
                                disabled: !target.supported || !target.discovered,
                                if !target.supported {
                                    "{target.board_label} ({target.hostname}) — OTA not implemented"
                                } else if target.discovered {
                                    "{target.board_label} ({target.hostname}) — online"
                                } else {
                                    "{target.board_label} ({target.hostname}) — not discovered"
                                }
                            }
                        }
                    }
                }

                label { style: "display:grid; gap:6px; font-weight:600;",
                    "SEDS firmware file"
                    input {
                        style: "{input_style}",
                        r#type: "file",
                        accept: ".seds",
                        disabled: active,
                        onchange: move |event| {
                            let Some(file) = event.files().first().cloned() else {
                                return;
                            };
                            let filename = file.name();
                            if !is_seds_filename(&filename) {
                                selected_filename.set(String::new());
                                selected_bytes.set(None);
                                acknowledged.set(false);
                                upload_error.set("Only .seds firmware files can be uploaded.".to_string());
                                return;
                            }
                            spawn(async move {
                                match file.read_bytes().await {
                                    Ok(bytes) => {
                                        selected_filename.set(filename);
                                        selected_bytes.set(Some(bytes.to_vec()));
                                        upload_error.set(String::new());
                                        acknowledged.set(false);
                                    }
                                    Err(err) => upload_error.set(format!("Failed to read the selected firmware file: {err}")),
                                }
                            });
                        }
                    }
                }
                if let Some(bytes) = selected_bytes.read().as_ref() {
                    div { style: "color:{theme.text_secondary};", "Selected: {selected_filename} ({human_bytes(bytes.len())})" }
                }
                if let Some(target) = selected_target.as_ref()
                    && !target.supported
                    && let Some(reason) = target.unavailable_reason.as_deref()
                {
                    div { style: "color:{theme.warning_text};", "{reason}" }
                }
                if let Some(target) = selected_target.as_ref()
                    && target.supported
                {
                    div { style: "color:{theme.text_muted}; font-size:0.9rem;", "Workflow: {target.workflow} on SEDSnet stream port {target.ota_stream_port}" }
                }

                div { style: "padding:12px; border:1px solid {theme.warning_border}; border-radius:10px; background:{theme.warning_background}; color:{theme.warning_text}; line-height:1.45;",
                    strong { "Warning: " }
                    "Only a live-update delta .seds artifact is accepted. It must be generated against the firmware currently installed on this board. Full-image recovery .seds files require the board's UART recovery tool. A successful live transfer reboots the target immediately."
                }
                label { style: "display:flex; align-items:flex-start; gap:9px; color:{theme.text_secondary};",
                    input {
                        r#type: "checkbox",
                        checked: acknowledged(),
                        disabled: active,
                        onchange: move |event| acknowledged.set(event.checked()),
                    }
                    span { "I selected the correct board and firmware base, and I understand that the board will reboot." }
                }

                if !can_flash {
                    div { style: "color:{theme.warning_text};", "Your session is not allowed to perform FirmwareUpdate operations." }
                }
                button {
                    style: "{primary_button}",
                    disabled: !ready,
                    onclick: move |_| {
                        if !ready {
                            return;
                        }
                        let board = selected_board();
                        let filename = selected_filename();
                        let Some(bytes) = selected_bytes.read().clone() else {
                            return;
                        };
                        upload_error.set(String::new());
                        spawn(async move {
                            match upload_firmware(&board, &filename, bytes).await {
                                Ok(status) => {
                                    let id = status.id;
                                    update.set(Some(status));
                                    poll_update(id, update).await;
                                }
                                Err(err) => upload_error.set(err),
                            }
                        });
                    },
                    "Upload and Flash"
                }
            }

            if !upload_error.read().is_empty() {
                div { style: "padding:12px; border:1px solid {theme.error_border}; border-radius:10px; background:{theme.error_background}; color:{theme.error_text};", "{upload_error}" }
            }

            if let Some(status) = update.read().as_ref() {
                div { style: "{panel_style} display:grid; gap:10px;",
                    div { style: "display:flex; justify-content:space-between; gap:12px; flex-wrap:wrap;",
                        strong { "{status.board_label} — {status.phase}" }
                        span { style: "color:{theme.text_secondary};", "Job #{status.id}" }
                    }
                    progress {
                        style: "width:100%; height:18px; accent-color:{theme.info_accent};",
                        max: "100",
                        value: "{status.progress_percent.clamp(0.0, 100.0)}",
                    }
                    div { style: "color:{theme.text_secondary};", "{human_bytes(status.bytes_sent)} / {human_bytes(status.total_bytes)} ({status.progress_percent:.1}%)" }
                    div { style: "color:{theme.text_secondary};", "{status.message}" }
                    if let Some(maximum) = status.board_max_bytes {
                        div { style: "color:{theme.text_muted}; font-size:0.9rem;", "Board capacity: {human_bytes(maximum as usize)}" }
                    }
                    if !terminal_phase(&status.phase) {
                        button {
                            style: "padding:8px 12px; justify-self:start; border:1px solid {theme.error_border}; border-radius:8px; background:{theme.error_background}; color:{theme.error_text}; cursor:pointer;",
                            onclick: {
                                let id = status.id;
                                move |_| {
                                    spawn(async move {
                                        match cancel_firmware(id).await {
                                            Ok(status) => update.set(Some(status)),
                                            Err(err) => upload_error.set(err),
                                        }
                                    });
                                }
                            },
                            "Cancel Update"
                        }
                    }
                }
            }
        }
    }
}
