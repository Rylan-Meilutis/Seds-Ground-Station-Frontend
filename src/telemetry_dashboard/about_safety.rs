//! Read-only "About safety": every station safety rule, rendered from the
//! station's live catalog (GET /api/safety/about). Nothing here is hard-coded
//! policy; the station names each rule's enforcing code and live values.
use dioxus::prelude::*;
use serde::Deserialize;
use std::collections::BTreeMap;

#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
pub(super) struct Modes {
    #[serde(default)]
    pub flight: bool,
    #[serde(default)]
    pub hitl: bool,
    #[serde(default)]
    pub test_fire: bool,
}
#[derive(Clone, Debug, Deserialize, PartialEq)]
pub(super) struct Rule {
    pub id: String,
    pub title: String,
    pub summary: String,
    #[serde(default)]
    pub details: Vec<String>,
    #[serde(default)]
    pub modes: Modes,
    #[serde(default)]
    pub applies_in_this_build: bool,
    #[serde(default)]
    pub enforced_in: Vec<String>,
    #[serde(default)]
    pub values: BTreeMap<String, serde_json::Value>,
}
#[derive(Clone, Debug, Deserialize, PartialEq)]
pub(super) struct Section {
    pub id: String,
    pub title: String,
    pub summary: String,
    #[serde(default)]
    pub rules: Vec<Rule>,
}
#[derive(Clone, Debug, Deserialize, PartialEq)]
pub(super) struct Catalog {
    pub version: u16,
    pub build_mode: String,
    #[serde(default)]
    pub sections: Vec<Section>,
}

fn mode_label(mode: &str) -> &'static str {
    match mode {
        "hitl" => "HITL",
        "test_fire" => "Test fire",
        _ => "Flight",
    }
}
fn modes_text(modes: &Modes) -> String {
    let mut names = Vec::new();
    if modes.flight {
        names.push("Flight");
    }
    if modes.test_fire {
        names.push("Test fire");
    }
    if modes.hitl {
        names.push("HITL");
    }
    if names.len() == 3 {
        "All builds".into()
    } else {
        names.join(" · ")
    }
}
fn value_text(value: &serde_json::Value) -> String {
    match value {
        serde_json::Value::Null => "unset".into(),
        serde_json::Value::Bool(true) => "on".into(),
        serde_json::Value::Bool(false) => "off".into(),
        serde_json::Value::String(s) => s.clone(),
        other => other.to_string(),
    }
}

#[component]
pub(super) fn AboutSafetyView(catalog: Catalog) -> Element {
    let mode = mode_label(&catalog.build_mode);
    rsx! {
        section {class:"gs26-about-safety",style:"display:grid;gap:14px;min-width:0;","data-build-mode":"{catalog.build_mode}",
            h3 {style:"margin:0;","About safety"}
            p {class:"gs-section-note",style:"margin:0;color:var(--gs-text-muted);font-size:var(--gs-text-sm);",
                "Every safety rule and interlock this station enforces, with its live settings. This station runs the {mode} build; rules marked \"Not in this build\" apply to other builds."}
            for section in catalog.sections.iter() {
                section {key:"{section.id}","data-safety-section":"{section.id}",style:"display:grid;gap:6px;min-width:0;border-top:1px solid var(--gs26-border,#2a3a4f);padding-top:10px;",
                    h4 {style:"margin:0;","{section.title}"}
                    p {style:"margin:0;color:var(--gs-text-muted);font-size:var(--gs-text-sm);","{section.summary}"}
                    for rule in section.rules.iter() {
                        details {class:"gs-info",key:"{rule.id}","data-safety-rule":"{rule.id}","data-applies":if rule.applies_in_this_build{"true"}else{"false"},
                            summary {
                                strong {style:"color:var(--gs26-app-text,inherit);","{rule.title}"}
                                if !rule.applies_in_this_build { span {class:"gs-badge",style:"margin-left:8px;","Not in this build"} }
                            }
                            div {style:"display:grid;gap:6px;padding:6px 0 4px;",
                                p {style:"margin:0;color:var(--gs26-text-secondary,inherit);","{rule.summary}"}
                                for (index, detail) in rule.details.iter().enumerate() { p {key:"{index}",style:"margin:0;","{detail}"} }
                                if !rule.values.is_empty() {
                                    dl {style:"display:grid;grid-template-columns:minmax(0,max-content) minmax(0,1fr);gap:2px 10px;margin:0;font-size:var(--gs-text-xs);",
                                        for (key, value) in rule.values.iter() {
                                            dt {key:"{key}",style:"color:var(--gs-text-muted);","{key}"}
                                            dd {style:"margin:0;","{value_text(value)}"}
                                        }
                                    }
                                }
                                small {style:"color:var(--gs-text-muted);","Builds: {modes_text(&rule.modes)} · Enforced in: {rule.enforced_in.join(\", \")}"}
                            }
                        }
                    }
                }
            }
        }
    }
}

