use super::*;
mod legacy {
    include!("fixtures/gse_config_8c08a70.rs");
}
mod current {
    include!("fixtures/gse_config_source19.rs");
}
fn legacy_settings() -> Settings {
    serde_json::from_value(serde_json::to_value(legacy::Config::default()).unwrap()).unwrap()
}
#[test]
fn exact_published_strict_parser_rejects_source19_default_but_accepts_legacy_payload() {
    let config = legacy_settings();
    let old_wire = serde_json::to_value(&config).unwrap();
    assert!(old_wire.get("cold_flow_target_psi").is_some());
    let error = serde_json::from_value::<legacy::Config>(old_wire)
        .unwrap_err()
        .to_string();
    assert!(
        error.contains("unknown field") && error.contains("cold_flow_target_psi"),
        "{error}"
    );
    let corrected = payload(&config, false, false).unwrap();
    assert!(corrected.get("cold_flow_target_psi").is_none());
    let parsed: legacy::Config = serde_json::from_value(corrected).unwrap();
    assert_eq!(parsed, legacy::Config::default());
}
#[test]
fn explicit_schema_presence_supports_values_and_null_clear_without_hidden_defaults() {
    let original = current::Config::default();
    let value = serde_json::to_value(&original).unwrap();
    assert!(supports_cold_flow(&value));
    assert!(!supports_cold_flow(
        &serde_json::to_value(legacy::Config::default()).unwrap()
    ));
    let mut config: Settings = serde_json::from_value(value.clone()).unwrap();
    assert_eq!(
        payload(&config, true, false).unwrap()["cold_flow_target_psi"],
        Value::Null
    );
    config.cold_flow_target_psi = Some(95.);
    let parsed: current::Config = serde_json::from_value(payload(&config, true, false).unwrap()).unwrap();
    assert_eq!(parsed.cold_flow_target_psi, Some(95.));
    config.cold_flow_target_psi = None;
    let cleared: current::Config = serde_json::from_value(payload(&config, true, false).unwrap()).unwrap();
    assert_eq!(cleared, original);
    let mut original_without_unsupported_deadband = serde_json::from_str::<Value>(&serde_json::to_string(&config).unwrap()).unwrap();
    original_without_unsupported_deadband.as_object_mut().unwrap().remove("turbocharge_pressure_deadband_psi");
    assert_eq!(payload(&config, true, false).unwrap(), original_without_unsupported_deadband);
}
#[test]
fn unsupported_some_and_nonfinite_never_silently_save_as_omitted_or_null() {
    let mut config = legacy_settings();
    config.cold_flow_target_psi = Some(95.);
    assert!(
        payload(&config, false, false)
            .unwrap_err()
            .contains("does not support")
    );
    for value in [f32::NAN, f32::INFINITY, 0., -1.] {
        config.cold_flow_target_psi = Some(value);
        assert!(payload(&config, true, false).is_err());
    }
}

