use super::prelude::*;
use super::recording_download::ExportOwner;
use super::recording_download::{saved_json, use_export_lifetime};
use crate::auth;
use serde::Deserialize;
use std::rc::Rc;
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

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
    matches!(
        phase,
        "prepared_reboot_unverified" | "completed" | "cancelled" | "failed"
    )
}

fn phase_label(phase: &str) -> &str {
    match phase {
        "prepared_reboot_unverified" => "Prepared — reboot not verified",
        "completed" => "Transfer accepted — reboot not verified (legacy status)",
        "rebooting" => "Reboot requested — not verified",
        _ => phase,
    }
}
fn capture_owner(
    alive: Arc<AtomicBool>,
    selected: impl Fn() -> bool + 'static,
) -> Result<Rc<ExportOwner>, String> {
    ExportOwner::capture_selected(move || alive.load(Ordering::Acquire) && selected())
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

fn firmware_url(owner: &ExportOwner, path: &str) -> Result<String, String> {
    let path = if path.starts_with('/') {
        path.to_string()
    } else {
        format!("/{path}")
    };
    owner.check()?;
    let base = owner.base();
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
        Ok(format!("{origin}{path}"))
    }
    #[cfg(not(target_arch = "wasm32"))]
    Ok(format!("http://localhost:3000{path}"))
}

async fn firmware_post(
    path: &str,
    filename: Option<&str>,
    bytes: Vec<u8>,
    owner: Rc<ExportOwner>,
) -> Result<FirmwareUpdateStatus, String> {
    owner.check()?;
    if !auth::can_send_command("FirmwareUpdate") {
        return Err("FirmwareUpdate permission is required.".into());
    }
    let url = firmware_url(&owner, path)?;
    #[cfg(not(target_arch = "wasm32"))]
    let (status, text) = {
        let client = auth::build_native_http_client(
            owner.skip_tls(),
            std::time::Duration::from_secs(10),
            std::time::Duration::from_secs(120),
        )?;
        let mut request = client.post(url).body(bytes);
        if let Some(filename) = filename {
            request = request
                .header("Content-Type", "application/octet-stream")
                .header("X-Firmware-Filename", filename);
        }
        if let Some(token) = owner.token() {
            request = request.bearer_auth(token.to_owned());
        }
        let response = owner
            .wait(request.send())
            .await?
            .map_err(|err| err.to_string())?;
        let status = response.status().as_u16();
        let text = owner
            .wait(response.text())
            .await?
            .map_err(|err| err.to_string())?;
        (status, text)
    };
    #[cfg(target_arch = "wasm32")]
    let (status, text) = {
        let controller = web_sys::AbortController::new()
            .map_err(|_| "Firmware request cancellation unavailable")?;
        let _cancel = super::HttpAbortOnDrop(controller.clone());
        let signal = controller.signal();
        let mut request = gloo_net::http::Request::post(&url).abort_signal(Some(&signal));
        if let Some(filename) = filename {
            request = request
                .header("Content-Type", "application/octet-stream")
                .header("X-Firmware-Filename", filename);
        }
        if let Some(token) = owner.token() {
            request = request.header("Authorization", &format!("Bearer {token}"));
        }
        let request = request.body(bytes).map_err(|err| err.to_string())?;
        let response = owner
            .wait(request.send())
            .await?
            .map_err(|err| err.to_string())?;
        let status = response.status();
        let text = owner
            .wait(response.text())
            .await?
            .map_err(|err| err.to_string())?;
        (status, text)
    };
    owner.check()?;
    // This response never clears a possibly newer account's global session.
    if !(200..300).contains(&status) {
        return Err(format!(
            "HTTP {status}: {}",
            text.chars().take(200).collect::<String>()
        ));
    }
    serde_json::from_str(&text).map_err(|err| err.to_string())
}
async fn upload_firmware(
    board: &str,
    filename: &str,
    bytes: Vec<u8>,
    owner: Rc<ExportOwner>,
) -> Result<FirmwareUpdateStatus, String> {
    firmware_post(
        &format!("/api/firmware/flash/{board}"),
        Some(filename),
        bytes,
        owner,
    )
    .await
}
async fn cancel_firmware(id: u64, owner: Rc<ExportOwner>) -> Result<FirmwareUpdateStatus, String> {
    firmware_post(
        &format!("/api/firmware/updates/{id}/cancel"),
        None,
        Vec::new(),
        owner,
    )
    .await
}

