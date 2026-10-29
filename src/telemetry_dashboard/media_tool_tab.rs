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
        div { style: if visible { "display:block;height:100%;width:100%;" } else { "display:none;height:100%;width:100%;" },
            if *opened.read() {
                iframe {
                    id, src, title,
                    style: "height:100%;width:100%;border:0;",
                    allow: "camera; microphone; autoplay; fullscreen; screen-wake-lock",
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
            // Keep one retry loop per mounted iframe. The first load message
            // can precede the child module listener, especially in webviews.
            // Reusing this bridge avoids creating a new Dioxus eval every tick.
            if (!frame.gs26SessionBridge) {{
                const bridge = {{payload: null}};
                bridge.send = () => {{
                    if (!frame.isConnected) {{
                        clearInterval(bridge.timer);
                        bridge.payload = null;
                        return;
                    }}
                    const origin = new URL(frame.src, location.href).origin;
                    const style = getComputedStyle(document.documentElement);
                    const theme = {{}};
                    for (const name of ['--gs26-app-background','--gs26-app-text','--gs26-panel-background','--gs26-panel-alt-background','--gs26-border','--gs26-text-muted','--gs26-button-background','--gs26-button-border','--gs26-button-text']) {{
                        const value=style.getPropertyValue(name).trim();
                        if(value) theme[name]=value;
                    }}
                    frame.contentWindow?.postMessage({{...bridge.payload,theme}}, origin);
                }};
                bridge.timer = setInterval(bridge.send, 500);
                frame.gs26SessionBridge = bridge;
            }}
            frame.gs26SessionBridge.payload = {payload};
            frame.gs26SessionBridge.send();
        }}
    "#
    ));
}

/// Camera/voice documents outlive dashboard, aiming and stream-only surfaces.
#[component]
pub(super) fn MediaSurface(media_visible: bool, voice_visible: bool, stream_visible:bool, children: Element) -> Element {
    use_drop(|| {
        // Iframe capture messages cannot clear their parent lock after removal.
        // Release only this surface's owners; other mounted media stays awake.
        document::eval(
            "for (const owner of ['media-view', 'gs26-media', 'gs26-crew-voice']) { window.GS26StayAwake?.setActive(owner, false); }",
        );
    });
    use_effect(use_reactive((&media_visible,&voice_visible,&stream_visible), move |(media,voice,stream)|{document::eval(include_str!("../../station_ui/scripts/stay-awake.js"));document::eval(&format!("window.GS26StayAwake?.setActive('media-view', {});",media||voice||stream));}));
    rsx! {
        div {style:"display:contents;",
            if crate::auth::can_publish_camera() || crate::auth::can_manage_stream() { MediaHost {host:"gs26-camera-host",slot:"gs26-media-slot",visible:media_visible,
                MediaToolTab {id:"gs26-media",path:"/media?embedded=1",title:"Cameras and recordings",visible:media_visible}
            }
            }
            if crate::auth::can_join_voice() { MediaHost {host:"gs26-voice-host",slot:"gs26-voice-slot",visible:voice_visible,
                MediaToolTab {id:"gs26-crew-voice",path:"/radio?embedded=1",title:"Voice chat",visible:voice_visible}
            }
            }
            {children}
        }
    }
}
#[component]
fn MediaHost(host: &'static str, slot: &'static str, visible: bool, children: Element) -> Element {
    rsx! {div {id:host,"data-visible":visible.to_string(),style:"position:fixed;display:none;z-index:50;",onmounted:move |_|bind_slot(host,slot),{children}}}
}
fn bind_slot(host: &str, slot: &str) {

        let host=serde_json::to_string(host).unwrap();let slot=serde_json::to_string(slot).unwrap();
        document::eval(&format!(r#"
            const host=document.getElementById({host});
            if(host && !host.gs26SlotBridge) {{
                const layout=()=>{{
                    if(!host.isConnected) {{clearInterval(timer);removeEventListener('resize',layout);removeEventListener('scroll',layout,true);return;}}
                    const slot=host.dataset.visible==='true'?document.getElementById({slot}):null;
                    if(!slot){{host.style.display='none';return;}}
                    const r=slot.getBoundingClientRect();host.style.display=r.width&&r.height?'block':'none';
                    const key=[r.left,r.top,r.width,r.height].join(',');
                    if(key!==host.gs26Bounds){{Object.assign(host.style,{{left:r.left+'px',top:r.top+'px',width:r.width+'px',height:r.height+'px'}});host.gs26Bounds=key;}}
                }};
                const timer=setInterval(layout,250);addEventListener('resize',layout);addEventListener('scroll',layout,true);
                host.gs26SlotBridge=true;layout();
            }}
        "#));

}
