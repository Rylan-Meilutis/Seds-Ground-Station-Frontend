//! Real mounted host selector and held loopback HTTP; production polling is unchanged.
use super::super as dashboard;
use super::*;
use std::{
    collections::BTreeMap,
    io::{Read, Write},
    net::{TcpListener, TcpStream},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
    time::Duration,
};

#[derive(Clone)]
struct Request {
    method: String,
    path: String,
    headers: BTreeMap<String, String>,
}
struct Reply {
    status: u16,
    body: serde_json::Value,
}
struct Server {
    base: String,
    requests: Arc<Mutex<Vec<Request>>>,
    changed: Arc<tokio::sync::Notify>,
    replies: mpsc::Sender<Reply>,
    stop: Arc<AtomicBool>,
    worker: Option<std::thread::JoinHandle<()>>,
}
impl Server {
    fn new() -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let requests = Arc::new(Mutex::new(Vec::new()));
        let changed = Arc::new(tokio::sync::Notify::new());
        let stop = Arc::new(AtomicBool::new(false));
        let (tx, rx) = mpsc::channel::<Reply>();
        let (rows, notify, stopping) = (requests.clone(), changed.clone(), stop.clone());
        let worker = std::thread::spawn(move || {
            while !stopping.load(Ordering::Acquire) {
                let (mut socket, _) = listener.accept().unwrap();
                if stopping.load(Ordering::Acquire) {
                    break;
                }
                socket
                    .set_read_timeout(Some(Duration::from_secs(5)))
                    .unwrap();
                let mut raw = Vec::new();
                let mut buf = [0; 2048];
                while !raw.windows(4).any(|v| v == b"\r\n\r\n") {
                    let n = socket.read(&mut buf).unwrap();
                    assert!(n > 0);
                    raw.extend_from_slice(&buf[..n]);
                    assert!(raw.len() < 16384);
                }
                let text = String::from_utf8(raw).unwrap();
                let mut words = text.lines().next().unwrap().split_whitespace();
                let request = Request {
                    method: words.next().unwrap().into(),
                    path: words.next().unwrap().into(),
                    headers: text
                        .lines()
                        .skip(1)
                        .filter_map(|v| {
                            v.split_once(':')
                                .map(|(k, v)| (k.to_ascii_lowercase(), v.trim().into()))
                        })
                        .collect(),
                };
                {
                    let mut guard = rows.lock().unwrap();
                    assert!(guard.len() < 16);
                    guard.push(request);
                }
                notify.notify_one();
                let reply = loop {
                    if stopping.load(Ordering::Acquire) {
                        return;
                    }
                    match rx.recv_timeout(Duration::from_millis(50)) {
                        Ok(reply) => break reply,
                        Err(mpsc::RecvTimeoutError::Timeout) => {}
                        Err(mpsc::RecvTimeoutError::Disconnected) => return,
                    }
                };
                let body = serde_json::to_vec(&reply.body).unwrap();
                let _ = write!(
                    socket,
                    "HTTP/1.1 {} Fixture\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                    reply.status,
                    body.len()
                );
                let _ = socket.write_all(&body);
            }
        });
        Self {
            base,
            requests,
            changed,
            replies: tx,
            stop,
            worker: Some(worker),
        }
    }
    fn reply(&self, status: u16, body: serde_json::Value) {
        self.replies.send(Reply { status, body }).unwrap();
    }
    fn count(&self) -> usize {
        self.requests.lock().unwrap().len()
    }
}
impl Drop for Server {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        let _ = TcpStream::connect(self.base.trim_start_matches("http://"));
        self.worker.take().unwrap().join().unwrap();
    }
}
fn root() -> Element {
    rsx! {TranscodingHosts{}}
}
fn session(token: &str) {
    crate::auth::set_current_session(crate::auth::StoredAuthSession {
        token: token.into(),
        remember_me: false,
        session: crate::auth::SessionStatus {
            authenticated: true,
            username: Some("host-manager".into()),
            roles: vec!["stream_admin".into(), "stream_master".into()],
            permissions: crate::auth::Permissions {
                view_data: true,
                send_commands: true,
                ..Default::default()
            },
            ..Default::default()
        },
    });
}
fn render(dom: &mut VirtualDom) -> String {
    dom.render_immediate_to_vec();
    dom.process_events();
    dom.render_immediate_to_vec();
    dioxus::ssr::render(dom)
}
async fn until(dom: &mut VirtualDom, server: &Server, predicate: impl Fn(&str) -> bool) -> String {
    tokio::time::timeout(Duration::from_secs(8), async {
        loop {
            let notified = server.changed.notified();
            let html = render(dom);
            if predicate(&html) {
                return html;
            }
            tokio::select! {_=dom.wait_for_work()=>{},_=notified=>{}}
        }
    })
    .await
    .expect("actual component/request barrier with unchanged 3-second production poll")
}
fn disabled(html: &str) -> bool {
    let select = html
        .split("<select")
        .nth(1)
        .expect("real mounted select")
        .split('>')
        .next()
        .unwrap();
    select.contains("disabled")
}
fn snapshot(host: bool) -> serde_json::Value {
    if host {
        serde_json::json!({"selected":"worker-a","hosts":[{"id":"worker-a","name":"Qualified worker","passed":true,"online":true,"capacity":2,"encoding_ms":900,"upload_bps":24000000,"download_bps":24000000}]})
    } else {
        serde_json::json!({"selected":null,"hosts":[]})
    }
}
#[tokio::test(flavor = "current_thread")]
async fn actual_host_snapshots_distinguish_loading_empty_failure_and_recovery() {
    if !dashboard::command_owner::isolated_test_child(
        module_path!(),
        "actual_host_snapshots_distinguish_loading_empty_failure_and_recovery",
    ) {
        if let Some(path) = std::env::var_os("GS_EXTERNAL_HOST_LEDGER") {
            let value: serde_json::Value = serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
            println!("ACTUAL_EXTERNAL_HOST_SNAPSHOTS {value}");
        }
        return;
    }
    let server = Server::new();
    let mut dom = VirtualDom::new(root);
    dom.in_scope(ScopeId::ROOT, || {
        dashboard::UrlConfig::set_base_url_and_persist(server.base.clone());
        *dashboard::WS_EPOCH.write() = 7;
        *dashboard::DASHBOARD_LIFE.write() = dashboard::DashboardLife::new_alive();
        *dashboard::FRONTEND_NETWORK_METRICS_STATE.lock().unwrap() =
            dashboard::FrontendNetworkMetrics {
                ws_connected: true,
                ..Default::default()
            };
        session("host-original");
    });
    dom.rebuild_in_place();
    let html = until(&mut dom, &server, |_| server.count() == 1).await;
    assert!(html.contains("Loading transcoding hosts"));
    assert!(disabled(&html));
    server.reply(503, serde_json::json!({"error":"temporary"}));
    let html = until(&mut dom, &server, |h| {
        h.contains("No successful host snapshot has been received")
    })
    .await;
    assert!(disabled(&html));
    assert!(!html.contains("last known and unverified"));
    until(&mut dom, &server, |_| server.count() == 2).await;
    server.reply(200, snapshot(false));
    let html = until(&mut dom, &server, |h| !disabled(h)).await;
    assert!(!html.contains("verification is unavailable"));
    assert!(!html.contains("Qualified worker"));
    until(&mut dom, &server, |_| server.count() == 3).await;
    server.reply(503, serde_json::json!({"error":"temporary"}));
    let html = until(&mut dom, &server, |h| {
        h.contains("last known and unverified")
    })
    .await;
    assert!(disabled(&html));
    assert!(!html.contains("No successful host snapshot")); // Empty success counts as a snapshot.
    until(&mut dom, &server, |_| server.count() == 4).await;
    server.reply(200, snapshot(true));
    let html = until(&mut dom, &server, |h| {
        h.contains("Qualified worker") && !disabled(h)
    })
    .await;
    assert!(!html.contains("last known and unverified"));
    assert!(!html.contains("Using original feeds"));
    until(&mut dom, &server, |_| server.count() == 5).await;
    server.reply(503, serde_json::json!({"error":"temporary"}));
    let html = until(&mut dom, &server, |h| {
        h.contains("last known and unverified")
    })
    .await;
    assert!(disabled(&html));
    assert!(html.contains("Qualified worker"));
    until(&mut dom, &server, |_| server.count() == 6).await;
    server.reply(200, snapshot(false));
    let html = until(&mut dom, &server, |h| !disabled(h)).await;
    assert!(!html.contains("Qualified worker"));
    assert!(!html.contains("verification is unavailable"));
    let requests = server.requests.lock().unwrap();
    assert_eq!(requests.len(), 6);
    for request in requests.iter() {
        assert_eq!(request.method, "GET");
        assert_eq!(request.path, "/api/video/transcode/hosts");
        assert_eq!(
            request.headers.get("authorization").map(String::as_str),
            Some("Bearer host-original")
        );
        assert!(!request.headers.contains_key("x-station-policy-epoch"));
    }
    // Read-only test evidence after every original periodic assertion.
    if let Some(path) = std::env::var_os("GS_EXTERNAL_HOST_LEDGER") {
        let rows: Vec<_> = requests.iter().map(|request| serde_json::json!({
            "method": request.method.as_str(),
            "path": request.path.as_str(),
            "original_auth_matches": request.headers.get("authorization").map(String::as_str) == Some("Bearer host-original"),
            "policy_epoch_header_present": request.headers.contains_key("x-station-policy-epoch")
        })).collect();
        let evidence = serde_json::json!({
            "requests": rows,
            "count": requests.len(),
            "post_count": requests.iter().filter(|request| request.method == "POST").count(),
            "loopback_bind": server.base.starts_with("http://127.0.0.1:")
        });
        let mut output = std::fs::OpenOptions::new().write(true).create_new(true).open(path).unwrap();
        output.write_all(&serde_json::to_vec(&evidence).unwrap()).unwrap();
    }
}
