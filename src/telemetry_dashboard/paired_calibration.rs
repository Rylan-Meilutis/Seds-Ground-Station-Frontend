//! Paired calibration conflicts stay in Settings; choosing a revision never
//! starts a sequence or changes a board's abort/qualification latch.
use super::{http_get_json, http_post_json, layout::ThemeConfig};
use dioxus::prelude::*;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;

#[derive(Clone, Default, Deserialize, PartialEq)]
struct Status {
    enabled: bool,
    link_available: bool,
    pending: usize,
    conflicts: usize,
    waiting_for_idle: bool,
    error: Option<String>,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
struct Revision {
    clock: BTreeMap<String, u64>,
    value: Value,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
struct Document {
    heads: Vec<Revision>,
}
#[derive(Serialize)]
struct Resolution {
    key: String,
    expected: Document,
    value: Value,
}
#[derive(Deserialize)]
struct Resolved {
    resolved: bool,
}
fn resolution(key: &str, document: &Document, index: usize) -> Result<Resolution, &'static str> {
    if document.heads.len() < 2 {
        return Err("This conflict has already changed");
    }
    let head = document
        .heads
        .get(index)
        .ok_or("This revision is no longer available")?;
    Ok(Resolution {
        key: key.into(),
        expected: document.clone(),
        value: head.value.clone(),
    })
}
fn label(key: &str) -> &str {
    match key {
        "calibration/ch1" => "1000 kg load cell",
        "calibration/kg50" => "50 kg load cell",
        "calibration/iadc" => "Tank pressure",
        "calibration/full_mass_kg" => "Full-mass reference",
        _ => "Calibration",
    }
}
fn station_label(id: &str) -> String {
    match id.to_ascii_lowercase().as_str() {
        "gs" | "primary" | "groundstation" => "GroundStation".into(),
        "rrls" => "RRLS".into(),
        _ => id.replace(['_', '-'], " "),
    }
}
fn origin(document: &Document, index: usize) -> String {
    let Some(head) = document.heads.get(index) else {
        return "Saved revision".into();
    };
    let actors = head
        .clock
        .iter()
        .filter(|(actor, number)| {
            document
                .heads
                .iter()
                .enumerate()
                .filter(|(i, _)| *i != index)
                .all(|(_, other)| **number > other.clock.get(*actor).copied().unwrap_or(0))
        })
        .map(|(actor, _)| station_label(actor))
        .collect::<Vec<_>>();
    if actors.is_empty() {
        "Saved revision".into()
    } else {
        actors.join(" + ")
    }
}
fn summary(key: &str, value: &Value) -> Vec<(String, String)> {
    if key == "calibration/full_mass_kg" {
        return vec![(
            "Mass".into(),
            value
                .as_f64()
                .map(|n| format!("{n:.2} kg"))
                .unwrap_or_else(|| "Not set".into()),
        )];
    }
    let active = value.get("active").unwrap_or(value);
    let calibration = &active["calibration"];
    let mut rows = Vec::new();
    if let Some(input) = value.get("input").and_then(Value::as_str) {
        rows.push((
            "Input".into(),
            match input {
                "amp2" => "AMP2".into(),
                "p6_differential" => "P6 differential".into(),
                _ => input.into(),
            },
        ));
    }
    let fit = calibration["fit"]["type"]
        .as_str()
        .map(str::to_owned)
        .or_else(|| calibration["linear"]["m"].as_f64().map(|_| "Linear".into()))
        .unwrap_or_else(|| "Not calibrated".into());
    rows.push(("Fit".into(), fit));
    for (name, key) in [("Slope", "m"), ("Offset", "b")] {
        if let Some(number) = calibration["linear"][key].as_f64() {
            rows.push((name.into(), format!("{number:.6}")));
        }
    }
    rows.push((
        "Points".into(),
        calibration["points"]
            .as_array()
            .map_or(0, Vec::len)
            .to_string(),
    ));
    rows
}
async fn sleep() {
    #[cfg(target_arch = "wasm32")]
    gloo_timers::future::TimeoutFuture::new(2000).await;
    #[cfg(not(target_arch = "wasm32"))]
    tokio::time::sleep(std::time::Duration::from_secs(2)).await;
}
#[component]
pub(super) fn PairedCalibrationSettings(theme: ThemeConfig) -> Element {
    let mut status = use_signal(Status::default);
    let mut documents = use_signal(BTreeMap::<String, Document>::new);
    let mut message = use_signal(String::new);
    let mut poll_error = use_signal(String::new);
    let mut busy = use_signal(|| false);
    let mut details = use_signal(|| None::<(String, Value)>);
    use_future(move || async move {
        loop {
            if !crate::auth::can_view_calibration() {
                break;
            }
            match http_get_json::<Status>("/api/mission-settings").await {
                Ok(current) => {
                    poll_error.set(String::new());
                    if current.conflicts > 0 {
                        match http_get_json::<BTreeMap<String, Document>>(
                            "/api/mission-settings/conflicts",
                        )
                        .await
                        {
                            Ok(items) => documents.set(items),
                            Err(_) => poll_error.set("Could not load calibration conflicts".into()),
                        }
                    } else {
                        documents.set(BTreeMap::new());
                    }
                    status.set(current);
                }
                Err(_) => {
                    poll_error.set("Shared calibration is unavailable on this station".into());
                }
            }
            sleep().await;
        }
    });
    let can_edit = crate::auth::can_edit_calibration();
    let state = status.read().clone();
    let entries = documents.read().clone();
    let detail = details.read().clone();
    let style = format!(
        "--paired-text:{};--paired-soft:{};--paired-border:{};--paired-panel:{};",
        theme.text_primary, theme.text_soft, theme.border, theme.panel_background
    );
    rsx! {
        style {{include_str!("paired_calibration.css")}}
        section {class:"gs26-paired-calibration",style,"aria-label":"Paired calibration",
            header {h3 {"Paired calibration"}
                span {class:"paired-state",
                    if !state.enabled {"Not configured"} else if state.link_available {"Connected"} else {"Waiting for peer"}
                }
            }
            if !state.enabled {p {"Pair GroundStation and RRLS to share calibration."}}
            if state.pending>0 {p {"{state.pending} incoming changes pending"}}
            if state.waiting_for_idle {p {"Return to Idle and clear qualification to apply pending calibration."}}
            if state.error.is_some() {p {"Synchronization paused. Local calibration is retained."}}
            if !poll_error.read().is_empty() {p {role:"status","{poll_error}"}}
            if !message.read().is_empty() {p {role:"status","{message}"}}
            for (key,document) in entries {
                article {key:"{key}",class:"paired-conflict",
                    h4 {"{label(&key)}"}
                    p {"Choose the calibration to keep."}
                    div {class:"paired-versions",
                        for (index,head) in document.heads.iter().enumerate() {
                            {
                                let source=origin(&document,index);
                                let rows=summary(&key,&head.value);
                                let request=resolution(&key,&document,index).ok();
                                let detail_title=format!("{} · {}",label(&key),source);
                                let detail_value=head.value.clone();
                                rsx! {div {class:"paired-version",key:"{index}",
                                    strong {"{source}"}
                                    dl {for (name,value) in rows {dt {"{name}"}dd {"{value}"}}}
                                    div {class:"paired-actions",
                                        if can_edit {button {disabled:*busy.read()||request.is_none(),onclick:move |_| {
                                            let Some(request)=request.as_ref() else {return};
                                            let request=Resolution {key:request.key.clone(),expected:request.expected.clone(),value:request.value.clone()};
                                            busy.set(true);message.set(String::new());
                                            spawn(async move {
                                                match http_post_json::<_,Resolved>("/api/mission-settings/resolve",&request).await {
                                                    Ok(reply) if reply.resolved=>message.set("Choice saved. Calibration applies when safe.".into()),
                                                    _=>message.set("Calibration changed elsewhere. Review the latest versions.".into()),
                                                }
                                                busy.set(false);
                                            });
                                        },"Use {source}"}}
                                        button {class:"paired-info","aria-label":"Inspect calibration values",onclick:move |_|details.set(Some((detail_title.clone(),detail_value.clone()))),"Details"}
                                    }
                                }}
                            }
                        }
                    }
                }
            }
            if let Some((title,value))=detail {
                div {class:"paired-modal-shade",role:"presentation",
                    section {class:"paired-modal",role:"dialog","aria-modal":"true","aria-label":"Calibration values",
                        onkeydown:move |event| {if event.key()==Key::Escape {details.set(None);}},
                        header {h3 {"{title}"}button {autofocus:true,onclick:move |_|details.set(None),"Close"}}
                        pre {"{serde_json::to_string_pretty(&value).unwrap_or_default()}"}
                    }
                }
            }
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    fn conflict() -> Document {
        Document {
            heads: vec![
                Revision {
                    clock: BTreeMap::from([("gs".into(), 2), ("rrls".into(), 1)]),
                    value: json!(20.0),
                },
                Revision {
                    clock: BTreeMap::from([("gs".into(), 1), ("rrls".into(), 2)]),
                    value: json!(21.0),
                },
            ],
        }
    }
    #[test]
    fn resolution_retains_exact_reviewed_heads_and_selected_value() {
        let document = conflict();
        let request = resolution("calibration/full_mass_kg", &document, 1).unwrap();
        assert_eq!(request.expected, document);
        assert_eq!(request.value, json!(21.0));
        assert!(resolution("x", &document, 4).is_err());
        assert!(
            resolution(
                "x",
                &Document {
                    heads: vec![document.heads[0].clone()]
                },
                0
            )
            .is_err()
        );
    }
    #[test]
    fn concurrent_versions_name_the_station_whose_causal_clock_advanced() {
        let document = conflict();
        assert_eq!(origin(&document, 0), "GroundStation");
        assert_eq!(origin(&document, 1), "RRLS");
        assert_eq!(station_label("recovery-pi"), "recovery pi");
    }
    #[test]
    fn summaries_are_short_and_raw_json_stays_in_details() {
        assert_eq!(
            summary("calibration/full_mass_kg", &json!(21.0)),
            vec![("Mass".into(), "21.00 kg".into())]
        );
        let rows = summary(
            "calibration/kg50",
            &json!({"input":"p6_differential","active":{"calibration":{"points":[],"linear":{"m":2.0,"b":3.0}}}}),
        );
        assert!(rows.contains(&("Input".into(), "P6 differential".into())));
        assert!(rows.len() <= 5);
    }
}
