//! Crew voice relay for a watch that cannot reach the station itself
//! (docs/watch-companion.md "Phone relay"). The watch speaks the station's
//! voice protocol inside watch messages; the phone opens `/api/voice/ws`
//! with the watch's own token (so the station sees the watch, with its voice
//! grant and echo-avoidance group) and moves frames both ways. The phone
//! never adds anything: it sends `auth` itself and forwards only the four
//! control messages the protocol allows.
use base64::Engine;
use base64::engine::general_purpose::STANDARD as B64;
use serde::{Deserialize, Serialize};

/// Frames in one relay message either way (200 ms of audio).
pub const MAX_RELAY_FRAMES: usize = 10;
/// Phone → watch batches go out at least this often while audio flows.
pub const FLUSH_MS: f64 = 60.0;
/// Largest Opus packet the station accepts per 20 ms frame.
pub const MAX_OPUS_BYTES: usize = 400;
pub const PCM_FRAME_BYTES: usize = 640;
/// Keeps a batch well under the watch transport's message limit.
const MAX_BATCH_BYTES: usize = 16 * 1024;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Codec {
    #[default]
    Opus,
    Pcm16,
}

impl Codec {
    fn uplink_valid(self, len: usize) -> bool {
        match self {
            Codec::Opus => (1..=MAX_OPUS_BYTES).contains(&len),
            Codec::Pcm16 => len == PCM_FRAME_BYTES,
        }
    }
}

/// Watch → phone relay message.
#[derive(Clone, Debug, PartialEq, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum RelayRequest {
    Open {
        #[serde(default)]
        codec: Codec,
    },
    Control {
        msg: serde_json::Value,
    },
    Audio {
        #[serde(default)]
        frames: Vec<String>,
    },
    Close,
}

impl RelayRequest {
    /// Parses `{"v":1,"type":"voice","op":…}`; anything else is ignored.
    pub fn parse(json: &str) -> Option<Self> {
        if json.len() > super::protocol::MAX_MESSAGE_BYTES {
            return None;
        }
        let value: serde_json::Value = serde_json::from_str(json).ok()?;
        if value.get("v").and_then(|v| v.as_u64()) != Some(u64::from(super::protocol::PROTOCOL_VERSION))
            || value.get("type").and_then(|t| t.as_str()) != Some("voice")
        {
            return None;
        }
        serde_json::from_value(value).ok()
    }
}

/// The station control message to forward for a watch `control`, rebuilt
/// from its type and flag so nothing else (a token, another sender id)
/// passes through. `auth` is never forwarded: the phone sends it itself.
pub fn sanitize_control(msg: &serde_json::Value) -> Option<String> {
    let kind = msg.get("type")?.as_str()?;
    let value = match kind {
        "ping" => serde_json::json!({"type": "ping"}),
        "transmitting" | "active" | "radio" => {
            serde_json::json!({"type": kind, "enabled": msg.get("enabled")?.as_bool()?})
        }
        _ => return None,
    };
    Some(value.to_string())
}

/// Decodes a watch `audio` message into station frames; refuses the whole
/// message when it is too long or any frame is not one valid frame.
pub fn decode_audio(frames: &[String], codec: Codec) -> Option<Vec<Vec<u8>>> {
    if frames.len() > MAX_RELAY_FRAMES {
        return None;
    }
    frames
        .iter()
        .map(|f| B64.decode(f).ok().filter(|b| codec.uplink_valid(b.len())))
        .collect()
}

/// `http(s)://host[:port]` → `ws(s)://host[:port]/api/voice/ws`.
pub fn voice_ws_url(base: &str) -> Option<String> {
    let base = base.trim().trim_end_matches('/');
    let rest = if let Some(rest) = base.strip_prefix("https://") {
        format!("wss://{rest}")
    } else if let Some(rest) = base.strip_prefix("http://") {
        format!("ws://{rest}")
    } else {
        return None;
    };
    Some(format!("{rest}/api/voice/ws"))
}

