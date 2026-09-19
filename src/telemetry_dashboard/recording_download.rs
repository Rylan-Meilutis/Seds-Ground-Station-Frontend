use super::{UrlConfig, http_get_json, layout::ThemeConfig};
use crate::auth;
use dioxus::prelude::*;
use serde::Deserialize;

#[derive(Clone, Debug, Deserialize, PartialEq)]
struct Recording {
    id: String,
    bytes: u64,
    active: bool,
}

fn download_path(id: &str) -> Result<String, String> {
    let valid = id
        .strip_prefix("groundstation_recording_")
        .and_then(|s| s.strip_suffix(".db"))
        .is_some_and(|s| {
            !s.is_empty()
                && s.bytes()
                    .all(|b| b.is_ascii_digit() || b == b'-' || b == b'_')
        });
    if !valid {
        return Err("Invalid recording selection. Refresh the list.".into());
    }
    Ok(format!("/api/recordings/{id}/csv"))
}

fn choose_recording(records: &[Recording], current: &str) -> String {
    if records.iter().any(|r| r.id == current) {
        current.to_owned()
    } else {
        records.first().map(|r| r.id.clone()).unwrap_or_default()
    }
}

#[cfg(target_arch = "wasm32")]
async fn download_csv(id: &str) -> Result<String, String> {
    let path = download_path(id)?;
    download_csv_path(&path, &format!("{}.csv", id.trim_end_matches(".db"))).await
}

#[cfg(target_arch = "wasm32")]
async fn download_csv_path(path: &str, filename: &str) -> Result<String, String> {
    let url = format!("{}{}", UrlConfig::base_http().trim_end_matches('/'), path);
    let token = auth::current_token();
    let script = format!(
        r#"
        return await (async () => {{
            const token = {token};
            const response = await fetch({url}, {{
                headers: token ? {{Authorization: "Bearer " + token}} : {{}},
                cache: "no-store"
            }});
            if (!response.ok) throw new Error("CSV download failed (" + response.status + "): " + await response.text());
            const blob = await response.blob();
            const address = URL.createObjectURL(blob);
            const link = document.createElement("a");
            link.href = address;
            link.download = {filename};
            document.body.appendChild(link);
            link.click();
            link.remove();
            setTimeout(() => URL.revokeObjectURL(address), 60000);
            return "CSV download ready. Check your browser downloads.";
        }})();
    "#,
        token = serde_json::to_string(&token).unwrap(),
        url = serde_json::to_string(&url).unwrap(),
        filename = serde_json::to_string(&filename).unwrap()
    );
    document::eval(&script)
        .join::<String>()
        .await
        .map_err(|e| e.to_string())
}

#[cfg(not(target_arch = "wasm32"))]
async fn download_csv(id: &str) -> Result<String, String> {
    let path = download_path(id)?;
    download_csv_path(&path, &format!("{}.csv", id.trim_end_matches(".db"))).await
}

#[cfg(not(target_arch = "wasm32"))]
async fn download_csv_path(path: &str, filename: &str) -> Result<String, String> {
    use std::io::Write;
    let base = UrlConfig::base_http();
    let base = if base.is_empty() {
        "http://localhost:3000"
    } else {
        base.trim_end_matches('/')
    };
    let client = auth::build_native_http_client(
        UrlConfig::_skip_tls_verify(),
        std::time::Duration::from_secs(10),
        std::time::Duration::from_secs(600),
    )?;
    let mut request = client.get(format!("{base}{path}"));
    if let Some(token) = auth::current_token() {
        request = request.bearer_auth(token);
    }
    let mut response = request.send().await.map_err(|e| e.to_string())?;
    if !response.status().is_success() {
        return Err(format!(
            "CSV download failed ({}): {}",
            response.status(),
            response.text().await.unwrap_or_default()
        ));
    }
    let dir = dirs::download_dir()
        .or_else(dirs::document_dir)
        .ok_or("No Downloads or Documents directory is available")?;
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|e| e.to_string())?
        .as_nanos();
    let target = dir.join(format!("{}-{stamp}.csv", filename.trim_end_matches(".csv")));
    let partial = target.with_extension("csv.part");
    let mut file = std::fs::OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(&partial)
        .map_err(|e| e.to_string())?;
    let result = async {
        while let Some(chunk) = response.chunk().await.map_err(|e| e.to_string())? {
            file.write_all(&chunk).map_err(|e| e.to_string())?;
        }
        file.flush().map_err(|e| e.to_string())?;
        Ok::<_, String>(())
    }
    .await;
    drop(file);
    if let Err(error) = result {
        let _ = std::fs::remove_file(&partial);
        return Err(error);
    }
    std::fs::rename(partial, &target).map_err(|e| e.to_string())?;
    Ok(format!("Saved CSV to {}", target.display()))
}

