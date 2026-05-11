#[cfg(any(target_arch = "wasm32", target_os = "android", target_os = "ios"))]
use base64::Engine;

#[cfg(not(target_arch = "wasm32"))]
const LOG_FILE_NAME: &str = "frontend.log";
#[cfg(not(target_arch = "wasm32"))]
const LOG_ROTATED_FILE_COUNT: usize = 3;
#[cfg(not(target_arch = "wasm32"))]
const LOG_MAX_TOTAL_BYTES: u64 = 100 * 1024 * 1024;
#[cfg(not(target_arch = "wasm32"))]
const LOG_MAX_ACTIVE_BYTES: u64 = LOG_MAX_TOTAL_BYTES / (LOG_ROTATED_FILE_COUNT as u64 + 1);

#[cfg(target_arch = "wasm32")]
const WEB_LOG_STORAGE_KEY: &str = "gs26_debug_logs_v1";
#[cfg(target_arch = "wasm32")]
const WEB_LOG_MAX_BYTES: usize = 1_000_000;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct LogArtifact {
    pub id: String,
    pub label: String,
}

#[cfg(not(target_arch = "wasm32"))]
fn fallback_storage_base_dir() -> std::path::PathBuf {
    dirs::data_local_dir()
        .or_else(dirs::data_dir)
        .unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| ".".into()))
}

#[cfg(all(not(target_arch = "wasm32"), target_os = "android"))]
fn native_storage_base_dir() -> std::path::PathBuf {
    crate::native_storage::android_files_dir().unwrap_or_else(fallback_storage_base_dir)
}

#[cfg(all(not(target_arch = "wasm32"), not(target_os = "android")))]
fn native_storage_base_dir() -> std::path::PathBuf {
    fallback_storage_base_dir()
}

#[cfg(not(target_arch = "wasm32"))]
fn log_directory_path() -> std::path::PathBuf {
    if let Ok(path) = std::env::var("GS26_FRONTEND_LOG")
        && !path.trim().is_empty()
    {
        let candidate = std::path::PathBuf::from(path);
        if let Some(parent) = candidate.parent() {
            return parent.to_path_buf();
        }
    }
    native_storage_base_dir().join("gs26").join("logs")
}

#[cfg(not(target_arch = "wasm32"))]
fn log_file_path() -> std::path::PathBuf {
    if let Ok(path) = std::env::var("GS26_FRONTEND_LOG")
        && !path.trim().is_empty()
    {
        return std::path::PathBuf::from(path);
    }
    log_directory_path().join(LOG_FILE_NAME)
}

#[cfg(not(target_arch = "wasm32"))]
fn rotated_log_path(index: usize) -> std::path::PathBuf {
    let mut name = LOG_FILE_NAME.to_string();
    name.push('.');
    name.push_str(&index.to_string());
    log_directory_path().join(name)
}

#[cfg(not(target_arch = "wasm32"))]
fn rotate_native_logs_if_needed(next_line_len: usize) -> Result<(), String> {
    use std::fs;

    let path = log_file_path();
    let current_len = fs::metadata(&path).map(|meta| meta.len()).unwrap_or(0);
    if current_len.saturating_add(next_line_len as u64) <= LOG_MAX_ACTIVE_BYTES {
        return Ok(());
    }

    let oldest = rotated_log_path(LOG_ROTATED_FILE_COUNT);
    let _ = fs::remove_file(&oldest);
    for idx in (1..LOG_ROTATED_FILE_COUNT).rev() {
        let src = rotated_log_path(idx);
        let dst = rotated_log_path(idx + 1);
        if src.exists() {
            let _ = fs::rename(src, dst);
        }
    }
    if path.exists() {
        let _ = fs::rename(&path, rotated_log_path(1));
    }
    Ok(())
}

#[cfg(not(target_arch = "wasm32"))]
fn timestamp_ms() -> u128 {
    use std::time::{SystemTime, UNIX_EPOCH};

    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0)
}

#[cfg(target_arch = "wasm32")]
fn timestamp_ms() -> u128 {
    js_sys::Date::now().max(0.0) as u128
}

#[cfg(not(target_arch = "wasm32"))]
fn formatted_log_line(message: &str) -> String {
    let thread_id = format!("{:?}", std::thread::current().id());
    format!("[{}][thread={thread_id}] {message}\n", timestamp_ms())
}

#[cfg(target_arch = "wasm32")]
fn formatted_log_line(message: &str) -> String {
    format!("[{}] {message}\n", timestamp_ms())
}

