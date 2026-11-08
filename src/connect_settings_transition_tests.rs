//! Actual Connect -> Settings navigation with original discovery still in flight.
//! Linux private-network test only; no WebView, Apple poke or hardware claims.
use super::*;
use dioxus::core::{AttributeValue, DynamicNode, ElementId, Task, TemplateNode, VNode};
use std::{cell::RefCell, path::PathBuf, process::Command, rc::Rc, sync::mpsc, task::Poll,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH}};

thread_local! { static DISCOVERY_TASKS: RefCell<Vec<Task>> = const { RefCell::new(Vec::new()) }; }
pub(super) fn observe_discovery_task(task: Task) {
    DISCOVERY_TASKS.with(|tasks| { let mut tasks = tasks.borrow_mut(); assert!(tasks.len() < 8); tasks.push(task); });
}
fn router_root() -> Element {
    use_hook(|| {
        dioxus::history::provide_history_context(Rc::new(dioxus::history::MemoryHistory::with_initial_path("/connect")));
        provide_context(Rc::new(dioxus::document::NoOpDocument) as Rc<dyn dioxus::document::Document>);
    });
    rsx! { Router::<Route> {} }
}
fn all_nodes(dom: &VirtualDom) -> Vec<VNode> {
    let mut output = Vec::new();
    let mut pending = vec![dom.base_scope().root_node().clone()];
    let mut visited = std::collections::HashSet::new();
    while let Some(node) = pending.pop() {
        assert!(output.len() < 4096, "finite mounted tree");
        for (index, dynamic) in node.dynamic_nodes.iter().enumerate() {
            match dynamic {
                DynamicNode::Fragment(nodes) => pending.extend(nodes.iter().cloned()),
                DynamicNode::Component(component) => {
                    if let Some(id) = component.mounted_scope_id(index, &node, dom)
                        && visited.insert(id)
                        && let Some(scope) = component.mounted_scope(index, &node, dom)
                    { pending.push(scope.root_node().clone()); }
                }
                _ => {}
            }
        }
        output.push(node);
    }
    output
}
fn component_names(dom: &VirtualDom) -> Vec<&'static str> {
    all_nodes(dom).iter().flat_map(|node| node.dynamic_nodes.iter().filter_map(|dynamic| {
        if let DynamicNode::Component(component) = dynamic { Some(component.name) } else { None }
    })).collect()
}
fn node_text(dom: &VirtualDom, node: &VNode, template: &TemplateNode) -> String {
    match template {
        TemplateNode::Text { text } => text.to_string(),
        TemplateNode::Element { children, .. } => children.iter().map(|child| node_text(dom, node, child)).collect(),
        TemplateNode::Dynamic { id } => match &node.dynamic_nodes[*id] {
            DynamicNode::Text(text) => text.value.clone(),
            DynamicNode::Fragment(nodes) => nodes.iter().map(|child| child.template.roots.iter().map(|t| node_text(dom, child, t)).collect::<String>()).collect(),
            DynamicNode::Component(component) => component.mounted_scope(*id, node, dom).map(|scope| scope.root_node().template.roots.iter().map(|t| node_text(dom, scope.root_node(), t)).collect()).unwrap_or_default(),
            DynamicNode::Placeholder(_) => String::new(),
        },
    }
}
fn settings_button(dom: &VirtualDom) -> ElementId {
    button_with_text(dom, "Settings")
}
fn button_with_text(dom: &VirtualDom, text: &str) -> ElementId {
    let mut found = Vec::new();
    for node in all_nodes(dom) {
        for (index, attributes) in node.dynamic_attrs.iter().enumerate() {
            if !attributes.iter().any(|a| (a.name == "onclick" || a.name == "click") && matches!(a.value, AttributeValue::Listener(_))) { continue; }
            let path = node.template.attr_paths[index];
            let mut template = &node.template.roots[usize::from(path[0])];
            for child in &path[1..] {
                let TemplateNode::Element { children, .. } = template else { panic!("actual event path"); };
                template = &children[usize::from(*child)];
            }
            if matches!(template, TemplateNode::Element { tag: "button", .. }) && node_text(dom, &node, template) == text {
                found.push(node.mounted_dynamic_attribute(index, dom).expect("actual mounted click event"));
            }
        }
    }
    assert_eq!(found.len(), 1, "genuine unique {text} button"); found[0]
}
fn assert_settings(dom: &VirtualDom, original_base: &str, original_status: &serde_json::Value) {
    let html = dioxus::ssr::render(dom);
    assert!(html.contains("Pin telemetry to the top bar") && html.contains("Back"), "actual route and pin provider");
    assert!(!html.contains("Settings scope"), "disconnected Settings shows client preferences only");
    assert!(!html.contains("Encountered panic"));
    let names = component_names(dom);
    assert!(!names.iter().any(|name| name.ends_with("Connect")), "old Connect scope retired: {names:?}");
    assert!(names.iter().any(|name| name.ends_with("NativeSettingsPage")));
    assert!(names.iter().any(|name| name.ends_with("HeaderPinSettings")));
    dom.in_scope(ScopeId::APP, || {
        assert_eq!(dioxus::history::history().current_route(), "/settings");
        assert_eq!(UrlConfig::base_http(), original_base);
        assert!(UrlConfig::_stored_base_url().is_none());
        assert!(crate::auth::current_token().is_none());
        assert_eq!(&serde_json::to_value(crate::auth::current_status()).unwrap(), original_status);
    });
}

