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
    let corrected = payload(&config, false).unwrap();
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
        payload(&config, true).unwrap()["cold_flow_target_psi"],
        Value::Null
    );
    config.cold_flow_target_psi = Some(95.);
    let parsed: current::Config = serde_json::from_value(payload(&config, true).unwrap()).unwrap();
    assert_eq!(parsed.cold_flow_target_psi, Some(95.));
    config.cold_flow_target_psi = None;
    let cleared: current::Config = serde_json::from_value(payload(&config, true).unwrap()).unwrap();
    assert_eq!(cleared, original);
    assert_eq!(
        payload(&config, true).unwrap(),
        serde_json::from_str::<Value>(&serde_json::to_string(&config).unwrap()).unwrap()
    );
}
#[test]
fn unsupported_some_and_nonfinite_never_silently_save_as_omitted_or_null() {
    let mut config = legacy_settings();
    config.cold_flow_target_psi = Some(95.);
    assert!(
        payload(&config, false)
            .unwrap_err()
            .contains("does not support")
    );
    for value in [f32::NAN, f32::INFINITY, 0., -1.] {
        config.cold_flow_target_psi = Some(value);
        assert!(payload(&config, true).is_err());
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
            let stop = Arc::new(AtomicBool::new(false));
            let stopped = stop.clone();
            let worker = std::thread::spawn(move || {
                while !stopped.load(Ordering::Acquire) {
                    match listener.accept() {
                        Ok((mut socket, _)) => {
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
                            let is_post = headers.starts_with("POST ");
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
                            let (status, text) = match parsed {
                                Ok(value) => ("200 OK", value.to_string()),
                                Err(error) => ("422 Unprocessable Entity", error.to_string()),
                            };
                            let response = format!(
                                "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{text}",
                                text.len()
                            );
                            let _ = socket.write_all(response.as_bytes());
                        }
                        Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                            std::thread::sleep(Duration::from_millis(2))
                        }
                        Err(error) => panic!("fixture accept: {error}"),
                    }
                }
            });
            Self {
                base,
                posted,
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
            let mut load = Box::pin(Grant::load());
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
            let mut load = Box::pin(Grant::load());
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
}