pub fn auth_message(token: &str, codec: Codec) -> String {
    serde_json::json!({"type": "auth", "token": token, "codec": codec}).to_string()
}

/// `closed` message for the watch.
pub fn closed_message(reason: &str) -> String {
    serde_json::json!({"v": super::protocol::PROTOCOL_VERSION, "type": "voice", "op": "closed", "reason": reason})
        .to_string()
}

/// Station → watch batch: text messages and binary frames (speaker id +
/// payload) collected for at most [`FLUSH_MS`].
#[derive(Debug, Default)]
pub struct Batch {
    text: Vec<String>,
    frames: Vec<String>,
    bytes: usize,
    first_at: Option<f64>,
}

impl Batch {
    pub fn push_text(&mut self, text: String, now: f64) {
        self.bytes += text.len();
        self.text.push(text);
        self.first_at.get_or_insert(now);
    }

    pub fn push_frame(&mut self, frame: &[u8], now: f64) {
        let encoded = B64.encode(frame);
        self.bytes += encoded.len();
        self.frames.push(encoded);
        self.first_at.get_or_insert(now);
    }

    pub fn is_empty(&self) -> bool {
        self.text.is_empty() && self.frames.is_empty()
    }

    /// Send now: text (roster, handoff, errors) goes at once; audio after
    /// [`FLUSH_MS`] or when the batch is full.
    pub fn due(&self, now: f64) -> bool {
        !self.text.is_empty()
            || self.frames.len() >= MAX_RELAY_FRAMES
            || self.bytes >= MAX_BATCH_BYTES
            || self.first_at.is_some_and(|t| now - t >= FLUSH_MS)
    }

    pub fn take_json(&mut self) -> String {
        let message = serde_json::json!({
            "v": super::protocol::PROTOCOL_VERSION, "type": "voice", "op": "server",
            "text": std::mem::take(&mut self.text), "frames": std::mem::take(&mut self.frames),
        });
        self.bytes = 0;
        self.first_at = None;
        message.to_string()
    }
}