#[cfg(target_arch = "wasm32")]
async fn poll_delay() {
    gloo_timers::future::TimeoutFuture::new(500).await;
}

#[cfg(not(target_arch = "wasm32"))]
async fn poll_delay() {
    tokio::time::sleep(std::time::Duration::from_millis(500)).await;
}

async fn poll_update(
    id: u64,
    mut status_signal: Signal<Option<FirmwareUpdateStatus>>,
    owner: Rc<ExportOwner>,
) {
    loop {
        if !owner.is_current() {
            return;
        }
        match saved_json::<FirmwareUpdateStatus>(
            &format!("/api/firmware/updates/{id}"),
            owner.clone(),
        )
        .await
        {
            Ok(status) if owner.is_current() => {
                let done = terminal_phase(&status.phase);
                status_signal.set(Some(status));
                if done {
                    return;
                }
            }
            Ok(_) => return,
            Err(err) => {
                if !owner.is_current() {
                    return;
                }
                let terminal_auth = err.contains("(401)") || err.contains("(403)");
                let mut current = status_signal.read().clone();
                if let Some(status) = current.as_mut() {
                    status.message = format!("Progress refresh failed: {err}");
                }
                status_signal.set(current);
                if terminal_auth {
                    return;
                }
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
    let lifetime = use_export_lifetime();
    let mut target_owner = use_signal(|| None::<Rc<ExportOwner>>);
    let mut job_owner = use_signal(|| None::<Rc<ExportOwner>>);
    let mut operation_epoch = use_signal(|| 0_u64);
    let mut submitting = use_signal(|| false);
    use_effect(move || {
        if update
            .read()
            .as_ref()
            .is_some_and(|status| terminal_phase(&status.phase))
        {
            // An unverified transfer never carries consent into another update.
            acknowledged.set(false);
        }
    });

    // Discovery continues after this tab mounts. Keep the address-book snapshot
    // current, without switching an operator's selected firmware destination.
    let discovery_lifetime = lifetime.clone();
    use_future(move || {
        let alive = discovery_lifetime.clone();
        async move {
            loop {
                let owner = match capture_owner(alive.clone(), || true) {
                    Ok(owner) => owner,
                    Err(_) => {
                        poll_delay().await;
                        continue;
                    }
                };
                match saved_json::<Vec<FirmwareTarget>>("/api/firmware/targets", owner.clone())
                    .await
                {
                    Ok(found) if owner.is_current() => {
                        if target_owner
                            .peek()
                            .as_ref()
                            .is_some_and(|prior| !prior.is_current())
                        {
                            selected_board.set(String::new());
                            selected_filename.set(String::new());
                            selected_bytes.set(None);
                            acknowledged.set(false);
                            update.set(None);
                            job_owner.set(None);
                            submitting.set(false);
                            operation_epoch.set(operation_epoch().saturating_add(1));
                        }
                        if selected_board.peek().is_empty()
                            && let Some(first) = found
                                .iter()
                                .find(|target| target.supported && target.discovered)
                        {
                            selected_board.set(first.board.clone());
                        }
                        targets.set(found);
                        target_owner.set(Some(owner));
                        target_error.set(None);
                        loading.set(false);
                    }
                    Ok(_) => {}
                    Err(err) if owner.is_current() => {
                        targets.set(Vec::new());
                        target_error
                            .set(Some(format!("Unable to refresh firmware targets: {err}")));
                        loading.set(false);
                    }
                    Err(_) => {}
                }
                for _ in 0..4 {
                    poll_delay().await;
                }
            }
        }
    });

    let can_flash = auth::can_send_command("FirmwareUpdate");
    let active = submitting()
        || update
            .read()
            .as_ref()
            .is_some_and(|status| !terminal_phase(&status.phase));
    let selected_target = targets
        .read()
        .iter()
        .find(|target| target.board == *selected_board.read())
        .cloned();
    let ready = can_flash
        && target_owner
            .read()
            .as_ref()
            .is_some_and(|owner| owner.is_current())
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

    let file_lifetime = lifetime.clone();
    let upload_lifetime = lifetime.clone();
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
                        onchange: move |event| { operation_epoch.set(operation_epoch().saturating_add(1)); acknowledged.set(false); selected_board.set(event.value()); },
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
                            let Some(source) = target_owner.peek().clone().filter(|owner| owner.is_current()) else { upload_error.set("Refresh targets for the current station before selecting firmware.".into()); return; };
                            let Some(epoch) = operation_epoch().checked_add(1) else { upload_error.set("Firmware selection ownership exhausted; reopen the dashboard.".into()); return; };
                            operation_epoch.set(epoch);
                            let board = selected_board();
                            let owner = match capture_owner(file_lifetime.clone(), move || source.is_current() && operation_epoch() == epoch && selected_board() == board) { Ok(owner) => owner, Err(_) => return };
                            spawn(async move {
                                match owner.wait(file.read_bytes()).await {
                                    Ok(Ok(bytes)) if owner.is_current() => {
                                        selected_filename.set(filename); selected_bytes.set(Some(bytes.to_vec()));
                                        upload_error.set(String::new()); acknowledged.set(false);
                                    }
                                    Ok(Err(err)) if owner.is_current() => upload_error.set(format!("Failed to read the selected firmware file: {err}")),
                                    _ => {}
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
                    "Only a live-update delta .seds artifact is accepted. It must match the installed firmware base. Full-image recovery files require the UART recovery tool. An accepted transfer requests a reset; it does not verify a reboot or the running image. Launch may prevent the reset."
                }
                label { style: "display:flex; align-items:flex-start; gap:9px; color:{theme.text_secondary};",
                    input {
                        r#type: "checkbox",
                        checked: acknowledged(),
                        disabled: active,
                        onchange: move |event| acknowledged.set(event.checked()),
                    }
                    span { "I selected the correct board and firmware base, and I understand that this requests a board reset." }
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
                        let Some(source) = target_owner.peek().clone().filter(|owner| owner.is_current()) else { upload_error.set("Refresh firmware targets for the current station.".into()); return; };
                        let Some(epoch) = operation_epoch().checked_add(1) else { upload_error.set("Firmware operation ownership exhausted; reopen the dashboard.".into()); return; };
                        operation_epoch.set(epoch);
                        let selected = board.clone();
                        let owner = match capture_owner(upload_lifetime.clone(), move || source.is_current() && operation_epoch() == epoch && selected_board() == selected && auth::can_send_command("FirmwareUpdate")) { Ok(owner) => owner, Err(_) => return };
                        job_owner.set(Some(owner.clone())); submitting.set(true); upload_error.set(String::new());
                        spawn(async move {
                            match upload_firmware(&board, &filename, bytes, owner.clone()).await {
                                Ok(status) if owner.is_current() => {
                                    submitting.set(false); let id = status.id;
                                    update.set(Some(status)); poll_update(id, update, owner).await;
                                }
                                Err(err) if owner.is_current() => { submitting.set(false); upload_error.set(err); }
                                _ => {}
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
                        strong { "{status.board_label} — {phase_label(&status.phase)}" }
                        span { style: "color:{theme.text_secondary};", "Job #{status.id}" }
                    }
                    progress {
                        style: "width:100%; height:18px; accent-color:{theme.info_accent};",
                        max: "100",
                        value: "{status.progress_percent.clamp(0.0, 100.0)}",
                    }
                    div { style: "color:{theme.text_secondary};", "{human_bytes(status.bytes_sent)} / {human_bytes(status.total_bytes)} ({status.progress_percent:.1}% transferred)" }
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
                                    let Some(owner) = job_owner.peek().clone().filter(|owner| owner.is_current()) else { upload_error.set("This update belongs to an earlier station or session; refresh targets.".into()); return; };
                                    spawn(async move {
                                        match cancel_firmware(id, owner.clone()).await {
                                            Ok(status) if owner.is_current() => update.set(Some(status)),
                                            Err(err) if owner.is_current() => upload_error.set(err),
                                            _ => {}
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

#[cfg(all(test, not(target_arch = "wasm32")))]
#[path = "firmware_update_owner_tests.rs"]
mod owner_tests;
