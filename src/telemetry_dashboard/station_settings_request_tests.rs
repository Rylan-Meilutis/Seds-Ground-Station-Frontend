#![cfg(not(target_arch = "wasm32"))]
use super::*;
use crate::{auth, telemetry_dashboard as dashboard};
use std::{
    future::Future,
    io::{Read, Write},
    net::TcpListener,
    pin::Pin,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};
fn session(token: &str) {
    auth::set_current_session(auth::StoredAuthSession {
        token: token.into(),
        remember_me: false,
        session: auth::SessionStatus {
            authenticated: true,
            username: Some("settings-operator".into()),
            permissions: auth::Permissions {
                view_data: true,
                send_commands: true,
                ..Default::default()
            },
            ..Default::default()
        },
    });
}
fn dom(base: &str) -> VirtualDom {
    let mut dom = VirtualDom::new(|| rsx! {div{}});
    dom.rebuild_in_place();
    dom.in_scope(ScopeId::ROOT, || {
        dashboard::UrlConfig::set_base_url_and_persist(base.into());
        *dashboard::WS_EPOCH.write() = 7;
        *dashboard::DASHBOARD_LIFE.write() = dashboard::DashboardLife::new_alive();
        *dashboard::FRONTEND_NETWORK_METRICS_STATE.lock().unwrap() = Default::default();
        session("settings-original");
    });
    dom
}
async fn in_dom<F: Future + ?Sized>(dom: &VirtualDom, mut future: Pin<&mut F>) -> F::Output {
    std::future::poll_fn(|cx| dom.in_scope(ScopeId::ROOT, || future.as_mut().poll(cx))).await
}
struct Server {
    base: String,
    seen: Arc<AtomicBool>,
    release: Arc<AtomicBool>,
    stop: Arc<AtomicBool>,
    requests: Arc<Mutex<Vec<(String, String)>>>,
    worker: Option<std::thread::JoinHandle<()>>,
}
impl Server {
    fn new(status: u16, held: bool) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let seen = Arc::new(AtomicBool::new(false));
        let release = Arc::new(AtomicBool::new(!held));
        let stop = Arc::new(AtomicBool::new(false));
        let requests = Arc::new(Mutex::new(Vec::new()));
        let (s, r, end, rows) = (
            seen.clone(),
            release.clone(),
            stop.clone(),
            requests.clone(),
        );
        let worker = std::thread::spawn(move || {
            while !end.load(Ordering::Acquire) {
                match listener.accept() {
                    Ok((mut socket, _)) => {
                        socket
                            .set_read_timeout(Some(Duration::from_secs(3)))
                            .unwrap();
                        let mut bytes = Vec::new();
                        let mut buffer = [0_u8; 2048];
                        let header_end = loop {
                            let n = socket.read(&mut buffer).unwrap();
                            assert!(n > 0);
                            bytes.extend_from_slice(&buffer[..n]);
                            if let Some(at) = bytes.windows(4).position(|s| s == b"\r\n\r\n") {
                                break at + 4;
                            }
                        };
                        let headers = String::from_utf8_lossy(&bytes[..header_end]).to_string();
                        let length = headers
                            .lines()
                            .find_map(|s| {
                                s.split_once(':')
                                    .filter(|(k, _)| k.eq_ignore_ascii_case("content-length"))
                                    .map(|(_, v)| v.trim().parse::<usize>().unwrap())
                            })
                            .unwrap_or(0);
                        while bytes.len() < header_end + length {
                            let n = socket.read(&mut buffer).unwrap();
                            assert!(n > 0);
                            bytes.extend_from_slice(&buffer[..n]);
                        }
                        rows.lock().unwrap().push((
                            headers,
                            String::from_utf8(bytes[header_end..header_end + length].to_vec())
                                .unwrap(),
                        ));
                        s.store(true, Ordering::Release);
                        while !r.load(Ordering::Acquire) && !end.load(Ordering::Acquire) {
                            std::thread::sleep(Duration::from_millis(2));
                        }
                        let body = r#"{"saved":true}"#;
                        let text = format!(
                            "HTTP/1.1 {status} Result\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                            body.len()
                        );
                        let _ = socket.write_all(text.as_bytes());
                    }
                    Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                        std::thread::sleep(Duration::from_millis(2))
                    }
                    Err(e) => panic!("fixture accept:{e}"),
                }
            }
        });
        Self {
            base,
            seen,
            release,
            stop,
            requests,
            worker: Some(worker),
        }
    }
}
impl Drop for Server {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        self.release.store(true, Ordering::Release);
        if let Some(worker) = self.worker.take() {
            worker.join().unwrap();
        }
    }
}
#[tokio::test(flavor = "current_thread")]
async fn actual_settings_post_retains_original_body_and_authorization() {
    let server = Server::new(200, false);
    let dom = dom(&server.base);
    let owner = dom.in_scope(ScopeId::ROOT, ExportOwner::capture).unwrap();
    let body = r#"{"target_pressure_psi":123.5,"utc_ms":1793744000123}"#.to_string();
    let mut request = Box::pin(post_owned::<serde_json::Value>(
        "/api/fill_targets",
        body.clone(),
        owner,
    ));
    let result = in_dom(&dom, request.as_mut()).await.unwrap();
    assert_eq!(result["saved"], true);
    let requests = server.requests.lock().unwrap();
    assert_eq!(requests.len(), 1);
    assert!(requests[0].0.starts_with("POST /api/fill_targets "));
    assert!(
        requests[0]
            .0
            .to_ascii_lowercase()
            .contains("authorization: bearer settings-original")
    );
    assert_eq!(requests[0].1, body);
}
#[tokio::test(flavor = "current_thread")]
async fn queued_settings_post_never_rebinds_after_station_or_credential_aba() {
    for change in ["station", "auth"] {
        let original = Server::new(200, false);
        let other = Server::new(200, false);
        let dom = dom(&original.base);
        let owner = dom.in_scope(ScopeId::ROOT, ExportOwner::capture).unwrap();
        let mut request = Box::pin(post_owned::<serde_json::Value>(
            "/api/calibration",
            "{}".into(),
            owner,
        ));
        dom.in_scope(ScopeId::ROOT, || {
            if change == "station" {
                dashboard::UrlConfig::set_base_url_and_persist(other.base.clone());
            } else {
                session("settings-new");
                session("settings-original");
            }
        });
        assert!(in_dom(&dom, request.as_mut()).await.is_err());
        assert!(original.requests.lock().unwrap().is_empty());
        assert!(other.requests.lock().unwrap().is_empty());
    }
}
#[tokio::test(flavor = "current_thread")]
async fn late_settings_response_cannot_return_old_config_or_clear_new_account() {
    for status in [200, 401] {
        let server = Server::new(status, true);
        let dom = dom(&server.base);
        let owner = dom.in_scope(ScopeId::ROOT, ExportOwner::capture).unwrap();
        let mut request = Box::pin(post_owned::<serde_json::Value>(
            "/api/calibration",
            "{}".into(),
            owner,
        ));
        tokio::time::timeout(Duration::from_secs(4),async{while !server.seen.load(Ordering::Acquire){tokio::select!{result=in_dom(&dom,request.as_mut())=>panic!("held fixture returned:{result:?}"),_=tokio::time::sleep(Duration::from_millis(5))=>{}}}}).await.unwrap();
        dom.in_scope(ScopeId::ROOT, || session("settings-replacement"));
        server.release.store(true, Ordering::Release);
        assert!(in_dom(&dom, request.as_mut()).await.is_err());
        assert_eq!(
            auth::current_token().as_deref(),
            Some("settings-replacement")
        );
        assert_eq!(
            server.requests.lock().unwrap().len(),
            1,
            "already accepted bytes cannot be recalled"
        );
    }
}

