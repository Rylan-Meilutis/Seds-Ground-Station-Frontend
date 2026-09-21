use super::{http_get_json, recording_download::download_csv_path};
use dioxus::prelude::*;
use serde::Deserialize;
use std::collections::BTreeSet;

#[derive(Clone, Deserialize, PartialEq)]
struct Preview {
    start_ms: i64,
    end_ms: i64,
    recorded_rows: usize,
    channels: Vec<Channel>,
}
#[derive(Clone, Deserialize, PartialEq)]
struct Channel {
    key: String,
    count: usize,
    points: Vec<(i64, Option<f64>)>,
    regression: serde_json::Value,
}

fn report_path(
    recording: &str,
    format: &str,
    window: Option<(i64, i64)>,
    channels: Option<Vec<String>>,
) -> String {
    let mut value = serde_json::json!({"recording":recording,"format":format,"channels":channels});
    if let Some((start, end)) = window {
        value["start_ms"] = start.into();
        value["end_ms"] = end.into();
    }
    let encoded: String = value
        .to_string()
        .bytes()
        .map(|b| format!("%{b:02X}"))
        .collect();
    format!("/api/recordings/report?request={encoded}")
}
fn utc(ms: i64) -> String {
    time::OffsetDateTime::from_unix_timestamp_nanos(ms as i128 * 1_000_000)
        .ok()
        .map(|v| format!("{} UTC", v))
        .unwrap_or_else(|| ms.to_string())
}
fn trace(channel: &Channel, start: i64, end: i64) -> String {
    let values: Vec<f64> = channel
        .points
        .iter()
        .filter_map(|p| p.1)
        .filter(|v| v.is_finite())
        .collect();
    let lo = values.iter().copied().fold(f64::INFINITY, f64::min);
    let hi = values.iter().copied().fold(f64::NEG_INFINITY, f64::max);
    let span = (hi - lo).max(1e-12);
    let mut out = String::new();
    let mut connected = false;
    for (t, v) in &channel.points {
        if let Some(v) = v.filter(|v| v.is_finite()) {
            let x = (*t - start) as f64 / (end - start).max(1) as f64 * 1000.;
            let y = 95. - (v - lo) / span * 90.;
            out.push_str(&format!(
                "{} {x:.2} {y:.2} ",
                if connected { "L" } else { "M" }
            ));
            connected = true;
        } else {
            connected = false;
        }
    }
    out
}

