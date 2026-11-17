//! Per-board config sync state (GET /api/board-config/status): whether each
//! board holds the station's operator-set config after a reboot or reflash,
//! and the operator's choice when a board holds a value the station never sent.
use super::layout::ThemeConfig;
use crate::auth;
use dioxus::prelude::*;
use serde_json::{Value, json};

#[derive(Clone, Debug, PartialEq)]
pub(super) struct ItemRow {
    pub item: String,
    pub label: String,
    pub state: String,
    pub message: String,
    pub board_value: Option<String>,
    pub conflict: bool,
    pub can_keep_board: bool,
}
#[derive(Clone, Debug, PartialEq)]
pub(super) struct BoardRow {
    pub board: String,
    pub state: String,
    pub items: Vec<ItemRow>,
}

pub(super) fn rows(value: &Value) -> Vec<BoardRow> {
    let text = |v: &Value| v.as_str().unwrap_or_default().to_string();
    value["boards"]
        .as_array()
        .map(|boards| {
            boards
                .iter()
                .map(|board| BoardRow {
                    board: text(&board["board"]),
                    state: text(&board["state"]),
                    items: board["items"]
                        .as_array()
                        .map(|items| {
                            items
                                .iter()
                                .map(|item| ItemRow {
                                    item: text(&item["item"]),
                                    label: text(&item["label"]),
                                    state: text(&item["state"]),
                                    message: text(&item["message"]),
                                    board_value: item["board_value"].as_str().map(str::to_string),
                                    conflict: item["conflict"] == true,
                                    can_keep_board: item["can_keep_board"] == true,
                                })
                                .collect()
                        })
                        .unwrap_or_default(),
                })
                .collect()
        })
        .unwrap_or_default()
}

/// Operator-facing label for a sync state.
pub(super) fn state_label(state: &str) -> &'static str {
    match state {
        "synced" => "Synced",
        "resyncing" => "Resyncing",
        "out_of_sync" => "Config out of sync",
        "conflict" => "Conflict",
        _ => "No readback",
    }
}

fn state_colors(theme: &ThemeConfig, state: &str) -> (String, String) {
    match state {
        "synced" => (theme.success_text.clone(), theme.panel_background.clone()),
        "resyncing" => (theme.info_text.clone(), theme.info_background.clone()),
        "out_of_sync" | "conflict" => (theme.warning_text.clone(), theme.warning_background.clone()),
        _ => (theme.text_muted.clone(), theme.panel_background.clone()),
    }
}

