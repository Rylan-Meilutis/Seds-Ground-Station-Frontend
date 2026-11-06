//! Real owned loopback GET -> production observer admission -> queue/reducer tests.
use super::*;
use std::{future::Future, io::{Read, Write}, net::TcpListener, pin::Pin, time::Duration};

fn child(name: &str) -> bool {
    let exact = format!("telemetry_dashboard::incoming_epoch_native_tests::{name}");
    if std::env::var("GS_INCOMING_EPOCH_CHILD").as_deref() == Ok(&exact) { return true; }
    let root = std::env::temp_dir().join(format!("gs-incoming-{}-{}", std::process::id(),
        std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()));
    std::fs::create_dir(&root).unwrap();
    let result = std::process::Command::new(std::env::current_exe().unwrap())
        .args(["--exact", &exact, "--nocapture"])
        .env("GS_INCOMING_EPOCH_CHILD", &exact)
        .env("XDG_DATA_HOME", root.join("data")).env("XDG_CONFIG_HOME", root.join("config"))
        .current_dir(&root).output().unwrap();
    std::fs::remove_dir_all(root).unwrap();
    assert!(result.status.success(), "{}\n{}", String::from_utf8_lossy(&result.stdout), String::from_utf8_lossy(&result.stderr));
    assert!(String::from_utf8_lossy(&result.stdout).contains("1 passed"));
    false
}

pub(super) struct Server {
    pub base: String,
    body: Arc<Mutex<serde_json::Value>>,
    status: Arc<std::sync::atomic::AtomicU16>,
    stop: Arc<AtomicBool>,
    thread: Option<std::thread::JoinHandle<()>>,
}
impl Server {
    pub(super) fn new() -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let body = Arc::new(Mutex::new(serde_json::to_value(station_mode_contract::tests::example_snapshot()).unwrap()));
        let status = Arc::new(std::sync::atomic::AtomicU16::new(200));
        let response_status = status.clone();
        let stop = Arc::new(AtomicBool::new(false));
        let (reply, end) = (body.clone(), stop.clone());
        let thread = std::thread::spawn(move || {
            while !end.load(Ordering::Acquire) {
                match listener.accept() {
                    Ok((mut stream, _)) => {
                        stream.set_read_timeout(Some(Duration::from_millis(500))).unwrap();
                        let mut bytes = Vec::new(); let mut buffer = [0; 2048];
                        while let Ok(n) = stream.read(&mut buffer) {
                            if n == 0 { break; } bytes.extend_from_slice(&buffer[..n]);
                            if bytes.windows(4).any(|w| w == b"\r\n\r\n") { break; }
                            assert!(bytes.len() < 65536);
                        }
                        let request = String::from_utf8(bytes).unwrap();
                        assert!(request.starts_with("GET /api/station-modes HTTP/1.1\r\n"));
                        assert!(request.to_ascii_lowercase().contains("authorization: bearer incoming-token"));
                        let body = serde_json::to_vec(&*reply.lock().unwrap()).unwrap();
                        write!(stream, "HTTP/1.1 {} Fixture\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", response_status.load(Ordering::Acquire), body.len()).unwrap();
                        stream.write_all(&body).unwrap();
                    }
                    Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => std::thread::sleep(Duration::from_millis(2)),
                    Err(e) => panic!("{e}"),
                }
            }
        });
        Self { base, body, status, stop, thread: Some(thread) }
    }
    pub(super) fn set(&self, snapshot: &station_mode_contract::Snapshot) {
        self.status.store(200, Ordering::Release);
        *self.body.lock().unwrap() = serde_json::to_value(snapshot).unwrap();
    }
}
impl Drop for Server {
    fn drop(&mut self) { self.stop.store(true, Ordering::Release); self.thread.take().unwrap().join().unwrap(); }
}
pub(super) fn dom(base: &str) -> VirtualDom { dom_with(|| rsx! { div {} }, base) }
fn dom_with(root: fn() -> Element, base: &str) -> VirtualDom {
    let mut dom = VirtualDom::new(root); dom.rebuild_in_place();
    dom.in_scope(ScopeId::ROOT, || {
        *BASE_URL.write() = base.into(); *WS_EPOCH.write() = 7;
        *DASHBOARD_LIFE.write() = DashboardLife::new_alive();
        *FRONTEND_NETWORK_METRICS_STATE.lock().unwrap() = FrontendNetworkMetrics { ws_connected: true, ..Default::default() };
        auth::set_current_session(auth::StoredAuthSession { token: "incoming-token".into(), remember_me: false,
            session: auth::SessionStatus { authenticated: true,
                permissions: auth::Permissions { view_data: true, send_commands: true, ..Default::default() }, ..Default::default() } });
    }); dom
}
async fn in_dom<F: Future + ?Sized>(dom: &VirtualDom, mut future: Pin<&mut F>) -> F::Output {
    std::future::poll_fn(|cx| dom.in_scope(ScopeId::ROOT, || future.as_mut().poll(cx))).await
}
pub(super) async fn mode_get_completion(dom: &VirtualDom) -> Result<serde_json::Value, String> {
    let (owner, ticket, origin, started) = dom.in_scope(ScopeId::ROOT, || {
        let owner = station_mode_observer::owner(Arc::new(AtomicBool::new(true))).unwrap();
        let origin = UrlConfig::base_http();
        let ticket = station_policy_context::observation_ticket(&origin).unwrap();
        (owner, ticket, origin, monotonic_now_ms())
    });
    let mut request = dom.in_scope(ScopeId::ROOT, || Box::pin(station_mode_transport::capabilities(Ok(owner.clone()))));
    let raw = in_dom(dom, request.as_mut()).await;
    let result = raw.clone();
    assert!(dom.in_scope(ScopeId::ROOT, || station_mode_observer::admit_owned_response(&origin, owner, &ticket, started, raw)));
    result
}
pub(super) async fn mode_get(dom: &VirtualDom) {
    let raw=mode_get_completion(dom).await.expect("actual owned loopback ModeGET success");
    dom.in_scope(ScopeId::ROOT,||{
        let context=station_policy_context::ingress_context();
        assert_eq!(context.applied.as_ref().map(station_policy_context::AppliedStamp::epoch),raw["policy_epoch"].as_str(),
            "actual transport/admission must establish the requested original applied proof");
    });
}

