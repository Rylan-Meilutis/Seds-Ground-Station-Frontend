//! The diagrams render in a native VirtualDom with derived state: valve
//! classes, flow animation, stale ages and the IGT-only rocket body.
use super::derive::tests::Fake;
use super::derive::{ControlView, derive};
use super::topology::{self, Topology};
use super::*;
use crate::telemetry_dashboard::layout::ThemeConfig;

#[derive(Props, Clone, PartialEq)]
struct HarnessProps {
    topology: Topology,
    state: derive::DiagramState,
    #[props(default)]
    theme: ThemeConfig,
}

#[allow(non_snake_case)]
fn Harness(props: HarnessProps) -> Element {
    let vars = render::theme_vars(&props.theme);
    rsx! {
        div { style: "{vars}",
            DiagramPanel { topology: props.topology, state: props.state, program: false, provenance: "Live".to_string() }
        }
    }
}

fn render(topology: Topology, fake: &Fake) -> String {
    let state = derive(&topology, fake);
    let mut dom = VirtualDom::new_with_props(Harness, HarnessProps { topology, state, theme: ThemeConfig::default() });
    dom.rebuild_in_place();
    dioxus::ssr::render(&dom)
}

fn element<'a>(html: &'a str, attr: &str) -> &'a str {
    let at = html.find(attr).unwrap_or_else(|| panic!("{attr} not rendered"));
    let start = html[..at].rfind('<').unwrap();
    &html[start..html[at..].find('>').map(|e| at + e).unwrap()]
}

#[test]
fn fill_diagram_renders_valves_flow_and_readings() {
    let mut fake = Fake::default();
    fake.valves([0., 0., 0., 0., 1., 0., 0.], 100).set("PRESSURE_TRANSDUCER_CALIBRATED", 0, 200.0, 100);
    fake.controls.insert("Dump".into(), ControlView { actuated: Some(false), requested: Some(true), running: false });
    let html = render(topology::parse(topology::BUNDLED[0].1).unwrap(), &fake);
    assert!(html.contains("data-diagram=\"fill\""));
    assert!(element(&html, "data-valve=\"nitrogen\"").contains("state-open"));
    assert!(element(&html, "data-valve=\"vent\"").contains("state-closed"));
    assert!(element(&html, "data-valve=\"dump\"").contains("state-pending"));
    assert!(html.contains("→ OPEN (pending)"));
    assert!(element(&html, "data-valve=\"n2_bottle_valve\"").contains("state-manual"));
    assert!(element(&html, "data-edge=\"fill_line\"").contains("data-flow=\"flowing\""));
    assert!(element(&html, "data-edge=\"vent_line\"").contains("data-flow=\"blocked\""));
    assert!(html.contains("pid-flow on"), "a flowing line animates");
    assert!(html.contains("200 psi"));
    assert!(html.contains("Nitrogen gas"), "legend lists the fluids");
}

#[test]
fn stale_values_grey_out_with_their_age() {
    let mut fake = Fake::default();
    fake.valves([0., 0., 0., 0., 1., 0., 0.], 7_000).set("PRESSURE_TRANSDUCER_CALIBRATED", 0, 200.0, 7_000);
    let html = render(topology::parse(topology::BUNDLED[0].1).unwrap(), &fake);
    assert!(element(&html, "data-valve=\"nitrogen\"").contains("state-stale"));
    assert!(html.contains("stale 7 s (was open)"));
    assert!(element(&html, "data-reading=\"tank_pt\"").contains("stale"));
    assert!(html.contains("200 psi · 7 s ago"));
    assert!(!html.contains("pid-flow on"), "nothing animates on stale data");
}

#[test]
fn every_bundled_diagram_renders_and_the_rocket_body_appears_only_in_igt() {
    for (id, text) in topology::BUNDLED {
        let html = render(topology::parse(text).unwrap(), &Fake::default());
        assert!(html.contains(&format!("data-diagram=\"{id}\"")), "{id}");
    }
    let stand = topology::parse(topology::BUNDLED[3].1).unwrap();
    let html = render(stand.clone(), &Fake::default());
    assert!(!html.contains("Rocket on the ground"));
    let mut fake = Fake::default();
    fake.igt = true;
    let html = render(stand, &fake);
    assert!(html.contains("Rocket on the ground (FC replaying: SIMULATED)"));
    assert!(html.contains("data-node=\"fc\""));
}

#[test]
fn av_bay_links_show_down_boards_with_their_age() {
    let mut fake = Fake::default();
    fake.boards.insert("FC".into(), 200);
    fake.boards.insert("PB".into(), 45_000);
    let html = render(topology::parse(topology::BUNDLED[1].1).unwrap(), &fake);
    assert!(element(&html, "data-node=\"fc\"").contains("online-up"));
    assert!(element(&html, "data-node=\"pb\"").contains("online-down"));
    assert!(html.contains("not heard for 45 s"));
    assert!(html.contains("not reported by the FC"), "pyro continuity is never invented");
}

