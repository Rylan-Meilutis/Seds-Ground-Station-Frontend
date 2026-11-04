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
async fn in_dom<F: Future>(dom: &VirtualDom, mut future: Pin<&mut F>) -> F::Output {
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
