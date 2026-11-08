//! Real loopback layout/policy/mode server -> production runtime-mode layout
//! hook. Proves an in-process mode switch replaces the displayed layout.
use super::*;
use std::{
    io::{Read, Write},
    net::TcpListener,
    time::{Duration, Instant},
};

fn child(name: &str) -> bool {
    let exact = format!("telemetry_dashboard::station_mode_layout_native_tests::{name}");
    if std::env::var("GS_MODE_LAYOUT_CHILD").as_deref() == Ok(&exact) {
        return true;
    }
    let root = std::env::temp_dir().join(format!(
        "gs-mode-layout-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir(&root).unwrap();
    let result = std::process::Command::new(std::env::current_exe().unwrap())
        .args(["--exact", &exact, "--nocapture"])
        .env("GS_MODE_LAYOUT_CHILD", &exact)
        .env("XDG_DATA_HOME", root.join("data"))
        .env("XDG_CONFIG_HOME", root.join("config"))
        .current_dir(&root)
        .output()
        .unwrap();
    std::fs::remove_dir_all(root).unwrap();
    assert!(
        result.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&result.stdout),
        String::from_utf8_lossy(&result.stderr)
    );
    assert!(String::from_utf8_lossy(&result.stdout).contains("1 passed"));
    false
}

fn layout(test_fire_mode: bool, tabs: &[&str]) -> serde_json::Value {
    let mut layout: serde_json::Value =
        serde_json::from_str(include_str!("../../docs/api-examples/layout.full.json")).unwrap();
    layout["test_fire_mode"] = test_fire_mode.into();
    layout["main_tabs"] = tabs.iter().map(|t| serde_json::Value::from(*t)).collect();
    let parsed: LayoutConfig = serde_json::from_value(layout.clone()).unwrap();
    parsed.validate().unwrap();
    layout
}
fn hitl_layout() -> serde_json::Value {
    layout(false, &["state", "actions", "notifications"])
}
fn test_fire_layout() -> serde_json::Value {
    layout(true, &["state", "connection-status", "warnings"])
}

/// A new single-binary backend: server-fenced in-process options and no
/// client epoch fencing.
fn mode_snapshot(epoch: &str, switched: bool) -> serde_json::Value {
    let mut raw =
        serde_json::to_value(station_mode_contract::tests::example_snapshot()).unwrap();
    raw["epoch_fencing_supported"] = false.into();
    raw["backend_instance"] = "single-binary-process".into();
    raw["policy_epoch"] = epoch.into();
    raw["options"][0]["transition_kind"] = "restart".into();
    raw["options"][0]["in_process"] = true.into();
    if switched {
        raw["current"] = serde_json::json!({"id":"third-sim","mode_id":"custom α",
            "label":"Third team simulated","simulation":true,
            "output_domain":"Isolated simulated actions and telemetry"});
    }
    let decoded = station_mode_contract::Snapshot::decode(raw.clone()).unwrap();
    assert!(decoded.options[0].switches_in_process());
    raw
}

#[derive(Default)]
struct State {
    mode: serde_json::Value,
    layout: serde_json::Value,
    hold_layout: bool,
    layout_reads: usize,
    policy_reads: usize,
}
struct Server {
    base: String,
    state: Arc<Mutex<State>>,
    stop: Arc<AtomicBool>,
    thread: Option<std::thread::JoinHandle<()>>,
}
impl Server {
    fn new() -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let state = Arc::new(Mutex::new(State {
            mode: mode_snapshot("boot-a:epoch-1", false),
            layout: hitl_layout(),
            ..Default::default()
        }));
        let stop = Arc::new(AtomicBool::new(false));
        let (shared, end) = (state.clone(), stop.clone());
        let thread = std::thread::spawn(move || {
            let mut workers = Vec::new();
            while !end.load(Ordering::Acquire) {
                match listener.accept() {
                    Ok((mut stream, _)) => {
                        let (shared, end) = (shared.clone(), end.clone());
                        workers.push(std::thread::spawn(move || {
                            stream.set_nonblocking(false).unwrap();
                            stream.set_read_timeout(Some(Duration::from_secs(2))).unwrap();
                            let mut bytes = Vec::new();
                            let mut buffer = [0; 2048];
                            while let Ok(n) = stream.read(&mut buffer) {
                                if n == 0 {
                                    break;
                                }
                                bytes.extend_from_slice(&buffer[..n]);
                                if bytes.windows(4).any(|w| w == b"\r\n\r\n") {
                                    break;
                                }
                                assert!(bytes.len() < 65536);
                            }
                            let request = String::from_utf8(bytes).unwrap();
                            let path = request
                                .lines()
                                .next()
                                .and_then(|line| line.split_whitespace().nth(1))
                                .unwrap_or_default()
                                .to_owned();
                            let body = match path.as_str() {
                                "/api/station-modes" => shared.lock().unwrap().mode.clone(),
                                "/api/layout" => {
                                    shared.lock().unwrap().layout_reads += 1;
                                    let started = Instant::now();
                                    while shared.lock().unwrap().hold_layout
                                        && !end.load(Ordering::Acquire)
                                    {
                                        assert!(started.elapsed() < Duration::from_secs(5));
                                        std::thread::sleep(Duration::from_millis(2));
                                    }
                                    shared.lock().unwrap().layout.clone()
                                }
                                "/api/action_policy" => {
                                    shared.lock().unwrap().policy_reads += 1;
                                    serde_json::json!({"key_enabled":true,"controls":[]})
                                }
                                other => panic!("unexpected path {other}"),
                            };
                            let body = serde_json::to_vec(&body).unwrap();
                            let _ = write!(
                                stream,
                                "HTTP/1.1 200 Fixture\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                                body.len()
                            );
                            let _ = stream.write_all(&body);
                        }));
                    }
                    Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                        std::thread::sleep(Duration::from_millis(2))
                    }
                    Err(e) => panic!("{e}"),
                }
            }
            for worker in workers {
                worker.join().unwrap();
            }
        });
        Self { base, state, stop, thread: Some(thread) }
    }
    fn switch_to_test_fire(&self) {
        let mut state = self.state.lock().unwrap();
        state.mode = mode_snapshot("boot-a:epoch-2", true);
        state.layout = test_fire_layout();
    }
}
impl Drop for Server {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        self.state.lock().unwrap().hold_layout = false;
        self.thread.take().unwrap().join().unwrap();
    }
}