/// Settings page: loads the live catalog through the mounted settings owner.
#[component]
pub(super) fn AboutSafety() -> Element {
    let requests = super::station_settings_request::use_factory();
    let mut catalog = use_signal(|| None::<Result<Catalog, String>>);
    use_effect(move || {
        let request = requests.get::<Catalog>("/api/safety/about");
        spawn(async move {
            let result = request.await;
            if requests.current() {
                catalog.set(Some(result));
            }
        });
    });
    match catalog() {
        None => rsx! { p {role:"status","Loading the station's safety rules…"} },
        Some(Err(error)) => rsx! { p {role:"status","Safety rules are unavailable: {error}"} },
        Some(Ok(catalog)) => rsx! { AboutSafetyView {catalog} },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn fixture() -> Catalog {
        serde_json::from_value(serde_json::json!({
            "version": 1,
            "build_mode": "test_fire",
            "sections": [
                {"id":"tank","title":"Tank pressure","summary":"Overpressure relief and abort.","rules":[
                    {"id":"overpressure-relief","title":"Tank overpressure relief","summary":"At or above 850 psi the station closes Nitrous and Nitrogen and opens Dump and Vent.",
                     "details":["Order: Nitrous close, Nitrogen close, Dump open, Vent open."],
                     "modes":{"flight":true,"hitl":true,"test_fire":true},"applies_in_this_build":true,
                     "enforced_in":["backend/src/gse/overpressure.rs:evaluate"],"config_keys":["gse.overpressure_relief_psi"],
                     "values":{"gse.overpressure_relief_psi":850.0,"gse.tank_rating_psi":null}}
                ]},
                {"id":"launch","title":"Launch","summary":"Launch gates.","rules":[
                    {"id":"qd-settle","title":"QD settlement (flight)","summary":"Queued Launch waits for QD.","details":[],
                     "modes":{"flight":true,"hitl":false,"test_fire":false},"applies_in_this_build":false,
                     "enforced_in":["shared/gse_sequence/src/qd_wait.rs:QdWait"],"values":{},"future_field":1}
                ]}
            ]
        })).unwrap()
    }
    fn html(catalog: Catalog) -> String {
        #[derive(Props, Clone, PartialEq)]
        struct P { catalog: Catalog }
        fn view(p: P) -> Element { rsx! {AboutSafetyView {catalog:p.catalog}} }
        let mut dom = VirtualDom::new_with_props(view, P { catalog });
        dom.rebuild_in_place();
        dioxus::ssr::render(&dom)
    }
    #[test]
    fn about_safety_renders_the_live_catalog_with_disclosures_and_build_applicability() {
        let html = html(fixture());
        assert!(html.contains("About safety"));
        assert!(html.contains("Test fire build"));
        assert!(html.contains("data-safety-section=\"tank\""));
        assert!(html.contains("data-safety-rule=\"overpressure-relief\""));
        assert!(html.contains("<details"), "rules are disclosures");
        assert!(html.contains("At or above 850 psi"), "server text, not hard-coded");
        assert!(html.contains("gse.overpressure_relief_psi"));
        assert!(html.contains("unset"), "null live value is shown as unset");
        assert!(html.contains("Enforced in: backend/src/gse/overpressure.rs:evaluate"));
        assert!(html.contains("Builds: All builds"));
        // A rule for another build is labelled, not hidden.
        assert!(html.contains("data-safety-rule=\"qd-settle\""));
        assert!(html.contains("data-applies=\"false\""));
        assert!(html.contains("Not in this build"));
        assert!(html.contains("Builds: Flight"));
    }
}