/// Fresh modeled producer event through the ACTUAL metadata parser/reducer.
/// This is a test transport adapter, not a policy/owner setter. The caller owns
/// the original producer epoch and creates fresh data; never copy old readiness
/// across a mode readback and relabel it with the new epoch.
pub(super) fn receive_action_policy(dom:&VirtualDom, epoch:Option<&str>, data:serde_json::Value) {
    let mut frame=serde_json::json!({"ty":"ActionPolicy","data":data});
    if let Some(epoch)=epoch { frame["epoch_scope"]="mode".into();frame["policy_epoch"]=epoch.into(); }
    dom.in_scope(ScopeId::ROOT,||handle_ws_message(
        &frame.to_string(),0,
        Signal::new(Vec::<AlertMsg>::new()),Signal::new(Vec::<AlertMsg>::new()),Signal::new(0),Signal::new(0),Signal::new(false),
        Signal::new(Vec::<PersistentNotification>::new()),Signal::new(Vec::<PersistentNotification>::new()),
        Signal::new(Vec::<PersistentNotification>::new()),Signal::new(Vec::<DismissedNotification>::new()),Signal::new(Vec::<u64>::new()),
        ACTION_POLICY_SIGNAL.signal(),Signal::new(RecordingStatusMsg{mode:"idle".into(),db_path:None}),Signal::new(None),Signal::new(None),Signal::new(None),
        Signal::new(NetworkTopologyMsg::default()),Signal::new(0),Signal::new(0),Signal::new("Unknown".into()),Signal::new(Vec::<BoardStatusEntry>::new()),
        Signal::new(None),Signal::new(None),Signal::new(None),Signal::new(None),
    ));
}

fn telemetry(epoch: &str, value: f32) -> String {
    serde_json::json!({"ty":"Telemetry", "epoch_scope":"mode", "policy_epoch":epoch,
        "data":{"timestamp_ms":current_wallclock_ms(), "received_timestamp_ms":current_wallclock_ms(),
        "source_timestamp_ms":null,"data_type":"KG1000","sender_id":"DAQ","values":[value]}}).to_string()
}