#[cfg(target_arch = "wasm32")]
fn web_log_storage() -> Option<web_sys::Storage> {
    let window = web_sys::window()?;
    window.local_storage().ok().flatten()
}

#[cfg(target_arch = "wasm32")]
fn trim_web_log(mut logs: String) -> String {
    if logs.len() <= WEB_LOG_MAX_BYTES {
        return logs;
    }
    let overflow = logs.len().saturating_sub(WEB_LOG_MAX_BYTES);
    let mut trim_idx = overflow;
    while !logs.is_char_boundary(trim_idx) && trim_idx < logs.len() {
        trim_idx += 1;
    }
    if trim_idx >= logs.len() {
        logs.clear();
        return logs;
    }
    logs.split_off(trim_idx)
}

#[cfg(not(target_arch = "wasm32"))]
fn native_log_artifact_path(id: &str) -> Option<std::path::PathBuf> {
    if id == LOG_FILE_NAME {
        return Some(log_file_path());
    }
    let suffix = id.strip_prefix("frontend.log.")?;
    let index = suffix.parse::<usize>().ok()?;
    if (1..=LOG_ROTATED_FILE_COUNT).contains(&index) {
        Some(rotated_log_path(index))
    } else {
        None
    }
}

#[cfg(not(target_arch = "wasm32"))]
#[allow(dead_code)]
fn read_log_artifact_bytes(id: &str) -> Result<Vec<u8>, String> {
    let path = native_log_artifact_path(id).ok_or_else(|| "unknown log artifact".to_string())?;
    std::fs::read(path).map_err(|e| e.to_string())
}

pub(crate) fn list_log_artifacts() -> Vec<LogArtifact> {
    #[cfg(target_arch = "wasm32")]
    {
        return vec![LogArtifact {
            id: "browser-storage".to_string(),
            label: "browser-storage.log".to_string(),
        }];
    }

    #[cfg(not(target_arch = "wasm32"))]
    {
        let mut artifacts = Vec::new();
        let active = log_file_path();
        if active.exists() {
            artifacts.push(LogArtifact {
                id: LOG_FILE_NAME.to_string(),
                label: LOG_FILE_NAME.to_string(),
            });
        }
        for idx in 1..=LOG_ROTATED_FILE_COUNT {
            let path = rotated_log_path(idx);
            if path.exists() {
                let name = format!("{LOG_FILE_NAME}.{idx}");
                artifacts.push(LogArtifact {
                    id: name.clone(),
                    label: name,
                });
            }
        }
        artifacts
    }
}