#[component]
pub(super) fn ReportStudio(recording: String) -> Element {
    let mut preview = use_signal(|| None::<Preview>);
    let mut selected = use_signal(BTreeSet::<String>::new);
    let mut start = use_signal(|| 0i64);
    let mut end = use_signal(|| 0i64);
    let mut all_time = use_signal(|| true);
    let mut from = use_signal(String::new);
    let mut to = use_signal(String::new);
    let mut loaded_window = use_signal(|| None::<(i64, i64)>);
    let mut busy = use_signal(|| false);
    let mut status = use_signal(String::new);
    let mut format = use_signal(|| "csv".to_string());
    let load_recording = recording.clone();
    let mut load = move |_| {
        let window = if from.read().is_empty() && to.read().is_empty() {
            None
        } else {
            match (
                super::recording_download::input_utc_ms(&from()),
                super::recording_download::input_utc_ms(&to()),
            ) {
                (Ok(a), Ok(b)) if b > a => Some((a, b)),
                _ => {
                    status.set("Choose valid UTC start/end times with end after start, or clear both to load all data.".into());
                    return;
                }
            }
        };
        let path = report_path(&load_recording, "preview", window, None);
        busy.set(true);
        status.set("Loading recorded data…".into());
        spawn(async move {
            match http_get_json::<Preview>(&path).await {
                Ok(next) => {
                    loaded_window.set(window);
                    start.set(next.start_ms);
                    end.set(next.end_ms);
                    selected.set(next.channels.iter().map(|c| c.key.clone()).collect());
                    preview.set(Some(next));
                    status.set(String::new());
                }
                Err(e) => status.set(format!("Preview failed: {e}")),
            }
            busy.set(false);
        });
    };
    rsx! {section {style:"margin-top:20px;display:grid;gap:10px;",
        h3 {"Preview and export"}
        p {"Select a recording above, then load its graphs. Preview retains peaks but is downsampled; downloads contain the selected recorded samples without display filtering or recalibration."}
        div {label {"Optional from (UTC) " input {r#type:"datetime-local",value:from(),oninput:move |e|from.set(e.value())}} label {"to (UTC, exclusive) " input {r#type:"datetime-local",value:to(),oninput:move |e|to.set(e.value())}}}
        button {disabled:busy(),onclick:move |e|load(e),"Load / refresh graphs"}
        if let Some(data)=preview.read().as_ref() {
            p {"{data.recorded_rows} recorded rows · {utc(data.start_ms)} to {utc(data.end_ms)}"}
            label {input {r#type:"checkbox",checked:all_time(),onchange:move |e|all_time.set(e.checked())} "Export entire loaded time span"}
            if !all_time() {
                label {"Window start: {utc(start())}" input {r#type:"range",style:"width:100%;",min:"{data.start_ms}",max:"{data.end_ms-1}",step:"1",value:"{start}",oninput:move |e|if let Ok(v)=e.value().parse::<i64>(){start.set(v.min(end()-1));}}}
                label {"Window end (exclusive): {utc(end())}" input {r#type:"range",style:"width:100%;",min:"{data.start_ms+1}",max:"{data.end_ms}",step:"1",value:"{end}",oninput:move |e|if let Ok(v)=e.value().parse::<i64>(){end.set(v.max(start()+1));}}}
            }
            div {
                button {onclick:move |_| {if let Some(p)=preview.read().as_ref(){selected.set(p.channels.iter().map(|c|c.key.clone()).collect());}},"Select all channels"}
                button {onclick:move |_|selected.set(BTreeSet::new()),"Clear selection"}
            }
            for channel in &data.channels {
                div {key:"{channel.key}",style:"border:1px solid #475569;padding:8px;border-radius:6px;",
                    label {input {r#type:"checkbox",checked:selected.read().contains(&channel.key),onchange:{let key=channel.key.clone();move |e:Event<FormData>|{if e.checked(){selected.write().insert(key.clone());}else{selected.write().remove(&key);}}}} "{channel.key} · {channel.count} samples"}
                    if selected.read().contains(&channel.key) {
                        svg {view_box:"0 0 1000 100",style:"width:100%;height:110px;background:#0f172a;",role:"img","aria-label":format!("Recorded channel {}",channel.key),
                            path {d:trace(channel,data.start_ms,data.end_ms),stroke:"#38bdf8",stroke_width:"1",fill:"none"}
                            if !all_time() {
                                rect {x:"{(start()-data.start_ms) as f64/(data.end_ms-data.start_ms) as f64*1000.}",y:"0",width:"{(end()-start()) as f64/(data.end_ms-data.start_ms) as f64*1000.}",height:"100",fill:"#22c55e",opacity:"0.2"}
                            }
                        }
                        details {summary {"Regression / calibration provenance"} pre {style:"white-space:pre-wrap;", "{channel.regression}"}}
                    }
                }
            }
        }
        label {"File format " select {value:format(),onchange:move |e|format.set(e.value()),option {value:"csv","CSV — data and regression metadata"}option {value:"xlsx","Excel — charts, data and regressions"}option {value:"pdf","PDF — graphs, data appendix and regressions"}}}
        button {disabled:busy() || preview.read().is_none() || selected.read().is_empty(),onclick:move |_| {
            let kind=format();let window=if all_time(){loaded_window()}else{Some((start(),end()))};
            let path=report_path(&recording,&kind,window,Some(selected.read().iter().cloned().collect()));
            busy.set(true);status.set("Preparing export… Large PDFs can take up to two minutes.".into());
            spawn(async move {status.set(match download_csv_path(&path,&format!("telemetry-report.{kind}")).await{Ok(s)=>s,Err(e)=>format!("Export failed: {e}")});busy.set(false);});
        },if busy(){"Working…"}else{"Export selected data"}}
        p {role:"status",style:"overflow-wrap:anywhere;","{status}"}
    }}
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn selection_is_encoded_not_interpolated_into_url() {
        let p = report_path(
            "a&b",
            "csv",
            Some((1, 3)),
            Some(vec!["DAQ/KG1000/0".into()]),
        );
        assert!(!p.contains("a&b"));
        assert!(p.starts_with("/api/recordings/report?request=%7B"));
    }
}
