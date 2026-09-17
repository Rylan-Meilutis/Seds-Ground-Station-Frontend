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
    let url = format!("{}{}", UrlConfig::base_http().trim_end_matches('/'), path);
    let filename = format!("{}.csv", id.trim_end_matches(".db"));
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
    use std::io::Write;
    let path = download_path(id)?;
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
    let target = dir.join(format!("{}-{stamp}.csv", id.trim_end_matches(".db")));
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
