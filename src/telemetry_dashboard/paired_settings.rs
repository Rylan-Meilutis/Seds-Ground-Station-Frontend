//! Review paired calibration and saved mission settings with per-document access.
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
        "mission/fill_targets" => "Fill targets",
        "mission/gse_limits" => "Sequence limits",
        "mission/saved_flight_setup" => "Saved flight setup",
        "mission/auto_zero_preference" => "Automatic zeroing",
        "mission/saved_station_positions" => "Saved station positions",
        _ => "Saved settings",
    }
}
fn is_calibration(key: &str) -> bool {
    matches!(
        key,
        "calibration/ch1" | "calibration/kg50" | "calibration/iadc" | "calibration/full_mass_kg"
    )
}
fn is_mission(key: &str) -> bool {
    matches!(
        key,
        "mission/fill_targets"
            | "mission/gse_limits"
            | "mission/saved_flight_setup"
            | "mission/auto_zero_preference"
            | "mission/saved_station_positions"
    )
}
fn can_resolve(key: &str, calibration_edit: bool, send_commands: bool, admin: bool) -> bool {
    if key == "mission/saved_station_positions" {
        return admin;
    }
    (is_calibration(key) && calibration_edit) || (is_mission(key) && send_commands)
}
fn can_view(key: &str, calibration_view: bool) -> bool {
    is_mission(key) || (is_calibration(key) && calibration_view)
}
fn short(value: &str) -> String {
    let mut chars = value.chars();
    let mut text: String = chars.by_ref().take(80).collect();
    if chars.next().is_some() {
        text.push('…');
    }
    text
}
fn number(value: &Value, decimals: usize, suffix: &str) -> String {
    value
        .as_f64()
        .filter(|n| n.is_finite())
        .map(|n| format!("{n:.decimals$}{suffix}"))
        .unwrap_or_else(|| "Default".into())
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
    match key {
        "mission/fill_targets" => {
            return vec![
                (
                    "Input".into(),
                    if value["fill_source"] == "kg1000_absolute" {
                        "1000 kg · absolute"
                    } else {
                        "50 kg"
                    }
                    .into(),
                ),
                (
                    "Nitrous".into(),
                    format!(
                        "{} · {}",
                        number(&value["nitrous"]["target_mass_kg"], 2, " kg"),
                        number(&value["nitrous"]["target_pressure_psi"], 0, " psi")
                    ),
                ),
                (
                    "Nitrogen".into(),
                    format!(
                        "{} · {}",
                        number(&value["nitrogen"]["target_mass_kg"], 2, " kg"),
                        number(&value["nitrogen"]["target_pressure_psi"], 0, " psi")
                    ),
                ),
            ];
        }
        "mission/gse_limits" => {
            return vec![
                (
                    "Nitrogen target".into(),
                    number(&value["nitrogen_target_psi"], 1, " psi"),
                ),
                (
                    "Pressure step".into(),
                    number(&value["pressure_step_psi"], 1, " psi"),
                ),
                (
                    "Pressure ceiling".into(),
                    number(&value["pressure_ceiling_psi"], 1, " psi"),
                ),
                (
                    "Empty-tank offset".into(),
                    number(&value["maximum_zero_offset_psi"], 1, " psi"),
                ),
            ];
        }
        "mission/auto_zero_preference" => {
            return vec![(
                "Automatic zeroing".into(),
                if value["enabled"] == true {
                    "Enabled"
                } else {
                    "Disabled"
                }
                .into(),
            )];
        }
        "mission/saved_station_positions" => {
            return [("ground_station", "GroundStation"), ("rrls", "RRLS")]
                .into_iter()
                .map(|(key, label)| {
                    let coordinate = &value[key];
                    let text = coordinate["latitude"]
                        .as_f64()
                        .zip(coordinate["longitude"].as_f64())
                        .filter(|(lat, lon)| lat.is_finite() && lon.is_finite())
                        .map(|(lat, lon)| format!("{lat:.6}, {lon:.6}"))
                        .unwrap_or_else(|| "Not saved".into());
                    (label.into(), text)
                })
                .collect();
        }
        "mission/saved_flight_setup" => {
            let selected = value["selected_profile_id"].as_str().unwrap_or("");
            let profiles = value["profiles"].as_array();
            let profile = profiles.and_then(|profiles| {
                profiles
                    .iter()
                    .find(|profile| profile["id"].as_str() == Some(selected))
            });
            return vec![
                (
                    "Profile".into(),
                    short(
                        profile
                            .and_then(|profile| profile["label"].as_str())
                            .unwrap_or(selected),
                    ),
                ),
                (
                    "Wind level".into(),
                    profile
                        .and_then(|profile| profile["wind_level"].as_u64())
                        .map(|level| level.to_string())
                        .unwrap_or_else(|| "Not set".into()),
                ),
                (
                    "Saved profiles".into(),
                    profiles.map_or(0, Vec::len).to_string(),
                ),
            ];
        }
        _ => {}
    }
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
pub(super) fn PairedSettings(theme: ThemeConfig) -> Element {
    let mut status = use_signal(Status::default);
    let mut documents = use_signal(BTreeMap::<String, Document>::new);
    let mut message = use_signal(String::new);
    let mut poll_error = use_signal(String::new);
    let mut busy = use_signal(|| false);
    let mut details = use_signal(|| None::<(String, Value)>);
    use_future(move || async move {
        loop {
            if !crate::auth::can_view_data() {
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
                            Err(_) => poll_error.set("Could not load settings conflicts".into()),
                        }
                    } else {
                        documents.set(BTreeMap::new());
                    }
                    status.set(current);
                }
                Err(_) => {
                    poll_error.set("Shared settings are unavailable on this station".into());
                }
            }
            sleep().await;
        }
    });
    let calibration_edit = crate::auth::can_edit_calibration();
    let calibration_view = crate::auth::can_view_calibration();
    let send_commands = crate::auth::can_view_actions();
    let admin = crate::auth::is_admin();
    let state = status.read().clone();
    let entries = documents.read().clone();
    let detail = details.read().clone();
    let style = format!(
        "--paired-text:{};--paired-soft:{};--paired-border:{};--paired-panel:{};",
        theme.text_primary, theme.text_soft, theme.border, theme.panel_background
    );
    rsx! {
        style {{include_str!("paired_settings.css")}}
        section {class:"gs26-paired-settings",style,"aria-label":"Paired settings",
            header {h3 {"Paired settings"}
                span {class:"paired-state",
                    if !state.enabled {"Not configured"} else if state.link_available {"Connected"} else {"Waiting for peer"}
                }
            }
            if !state.enabled {p {"Pair GroundStation and RRLS to share saved settings."}}
            if state.pending>0 {p {"{state.pending} incoming changes pending"}}
            if state.waiting_for_idle {p {"Return to Idle and clear qualification to apply pending settings."}}
            if state.error.is_some() {p {"Synchronization paused. Local settings are retained."}}
            if !poll_error.read().is_empty() {p {role:"status","{poll_error}"}}
            if !message.read().is_empty() {p {role:"status","{message}"}}
            for (key,document) in entries.into_iter().filter(|(key, _)| can_view(key, calibration_view)) {
                article {key:"{key}",class:"paired-conflict",
                    h4 {"{label(&key)}"}
                    p {"Choose the version to keep."}
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
                                        if can_resolve(&key, calibration_edit, send_commands, admin) {button {disabled:*busy.read()||request.is_none(),onclick:move |_| {
                                            let Some(request)=request.as_ref() else {return};
                                            let request=Resolution {key:request.key.clone(),expected:request.expected.clone(),value:request.value.clone()};
                                            busy.set(true);message.set(String::new());
                                            spawn(async move {
                                                match http_post_json::<_,Resolved>("/api/mission-settings/resolve",&request).await {
                                                    Ok(reply) if reply.resolved=>message.set("Choice saved. Settings apply when safe.".into()),
                                                    _=>message.set("Settings changed elsewhere. Review the latest versions.".into()),
                                                }
                                                busy.set(false);
                                            });
                                        },"Use {source}"}}
                                        button {class:"paired-info","aria-label":"Inspect saved values",onclick:move |_|details.set(Some((detail_title.clone(),detail_value.clone()))),"Details"}
                                    }
                                }}
                            }
                        }
                    }
                }
            }
            if let Some((title,value))=detail {
                div {class:"paired-modal-shade",role:"presentation",
                    section {class:"paired-modal",role:"dialog","aria-modal":"true","aria-label":"Saved values",
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
    fn causal_heads_preserve_exact_numeric_values_through_json() {
        for bits in 0x3d000000..0x3d001000u32 {
            let number = serde_json::to_value(f32::from_bits(bits)).unwrap();
            let document = Document {
                heads: vec![Revision {
                    clock: BTreeMap::from([("gs".into(), 2), ("rrls".into(), 1)]),
                    value: number,
                }],
            };
            let encoded = serde_json::to_string(&document).unwrap();
            let decoded: Document = serde_json::from_str(&encoded).unwrap();
            assert_eq!(decoded, document, "bit pattern {bits}");
        }
    }
    #[test]
    fn mission_permissions_do_not_grant_calibration_access_or_reverse() {
        assert!(can_resolve("mission/fill_targets", false, true, false));
        assert!(!can_resolve("mission/fill_targets", true, false, false));
        assert!(can_resolve("calibration/ch1", true, false, false));
        assert!(!can_resolve("calibration/ch1", false, true, false));
        assert!(!can_view("calibration/ch1", false));
        assert!(can_view("mission/gse_limits", false));
        assert!(!can_resolve("mission/unknown", true, true, true));
        assert!(can_view("mission/saved_station_positions", false));
        assert!(!can_resolve(
            "mission/saved_station_positions",
            true,
            true,
            false
        ));
        assert!(can_resolve(
            "mission/saved_station_positions",
            false,
            false,
            true
        ));
        assert_eq!(
            summary(
                "mission/saved_station_positions",
                &json!({
                    "ground_station":{"latitude":43.0,"longitude":-78.8},"rrls":null
                })
            ),
            vec![
                ("GroundStation".into(), "43.000000, -78.800000".into()),
                ("RRLS".into(), "Not saved".into())
            ]
        );
        assert!(!can_view("unknown", true));
    }
    #[test]
    fn mission_summaries_show_saved_settings_instead_of_calibration_fields() {
        let fill = summary(
            "mission/fill_targets",
            &json!({"fill_source":"kg1000_absolute", "nitrous":{"target_mass_kg":21.,"target_pressure_psi":745.}, "nitrogen":{"target_mass_kg":10.,"target_pressure_psi":120.}}),
        );
        assert!(fill.contains(&("Nitrous".into(), "21.00 kg · 745 psi".into())));
        let limits = summary(
            "mission/gse_limits",
            &json!({"nitrogen_target_psi":120.,"pressure_step_psi":20.,"pressure_ceiling_psi":null,"maximum_zero_offset_psi":20.}),
        );
        assert!(limits.contains(&("Pressure ceiling".into(), "Default".into())));
        let setup = summary(
            "mission/saved_flight_setup",
            &json!({"selected_profile_id":"wind2","profiles":[{"id":"wind2","label":"Wind 2","wind_level":2}]}),
        );
        assert!(setup.contains(&("Profile".into(), "Wind 2".into())));
        assert_eq!(
            summary("mission/auto_zero_preference", &json!({"enabled":true})),
            vec![("Automatic zeroing".into(), "Enabled".into())]
        );
        assert!(short(&"😀".repeat(100)).chars().count() <= 81);
        for rows in [fill, limits, setup] {
            assert!(rows.len() <= 4);
            assert!(!rows.iter().any(|(name, _)| name == "Fit"));
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
