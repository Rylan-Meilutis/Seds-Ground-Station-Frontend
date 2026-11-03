//! Production OTA HTTP helpers, actual Dioxus owners, inert loopback responders only.
use super::*;
use std::{
    future::Future,
    io::{Read, Write},
    net::TcpListener,
    pin::Pin,
    sync::{Mutex, mpsc},
    time::{Duration, Instant},
};
fn session(token: &str, allowed: bool) {
    auth::set_current_session(auth::StoredAuthSession {
        token: token.into(),
        remember_me: false,
        session: auth::SessionStatus {
            authenticated: true,
            username: Some("fixture-operator".into()),
            permissions: auth::Permissions {
                view_data: true,
                send_commands: allowed,
                ..Default::default()
            },
            allowed_commands: vec!["FirmwareUpdate".into()],
            ..Default::default()
        },
    });
}
fn dom(base: &str) -> VirtualDom {
    let mut dom = VirtualDom::new(|| rsx! {div{}});
    dom.rebuild_in_place();
    dom.in_scope(ScopeId::ROOT, || {
        super::super::UrlConfig::set_base_url_and_persist(base.into());
        *super::super::WS_EPOCH.write() = 7;
        *super::super::DASHBOARD_LIFE.write() = super::super::DashboardLife::new_alive();
        *super::super::FRONTEND_NETWORK_METRICS_STATE.lock().unwrap() = Default::default();
        session("fixture-original", true);
    });
    dom
}
async fn in_dom<F: Future>(dom: &VirtualDom, mut future: Pin<&mut F>) -> F::Output {
    std::future::poll_fn(|cx| dom.in_scope(ScopeId::ROOT, || future.as_mut().poll(cx))).await
}
struct Server {
    base: String,
    seen: Arc<Mutex<Vec<String>>>,
    stop: Arc<AtomicBool>,
    release: Option<mpsc::Sender<()>>,
    worker: Option<std::thread::JoinHandle<()>>,
}
impl Server {
    fn new(held: bool) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let seen = Arc::new(Mutex::new(Vec::new()));
        let stop = Arc::new(AtomicBool::new(false));
        let (tx, rx) = mpsc::channel();
        let requests = seen.clone();
        let stopped = stop.clone();
        let worker = std::thread::spawn(move || {
            while !stopped.load(Ordering::Acquire) {
                match listener.accept() {
                    Ok((mut socket, _)) => {
                        socket
                            .set_read_timeout(Some(Duration::from_secs(2)))
                            .unwrap();
                        let mut bytes = Vec::new();
                        let mut buf = [0u8; 4096];
                        while !bytes.windows(4).any(|b| b == b"\r\n\r\n") {
                            let n = socket.read(&mut buf).unwrap();
                            if n == 0 {
                                break;
                            }
                            bytes.extend_from_slice(&buf[..n]);
                        }
                        requests
                            .lock()
                            .unwrap()
                            .push(String::from_utf8_lossy(&bytes).into_owned());
                        if held {
                            let _ = rx.recv_timeout(Duration::from_secs(5));
                        }
                        let body = r#"{"id":7,"board":"AB","board_label":"Actuator Board","filename":"fixture.seds","artifact_kind":"launchcore_delta","phase":"prepared_reboot_unverified","bytes_sent":1,"total_bytes":1,"progress_percent":100.0,"board_max_bytes":4096,"message":"fixture prepared"}"#;
                        let response = format!(
                            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                            body.len()
                        );
                        let _ = socket.write_all(response.as_bytes());
                    }
                    Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                        std::thread::sleep(Duration::from_millis(2))
                    }
                    Err(e) => panic!("fixture accept: {e}"),
                }
            }
        });
        Self {
            base,
            seen,
            stop,
            release: Some(tx),
            worker: Some(worker),
        }
    }
    fn release(&mut self) {
        if let Some(tx) = self.release.take() {
            let _ = tx.send(());
        }
    }
}
impl Drop for Server {
    fn drop(&mut self) {
        self.release();
        self.stop.store(true, Ordering::Release);
        if let Some(worker) = self.worker.take() {
            worker.join().unwrap();
        }
    }
}
#[test]
fn firmware_prepared_and_legacy_complete_never_claim_verified_reboot() {
    assert!(terminal_phase("prepared_reboot_unverified"));
    assert!(terminal_phase("completed"));
    assert!(!terminal_phase("finishing"));
    assert!(phase_label("prepared_reboot_unverified").contains("not verified"));
    assert!(phase_label("completed").contains("not verified"));
    assert!(phase_label("rebooting").contains("not verified"));
}
#[tokio::test(flavor = "current_thread")]
async fn firmware_actual_posts_reject_retired_owners_before_any_send() {
    for case in [
        "station",
        "auth_aba",
        "epoch",
        "lifetime",
        "selection",
        "permission",
    ] {
        let server = Server::new(false);
        let dom = dom(&server.base);
        let alive = Arc::new(AtomicBool::new(true));
        let selected = Arc::new(AtomicBool::new(true));
        let selection = selected.clone();
        let owner = dom.in_scope(ScopeId::ROOT, || {
            capture_owner(alive.clone(), move || {
                selection.load(Ordering::Acquire) && auth::can_send_command("FirmwareUpdate")
            })
            .unwrap()
        });
        dom.in_scope(ScopeId::ROOT, || match case {
            "station" => {
                super::super::UrlConfig::set_base_url_and_persist("http://other.invalid".into())
            }
            "auth_aba" => {
                session("fixture-replacement", true);
                session("fixture-original", true)
            }
            "epoch" => *super::super::WS_EPOCH.write() += 1,
            "lifetime" => alive.store(false, Ordering::Release),
            "selection" => selected.store(false, Ordering::Release),
            "permission" => session("fixture-original", false),
            _ => unreachable!(),
        });
        let mut upload = Box::pin(upload_firmware(
            "AB",
            "fixture.seds",
            vec![1],
            owner.clone(),
        ));
        assert!(in_dom(&dom, upload.as_mut()).await.is_err(), "{case}");
        let mut cancel = Box::pin(cancel_firmware(7, owner));
        assert!(in_dom(&dom, cancel.as_mut()).await.is_err(), "{case}");
        assert!(
            server.seen.lock().unwrap().is_empty(),
            "{case}: stale POST escaped"
        );
    }
}
#[tokio::test(flavor = "current_thread")]
async fn firmware_actual_held_response_cannot_follow_station_or_auth_switch() {
    let mut server = Server::new(true);
    let other = Server::new(false);
    let dom = dom(&server.base);
    let owner = dom.in_scope(ScopeId::ROOT, || {
        capture_owner(Arc::new(AtomicBool::new(true)), || true).unwrap()
    });
    let mut upload = Box::pin(upload_firmware(
        "AB",
        "fixture.seds",
        vec![1],
        owner.clone(),
    ));
    let seen = server.seen.clone();
    tokio::select! {result=in_dom(&dom,upload.as_mut())=>panic!("fixture released too soon: {result:?}"),_=async{let end=Instant::now()+Duration::from_secs(3);while seen.lock().unwrap().is_empty(){assert!(Instant::now()<end);tokio::time::sleep(Duration::from_millis(2)).await;}}=>{}}
    dom.in_scope(ScopeId::ROOT, || {
        super::super::UrlConfig::set_base_url_and_persist(other.base.clone());
        session("fixture-new-station", true);
    });
    server.release();
    assert!(in_dom(&dom, upload.as_mut()).await.is_err());
    let mut cancel = Box::pin(cancel_firmware(7, owner));
    assert!(in_dom(&dom, cancel.as_mut()).await.is_err());
    assert!(other.seen.lock().unwrap().is_empty());
    assert_eq!(
        auth::current_token().as_deref(),
        Some("fixture-new-station")
    );
    assert_eq!(server.seen.lock().unwrap().len(), 1);
}
#[tokio::test(flavor = "current_thread")]
async fn firmware_actual_current_owner_upload_and_cancel_use_original_receiver() {
    let server = Server::new(false);
    let dom = dom(&server.base);
    let owner = dom.in_scope(ScopeId::ROOT, || {
        capture_owner(Arc::new(AtomicBool::new(true)), || true).unwrap()
    });
    let mut upload = Box::pin(upload_firmware(
        "AB",
        "fixture.seds",
        vec![1],
        owner.clone(),
    ));
    assert_eq!(
        in_dom(&dom, upload.as_mut()).await.unwrap().phase,
        "prepared_reboot_unverified"
    );
    let mut cancel = Box::pin(cancel_firmware(7, owner));
    assert_eq!(in_dom(&dom, cancel.as_mut()).await.unwrap().id, 7);
    let requests = server.seen.lock().unwrap();
    assert_eq!(requests.len(), 2);
    assert!(requests[0].starts_with("POST /api/firmware/flash/AB "));
    assert!(requests[1].starts_with("POST /api/firmware/updates/7/cancel "));
}

