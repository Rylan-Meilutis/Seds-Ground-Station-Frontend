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
        String::new()
    }
}

#[cfg(target_arch = "wasm32")]
async fn download_csv(id: &str) -> Result<String, String> {
    let path = download_path(id)?;
    download_csv_path(&path, &format!("{}.csv", id.trim_end_matches(".db"))).await
}

#[cfg(target_arch = "wasm32")]
pub(super) async fn download_csv_path(path: &str, filename: &str) -> Result<String, String> {
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
            if (!response.ok) throw new Error("Export failed (" + response.status + "): " + await response.text());
            const blob = await response.blob();
            const address = URL.createObjectURL(blob);
            const link = document.createElement("a");
            link.href = address;
            link.download = {filename};
            document.body.appendChild(link);
            link.click();
            link.remove();
            setTimeout(() => URL.revokeObjectURL(address), 60000);
            return "Export ready. Check your downloads.";
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
pub(super) async fn download_csv_path(path: &str, filename: &str) -> Result<String, String> {
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
            "Export failed ({}): {}",
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
    let name = std::path::Path::new(filename);
    let stem = name.file_stem().and_then(|s| s.to_str()).unwrap_or("telemetry");
    let extension = name.extension().and_then(|s| s.to_str()).unwrap_or("csv");
    let target = dir.join(format!("{stem}-{stamp}.{extension}"));
    let partial = target.with_extension(format!("{extension}.part"));
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
    Ok(format!("Saved export to {}", target.display()))
}

pub(super) fn input_utc_ms(value: &str) -> Result<i64, String> {
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
    #[serde(default)]
    set_system_time_permission: bool,
    #[serde(default)]
    system_time_unavailable_reason: Option<String>,
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
        p { role: "status", "{message}" }
    }
}

