use super::{UrlConfig, layout::ThemeConfig};
use crate::auth;
use dioxus::prelude::*;
use serde_json::{Value, json};

fn selection_context(value: &Value) -> Value {
    json!([
        value["backend_instance"],
        value["reset_epoch"],
        value["desired"]["revision"],
        value["board"]["generation"],
        value["board"]["epoch"],
        value["board"]["mode"],
        value["board"]["capabilities"]
    ])
}

fn settings_request(snapshot: &Value, mode: &str, release: bool) -> Result<Value, &'static str> {
    if !matches!(mode, "stepper" | "h_bridge" | "auxiliary") {
        return Err("Choose a supported drive.");
    }
    if mode == "h_bridge" {
        return Err("H-bridge drive is unavailable on this board revision.");
    }
    if snapshot["can_configure"] != true || snapshot["board"]["fresh"] != true {
        return Err("Fresh, idle actuator readback is required before changing drives.");
    }
    if snapshot["board"]["capabilities"][mode] != true {
        return Err("This drive is unavailable on the actuator board.");
    }
    if snapshot["board"]["mode"] != "auxiliary" && mode == "auxiliary" && !release {
        return Err("Switching drives requires explicitly releasing stepper holding torque.");
    }
    let instance = snapshot["backend_instance"]
        .as_str()
        .ok_or("Backend identity unavailable.")?;
    let generation = snapshot["board"]["generation"]
        .as_str()
        .ok_or("Board identity unavailable.")?;
    let epoch = snapshot["board"]["epoch"]
        .as_u64()
        .filter(|n| *n > 0 && *n < u32::MAX as u64)
        .ok_or("Board state is unavailable.")?;
    let reset_epoch = snapshot["reset_epoch"]
        .as_str()
        .filter(|s| s.len() == 16 && s.bytes().all(|b| b.is_ascii_hexdigit()))
        .ok_or("Recovery context unavailable.")?;
    let revision = snapshot["desired"]["revision"]
        .as_u64()
        .ok_or("Saved configuration unavailable.")?;
    Ok(
        json!({"mode":mode,"expected_generation":generation,"expected_epoch":epoch,
        "backend_instance":instance,"expected_revision":revision,"expected_reset_epoch":reset_epoch,"release_stepper":release}),
    )
}

#[derive(Clone)]
struct SourceContext {
    owner: Result<std::rc::Rc<super::recording_download::ExportOwner>, String>,
    base: String,
    token: Option<String>,
    #[cfg(not(target_arch = "wasm32"))]
    skip_tls: bool,
}
impl SourceContext {
    fn current() -> Self {
        Self {
            owner: super::recording_download::ExportOwner::capture_ordinary_selected(|| true),
            base: UrlConfig::base_http(),
            token: auth::current_token(),
            #[cfg(not(target_arch = "wasm32"))]
            skip_tls: UrlConfig::_skip_tls_verify(),
        }
    }
    fn matches_current(&self) -> bool {
        self.owner.as_ref().is_ok_and(|owner| owner.is_current())
    }
}
/// SET uses the exact clicked URL/token, even if the source changes before the
/// spawned future starts. A response from the old source cannot clear a new login.
async fn save_to_source(context: &SourceContext, body: &Value) -> Result<Value, String> {
    if !context.matches_current() {
        return Err("Connection changed; query and choose the drive again.".into());
    }
    let owner = context.owner.as_ref().map_err(Clone::clone)?;
    owner.check_ordinary()?;
    #[cfg(target_arch = "wasm32")]
    let (status, text) = {
        let base = if context.base.is_empty() {
            web_sys::window()
                .ok_or("No window")?
                .location()
                .origin()
                .map_err(|_| "Backend origin unavailable")?
        } else {
            context.base.clone()
        };
        let controller =
            web_sys::AbortController::new().map_err(|_| "HTTP cancellation unavailable")?;
        let _abort = super::HttpAbortOnDrop(controller.clone());
        let mut request =
            gloo_net::http::Request::post(&format!("{base}/api/actuator/plumbing/settings"))
                .abort_signal(Some(&controller.signal()));
        if let Some(epoch) = owner.policy_epoch() {
            request = request.header(super::station_policy_context::HTTP_EPOCH_HEADER, epoch);
        }
        if let Some(origin) = owner.control_origin() {
            request = request.header("X-Station-Control-Origin", origin);
        }
        if let Some(token) = &context.token {
            request = request.header("Authorization", &format!("Bearer {token}"));
        }
        let request = request.json(body).map_err(|e| e.to_string())?;
        let fetch = async {
            let response = request.send().await.map_err(|e| e.to_string())?;
            let status = response.status();
            let text = response.text().await.map_err(|e| e.to_string())?;
            Ok::<_, String>((status, text))
        };
        let timeout = gloo_timers::future::TimeoutFuture::new(8000);
        futures_util::pin_mut!(fetch, timeout);
        match owner.wait(futures_util::future::select(fetch, timeout)).await? {
            futures_util::future::Either::Left((result, _)) => result?,
            futures_util::future::Either::Right(_) => {
                return Err(
                    "Drive request timed out; query actual state before explicitly retrying."
                        .into(),
                );
            }
        }
    };
    #[cfg(not(target_arch = "wasm32"))]
    let (status, text) = {
        let base = if context.base.is_empty() {
            "http://localhost:3000"
        } else {
            context.base.as_str()
        };
        let client = auth::build_native_http_client(
            context.skip_tls,
            std::time::Duration::from_secs(8),
            std::time::Duration::from_secs(8),
        )?;
        let mut request = client
            .post(format!("{base}/api/actuator/plumbing/settings"))
            .json(body);
        if let Some(epoch) = owner.policy_epoch() {
            request = request.header(super::station_policy_context::HTTP_EPOCH_HEADER, epoch);
        }
        if let Some(origin) = owner.control_origin() {
            request = request.header("X-Station-Control-Origin", origin);
        }
        if let Some(token) = &context.token {
            request = request.bearer_auth(token);
        }
        let response = owner.wait(request.send()).await?.map_err(|e| e.to_string())?;
        let status = response.status().as_u16();
        let text = owner.wait(response.text()).await?.map_err(|e| e.to_string())?;
        (status, text)
    };
    owner.check()?;
    if status == 401 && context.matches_current() {
        auth::clear_current_session();
    }
    if !(200..300).contains(&status) {
        return Err(format!(
            "HTTP {status}: {}",
            text.chars().take(240).collect::<String>()
        ));
    }
    serde_json::from_str(&text).map_err(|e| e.to_string())
}