#[tokio::test(flavor = "current_thread")]
async fn firmware_actual_held_poll_cannot_paint_status_after_owner_switch() {
    let mut server = Server::new(true);
    let other = Server::new(false);
    let dom = dom(&server.base);
    let owner = dom.in_scope(ScopeId::ROOT, || {
        capture_owner(Arc::new(AtomicBool::new(true)), || true).unwrap()
    });
    let status = dom.in_scope(ScopeId::ROOT, || Signal::new(None::<FirmwareUpdateStatus>));
    let mut poll = Box::pin(poll_update(7, status, owner));
    let seen = server.seen.clone();
    tokio::select! { _=in_dom(&dom,poll.as_mut())=>panic!("held poll returned before fixture release"),_=async {let end=Instant::now()+Duration::from_secs(3);while seen.lock().unwrap().is_empty(){assert!(Instant::now()<end);tokio::time::sleep(Duration::from_millis(2)).await;}}=>{} }
    dom.in_scope(ScopeId::ROOT, || {
        super::super::UrlConfig::set_base_url_and_persist(other.base.clone());
        session("fixture-new-station", true);
    });
    server.release();
    in_dom(&dom, poll.as_mut()).await;
    dom.in_scope(ScopeId::ROOT, || assert!(status.read().is_none()));
    assert!(other.seen.lock().unwrap().is_empty());
    assert_eq!(
        auth::current_token().as_deref(),
        Some("fixture-new-station")
    );
}