#[tokio::test(flavor="current_thread")]
async fn actual_mode_readback_retires_queued_rows_but_closing_allows_matching_readonly_data() {
    if !child("actual_mode_readback_retires_queued_rows_but_closing_allows_matching_readonly_data") { return; }
    let server = Server::new(); let dom = dom(&server.base); mode_get(&dom).await;
    let (notifications, history, unread) = dom.in_scope(ScopeId::ROOT, ||
        (Signal::new(Vec::<PersistentNotification>::new()), Signal::new(Vec::<PersistentNotification>::new()), Signal::new(Vec::<u64>::new())));
    let flight = dom.in_scope(ScopeId::ROOT, || {
        let flight = Signal::new("Armed".to_string());
        mode_live_view::register(mode_live_view::Mounted {
            alive: dashboard_alive(), mount: dashboard_gen(), flight_state: flight,
            launch_clock: Signal::new(None), board_status: Signal::new(Vec::new()),
            network_topology: Signal::new(NetworkTopologyMsg::default()),
            action_policy: Signal::new(ActionPolicyMsg::default_locked()), notifications,
            sequence_status: Signal::new(sequence_progress_host::StatusObservation::default()),
            sequence_progress: Signal::new(sequence_progress_model::Model::default()),
            fill_targets: Signal::new(None), network_time: Signal::new(None),
            rocket_gps: Signal::new(Some((42., -78.))), rocket_gps_altitude: Signal::new(Some(99.)),
        });
        flight
    });
    let condition_connection = dom.in_scope(ScopeId::ROOT, || {
        let old = PersistentNotification { id: 41, timestamp_ms: 1, message: "Original domain history".into(),
            persistent: true, action_label: Some("Old action".into()), action_cmd: Some("StartFill".into()) };
        let mut popup = notifications; popup.set(vec![old.clone()]);
        let mut archive = history; archive.set(vec![old]);
        ALERT_CONDITION_VIEW.write().begin_connection()
    });
    let first = telemetry("owner-A:epoch-1", 7.);
    let original = dom.in_scope(ScopeId::ROOT, || {
        let permit = incoming_epoch::PacketPermit::capture(&first).unwrap();
        assert!(try_handle_ws_telemetry_message_fast(&first, notifications, history, unread));
        assert_eq!(TELEMETRY_QUEUE.lock().unwrap().len(), 1);
        permit
    });
    let mut closed = station_mode_contract::tests::example_snapshot(); closed.admissions_open = false;
    server.set(&closed); mode_get(&dom).await;
    dom.in_scope(ScopeId::ROOT, || {
        assert!(station_policy_context::capture_current().is_err());
        original.check_current().unwrap();
        assert_eq!(&*flight.read(), "Armed", "ordinary closure must not clear matching applied read-only facts");
        let rows = drain_telemetry_queue_for_frame(); assert_eq!(rows.len(), 1); assert_eq!(rows[0].values[0], Some(7.));
        assert!(try_handle_ws_telemetry_message_fast(&first, notifications, history, unread));
        assert_eq!(TELEMETRY_QUEUE.lock().unwrap().len(), 1);
    });
    server.status.store(503, Ordering::Release);
    assert!(mode_get_completion(&dom).await.is_err());
    dom.in_scope(ScopeId::ROOT, || {
        original.check_current().unwrap();
        station_policy_context::capture_emergency_current().unwrap().check_current().unwrap();
        assert_eq!(&*flight.read(), "Armed", "ModeGET outage must not erase the retained applied read-only context");
    });
    closed.admissions_open = true; closed.policy_epoch = Some("owner-A:epoch-2".into());
    let pair = closed.current.as_mut().unwrap(); pair.id = "third-sim".into(); pair.mode_id = "custom α".into(); pair.simulation = true;
    server.set(&closed); mode_get(&dom).await;
    dom.in_scope(ScopeId::ROOT, || {
        assert!(original.check_current().is_err());
        assert_eq!(&*flight.read(), "Unknown", "actual owned admission cleared mounted current facts");
        assert!(notifications.read().is_empty(), "old live popup cannot supply a fresh new-domain action");
        assert_eq!(history.read()[0].id, 41, "immutable history cache survives current-domain clearing");
        let conditions = alert_conditions::Snapshot { version: 1, authority: "new-domain-condition-authority".into(), revision: 1,
            complete: true, evaluated: Vec::new(), awaiting_evidence: Vec::new(), persistence_error: None, records: Vec::new() };
        assert_eq!(ALERT_CONDITION_VIEW.write().ingest(condition_connection, conditions), alert_conditions::Ingest::Accepted,
            "matching new-domain conditions use the SAME live socket token, without reconnect");
        assert!(station_mode_observer::OBSERVED.read().as_ref().unwrap().value.current.as_ref().unwrap().simulation);
        assert!(TELEMETRY_QUEUE.lock().unwrap().is_empty(), "actual admission hook clears old rows BEFORE new mode publication");
        assert!(try_handle_ws_telemetry_message_fast(&first, notifications, history, unread));
        assert!(drain_telemetry_queue_for_frame().is_empty(), "late original producer epoch never becomes current");
        let fresh = telemetry("owner-A:epoch-2", 9.);
        assert!(try_handle_ws_telemetry_message_fast(&fresh, notifications, history, unread));
        let rows = drain_telemetry_queue_for_frame(); assert_eq!(rows.len(), 1); assert_eq!(rows[0].values[0], Some(9.));
        assert!(incoming_epoch::PacketPermit::capture(r#"{"ty":"ActionPolicy","epoch_scope":"independent","data":{}}"#).is_err());
        assert!(incoming_epoch::PacketPermit::capture(r#"{"ty":"Telemetry","epoch_scope":"mode","policy_epoch":"owner-A:epoch-2","policy_epoch":"owner-A:epoch-2","data":{}}"#).is_err());
    });
}

#[tokio::test(flavor="current_thread")]
async fn actual_new_vdom_cannot_revive_old_seal_from_same_epoch_mode_get() {
    if !child("actual_new_vdom_cannot_revive_old_seal_from_same_epoch_mode_get") { return; }
    let server = Server::new(); let first = dom(&server.base); mode_get(&first).await;
    let seal = first.in_scope(ScopeId::ROOT, || station_policy_context::capture_emergency_current().unwrap());
    first.in_scope(ScopeId::ROOT, || {
        seal.check_current().unwrap();
        bump_frontend_data_clear_epoch();
        seal.check_current().unwrap();
    });
    drop(first);
    let second = dom(&server.base);
    second.in_scope(ScopeId::ROOT, || assert!(seal.check_current().is_err()));
    mode_get(&second).await;
    second.in_scope(ScopeId::ROOT, || {
        assert!(seal.check_current().is_err(), "new authenticated observer proof must not rehabilitate old seal");
        station_policy_context::capture_emergency_current().unwrap().check_current().unwrap();
    });
}

#[derive(Clone, Default)]
struct Gate {
    entered: Arc<AtomicBool>,
    released: Arc<(Mutex<bool>, std::sync::Condvar)>,
}
impl Gate {
    fn hold(&self) {
        self.entered.store(true, Ordering::Release);
        let (lock, ready) = &*self.released;
        let mut released = lock.lock().unwrap();
        while !*released { released = ready.wait(released).unwrap(); }
    }
    fn release(&self) {
        let (lock, ready) = &*self.released;
        *lock.lock().unwrap() = true; ready.notify_all();
    }
}
#[derive(Clone, Copy)]
enum Headers { Correct, Missing, Wrong, Duplicate }
struct LiveState {
    mode: station_mode_contract::Snapshot,
    value: String,
    status: u16,
    headers: Headers,
    gate: Option<Gate>,
    live_requests: Vec<String>,
    gse_requests: Vec<std::time::Instant>,
    held_gates: Vec<Gate>,
}
struct LiveServer {
    base: String,
    state: Arc<Mutex<LiveState>>,
    stop: Arc<AtomicBool>,
    thread: Option<std::thread::JoinHandle<()>>,
}
impl LiveServer {
    fn new() -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap(); listener.set_nonblocking(true).unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let state = Arc::new(Mutex::new(LiveState { mode: station_mode_contract::tests::example_snapshot(),
            value: "Idle".into(), status: 200, headers: Headers::Correct, gate: None, live_requests: Vec::new(), gse_requests: Vec::new(), held_gates: Vec::new() }));
        let stop = Arc::new(AtomicBool::new(false)); let (config,end) = (state.clone(), stop.clone());
        let thread = std::thread::spawn(move || {
            let mut workers = Vec::new();
            while !end.load(Ordering::Acquire) {
                match listener.accept() {
                    Ok((mut stream,_)) => {
                        let config = config.clone();
                        workers.push(std::thread::spawn(move || {
                            stream.set_read_timeout(Some(Duration::from_millis(500))).unwrap();
                            let mut bytes = Vec::new(); let mut buffer = [0;2048];
                            while let Ok(n) = stream.read(&mut buffer) {
                                if n == 0 { break; } bytes.extend_from_slice(&buffer[..n]);
                                if bytes.windows(4).any(|w|w==b"\r\n\r\n") { break; }
                                assert!(bytes.len()<65536);
                            }
                            let request=String::from_utf8(bytes).unwrap();
                            let (status,body,headers,gate) = {
                                let mut state=config.lock().unwrap();
                                if request.starts_with("GET /api/station-modes ") {
                                    (200,serde_json::to_vec(&state.mode).unwrap(),String::new(),None)
                                } else {
                                    let gse = request.starts_with("GET /api/gse/status ");
                                    assert!(gse || request.starts_with("GET /flightstate "));
                                    let epoch=request.lines().find_map(|line|line.split_once(':')
                                        .filter(|(name,_)|name.eq_ignore_ascii_case("x-station-policy-epoch"))
                                        .map(|(_,value)|value.trim().to_owned())).expect("original request epoch header");
                                    if gse { state.gse_requests.push(std::time::Instant::now()); }
                                    else { state.live_requests.push(request); }
                                    // Capture status, body AND original producer echo BEFORE hold.
                                    let headers=match state.headers {
                                        Headers::Correct=>format!("X-Station-Epoch-Scope: mode\r\nX-Station-Policy-Epoch: {epoch}\r\n"),
                                        Headers::Missing=>String::new(),
                                        Headers::Wrong=>"X-Station-Epoch-Scope: mode\r\nX-Station-Policy-Epoch: unrelated-original-epoch\r\n".into(),
                                        Headers::Duplicate=>format!("X-Station-Epoch-Scope: mode\r\nX-Station-Policy-Epoch: {epoch}\r\nX-Station-Policy-Epoch: {epoch}\r\n"),
                                    };
                                    let gate = state.gate.take();
                                    if let Some(gate) = &gate { state.held_gates.push(gate.clone()); }
                                    let body = if gse {
                                        serde_json::to_vec(&serde_json::json!({"sequence_progress":{"version":1,
                                            "backend_instance":"11111111111111111111111111111111","reset_epoch":"2222222222222222",
                                            "cancel_api_version":1,"active":[],"terminal":[],"reporting_error":null,"board_display":{}}})).unwrap()
                                    } else { serde_json::to_vec(&state.value).unwrap() };
                                    let status=if state.mode.policy_epoch.as_deref()!=Some(epoch.as_str()) || state.mode.current.is_none() {409}else{state.status};
                                    (status,body,headers,gate)
                                }
                            };
                            if let Some(gate)=gate { gate.hold(); }
                            let head=format!("HTTP/1.1 {status} Fixture\r\nContent-Type: application/json\r\n{headers}Content-Length: {}\r\nConnection: close\r\n\r\n",body.len());
                            // A retired client may abort its exact socket. That is a
                            // legitimate cancellation; the test still requires no data apply.
                            let _=stream.write_all(head.as_bytes()).and_then(|_|stream.write_all(&body));
                        }));
                        assert!(workers.len()<=32,"bounded synthetic HTTP workers");
                    }
                    Err(e) if e.kind()==std::io::ErrorKind::WouldBlock=>std::thread::sleep(Duration::from_millis(2)),
                    Err(e)=>panic!("{e}"),
                }
            }
            for worker in workers { worker.join().unwrap(); }
        }); Self {base,state,stop,thread:Some(thread)}
    }
}
impl Drop for LiveServer {
    fn drop(&mut self) {
        { let mut state = self.state.lock().unwrap();
          if let Some(gate)=state.gate.take() { gate.release(); }
          for gate in state.held_gates.drain(..) { gate.release(); }
        }
        self.stop.store(true,Ordering::Release); self.thread.take().unwrap().join().unwrap();
    }
}
async fn wait_held<F:Future+?Sized>(dom:&VirtualDom, mut request:Pin<&mut F>, gate:&Gate) {
    let started=std::time::Instant::now();
    while !gate.entered.load(Ordering::Acquire) {
        assert!(started.elapsed()<Duration::from_secs(3),"actual original request must reach held server");
        std::future::poll_fn(|cx| {
            assert!(dom.in_scope(ScopeId::ROOT,||request.as_mut().poll(cx)).is_pending());
            std::task::Poll::Ready(())
        }).await;
        tokio::time::sleep(Duration::from_millis(2)).await;
    }
}

#[tokio::test(flavor="current_thread")]
async fn live_http_captures_before_first_poll_checks_original_echo_and_retires_held_replies() {
    if !child("live_http_captures_before_first_poll_checks_original_echo_and_retires_held_replies") { return; }
    let server=LiveServer::new(); let dom=dom(&server.base); mode_get(&dom).await;
    let mut old=dom.in_scope(ScopeId::ROOT,||Box::pin(http_get_json::<FlightState>("/flightstate")));
    server.state.lock().unwrap().mode.policy_epoch=Some("owner-A:epoch-2".into()); mode_get(&dom).await;
    assert!(in_dom(&dom,old.as_mut()).await.is_err());
    assert!(server.state.lock().unwrap().live_requests.is_empty(),"retired before first poll sends ZERO HTTP requests");
    let mut fresh=dom.in_scope(ScopeId::ROOT,||Box::pin(http_get_json::<FlightState>("/flightstate")));
    assert_eq!(in_dom(&dom,fresh.as_mut()).await.unwrap(),"Idle");
    assert!(server.state.lock().unwrap().live_requests[0].to_ascii_lowercase().contains("x-station-policy-epoch: owner-a:epoch-2"));
    for bad in [Headers::Missing,Headers::Wrong,Headers::Duplicate] {
        server.state.lock().unwrap().headers=bad;
        let mut request=dom.in_scope(ScopeId::ROOT,||Box::pin(http_get_json::<FlightState>("/flightstate")));
        assert!(in_dom(&dom,request.as_mut()).await.is_err(),"unproven/mismatched/duplicate original response metadata must fail");
    }
    let held=Gate::default();
    {let mut state=server.state.lock().unwrap();state.headers=Headers::Correct;state.gate=Some(held.clone());}
    let mut request=dom.in_scope(ScopeId::ROOT,||Box::pin(http_get_json::<FlightState>("/flightstate")));
    wait_held(&dom,request.as_mut(),&held).await;
    {let mut state=server.state.lock().unwrap();state.mode.policy_epoch=Some("owner-A:epoch-3".into());state.value="Launch".into();}
    mode_get(&dom).await; held.release();
    assert!(in_dom(&dom,request.as_mut()).await.is_err(),"unchanged held old200 cannot apply to new epoch");
    let mut fresh=dom.in_scope(ScopeId::ROOT,||Box::pin(http_get_json::<FlightState>("/flightstate")));
    assert_eq!(in_dom(&dom,fresh.as_mut()).await.unwrap(),"Launch");
    let held=Gate::default();
    {let mut state=server.state.lock().unwrap();state.status=401;state.gate=Some(held.clone());}
    let mut request=dom.in_scope(ScopeId::ROOT,||Box::pin(http_get_json::<FlightState>("/flightstate")));
    wait_held(&dom,request.as_mut(),&held).await;
    dom.in_scope(ScopeId::ROOT,||auth::set_current_session(auth::StoredAuthSession{token:"replacement-token".into(),remember_me:false,
        session:auth::SessionStatus{authenticated:true,permissions:auth::Permissions{view_data:true,send_commands:true,..Default::default()},..Default::default()}}));
    {let mut state=server.state.lock().unwrap();state.status=200;state.mode.principal_context="replacement-session".into();}mode_get(&dom).await;held.release();
    assert!(in_dom(&dom,request.as_mut()).await.is_err());
    dom.in_scope(ScopeId::ROOT,||assert_eq!(auth::current_token().as_deref(),Some("replacement-token")));
    let mut fresh=dom.in_scope(ScopeId::ROOT,||Box::pin(http_get_json::<FlightState>("/flightstate")));
    assert_eq!(in_dom(&dom,fresh.as_mut()).await.unwrap(),"Launch","late401 does not disable renewed session's fresh owned read");
    let state = server.state.lock().unwrap();
    assert_eq!(state.live_requests.len(), 8, "no automatic retry or response-driven replay");
    assert!(state.live_requests[6].to_ascii_lowercase().contains("authorization: bearer incoming-token"));
    assert!(state.live_requests[7].to_ascii_lowercase().contains("authorization: bearer replacement-token"));
}

fn progress_poll_component() -> Element {
    let state=use_signal(sequence_progress_model::Model::default);
    let status=use_signal(sequence_progress_host::StatusObservation::default);
    let open_result=use_signal(||None::<sequence_progress_model::RunKey>);
    use_context_provider(|| status);
    rsx!{sequence_progress_host::SequenceProgressHost{state,status,open_result,on_actions:true}}
}
async fn drive_poll_window(dom:&mut VirtualDom,server:&LiveServer) {
    let start=std::time::Instant::now();
    let before=server.state.lock().unwrap().gse_requests.len();
    while start.elapsed()<Duration::from_millis(2200) {
        tokio::select! {
            _=dom.wait_for_work()=>{let _=dom.render_immediate_to_vec();},
            _=tokio::time::sleep(Duration::from_millis(20))=>{},
        }
        assert!(server.state.lock().unwrap().gse_requests.len()<=before+4,"actual immediate-response poll loop must not spin");
    }
    let state=server.state.lock().unwrap();
    for pair in state.gse_requests.windows(2) {
        assert!(pair[1].duration_since(pair[0])>=Duration::from_millis(900),"actual status GET cadence stays at one second, not a busy retry");
    }
}
#[tokio::test(flavor="current_thread")]
async fn actual_sequence_poller_remains_bounded_when_closing_503_unknown_and_auth_retires() {
    if !child("actual_sequence_poller_remains_bounded_when_closing_503_unknown_and_auth_retires") { return; }
    let server=LiveServer::new();server.state.lock().unwrap().mode.admissions_open=false;
    let mut dom=dom_with(progress_poll_component,&server.base);mode_get(&dom).await;
    assert!(dom.in_scope(ScopeId::ROOT,station_policy_context::capture_current).is_err());
    drive_poll_window(&mut dom,&server).await;
    assert!(server.state.lock().unwrap().gse_requests.len()>=2,"readonly matching-epoch Closing GETs actually occur");
    let before=server.state.lock().unwrap().gse_requests.len();server.state.lock().unwrap().status=503;
    drive_poll_window(&mut dom,&server).await;
    assert!(server.state.lock().unwrap().gse_requests.len()>before,"503 retries remain scheduled");
    {let mut state=server.state.lock().unwrap();state.mode.backend_instance="replacement-unknown-process".into();
        state.mode.current=None;state.mode.policy_epoch=None;state.status=200;}
    mode_get_completion(&dom).await.unwrap();
    let before=server.state.lock().unwrap().gse_requests.len();drive_poll_window(&mut dom,&server).await;
    assert!(server.state.lock().unwrap().gse_requests.len()<=before+1,"unknown context cannot start repeated reads; at most one previously issued original-epoch request may still arrive");
    dom.in_scope(ScopeId::APP,||{
        let status=consume_context::<Signal<sequence_progress_host::StatusObservation>>();
        assert!(!status.read().current_binding(),"old response cannot become current while applied context is unknown");
    });
    let before_renewal=server.state.lock().unwrap().gse_requests.len();
    dom.in_scope(ScopeId::ROOT,||auth::set_current_session(auth::StoredAuthSession{token:"renewed-poll-token".into(),remember_me:false,
        session:auth::SessionStatus{authenticated:true,permissions:auth::Permissions{view_data:true,send_commands:true,..Default::default()},..Default::default()}}));
    {let mut state=server.state.lock().unwrap();state.mode=station_mode_contract::tests::example_snapshot();
        state.mode.policy_epoch=Some("owner-A:epoch-2".into());state.mode.principal_context="renewed-poll-session".into();state.mode.admissions_open=false;}
    mode_get(&dom).await;drive_poll_window(&mut dom,&server).await;
    assert!(server.state.lock().unwrap().gse_requests.len()>before_renewal,"new session obtains fresh owned readonly status without reviving old mutators");
    dom.in_scope(ScopeId::APP,||{
        let status=consume_context::<Signal<sequence_progress_host::StatusObservation>>();
        assert!(status.read().current_binding(),"fresh renewed-session readonly status genuinely publishes");
    });
    assert!(dom.in_scope(ScopeId::ROOT,station_policy_context::capture_current).is_err());
}

fn receive_reset_test_frame(dom:&VirtualDom,frame:serde_json::Value,archive:Signal<Vec<PersistentNotification>>) {
    dom.in_scope(ScopeId::ROOT,||handle_ws_message(
        &frame.to_string(),0,
        Signal::new(Vec::<AlertMsg>::new()),Signal::new(Vec::<AlertMsg>::new()),Signal::new(0),Signal::new(0),Signal::new(false),
        Signal::new(Vec::<PersistentNotification>::new()),archive,
        Signal::new(Vec::<PersistentNotification>::new()),Signal::new(Vec::<DismissedNotification>::new()),Signal::new(Vec::<u64>::new()),
        ACTION_POLICY_SIGNAL.signal(),Signal::new(RecordingStatusMsg{mode:"idle".into(),db_path:None}),Signal::new(None),Signal::new(None),Signal::new(None),
        Signal::new(NetworkTopologyMsg::default()),Signal::new(0),Signal::new(0),Signal::new("Unknown".into()),Signal::new(Vec::<BoardStatusEntry>::new()),
        Signal::new(None),Signal::new(None),Signal::new(None),Signal::new(None),
    ));
}
#[tokio::test(flavor="current_thread")]
async fn actual_dashboard_reset_uses_original_mode_permit_and_preserves_archives() {
    if !child("actual_dashboard_reset_uses_original_mode_permit_and_preserves_archives") { return; }
    let server=Server::new();let dom=dom(&server.base);mode_get(&dom).await;
    let (archive,notifications,unread)=dom.in_scope(ScopeId::ROOT,||(
        Signal::new(vec![PersistentNotification{id:77,timestamp_ms:1,message:"Immutable archived event".into(),persistent:true,action_label:None,action_cmd:None}]),
        Signal::new(Vec::<PersistentNotification>::new()),Signal::new(Vec::<u64>::new())));
    let mut current=station_mode_contract::tests::example_snapshot();current.admissions_open=false;
    server.set(&current);mode_get(&dom).await;
    let before=dom.in_scope(ScopeId::ROOT,||{
        assert!(station_policy_context::capture_current().is_err());
        assert!(try_handle_ws_telemetry_message_fast(&telemetry("owner-A:epoch-1",7.),notifications,archive,unread));
        assert_eq!(TELEMETRY_QUEUE.lock().unwrap().len(),1);
        (GRAPH_HISTORY_GENERATION.load(Ordering::Relaxed),*SEED_EPOCH.read())
    });
    receive_reset_test_frame(&dom,serde_json::json!({"ty":"DashboardReset","epoch_scope":"mode","policy_epoch":"owner-A:epoch-1"}),archive);
    dom.in_scope(ScopeId::ROOT,||{
        assert_eq!(GRAPH_HISTORY_GENERATION.load(Ordering::Relaxed),before.0+1);
        assert_eq!(*SEED_EPOCH.read(),before.1+1);
        assert!(TELEMETRY_QUEUE.lock().unwrap().is_empty());
        assert_eq!(archive.read()[0].id,77,"mode-scoped display reset preserves immutable archives");
    });
    current.policy_epoch=Some("owner-A:epoch-2".into());current.admissions_open=true;
    server.set(&current);mode_get(&dom).await;
    let before=dom.in_scope(ScopeId::ROOT,||{
        assert!(try_handle_ws_telemetry_message_fast(&telemetry("owner-A:epoch-2",9.),notifications,archive,unread));
        assert_eq!(TELEMETRY_QUEUE.lock().unwrap().len(),1);
        (GRAPH_HISTORY_GENERATION.load(Ordering::Relaxed),*SEED_EPOCH.read())
    });
    for bad in [
        serde_json::json!({"ty":"DashboardReset","epoch_scope":"mode","policy_epoch":"owner-A:epoch-1"}),
        serde_json::json!({"ty":"DashboardReset"}),
        serde_json::json!({"ty":"DashboardReset","epoch_scope":"independent"}),
        serde_json::json!({"ty":"UnknownFutureReset","epoch_scope":"mode","policy_epoch":"owner-A:epoch-2"}),
    ]{
        receive_reset_test_frame(&dom,bad,archive);
        dom.in_scope(ScopeId::ROOT,||{
            assert_eq!(GRAPH_HISTORY_GENERATION.load(Ordering::Relaxed),before.0);
            assert_eq!(*SEED_EPOCH.read(),before.1);
            assert_eq!(TELEMETRY_QUEUE.lock().unwrap().len(),1,"retired/unscoped/independent/unknown event cannot clear current data");
            assert_eq!(archive.read()[0].id,77);
        });
    }
    receive_reset_test_frame(&dom,serde_json::json!({"ty":"DashboardReset","epoch_scope":"mode","policy_epoch":"owner-A:epoch-2"}),archive);
    dom.in_scope(ScopeId::ROOT,||{
        assert_eq!(GRAPH_HISTORY_GENERATION.load(Ordering::Relaxed),before.0+1);
        assert_eq!(*SEED_EPOCH.read(),before.1+1);
        assert!(TELEMETRY_QUEUE.lock().unwrap().is_empty());assert_eq!(archive.read()[0].id,77);
    });
}
