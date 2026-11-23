//! Firmware simulation panel (GS docs/firmware-sim.md): shown only while the station runs against simulated board
//! firmware. Everyone signed in sees the simulated firmware revisions, the bay's pace and the plant; administrators
//! also set the plant inputs (QD, bottles), the conditions (ambient, wind, gusts), calibration masses on the load
//! cells (for the load-cell calibration flow), the IGT dataset on the simulated FC SD card and the injected faults.
//! The GS validates and applies the whole control document (`POST /api/firmware-sim/control`).
use super::layout::ThemeConfig;
use dioxus::prelude::*;
use serde_json::{Value, json};

const VALVES: [&str; 5] = ["nitrogen", "nitrous", "vent", "dump", "pilot"];
const FAULTS: [(&str, &str); 7] = [
    ("dead_tank_pt", "Dead tank PT (open input)"),
    ("dead_chamber_pt", "Dead chamber PT"),
    ("radio_drop", "Radio link drop"),
    ("kg1000_supply_dip", "KG1000 supply-dip clamp"),
    ("kg50_open_lead", "KG50 open lead"),
    ("kg1000_open_lead", "KG1000 open lead"),
    ("fc_sd_missing", "FC SD card missing"),
];

async fn sleep_ms(ms: u32) {
    #[cfg(target_arch = "wasm32")]
    gloo_timers::future::TimeoutFuture::new(ms).await;
    #[cfg(not(target_arch = "wasm32"))]
    tokio::time::sleep(std::time::Duration::from_millis(ms as u64)).await;
}