#[component]
pub(super) fn PlumbingDriveSelector(theme: ThemeConfig) -> Element {
    let mut snapshot = use_signal(|| None::<Value>);
    let mut snapshot_source = use_signal(|| None::<SourceContext>);
    let mut draft = use_signal(|| None::<String>);
    let mut release = use_signal(|| false);
    let mut busy = use_signal(|| false);
    let mut message = use_signal(String::new);
    use_future(move || async move {
        let mut context = None::<SourceContext>;
        let mut query_tick = 0u8;
        loop {
            let current = SourceContext::current();
            if context.as_ref().is_none_or(|old| !old.matches_current()) {
                snapshot.set(None);
                snapshot_source.set(None);
                query_tick = 0;
                draft.set(None);
                release.set(false);
                message.set(String::new());
                context = Some(current.clone());
            }
            if !*busy.peek() {
                if query_tick == 0 && auth::current_status().station_role.as_deref() != Some("rrls")
                {
                    let _ = super::station_settings_request::post_captured::<_, Value>("/api/actuator/plumbing/query", &json!({}), current.owner.clone())
                        .await;
                    if current.matches_current() {
                        let _ = super::station_settings_request::post_captured::<_, Value>(
                            "/api/board-recovery/query",
                            &json!({"board":"AB"}), current.owner.clone(),
                        )
                        .await;
                    }
                }
                let next = match current.owner.clone() {
                    Ok(owner) => super::recording_download::saved_json::<Value>("/api/actuator/plumbing/status", owner).await,
                    Err(error) => Err(error),
                };
                if current.matches_current() && !*busy.peek() {
                    match next {
                        Ok(next) => {
                            if snapshot.peek().as_ref().is_some_and(|old| {
                                selection_context(old) != selection_context(&next)
                            }) {
                                draft.set(None);
                                release.set(false);
                                message.set("Board or saved configuration changed. Choose the drive again before saving.".into());
                            }
                            snapshot_source.set(Some(current.clone()));
                            snapshot.set(Some(next));
                        }
                        Err(error) => {
                            snapshot.set(None);
                            snapshot_source.set(None);
                            draft.set(None);
                            release.set(false);
                            message.set(format!("Actuator configuration unavailable: {error}"));
                        }
                    }
                }
            }
            query_tick = (query_tick + 1) % 4;
            #[cfg(target_arch = "wasm32")]
            gloo_timers::future::TimeoutFuture::new(500).await;
            #[cfg(not(target_arch = "wasm32"))]
            tokio::time::sleep(std::time::Duration::from_millis(500)).await;
        }
    });
    let current = snapshot.read().clone().unwrap_or(Value::Null);
    let selected = draft.read().clone().unwrap_or_else(|| {
        current["desired"]["mode"]
            .as_str()
            .unwrap_or("stepper")
            .to_string()
    });
    let can_edit = auth::can_send_command("RetractPlumbing")
        && snapshot_source
            .read()
            .as_ref()
            .is_some_and(|source| source.matches_current())
        && current["board"].is_object()
        && !*busy.read();
    let can_save =
        can_edit && current["can_configure"] == true && current["board"]["fresh"] == true;
    let blocked_reason = current["blocked_reason"]
        .as_str()
        .filter(|reason| !reason.is_empty())
        .map(|reason| reason.chars().take(180).collect::<String>())
        .unwrap_or_else(|| "Waiting for safe actuator readback before saving.".into());
    let releases_hold = current["board"]["mode"] != "auxiliary" && selected == "auxiliary";
    let board_mode = current["board"]["mode"]
        .as_str()
        .unwrap_or("Unknown")
        .to_string();
    let running = current["board"]["fresh"] == true && current["board"]["busy"] == true;
    let remaining = current["board"]["remaining_ms"].as_u64().unwrap_or(0) as f64 / 1000.0;
    rsx! {
        section { aria_label: "Fill-line drive", style: "display:grid;gap:10px;min-width:0;padding:12px;border:1px solid {theme.border_soft};border-radius:10px;background:{theme.panel_background};color:{theme.text_primary};font:inherit;",
            strong { "Fill-line drive" }
            label { r#for: "plumbing-drive", "Use for fill-line retraction" }
            select { id: "plumbing-drive", value: "{selected}", disabled: !can_edit,
                style: "box-sizing:border-box;width:100%;min-width:0;min-height:44px;padding:8px;font:inherit;background:{theme.button_background};color:{theme.text_primary};border:1px solid {theme.border};border-radius:6px;",
                onchange: move |event| { draft.set(Some(event.value())); release.set(false); },
                option { value: "stepper", disabled: current["board"]["capabilities"]["stepper"] != true, "Stepper · existing travel profile" }
                option { value: "h_bridge", disabled: true, "H-bridge · extend / retract, 45 seconds" }
                option { value: "auxiliary", disabled: current["board"]["capabilities"]["auxiliary"] != true, "Auxiliary output · retract only, 45 seconds" }
            }
            p { style: "margin:0;color:{theme.text_muted};", "Actuator selection: {board_mode}. Timed motion does not verify physical fill-line position." }
            if current["pending"] == true {
                p { role: "status", style: "margin:0;", if current["desired"]["durable"] == true { "Desired setting saved; waiting for matching actuator readback." } else { "Waiting for actual actuator drive readback." } }
            }
            if running {
                p { role: "status", style: "margin:0;",
                    if remaining > 0.0 { "Drive running · {remaining:.1} seconds remaining" }
                    else { "Drive running" }
                }
            }
            p { style: "margin:0;color:{theme.text_muted};", "H-bridge drive is unavailable on the current board revision." }
            if current["board"]["result"] == "io" {
                p { role: "status", style: "margin:0;", "Drive metadata needs an explicit retry. Query actual Idle state, choose the drive, and save again. Motion remains inhibited." }
            }
            if releases_hold {
                label { style: "display:flex;align-items:flex-start;gap:8px;min-width:0;",
                    input { r#type: "checkbox", checked: *release.read(), disabled: !can_edit,
                        onchange: move |event| release.set(event.checked()) }
                    span { "Release the idle stepper’s holding torque when switching drives." }
                }
            }
            button { r#type: "button", disabled: !can_save || (releases_hold && !*release.read()),
                style: "min-height:44px;max-width:100%;width:auto;padding:10px 14px;white-space:normal;font:inherit;",
                onclick: move |_| {
                    let Some(before) = snapshot.peek().clone() else { return; };
                    let Some(context)=snapshot_source.peek().clone() else {return;};
                    if !context.matches_current() {snapshot.set(None);draft.set(None);release.set(false);message.set("Connection changed. Query and choose the drive again.".into());return;}
                    let mode = draft.peek().clone().unwrap_or_else(|| before["desired"]["mode"].as_str().unwrap_or("stepper").to_string());
                    let body = match settings_request(&before, &mode, *release.peek()) {
                        Ok(body) => body, Err(error) => {message.set(error.into()); return;}
                    };
                    busy.set(true);
                    spawn(async move {
                        let result = save_to_source(&context, &body).await;
                        if context.matches_current() {
                            match result {
                                Ok(next) => { let note=next["message"].as_str().unwrap_or("Drive response received; desired and actual readback are shown separately.").to_string(); snapshot_source.set(Some(context.clone())); snapshot.set(Some(next)); draft.set(None); release.set(false); message.set(note); }
                                Err(error) => { snapshot.set(None); snapshot_source.set(None); draft.set(None);release.set(false); message.set(format!("Drive change failed: {error}")); }
                            }
                        }
                        busy.set(false);
                    });
                },
                if releases_hold { "Switch drive and release stepper holding torque" } else { "Save drive selection" }
            }
            if can_edit && !can_save {
                p { role: "status", style: "margin:0;color:{theme.text_muted};", "{blocked_reason}" }
            }
            if !message.read().is_empty() { p { role: "status", style: "margin:0;overflow-wrap:anywhere;", "{message}" } }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn snapshot() -> Value {
        json!({"can_configure":true,"backend_instance":"0000000000000001","reset_epoch":"0000000000000001","desired":{"mode":"stepper","revision":3},
            "board":{"mode":"stepper","fresh":true,"generation":"0000000000000007","epoch":4,
                "capabilities":{"stepper":true,"h_bridge":false,"auxiliary":true}}})
    }
    #[test]
    fn mode_change_requires_actual_capability_fresh_context_and_explicit_holding_release() {
        let value = snapshot();
        assert!(settings_request(&value, "h_bridge", true).is_err());
        assert!(settings_request(&value, "auxiliary", false).is_err());
        let request = settings_request(&value, "auxiliary", true).unwrap();
        assert_eq!(request["expected_generation"], "0000000000000007");
        assert_eq!(request["expected_revision"], 3);
        for path in ["can_configure", "fresh"] {
            let mut invalid = value.clone();
            if path == "fresh" {
                invalid["board"][path] = json!(false);
            } else {
                invalid[path] = json!(false);
            }
            assert!(settings_request(&invalid, "auxiliary", true).is_err());
        }
        let mut unknown = value;
        unknown["board"]["generation"] = Value::Null;
        assert!(settings_request(&unknown, "auxiliary", true).is_err());
    }
    #[test]
    fn another_backend_reboot_or_saved_revision_invalidates_the_draft_context() {
        let value = snapshot();
        let before = selection_context(&value);
        for (field, next) in [
            ("backend_instance", json!("another")),
            ("desired", json!({"revision":4})),
            ("board", json!({"generation":"new","epoch":4})),
        ] {
            let mut after = value.clone();
            after[field] = next;
            assert_ne!(selection_context(&after), before);
        }
    }
    #[test]
    fn temporary_readback_expiry_and_result_do_not_discard_the_selected_drive() {
        let value = snapshot();
        let authority = selection_context(&value);
        let mut expired = value.clone();
        expired["can_configure"] = json!(false);
        expired["board"]["fresh"] = json!(false);
        expired["board"]["result"] = json!("pending");
        expired["blocked_reason"] = json!("Waiting for matching readback.");
        assert_eq!(selection_context(&expired), authority);
        assert!(settings_request(&expired, "auxiliary", true).is_err());
        expired["can_configure"] = json!(true);
        expired["board"]["fresh"] = json!(true);
        assert!(settings_request(&expired, "auxiliary", true).is_ok());
        assert!(settings_request(&expired, "auxiliary", false).is_err());
        for field in ["mode", "epoch", "generation", "capabilities"] {
            let mut changed = value.clone();
            changed["board"][field] = Value::Null;
            assert_ne!(selection_context(&changed), authority, "{field}");
        }
    }
    #[test]
    fn unavailable_bridge_stays_blocked_even_when_a_capability_bit_is_wrong() {
        let mut value = snapshot();
        value["board"]["capabilities"]["h_bridge"] = json!(true);
        assert!(settings_request(&value, "h_bridge", true).is_err());
    }
    #[test]
    fn recovery_context_and_unknown_mode_require_new_binding_and_explicit_release() {
        let value = snapshot();
        let before = selection_context(&value);
        let mut changed = value.clone();
        changed["reset_epoch"] = json!("0000000000000002");
        assert_ne!(selection_context(&changed), before);
        assert_eq!(
            settings_request(&changed, "auxiliary", true).unwrap()["expected_reset_epoch"],
            "0000000000000002"
        );
        changed["board"]["mode"] = Value::Null;
        assert!(settings_request(&changed, "auxiliary", false).is_err());
        assert!(settings_request(&changed, "auxiliary", true).is_ok());
        changed["reset_epoch"] = Value::Null;
        assert!(settings_request(&changed, "auxiliary", true).is_err());
    }
}