#[component]
pub fn SystemClockSettings() -> Element {
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
                Err(e) => message.set(format!("Could not read station clock: {e}")),
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
                    message.set("OS clock updated immediately. Network UTC source accepted; network time synchronizes gradually.".into());
                }
                Err(e) => message.set(format!("Clock update failed: {e}")),
            }
            busy.set(false);
        });
    };
    rsx! {
        section { class: "system-clock-settings",
            h3 { "Station system date/time" }
            if let Some(c) = clock.read().as_ref() {
                p { "OS: {utc_label(c.utc_ms as i128)}" }
                if let Some(network) = c.network_utc_ms { p { "Network: {utc_label(network as i128)}" } }
            }
            button { disabled:busy(), onclick:move |_| refresh += 1, "Refresh time and permission" }
            if can_set {
                p { "OS updates immediately. Network UTC synchronizes gradually." }
                details {
                    summary { "About clock synchronization" }
                    p { "Network corrections normally use a 5% slew rate and preserve monotonic timers. NTP settings remain unchanged. A large OS clock change may require signing in again." }
                }
                label { input { r#type:"checkbox", checked:confirmed(), onchange:move |e| confirmed.set(e.checked()) } "Change this station’s system clock" }
                button { disabled:busy() || !confirmed(), onclick:move |_| sync(serde_json::json!({"source":"client","utc_ms":client_now_ms()})), "Set OS + network from this device" }
                button { disabled:busy() || !confirmed() || !has_network, onclick:move |_| sync(serde_json::json!({"source":"network"})), "Set OS from current network UTC" }
                label { "Set date/time (UTC) " input { r#type:"datetime-local", value:"{manual}", oninput:move |e| manual.set(e.value()) } }
                button { disabled:busy() || !confirmed(), onclick:move |_| {
                    match input_utc_ms(&manual()) { Ok(ms)=>sync(serde_json::json!({"source":"client","utc_ms":ms})), Err(e)=>message.set(e) }
                }, "Set selected date/time" }
            } else if let Some(c) = clock.read().as_ref() {
                if !c.set_system_time_permission { p { "Set System Date/Time permission is required." } }
                else if let Some(reason) = &c.system_time_unavailable_reason { p { "{reason}" } }
                else { p { "OS clock updates are unavailable on this host." } }
            }
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
        section { style:"border:1px solid {theme.border};border-radius:10px;padding:10px;color:{theme.text_primary};background:{theme.panel_background};",
            h2 { "Data Export" }
            p { "Available in Test Fire, HITL, and normal mode. Use the recording actions to start or stop capture, then refresh this list." }
            p { style:"font-size:12px;color:{theme.text_muted};",
                "Exports every recorded telemetry row, with source identity, receive/source timestamps, raw values and payload bytes. Active recordings download committed rows up to the request; stop recording first for a complete capture."
            }
            div { style:"display:flex;gap:8px;flex-wrap:wrap;",
                select { aria_label:"Recording to download", value:"{selected}", disabled:loading() || downloading(),
                    onchange:move |e| selected.set(e.value()),
                    option { value:"", "1. Select a recording…" }
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
            RecoveryHistoryExport {}
            ChecklistExport {}
            if !selected.read().is_empty() {
                super::report_studio::ReportStudio { key:"{selected}", recording:selected() }
            } else { p { "Select a recording first, then choose the data to export." } }
            details { summary { "Legacy CSV / clock settings" } RangeAndClock {} }
            p { role:"status", style:"overflow-wrap:anywhere;", "{status}" }
        }
    }
}

#[component]
fn ChecklistExport() -> Element {
    let mut busy = use_signal(|| false);
    let mut message = use_signal(String::new);
    rsx! { section {
        h3 { "Checklists" }
        div {style:"display:flex;gap:8px;flex-wrap:wrap;",
            for (path, file, label) in [("/api/checklists/export", "checklists.json", "Export checklists"), ("/api/checklists/export.csv", "checklist-signoffs.csv", "Export sign-offs CSV")] {
                button {disabled:busy(),onclick:move |_| {
                    busy.set(true);
                    spawn(async move {
                        message.set(match download_csv_path(path, file).await {Ok(text)=>text, Err(error)=>format!("Export failed: {error}")});
                        busy.set(false);
                    });
                }, "{label}"}
            }
        }
        if !message.read().is_empty() {p {role:"status", "{message}"}}
    }}
}

/// Recovery history belongs with recording exports, never in the global header.
#[component]
fn RecoveryHistoryExport() -> Element {
    let mut busy = use_signal(|| false);
    let mut message = use_signal(String::new);
    rsx! { section { class:"gs26-recovery-export",
        h3 { "Recovery history" }
        div { style:"display:flex;gap:8px;flex-wrap:wrap;",
            for kind in ["json", "csv"] {
                button { disabled:busy(), onclick:move |_| {
                    busy.set(true);
                    spawn(async move {
                        message.set(match download_csv_path(&format!("/api/recovery/export.{kind}"), &format!("launch-recovery-history.{kind}")).await {
                            Ok(text) => text, Err(error) => format!("Export failed: {error}"),
                        });
                        busy.set(false);
                    });
                }, "Export recovery {kind}" }
            }
        }
        if !message.read().is_empty() { p {role:"status", "{message}"} }
    }}
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
        assert_eq!(choose_recording(&records, ""), "");
        assert_eq!(choose_recording(&records, id), id);
        assert!(choose_recording(&[], id).is_empty());
    }
    #[test]
    fn clock_capability_distinguishes_host_limits_from_account_permission() {
        let clock: ClockStatus = serde_json::from_value(serde_json::json!({
            "utc_ms":1_800_000_000_000u64,"network_utc_ms":1_799_999_000_000i64,
            "can_set_system_time":false,"set_system_time_permission":true,
            "system_time_unavailable_reason":"Linux host needs CAP_SYS_TIME"
        })).unwrap();
        assert!(clock.set_system_time_permission);
        assert!(!clock.can_set_system_time);
        assert!(clock.system_time_unavailable_reason.unwrap().contains("CAP_SYS_TIME"));
    }

}

/// Save a complete portable replay, without a subsequent backend request.
pub(super) async fn save_replay_file(value: &serde_json::Value, standalone: bool) -> Result<String, String> {
    let contents = if standalone { portable_replay_html(value)? } else { serde_json::to_string(value).map_err(|e| e.to_string())? };
    let extension=if standalone { "html" } else { "json" };
    if contents.len() > 64 * 1024 * 1024 { return Err("Replay exceeds 64 MiB. Select a smaller recording.".into()); }
    #[cfg(target_arch = "wasm32")]
    {
        let text = serde_json::to_string(&contents).map_err(|e| e.to_string())?;
        document::eval(&format!(r#"const blob=new Blob([{text}],{{type:'application/json'}});const url=URL.createObjectURL(blob);const a=document.createElement('a');a.href=url;a.download='recording.seds-replay.{extension}';document.body.append(a);a.click();a.remove();setTimeout(()=>URL.revokeObjectURL(url),60000);return 'Replay exported. Import this file without a station connection.';"#)).join::<String>().await.map_err(|e|e.to_string())
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        use std::io::Write;
        let directory = dirs::download_dir().or_else(dirs::document_dir).ok_or("No Downloads or Documents directory")?;
        std::fs::create_dir_all(&directory).map_err(|e| e.to_string())?;
        let stamp=std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_err(|e|e.to_string())?.as_nanos();
        let path=directory.join(format!("recording-{stamp}.seds-replay.{extension}"));
        let mut file=std::fs::OpenOptions::new().create_new(true).write(true).open(&path).map_err(|e|e.to_string())?;
        file.write_all(contents.as_bytes()).and_then(|_|file.flush()).map_err(|e|e.to_string())?;
        Ok(format!("Replay saved to {}",path.display()))
    }
}

/// The standalone file carries its player, data and config. It runs from file://
/// without a web server, login, external models, scripts or network requests.
pub(super) fn offline_replay_script() -> Result<String, String> {
    // Every module is shipped with the app/export. Imported recording files
    // cannot select executable URLs or replace these assets.
    let assets=serde_json::json!({
        "core":include_str!("../../station_ui/rendering/three/three.core.js"),
        "three":include_str!("../../station_ui/rendering/three/three.module.js"),
        "scene":include_str!("../../station_ui/rendering/three/replay-scene.js"),
        "software":include_str!("../../station_ui/rendering/three/software-renderer.js"),
        "license":include_str!("../../station_ui/rendering/three/LICENSE")
    });
    let assets=serde_json::to_string(&assets).map_err(|e|e.to_string())?.replace('<',"\\u003c").replace('\u{2028}',"\\u2028").replace('\u{2029}',"\\u2029");
    let viewer=include_str!("../../station_ui/rendering/three/offline-replay.js");
    Ok(format!(r#"if(!globalThis.GSLoadReplay3D){{
const assets={assets};let loading;const urls=[];
const moduleURL=text=>{{const url=URL.createObjectURL(new Blob([text],{{type:'text/javascript'}}));urls.push(url);return url;}};
globalThis.GSReplay3DLicense=assets.license;
globalThis.GSLoadReplay3D=()=>loading||=(async()=>{{
 const core=moduleURL(assets.core);
 const three=moduleURL(assets.three.replaceAll("'./three.core.js'",JSON.stringify(core)));
 const scene=moduleURL(assets.scene);
 const software=moduleURL(assets.software.replaceAll("'./three.module.js'",JSON.stringify(three)));
 const [T,visuals,fallback]=await Promise.all([import(three),import(scene),import(software)]);
 return {{T,...visuals,SoftwareRenderer:fallback.SoftwareRenderer}};
}})();
addEventListener('pagehide',()=>{{for(const url of urls)URL.revokeObjectURL(url);}},{{once:true}});
}}
{viewer}"#))
}

fn portable_replay_html(value: &serde_json::Value) -> Result<String, String> {
    let data=serde_json::to_string(value).map_err(|e|e.to_string())?.replace('<', "\\u003c").replace('\u{2028}',"\\u2028").replace('\u{2029}',"\\u2029");
    let viewer=offline_replay_script()?;
    Ok(format!(r#"<!doctype html><html lang="en"><meta charset="utf-8"><meta name="viewport" content="width=device-width,initial-scale=1"><title>Saved vehicle replay</title><style>body{{font:16px system-ui;background:#101827;color:#f1f5f9;margin:20px;max-width:1100px}}button,select,input{{font:inherit;min-height:44px;margin:4px}}button,select{{background:#1e293b;color:white;border:1px solid #64748b;border-radius:6px;padding:6px 12px}}label{{display:block}}#import{{box-sizing:border-box;max-width:calc(100% - 8px)}}svg text{{font-family:system-ui}}</style><h1>Saved vehicle replay</h1><label>Import another portable JSON replay <input id="import" type="file" accept=".json"></label><p id="status" role="status"></p><main id="replay"></main><script type="application/json" id="recording">{data}</script><script>{viewer}
let player;function load(text){{const next=GSOfflineReplay.validate(text);player?.dispose();player=GSOfflineReplay.mount(document.getElementById('replay'),next);document.getElementById('status').textContent='Offline file · no backend connection';}}load(document.getElementById('recording').textContent);document.getElementById('import').onchange=async e=>{{try{{const file=e.target.files[0];if(!file)return;if(file.size>GSOfflineReplay.MAX_BYTES)throw Error('Replay exceeds 64 MiB.');load(await file.text());}}catch(error){{document.getElementById('status').textContent='Import failed: '+error.message;}}}};</script></html>"#))
}

#[cfg(test)]
#[test]
fn portable_html_embeds_config_and_escapes_script_end_from_saved_labels() {
    let value=serde_json::json!({"format":"seds-replay","schema":{"unit":"psi"},"valve_mappings":[{"label":"</script><script>bad()"}]});
    let html=portable_replay_html(&value).unwrap();
    assert!(!html.contains("</script><script>bad()"));
    assert!(html.contains("\\u003c/script>"));
    assert!(html.contains("Offline file · no backend connection"));
    assert!(html.contains("const MAX_BYTES"));
}

#[cfg(test)]
#[test]
fn offline_three_assets_and_license_are_embedded_without_recording_supplied_code() {
    let script=offline_replay_script().unwrap();
    let encoded=script.split("const assets=").nth(1).unwrap().split(";let loading;").next().unwrap();
    let assets:serde_json::Value=serde_json::from_str(encoded).unwrap();
    assert!(assets["license"].as_str().unwrap().contains("Permission is hereby granted"));
    assert!(assets["core"].as_str().unwrap().contains("class Vector3"));
    assert!(assets["scene"].as_str().unwrap().contains("ReplayMotorStand"));
    assert!(script.contains("URL.createObjectURL(new Blob"));
    assert!(!script.contains("</script>"));
    let html=portable_replay_html(&serde_json::json!({"label":"</script>\u{2028}\u{2029}"})).unwrap();
    assert!(!html.contains("</script>\u{2028}"));
    assert!(!html.contains('\u{2028}'));
    assert!(!html.contains('\u{2029}'));
    assert!(html.contains("\\u003c/script>\\u2028\\u2029"));
}

#[cfg(test)]
#[test]
fn actual_offline_html_export_contains_roundtrip_configuration_and_saved_samples() {
    let data:serde_json::Value=serde_json::from_str(include_str!("../../tests/fixtures/offline-flight.json")).unwrap();
    let html=portable_replay_html(&data).unwrap();
    let encoded=html.split("id=\"recording\">").nth(1).unwrap().split("</script>").next().unwrap();
    let restored:serde_json::Value=serde_json::from_str(encoded).unwrap();
    assert_eq!(restored,data);
    if let Some(path)=std::env::var_os("GS_REPLAY_HTML_PROOF") { std::fs::write(path,html).unwrap(); }
}
