//! The diagrams render in a native VirtualDom with derived state: valve
//! classes, flow animation, stale ages, the IGT-only rocket body, and each
//! station mode's combined view with its own instruments.
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

fn view(id: &str) -> Topology {
    topology::parse(topology::bundled_text(id)).unwrap()
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
    let html = render(view("flight"), &fake);
    assert!(html.contains("data-diagram=\"flight\""));
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
    let html = render(view("test_fire"), &fake);
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
    let stand = view("ground_test");
    let html = render(stand.clone(), &Fake::default());
    assert!(!html.contains("Rocket on the ground"));
    let mut fake = Fake::default();
    fake.igt = true;
    let html = render(stand, &fake);
    assert!(html.contains("Rocket on the ground (FC replaying: SIMULATED)"));
    assert!(html.contains("data-node=\"fc\""));
}

#[derive(Props, Clone, PartialEq)]
struct SvgProps {
    topology: Topology,
    state: derive::DiagramState,
    vehicle: Option<crate::telemetry_dashboard::vehicle_geometry::Geometry>,
}

#[allow(non_snake_case)]
fn SvgHarness(props: SvgProps) -> Element {
    rsx! { render::DiagramSvg { topology: props.topology, state: props.state, vehicle: props.vehicle } }
}

fn render_with_vehicle(topology: Topology, fake: &Fake, vehicle: Option<crate::telemetry_dashboard::vehicle_geometry::Geometry>) -> String {
    let state = derive(&topology, fake);
    let mut dom = VirtualDom::new_with_props(SvgHarness, SvgProps { topology, state, vehicle });
    dom.rebuild_in_place();
    dioxus::ssr::render(&dom)
}

/// The IGT and AV-bay views draw the station's vehicle (vehicle_geometry.rs),
/// else the generic rocket; the station's is SWEET_V6 from its .ork.
#[test]
fn igt_and_av_bay_draw_the_station_vehicle_to_scale() {
    use crate::telemetry_dashboard::vehicle_geometry::{parse, tests::station_example};
    let station = parse(&station_example()).unwrap();
    let mut fake = Fake::default();
    fake.igt = true;
    let stand = view("ground_test");
    let html = render_with_vehicle(stand.clone(), &fake, Some(station.clone()));
    let vehicle = element(&html, "data-vehicle=\"rocket\"");
    assert!(vehicle.contains("data-vehicle-source=\"default\""), "{vehicle}");
    assert!(html.contains("SWEET_V6: 4.39 m (14.4 ft) by 156 mm (6.2 in), to scale"));
    assert!(element(&html, "data-role=\"avionics\"").contains("highlight"));
    assert!(html.contains("pid-vehicle-separation"), "the recovery separation is marked");
    // Not in IGT: no rocket frame, no vehicle.
    assert!(!render_with_vehicle(stand.clone(), &Fake::default(), Some(station.clone())).contains("pid-vehicle"));
    // Without station data the generic rocket is drawn.
    let generic = render_with_vehicle(stand, &fake, None);
    assert!(element(&generic, "data-vehicle=\"rocket\"").contains("data-vehicle-source=\"generic\""));
    let av = render_with_vehicle(view("flight"), &Fake::default(), Some(station));
    assert!(av.contains("data-vehicle=\"rocket\""));
    assert!(element(&av, "data-role=\"recovery\"").contains("highlight"));
}

