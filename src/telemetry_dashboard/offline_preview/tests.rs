//! Every page of the offline workspace renders populated content, in demo
//! mode and from a replay file in each mode, with no station and no cache.
use super::*;

#[component]
fn Harness(page: String, replay: Option<(String, String)>, bundle: Option<(String, Vec<u8>)>) -> Element {
    rsx! { OfflinePreview { theme: ThemeConfig::default(), on_exit: |_| {}, page: Some(page), replay, bundle } }
}

fn render(page: &str, replay: Option<(String, String)>) -> String {
    render_with(page, replay, None)
}

fn render_with(page: &str, replay: Option<(String, String)>, bundle: Option<(String, Vec<u8>)>) -> String {
    let mut dom = VirtualDom::new_with_props(Harness, HarnessProps { page: page.into(), replay, bundle });
    dom.rebuild_in_place();
    let html = dioxus::ssr::render(&dom);
    drop(dom);
    html
}

fn assert_clean(page: &str, html: &str) {
    for bad in ["NaN", "inf ", "undefined"] {
        assert!(!html.contains(bad), "{page}: renders {bad}");
    }
}

#[test]
fn page_allowlist_is_bounded() {
    assert_eq!(known_page("https://station/api/command"), "state");
    for (id, label) in PAGES {
        assert_eq!(known_page(id), *id);
        assert_eq!(title(id), *label);
    }
    assert_eq!(known_page("settings"), "settings");
}

#[test]
fn demo_mode_populates_every_page() {
    let expect: &[(&str, &[&str])] = &[
        ("state", &["Demo mode — no station connected", "Oxidizer mass", " kg", "Nitrous", "Open replay file"]),
        ("mission", &["Flight procedure", "Nitrous Fill", "Launch", "Abort", "Oxidizer loaded"]),
        ("data", &["Tank pressure", "role=\"tab\"", "<path d=\"M"]),
        ("detailed", &["Fill system", "data-diagram-panel"]),
        ("network-topology", &["Flight Computer", "DAQ Board", "packets", "Warnings"]),
        ("map", &["Recorded GPS track", "landing"]),
        ("vehicle", &["Flight summary", "Apogee (barometer)", "Load 3D replay"]),
        ("calibration", &["Fill loadcell (KG50)", "Capture zero", "Fit: kg"]),
        ("data-export", &["Demo flight", "Download"]),
        ("checklists", &["Reset practice checks"]),
        ("crew-voice", &["Join channel"]),
        ("stream-manager", &["Open stream controls"]),
        ("settings", &["Use sidebar navigation"]),
    ];
    let before = local_station::live_sessions();
    for (page, needles) in expect {
        let html = render(page, None);
        for needle in *needles {
            assert!(html.contains(needle), "{page}: missing {needle:?}");
        }
        assert_clean(page, &html);
        assert!(!html.contains("disabled=true") && !html.contains("disabled=\"true\""), "{page}: demo controls answer instead of being disabled");
    }
    assert_eq!(local_station::live_sessions(), before, "unmounting drops the demo session");
}

#[test]
fn a_replay_file_opens_on_a_fresh_device_in_every_mode() {
    let text = include_str!("../../../tests/fixtures/backend-contract/replay-export/flight.json");
    let mut file: Value = serde_json::from_str(text).unwrap();
    for (context, heading) in [
        (serde_json::json!({"recorded_mode": "normal"}), "Flight summary"),
        (serde_json::json!({"recorded_mode": "test_fire", "test_fire": true}), "Burn summary"),
        (serde_json::json!({"recorded_mode": "integrated_ground_test", "igt": true}), "Flight summary"),
        (serde_json::json!({"recorded_mode": "hitl", "hitl": true}), "Flight summary"),
    ] {
        file["replay_context"] = context;
        let replay = Some(("flight.seds-replay.json".to_string(), file.to_string()));
        for page in ["state", "mission", "data", "detailed", "network-topology", "map", "vehicle", "data-export", "calibration"] {
            let html = render(page, replay.clone());
            assert!(html.contains("Offline replay — flight.seds-replay.json"), "{page}: replay banner");
            assert_clean(page, &html);
            if page == "vehicle" {
                assert!(html.contains(heading), "{page}: {heading}");
            }
            if page == "data" {
                assert!(html.contains("<path d=\"M"), "data: charts drawn");
            }
            if page == "map" {
                assert!(html.contains("Recorded GPS track"), "map: track drawn");
            }
            if page == "state" {
                assert!(!html.contains("No data yet"), "state: only widgets with recorded data are drawn");
                assert!(html.contains("gs26-preview-card"), "state: the file's dashboard has cards");
            }
        }
    }
}

#[test]
fn a_replay_bundle_populates_every_page_with_no_station() {
    let bundle = Some(("flight.gsreplay".to_string(), include_bytes!("../../../tests/fixtures/backend-contract/replay-bundle/flight.gsreplay").to_vec()));
    let expect: &[(&str, &[&str])] = &[
        ("state", &["Offline replay — flight.gsreplay", "gs26-preview-card"]),
        ("data", &["<path d=\"M"]),
        ("detailed", &["data-diagram-panel"]),
        ("network-topology", &["Flight Computer"]),
        ("map", &["Recorded GPS track"]),
        ("vehicle", &["Flight summary", "Load 3D replay"]),
        ("calibration", &["Fit:"]),
        ("data-export", &["Export CSV (as the station exports it)", "Replay bundle (.gsreplay)", "Export layout.json"]),
    ];
    for (page, needles) in expect {
        let html = render_with(page, None, bundle.clone());
        for needle in *needles {
            assert!(html.contains(needle), "{page}: missing {needle:?}");
        }
        assert_clean(page, &html);
    }
}