#[tokio::test(flavor = "current_thread")]
async fn actual_connect_settings_click_cancels_scoped_task_while_real_discovery_worker_held() {
    const CHILD: &str = "GS_CONNECT_SETTINGS_CHILD";
    if std::env::var(CHILD).as_deref() != Ok("1") {
        let nonce = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos();
        let owned = std::env::temp_dir().join(format!("gs-connect-settings-{}-{nonce}", std::process::id()));
        std::fs::create_dir_all(&owned).unwrap();
        let exact = format!("{}::actual_connect_settings_click_cancels_scoped_task_while_real_discovery_worker_held", module_path!().split_once("::").unwrap().1);
        let mut child = Command::new(std::env::current_exe().unwrap())
            .args(["--exact", &exact, "--nocapture", "--test-threads=1"])
            .env(CHILD, "1").env("GS_CONNECT_SETTINGS_OWNED", &owned)
            .env("XDG_DATA_HOME", owned.join("data")).env("XDG_CONFIG_HOME", owned.join("config")).env("XDG_CACHE_HOME", owned.join("cache"))
            .spawn().unwrap();
        let start = Instant::now();
        let exit = loop {
            if let Some(status) = child.try_wait().unwrap() { break Some(status); }
            if start.elapsed() > Duration::from_secs(30) { child.kill().unwrap(); child.wait().unwrap(); break None; }
            std::thread::sleep(Duration::from_millis(10));
        };
        let proof = std::fs::read_to_string(owned.join("actual-transition-proof.json")).ok();
        std::fs::remove_dir_all(&owned).unwrap();
        assert!(exit.is_some_and(|status| status.success()), "actual isolated transition child failed: {exit:?}");
        let proof: serde_json::Value = serde_json::from_str(&proof.expect("actual child proof")).unwrap();
        assert_eq!(proof["route"], "/settings");
        assert_eq!(proof["old_task_ready_before_worker_release"], true);
        assert_eq!(proof["actual_worker_body_completed"], true);
        return;
    }
    assert!(std::env::var_os("RUST_MIN_STACK").is_none());
    // The enclosing reviewed runner must also attest a private network namespace
    // with only lo. This local check rejects visible non-loopback interfaces.
    assert!(if_addrs::get_if_addrs().unwrap().iter().all(|interface| interface.name == "lo"));
    let owned = PathBuf::from(std::env::var_os("GS_CONNECT_SETTINGS_OWNED").unwrap());
    assert_eq!(PathBuf::from(std::env::var_os("XDG_DATA_HOME").unwrap()), owned.join("data"));
    assert!(!owned.join("data/gs26/storage.json").exists());
    DISCOVERY_TASKS.with(|tasks| tasks.borrow_mut().clear());
    let probe = crate::native_station::discovery_test_barrier::Probe::install();
    let mut dom = VirtualDom::new(router_root);
    let (original_base, original_status) = dom.in_scope(ScopeId::ROOT, || {
        assert!(UrlConfig::_stored_base_url().is_none());
        assert!(crate::auth::current_token().is_none());
        (UrlConfig::base_http(), serde_json::to_value(crate::auth::current_status()).unwrap())
    });
    dom.rebuild_in_place();
    // Real effect/task scheduler only; no direct call of discover_stations.
    let deadline = Instant::now() + Duration::from_secs(3);
    while DISCOVERY_TASKS.with(|tasks| tasks.borrow().is_empty()) {
        assert!(Instant::now() < deadline, "actual Connect discovery task did not register");
        dom.process_events();
        dom.render_immediate_to_vec();
    }
    let task = DISCOVERY_TASKS.with(|tasks| { let tasks = tasks.borrow(); assert_eq!(tasks.len(), 1); tasks[0] });
    assert!(matches!(dom.in_scope(ScopeId::ROOT, || task.poll_now()), Poll::Pending));
    probe.entered.recv_timeout(Duration::from_secs(3)).expect("actual spawn_blocking worker entered");
    assert!(matches!(probe.completed.try_recv(), Err(mpsc::TryRecvError::Empty)));
    assert!(component_names(&dom).iter().any(|name| name.ends_with("Connect")));
    dioxus::html::set_event_converter(Box::new(dioxus::html::SerializedHtmlEventConverter));
    let event = dioxus::html::PlatformEventData::new(Box::new(dioxus::html::SerializedMouseData::default()));
    dom.runtime().handle_event("click", Event::new(Rc::new(event) as Rc<dyn std::any::Any>, true), settings_button(&dom));
    dom.render_immediate_to_vec();
    assert_settings(&dom, &original_base, &original_status);
    assert!(matches!(probe.completed.try_recv(), Err(mpsc::TryRecvError::Empty)), "worker still held while old task retires");
    assert!(matches!(dom.in_scope(ScopeId::ROOT, || task.poll_now()), Poll::Ready(())), "actual ScopeDrop removed pending task without any test cancel/pause");
    probe.release();
    assert!(probe.completed.recv_timeout(Duration::from_secs(3)).expect("actual unmodified worker body completed"));
    assert!(matches!(probe.entered.try_recv(), Err(mpsc::TryRecvError::Empty)), "one genuine discovery only");
    dom.process_events(); dom.render_immediate_to_vec();
    assert_settings(&dom, &original_base, &original_status);
    assert!(matches!(dom.in_scope(ScopeId::ROOT, || task.poll_now()), Poll::Ready(())));
    let proof = serde_json::json!({"route":"/settings","old_task_ready_before_worker_release":true,"actual_worker_body_completed":true,"scope":"real Connect Settings click/scoped cancellation and original worker body in PRIVATE network; no LAN discovery success, WebView/Apple/hardware/panic cause claim"});
    std::fs::write(owned.join("actual-transition-proof.json"), serde_json::to_vec_pretty(&proof).unwrap()).unwrap();
    println!("ACTUAL_CONNECT_SETTINGS_TRANSITION {proof}");
    // Probe Drop unblocks on unwind too. After this function returns, the real
    // tokio test runtime joins its blocking work; parent requires child exit0.
    drop(dom); drop(probe);
}

