//! Voice, cameras/recordings and the stream manager are reachable on every
//! build target (web, desktop, iOS, Android): nothing between the tab list and
//! the station iframes is compiled per platform. Platform differences live in
//! the WebView set-up (`native_media`), never in whether a page renders.

fn body(source: &str, signature: &str) -> String {
    let start = source.find(signature).unwrap_or_else(|| panic!("missing {signature}"));
    let open = start + source[start..].find('{').unwrap();
    let mut depth = 1;
    for (offset, c) in source[open + 1..].char_indices() {
        match c {
            '{' => depth += 1,
            '}' => depth -= 1,
            _ => {}
        }
        if depth == 0 {
            return source[open..open + 2 + offset].to_string();
        }
    }
    panic!("unclosed body for {signature}")
}

fn assert_ungated(text: &str, what: &str) {
    for gate in ["cfg!(", "#[cfg(", "target_os", "target_arch", "feature ="] {
        assert!(!text.contains(gate), "{what} must not be gated per platform ({gate})");
    }
}

#[test]
fn media_tabs_are_selected_without_platform_gates() {
    let connection = include_str!("dashboard_connection.rs");
    for signature in ["fn _available_main_tabs(", "fn _configured_main_tabs(", "fn _main_tab_from_str(", "fn _main_tab_to_str("] {
        assert_ungated(&body(connection, signature), signature);
    }
    let available = body(connection, "fn _available_main_tabs(");
    assert!(available.contains("MainTab::CrewVoice, MainTab::Media"), "voice and media are always offered");
    assert!(available.contains("tabs.push(MainTab::StreamManager)"), "stream manager is offered to managers");
    let rail = include_str!("navigation_rail.rs");
    for id in ["\"crew-voice\"", "\"media\"", "\"stream-manager\""] {
        assert!(rail.contains(id), "navigation rail lists {id}");
    }
}

#[test]
fn media_surface_and_iframes_render_on_every_target() {
    let surface = include_str!("media_tool_tab.rs");
    for component in ["fn MediaToolTab(", "fn MediaSurface(", "fn MediaHost("] {
        let component_body = body(surface, component);
        let markup = &component_body[component_body.find("rsx!").expect("component renders")..];
        assert_ungated(markup, component);
    }
    for (id, path) in [("gs26-media", "/media?embedded=1"), ("gs26-crew-voice", "/radio?embedded=1")] {
        assert!(surface.contains(&format!("id:\"{id}\",path:\"{path}\"")), "{id} mounts {path}");
    }
    assert!(surface.contains(r#"allow: "camera; microphone; autoplay; fullscreen; screen-wake-lock""#));
    // The only per-target code is the explanation hook, defined for both families.
    assert!(surface.contains("#[cfg(target_arch = \"wasm32\")]\nfn use_native_preflight("));
    assert!(surface.contains("#[cfg(not(target_arch = \"wasm32\"))]\nfn use_native_preflight("));

    let component = include_str!("dashboard_component.rs");
    let start = component.find("let surface_session = auth::current_status();").unwrap();
    let end = start + component[start..].find("media_tool_tab::MediaSurface {").unwrap();
    assert_ungated(&component[start..end], "media/voice visibility and the dedicated stream view");

    let program = include_str!("live_stream_tab.rs");
    let tab = body(program, "pub(crate) fn LiveStreamTab(");
    assert_ungated(&tab[tab.find("rsx!").unwrap()..], "LiveStreamTab markup");
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn every_media_document_the_tabs_mount_may_navigate_in_app() {
    let station = "https://gs.example.invalid:3443";
    for path in ["/media?embedded=1", "/radio?embedded=1", "/program", "/player", "/studio-audio"] {
        assert!(
            crate::native_navigation::station_document_allowed(&format!("{station}{path}"), station),
            "{path}"
        );
    }
}
