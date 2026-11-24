//! Simulated loopback cable for the DAC validation (firmware simulation only, GS docs/firmware-sim.md): a patch
//! diagram from the DAQ's DAC outputs (P10) to the DAQ inputs. The simulated plant reads the DAQ firmware's own DAC
//! output and puts it on the patched input through the real acquisition path, so the validation and sweep below run
//! end to end. Hidden against real hardware; administrators connect and disconnect.
use super::layout::ThemeConfig;
use dioxus::prelude::*;
use serde_json::{Value, json};

/// The DAQ inputs the simulated cable reaches, with their DAC-validation target ids.
pub(super) const INPUTS: [(&str, &str); 9] = [
    ("loadcell_kg50", "KG50 load cell AMP2 (P8)"),
    ("p6_diff", "KG50 P6 differential"),
    ("loadcell_kg1000", "KG1000 load cell AMP1 (P7)"),
    ("hvain1", "CC PT HVAIN1 (P4 pin 3)"),
    ("hvain2", "HVAIN2 (P4 pin 4)"),
    ("mcp_ch2", "MCP CH2 (P6 pin 3)"),
    ("mcp_ch3", "MCP CH3 (P6 pin 4)"),
    ("mcp_ch4", "MCP CH4 (P5 pin 3)"),
    ("mcp_ch5", "MCP CH5 (P5 pin 4)"),
];

async fn sleep_ms(ms: u32) {
    #[cfg(target_arch = "wasm32")]
    gloo_timers::future::TimeoutFuture::new(ms).await;
    #[cfg(not(target_arch = "wasm32"))]
    tokio::time::sleep(std::time::Duration::from_millis(ms as u64)).await;
}

/// The patch section for the control document: each DAC output to one input or none.
/// The links only: the station merges `patch` per key, so the attenuations
/// set elsewhere (a test tool) are kept rather than reset to 1.
pub(super) fn patch_body(links: &[Option<String>; 2]) -> Value {
    json!({"patch": {"dac1": links[0], "dac2": links[1]}})
}

/// Current links from `/api/firmware-sim` (bay.plant.dac.patch).
pub(super) fn links_from(status: &Value) -> [Option<String>; 2] {
    let get = |k: &str| status.pointer(&format!("/bay/plant/dac/patch/{k}/target")).and_then(Value::as_str).map(str::to_owned);
    [get("dac1"), get("dac2")]
}

#[component]
pub(super) fn DacSimPatch(theme: ThemeConfig) -> Element {
    let mut status = use_signal(|| None::<Value>);
    use_future(move || async move {
        loop {
            if let Ok(next) = super::http_get_json::<Value>("/api/firmware-sim").await {
                status.set(Some(next));
            }
            sleep_ms(1500).await;
        }
    });
    let s = status.read().clone();
    match s {
        Some(s) if s.get("active").and_then(Value::as_bool) == Some(true) => {
            rsx! { DacSimPatchView { theme, status: s, admin: crate::auth::is_admin() } }
        }
        // real hardware: no simulated cable
        _ => rsx! {},
    }
}

