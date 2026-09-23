use super::UrlConfig;
use dioxus::prelude::*;

// Keep media documents mounted after their first visit. Switching a dashboard
// tab must not tear down the microphone, audio worklet, or camera connection.
#[component]
pub(super) fn MediaToolTab(
    id: &'static str,
    path: &'static str,
    title: &'static str,
    visible: bool,
) -> Element {
    let mut opened = use_signal(|| visible);
    let mut token = use_signal(crate::auth::current_token);
    use_future(move || async move {
        loop {
            let next = crate::auth::current_token();
            if *token.peek() != next {
                token.set(next);
            }
            #[cfg(target_arch = "wasm32")]
            gloo_timers::future::TimeoutFuture::new(500).await;
            #[cfg(not(target_arch = "wasm32"))]
            tokio::time::sleep(std::time::Duration::from_millis(500)).await;
        }
    });
    use_effect(use_reactive((&visible,), move |(visible,)| {
        if visible {
            opened.set(true);
        }
        send_session(id, token.read().as_deref(), visible);
    }));
    let src = format!("{}{path}", UrlConfig::base_http().trim_end_matches('/'));
    rsx! {
        div { style: if visible { "height:100%;width:100%;" } else { "display:none;" },
            if *opened.read() {
                iframe {
                    id, src, title,
                    style: "height:100%;width:100%;border:0;",
                    allow: "camera; microphone; autoplay; fullscreen",
                    "sandbox": "allow-scripts allow-same-origin allow-forms allow-downloads",
                    onload: move |_| send_session(id, token.read().as_deref(), visible),
                }
            }
        }
    }
}

fn send_session(id: &str, token: Option<&str>, visible: bool) {
    // Tokens never enter URLs, storage, or wildcard-origin messages. The target
    // origin comes from the configured iframe URL, including for native clients.
    let id = serde_json::to_string(id).unwrap();
    let payload = serde_json::json!({
        "type": "gs26-session", "token": token.unwrap_or(""), "visible": visible,
    });
    document::eval(&format!(
        r#"
        const frame = document.getElementById({id});
        if (frame?.contentWindow) {{
            const origin = new URL(frame.src, location.href).origin;
            frame.contentWindow.postMessage({payload}, origin);
        }}
    "#
    ));
}