/// The dashboard's layout signals, the production runtime-mode hook, and a
/// render of what the operator sees from the displayed layout.
fn layout_root() -> Element {
    let initial: LayoutConfig = serde_json::from_value(hitl_layout()).unwrap();
    let layout_config = use_signal(|| Some(initial));
    let action_policy = use_signal(ActionPolicyMsg::default_locked);
    use_runtime_mode_layout_refresh(RuntimeLayoutSignals {
        layout_config,
        layout_loading: use_signal(|| false),
        layout_error: use_signal(|| None),
        layout_error_dismissed: use_signal(|| None),
        layout_request_base: use_signal(UrlConfig::base_http),
        calibration_has_sensors: use_signal(|| None),
        calibration_request_base: use_signal(String::new),
        action_policy,
    });
    let layout = layout_config.read();
    let layout = layout.as_ref().unwrap();
    let key = action_policy.read().key_enabled;
    rsx! {
        div {
            p { "test_fire_mode={layout.test_fire_mode}" }
            p { "key_enabled={key}" }
            for tab in layout.main_tabs.iter() { span { "tab:{tab};" } }
        }
    }
}

fn dom(base: &str) -> VirtualDom {
    let mut dom = VirtualDom::new(layout_root);
    dom.rebuild_in_place();
    dom.in_scope(ScopeId::ROOT, || {
        *BASE_URL.write() = base.into();
        *WS_EPOCH.write() = 7;
        *DASHBOARD_LIFE.write() = DashboardLife::new_alive();
        *FRONTEND_NETWORK_METRICS_STATE.lock().unwrap() =
            FrontendNetworkMetrics { ws_connected: true, ..Default::default() };
        auth::set_current_session(auth::StoredAuthSession {
            token: "incoming-token".into(),
            remember_me: false,
            session: auth::SessionStatus {
                authenticated: true,
                permissions: auth::Permissions {
                    view_data: true,
                    send_commands: true,
                    ..Default::default()
                },
                ..Default::default()
            },
        });
    });
    dom
}

