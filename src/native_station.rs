//! Native-only, signed LAN discovery and authenticated station handover.
use crate::auth::{SessionStatus, StoredAuthSession};
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD as B64};
use serde::Deserialize;
use std::{
    net::{SocketAddr, UdpSocket},
    time::{Duration, Instant},
};
pub type Pairing = crate::auth::StationPairing;
#[derive(Deserialize)]
struct Envelope {
    payload: String,
    signature: String,
}
#[derive(Deserialize)]
struct Discovery {
    nonce: String,
    installation_id: String,
    station_id: String,
    role: String,
    port: u16,
    scheme: String,
}
fn verified(bytes: &[u8], nonce: &str, trust: &Pairing) -> Option<Discovery> {
    if bytes.len() > 8192 {
        return None;
    }
    let e: Envelope = serde_json::from_slice(bytes).ok()?;
    let key = B64.decode(&trust.verify_key).ok()?;
    ring::signature::UnparsedPublicKey::new(&ring::signature::ED25519, &key)
        .verify(
            format!("station1.{}", e.payload).as_bytes(),
            &B64.decode(e.signature).ok()?,
        )
        .ok()?;
    let d: Discovery = serde_json::from_slice(&B64.decode(e.payload).ok()?).ok()?;
    if d.nonce != nonce
        || d.installation_id != trust.installation_id
        || d.port == 0
        || !matches!(d.scheme.as_str(), "http" | "https")
        || !trust
            .peers
            .iter()
            .any(|p| p.id == d.station_id && p.role == d.role)
    {
        return None;
    }
    Some(d)
}
fn sweep(nonce: &str, trust: &Pairing) -> Vec<String> {
    let Ok(socket) = UdpSocket::bind("0.0.0.0:0") else {
        return vec![];
    };
    let _ = socket.set_broadcast(true);
    let _ = socket.set_read_timeout(Some(Duration::from_millis(100)));
    let request = format!("GS26-STATION1 {nonce}");
    let mut destinations = vec!["255.255.255.255:49363".parse::<SocketAddr>().unwrap()];
    if let Ok(interfaces) = if_addrs::get_if_addrs() {
        for i in interfaces {
            if let if_addrs::IfAddr::V4(v) = i.addr
                && let Some(ip) = v.broadcast
            {
                destinations.push(SocketAddr::new(ip.into(), 49363));
            }
        }
    }
    destinations.sort();
    destinations.dedup();
    for d in destinations {
        let _ = socket.send_to(request.as_bytes(), d);
    }
    let end = Instant::now() + Duration::from_millis(650);
    let mut buf = [0; 8192];
    let mut result = vec![];
    let mut received = 0;
    while Instant::now() < end && received < 32 {
        let Ok((n, from)) = socket.recv_from(&mut buf) else {
            continue;
        };
        received += 1;
        if let Some(d) = verified(&buf[..n], nonce, trust) {
            let base = format!("{}://{}:{}", d.scheme, from.ip(), d.port);
            if !result.iter().any(|b| b == &base) {
                result.push(base);
            }
            if result.len() >= 8 {
                break;
            }
        }
    }
    result
}
/// Untrusted address hints for explicit user selection. No authentication request
/// or saved bearer token is sent by this discovery path.
#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
pub struct StationHint {
    pub name: String,
    pub role: String,
    pub base: String,
}
fn public_hint(bytes: &[u8], nonce: &str, from: SocketAddr) -> Option<StationHint> {
    #[derive(Deserialize)]
    struct Reply {
        protocol: String,
        nonce: String,
        name: String,
        role: String,
        scheme: String,
        port: u16,
    }
    if bytes.len() > 1024 {
        return None;
    }
    let r: Reply = serde_json::from_slice(bytes).ok()?;
    if r.protocol != "GS26-FIND1"
        || r.nonce != nonce
        || r.port == 0
        || !matches!(r.role.as_str(), "primary" | "rrls")
        || !matches!(r.scheme.as_str(), "http" | "https")
        || r.name.is_empty()
        || r.name.len() > 80
        || r.name.chars().any(char::is_control)
        || from.ip().is_unspecified()
        || from.ip().is_multicast()
    {
        return None;
    }
    Some(StationHint {
        name: r.name,
        role: r.role,
        base: format!("{}://{}", r.scheme, SocketAddr::new(from.ip(), r.port)),
    })
}
fn discover_lan() -> Result<Vec<StationHint>, String> {
    let socket =
        UdpSocket::bind("0.0.0.0:0").map_err(|e| format!("Cannot open LAN discovery: {e}"))?;
    socket
        .set_broadcast(true)
        .map_err(|e| format!("Cannot enable LAN discovery: {e}"))?;
    socket
        .set_read_timeout(Some(Duration::from_millis(100)))
        .map_err(|e| e.to_string())?;
    let mut random = [0; 24];
    getrandom::fill(&mut random).map_err(|e| e.to_string())?;
    let nonce: String = random.iter().map(|b| format!("{b:02x}")).collect();
    let request = format!("GS26-FIND1 {nonce}");
    let mut destinations = vec!["255.255.255.255:49363".parse::<SocketAddr>().unwrap()];
    if let Ok(interfaces) = if_addrs::get_if_addrs() {
        for i in interfaces {
            if let if_addrs::IfAddr::V4(v) = i.addr
                && let Some(ip) = v.broadcast
            {
                destinations.push(SocketAddr::new(ip.into(), 49363));
            }
        }
    }
    destinations.sort();
    destinations.dedup();
    let mut sent = false;
    for d in destinations {
        sent |= socket.send_to(request.as_bytes(), d).is_ok();
    }
    if !sent {
        return Err("LAN discovery could not send on this network.".into());
    }
    collect_public_hints(&socket, &nonce, Duration::from_millis(1500))
}
fn collect_public_hints(
    socket: &UdpSocket,
    nonce: &str,
    duration: Duration,
) -> Result<Vec<StationHint>, String> {
    let end = Instant::now() + duration;
    let mut buffer = [0; 1025];
    let mut stations = vec![];
    let mut received = 0;
    while Instant::now() < end && received < 128 {
        match socket.recv_from(&mut buffer) {
            Ok((n, from)) => {
                received += 1;
                if let Some(hint) = public_hint(&buffer[..n], &nonce, from) {
                    if !stations.iter().any(|s: &StationHint| s.base == hint.base) {
                        stations.push(hint);
                    }
                    if stations.len() >= 16 {
                        break;
                    }
                }
            }
            Err(e)
                if matches!(
                    e.kind(),
                    std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                ) => {}
            Err(e) => return Err(format!("LAN discovery failed: {e}")),
        }
    }
    stations.sort_by(|a, b| (&a.role, &a.base).cmp(&(&b.role, &b.base)));
    Ok(stations)
}
pub async fn discover() -> Result<Vec<StationHint>, String> {
    tokio::task::spawn_blocking(discover_lan)
        .await
        .map_err(|e| format!("LAN discovery stopped: {e}"))?
}
async fn limited_body(mut response: reqwest::Response) -> Option<Vec<u8>> {
    if !response.status().is_success() {
        return None;
    }
    let mut data = vec![];
    while let Some(bytes) = response.chunk().await.ok()? {
        if data.len() + bytes.len() > 8192 {
            return None;
        }
        data.extend(bytes);
    }
    Some(data)
}
/// No credentials leave the device until a nonce-bound response proves the
/// installation identity independently of HTTP, HTTPS, or a reverse proxy.
pub async fn find(
    session: &StoredAuthSession,
    skip_tls_verify: bool,
) -> Option<(String, SessionStatus)> {
    find_impl(session, skip_tls_verify, true).await
}
async fn find_impl(
    session: &StoredAuthSession,
    skip_tls_verify: bool,
    lan: bool,
) -> Option<(String, SessionStatus)> {
    if !session.token.starts_with("paired1.") {
        return None;
    }
    let trust = session.session.station_pairing.clone()?;
    let mut random = [0; 24];
    getrandom::fill(&mut random).ok()?;
    let nonce = B64.encode(random);
    let (n, t) = (nonce.clone(), trust.clone());
    let mut candidates = if lan {
        tokio::task::spawn_blocking(move || sweep(&n, &t))
            .await
            .ok()?
    } else {
        vec![]
    };
    // Saved paired HTTP(S) hostnames also work across routed LANs. This request
    // contains only a nonce; its result still needs the installation signature.
    let probe = reqwest::Client::builder()
        .danger_accept_invalid_certs(true)
        .redirect(reqwest::redirect::Policy::none())
        .timeout(Duration::from_millis(800))
        .build()
        .ok()?;
    let urls: Vec<_> = trust
        .peers
        .iter()
        .flat_map(|p| &p.urls)
        .take(8)
        .cloned()
        .collect();
    let results = futures_util::future::join_all(urls.into_iter().map(|base| {
        let probe = &probe;
        let nonce = &nonce;
        let trust = &trust;
        async move {
            let url = url::Url::parse(&base).ok()?;
            if !matches!(url.scheme(), "http" | "https")
                || !url.username().is_empty()
                || url.password().is_some()
                || url.query().is_some()
                || url.fragment().is_some()
                || url.path() != "/"
            {
                return None;
            }
            let response = probe
                .get(format!(
                    "{}/api/station/discovery?nonce={nonce}",
                    base.trim_end_matches('/')
                ))
                .send()
                .await
                .ok()?;
            let bytes = limited_body(response).await?;
            verified(&bytes, nonce, trust)?;
            Some(base.trim_end_matches('/').to_string())
        }
    }))
    .await;
    candidates.extend(results.into_iter().flatten());
    candidates.sort();
    candidates.dedup();
    candidates.truncate(8);
    let client = crate::auth::build_native_http_client(
        skip_tls_verify,
        Duration::from_millis(500),
        Duration::from_millis(1000),
    )
    .ok()?;
    for base in candidates {
        let Ok(response) = client
            .get(format!("{base}/api/auth/session"))
            .bearer_auth(&session.token)
            .send()
            .await
        else {
            continue;
        };
        let Some(body) = limited_body(response).await else {
            continue;
        };
        let Ok(status) = serde_json::from_slice::<SessionStatus>(&body) else {
            continue;
        };
        if status.authenticated
            && status.permissions.view_data
            && status.station_pairing.as_ref().is_some_and(|p| {
                p.installation_id == trust.installation_id
                    && p.verify_key == trust.verify_key
                    && trust.peers.iter().any(|peer| peer.id == p.station_id)
            })
        {
            return Some((base, status));
        }
    }
    None
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::auth::StationPeer as Peer;
    use ring::signature::KeyPair;
    #[test]
    fn public_hint_collection_uses_udp_reply_address_deduplicates_and_times_out() {
        let client = UdpSocket::bind("127.0.0.1:0").unwrap();
        client
            .set_read_timeout(Some(Duration::from_millis(10)))
            .unwrap();
        let sender = UdpSocket::bind("127.0.0.1:0").unwrap();
        let nonce = "0123456789abcdef";
        let reply = serde_json::json!({"protocol":"GS26-FIND1","nonce":nonce,"name":"GroundStation","role":"primary","scheme":"https","port":8443});
        for _ in 0..3 {
            sender
                .send_to(
                    &serde_json::to_vec(&reply).unwrap(),
                    client.local_addr().unwrap(),
                )
                .unwrap();
        }
        sender
            .send_to(b"not-json", client.local_addr().unwrap())
            .unwrap();
        let hints = collect_public_hints(&client, nonce, Duration::from_millis(30)).unwrap();
        assert_eq!(hints.len(), 1);
        assert_eq!(hints[0].base, "https://127.0.0.1:8443");
        assert!(
            collect_public_hints(&client, nonce, Duration::from_millis(20))
                .unwrap()
                .is_empty()
        );
    }
    #[test]
    fn public_hints_validate_nonce_role_size_and_source_address() {
        let nonce = "0123456789abcdef";
        let from = "192.0.2.8:49363".parse().unwrap();
        let value = serde_json::json!({"protocol":"GS26-FIND1","nonce":nonce,"name":"RRLS","role":"rrls","scheme":"https","port":8443});
        let bytes = serde_json::to_vec(&value).unwrap();
        let hint = public_hint(&bytes, nonce, from).unwrap();
        assert_eq!(hint.base, "https://192.0.2.8:8443");
        assert!(public_hint(&bytes, "wrong", from).is_none());
        for (field, wrong) in [
            ("role", serde_json::json!("other")),
            ("port", serde_json::json!(0)),
            ("scheme", serde_json::json!("file")),
            ("name", serde_json::json!("bad\nname")),
        ] {
            let mut bad = value.clone();
            bad[field] = wrong;
            assert!(public_hint(&serde_json::to_vec(&bad).unwrap(), nonce, from).is_none());
        }
        assert!(public_hint(&vec![b' '; 1025], nonce, from).is_none());
        assert!(public_hint(&bytes, nonce, "0.0.0.0:49363".parse().unwrap()).is_none());
    }
    #[test]
    fn rejects_unpaired_replayed_forged_or_wrong_role_discovery() {
        let key = ring::signature::Ed25519KeyPair::from_seed_unchecked(&[7; 32]).unwrap();
        let trust = Pairing {
            installation_id: "test".into(),
            station_id: "gs".into(),
            verify_key: B64.encode(key.public_key().as_ref()),
            peers: vec![Peer {
                id: "rrls".into(),
                role: "rrls".into(),
                urls: vec![],
            }],
        };
        let payload=B64.encode(serde_json::to_vec(&serde_json::json!({"nonce":"1234567890123456","installation_id":"test","station_id":"rrls","role":"rrls","port":443,"scheme":"https"})).unwrap());
        let message = format!("station1.{payload}");
        let signature = B64.encode(key.sign(message.as_bytes()).as_ref());
        let bytes =
            serde_json::to_vec(&serde_json::json!({"payload":payload,"signature":signature}))
                .unwrap();
        assert!(verified(&bytes, "1234567890123456", &trust).is_some());
        assert!(verified(&bytes, "different-nonce", &trust).is_none());
        let mut bad = trust.clone();
        bad.peers[0].role = "primary".into();
        assert!(verified(&bytes, "1234567890123456", &bad).is_none());
        bad = trust.clone();
        bad.verify_key = B64.encode([0; 32]);
        assert!(verified(&bytes, "1234567890123456", &bad).is_none());
        bad = trust.clone();
        bad.installation_id = "other".into();
        assert!(verified(&bytes, "1234567890123456", &bad).is_none());
    }
    #[tokio::test]
    async fn loopback_handover_sends_login_only_after_valid_application_identity() {
        use std::{
            io::{Read, Write},
            sync::{
                Arc,
                atomic::{AtomicBool, AtomicUsize, Ordering},
            },
        };
        for valid in [false, true] {
            let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
            listener.set_nonblocking(true).unwrap();
            let origin = format!("http://{}", listener.local_addr().unwrap());
            let key = ring::signature::Ed25519KeyPair::from_seed_unchecked(&[7; 32]).unwrap();
            let trust = Pairing {
                installation_id: "test".into(),
                station_id: "primary".into(),
                verify_key: B64.encode(key.public_key().as_ref()),
                peers: vec![Peer {
                    id: "rrls".into(),
                    role: "rrls".into(),
                    urls: vec![origin.clone()],
                }],
            };
            let target = Pairing {
                station_id: "rrls".into(),
                peers: vec![Peer {
                    id: "primary".into(),
                    role: "primary".into(),
                    urls: vec![],
                }],
                ..trust.clone()
            };
            let session = StoredAuthSession {
                token: "paired1.test-session".into(),
                session: SessionStatus {
                    authenticated: true,
                    station_pairing: Some(trust),
                    ..Default::default()
                },
                remember_me: true,
            };
            let status = SessionStatus {
                authenticated: true,
                username: Some("operator".into()),
                station_role: Some("rrls".into()),
                permissions: crate::auth::Permissions {
                    view_data: true,
                    ..Default::default()
                },
                station_pairing: Some(target),
                ..Default::default()
            };
            let stopped = Arc::new(AtomicBool::new(false));
            let auth_count = Arc::new(AtomicUsize::new(0));
            let (stop, count) = (stopped.clone(), auth_count.clone());
            let server = std::thread::spawn(move || {
                let deadline = Instant::now() + Duration::from_secs(5);
                while !stop.load(Ordering::Relaxed) && Instant::now() < deadline {
                    let Ok((mut stream, _)) = listener.accept() else {
                        std::thread::sleep(Duration::from_millis(5));
                        continue;
                    };
                    stream
                        .set_read_timeout(Some(Duration::from_millis(300)))
                        .unwrap();
                    let mut data = vec![];
                    let mut buf = [0; 1024];
                    while data.len() < 8192 && !data.windows(4).any(|w| w == b"\r\n\r\n") {
                        let Ok(n) = stream.read(&mut buf) else { break };
                        if n == 0 {
                            break;
                        }
                        data.extend_from_slice(&buf[..n]);
                    }
                    if data.is_empty() {
                        continue;
                    }
                    let request = String::from_utf8_lossy(&data);
                    let body = if request.starts_with("GET /api/station/discovery?") {
                        assert!(!request.to_lowercase().contains("authorization:"));
                        let path = request
                            .lines()
                            .next()
                            .unwrap()
                            .split_whitespace()
                            .nth(1)
                            .unwrap();
                        let url = url::Url::parse(&format!("http://fixture{path}")).unwrap();
                        let nonce = url
                            .query_pairs()
                            .find(|(k, _)| k == "nonce")
                            .unwrap()
                            .1
                            .into_owned();
                        let payload=B64.encode(serde_json::to_vec(&serde_json::json!({"nonce":nonce,"installation_id":"test","station_id":"rrls","role":"rrls","port":80,"scheme":"http"})).unwrap());
                        let signature = B64.encode(
                            key.sign(
                                format!(
                                    "{}.{}",
                                    if valid { "station1" } else { "wrong-domain" },
                                    payload
                                )
                                .as_bytes(),
                            )
                            .as_ref(),
                        );
                        serde_json::to_vec(
                            &serde_json::json!({"payload":payload,"signature":signature}),
                        )
                        .unwrap()
                    } else {
                        assert!(
                            request.starts_with("GET /api/auth/session "),
                            "unexpected fixture request: {request:?}"
                        );
                        assert!(
                            request
                                .to_lowercase()
                                .contains("authorization: bearer paired1.test-session")
                        );
                        count.fetch_add(1, Ordering::Relaxed);
                        serde_json::to_vec(&status).unwrap()
                    };
                    let header = format!(
                        "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                        body.len()
                    );
                    stream.write_all(header.as_bytes()).unwrap();
                    stream.write_all(&body).unwrap();
                }
            });
            let found = find_impl(&session, false, false).await;
            stopped.store(true, Ordering::Relaxed);
            server.join().unwrap();
            assert_eq!(found.is_some(), valid);
            assert_eq!(auth_count.load(Ordering::Relaxed), usize::from(valid));
            if let Some((base, status)) = found {
                assert_eq!(base, origin);
                assert_eq!(status.station_role.as_deref(), Some("rrls"));
            }
        }
    }
}