/// Runtime: one relayed session at a time, driven from the app-wide watch
/// publisher. Ends when the watch closes it, the socket closes, the watch
/// stops sending for 30 s (it pings every 10 s) or the token loses voice.
pub(super) async fn run() {
    use futures_util::{SinkExt, StreamExt};
    use tokio_tungstenite::tungstenite::Message;
    loop {
        // Idle: wait for the watch to open a relay.
        let codec = loop {
            match super::bridge::take_voice().as_deref().and_then(RelayRequest::parse) {
                Some(RelayRequest::Open { codec }) => break codec,
                Some(_) => super::bridge::send_voice(&closed_message("not open")),
                None => super::sleep_ms(100).await,
            }
        };
        let Some(held) = super::direct_held().filter(|h| h.grant.voice) else {
            super::bridge::send_voice(&closed_message("This watch has no voice access. Check Settings → Watch → Voice on the phone."));
            continue;
        };
        let Some(url) = voice_ws_url(&held.want.base) else {
            super::bridge::send_voice(&closed_message("unsupported station address"));
            continue;
        };
        let connected = tokio::time::timeout(std::time::Duration::from_secs(8), async {
            if url.starts_with("wss://") && held.want.skip_tls {
                let tls = super::super::insecure_rustls_connector()?;
                tokio_tungstenite::connect_async_tls_with_config(url.as_str(), None, false, Some(tls))
                    .await
                    .map_err(|e| e.to_string())
            } else if url.starts_with("wss://") {
                #[cfg(any(target_os = "android", target_os = "ios", target_os = "macos"))]
                {
                    let tls = super::super::platform_rustls_connector()?;
                    tokio_tungstenite::connect_async_tls_with_config(url.as_str(), None, false, Some(tls))
                        .await
                        .map_err(|e| e.to_string())
                }
                #[cfg(not(any(target_os = "android", target_os = "ios", target_os = "macos")))]
                {
                    tokio_tungstenite::connect_async(url.as_str()).await.map_err(|e| e.to_string())
                }
            } else {
                tokio_tungstenite::connect_async(url.as_str()).await.map_err(|e| e.to_string())
            }
        })
        .await;
        let mut ws = match connected {
            Ok(Ok((ws, _))) => ws,
            Ok(Err(e)) => {
                super::bridge::send_voice(&closed_message(&format!("phone cannot reach the station: {e}")));
                continue;
            }
            Err(_) => {
                super::bridge::send_voice(&closed_message("phone cannot reach the station: timed out"));
                continue;
            }
        };
        if ws.send(Message::Text(auth_message(&held.grant.token, codec).into())).await.is_err() {
            super::bridge::send_voice(&closed_message("station closed the connection"));
            continue;
        }
        let line = "[watch] voice relay open";
        eprintln!("{line}");
        crate::debug_log::append(line);
        let mut batch = Batch::default();
        let mut last_watch = super::super::monotonic_now_ms();
        let reason = loop {
            let now = super::super::monotonic_now_ms();
            if now - last_watch > 30_000.0 {
                break "watch stopped sending";
            }
            // Watch → station.
            let mut closed = false;
            while let Some(raw) = super::bridge::take_voice() {
                last_watch = now;
                match RelayRequest::parse(&raw) {
                    Some(RelayRequest::Control { msg }) => {
                        if let Some(text) = sanitize_control(&msg) {
                            let _ = ws.send(Message::Text(text.into())).await;
                        }
                    }
                    Some(RelayRequest::Audio { frames }) => {
                        for frame in decode_audio(&frames, codec).unwrap_or_default() {
                            let _ = ws.send(Message::Binary(frame.into())).await;
                        }
                    }
                    Some(RelayRequest::Close) | Some(RelayRequest::Open { .. }) => closed = true,
                    None => {}
                }
            }
            if closed {
                break "closed by the watch";
            }
            // Station → watch, for up to 20 ms.
            match tokio::time::timeout(std::time::Duration::from_millis(20), ws.next()).await {
                Ok(Some(Ok(Message::Text(text)))) => batch.push_text(text.to_string(), now),
                Ok(Some(Ok(Message::Binary(bytes)))) => batch.push_frame(&bytes, now),
                Ok(Some(Ok(Message::Close(_)))) | Ok(None) | Ok(Some(Err(_))) => break "station closed the connection",
                Ok(Some(Ok(_))) | Err(_) => {}
            }
            if batch.due(super::super::monotonic_now_ms()) {
                super::bridge::send_voice(&batch.take_json());
            }
        };
        if !batch.is_empty() {
            super::bridge::send_voice(&batch.take_json());
        }
        let _ = ws.close(None).await;
        super::bridge::send_voice(&closed_message(reason));
        let line = format!("[watch] voice relay closed: {reason}");
        eprintln!("{line}");
        crate::debug_log::append(&line);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn relay_requests_parse_strictly() {
        assert_eq!(
            RelayRequest::parse(r#"{"v":1,"type":"voice","op":"open","codec":"opus"}"#),
            Some(RelayRequest::Open { codec: Codec::Opus })
        );
        assert_eq!(
            RelayRequest::parse(r#"{"v":1,"type":"voice","op":"open","codec":"pcm16"}"#),
            Some(RelayRequest::Open { codec: Codec::Pcm16 })
        );
        assert_eq!(RelayRequest::parse(r#"{"v":1,"type":"voice","op":"close"}"#), Some(RelayRequest::Close));
        assert!(matches!(
            RelayRequest::parse(r#"{"v":1,"type":"voice","op":"audio","frames":["AQ=="]}"#),
            Some(RelayRequest::Audio { .. })
        ));
        for bad in [
            r#"{"v":2,"type":"voice","op":"close"}"#,
            r#"{"v":1,"type":"settings","op":"close"}"#,
            r#"{"v":1,"type":"voice","op":"auth","token":"x"}"#,
            r#"{"v":1,"type":"voice","op":"open","codec":"mp3"}"#,
            "not json",
        ] {
            assert_eq!(RelayRequest::parse(bad), None, "{bad}");
        }
    }

    #[test]
    fn only_the_four_controls_pass_and_are_rebuilt() {
        assert_eq!(
            sanitize_control(&serde_json::json!({"type":"transmitting","enabled":true,"token":"steal"})).as_deref(),
            Some(r#"{"enabled":true,"type":"transmitting"}"#)
        );
        assert!(sanitize_control(&serde_json::json!({"type":"active","enabled":false})).is_some());
        assert!(sanitize_control(&serde_json::json!({"type":"radio","enabled":true})).is_some());
        assert_eq!(sanitize_control(&serde_json::json!({"type":"ping","x":1})).as_deref(), Some(r#"{"type":"ping"}"#));
        for bad in [
            serde_json::json!({"type":"auth","token":"t"}),
            serde_json::json!({"type":"transmitting"}),
            serde_json::json!({"type":"transmitting","enabled":"yes"}),
            serde_json::json!({"type":"command","cmd":"Abort"}),
            serde_json::json!("ping"),
        ] {
            assert_eq!(sanitize_control(&bad), None, "{bad}");
        }
    }

    #[test]
    fn audio_frames_are_validated_per_codec() {
        let opus = B64.encode([1u8, 2, 3]);
        let pcm = B64.encode([0u8; PCM_FRAME_BYTES]);
        assert_eq!(decode_audio(std::slice::from_ref(&opus), Codec::Opus).unwrap(), vec![vec![1, 2, 3]]);
        assert!(decode_audio(std::slice::from_ref(&opus), Codec::Pcm16).is_none());
        assert_eq!(decode_audio(std::slice::from_ref(&pcm), Codec::Pcm16).unwrap().len(), 1);
        assert!(decode_audio(&[B64.encode([0u8; MAX_OPUS_BYTES + 1])], Codec::Opus).is_none());
        assert!(decode_audio(&["!!".into()], Codec::Opus).is_none());
        assert!(decode_audio(&vec![opus; MAX_RELAY_FRAMES + 1], Codec::Opus).is_none());
    }

    #[test]
    fn urls_and_auth() {
        assert_eq!(voice_ws_url("https://gs.local:3000/").as_deref(), Some("wss://gs.local:3000/api/voice/ws"));
        assert_eq!(voice_ws_url("http://10.0.0.2").as_deref(), Some("ws://10.0.0.2/api/voice/ws"));
        assert_eq!(voice_ws_url("ftp://x"), None);
        let auth: serde_json::Value = serde_json::from_str(&auth_message("gsw1.t", Codec::Opus)).unwrap();
        assert_eq!(auth, serde_json::json!({"type":"auth","token":"gsw1.t","codec":"opus"}));
        let closed: serde_json::Value = serde_json::from_str(&closed_message("bye")).unwrap();
        assert_eq!(closed["op"], "closed");
        assert_eq!(closed["reason"], "bye");
    }

    #[test]
    fn batches_flush_text_at_once_and_audio_within_60_ms() {
        let mut batch = Batch::default();
        assert!(!batch.due(0.0));
        batch.push_frame(&[7, 0, 0, 0, 1], 100.0);
        assert!(!batch.due(120.0));
        assert!(batch.due(160.0));
        let json: serde_json::Value = serde_json::from_str(&batch.take_json()).unwrap();
        assert_eq!(json["op"], "server");
        assert_eq!(json["frames"], serde_json::json!([B64.encode([7, 0, 0, 0, 1])]));
        assert!(batch.is_empty() && !batch.due(1_000.0));
        batch.push_text(r#"{"type":"roster"}"#.into(), 0.0);
        assert!(batch.due(0.0));
        batch.take_json();
        for _ in 0..MAX_RELAY_FRAMES {
            batch.push_frame(&[0; 64], 0.0);
        }
        assert!(batch.due(0.0), "a full batch goes at once");
        assert!(batch.take_json().len() < super::super::protocol::MAX_MESSAGE_BYTES);
    }
}