async fn drive_until(
    dom: &mut VirtualDom,
    limit: Duration,
    mut done: impl FnMut(&mut VirtualDom) -> bool,
) -> bool {
    let start = Instant::now();
    while start.elapsed() < limit {
        if done(dom) {
            return true;
        }
        tokio::select! {
            _ = dom.wait_for_work() => { let _ = dom.render_immediate_to_vec(); },
            _ = tokio::time::sleep(Duration::from_millis(10)) => {},
        }
    }
    done(dom)
}

fn shows_hitl(html: &str) -> bool {
    html.contains("test_fire_mode=false")
        && html.contains("tab:actions;")
        && html.contains("tab:notifications;")
        && !html.contains("tab:warnings;")
}
fn shows_test_fire(html: &str) -> bool {
    html.contains("test_fire_mode=true")
        && html.contains("tab:connection-status;")
        && html.contains("tab:warnings;")
        && !html.contains("tab:actions;")
}

/// The actual websocket reducer, exactly as a live frame reaches it.
fn receive_frame(dom: &VirtualDom, frame: serde_json::Value) {
    dom.in_scope(ScopeId::ROOT, || {
        handle_ws_message(
            &frame.to_string(),
            0,
            Signal::new(Vec::<AlertMsg>::new()),
            Signal::new(Vec::<AlertMsg>::new()),
            Signal::new(0),
            Signal::new(0),
            Signal::new(false),
            Signal::new(Vec::<PersistentNotification>::new()),
            Signal::new(Vec::<PersistentNotification>::new()),
            Signal::new(Vec::<PersistentNotification>::new()),
            Signal::new(Vec::<DismissedNotification>::new()),
            Signal::new(Vec::<u64>::new()),
            ACTION_POLICY_SIGNAL.signal(),
            Signal::new(RecordingStatusMsg { mode: "idle".into(), db_path: None }),
            Signal::new(None),
            Signal::new(None),
            Signal::new(None),
            Signal::new(NetworkTopologyMsg::default()),
            Signal::new(0),
            Signal::new(0),
            Signal::new("Unknown".into()),
            Signal::new(Vec::<BoardStatusEntry>::new()),
            Signal::new(None),
            Signal::new(None),
            Signal::new(None),
            Signal::new(None),
        )
    });
}

async fn observe_initial_mode(dom: &mut VirtualDom) {
    let raw = incoming_epoch_native_tests::mode_get_completion(dom).await.unwrap();
    assert_eq!(raw["policy_epoch"], "boot-a:epoch-1");
    // The first observation only records the displayed epoch; it never refetches.
    drive_until(dom, Duration::from_millis(100), |_| false).await;
    assert!(shows_hitl(&dioxus::ssr::render(dom)));
}