/// Writes each diagram with a representative state as standalone HTML for a
/// headless screenshot: `GS26_DIAGRAM_DUMP=/dir cargo test ... -- --ignored dump_`.
#[test]
#[ignore]
fn dump_diagrams_for_screenshots() {
    let Ok(dir) = std::env::var("GS26_DIAGRAM_DUMP") else { return };
    let mut fake = Fake::default();
    // Mid fill: nitrous and vent open, QD connected, 520 psi, 61 %.
    fake.valves([0., 1., 0., 0., 0., 1., 0.], 120)
        .set("PRESSURE_TRANSDUCER_CALIBRATED", 0, 520.0, 40)
        .set("LOADCELL_50_WEIGHT_KG", 0, 4.87, 60)
        .set("LOADCELL_FILL_PERCENT", 0, 61.0, 60)
        .set("LOADCELL_WEIGHT_KG", 0, 0.4, 60)
        .set("IGNITER_SENSE_SIGNAL", 1, 0.02, 300)
        .set("COMBUSTION_CHAMBER_PT_INSTALLED", 0, 1.0, 100)
        .set("COMBUSTION_CHAMBER_PRESSURE", 0, 1.0, 100)
        .set("BATTERY_VOLTAGE", 0, 15.4, 700)
        .set("DAQ_ADC_TEMPERATURE", 0, 28.4, 900)
        .set("ACCEL_DATA", 0, 0.1, 400).set("ACCEL_DATA", 1, -0.2, 400).set("ACCEL_DATA", 2, 9.79, 400)
        .set("GYRO_DATA", 0, 0.3, 400).set("GYRO_DATA", 1, -0.1, 400).set("GYRO_DATA", 2, 0.0, 400)
        .set("BAROMETER_DATA", 0, 1.2, 400).set("BAROMETER_DATA", 1, 908.4, 400).set("BAROMETER_DATA", 2, 21.5, 400)
        .set("BATTERY_CURRENT", 0, 0.82, 700)
        .set("GPS_SATELLITE_NUMBER", 0, 9.0, 800).set("GPS_STATUS", 0, 3.0, 800);
    fake.controls.insert("Dump".into(), ControlView { actuated: Some(false), requested: Some(true), running: false });
    for b in ["AB", "VB", "DAQ", "GB", "FC", "RF"] { fake.boards.insert(b.into(), 300); }
    fake.boards.insert("PB".into(), 41_000);
    for (k, v) in [("fill_phase", "Nitrous fill"), ("flight_state", "Fill"), ("fc_phase", "Pad"), ("estimator", "FC estimator: not running"),
        ("provenance", "Live"), ("pyro_drogue", "Safe (FC arms after Launch)"), ("pyro_main", "Safe (FC arms after Launch)"), ("fc_sd", "Ready")] {
        fake.statuses.insert(k.into(), v.into());
    }
    let css = format!("{}{}", render::CSS, PAGE_CSS);
    for (theme_name, theme) in [("dark", ThemeConfig::default()), ("light", {
        let mut t = ThemeConfig::default();
        t.app_background = "#f6f7f9".into(); t.panel_background = "#ffffff".into(); t.panel_background_alt = "#eef1f5".into();
        t.text_primary = "#0b0b0b".into(); t.text_muted = "#5b6470".into(); t.border = "#c9d0d8".into(); t.border_strong = "#8a96a3".into(); t
    })] {
        for (index, (id, text)) in topology::BUNDLED.iter().enumerate() {
            let topology = topology::parse(text).unwrap();
            fake.igt = index == 3;
            let state = derive(&topology, &fake);
            let mut dom = VirtualDom::new_with_props(Harness, HarnessProps { topology, state, theme: theme.clone() });
            dom.rebuild_in_place();
            let body = dioxus::ssr::render(&dom);
            let vars = render::theme_vars(&theme);
            let html = format!("<!doctype html><meta charset=utf-8><style>body{{margin:0;background:{};color:{};font-family:system-ui,sans-serif}}{css}</style><div class=gs26-pid-page style=\"{vars}height:auto\">{body}</div>", theme.app_background, theme.text_primary);
            std::fs::write(format!("{dir}/{id}-{theme_name}.html"), html).unwrap();
        }
    }
}

#[test]
fn over_range_readings_show_ovr_never_a_number() {
    use crate::telemetry_dashboard::over_range::OverRange;
    let mut fake = Fake::default();
    fake.set("LOADCELL_50_WEIGHT_KG", 0, 12.5, 100);
    fake.over_range.insert(("LOADCELL_WEIGHT_KG".into(), 0), (OverRange::High, 100));
    fake.over_range.insert(("COMBUSTION_CHAMBER_PRESSURE".into(), 0), (OverRange::Low, 100));
    let html = render(topology::parse(topology::BUNDLED[3].1).unwrap(), &fake);
    let kg1000 = element(&html, "data-reading=\"kg1000_kg\"");
    assert!(kg1000.contains("over-range"), "{kg1000}");
    // The scaled thrust reading is OVR too, not OVR × g or a number.
    assert!(element(&html, "data-reading=\"thrust\"").contains("over-range"));
    assert!(element(&html, "data-reading=\"chamber_pt\"").contains("over-range"));
    assert!(html.contains("ADC over range: reading is not valid"));
    assert!(html.contains("ADC over range (below −full scale): reading is not valid"));
    assert!(html.contains("−OVR"));
    assert!(!html.contains("OVR kg") && !html.contains("OVR N"), "no unit after OVR");
    // A channel that is not over range keeps its number.
    assert!(html.contains("12.50 kg"));
    assert!(!element(&html, "data-reading=\"kg50_kg\"").contains("over-range"));
}
