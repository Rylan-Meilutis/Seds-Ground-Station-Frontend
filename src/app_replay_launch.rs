// "Open replay file" on the connection page and the connection-failure card
// (docs/offline-preview.md, "Opening a replay file").
//
// A picked, dropped or recent file goes through the same hand-off as a file
// opened from another app (`crate::opened_file`): the app root then moves to
// the replay view, which opens it directly. Demo mode is never entered on the
// way. Nothing here runs until the user picks a file, and it is only mounted
// where no station is connected.

/// Reads a picked or dropped file and hands it to the replay view. A file
/// too large to open is refused here, before it is read.
fn launch_replay_file(file: dioxus::html::FileData, mut status: Signal<String>) {
    spawn(async move {
        let name = file.name();
        if file.size() as usize > crate::opened_file::MAX_BYTES {
            status.set(crate::opened_file::too_large(&name));
            return;
        }
        status.set(format!("Opening {name}…"));
        match file.read_bytes().await {
            Ok(bytes) => {
                status.set(String::new());
                let path = telemetry_dashboard::offline_preview::picked_path(&file);
                crate::opened_file::offer_from(name, bytes.to_vec(), path);
            }
            Err(error) => status.set(format!("Could not read {name}: {error}")),
        }
    });
}

/// Handlers that let a whole screen take a dropped replay file.
fn replay_drop_target(status: Signal<String>) -> (impl FnMut(DragEvent), impl FnMut(DragEvent)) {
    (
        move |event: DragEvent| event.prevent_default(),
        move |event: DragEvent| {
            event.prevent_default();
            if let Some(file) = dioxus::html::HasFileData::files(&**event).into_iter().next() {
                launch_replay_file(file, status);
            }
        },
    )
}

/// Phones have no drag and drop; desktops and browsers say they take it.
const DROP_HINT: bool = !cfg!(any(target_os = "android", target_os = "ios"));

#[component]
/// The **Open replay file** button (a file input styled as a button) and,
/// on desktops, the recently opened replays.
fn OpenReplayFile(status: Signal<String>) -> Element {
    let theme = shell_theme();
    #[cfg(not(any(target_arch = "wasm32", target_os = "android", target_os = "ios")))]
    let recent = use_hook(crate::recent_replays::list);
    #[cfg(any(target_arch = "wasm32", target_os = "android", target_os = "ios"))]
    let recent: Vec<(String, std::path::PathBuf)> = Vec::new();
    #[cfg(not(any(target_arch = "wasm32", target_os = "android", target_os = "ios")))]
    let recent: Vec<(String, std::path::PathBuf)> = recent.into_iter().map(|entry| (entry.name, entry.path)).collect();
    rsx! {
        style { {include_str!("open_replay.css")} }
        div { class: "gs26-open-replay",
            label {
                class: "gs26-open-replay-button",
                style: shell_button_alt_style(&theme),
                // No `accept` filter: phone pickers cannot match the .gsreplay extension; the content is checked on open.
                input { r#type: "file", "aria-label": "Open replay file",
                    onchange: move |event| {
                        if let Some(file) = dioxus::html::HasFileData::files(&**event).into_iter().next() {
                            launch_replay_file(file, status);
                        }
                    }
                }
                "Open replay file"
            }
            if DROP_HINT {
                span { class: "gs26-open-replay-hint", style: "color:{theme.text_muted};", "or drop a .gsreplay file here" }
            }
        }
        if !recent.is_empty() {
            div { class: "gs26-recent-replays", role: "group", "aria-label": "Recent replays",
                span { style: "font-size:13px; color:{theme.text_muted};", "Recent replays" }
                for (name, path) in recent {
                    button {
                        key: "{path.display()}",
                        r#type: "button",
                        class: "gs26-recent-replay",
                        style: shell_button_alt_style(&theme),
                        title: "{path.display()}",
                        onclick: move |_| reopen_recent(path.clone(), status),
                        "{name}"
                    }
                }
            }
        }
        if !status().is_empty() {
            div { role: "status", class: "gs26-open-replay-status", style: "font-size:13px; color:{theme.warning_text}; overflow-wrap:anywhere;", "{status}" }
        }
    }
}

/// Opens a recent replay again from its path, off the UI thread.
#[cfg(not(any(target_arch = "wasm32", target_os = "android", target_os = "ios")))]
fn reopen_recent(path: std::path::PathBuf, mut status: Signal<String>) {
    if !path.is_file() {
        crate::recent_replays::forget(&path);
        status.set(format!("{} is no longer there.", path.display()));
        return;
    }
    status.set(String::new());
    std::thread::spawn(move || crate::opened_file::offer_path(None, &path));
}

#[cfg(any(target_arch = "wasm32", target_os = "android", target_os = "ios"))]
fn reopen_recent(_path: std::path::PathBuf, _status: Signal<String>) {}

/// Where the replay view's leave button goes from a replay opened with no
/// station: back to connection setup.
fn replay_exit_label() -> &'static str {
    if cfg!(target_arch = "wasm32") { "Leave replay" } else { "Back to connection" }
}
