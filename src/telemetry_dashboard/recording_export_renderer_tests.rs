//! Actual Dioxus scopes + production request/owner/file paths, loopback HTTP only.
use super::*;
use crate::{auth, telemetry_dashboard as dashboard};
use std::{
    cell::RefCell,
    future::Future,
    io::{Read, Write},
    net::TcpListener,
    pin::Pin,
    rc::Rc,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
    time::{Duration, Instant},
};

#[derive(Clone, Copy)]
pub(super) struct Handle {
    pub(super) preview: Signal<Option<Preview>>,
    pub(super) selected: Signal<BTreeSet<String>>,
    pub(super) loaded_window: Signal<Option<(i64, i64)>>,
    pub(super) owner_state: Signal<Option<Rc<ExportOwner>>>,
    pub(super) busy: Signal<bool>,
}
thread_local! {
    static HANDLE: RefCell<Option<Handle>> = const { RefCell::new(None) };
}
pub(super) fn capture(handle: Handle) {
    HANDLE.with(|slot| *slot.borrow_mut() = Some(handle));
}
fn handle() -> Handle {
    HANDLE.with(|slot| slot.borrow().as_ref().copied().unwrap())
}
static RECORDING: GlobalSignal<String> = Signal::global(|| "groundstation_recording_1.db".into());
fn report_app() -> Element {
    rsx! { ReportStudio { recording: RECORDING() } }
}
fn session(token: &str) {
    auth::set_current_session(auth::StoredAuthSession {
        token: token.to_owned(),
        remember_me: false,
        session: auth::SessionStatus {
            authenticated: true,
            username: Some("fixture-operator".into()),
            permissions: auth::Permissions {
                view_data: true,
                ..Default::default()
            },
            ..Default::default()
        },
    });
}
fn setup(dom: &VirtualDom, base: &str) {
    dom.in_scope(ScopeId::ROOT, || {
        dashboard::UrlConfig::set_base_url_and_persist(base.to_owned());
        *dashboard::WS_EPOCH.write() = 3;
        *dashboard::DASHBOARD_LIFE.write() = dashboard::DashboardLife::new_alive();
        *dashboard::FRONTEND_NETWORK_METRICS_STATE.lock().unwrap() = Default::default();
        session("fixture-original");
    });
}
fn dom(base: &str) -> VirtualDom {
    let mut dom = VirtualDom::new(|| rsx! { div {} });
    dom.rebuild_in_place();
    setup(&dom, base);
    dom
}

#[test]
fn actual_renderer_owner_retires_each_identity_aba_without_reusing_strings() {
    for case in ["auth", "url", "connection", "data", "provider", "selection"] {
        let dom = dom("http://fixture.invalid");
        let selection = Arc::new(std::sync::atomic::AtomicU64::new(1));
        let selected = selection.clone();
        let owner = dom.in_scope(ScopeId::ROOT, || {
            ExportOwner::capture_selected(move || selected.load(Ordering::SeqCst) == 1).unwrap()
        });
        dom.in_scope(ScopeId::ROOT, || {
            assert!(owner.is_current());
            match case {
                "auth" => {
                    session("fixture-replacement");
                    session("fixture-original");
                }
                "url" => {
                    dashboard::UrlConfig::set_base_url_and_persist("http://other.invalid".into());
                    dashboard::UrlConfig::set_base_url_and_persist("http://fixture.invalid".into());
                }
                "connection" => {
                    let mut metrics = dashboard::FRONTEND_NETWORK_METRICS_STATE.lock().unwrap();
                    metrics.ws_disconnects_total += 1;
                }
                "data" => {
                    *dashboard::FRONTEND_DATA_CLEAR_EPOCH.write() += 2;
                }
                "provider" => {
                    *dashboard::DASHBOARD_LIFE.write() = dashboard::DashboardLife::new_alive();
                }
                "selection" => {
                    selection.store(3, Ordering::SeqCst);
                }
                _ => unreachable!(),
            }
            assert!(!owner.is_current(), "{case}");
            assert!(ExportOwner::capture().unwrap().is_current());
        });
    }
}