#[tokio::test(flavor = "current_thread")]
async fn station_mode_changed_frame_replaces_layout_without_waiting_for_mode_poll() {
    if !child("station_mode_changed_frame_replaces_layout_without_waiting_for_mode_poll") {
        return;
    }
    let server = Server::new();
    let mut dom = dom(&server.base);
    observe_initial_mode(&mut dom).await;
    assert_eq!(server.state.lock().unwrap().layout_reads, 0);
    let cache_key = layout_cache_key_for_base(&server.base);
    persist::set_string(&cache_key, &hitl_layout().to_string());

    server.switch_to_test_fire();
    server.state.lock().unwrap().hold_layout = true;
    let refresh_before = dom.in_scope(ScopeId::ROOT, || *station_mode_observer::REFRESH.peek());
    receive_frame(
        &dom,
        serde_json::json!({"ty":"StationModeChanged","data":{"mode":"test_fire",
            "label":"Test fire","policy_epoch":"boot-a:epoch-2","layout_revision":4}}),
    );
    dom.in_scope(ScopeId::ROOT, || {
        assert_eq!(
            *station_mode_observer::REFRESH.peek(),
            refresh_before + 1,
            "the frame asks the mode observer for an immediate readback"
        );
        assert_eq!(STATION_MODE_CHANGE.peek().1.as_deref(), Some("boot-a:epoch-2"));
    });

    // Held /api/layout: the old mode's saved layout is gone, so a reload while
    // the refetch is in flight cannot restore it.
    let shared = server.state.clone();
    assert!(
        drive_until(&mut dom, Duration::from_secs(1), |_| shared.lock().unwrap().layout_reads == 1)
            .await,
        "the notification alone starts the layout refetch"
    );
    assert!(persist::get_string(&cache_key).is_none());
    assert!(shows_hitl(&dioxus::ssr::render(&dom)));

    server.state.lock().unwrap().hold_layout = false;
    let started = Instant::now();
    assert!(
        drive_until(&mut dom, Duration::from_secs(3), |dom| shows_test_fire(
            &dioxus::ssr::render(dom)
        ))
        .await,
        "{}",
        dioxus::ssr::render(&dom)
    );
    assert!(started.elapsed() < Duration::from_secs(2), "faster than the 2 s mode poll");
    assert!(dioxus::ssr::render(&dom).contains("key_enabled=true"));
    {
        let state = server.state.lock().unwrap();
        assert_eq!(state.layout_reads, 1);
        assert_eq!(state.policy_reads, 1);
    }
    let cached: serde_json::Value =
        serde_json::from_str(&persist::get_string(&cache_key).unwrap()).unwrap();
    assert_eq!(cached["test_fire_mode"], true);

    // The later mode readback carries the announced epoch: no second refetch.
    let raw = incoming_epoch_native_tests::mode_get_completion(&dom).await.unwrap();
    assert_eq!(raw["policy_epoch"], "boot-a:epoch-2");
    drive_until(&mut dom, Duration::from_millis(200), |_| false).await;
    assert_eq!(server.state.lock().unwrap().layout_reads, 1);
    assert!(shows_test_fire(&dioxus::ssr::render(&dom)));
}

#[tokio::test(flavor = "current_thread")]
async fn observed_policy_epoch_change_replaces_layout_once() {
    if !child("observed_policy_epoch_change_replaces_layout_once") {
        return;
    }
    let server = Server::new();
    let mut dom = dom(&server.base);
    observe_initial_mode(&mut dom).await;

    // No websocket notice (for example it was missed): the mode readback alone
    // shows the new epoch and the layout follows it.
    server.switch_to_test_fire();
    let raw = incoming_epoch_native_tests::mode_get_completion(&dom).await.unwrap();
    assert_eq!(raw["current"]["id"], "third-sim");
    assert!(
        drive_until(&mut dom, Duration::from_secs(3), |dom| shows_test_fire(
            &dioxus::ssr::render(dom)
        ))
        .await,
        "{}",
        dioxus::ssr::render(&dom)
    );
    // Repeated readbacks of the same epoch do not refetch again.
    incoming_epoch_native_tests::mode_get_completion(&dom).await.unwrap();
    drive_until(&mut dom, Duration::from_millis(200), |_| false).await;
    assert_eq!(server.state.lock().unwrap().layout_reads, 1);
}

#[test]
fn only_the_latest_layout_load_may_apply() {
    let mut dom = VirtualDom::new(|| rsx! { div {} });
    dom.rebuild_in_place();
    dom.in_scope(ScopeId::ROOT, || {
        let initial_fetch = next_layout_generation();
        assert!(layout_generation_current(initial_fetch));
        let mode_refresh = next_layout_generation();
        assert!(
            !layout_generation_current(initial_fetch),
            "a fetch started before the mode switch cannot replace the new layout"
        );
        assert!(layout_generation_current(mode_refresh));
    });
}
