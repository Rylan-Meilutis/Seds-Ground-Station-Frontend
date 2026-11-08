//! Actual native /settings route using real framework history and headless document.
//! Never mounts App/Connect, opens a WebView or starts discovery/probe/hardware.
use super::*;
use std::{path::PathBuf, process::Command, rc::Rc, time::{Duration, Instant, SystemTime, UNIX_EPOCH}};

fn settings_route_root() -> Element {
    use_hook(|| {
        dioxus::history::provide_history_context(Rc::new(
            dioxus::history::MemoryHistory::with_initial_path("/settings"),
        ));
        provide_context(Rc::new(dioxus::document::NoOpDocument) as Rc<dyn dioxus::document::Document>);
    });
    rsx! { Router::<Route> {} }
}

fn component_names(dom: &VirtualDom) -> Vec<&'static str> {
    let mut names = Vec::new();
    let mut pending = vec![dom.base_scope().root_node().clone()];
    let mut visited = std::collections::HashSet::new();
    let mut scanned = 0;
    while let Some(node) = pending.pop() {
        scanned += 1;
        assert!(scanned <= 4096, "finite actual mounted tree");
        for (index, dynamic) in node.dynamic_nodes.iter().enumerate() {
            match dynamic {
                dioxus::core::DynamicNode::Fragment(nodes) => pending.extend(nodes.iter().cloned()),
                dioxus::core::DynamicNode::Component(component) => {
                    names.push(component.name);
                    if let Some(id) = component.mounted_scope_id(index, &node, dom)
                        && visited.insert(id)
                        && let Some(scope) = component.mounted_scope(index, &node, dom)
                    {
                        pending.push(scope.root_node().clone());
                    }
                }
                _ => {}
            }
        }
    }
    names
}

#[test]
fn actual_native_settings_route_first_open_has_pin_context_without_connect_or_auth() {
    const CHILD: &str = "GS_NATIVE_SETTINGS_ROUTE_CHILD";
    if std::env::var(CHILD).as_deref() != Ok("1") {
        let nonce = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos();
        let owned = std::env::temp_dir().join(format!("gs-native-settings-route-{}-{nonce}", std::process::id()));
        std::fs::create_dir_all(&owned).unwrap();
        let exact = format!("{}::actual_native_settings_route_first_open_has_pin_context_without_connect_or_auth", module_path!().split_once("::").unwrap().1);
        let mut child = Command::new(std::env::current_exe().unwrap())
            .args(["--exact", &exact, "--nocapture", "--test-threads=1"])
            .env(CHILD, "1")
            .env("GS_NATIVE_SETTINGS_ROUTE_OWNED", &owned)
            .env("XDG_DATA_HOME", owned.join("data"))
            .env("XDG_CONFIG_HOME", owned.join("config"))
            .env("XDG_CACHE_HOME", owned.join("cache"))
            .spawn().unwrap();
        let start = Instant::now();
        let exit = loop {
            if let Some(status) = child.try_wait().unwrap() { break Some(status); }
            if start.elapsed() > Duration::from_secs(30) {
                child.kill().unwrap(); child.wait().unwrap(); break None;
            }
            std::thread::sleep(Duration::from_millis(10));
        };
        let proof = std::fs::read_to_string(owned.join("actual-route-proof.json")).ok();
        std::fs::remove_dir_all(&owned).unwrap();
        assert!(exit.is_some_and(|status| status.success()), "actual isolated route child failed: {exit:?}");
        let proof: serde_json::Value = serde_json::from_str(&proof.expect("actual child proof")).unwrap();
        assert_eq!(proof["route"], "/settings");
        assert_eq!(proof["connect_mounted"], false);
        return;
    }
    assert!(std::env::var_os("RUST_MIN_STACK").is_none(), "unchanged default stack required");
    let owned = PathBuf::from(std::env::var_os("GS_NATIVE_SETTINGS_ROUTE_OWNED").unwrap());
    assert_eq!(PathBuf::from(std::env::var_os("XDG_DATA_HOME").unwrap()), owned.join("data"));
    assert!(!owned.join("data/gs26/storage.json").exists(), "fresh owned preferences only");
    let mut dom = VirtualDom::new(settings_route_root);
    let before = dom.in_scope(ScopeId::ROOT, || {
        assert!(UrlConfig::_stored_base_url().is_none());
        assert!(crate::auth::current_token().is_none());
        (UrlConfig::base_http(), crate::auth::current_status())
    });
    dom.rebuild_in_place();
    dom.process_events();
    dom.render_immediate_to_vec();
    let html = dioxus::ssr::render(&dom);
    assert!(html.contains("Settings"));
    assert!(html.contains("Back"), "actual route wrapper, not only inner component");
    assert!(!html.contains("Settings scope"), "disconnected Settings shows client preferences only");
    assert!(html.contains("Pin telemetry to the top bar"), "actual NativeSettingsPage provides HeaderPinSettings context");
    assert!(!html.contains("Encountered panic"));
    assert!(!html.contains("Searching this local network"));
    let names = component_names(&dom);
    assert!(names.iter().any(|name| name.ends_with("NativeSettingsPage")), "actual native component tree: {names:?}");
    assert!(names.iter().any(|name| name.ends_with("HeaderPinSettings")), "actual pin editor tree: {names:?}");
    let connect_mounted = names.iter().any(|name| name.ends_with("Connect"));
    assert!(!connect_mounted, "no Connect scope/discovery effect may mount: {names:?}");
    dom.in_scope(ScopeId::APP, || {
        assert_eq!(dioxus::history::history().current_route(), "/settings");
        assert_eq!(UrlConfig::base_http(), before.0);
        assert!(UrlConfig::_stored_base_url().is_none());
        assert_eq!(serde_json::to_value(crate::auth::current_status()).unwrap(), serde_json::to_value(&before.1).unwrap());
        assert!(crate::auth::current_token().is_none());
    });
    let proof = serde_json::json!({"route":"/settings","connect_mounted":connect_mounted,"components":names,"scope":"actual framework Router/Settings/NativeSettingsPage/real pin Signal provider; headless NoOpDocument, not iOS/WebView/Connect/discovery proof"});
    std::fs::write(owned.join("actual-route-proof.json"), serde_json::to_vec_pretty(&proof).unwrap()).unwrap();
    println!("ACTUAL_NATIVE_SETTINGS_ROUTE {proof}");
}