/// The operator's edit of the control document.
#[derive(Clone, Debug, PartialEq)]
pub(super) struct ControlDraft {
    pub qd_connected: bool,
    pub nitrogen_bottle_open: bool,
    pub nitrous_bottle_open: bool,
    pub ambient_c: String,
    pub wind_mps: String,
    pub wind_from_deg: String,
    pub gust_mps: String,
    pub kg50_kg: String,
    pub kg1000_kg: String,
    pub kg50_bypass: String,
    pub kg1000_bypass: String,
    pub faults: Vec<(String, bool)>,
    /// valve -> "" (follows the board), "open" or "closed"
    pub stuck: Vec<(String, String)>,
    pub igt_dataset: String,
}
fn num(v: Option<&Value>, default: f64) -> String {
    let x = v.and_then(Value::as_f64).unwrap_or(default);
    if x.fract() == 0.0 { format!("{x:.0}") } else { format!("{x}") }
}
impl ControlDraft {
    /// From the current control document and the plant's reported state (for defaults).
    pub fn from_status(control: &Value, plant: &Value) -> Self {
        let p = |k: &str| control.pointer(&format!("/plant/{k}")).and_then(Value::as_bool)
            .or_else(|| plant.pointer(&format!("/supply/{k}")).and_then(Value::as_bool)).unwrap_or(true);
        let c = |k: &str, d: f64| num(control.pointer(&format!("/conditions/{k}")).or_else(|| plant.pointer(&format!("/conditions/{k}"))), d);
        Self {
            qd_connected: p("qd_connected"),
            nitrogen_bottle_open: p("nitrogen_bottle_open"),
            nitrous_bottle_open: p("nitrous_bottle_open"),
            ambient_c: c("ambient_c", 20.0),
            wind_mps: c("wind_mps", 4.0),
            wind_from_deg: c("wind_from_deg", 300.0),
            gust_mps: c("gust_mps", 1.0),
            kg50_kg: num(control.pointer("/loads/kg50_kg"), 0.0),
            kg1000_kg: num(control.pointer("/loads/kg1000_kg"), 0.0),
            kg50_bypass: num(control.pointer("/faults/kg50_bypass"), 0.0),
            kg1000_bypass: num(control.pointer("/faults/kg1000_bypass"), 0.0),
            faults: FAULTS
                .iter()
                .map(|(k, _)| (k.to_string(), control.pointer(&format!("/faults/{k}")).and_then(Value::as_bool).unwrap_or(false)))
                .collect(),
            stuck: VALVES
                .iter()
                .map(|v| (v.to_string(), control.pointer(&format!("/faults/stuck_valves/{v}")).and_then(Value::as_str).unwrap_or("").to_string()))
                .collect(),
            igt_dataset: control.pointer("/igt/dataset").and_then(Value::as_str).unwrap_or("").to_string(),
        }
    }
    /// The control document, or why the draft cannot be sent.
    pub fn body(&self) -> Result<Value, String> {
        let f = |name: &str, s: &str, lo: f64, hi: f64| -> Result<f64, String> {
            let x: f64 = s.trim().parse().map_err(|_| format!("{name} must be a number"))?;
            if (lo..=hi).contains(&x) { Ok(x) } else { Err(format!("{name} must be {lo} to {hi}")) }
        };
        let mut faults = serde_json::Map::new();
        for (k, on) in &self.faults {
            if *on {
                faults.insert(k.clone(), json!(true));
            }
        }
        for (name, s, lo, hi) in [("KG50 bypass", &self.kg50_bypass, 0.0, 1.0), ("KG1000 bypass", &self.kg1000_bypass, 0.0, 1.0)] {
            let x = f(name, s, lo, hi)?;
            if x > 0.0 {
                faults.insert(if name.starts_with("KG50") { "kg50_bypass" } else { "kg1000_bypass" }.into(), json!(x));
            }
        }
        let stuck: serde_json::Map<String, Value> =
            self.stuck.iter().filter(|(_, s)| s == "open" || s == "closed").map(|(v, s)| (v.clone(), json!(s))).collect();
        if !stuck.is_empty() {
            faults.insert("stuck_valves".into(), Value::Object(stuck));
        }
        let mut body = json!({
            "plant": {"qd_connected": self.qd_connected, "nitrogen_bottle_open": self.nitrogen_bottle_open,
                      "nitrous_bottle_open": self.nitrous_bottle_open},
            "conditions": {"ambient_c": f("Ambient", &self.ambient_c, -30.0, 50.0)?, "wind_mps": f("Wind", &self.wind_mps, 0.0, 40.0)?,
                           "wind_from_deg": f("Wind direction", &self.wind_from_deg, 0.0, 360.0)?, "gust_mps": f("Gusts", &self.gust_mps, 0.0, 20.0)?},
            "loads": {"kg50_kg": f("KG50 mass", &self.kg50_kg, 0.0, 60.0)?, "kg1000_kg": f("KG1000 mass", &self.kg1000_kg, 0.0, 1200.0)?},
            "faults": Value::Object(faults),
        });
        if !self.igt_dataset.is_empty() {
            body["igt"] = json!({"dataset": self.igt_dataset});
        }
        Ok(body)
    }
}

fn field_style(theme: &ThemeConfig) -> String {
    format!(
        "padding:6px; border-radius:6px; background:{}; color:{}; border:1px solid {}; font:inherit; width:7em;",
        theme.button_background, theme.text_primary, theme.border
    )
}

#[component]
pub(super) fn FirmwareSimPanel(theme: ThemeConfig) -> Element {
    let mut status = use_signal(|| None::<Value>);
    let mut draft = use_signal(|| None::<ControlDraft>);
    let message = use_signal(String::new);
    use_future(move || async move {
        loop {
            if let Ok(next) = super::http_get_json::<Value>("/api/firmware-sim").await {
                if draft.peek().is_none() && next.get("active").and_then(Value::as_bool) == Some(true) {
                    let plant = next.pointer("/bay/plant").cloned().unwrap_or(Value::Null);
                    draft.set(Some(ControlDraft::from_status(next.get("control").unwrap_or(&Value::Null), &plant)));
                }
                status.set(Some(next));
            }
            sleep_ms(2000).await;
        }
    });
    let s = status.read().clone();
    let Some(s) = s.filter(|s| s.get("active").and_then(Value::as_bool) == Some(true)) else {
        return rsx! {};
    };
    let admin = crate::auth::is_admin();
    rsx! { FirmwareSimView { theme: theme.clone(), status: s, admin, draft, message } }
}