#[test]
fn av_bay_links_show_down_boards_with_their_age() {
    let mut fake = Fake::default();
    fake.boards.insert("FC".into(), 200);
    fake.boards.insert("PB".into(), 45_000);
    let html = render(view("flight"), &fake);
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
        for (id, text) in topology::BUNDLED.iter() {
            let topology = topology::parse(text).unwrap();
            fake.igt = *id == "ground_test";
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
    let html = render(view("test_fire"), &fake);
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

fn ids(t: &Topology) -> std::collections::BTreeSet<String> {
    let mut out = std::collections::BTreeSet::new();
    out.extend(t.nodes.iter().map(|n| n.id.clone()));
    out.extend(t.valves.iter().map(|v| v.id.clone()));
    out.extend(t.edges.iter().map(|e| e.id.clone()));
    out.extend(t.readings.iter().map(|r| r.id.clone()));
    out
}

fn reading_types(t: &Topology) -> std::collections::BTreeSet<String> {
    t.readings.iter().chain(std::iter::empty()).filter_map(|r| r.value.telemetry.clone()).collect()
}

/// One view per mode, each with the whole fill system. Test fire: the motor
/// on the stand with the KG50, KG1000 and chamber PT, and no AV bay. Flight:
/// the KG50 under the motor and the vehicle's AV bay, no KG1000 or chamber
/// PT. The ground test: the stand, and the rocket only during the test.
#[test]
fn each_mode_view_draws_its_own_system() {
    let flight = view("flight");
    let test_fire = view("test_fire");
    let ground = view("ground_test");
    // The fill system is in every mode view, element for element.
    let fill_elements = ["n2_bottle", "n2_reg", "n2o_bottle", "manifold", "tank", "atm_vent", "atm_dump", "chamber", "ab", "gw", "daq", "vb",
        "n2_bottle_valve", "nitrogen", "n2o_bottle_valve", "nitrous", "qd", "vent", "dump", "main",
        "n2_supply", "n2_branch", "n2o_branch", "fill_line", "vent_line", "dump_line", "feed_line",
        "fill_phase", "flight_state", "relief", "tank_pt", "fill_mass", "fill_pct", "igniter", "igniter_sense", "igniter_current", "fill_battery", "daq_temp", "vb_battery"];
    for t in [&flight, &test_fire, &ground] {
        let have = ids(t);
        for id in fill_elements {
            assert!(have.contains(id), "{}: missing {id}", t.id);
        }
        assert!(t.valves.iter().map(|v| (&v.id, &v.state, &v.command)).eq(flight.valves.iter().map(|v| (&v.id, &v.state, &v.command))), "{}: same valves", t.id);
    }
    // Test fire: KG50 + KG1000 + chamber PT; no AV bay.
    let tf = ids(&test_fire);
    for id in ["kg50", "kg1000", "kg50_kg", "kg1000_kg", "thrust", "chamber_pt", "chamber_installed"] {
        assert!(tf.contains(id), "test fire: {id}");
    }
    for id in ["fc", "pb", "rf", "av_can", "rf_gs", "pyro_drogue"] {
        assert!(!tf.contains(id), "test fire has no AV bay: {id}");
    }
    let types = reading_types(&test_fire);
    assert!(types.contains("LOADCELL_WEIGHT_KG") && types.contains("LOADCELL_50_WEIGHT_KG") && types.contains("COMBUSTION_CHAMBER_PRESSURE"));
    // Flight: the KG50 under the motor, the AV bay; no KG1000, no chamber PT.
    let fl = ids(&flight);
    for id in ["kg50", "motor_kg50", "fc", "pb", "rf", "av_can", "rf_gs", "rf_rrls", "pyro_drogue", "pyro_main", "gps_sats", "rssi"] {
        assert!(fl.contains(id), "flight: {id}");
    }
    for id in ["kg1000", "kg1000_kg", "thrust", "chamber_pt", "chamber_installed"] {
        assert!(!fl.contains(id), "flight: no {id}");
    }
    let types = reading_types(&flight);
    assert!(!types.contains("LOADCELL_WEIGHT_KG") && !types.contains("COMBUSTION_CHAMBER_PRESSURE"));
    assert!(flight.nodes.iter().find(|n| n.id == "chamber").unwrap().pressure.is_none(), "no chamber PT in flight");
    // The ground test: the stand's instruments, and the rocket body (FC,
    // replay status) only while the ground test runs.
    let gt = ids(&ground);
    for id in tf.iter() {
        assert!(gt.contains(id), "ground test keeps the stand: {id}");
    }
    let mut fake = Fake::default();
    let html = render(ground.clone(), &fake);
    assert!(!html.contains("Rocket on the ground"));
    fake.igt = true;
    let html = render(ground, &fake);
    for needle in ["Rocket on the ground (FC replaying: SIMULATED)", "data-node=\"fc\"", "data-node=\"rf\"", "data-reading=\"sim_alt\""] {
        assert!(html.contains(needle), "{needle}");
    }
}

/// The instruments of each mode render live: load cells, the chamber PT, the
/// fill mass under the motor, and OVR on an over-range channel.
#[test]
fn mode_views_render_their_load_cells_and_chamber_live() {
    use crate::telemetry_dashboard::over_range::OverRange;
    let mut fake = Fake::default();
    fake.set("LOADCELL_50_WEIGHT_KG", 0, 31.25, 100).set("LOADCELL_WEIGHT_KG", 0, 102.0, 100).set("COMBUSTION_CHAMBER_PRESSURE", 0, 388.0, 100);
    let html = render(view("test_fire"), &fake);
    assert!(element(&html, "data-reading=\"kg50_kg\"").contains("data-reading"));
    assert!(html.contains("31.25 kg") && html.contains("102.0 kg") && html.contains("388 psig"));
    assert!(html.contains("1000 N"), "KG1000 force: 102 kg × g");
    let html = render(view("flight"), &fake);
    assert!(html.contains("31.25 kg"), "fill mass from the KG50 under the motor");
    assert!(!html.contains("102.0 kg") && !html.contains("388 psig"), "no stand instruments in flight");
    fake.over_range.insert(("LOADCELL_50_WEIGHT_KG".into(), 0), (OverRange::High, 100));
    let html = render(view("flight"), &fake);
    assert!(element(&html, "data-reading=\"fill_mass\"").contains("over-range"));
    // Stale over 5 s on the fill side.
    let mut fake = Fake::default();
    fake.set("LOADCELL_WEIGHT_KG", 0, 4.0, 8_000);
    let html = render(view("test_fire"), &fake);
    assert!(element(&html, "data-reading=\"kg1000_kg\"").contains("stale"));
}

#[derive(Props, Clone, PartialEq)]
struct FullProps {
    topology: Topology,
    state: derive::DiagramState,
    on: bool,
}

#[allow(non_snake_case)]
fn FullHarness(props: FullProps) -> Element {
    let fullscreen = Fullscreen { on: use_signal(|| props.on) };
    rsx! { DiagramPanel { topology: props.topology, state: props.state, program: true, provenance: "Live".to_string(), fullscreen, mode_view: true } }
}

/// Every view has a full-screen button; full screen covers the viewport,
/// keeps the live SVG (vector, so it stays crisp) and offers the exit.
#[test]
fn full_screen_button_and_overlay() {
    for (id, _) in topology::BUNDLED {
        let topology = view(id);
        for on in [false, true] {
            let state = derive(&topology, &Fake::default());
            let mut dom = VirtualDom::new_with_props(FullHarness, FullProps { topology: topology.clone(), state, on });
            dom.rebuild_in_place();
            let html = dioxus::ssr::render(&dom);
            let panel = element(&html, "data-diagram-panel=");
            assert!(panel.contains(&format!("data-full=\"{on}\"")), "{panel}");
            assert!(html.contains(if on { "data-fullscreen=\"exit\"" } else { "data-fullscreen=\"enter\"" }), "{id} {on}");
            assert!(html.contains(&format!("data-diagram=\"{id}\"")) && html.contains("station mode view"));
            assert_eq!(panel.contains("aria-modal=\"true\""), on);
        }
    }
    // The overlay CSS: fixed over the viewport, the SVG filling it.
    assert!(PAGE_CSS.contains(".gs26-pid-wrap[data-full=true] { position:fixed; inset:0;"));
    assert!(PAGE_CSS.contains(".gs26-pid-svg > .gs26-pid { width:100%; height:100%; max-height:none; }"));
    // The script: the Fullscreen API (and Safari's prefix) when present, Esc and the platform exit report back.
    for needle in ["requestFullscreen", "webkitRequestFullscreen", "fullscreenchange", "'Escape'", "dioxus.send('exit')"] {
        assert!(FULLSCREEN_JS.contains(needle), "{needle}");
    }
    // Without a fullscreen handle (the broadcast preview, older callers) there is no button.
    let topology = view("network");
    let html = render(topology, &Fake::default());
    assert!(!html.contains("data-fullscreen="));
}