fn input_utc_ms(value: &str) -> Result<i64, String> {
    let format = time::macros::format_description!("[year]-[month]-[day]T[hour]:[minute]");
    let date = time::PrimitiveDateTime::parse(value, format)
        .map_err(|_| "Choose a valid UTC date and time.".to_string())?;
    i64::try_from(date.assume_utc().unix_timestamp_nanos() / 1_000_000).map_err(|e| e.to_string())
}
fn range_path(start: &str, end: &str) -> Result<String, String> {
    let start = input_utc_ms(start)?;
    let end = input_utc_ms(end)?;
    if start < 0 || end <= start {
        return Err("End time must be later than start time.".into());
    }
    Ok(format!("/api/recordings/csv?start_ms={start}&end_ms={end}"))
}
#[derive(Clone, Deserialize)]
struct ClockStatus {
    utc_ms: u64,
    network_utc_ms: Option<i64>,
    can_set_system_time: bool,
}
fn utc_label(ms: i128) -> String {
    time::OffsetDateTime::from_unix_timestamp_nanos(ms * 1_000_000)
        .map(|d| format!("{d} (UTC)"))
        .unwrap_or_else(|_| "Unknown".into())
}
fn client_now_ms() -> i64 {
    #[cfg(target_arch = "wasm32")]
    {
        js_sys::Date::now() as i64
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as i64
    }
}
#[component]
fn RangeAndClock() -> Element {
    let mut start = use_signal(String::new);
    let mut end = use_signal(String::new);
    let mut message = use_signal(String::new);
    let mut busy = use_signal(|| false);
    let mut clock = use_signal(|| None::<ClockStatus>);
    let mut refresh = use_signal(|| 0u32);
    let mut confirmed = use_signal(|| false);
    let mut manual = use_signal(String::new);
    use_effect(move || {
        let _ = refresh();
        spawn(async move {
            match http_get_json::<ClockStatus>("/api/system/time").await {
                Ok(c) => clock.set(Some(c)),
                Err(e) => message.set(format!("Could not read GroundStation clock: {e}")),
            }
        });
    });
    let can_set = clock.read().as_ref().is_some_and(|c| c.can_set_system_time);
    let has_network = clock
        .read()
        .as_ref()
        .is_some_and(|c| c.network_utc_ms.is_some());
    let mut sync = move |body: serde_json::Value| {
        busy.set(true);
        spawn(async move {
            match super::http_post_json::<_, ClockStatus>("/api/system/time", &body).await {
                Ok(c) => {
                    clock.set(Some(c));
                    confirmed.set(false);
                    message.set("GroundStation system clock updated. Future logs use this date/time. A large correction may require signing in again.".into());
                }
                Err(e) => message.set(format!("System clock was not updated: {e}")),
            }
            busy.set(false);
        });
    };
    rsx! {
        fieldset {
            legend { "Export a time range from all local recordings" }
            label { "Start (UTC, inclusive) " input { r#type:"datetime-local", value:"{start}", oninput: move |e| start.set(e.value()) } }
            label { "End (UTC, exclusive) " input { r#type:"datetime-local", value:"{end}", oninput: move |e| end.set(e.value()) } }
            button { disabled:busy(), onclick:move |_| {
                match range_path(&start(), &end()) {
                    Err(e) => message.set(e),
                    Ok(path) => { busy.set(true); spawn(async move {
                        message.set(match download_csv_path(&path, "telemetry-range.csv").await { Ok(s)=>s,Err(e)=>format!("Export failed: {e}") }); busy.set(false);
                    }); }
                }
            }, "Download date-range CSV" }
            p { "Automatically searches saved database contents. Old data recorded with a wrong clock remains under its original timestamp; use a session download below if needed." }
        }
        details {
            summary { "GroundStation system date/time" }
            if let Some(c) = clock.read().as_ref() { p { "Current server time: {utc_label(c.utc_ms as i128)}" } }
            button { disabled:busy(), onclick:move |_| refresh += 1, "Refresh time and permission" }
            if can_set {
                p { "This changes the machine clock for all logs, not just this export. Stop active operations first. NTP settings are not changed." }
                label { input { r#type:"checkbox", checked:confirmed(), onchange:move |e| confirmed.set(e.checked()) } "I intend to change the GroundStation system clock" }
                button { disabled:busy() || !confirmed(), onclick:move |_| sync(serde_json::json!({"source":"client","utc_ms":client_now_ms()})), "Set from this device" }
                button { disabled:busy() || !confirmed() || !has_network, onclick:move |_| sync(serde_json::json!({"source":"network"})), "Set from network UTC (RF GPS when available)" }
                label { "Set date/time (UTC) " input { r#type:"datetime-local", value:"{manual}", oninput:move |e| manual.set(e.value()) } }
                button { disabled:busy() || !confirmed(), onclick:move |_| {
                    match input_utc_ms(&manual()) { Ok(ms)=>sync(serde_json::json!({"source":"client","utc_ms":ms})), Err(e)=>message.set(e) }
                }, "Set selected date/time" }
            } else { p { "Sign in with Set System Date/Time permission to change the server clock." } }
        }
        p { role:"status", "{message}" }
    }
}

#[component]
pub fn RecordingDownloads(theme: ThemeConfig) -> Element {
    let mut recordings = use_signal(Vec::<Recording>::new);
    let mut selected = use_signal(String::new);
    let mut status = use_signal(String::new);
    let mut loading = use_signal(|| false);
    let mut downloading = use_signal(|| false);
    let mut refresh = use_signal(|| 0u32);
    use_effect(move || {
        let _ = refresh();
        loading.set(true);
        spawn(async move {
            match http_get_json::<Vec<Recording>>("/api/recordings").await {
                Ok(next) => {
                    let choice = choose_recording(&next, &selected.read());
                    selected.set(choice);
                    recordings.set(next);
                    status.set(String::new());
                }
                Err(error) => status.set(format!("Could not load recordings: {error}")),
            }
            loading.set(false);
        });
    });
    rsx! {
        details { style:"border:1px solid {theme.border};border-radius:10px;padding:10px;color:{theme.text_primary};background:{theme.panel_background};",
            summary { style:"cursor:pointer;font-weight:700;", "Data capture · CSV downloads" }
            RangeAndClock {}
            p { "Available in Test Fire, HITL, and normal mode. Use the recording actions to start or stop capture, then refresh this list." }
            p { style:"font-size:12px;color:{theme.text_muted};",
                "Exports every recorded telemetry row, with source identity, receive/source timestamps, raw values and payload bytes. Active recordings download committed rows up to the request; stop recording first for a complete capture."
            }
            div { style:"display:flex;gap:8px;flex-wrap:wrap;",
                select { aria_label:"Recording to download", value:"{selected}", disabled:loading() || downloading(),
                    onchange:move |e| selected.set(e.value()),
                    if recordings.read().is_empty() { option { value:"", "No recorded sessions yet" } }
                    for recording in recordings.read().iter() {
                        option { value:"{recording.id}",
                            "{recording.id} · {recording.bytes} bytes"
                            if recording.active { " · active" }
                        }
                    }
                }
                button { disabled:loading() || downloading(), onclick:move |_| refresh += 1,
                    if loading() { "Refreshing…" } else { "Refresh recordings" }
                }
                button { disabled:selected.read().is_empty() || downloading(), onclick:move |_| {
                    let id = selected();
                    downloading.set(true);
                    status.set("Downloading CSV…".into());
                    spawn(async move {
                        status.set(match download_csv(&id).await { Ok(message)=>message, Err(error)=>format!("Download failed: {error}") });
                        downloading.set(false);
                    });
                }, if downloading() { "Downloading…" } else { "Download CSV" } }
            }
            p { role:"status", style:"overflow-wrap:anywhere;", "{status}" }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn export_tab_has_a_distinct_persistent_id() {
        use super::super::{MainTab, _main_tab_from_str, _main_tab_to_str};
        assert_eq!(_main_tab_to_str(MainTab::DataExport), "data-export");
        assert!(_main_tab_from_str("data-export") == MainTab::DataExport);
        assert!(_main_tab_from_str("data") == MainTab::Data);
    }
    #[test]
    fn date_range_is_explicit_utc_and_validated() {
        assert_eq!(input_utc_ms("2026-01-01T00:00").unwrap(), 1767225600000);
        assert!(
            range_path("2026-01-01T00:00", "2026-01-01T00:01")
                .unwrap()
                .contains("start_ms=1767225600000")
        );
        assert!(range_path("2026-01-01T00:01", "2026-01-01T00:00").is_err());
        assert!(input_utc_ms("2026-02-30T00:00").is_err());
    }
    #[test]
    fn csv_route_and_selection_are_mode_independent() {
        let id = "groundstation_recording_2026-09-16_12-00-00_000.db";
        assert_eq!(
            download_path(id).unwrap(),
            format!("/api/recordings/{id}/csv")
        );
        assert!(download_path("../auth.db").is_err());
        let records = vec![Recording {
            id: id.into(),
            bytes: 1,
            active: true,
        }];
        assert_eq!(choose_recording(&records, ""), id);
        assert_eq!(choose_recording(&records, id), id);
        assert!(choose_recording(&[], id).is_empty());
    }
}