struct Server {
    base: String,
    seen: mpsc::Receiver<String>,
    release: Option<mpsc::Sender<()>>,
    start: Option<mpsc::Sender<()>>,
    stop: Arc<AtomicBool>,
    worker: Option<std::thread::JoinHandle<()>>,
}
impl Server {
    fn new(csv: bool) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let (seen_tx, seen) = mpsc::channel();
        let (release, wait) = mpsc::channel();
        let (start, started) = mpsc::channel();
        let stop = Arc::new(AtomicBool::new(false));
        let canceled = stop.clone();
        let worker = std::thread::spawn(move || {
            if started.recv_timeout(Duration::from_secs(30)).is_err()
                || canceled.load(Ordering::Acquire)
            {
                return;
            }
            let deadline = Instant::now() + Duration::from_secs(5);
            let mut socket = loop {
                if canceled.load(Ordering::Acquire) || Instant::now() > deadline {
                    return;
                }
                match listener.accept() {
                    Ok((socket, _)) => break socket,
                    Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                        std::thread::sleep(Duration::from_millis(2))
                    }
                    Err(e) => panic!("{e}"),
                }
            };
            socket
                .set_read_timeout(Some(Duration::from_secs(3)))
                .unwrap();
            let mut request = Vec::new();
            let mut buffer = [0u8; 512];
            while !request.ends_with(b"\r\n\r\n") {
                match socket.read(&mut buffer) {
                    Ok(0) | Err(_) => return,
                    Ok(n) => request.extend_from_slice(&buffer[..n]),
                }
                if request.len() > 8192 {
                    return;
                }
            }
            if csv {
                let _=socket.write_all(b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\nConnection: close\r\n\r\n4\r\nrow\n\r\n");
                let _ = socket.flush();
            }
            let _ = seen_tx.send(String::from_utf8(request).unwrap());
            let _ = wait.recv_timeout(Duration::from_secs(5));
            if csv {
                let _ = socket.write_all(b"0\r\n\r\n");
            } else {
                let body=serde_json::json!({"start_ms":1000,"end_ms":2000,"recorded_rows":2,"channels":[{"key":"fixture-selected","label":"Original saved channel","count":2,"points":[[1000,1.0],[1999,2.0]],"regression":{}}]}).to_string();
                let response = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                );
                let _ = socket.write_all(response.as_bytes());
            }
        });
        Self {
            base,
            seen,
            release: Some(release),
            start: Some(start),
            stop,
            worker: Some(worker),
        }
    }
    fn start(&mut self) {
        if let Some(send) = self.start.take() {
            let _ = send.send(());
        }
    }
    fn release(&mut self) {
        if let Some(send) = self.release.take() {
            let _ = send.send(());
        }
    }
}
impl Drop for Server {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        self.start();
        self.release();
        if let Some(worker) = self.worker.take() {
            worker.join().unwrap();
        }
    }
}
async fn poll_in_dom<F: Future>(dom: &VirtualDom, mut future: Pin<&mut F>) -> F::Output {
    std::future::poll_fn(|cx| dom.in_scope(ScopeId::ROOT, || future.as_mut().poll(cx))).await
}
async fn pump(dom: &mut VirtualDom, millis: u64) {
    let until = Instant::now() + Duration::from_millis(millis);
    while Instant::now() < until {
        let _ = tokio::time::timeout(Duration::from_millis(10), dom.wait_for_work()).await;
        dom.render_immediate_to_vec();
    }
}
fn click(dom: &VirtualDom, id: dioxus::core::ElementId) {
    dioxus::html::set_event_converter(Box::new(dioxus::html::SerializedHtmlEventConverter));
    let payload = dioxus::html::PlatformEventData::new(Box::new(
        dioxus::html::SerializedMouseData::default(),
    ));
    let event = Event::new(Rc::new(payload) as Rc<dyn std::any::Any>, true);
    dom.runtime().handle_event("click", event, id);
}