fn app_entry_root() -> Element {
    use_hook(|| {
        dioxus::history::provide_history_context(Rc::new(dioxus::history::MemoryHistory::with_initial_path("/")));
        provide_context(Rc::new(dioxus::document::NoOpDocument) as Rc<dyn dioxus::document::Document>);
    });
    rsx! { AppContent {} }
}
fn click_button(dom: &mut VirtualDom, text: &str) {
    let event = dioxus::html::PlatformEventData::new(Box::new(dioxus::html::SerializedMouseData::default()));
    let button = button_with_text(dom, text);
    dom.runtime().handle_event("click", Event::new(Rc::new(event) as Rc<dyn std::any::Any>, true), button);
    dom.render_immediate_to_vec();
}

/// The router and document assets `App` really mounts, from the launch route.
/// Platform bootstrap (WebView, DesktopContext, station assets) is excluded, so
/// this is not a reproduction of a device panic and proves nothing about iOS.
#[tokio::test(flavor = "current_thread")]
async fn actual_app_content_entry_connect_settings_back_settings_has_no_panic() {
    const CHILD: &str = "GS_APP_ENTRY_SETTINGS_CHILD";
    if std::env::var(CHILD).as_deref() != Ok("1") {
        let nonce = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos();
        let owned = std::env::temp_dir().join(format!("gs-app-entry-settings-{}-{nonce}", std::process::id()));
        std::fs::create_dir_all(&owned).unwrap();
        let exact = format!("{}::actual_app_content_entry_connect_settings_back_settings_has_no_panic", module_path!().split_once("::").unwrap().1);
        let mut child = Command::new(std::env::current_exe().unwrap())
            .args(["--exact", &exact, "--nocapture", "--test-threads=1"])
            .env(CHILD, "1")
            .env("XDG_DATA_HOME", owned.join("data")).env("XDG_CONFIG_HOME", owned.join("config")).env("XDG_CACHE_HOME", owned.join("cache"))
            .spawn().unwrap();
        let start = Instant::now();
        let exit = loop {
            if let Some(status) = child.try_wait().unwrap() { break Some(status); }
            if start.elapsed() > Duration::from_secs(30) { child.kill().unwrap(); child.wait().unwrap(); break None; }
            std::thread::sleep(Duration::from_millis(10));
        };
        std::fs::remove_dir_all(&owned).unwrap();
        assert!(exit.is_some_and(|status| status.success()), "isolated app entry child failed: {exit:?}");
        return;
    }
    // Connect runs genuine LAN discovery, so stay off real networks as above.
    assert!(if_addrs::get_if_addrs().unwrap().iter().all(|interface| interface.name == "lo"));
    let mut dom = VirtualDom::new(app_entry_root);
    let (original_base, original_status) = dom.in_scope(ScopeId::ROOT, || {
        assert!(UrlConfig::_stored_base_url().is_none());
        (UrlConfig::base_http(), serde_json::to_value(crate::auth::current_status()).unwrap())
    });
    dom.rebuild_in_place();
    let deadline = Instant::now() + Duration::from_secs(3);
    while !component_names(&dom).iter().any(|name| name.ends_with("Connect")) {
        assert!(Instant::now() < deadline, "launch route did not redirect to Connect");
        dom.process_events();
        dom.render_immediate_to_vec();
    }
    dioxus::html::set_event_converter(Box::new(dioxus::html::SerializedHtmlEventConverter));
    for _ in 0..2 {
        click_button(&mut dom, "Settings");
        assert_settings(&dom, &original_base, &original_status);
        click_button(&mut dom, "Back");
        assert!(component_names(&dom).iter().any(|name| name.ends_with("Connect")));
        assert!(!dioxus::ssr::render(&dom).contains("Encountered panic"));
        dom.in_scope(ScopeId::APP, || assert_eq!(dioxus::history::history().current_route(), "/connect"));
    }
    drop(dom);
}