#[component]
pub(super) fn DacSimPatchView(theme: ThemeConfig, status: Value, admin: bool) -> Element {
    let links = links_from(&status);
    let mut pending = use_signal(|| None::<usize>);
    let mut message = use_signal(String::new);
    let volts: Vec<String> = (0..2)
        .map(|c| status.pointer(&format!("/bay/plant/dac/connector_v/{c}")).and_then(Value::as_f64).map(|v| format!("{v:.3} V")).unwrap_or_else(|| "off".into()))
        .collect();
    let swapped = status.pointer("/bay/plant/dac/p10_inputs_swapped").and_then(Value::as_bool).unwrap_or(false);
    let row_h = 26.0;
    let height = row_h * INPUTS.len() as f64 + 10.0;
    let y_in = |i: usize| 15.0 + row_h * i as f64;
    let y_dac = |c: usize| height / 3.0 * (c as f64 + 1.0);
    let mut send = move |next: [Option<String>; 2]| {
        let body = patch_body(&next);
        let owner = super::recording_download::ExportOwner::capture_ordinary_selected(|| true);
        let request = super::station_settings_request::post_captured::<_, Value>("/api/firmware-sim/control", &body, owner);
        message.set("Patching...".into());
        spawn(async move {
            message.set(match request.await { Ok(_) => "Patched.".into(), Err(e) => format!("Refused: {e}") });
        });
    };
    let lines: Vec<(usize, usize)> = (0..2).filter_map(|c| links[c].as_ref().and_then(|t| INPUTS.iter().position(|(id, _)| id == t)).map(|i| (c, i))).collect();
    rsx! {
        section {class:"gs26-dac-sim-patch", style:"border:2px dashed #a33; border-radius:8px; padding:10px; margin:10px 0;",
            h4 {style:"margin:0 0 6px 0;", "Simulated loopback cable (SIMULATED FIRMWARE)"}
            p {style:"margin:0 0 6px 0;", "DAC1 {volts[0]}, DAC2 {volts[1]} at P10."
                if swapped { " P10 buffer inputs swapped (fault)." } }
            svg {width:"100%", view_box:"0 0 520 {height}", style:"max-width:640px; background:{theme.button_background};",
                for c in 0..2usize {
                    g {key:"dac{c}",
                        rect {x:"8", y:"{y_dac(c) - 12.0}", width:"90", height:"24", rx:"4",
                            fill: if *pending.read() == Some(c) { "#a33" } else { "#444" },
                            onclick: move |_| { if admin { pending.set(Some(c)); } }}
                        text {x:"16", y:"{y_dac(c) + 5.0}", fill:"#fff", "DAC{c + 1} (P10)"}
                    }
                }
                for (c, i) in lines.iter().copied() {
                    line {key:"l{c}-{i}", x1:"98", y1:"{y_dac(c)}", x2:"300", y2:"{y_in(i)}", stroke: if c == 0 { "#e66" } else { "#6ae" }, stroke_width:"3"}
                }
                for (i, (id, label)) in INPUTS.iter().enumerate() {
                    g {key:"{id}",
                        rect {x:"300", y:"{y_in(i) - 10.0}", width:"212", height:"20", rx:"3", fill:"#333",
                            onclick: {
                                let id = id.to_string();
                                let links = links.clone();
                                move |_| {
                                    if !admin { return; }
                                    let Some(c) = *pending.peek() else { return };
                                    let mut next = links.clone();
                                    next[c] = if next[c].as_deref() == Some(id.as_str()) { None } else { Some(id.clone()) };
                                    pending.set(None);
                                    send(next);
                                }
                            }}
                        text {x:"306", y:"{y_in(i) + 4.0}", fill:"#fff", font_size:"12", "{label}"}
                    }
                }
            }
            if admin {
                p {style:"margin:4px 0;", "Click a DAC output, then an input to connect it (click a connected input again to disconnect)."}
                button {onclick: move |_| send([None, None]), "Disconnect all"}
                span {style:"margin-left:8px;", "{message}"}
            } else {
                p {class:"gs-notice", "Patching the simulated cable needs an administrator."}
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn status() -> Value {
        json!({"active": true, "bay": {"plant": {"dac": {"connector_v": [3.3, null], "p10_inputs_swapped": false,
            "patch": {"dac1": {"target": "hvain1", "attenuation": 1.0}, "dac2": null}}}}})
    }

    #[test]
    fn links_and_body_round_trip() {
        let links = links_from(&status());
        assert_eq!(links, [Some("hvain1".to_string()), None]);
        let body = patch_body(&links);
        assert_eq!(body["patch"]["dac1"], "hvain1");
        assert!(body["patch"]["dac2"].is_null());
        assert!(body["patch"].get("dac1_attenuation").is_none(), "attenuations are not reset by re-patching");
        assert!(INPUTS.iter().all(|(id, _)| !id.starts_with("sar")), "only inputs the plant routes");
    }

    fn render(root: fn() -> Element) -> String {
        let mut dom = VirtualDom::new(root);
        dom.rebuild_in_place();
        dioxus::ssr::render(&dom)
    }

    #[test]
    fn diagram_shows_the_patch_and_gates_controls() {
        let html = render(|| rsx! { DacSimPatchView { theme: ThemeConfig::default(), status: status(), admin: true } });
        assert!(html.contains("Simulated loopback cable") && html.contains("CC PT HVAIN1") && html.contains("<line"));
        assert!(html.contains("3.300 V") && html.contains("Disconnect all"));
        let viewer = render(|| rsx! { DacSimPatchView { theme: ThemeConfig::default(), status: status(), admin: false } });
        assert!(!viewer.contains("Disconnect all") && viewer.contains("needs an administrator"));
    }
}
