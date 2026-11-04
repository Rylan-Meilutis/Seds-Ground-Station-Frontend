//! Selected saved-file data only; the existing export owner fences every await.
use super::{ExportOwner, saved_json};
use dioxus::prelude::*;
use serde::Deserialize;
use serde_json::Value;
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

#[derive(Clone, Deserialize)]
struct Snapshot {
    upper_id: i64,
    start_ms: Option<i64>,
    end_ms: Option<i64>,
}
#[derive(Clone, Deserialize)]
struct Page {
    rows: Vec<Value>,
    next_id: i64,
    has_more: bool,
}
fn encoded(value: &str) -> String {
    value.bytes().map(|b| format!("%{b:02X}")).collect()
}
fn description(row: &Value) -> String {
    let Some(d) = row.get("diagnostic") else {
        return "No saved diagnostic payload".into();
    };
    if d.get("context")
        .and_then(|v| v.get("version"))
        .or_else(|| d.get("version"))
        .and_then(Value::as_u64)
        != Some(1)
    {
        return "Unknown saved diagnostic schema; raw payload retained".into();
    }
    match row["data_type"].as_str().unwrap_or("") {
        "REC_SIDE_COUNTERS" => d["totals_decimal"]
            .as_object()
            .map(|v| {
                v.iter()
                    .map(|(k, n)| format!("{k}: {}", n.as_str().unwrap_or("Unknown")))
                    .collect::<Vec<_>>()
                    .join(" · ")
            })
            .unwrap_or_else(|| "Counters unavailable".into()),
        "REC_BOARD_DIE_TEMPERATURE" | "REC_HOST_CPU_TEMPERATURE" => {
            if d["valid"] == true {
                d["centi_c"]
                    .as_i64()
                    .map(|n| {
                        format!(
                            "{:.2} °C · saved sample age {}ms",
                            n as f64 / 100.,
                            d["sample_age_ms"]
                        )
                    })
                    .unwrap_or_else(|| "Unknown temperature".into())
            } else {
                "Unknown / stale / unavailable temperature".into()
            }
        }
        "REC_DIAGNOSTIC_GAP" => format!(
            "Capture gap: {}",
            d["reason"].as_str().unwrap_or("unavailable")
        ),
        "REC_NETWORK_STATE" => match (
            d["topology_valid"].as_bool(),
            resolved_nodes(d),
            d["links"].as_array(),
        ) {
            (Some(true), Some(nodes), Some(links)) => format!(
                "Observed saved topology: {} routers, {} links; inspect exact payload below",
                nodes.len(),
                links.len()
            ),
            _ => "Saved topology unavailable; no empty graph inferred".into(),
        },
        _ => row["values"].to_string(),
    }
}
fn resolved_nodes(d: &Value) -> Option<Vec<Value>> {
    if d["topology_encoding"] != "node-table-v1" {
        return d["nodes"]
            .as_array()
            .filter(|nodes| nodes.iter().all(Value::is_object))
            .cloned();
    }
    let table = d["node_table"].as_array()?;
    let resolve = |refs: &Value| -> Option<Vec<Value>> {
        refs.as_array()?
            .iter()
            .map(|v| {
                table
                    .get(usize::try_from(v.as_u64()?).ok()?)
                    .filter(|v| v.is_object())
                    .cloned()
            })
            .collect()
    };
    let nodes = resolve(&d["nodes"])?;
    for route in d["routes"].as_array()? {
        for announcer in route["announcers"].as_array()? {
            resolve(&announcer["router_refs"])?;
        }
    }
    Some(nodes)
}
fn saved_details(row: &Value, rows: &[Value]) -> String {
    let id = row["diagnostic"]["context"]["snapshot_id"].as_str();
    let contexts: Vec<_> = rows
        .iter()
        .filter(|r| {
            matches!(
                r["data_type"].as_str(),
                Some("REC_NETWORK_STATE" | "REC_DIAGNOSTIC_GAP")
            )
        })
        .map(|r| &r["diagnostic"]["context"])
        .filter(|c| {
            id.is_some()
                && c["snapshot_id"].as_str() == id
                && c["recording_generation_decimal"].is_string()
        })
        .collect();
    let resolved = contexts
        .first()
        .filter(|first| contexts.iter().all(|c| c == *first))
        .copied();
    serde_json::to_string_pretty(&serde_json::json!({"row":row,"resolved_capture_context":resolved,
        "context_resolution":if resolved.is_some(){"resolved"}else{"Unknown / missing or ambiguous saved reference"},
        "resolved_nodes":if row["data_type"]=="REC_NETWORK_STATE"{resolved_nodes(&row["diagnostic"])}else{None}})).unwrap_or_default()
}
fn row_id(row: &Value) -> String {
    row["id"].to_string()
}
fn caption(row: &Value) -> String {
    format!(
        "{} · {} · {} · {}",
        row["timestamp_ms"],
        row["sender_id"],
        row["data_type"],
        description(row)
    )
}
#[component]
pub(super) fn RecordedDiagnostics(recording: String) -> Element {
    let mut rows = use_signal(Vec::<Value>::new);
    let mut status = use_signal(String::new);
    let mut from = use_signal(String::new);
    let mut busy = use_signal(|| false);
    let mut revision = use_signal(|| 0u64);
    let mut owner_state = use_signal(|| None::<std::rc::Rc<ExportOwner>>);
    let mut current_recording = use_signal(|| recording.clone());
    use_effect(use_reactive((&recording,), move |(next,)| {
        if *current_recording.peek() != next {
            current_recording.set(next);
            revision.set(revision.with_peek(|n| n.saturating_add(1)));
            rows.set(Vec::new());
            owner_state.set(None);
            from.set(String::new());
            busy.set(false);
        }
    }));
    let alive = use_hook(|| Arc::new(AtomicBool::new(true)));
    let drop_alive = alive.clone();
    use_drop(move || drop_alive.store(false, Ordering::Release));
    use_future(move || async move {
        loop {
            #[cfg(target_arch = "wasm32")]
            gloo_timers::future::TimeoutFuture::new(150).await;
            #[cfg(not(target_arch = "wasm32"))]
            tokio::time::sleep(std::time::Duration::from_millis(150)).await;
            if owner_state.peek().as_ref().is_some_and(|o| !o.is_current()) {
                rows.set(Vec::new());
                from.set(String::new());
                revision.set(revision.with_peek(|n| n.saturating_add(1)));
                owner_state.set(None);
                busy.set(false);
                status.set(
                    "Recording, login or station changed. Load saved diagnostics again.".into(),
                );
            }
        }
    });
    let load = move |_| {
        let id = recording.clone();
        if let Err(error) = super::download_path(&id) {
            status.set(error);
            return;
        }
        let start = if from.read().trim().is_empty() {
            None
        } else {
            match super::input_utc_ms(&from()) {
                Ok(v) => Some(v),
                Err(e) => {
                    status.set(e);
                    return;
                }
            }
        };
        let ticket = revision().saturating_add(1);
        revision.set(ticket);
        let live = alive.clone();
        let selected_id = id.clone();
        let owner = match ExportOwner::capture_selected(move || {
            ticket != u64::MAX
                && live.load(Ordering::Acquire)
                && revision.try_peek().is_ok_and(|v| *v == ticket)
                && current_recording
                    .try_peek()
                    .is_ok_and(|v| *v == selected_id)
        }) {
            Ok(o) => o,
            Err(e) => {
                status.set(e);
                return;
            }
        };
        owner_state.set(Some(owner.clone()));
        rows.set(Vec::new());
        busy.set(true);
        status.set("Loading saved diagnostics…".into());
        spawn(async move {
            let result=async{
                let snap:Snapshot=saved_json(&format!("/api/recordings/{id}/replay"),owner.clone()).await?;
                let end=snap.end_ms.ok_or("This recording has no saved rows")?;
                let beginning=start.unwrap_or(end.saturating_sub(30_000).max(snap.start_ms.unwrap_or(end)));
                let stop=beginning.checked_add(30_000).ok_or("Invalid time window")?.min(end);
                if beginning>stop{return Err("Choose a saved30-second window within this recording".to_string());}
                let kinds=encoded(r#"["REC_NETWORK_STATE","REC_SIDE_COUNTERS","REC_SIDE_RATES","REC_BOARD_DIE_TEMPERATURE","REC_HOST_CPU_TEMPERATURE","REC_DIAGNOSTIC_GAP"]"#);
                let mut next=0;let mut saved=Vec::new();let mut bytes=0usize;
                loop{
                    let path=format!("/api/recordings/{id}/replay/window?upper_id={}&after_id={next}&start_ms={beginning}&end_ms={stop}&types={kinds}",snap.upper_id);
                    let page:Page=saved_json(&path,owner.clone()).await?;
                    owner.check()?;
                    bytes=bytes.saturating_add(serde_json::to_vec(&page.rows).map_err(|e|e.to_string())?.len());
                    if bytes>12*1024*1024||saved.len()+page.rows.len()>2048{return Err("Saved diagnostic window exceeds viewer bounds; choose a narrower recording. No rows were silently dropped.".into());}
                    saved.extend(page.rows);
                    if !page.has_more {break;}
                    if page.next_id<=next{return Err("Saved diagnostic cursor did not advance".into());}next=page.next_id;
                }
                owner.check()?;Ok::<_,String>(saved)
            }.await;
            if !owner.is_current() {
                return;
            }
            match result {
                Ok(saved) => {
                    status.set(if saved.is_empty() {
                        "No saved diagnostics in this window. Live data was not substituted.".into()
                    } else {
                        format!(
                            "{} saved observations · immutable recording snapshot",
                            saved.len()
                        )
                    });
                    rows.set(saved);
                }
                Err(e) => status.set(e),
            }
            busy.set(false);
        });
    };
    rsx! {details{
        summary{"Recorded network & temperatures"}
        p{"Saved30-second windows only. Select temperature/statistic channels above for CSV export. Unknown readings are not zero."}
        label{"Optional window start (UTC) " input{r#type:"datetime-local",value:"{from}",oninput:move|e|{revision.set(revision.with_peek(|n|n.saturating_add(1)));rows.set(Vec::new());owner_state.set(None);busy.set(false);from.set(e.value());}}}
        button{disabled:busy(),onclick:load,"Load saved diagnostics"}
        p{role:"status","{status}"}
        div{style:"max-height:32rem;overflow:auto;",
            for row in rows.read().iter(){
                details{key:"{row_id(row)}",
                    summary{"{caption(row)}"}
                    pre{style:"white-space:pre-wrap;overflow-wrap:anywhere;","{saved_details(row, &rows.read())}"}
                }
            }
        }
    }}
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn recorded_diagnostic_labels_never_turn_missing_graph_or_sensor_into_zero() {
        let unknown = serde_json::json!({"data_type":"REC_NETWORK_STATE","diagnostic":{"context":{"version":1}}});
        assert!(description(&unknown).contains("unavailable"));
        let cpu = serde_json::json!({"data_type":"REC_HOST_CPU_TEMPERATURE","diagnostic":{"context":{"version":1},"valid":false,"centi_c":null}});
        assert!(description(&cpu).contains("Unknown"));
        let counters = serde_json::json!({"data_type":"REC_SIDE_COUNTERS","diagnostic":{"context":{"version":1,"snapshot_id":"fixture:1:0"},"totals_decimal":{"tx_packets":u64::MAX.to_string()}}});
        assert!(description(&counters).contains(&u64::MAX.to_string()));
    }
    #[test]
    fn recording_saved_references_resolve_only_exact_unambiguous_contexts() {
        let full = serde_json::json!({"data_type":"REC_DIAGNOSTIC_GAP","diagnostic":{"context":{"version":1,"snapshot_id":"p:1:2","recording_generation_decimal":"1"}}});
        let row = serde_json::json!({"data_type":"REC_SIDE_COUNTERS","diagnostic":{"context":{"version":1,"snapshot_id":"p:1:2"}}});
        assert!(
            saved_details(&row, std::slice::from_ref(&full)).contains("resolved_capture_context")
        );
        let details: Value =
            serde_json::from_str(&saved_details(&row, std::slice::from_ref(&full))).unwrap();
        assert_eq!(details["context_resolution"], "resolved");
        let mut conflicting = full.clone();
        conflicting["diagnostic"]["context"]["recording_generation_decimal"] = "2".into();
        let details: Value =
            serde_json::from_str(&saved_details(&row, &[full, conflicting])).unwrap();
        assert!(details["resolved_capture_context"].is_null());
        assert!(resolved_nodes(&serde_json::json!({"topology_encoding":"node-table-v1","nodes":[1],"node_table":[{"sender":"FC"}],"routes":[]})).is_none());
    }
    #[test]
    fn actual_rust_html_embeds_adversarial_saved_diagnostics_without_executable_metadata() {
        let mut data: Value =
            serde_json::from_str(include_str!("../../tests/fixtures/offline-flight.json")).unwrap();
        let at = data["samples"]
            .as_array()
            .unwrap()
            .iter()
            .map(|s| s["timestamp_ms"].as_i64().unwrap())
            .min()
            .unwrap();
        let evil =
            "</script><img src=x onerror=globalThis.diagnosticInjected=true>\u{2028}\u{2029}";
        let entries = [
            serde_json::json!({"id":900001,"timestamp_ms":at,"source_timestamp_ms":null,"sender_id":evil,"data_type":"REC_NETWORK_STATE","values":[1,1,1],"phase":null,"diagnostic":{"context":{"version":1,"snapshot_id":"fixture:1:0","recording_generation_decimal":"1"},"topology_valid":true,"topology_encoding":"node-table-v1","nodes":[0],"node_table":[{"sender":evil}],"routes":[],"links":[{"source":"GS","target":evil}],"__proto__":{"diagnosticPolluted":true}}}),
            serde_json::json!({"id":900002,"timestamp_ms":at,"source_timestamp_ms":at-1,"sender_id":"GS/radio","data_type":"REC_SIDE_COUNTERS","values":[123],"phase":null,"diagnostic":{"context":{"version":1,"snapshot_id":"fixture:1:0"},"totals_decimal":{"tx_packets":u64::MAX.to_string()}}}),
            serde_json::json!({"id":900003,"timestamp_ms":at,"source_timestamp_ms":null,"sender_id":"FC","data_type":"REC_BOARD_DIE_TEMPERATURE","values":[-1.23],"phase":null,"diagnostic":{"context":{"version":1,"snapshot_id":"fixture:1:0"},"valid":true,"centi_c":-123,"sample_age_ms":0}}),
            serde_json::json!({"id":900004,"timestamp_ms":at+20000,"source_timestamp_ms":null,"sender_id":"GS","data_type":"ORDINARY_SAVED_DATA","values":[1],"phase":null}),
        ];
        data["samples"].as_array_mut().unwrap().extend(entries);
        let html = super::super::portable_replay_html(&data).unwrap();
        assert!(!html.contains(evil));
        assert!(!html.contains('\u{2028}'));
        assert!(!html.contains('\u{2029}'));
        assert!(html.contains("GSRecordedDiagnostics.mount"));
        let embedded = html
            .split("id=\"recording\">")
            .nth(1)
            .unwrap()
            .split("</script>")
            .next()
            .unwrap();
        assert_eq!(serde_json::from_str::<Value>(embedded).unwrap(), data);
        if let Some(path) = std::env::var_os("GS_DIAGNOSTIC_HTML_PROOF") {
            use std::io::Write;
            let mut file = std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(path)
                .unwrap();
            file.write_all(html.as_bytes()).unwrap();
        }
    }
}