#[component]
pub(super) fn BoardConfigSyncPanel(theme: ThemeConfig) -> Element {
    let mut status = use_signal(Vec::<BoardRow>::new);
    let mut error = use_signal(|| None::<String>);
    let mut note = use_signal(String::new);
    let mut busy = use_signal(|| false);
    use_future(move || async move {
        loop {
            let owner = super::recording_download::ExportOwner::capture_ordinary_selected(|| true);
            let next = match owner {
                Ok(owner) => super::recording_download::saved_json::<Value>("/api/board-config/status", owner).await,
                Err(e) => Err(e),
            };
            match next {
                Ok(value) => {
                    status.set(rows(&value));
                    error.set(None);
                }
                Err(e) => error.set(Some(e)),
            }
            #[cfg(target_arch = "wasm32")]
            gloo_timers::future::TimeoutFuture::new(2_000).await;
            #[cfg(not(target_arch = "wasm32"))]
            tokio::time::sleep(std::time::Duration::from_millis(2_000)).await;
        }
    });
    let resolve = move |item: String, keep: &'static str| {
        if *busy.peek() {
            return;
        }
        busy.set(true);
        spawn(async move {
            let owner = super::recording_download::ExportOwner::capture_ordinary_selected(|| true);
            let result = super::station_settings_request::post_captured::<_, Value>(
                "/api/board-config/resolve",
                &json!({"item": item, "keep": keep}),
                owner,
            )
            .await;
            match result {
                Ok(value) => {
                    note.set(value["message"].as_str().unwrap_or("Resolved.").to_string());
                    if value["status"].is_object() {
                        status.set(rows(&value["status"]));
                    }
                }
                Err(e) => note.set(format!("Could not resolve: {e}")),
            }
            busy.set(false);
        });
    };
    let can_command = auth::can_send_command("RetractPlumbing");
    let boards = status.read().clone();
    rsx! {
        section { aria_label: "Board config sync",
            style: "display:grid;gap:8px;margin-top:12px;min-width:0;color:{theme.text_primary};",
            div { style: "font-size:14px;color:{theme.text_muted};", "Board config sync" }
            if let Some(e) = error.read().clone() {
                p { role: "status", style: "margin:0;color:{theme.text_muted};overflow-wrap:anywhere;", "Config sync status unavailable: {e}" }
            }
            for board in boards {
                div { key: "{board.board}", style: "display:grid;gap:6px;padding:10px;border:1px solid {theme.border_soft};border-radius:10px;min-width:0;",
                    div { style: "display:flex;align-items:center;gap:8px;flex-wrap:wrap;",
                        strong { "{board.board}" }
                        {
                            let (fg, bg) = state_colors(&theme, &board.state);
                            rsx! { span { style: "padding:2px 10px;border-radius:999px;font-size:0.85rem;color:{fg};background:{bg};", "{state_label(&board.state)}" } }
                        }
                    }
                    for row in board.items {
                        div { key: "{row.item}", style: "display:grid;gap:4px;min-width:0;padding-left:8px;border-left:3px solid {theme.border_soft};",
                            div { style: "display:flex;gap:8px;flex-wrap:wrap;align-items:baseline;",
                                span { "{row.label}" }
                                {
                                    let (fg, _) = state_colors(&theme, &row.state);
                                    rsx! { span { style: "font-size:0.85rem;color:{fg};", "{state_label(&row.state)}" } }
                                }
                            }
                            if !row.message.is_empty() {
                                p { style: "margin:0;color:{theme.text_muted};font-size:0.85rem;overflow-wrap:anywhere;", "{row.message}" }
                            }
                            if row.conflict && can_command {
                                div { style: "display:flex;gap:8px;flex-wrap:wrap;",
                                    button { r#type: "button", disabled: *busy.read(),
                                        style: "min-height:40px;padding:6px 12px;white-space:normal;",
                                        onclick: { let item = row.item.clone(); let mut resolve = resolve; move |_| resolve(item.clone(), "station") },
                                        "Keep the station's value"
                                    }
                                    if row.can_keep_board {
                                        button { r#type: "button", disabled: *busy.read(),
                                            style: "min-height:40px;padding:6px 12px;white-space:normal;",
                                            onclick: { let item = row.item.clone(); let mut resolve = resolve; move |_| resolve(item.clone(), "board") },
                                            "Keep the board's value"
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
            if !note.read().is_empty() {
                p { role: "status", style: "margin:0;overflow-wrap:anywhere;", "{note}" }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn status_rows_and_labels_follow_the_backend_contract() {
        let value = json!({"boards":[
            {"board":"DAQ","state":"resyncing","items":[
                {"item":"daq_calibration","label":"Load-cell calibration","state":"resyncing","message":"Re-applying.","board_value":null,"conflict":false,"can_keep_board":false},
                {"item":"daq_kg50_input","label":"50 kg input","state":"synced","message":"","board_value":null,"conflict":false,"can_keep_board":false}]},
            {"board":"AB","state":"conflict","items":[
                {"item":"ab_fill_drive","label":"Fill-line drive selection","state":"conflict","message":"never sent","board_value":"the auxiliary drive","conflict":true,"can_keep_board":true}]}
        ]});
        let rows = rows(&value);
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].items.len(), 2);
        assert!(rows[1].items[0].conflict && rows[1].items[0].can_keep_board);
        assert_eq!(rows[1].items[0].board_value.as_deref(), Some("the auxiliary drive"));
        for (state, label) in [("synced", "Synced"), ("resyncing", "Resyncing"), ("out_of_sync", "Config out of sync"),
                               ("conflict", "Conflict"), ("unknown", "No readback")] {
            assert_eq!(state_label(state), label);
        }
        assert!(super::rows(&json!({})).is_empty());
    }
}