#[cfg(not(target_arch = "wasm32"))]
mod native {
    use super::*;
    use crate::{auth, telemetry_dashboard as dashboard};
    use dioxus::prelude::*;
    use std::{
        future::Future,
        io::{Read, Write},
        net::TcpListener,
        pin::Pin,
        sync::{
            Arc, Mutex,
            atomic::{AtomicBool, AtomicU16, Ordering},
        },
        time::Duration,
    };
    fn session(token: &str) {
        auth::set_current_session(auth::StoredAuthSession {
            token: token.into(),
            remember_me: false,
            session: auth::SessionStatus {
                authenticated: true,
                username: Some("fixture-operator".into()),
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
            session("fixture-original");
        });
        dom
    }
    async fn in_dom<F: Future>(dom: &VirtualDom, mut future: Pin<&mut F>) -> F::Output {
        std::future::poll_fn(|cx| dom.in_scope(ScopeId::ROOT, || future.as_mut().poll(cx))).await
    }
    struct Server {
        base: String,
        posted: Arc<Mutex<Vec<Value>>>,
        headers: Arc<Mutex<Vec<String>>>,
        mode_headers: Arc<Mutex<Vec<String>>>,
        mode_reply: Arc<Mutex<Value>>,
        hold_posts: Arc<AtomicBool>,
        response_code: Arc<AtomicU16>,
        stop: Arc<AtomicBool>,
        worker: Option<std::thread::JoinHandle<()>>,
    }
    impl Server {
        fn new(current: bool) -> Self {
            let listener = TcpListener::bind("127.0.0.1:0").unwrap();
            listener.set_nonblocking(true).unwrap();
            let base = format!("http://{}", listener.local_addr().unwrap());
            let posted = Arc::new(Mutex::new(Vec::new()));
            let requests = posted.clone();
            let headers = Arc::new(Mutex::new(Vec::new()));
            let header_rows = headers.clone();
            let mode_headers = Arc::new(Mutex::new(Vec::new()));
            let mode_header_rows = mode_headers.clone();
            let mode_reply = Arc::new(Mutex::new(serde_json::to_value(dashboard::station_mode_contract::tests::example_snapshot()).unwrap()));
            let mode_response = mode_reply.clone();
            let hold_posts = Arc::new(AtomicBool::new(false));
            let hold = hold_posts.clone();
            let response_code = Arc::new(AtomicU16::new(200));
            let code = response_code.clone();
            let stop = Arc::new(AtomicBool::new(false));
            let stopped = stop.clone();
            let worker = std::thread::spawn(move || {
                let mut workers = Vec::new();
                while !stopped.load(Ordering::Acquire) {
                    match listener.accept() {
                        Ok((mut socket, _)) => {
                            let (requests, header_rows, hold, code, stopped, mode_response, mode_header_rows) =
                                (requests.clone(), header_rows.clone(), hold.clone(), code.clone(), stopped.clone(), mode_response.clone(), mode_header_rows.clone());
                            workers.push(std::thread::spawn(move || {
                            socket
                                .set_read_timeout(Some(Duration::from_secs(2)))
                                .unwrap();
                            let mut bytes = Vec::new();
                            let mut buffer = [0u8; 4096];
                            let header_end;
                            loop {
                                let n = socket.read(&mut buffer).unwrap();
                                assert_ne!(n, 0);
                                bytes.extend_from_slice(&buffer[..n]);
                                if let Some(end) = bytes.windows(4).position(|b| b == b"\r\n\r\n") {
                                    header_end = end + 4;
                                    break;
                                }
                            }
                            let headers = String::from_utf8_lossy(&bytes[..header_end]);
                            if headers.starts_with("GET /api/station-modes ") {
                                mode_header_rows.lock().unwrap().push(headers.to_string());
                                let text = mode_response.lock().unwrap().to_string();
                                let response = format!("HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{text}", text.len());
                                let _ = socket.write_all(response.as_bytes());
                                return;
                            }
                            let request_epoch = headers.lines().find_map(|line| line.split_once(':')
                                .filter(|(name,_)| name.eq_ignore_ascii_case("x-station-policy-epoch"))
                                .map(|(_, value)| value.trim().to_owned()));
                            header_rows.lock().unwrap().push(headers.to_string());
                            let is_post = headers.starts_with("POST ");
                            // Freeze old status BEFORE publishing request-arrived ledger.
                            let captured_code = code.load(Ordering::Acquire);
                            let length = headers
                                .lines()
                                .find_map(|line| {
                                    line.split_once(':')
                                        .filter(|(key, _)| {
                                            key.eq_ignore_ascii_case("content-length")
                                        })
                                        .map(|(_, value)| value.trim().parse::<usize>().unwrap())
                                })
                                .unwrap_or(0);
                            while bytes.len() < header_end + length {
                                let n = socket.read(&mut buffer).unwrap();
                                assert_ne!(n, 0);
                                bytes.extend_from_slice(&buffer[..n]);
                            }
                            let parsed = if is_post {
                                let value: Value =
                                    serde_json::from_slice(&bytes[header_end..header_end + length])
                                        .unwrap();
                                requests.lock().unwrap().push(value.clone());
                                if current {
                                    serde_json::from_value::<current::Config>(value)
                                        .and_then(serde_json::to_value)
                                } else {
                                    serde_json::from_value::<legacy::Config>(value)
                                        .and_then(serde_json::to_value)
                                }
                            } else if current {
                                Ok(serde_json::to_value(current::Config::default()).unwrap())
                            } else {
                                Ok(serde_json::to_value(legacy::Config::default()).unwrap())
                            };
                            while is_post && hold.load(Ordering::Acquire) && !stopped.load(Ordering::Acquire) {
                                std::thread::sleep(Duration::from_millis(2));
                            }
                            let (status, text) = match parsed {
                                Ok(value) => ("200 OK", value.to_string()),
                                Err(error) => ("422 Unprocessable Entity", error.to_string()),
                            };
                            let status = if captured_code == 401 { "401 Unauthorized" } else { status };
                            let scope = if !is_post { request_epoch.map(|epoch| format!("X-Station-Epoch-Scope: mode\r\nX-Station-Policy-Epoch: {epoch}\r\n")).unwrap_or_default() } else { String::new() };
                            let response = format!(
                                "HTTP/1.1 {status}\r\nContent-Type: application/json\r\n{scope}Content-Length: {}\r\nConnection: close\r\n\r\n{text}",
                                text.len()
                            );
                            let _ = socket.write_all(response.as_bytes());
                            }));
                            assert!(workers.len() <= 64, "bounded synthetic compatibility requests");
                        }
                        Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                            std::thread::sleep(Duration::from_millis(2))
                        }
                        Err(error) => panic!("fixture accept: {error}"),
                    }
                }
                for worker in workers { worker.join().unwrap(); }
            });
            Self {
                base,
                posted,
                headers,
                mode_headers,
                mode_reply,
                hold_posts,
                response_code,
                stop,
                worker: Some(worker),
            }
        }
    }
    impl Drop for Server {
        fn drop(&mut self) {
            self.stop.store(true, Ordering::Release);
            if let Some(worker) = self.worker.take() {
                worker.join().unwrap();
            }
        }
    }
    #[tokio::test(flavor = "current_thread")]
    async fn actual_owned_http_preserves_published_strict_config_and_supported_roundtrip() {
        for supported in [false, true] {
            let server = Server::new(supported);
            let dom = dom(&server.base);
            let mut load = Box::pin(dom.in_scope(ScopeId::ROOT, Grant::load));
            let (mut config, grant) = in_dom(&dom, load.as_mut()).await.unwrap();
            assert_eq!(
                dom.in_scope(ScopeId::ROOT, || grant.cold_flow_supported()),
                supported
            );
            if supported {
                config.cold_flow_target_psi = Some(95.);
            }
            let mut save = Box::pin(grant.save(&config));
            let (next, _) = in_dom(&dom, save.as_mut()).await.unwrap();
            drop(save);
            assert_eq!(
                next.cold_flow_target_psi,
                if supported { Some(95.) } else { None }
            );
            {
                let posts = server.posted.lock().unwrap();
                assert_eq!(posts.len(), 1);
                assert_eq!(posts[0].get("cold_flow_target_psi").is_some(), supported);
            }
            if !supported {
                config.cold_flow_target_psi = Some(95.);
                let mut unsupported = Box::pin(grant.save(&config));
                assert!(
                    matches!(in_dom(&dom,unsupported.as_mut()).await, Err(error) if error.contains("does not support"))
                );
                assert_eq!(
                    server.posted.lock().unwrap().len(),
                    1,
                    "unsupportedSome must not produce a second request"
                );
            }
        }
    }
    #[tokio::test(flavor = "current_thread")]
    async fn actual_pending_old_config_grant_cannot_send_after_station_or_auth_aba() {
        for change in ["station", "auth"] {
            let original = Server::new(true);
            let other = Server::new(false);
            let dom = dom(&original.base);
            let mut load = Box::pin(dom.in_scope(ScopeId::ROOT, Grant::load));
            let (mut config, grant) = in_dom(&dom, load.as_mut()).await.unwrap();
            config.cold_flow_target_psi = Some(95.);
            let mut pending = Box::pin(grant.save(&config));
            dom.in_scope(ScopeId::ROOT, || {
                if change == "station" {
                    dashboard::UrlConfig::set_base_url_and_persist(other.base.clone());
                } else {
                    session("fixture-replacement");
                    session("fixture-original");
                }
            });
            assert!(in_dom(&dom, pending.as_mut()).await.is_err());
            assert!(original.posted.lock().unwrap().is_empty());
            assert!(other.posted.lock().unwrap().is_empty());
            assert!(!dom.in_scope(ScopeId::ROOT, || grant.cold_flow_supported()));
        }
    }
    async fn observed_policy(dom: &VirtualDom, server: &Server, epoch: &str) {
        let mut snapshot = dashboard::station_mode_contract::tests::example_snapshot();
        snapshot.policy_epoch = Some(epoch.into());
        *server.mode_reply.lock().unwrap() = serde_json::to_value(snapshot).unwrap();
        dashboard::incoming_epoch_native_tests::mode_get(dom).await;
    }
    #[tokio::test(flavor = "current_thread")]
    async fn actual_grant_original_epoch_before_first_poll_and_current_body_header() {
        if !dashboard::command_owner::isolated_test_child(module_path!(), "actual_grant_original_epoch_before_first_poll_and_current_body_header") { return; }
        let server = Server::new(true);
        let dom = dom(&server.base);
        observed_policy(&dom, &server, "grant-owner:1").await;
        let mut retired_load = Box::pin(dom.in_scope(ScopeId::ROOT, Grant::load));
        observed_policy(&dom, &server, "grant-owner:2").await;
        assert!(in_dom(&dom, retired_load.as_mut()).await.is_err());
        assert!(server.headers.lock().unwrap().is_empty(), "old load must do zero CONFIG IO");
        assert_eq!(server.mode_headers.lock().unwrap().len(), 2, "two independent ModeGETs are explicit setup, not config IO");
        let mut load = Box::pin(dom.in_scope(ScopeId::ROOT, Grant::load));
        let (mut config, grant) = in_dom(&dom, load.as_mut()).await.unwrap();
        config.cold_flow_target_psi = Some(95.);
        let mut retired_save = Box::pin(grant.save(&config));
        observed_policy(&dom, &server, "grant-owner:3").await;
        assert!(in_dom(&dom, retired_save.as_mut()).await.is_err());
        assert!(server.posted.lock().unwrap().is_empty(), "old grant must not restamp a save");
        let mut load = Box::pin(dom.in_scope(ScopeId::ROOT, Grant::load));
        let (mut config, grant) = in_dom(&dom, load.as_mut()).await.unwrap();
        config.cold_flow_target_psi = Some(130.);
        let expected = payload(&config, true, false).unwrap();
        let mut save = Box::pin(grant.save(&config));
        let (saved, _) = in_dom(&dom, save.as_mut()).await.unwrap();
        assert_eq!(saved.cold_flow_target_psi, Some(130.));
        assert_eq!(*server.posted.lock().unwrap(), vec![expected]);
        let headers = server.headers.lock().unwrap();
        let posts: Vec<_> = headers.iter().filter(|row| row.starts_with("POST ")).collect();
        assert_eq!(posts.len(), 1);
        assert!(posts[0].to_ascii_lowercase().contains("x-station-policy-epoch: grant-owner:3\r\n"));
        assert!(posts[0].to_ascii_lowercase().contains("authorization: bearer fixture-original\r\n"));
    }
    #[tokio::test(flavor = "current_thread")]
    async fn actual_held_grant_401_retires_without_auth_clear_or_automatic_retry() {
        if !dashboard::command_owner::isolated_test_child(module_path!(), "actual_held_grant_401_retires_without_auth_clear_or_automatic_retry") { return; }
        let server = Server::new(true);
        let dom = dom(&server.base);
        observed_policy(&dom, &server, "held-grant:1").await;
        let mut load = Box::pin(dom.in_scope(ScopeId::ROOT, Grant::load));
        let (mut config, grant) = in_dom(&dom, load.as_mut()).await.unwrap();
        config.cold_flow_target_psi = Some(95.);
        server.hold_posts.store(true, Ordering::Release);
        server.response_code.store(401, Ordering::Release);
        let mut save = Box::pin(grant.save(&config));
        tokio::time::timeout(Duration::from_secs(4), async {
            while server.posted.lock().unwrap().is_empty() {
                tokio::select! {
                    result=in_dom(&dom,save.as_mut())=>panic!("held request returned early: {}",result.is_ok()),
                    _=tokio::time::sleep(Duration::from_millis(2))=>{}
                }
            }
        }).await.unwrap();
        dom.in_scope(ScopeId::ROOT, || session("fixture-replacement"));
        observed_policy(&dom, &server, "held-grant:2").await;
        server.hold_posts.store(false, Ordering::Release);
        assert!(in_dom(&dom, save.as_mut()).await.is_err());
        assert_eq!(dom.in_scope(ScopeId::ROOT, auth::current_token).as_deref(), Some("fixture-replacement"));
        assert_eq!(server.posted.lock().unwrap().len(), 1, "no retry of an old admitted body");
        server.response_code.store(200, Ordering::Release);
        let mut load = Box::pin(dom.in_scope(ScopeId::ROOT, Grant::load));
        let (mut config, grant) = in_dom(&dom, load.as_mut()).await.unwrap();
        config.cold_flow_target_psi = Some(130.);
        let mut fresh = Box::pin(grant.save(&config));
        in_dom(&dom, fresh.as_mut()).await.unwrap();
        assert_eq!(server.posted.lock().unwrap().len(), 2);
        let headers=server.headers.lock().unwrap();
        let posts:Vec<_>=headers.iter().filter(|row|row.starts_with("POST ")).collect();
        assert!(posts[0].to_ascii_lowercase().contains("x-station-policy-epoch: held-grant:1\r\n"));
        assert!(posts[1].to_ascii_lowercase().contains("x-station-policy-epoch: held-grant:2\r\n"));
        assert!(posts[1].to_ascii_lowercase().contains("authorization: bearer fixture-replacement\r\n"));
    }
}

#[test]
fn turbo_deadband_defaults_preserves_custom_values_and_gates_legacy_payloads() {
    let mut config = legacy_settings();
    assert_eq!(config.turbocharge_pressure_deadband_psi, 12.0);
    assert_eq!(Settings::default().turbocharge_pressure_deadband_psi, 12.0);
    assert!(!supports_turbo_deadband(&serde_json::to_value(legacy::Config::default()).unwrap()));
    for value in [8.0, 9.5, 12.0, 16.0] {
        config.turbocharge_pressure_deadband_psi = value;
        let wire = payload(&config, false, true).unwrap();
        assert!(supports_turbo_deadband(&wire));
        assert_eq!(wire["turbocharge_pressure_deadband_psi"], serde_json::json!(value));
        let restored: Settings = serde_json::from_value(wire).unwrap();
        assert_eq!(restored.turbocharge_pressure_deadband_psi, value);
        if value != 12.0 { assert!(payload(&config, false, false).is_err()); }
    }
    for value in [f32::NAN, f32::INFINITY, 7.99, 16.01] {
        config.turbocharge_pressure_deadband_psi = value;
        assert!(payload(&config, false, true).is_err());
    }
}