#[tokio::test(flavor = "current_thread")]
async fn actual_native_http_download_retires_auth_aba_and_cleans_owned_partial() {
    for case in [
        "current",
        "auth",
        "url",
        "data",
        "selection",
        "provider",
        "cancel",
    ] {
        let begin = Instant::now();
        let mut server = Server::new(true);
        let dom = dom(&server.base);
        eprintln!("download fixture {case}: prepared in {:?}", begin.elapsed());
        let selected = Arc::new(std::sync::atomic::AtomicU64::new(1));
        let selection = selected.clone();
        let owner = dom.in_scope(ScopeId::ROOT, || {
            ExportOwner::capture_selected(move || selection.load(Ordering::SeqCst) == 1).unwrap()
        });
        let directory = std::env::temp_dir().join(format!(
            "gs-export-renderer-{}-{}-{case}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir(&directory).unwrap();
        std::fs::write(directory.join("preexisting.csv"), b"keep").unwrap();
        let mut request = Box::pin(dashboard::recording_download::test_download_to_directory(
            "/api/recordings/groundstation_recording_1.db/csv",
            "fixture.csv",
            owner,
            &directory,
        ));
        let partials = || {
            std::fs::read_dir(&directory)
                .unwrap()
                .map(|e| e.unwrap().path())
                .filter(|p| p.extension().is_some_and(|e| e == "part"))
                .collect::<Vec<_>>()
        };
        server.start();
        let waiting = async {
            for _ in 0..200 {
                if !partials().is_empty() {
                    return;
                }
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
            panic!("actual request did not create its partial");
        };
        tokio::select! {result=poll_in_dom(&dom,request.as_mut())=>panic!("download {case} finished before release after {:?}: {result:?}", begin.elapsed()),_=waiting=>{}}
        let headers = server.seen.try_recv().unwrap();
        assert!(
            headers
                .to_ascii_lowercase()
                .contains("authorization: bearer fixture-original")
        );
        if case == "cancel" {
            drop(request);
            server.release();
            assert!(partials().is_empty());
        } else {
            dom.in_scope(ScopeId::ROOT, || match case {
                "auth" => {
                    session("fixture-replacement");
                    session("fixture-original");
                }
                "url" => {
                    dashboard::UrlConfig::set_base_url_and_persist("http://other.invalid".into());
                    dashboard::UrlConfig::set_base_url_and_persist(server.base.clone());
                }
                "data" => {
                    *dashboard::FRONTEND_DATA_CLEAR_EPOCH.write() += 1;
                }
                "selection" => {
                    selected.store(3, Ordering::SeqCst);
                }
                "provider" => {
                    *dashboard::DASHBOARD_LIFE.write() = dashboard::DashboardLife::new_alive();
                }
                _ => {}
            });
            server.release();
            let result =
                tokio::time::timeout(Duration::from_secs(3), poll_in_dom(&dom, request.as_mut()))
                    .await
                    .unwrap();
            assert_eq!(result.is_ok(), case == "current");
            assert!(partials().is_empty());
            let completed = std::fs::read_dir(&directory)
                .unwrap()
                .map(|e| e.unwrap().path())
                .filter(|p| p.file_name().unwrap() != "preexisting.csv")
                .collect::<Vec<_>>();
            assert_eq!(completed.len(), usize::from(case == "current"));
            if case == "current" {
                assert_eq!(std::fs::read(&completed[0]).unwrap(), b"row\n");
            }
        }
        assert_eq!(
            std::fs::read(directory.join("preexisting.csv")).unwrap(),
            b"keep"
        );
        std::fs::remove_dir_all(&directory).unwrap();
    }
}

#[tokio::test(flavor = "current_thread")]
async fn actual_report_button_rejects_late_preview_after_prop_or_auth_change() {
    for case in ["current", "recording", "auth"] {
        let mut server = Server::new(false);
        let mut dom = VirtualDom::new(report_app);
        setup(&dom, &server.base);
        dom.in_scope(ScopeId::ROOT, || {
            *RECORDING.write() = "groundstation_recording_1.db".into()
        });
        let edits = dom.rebuild_to_vec();
        let button = edits
            .edits
            .iter()
            .find_map(|edit| match edit {
                dioxus::core::Mutation::NewEventListener { name, id } if *name == "click" => {
                    Some(*id)
                }
                _ => None,
            })
            .expect("actual Load graphs listener");
        server.start();
        click(&dom, button);
        let mut headers = None;
        for _ in 0..100 {
            pump(&mut dom, 10).await;
            if let Ok(value) = server.seen.try_recv() {
                headers = Some(value);
                break;
            }
        }
        let headers = headers.expect("production preview request");
        assert!(headers.contains("/api/recordings/report?request="));
        assert!(
            headers
                .to_ascii_lowercase()
                .contains("authorization: bearer fixture-original")
        );
        if case == "recording" {
            dom.in_scope(ScopeId::ROOT, || {
                *RECORDING.write() = "groundstation_recording_2.db".into()
            });
            pump(&mut dom, 20).await;
        }
        if case == "auth" {
            dom.in_scope(ScopeId::ROOT, || {
                session("fixture-replacement");
                session("fixture-original");
            });
        }
        server.release();
        pump(&mut dom, 350).await;
        dom.in_scope(ScopeId::ROOT, || {
            let state = handle();
            assert_eq!(state.preview.read().is_some(), case == "current");
            assert_eq!(!state.selected.read().is_empty(), case == "current");
            assert!(!*state.busy.read());
            if case == "current" {
                assert!(state.owner_state.read().as_ref().unwrap().is_current());
                assert!(state.loaded_window.read().is_none());
            } else {
                assert!(state.loaded_window.read().is_none());
            }
        });
    }
}

/// A view mounted before its connection first came up (the tab a page opens
/// on) keeps its request owner when that connection connects. Any later drop
/// still retires it.
#[test]
fn owner_captured_before_first_connect_survives_the_connect_but_not_a_drop() {
    let dom = dom("http://fixture.invalid");
    dashboard::FRONTEND_NETWORK_METRICS_STATE.lock().unwrap().ws_connected = false;
    let owner = dom.in_scope(ScopeId::ROOT, || ExportOwner::capture().unwrap());
    dom.in_scope(ScopeId::ROOT, || {
        dashboard::FRONTEND_NETWORK_METRICS_STATE.lock().unwrap().ws_connected = true;
        assert!(owner.is_current(), "the first connect is not a connection change");
        {
            let mut metrics = dashboard::FRONTEND_NETWORK_METRICS_STATE.lock().unwrap();
            metrics.ws_connected = false;
            metrics.ws_disconnects_total += 1;
        }
        assert!(!owner.is_current(), "a drop after connecting retires it");
    });
}
