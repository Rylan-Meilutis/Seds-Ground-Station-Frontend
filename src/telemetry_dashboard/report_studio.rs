use super::recording_download::{ExportOwner, saved_json, download_csv_path_owned};
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
    #[serde(default)]
    label: String,
    count: usize,
    #[serde(default)]
    finite_count:Option<usize>,
    #[serde(default)]
    sender:String,
    #[serde(default)]
    data_type:String,
    #[serde(default)]
    unit:Option<String>,
    #[serde(default)]
    value_kind:String,
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
fn visible(channel:&Channel,focus:&str)->bool {
    let focus=focus.trim().to_lowercase();
    focus.is_empty() || format!("{} {} {} {} {}",channel.label,channel.key,channel.sender,channel.data_type,channel.unit.as_deref().unwrap_or("")).to_lowercase().contains(&focus)
}
fn value_kind_label(kind:&str)->&str {
    match kind {"recorded_derived"=>"Derived saved value","recorded_raw_input"=>"Raw saved input","recorded_state"=>"Saved state",_=>"Saved value; provenance unclassified"}
}

fn report_filename(format:&str)->String {
    match format {"daq_csv"=>"telemetry-daq-sd.csv".into(),"daq_tsv"=>"telemetry-daq-sd.tsv".into(),_=>format!("telemetry-report.{format}")}
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
    let mut focus=use_signal(String::new);
    let mut query_epoch = use_signal(|| 1u64);
    let mut owner_state = use_signal(|| None::<std::rc::Rc<ExportOwner>>);
    let mut current_recording = use_signal(|| recording.clone());
    let alive = use_hook(|| std::sync::Arc::new(std::sync::atomic::AtomicBool::new(true)));
    let drop_alive = alive.clone();
    use_drop(move || drop_alive.store(false, std::sync::atomic::Ordering::Release));
    use_effect(use_reactive((&recording,), move |(next,)| {
        if *current_recording.peek() != next {
            current_recording.set(next);
            query_epoch.set(query_epoch.with_peek(|epoch| epoch.saturating_add(1)));
            owner_state.set(None); preview.set(None); selected.set(BTreeSet::new());
            loaded_window.set(None); from.set(String::new()); to.set(String::new());
            start.set(0); end.set(0); busy.set(false);
        }
    }));
    use_future(move || async move {
        loop {
            #[cfg(target_arch = "wasm32")]
            gloo_timers::future::TimeoutFuture::new(200).await;
            #[cfg(not(target_arch = "wasm32"))]
            tokio::time::sleep(std::time::Duration::from_millis(200)).await;
            let retired = owner_state.peek().as_ref().is_some_and(|owner| !owner.is_current());
            if retired {
                owner_state.set(None); preview.set(None); selected.set(BTreeSet::new());
                loaded_window.set(None); from.set(String::new()); to.set(String::new());
                start.set(0); end.set(0); busy.set(false);
                status.set("Recording or station session changed. Load the selected recording again.".into());
            }
        }
    });
    #[cfg(all(test, not(target_arch = "wasm32")))]
    renderer_tests::capture(renderer_tests::Handle { preview, selected, loaded_window, owner_state, busy });
    let load_recording = recording.clone();
    let load = move |_| {
        if load_recording.is_empty() { status.set("Select a recording first.".into()); return; }
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
        let expected_query = (*query_epoch.peek()).saturating_add(1);
        query_epoch.set(expected_query);
        let original_recording = load_recording.clone();
        let alive = alive.clone();
        let owner = match ExportOwner::capture_selected(move || {
            alive.load(std::sync::atomic::Ordering::Acquire) && expected_query != u64::MAX
                && query_epoch.try_peek().is_ok_and(|epoch| *epoch == expected_query)
                && current_recording.try_peek().is_ok_and(|current| *current == original_recording)
        }) { Ok(owner) => owner, Err(error) => {status.set(error);return;} };
        owner_state.set(Some(owner.clone())); preview.set(None); selected.set(BTreeSet::new());
        loaded_window.set(None);
        let path = report_path(&load_recording, "preview", window, None);
        busy.set(true);
        status.set("Loading recorded data…".into());
        spawn(async move {
            let result = saved_json::<Preview>(&path, owner.clone()).await;
            if !owner.is_current() { return; }
            match result {
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
        h3 {"2. Select data and export"}
        p { "Recording: {recording}" }
        p {"Select a recording above, then load its graphs. Preview retains peaks but is downsampled; downloads contain the selected recorded samples without display filtering or recalibration."}
        div {label {"Optional from (UTC) " input {r#type:"datetime-local",value:from(),oninput:move |e|from.set(e.value())}} label {"to (UTC, exclusive) " input {r#type:"datetime-local",value:to(),oninput:move |e|to.set(e.value())}}}
        button {disabled:busy() || recording.is_empty(),onclick:load,"Load / refresh graphs"}
        if let Some(data)=preview.read().as_ref() {
            p {"{data.recorded_rows} recorded rows · {utc(data.start_ms)} to {utc(data.end_ms)}"}
            label {input {r#type:"checkbox",checked:all_time(),onchange:move |e|all_time.set(e.checked())} "Export entire loaded time span"}
            if !all_time() {
                label {"Window start: {utc(start())}" input {r#type:"range",style:"width:100%;",min:"{data.start_ms}",max:"{data.end_ms-1}",step:"1",value:"{start}",oninput:move |e|if let Ok(v)=e.value().parse::<i64>(){start.set(v.min(end()-1));}}}
                label {"Window end (exclusive): {utc(end())}" input {r#type:"range",style:"width:100%;",min:"{data.start_ms+1}",max:"{data.end_ms}",step:"1",value:"{end}",oninput:move |e|if let Ok(v)=e.value().parse::<i64>(){end.set(v.max(start()+1));}}}
            }
            small {"Units use recorded SD manifest metadata when present, otherwise this station’s export-time display configuration. Unknown units remain unknown."}
            label {"Focus channels by name, source or units " input {r#type:"search",value:focus(),oninput:move |e|focus.set(e.value())}}
            p {"{selected.read().len()} channels selected. Searching changes the list; use visible channels only to limit the export."}
            div {
                button {onclick:move |_| {if let Some(p)=preview.read().as_ref(){selected.set(p.channels.iter().map(|c|c.key.clone()).collect());}},"Select all channels"}
                button {onclick:move |_| {if let Some(p)=preview.read().as_ref(){selected.set(p.channels.iter().filter(|c|visible(c,&focus())).map(|c|c.key.clone()).collect());}},"Use visible channels only"}
                button {onclick:move |_|selected.set(BTreeSet::new()),"Clear selection"}
            }
            for channel in data.channels.iter().filter(|channel|visible(channel,&focus())) {
                div {key:"{channel.key}",style:"border:1px solid #475569;padding:8px;border-radius:6px;",
                    label {input {r#type:"checkbox",checked:selected.read().contains(&channel.key),onchange:{let key=channel.key.clone();move |e:Event<FormData>|{if e.checked(){selected.write().insert(key.clone());}else{selected.write().remove(&key);}}}} {format!("{} · {} samples",if channel.label.is_empty(){&channel.key}else{&channel.label},channel.count)}}
                    small {style:"display:block;overflow-wrap:anywhere;",
                        {format!("{} / {} · {} · {} · {}",channel.sender,channel.data_type,channel.unit.as_deref().unwrap_or("units unknown"),value_kind_label(&channel.value_kind),channel.finite_count.map(|n|format!("{n} values / {} observations",channel.count)).unwrap_or_else(||format!("{} observations",channel.count)))}
                    }
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
        super::vehicle_tab::ReplayFileExport { recording:recording.clone() }
        label {"File format " select {value:format(),onchange:move |e|format.set(e.value()),option {value:"csv","CSV — data and regression metadata"}option {value:"daq_csv","DAQ SD column layout — CSV"}option {value:"daq_tsv","DAQ SD column layout — TSV"}option {value:"xlsx","Excel — charts, data and regressions"}option {value:"pdf","PDF — graphs, data appendix and regressions"}}}
        if matches!(format().as_str(),"daq_csv"|"daq_tsv") {p {"Uses the DAQ’s thirteen columns. Missing ADC, uptime and sequence fields stay blank. Per-sample metadata preserves original receive/source timestamps; no current calibration is substituted."}}
        button {disabled:busy() || preview.read().is_none() || selected.read().is_empty() || owner_state.read().as_ref().is_none_or(|owner| !owner.is_current()),onclick:move |_| {
            let Some(owner) = owner_state.peek().clone() else {status.set("Load this recording first.".into());return;};
            if let Err(error) = owner.check() {status.set(error);return;}
            if *current_recording.peek() != recording {status.set("Load the selected recording again.".into());return;}
            let kind=format();let window=if all_time(){loaded_window()}else{Some((start(),end()))};
            let path=report_path(&recording,&kind,window,Some(selected.read().iter().cloned().collect()));
            busy.set(true);status.set("Preparing export… Large PDFs can take up to two minutes.".into());
            spawn(async move {let result=download_csv_path_owned(&path,&report_filename(&kind),owner.clone()).await;if owner.is_current(){status.set(result.unwrap_or_else(|error|format!("Export failed: {error}")));busy.set(false);}});
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
    #[test]
    fn daq_download_extensions_remain_standard_and_selection_is_exact() {
        assert_eq!(report_filename("daq_csv"),"telemetry-daq-sd.csv");
        assert_eq!(report_filename("daq_tsv"),"telemetry-daq-sd.tsv");
        let path=report_path("groundstation_recording_1.db","daq_tsv",Some((10,20)),Some(vec!["[\"DAQ\",\"KG1000\",0]".into()]));
        assert!(path.contains("%64%61%71%5F%74%73%76"));
        assert!(!path.contains("[\"DAQ\""));
    }

}

#[cfg(all(test, not(target_arch = "wasm32")))]
#[path = "recording_export_renderer_tests.rs"]
mod renderer_tests;