pub(crate) fn append(message: &str) {
    let line = formatted_log_line(message);

    #[cfg(target_arch = "wasm32")]
    {
        web_sys::console::log_1(&message.into());
        if let Some(storage) = web_log_storage() {
            let existing = storage
                .get_item(WEB_LOG_STORAGE_KEY)
                .ok()
                .flatten()
                .unwrap_or_default();
            let mut combined = existing;
            combined.push_str(&line);
            let _ = storage.set_item(WEB_LOG_STORAGE_KEY, &trim_web_log(combined));
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    {
        use std::fs::{OpenOptions, create_dir_all};
        use std::io::Write;

        let path = log_file_path();
        if let Some(parent) = path.parent() {
            let _ = create_dir_all(parent);
        }
        let _ = rotate_native_logs_if_needed(line.len());
        if let Ok(mut file) = OpenOptions::new().create(true).append(true).open(&path) {
            let _ = file.write_all(line.as_bytes());
        }
        println!("{message}");
    }
}

#[cfg(target_arch = "wasm32")]
pub(crate) fn clear_logs() -> Result<(), String> {
    let Some(storage) = web_log_storage() else {
        return Err("browser storage is unavailable".to_string());
    };
    storage
        .remove_item(WEB_LOG_STORAGE_KEY)
        .map_err(|_| "failed to clear browser log storage".to_string())?;
    Ok(())
}

#[cfg(not(target_arch = "wasm32"))]
pub(crate) fn clear_logs() -> Result<(), String> {
    use std::fs;

    let _ = fs::remove_file(log_file_path());
    for idx in 1..=LOG_ROTATED_FILE_COUNT {
        let _ = fs::remove_file(rotated_log_path(idx));
    }
    Ok(())
}

#[cfg(target_arch = "wasm32")]
pub(crate) fn read_logs() -> Result<String, String> {
    let Some(storage) = web_log_storage() else {
        return Err("browser storage is unavailable".to_string());
    };
    Ok(storage
        .get_item(WEB_LOG_STORAGE_KEY)
        .ok()
        .flatten()
        .unwrap_or_default())
}

#[cfg(not(target_arch = "wasm32"))]
#[allow(dead_code)]
pub(crate) fn read_logs() -> Result<String, String> {
    use std::fs;

    let mut out = String::new();
    for idx in (1..=LOG_ROTATED_FILE_COUNT).rev() {
        let path = rotated_log_path(idx);
        if let Ok(chunk) = fs::read_to_string(path) {
            out.push_str(&chunk);
        }
    }
    if let Ok(chunk) = fs::read_to_string(log_file_path()) {
        out.push_str(&chunk);
    }
    Ok(out)
}

#[cfg(all(not(target_arch = "wasm32"), not(any(target_os = "android", target_os = "ios"))))]
pub(crate) fn export_log_artifact_for_user(id: &str) -> Result<(), String> {
    use std::process::Command;

    let path = native_log_artifact_path(id).ok_or_else(|| "unknown log artifact".to_string())?;
    if !path.exists() {
        return Err("selected log file does not exist".to_string());
    }

    #[cfg(target_os = "macos")]
    let mut command = {
        let mut cmd = Command::new("open");
        cmd.arg("-R");
        cmd.arg(&path);
        cmd
    };

    #[cfg(target_os = "linux")]
    let mut command = {
        let mut cmd = Command::new("xdg-open");
        let target = path.parent().unwrap_or(path.as_path());
        cmd.arg(target);
        cmd
    };

    #[cfg(target_os = "windows")]
    let mut command = {
        let mut cmd = Command::new("explorer");
        cmd.arg("/select,");
        cmd.arg(&path);
        cmd
    };

    command.spawn().map_err(|e| e.to_string())?;
    Ok(())
}

#[cfg(target_arch = "wasm32")]
pub(crate) fn export_log_artifact_for_user(id: &str) -> Result<(), String> {
    if id != "browser-storage" {
        return Err("unknown log artifact".to_string());
    }
    let logs = read_logs()?;
    let payload = base64::engine::general_purpose::STANDARD.encode(logs.as_bytes());
    let filename = format!("gs26-debug-log-{}.txt", timestamp_ms());
    let script = format!(
        r#"
        (function() {{
          try {{
            const bytes = atob({payload:?});
            const buffer = new Uint8Array(bytes.length);
            for (let i = 0; i < bytes.length; i += 1) {{
              buffer[i] = bytes.charCodeAt(i);
            }}
            const blob = new Blob([buffer], {{ type: "text/plain;charset=utf-8" }});
            const url = URL.createObjectURL(blob);
            const a = document.createElement("a");
            a.href = url;
            a.download = {filename:?};
            document.body.appendChild(a);
            a.click();
            a.remove();
            setTimeout(() => URL.revokeObjectURL(url), 1000);
          }} catch (e) {{
            console.error("GS26 log download failed", e);
          }}
        }})();
        "#
    );
    let _ = js_sys::eval(&script);
    Ok(())
}

#[cfg(any(target_os = "android", target_os = "ios"))]
pub(crate) fn export_log_artifact_for_user(id: &str) -> Result<(), String> {
    let logs = String::from_utf8(read_log_artifact_bytes(id)?).map_err(|e| e.to_string())?;
    let payload = base64::engine::general_purpose::STANDARD.encode(logs.as_bytes());
    let filename = id.to_string();
    crate::telemetry_dashboard::js_eval(&format!(
        r#"
        (function() {{
          try {{
            const decoded = atob({payload:?});
            const buffer = new Uint8Array(decoded.length);
            for (let i = 0; i < decoded.length; i += 1) {{
              buffer[i] = decoded.charCodeAt(i);
            }}
            const file = new File([buffer], {filename:?}, {{ type: "text/plain;charset=utf-8" }});
            if (navigator.canShare && navigator.canShare({{ files: [file] }}) && navigator.share) {{
              navigator.share({{
                title: "GS26 Debug Logs",
                files: [file]
              }}).catch((err) => console.error("GS26 log share failed", err));
              return;
            }}
            if (navigator.share) {{
              navigator.share({{
                title: "GS26 Debug Logs",
                text: decoded
              }}).catch((err) => console.error("GS26 log share failed", err));
              return;
            }}
            console.error("GS26 log share unsupported");
          }} catch (e) {{
            console.error("GS26 log share failed", e);
          }}
        }})();
        "#
    ));
    Ok(())
}