/// Pure view (rendered by tests): status for everyone, controls for administrators.
#[component]
pub(super) fn FirmwareSimView(theme: ThemeConfig, status: Value, admin: bool, draft: Signal<Option<ControlDraft>>, message: Signal<String>) -> Element {
    let bay = status.get("bay").cloned().unwrap_or(Value::Null);
    let plant = bay.get("plant").cloned().unwrap_or(Value::Null);
    let running = status.get("bay_running").and_then(Value::as_bool).unwrap_or(false);
    let pace = bay.get("pace_actual").and_then(Value::as_f64).unwrap_or(0.0);
    let firmware: Vec<(String, String)> = bay
        .get("firmware")
        .and_then(Value::as_object)
        .map(|m| {
            m.iter()
                .map(|(k, v)| (k.clone(), format!("{} {} {}", v.get("repo").and_then(Value::as_str).unwrap_or(""),
                    v.get("short").and_then(Value::as_str).unwrap_or(""), v.get("subject").and_then(Value::as_str).unwrap_or(""))))
                .collect()
        })
        .unwrap_or_default();
    let nodes: Vec<String> = bay.get("nodes").and_then(Value::as_array).map(|a| a.iter().map(|n| format!("{}{}{}",
        n.get("node").and_then(Value::as_str).unwrap_or("?"),
        if n.get("alive").and_then(Value::as_bool) == Some(true) { "" } else { " (down)" },
        n.get("boots").and_then(Value::as_u64).filter(|b| *b > 0).map(|b| format!(" x{b} resets")).unwrap_or_default())).collect()).unwrap_or_default();
    let g = |p: &str| plant.pointer(p).map(|v| v.to_string()).unwrap_or_else(|| "-".into());
    let events: Vec<String> = plant.get("events").and_then(Value::as_array).map(|a| a.iter().rev().take(6)
        .map(|e| format!("{} s  {}", e.get("t_s").map(|v| v.to_string()).unwrap_or_default(), e.get("event").and_then(Value::as_str).unwrap_or(""))).collect()).unwrap_or_default();
    let datasets: Vec<String> = status.get("igt_datasets").and_then(Value::as_array)
        .map(|a| a.iter().filter_map(|v| v.as_str().map(str::to_owned)).collect()).unwrap_or_default();
    let fs = field_style(&theme);
    let d = draft.read().clone();
    rsx! {
        section {class:"gs26-firmware-sim-panel", style:"border:2px solid #a33; border-radius:8px; padding:12px; margin:12px 0;",
            h3 {style:"margin:0 0 8px 0;", "SIMULATED FIRMWARE"}
            p {style:"margin:0 0 8px 0;",
                if running { "Bay running at {pace:.2}x real time. " } else { "Bay not running (no status in the last 5 s). " }
                "Nodes: {nodes.join(\", \")}"
            }
            details {
                summary {"Simulated firmware revisions"}
                ul { for (k, v) in firmware.iter() { li {key:"{k}", code {"{k}"} " {v}"} } }
            }
            div {style:"display:grid; grid-template-columns:repeat(auto-fit,minmax(14em,1fr)); gap:6px; margin:8px 0;",
                div {"Tank " b{"{g(\"/tank/pressure_psi\")} psi"} ", liquid {g(\"/tank/liquid_kg\")} kg, {g(\"/tank/temperature_c\")} C"}
                div {"Valves {g(\"/valves\")}"}
                div {"Supply {g(\"/supply\")}"}
                div {"Motor {g(\"/motor\")}"}
                div {"Flight " b{"{g(\"/flight/phase\")}"} " {g(\"/flight/altitude_m\")} m, {g(\"/flight/velocity_m_s\")} m/s, apogee {g(\"/flight/apogee_m\")} m"}
                div {"Conditions {g(\"/conditions\")}"}
            }
            details { summary {"Plant events"} ul { for e in events.iter() { li {"{e}"} } } }
            if admin {
                if let Some(cur) = d.clone() {
                    div {class:"gs26-firmware-sim-controls", style:"margin-top:8px;",
                        h4 {style:"margin:4px 0;", "Plant inputs"}
                        for (label, which) in [("QD connected", 0u8), ("Nitrogen bottle open", 1), ("Nitrous bottle open", 2)] {
                            label {key:"{label}", style:"margin-right:12px;",
                                input {r#type:"checkbox", checked: [cur.qd_connected, cur.nitrogen_bottle_open, cur.nitrous_bottle_open][which as usize],
                                    onchange: move |e| { if let Some(x) = draft.write().as_mut() { let v = e.checked(); match which { 0 => x.qd_connected = v, 1 => x.nitrogen_bottle_open = v, _ => x.nitrous_bottle_open = v } } }}
                                " {label}"
                            }
                        }
                        h4 {style:"margin:8px 0 4px 0;", "Conditions and calibration masses"}
                        for (label, which, value) in [("Ambient C", 0u8, cur.ambient_c.clone()), ("Wind m/s", 1, cur.wind_mps.clone()), ("Wind from deg", 2, cur.wind_from_deg.clone()),
                                                     ("Gusts m/s", 3, cur.gust_mps.clone()), ("KG50 mass kg", 4, cur.kg50_kg.clone()), ("KG1000 mass kg", 5, cur.kg1000_kg.clone()),
                                                     ("KG50 bypass 0-1", 6, cur.kg50_bypass.clone()), ("KG1000 bypass 0-1", 7, cur.kg1000_bypass.clone())] {
                            label {key:"{label}", style:"margin-right:10px; display:inline-block;", "{label} "
                                input {style:"{fs}", value:"{value}", oninput: move |e| { if let Some(x) = draft.write().as_mut() { let v = e.value(); match which {
                                    0 => x.ambient_c = v, 1 => x.wind_mps = v, 2 => x.wind_from_deg = v, 3 => x.gust_mps = v,
                                    4 => x.kg50_kg = v, 5 => x.kg1000_kg = v, 6 => x.kg50_bypass = v, _ => x.kg1000_bypass = v } } }}
                            }
                        }
                        h4 {style:"margin:8px 0 4px 0;", "Faults"}
                        for (i, (key, label)) in FAULTS.iter().enumerate() {
                            label {key:"{key}", style:"margin-right:12px; display:inline-block;",
                                input {r#type:"checkbox", checked: cur.faults.get(i).map(|f| f.1).unwrap_or(false),
                                    onchange: move |e| { if let Some(x) = draft.write().as_mut() { if let Some(f) = x.faults.get_mut(i) { f.1 = e.checked(); } } }}
                                " {label}"
                            }
                        }
                        div {
                            for (i, (valve, state)) in cur.stuck.iter().enumerate() {
                                label {key:"{valve}", style:"margin-right:10px; display:inline-block;", "{valve} "
                                    select {value:"{state}", onchange: move |e| { if let Some(x) = draft.write().as_mut() { if let Some(s) = x.stuck.get_mut(i) { s.1 = e.value(); } } },
                                        option {value:"", "follows board"} option {value:"open", "stuck open"} option {value:"closed", "stuck closed"}}
                                }
                            }
                        }
                        if !datasets.is_empty() {
                            label {style:"display:block; margin-top:8px;", "IGT dataset on the FC SD card "
                                select {value:"{cur.igt_dataset}", onchange: move |e| { if let Some(x) = draft.write().as_mut() { x.igt_dataset = e.value(); } },
                                    option {value:"", "none"}
                                    for name in datasets.iter() { option {key:"{name}", value:"{name}", "{name}"} }
                                }
                            }
                        }
                        button {style:"margin-top:8px; min-height:40px;", onclick: move |_| {
                                let Some(cur) = draft.peek().clone() else { return };
                                match cur.body() {
                                    Err(e) => message.set(e),
                                    Ok(body) => {
                                        let owner = super::recording_download::ExportOwner::capture_ordinary_selected(|| true);
                                        let request = super::station_settings_request::post_captured::<_, Value>("/api/firmware-sim/control", &body, owner);
                                        message.set("Applying...".into());
                                        spawn(async move {
                                            message.set(match request.await { Ok(_) => "Applied.".into(), Err(e) => format!("Refused: {e}") });
                                        });
                                    }
                                }
                            }, "Apply to the simulation"}
                        span {style:"margin-left:10px;", "{message}"}
                    }
                }
            } else {
                p {class:"gs-notice", "Simulation controls need an administrator."}
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn status() -> Value {
        json!({"active": true, "banner": "SIMULATED FIRMWARE", "bay_running": true, "igt_datasets": ["nominal_10k"],
               "bay": {"pace_actual": 1.0, "nodes": [{"node": "flight", "alive": true, "boots": 0}],
                       "firmware": {"flight": {"repo": "FlightComputer26", "short": "659440e", "subject": "baro"}},
                       "plant": {"tank": {"pressure_psi": 745.0, "liquid_kg": 5.1, "temperature_c": 18.0},
                                 "supply": {"qd_connected": false, "nitrogen_bottle_open": true, "nitrous_bottle_open": true},
                                 "conditions": {"ambient_c": 25.0, "wind_mps": 4.0, "wind_from_deg": 300.0, "gust_mps": 1.0},
                                 "flight": {"phase": "pad"}, "events": [{"t_s": 1.0, "event": "valves open: [vent]"}]}},
               "control": {"faults": {"stuck_valves": {"vent": "closed"}, "radio_drop": true}, "loads": {"kg50_kg": 10.0}}})
    }

    #[test]
    fn draft_round_trips_the_control_document_and_validates_ranges() {
        let s = status();
        let d = ControlDraft::from_status(&s["control"], &s["bay"]["plant"]);
        assert!(!d.qd_connected, "defaults from the plant's reported supply");
        assert_eq!(d.ambient_c, "25");
        let body = d.body().unwrap();
        assert_eq!(body["faults"]["stuck_valves"]["vent"], "closed");
        assert_eq!(body["faults"]["radio_drop"], true);
        assert_eq!(body["loads"]["kg50_kg"], 10.0);
        assert!(body.get("igt").is_none());
        let mut bad = d.clone();
        bad.ambient_c = "90".into();
        assert!(bad.body().unwrap_err().contains("Ambient"));
        bad.ambient_c = "x".into();
        assert!(bad.body().is_err());
    }

    fn render(root: fn() -> Element) -> String {
        let mut dom = VirtualDom::new(root);
        dom.rebuild_in_place();
        dioxus::ssr::render(&dom)
    }

    #[test]
    fn controls_render_only_for_administrators() {
        let admin = render(|| {
            let s = status();
            let d = ControlDraft::from_status(&s["control"], &s["bay"]["plant"]);
            rsx! { FirmwareSimView { theme: ThemeConfig::default(), status: s, admin: true, draft: use_signal(|| Some(d)), message: use_signal(String::new) } }
        });
        assert!(admin.contains("SIMULATED FIRMWARE") && admin.contains("Apply to the simulation") && admin.contains("nominal_10k"));
        assert!(admin.contains("659440e"));
        let viewer = render(|| {
            rsx! { FirmwareSimView { theme: ThemeConfig::default(), status: status(), admin: false, draft: use_signal(|| None), message: use_signal(String::new) } }
        });
        assert!(viewer.contains("SIMULATED FIRMWARE") && !viewer.contains("Apply to the simulation"));
        assert!(viewer.contains("need an administrator"));
    }
}