#[tokio::test(flavor = "current_thread")]
async fn actual_input_epoch_selection_retires_queued_http_before_io() {
    let server = Server::new(200, false);
    let dom = dom(&server.base);
    let epoch = Rc::new(std::cell::Cell::new(4_u64));
    let observed = epoch.clone();
    let owner = dom
        .in_scope(ScopeId::ROOT, || {
            ExportOwner::capture_selected(move || observed.get() == 4)
        })
        .unwrap();
    let mut request = Box::pin(post_owned::<serde_json::Value>(
        "/api/calibration",
        "{}".into(),
        owner.clone(),
    ));
    epoch.set(super::super::settings_navigation::next_input_epoch(
        epoch.get(),
    ));
    assert!(!dom.in_scope(ScopeId::ROOT, || owner.is_current()));
    assert!(in_dom(&dom, request.as_mut()).await.is_err());
    assert!(server.requests.lock().unwrap().is_empty());
}

#[tokio::test(flavor = "current_thread")]
async fn captured_confirmation_retains_exact_api_body_and_original_token() {
    let server = Server::new(200, false);
    let dom = dom(&server.base);
    let owner = dom.in_scope(ScopeId::ROOT, ExportOwner::capture).unwrap();
    let mut request = Box::pin(post_captured::<_, serde_json::Value>(
        "/api/gse/self-test-confirmation",
        &serde_json::json!({"confirmed":true}),
        Ok(owner),
    ));
    in_dom(&dom, request.as_mut()).await.unwrap();
    let rows = server.requests.lock().unwrap();
    assert_eq!(rows.len(), 1);
    assert!(
        rows[0]
            .0
            .starts_with("POST /api/gse/self-test-confirmation ")
    );
    assert!(
        rows[0]
            .0
            .to_ascii_lowercase()
            .contains("authorization: bearer settings-original")
    );
    assert_eq!(rows[0].1, r#"{"confirmed":true}"#);
}
#[tokio::test(flavor = "current_thread")]
async fn captured_confirmation_held_response_cannot_confirm_new_actor_or_clear_login() {
    for status in [200, 401] {
        let server = Server::new(status, true);
        let dom = dom(&server.base);
        let selected = Rc::new(std::cell::Cell::new(1_u64));
        let original = selected.clone();
        let owner = dom
            .in_scope(ScopeId::ROOT, || {
                ExportOwner::capture_selected(move || original.get() == 1)
            })
            .unwrap();
        let completion_owner = owner.clone();
        let mut request = Box::pin(post_captured::<_, serde_json::Value>(
            "/api/gse/self-test-confirmation",
            &serde_json::json!({"confirmed":true}),
            Ok(owner),
        ));
        tokio::time::timeout(Duration::from_secs(4),async{while !server.seen.load(Ordering::Acquire){tokio::select!{result=in_dom(&dom,request.as_mut())=>panic!("held confirmation returned:{result:?}"),_=tokio::time::sleep(Duration::from_millis(5))=>{}}}}).await.unwrap();
        dom.in_scope(ScopeId::ROOT, || {
            selected.set(2);
            session("confirmation-replacement");
        });
        server.release.store(true, Ordering::Release);
        assert!(in_dom(&dom, request.as_mut()).await.is_err());
        dom.in_scope(ScopeId::ROOT, || {
            assert!(!completion_owner.is_current());
            assert_eq!(
                auth::current_token().as_deref(),
                Some("confirmation-replacement")
            );
        });
    }
}

// These are the actual serialized fill endpoints and the same synchronous
// constructor used by both GSE surfaces. No backend or physical outputs run.
fn fill_body(path: &str) -> serde_json::Value {
    if path == "/api/command" {
        serde_json::json!("Turbocharge")
    } else {
        serde_json::json!({})
    }
}
#[tokio::test(flavor = "current_thread")]
async fn captured_fill_gestures_preserve_exact_route_body_and_authorization() {
    for path in ["/api/command", "/api/gse/abort/clear"] {
        let server = Server::new(200, false);
        let dom = dom(&server.base);
        let owner = dom.in_scope(ScopeId::ROOT, ExportOwner::capture).unwrap();
        let body = fill_body(path);
        let mut request = Box::pin(post_captured_no_redirect::<_, serde_json::Value>(
            path,
            &body,
            Ok(owner),
        ));
        in_dom(&dom, request.as_mut()).await.unwrap();
        let rows = server.requests.lock().unwrap();
        assert_eq!(rows.len(), 1);
        assert!(rows[0].0.starts_with(&format!("POST {path} ")));
        assert!(
            rows[0]
                .0
                .to_ascii_lowercase()
                .contains("authorization: bearer settings-original")
        );
        assert_eq!(rows[0].1, serde_json::to_string(&body).unwrap());
    }
}
#[tokio::test(flavor = "current_thread")]
async fn captured_fill_gestures_refuse_station_token_and_aba_changes_before_first_poll() {
    for path in ["/api/command", "/api/gse/abort/clear"] {
        for change in [
            "station",
            "same-user-new-token",
            "credential-aba",
            "dialog-before-confirm",
        ] {
            let original = Server::new(200, false);
            let other = Server::new(200, false);
            let dom = dom(&original.base);
            // Clear-abort captures here when opening its confirmation, NOT at
            // confirm or when the spawned future first runs.
            let owner = dom.in_scope(ScopeId::ROOT, ExportOwner::capture).unwrap();
            let body = fill_body(path);
            let mut request = Box::pin(post_captured_no_redirect::<_, serde_json::Value>(
                path,
                &body,
                Ok(owner.clone()),
            ));
            dom.in_scope(ScopeId::ROOT, || {
                if change == "station" {
                    dashboard::UrlConfig::set_base_url_and_persist(other.base.clone());
                } else {
                    session("fill-replacement");
                    if change == "credential-aba" {
                        session("settings-original");
                    }
                }
            });
            assert!(!dom.in_scope(ScopeId::ROOT, || owner.is_current()));
            assert!(
                in_dom(&dom, request.as_mut()).await.is_err(),
                "{path} {change}"
            );
            assert!(original.requests.lock().unwrap().is_empty());
            assert!(other.requests.lock().unwrap().is_empty());
            if change != "station" && change != "credential-aba" {
                assert_eq!(auth::current_token().as_deref(), Some("fill-replacement"));
            }
        }
    }
}
#[tokio::test(flavor = "current_thread")]
async fn held_fill_commands_and_status_cannot_clear_a_replacement_session() {
    for path in ["/api/command", "/api/gse/abort/clear", "/api/gse/status"] {
        for change_station in [false, true] {
            let original = Server::new(401, true);
            let other = Server::new(200, false);
            let dom = dom(&original.base);
            let owner = dom.in_scope(ScopeId::ROOT, ExportOwner::capture).unwrap();
            let body = fill_body(path);
            let mut request: Pin<Box<dyn Future<Output = Result<serde_json::Value, String>>>> =
                if path == "/api/gse/status" {
                    Box::pin(dashboard::recording_download::saved_json(
                        path,
                        owner.clone(),
                    ))
                } else {
                    Box::pin(post_captured_no_redirect(path, &body, Ok(owner.clone())))
                };
            tokio::time::timeout(Duration::from_secs(4),async {
                while !original.seen.load(Ordering::Acquire) {
                    tokio::select! {
                        result=in_dom(&dom,request.as_mut())=>panic!("held fill response returned:{result:?}"),
                        _=tokio::time::sleep(Duration::from_millis(5))=>{}
                    }
                }
            }).await.unwrap();
            dom.in_scope(ScopeId::ROOT, || {
                if change_station {
                    dashboard::UrlConfig::set_base_url_and_persist(other.base.clone());
                }
                session("fill-new-same-user-token");
            });
            original.release.store(true, Ordering::Release);
            assert!(in_dom(&dom, request.as_mut()).await.is_err());
            assert!(!dom.in_scope(ScopeId::ROOT, || owner.is_current()));
            assert_eq!(
                auth::current_token().as_deref(),
                Some("fill-new-same-user-token")
            );
            assert_eq!(original.requests.lock().unwrap().len(), 1);
            assert!(other.requests.lock().unwrap().is_empty());
        }
    }
}

fn dispatch_fill_click(dom: &VirtualDom, id: dioxus::core::ElementId) {
    dioxus::html::set_event_converter(Box::new(dioxus::html::SerializedHtmlEventConverter));
    let payload = dioxus::html::PlatformEventData::new(Box::new(
        dioxus::html::SerializedMouseData::default(),
    ));
    let event = Event::new(Rc::new(payload) as Rc<dyn std::any::Any>, true);
    dom.runtime().handle_event("click", event, id);
}
async fn pump_fill_dom(dom: &mut VirtualDom) {
    let end = std::time::Instant::now() + Duration::from_millis(350);
    while std::time::Instant::now() < end {
        let _ = tokio::time::timeout(Duration::from_millis(10), dom.wait_for_work()).await;
        dom.render_immediate_to_vec();
    }
}
#[tokio::test(flavor = "current_thread")]
async fn actual_clear_abort_component_captures_dialog_and_confirm_before_task_first_poll() {
    for change in [
        "current",
        "station-after-confirm",
        "token-after-confirm",
        "token-before-confirm",
    ] {
        let original = Server::new(200, false);
        let replacement = Server::new(200, false);
        let mut dom = VirtualDom::new(|| rsx! {dashboard::gse_panel::ClearGseAbort{}});
        dom.in_scope(ScopeId::ROOT, || {
            dashboard::UrlConfig::set_base_url_and_persist(original.base.clone());
            *dashboard::WS_EPOCH.write() = 7;
            *dashboard::DASHBOARD_LIFE.write() = dashboard::DashboardLife::new_alive();
            *dashboard::FRONTEND_NETWORK_METRICS_STATE.lock().unwrap() = Default::default();
            session("settings-original");
        });
        let initial = dom.rebuild_to_vec();
        let open = initial
            .edits
            .iter()
            .find_map(|edit| match edit {
                dioxus::core::Mutation::NewEventListener { name, id } if name == "click" => {
                    Some(*id)
                }
                _ => None,
            })
            .expect("actual clear-abort opening handler");
        dispatch_fill_click(&dom, open);
        let opened = dom.render_immediate_to_vec();
        assert!(dioxus::ssr::render(&dom).contains("Confirm clear abort"));
        let confirm = opened
            .edits
            .iter()
            .find_map(|edit| match edit {
                dioxus::core::Mutation::NewEventListener { name, id } if name == "click" => {
                    Some(*id)
                }
                _ => None,
            })
            .expect("actual confirmation handler");
        if change == "token-before-confirm" {
            dom.in_scope(ScopeId::ROOT, || session("fill-replacement"));
        }
        dispatch_fill_click(&dom, confirm);
        // No wait_for_work/pump/await between this actual handler dispatch and
        // replacement: a spawned legacy async helper has NOT been first-polled.
        dom.in_scope(ScopeId::ROOT, || match change {
            "station-after-confirm" => {
                dashboard::UrlConfig::set_base_url_and_persist(replacement.base.clone())
            }
            "token-after-confirm" => session("fill-replacement"),
            _ => (),
        });
        pump_fill_dom(&mut dom).await;
        let rows = original.requests.lock().unwrap();
        if change == "current" {
            assert_eq!(rows.len(), 1, "positive actual component must send");
            assert!(rows[0].0.starts_with("POST /api/gse/abort/clear "));
            assert_eq!(rows[0].1, "{}");
            assert!(dioxus::ssr::render(&dom).contains("GSE abort cleared."));
        } else {
            assert!(rows.is_empty(), "retired actual component gesture:{change}");
            assert!(!dioxus::ssr::render(&dom).contains("GSE abort cleared."));
        }
        assert!(
            replacement.requests.lock().unwrap().is_empty(),
            "never retarget new station"
        );
    }
}
