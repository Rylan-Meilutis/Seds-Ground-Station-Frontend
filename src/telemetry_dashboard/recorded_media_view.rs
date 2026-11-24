//! Recorded camera video and crew voice of an offline replay, played in step
//! with the local station's clock (`local_station::media`).
//!
//! Shown on the broadcast page while a local station is selected, in place of
//! the live camera and voice (which need a station). Each camera stream gets
//! a `<video>` showing the segment that covers the playback position; each
//! voice burst playing at that position gets an `<audio>`. Every 250 ms the
//! elements are brought to the clock: rate, play/pause and, past the drift
//! the speed allows, a seek ([`media::sync`]). Clip bytes come from the
//! bundle: blob URLs in the browser, the app's own `local-media/` protocol
//! path in native webviews. Nothing is fetched from a network; with no local
//! station this renders nothing and starts no timer.
use super::local_station::media::{self, Clip, Kind, Media};
use super::local_station::LocalStation;
use dioxus::prelude::*;
use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

async fn wait_tick() {
    #[cfg(target_arch = "wasm32")]
    gloo_timers::future::TimeoutFuture::new(250).await;
    #[cfg(not(target_arch = "wasm32"))]
    tokio::time::sleep(std::time::Duration::from_millis(250)).await;
}

/// `HH:MM:SS` UTC of a Unix time in ms.
fn utc(ms: i64) -> String {
    let s = ms.div_euclid(1000).rem_euclid(86_400);
    format!("{:02}:{:02}:{:02} UTC", s / 3600, s / 60 % 60, s % 60)
}

/// A DOM id for a clip's element.
pub(crate) fn element_id(clip: &Clip) -> String {
    let mut id = String::from("gs26-rec-");
    id.extend(clip.path.chars().map(|c| if c.is_ascii_alphanumeric() { c } else { '-' }));
    id
}

/// Playable URLs of clips, made on first use and released when unused.
#[derive(Default)]
struct Urls {
    made: RefCell<HashMap<String, (String, u64)>>,
}

impl Urls {
    fn url(&self, clip: &Clip, tick: u64) -> Option<String> {
        if let Some((url, last)) = self.made.borrow_mut().get_mut(&clip.path) {
            *last = tick;
            return Some(url.clone());
        }
        let url = make_url(clip)?;
        self.made.borrow_mut().insert(clip.path.clone(), (url.clone(), tick));
        Some(url)
    }
    /// Releases URLs no element has used for two seconds.
    fn sweep(&self, tick: u64) {
        self.made.borrow_mut().retain(|_, (url, last)| {
            let keep = tick.saturating_sub(*last) < 8;
            if !keep {
                release_url(url);
            }
            keep
        });
    }
    fn clear(&self) {
        for (_, (url, _)) in self.made.borrow_mut().drain() {
            release_url(&url);
        }
    }
}

#[cfg(target_arch = "wasm32")]
fn make_url(clip: &Clip) -> Option<String> {
    let bytes = js_sys::Uint8Array::from(clip.body.as_slice());
    let options = web_sys::BlobPropertyBag::new();
    options.set_type(&clip.media_type);
    let blob = web_sys::Blob::new_with_u8_array_sequence_and_options(&js_sys::Array::of1(&bytes), &options).ok()?;
    web_sys::Url::create_object_url_with_blob(&blob).ok()
}
#[cfg(target_arch = "wasm32")]
fn release_url(url: &str) {
    let _ = web_sys::Url::revoke_object_url(url);
}

/// Native webviews read the clip through the app's protocol handler, which
/// serves only the selected local station's clips.
#[cfg(not(target_arch = "wasm32"))]
fn make_url(clip: &Clip) -> Option<String> {
    Some(super::map_tiles_url().replace("tiles/{z}/{x}/{y}.jpg", "local-media/") + &clip.path)
}
#[cfg(not(target_arch = "wasm32"))]
fn release_url(_url: &str) {}

/// The script that brings each element to its clip's [`media::Sync`].
fn sync_script(targets: &[(String, media::Sync)]) -> String {
    let list: Vec<serde_json::Value> = targets
        .iter()
        .map(|(id, s)| serde_json::json!({"id": id, "at": s.offset_s, "rate": s.rate, "play": s.play, "tol": s.tolerance_s, "muted": s.muted}))
        .collect();
    format!(
        "(()=>{{for(const c of {}){{const e=document.getElementById(c.id);if(!e||e.readyState<1)continue;\
         const d=Number.isFinite(e.duration)?e.duration:Infinity;const at=Math.min(c.at,d);\
         if(e.playbackRate!==c.rate)e.playbackRate=c.rate;e.muted=c.muted;\
         if(Math.abs(e.currentTime-at)>c.tol)e.currentTime=at;\
         if(c.play&&at<d){{if(e.paused)e.play().catch(()=>{{}});}}else if(!e.paused)e.pause();}}}})()",
        serde_json::Value::Array(list)
    )
}

/// What plays at `position`: (stream, clip) per camera, and the voice bursts.
fn playing(media: &Media, position: i64) -> (Vec<(String, Option<&Clip>)>, Vec<&Clip>) {
    let cameras = media.streams(Kind::Video).into_iter().map(|s| { let at = media.at(Kind::Video, &s, position); (s, at) }).collect();
    (cameras, media.playing(Kind::Voice, position))
}

/// Why a kind has no clips in this bundle, from the index's summary.
fn absent(kind: Kind, media: Option<&Media>) -> String {
    let (what, toggle) = match kind {
        Kind::Video => ("camera video", "Camera video"),
        Kind::Voice => ("crew voice", "Crew voice"),
    };
    let summary = media.map(|m| if kind == Kind::Video { &m.video } else { &m.voice });
    match summary {
        Some(s) if s["recorded"] == false => format!("The station that made this bundle does not record {what}."),
        Some(s) if s["requested"] == true && s["skipped"].as_array().is_some_and(|k| !k.is_empty()) => {
            format!("No recorded {what} fit the bundle's size cap ({} clips left out).", s["skipped"].as_array().map_or(0, Vec::len))
        }
        Some(s) if s["requested"] == true => format!("No recorded {what} overlaps this recording."),
        _ => format!("This bundle has no recorded {what}. Download it with “{toggle}” ticked to include it."),
    }
}

#[component]
pub(crate) fn RecordedMedia() -> Element {
    let station: Option<Rc<LocalStation>> = use_hook(super::station_link::local);
    let urls = use_hook(|| Rc::new(Urls::default()));
    {
        let urls = urls.clone();
        use_drop(move || urls.clear());
    }
    let mut position = use_signal(|| None::<super::local_station::station::Playback>);
    let mut ticks = use_signal(|| 0u64);
    {
        let station = station.clone();
        let urls = urls.clone();
        use_future(move || {
            let station = station.clone();
            let urls = urls.clone();
            async move {
                // Remote station: nothing to follow.
                let Some(station) = station else { return };
                let Some(media) = station.session.media.clone() else { return };
                loop {
                    let now = super::current_wallclock_ms();
                    let playback = station.playback(now);
                    let (cameras, voice) = playing(&media, playback.position);
                    let targets: Vec<(String, media::Sync)> =
                        cameras.iter().filter_map(|(_, c)| *c).chain(voice).map(|c| (element_id(c), media::sync(c, &playback))).collect();
                    if position.peek().as_ref() != Some(&playback) {
                        position.set(Some(playback));
                    }
                    if !targets.is_empty() {
                        document::eval(&sync_script(&targets));
                    }
                    let tick = *ticks.peek() + 1;
                    ticks.set(tick);
                    urls.sweep(tick);
                    wait_tick().await;
                }
            }
        });
    }
    let Some(station) = station else { return rsx! {} };
    let media = station.session.media.clone();
    if media.is_none() {
        return rsx! {
            RecordedMediaShell {
                h4 { "Recorded cameras" }
                p { class: "gs26-recorded-empty", "{absent(Kind::Video, None)}" }
                h4 { "Recorded voice" }
                p { class: "gs26-recorded-empty", "{absent(Kind::Voice, None)}" }
            }
        };
    }
    let Some(playback) = position() else {
        return rsx! { RecordedMediaShell { p { "Loading recorded media…" } } };
    };
    let tick = *ticks.peek();
    let at = playback.position;
    let (cameras, voice) = media.as_deref().map(|m| playing(m, at)).unwrap_or_default();
    rsx! {
        RecordedMediaShell {
            h4 { "Recorded cameras" }
            if cameras.is_empty() {
                p { class: "gs26-recorded-empty", "{absent(Kind::Video, media.as_deref())}" }
            }
            div { class: "gs26-recorded-cameras",
                for (stream, clip) in cameras {
                    figure { key: "{stream}", class: "gs26-recorded-camera", "data-stream": "{stream}",
                        if let Some(clip) = clip.filter(|c| urls.url(c, tick).is_some()) {
                            video { key: "{clip.path}", id: "{element_id(clip)}", src: urls.url(clip, tick).unwrap_or_default(),
                                preload: "auto", playsinline: true, muted: playback.speed > 1.0, "aria-label": "Recorded camera {stream}" }
                            figcaption { "{stream} · segment from {utc(clip.start_ms)}" }
                        } else {
                            div { class: "gs26-recorded-gap", "No recording at this moment" }
                            figcaption {
                                "{stream}"
                                if let Some(next) = media.as_deref().and_then(|m| m.next(Kind::Video, Some(&stream), at)) { " · next segment at {utc(next.start_ms)}" }
                            }
                        }
                    }
                }
            }
            h4 { "Recorded voice" }
            if media.as_deref().is_none_or(|m| m.streams(Kind::Voice).is_empty()) {
                p { class: "gs26-recorded-empty", "{absent(Kind::Voice, media.as_deref())}" }
            } else if voice.is_empty() {
                p { class: "gs26-recorded-empty",
                    "Nobody is transmitting at this moment."
                    if let Some(next) = media.as_deref().and_then(|m| m.next(Kind::Voice, None, at)) { " Next transmission at {utc(next.start_ms)} ({next.label()})." }
                }
            }
            ul { class: "gs26-recorded-voice",
                for clip in voice {
                    if let Some(src) = urls.url(clip, tick) {
                        li { key: "{clip.path}",
                            audio { id: "{element_id(clip)}", src, preload: "auto" }
                            span { class: "gs-badge gs-badge--info", "On air" }
                            " {clip.label()} · from {utc(clip.start_ms)}"
                        }
                    }
                }
            }
        }
    }
}

#[component]
fn RecordedMediaShell(children: Element) -> Element {
    rsx! {
        style { {r#"
            .gs26-recorded-media{display:flex;flex-direction:column;gap:8px;padding:10px 0;min-width:0;}
            .gs26-recorded-media h4{margin:6px 0 0;}
            .gs26-recorded-cameras{display:grid;grid-template-columns:repeat(auto-fill,minmax(min(100%,320px),1fr));gap:10px;}
            .gs26-recorded-camera{margin:0;display:flex;flex-direction:column;gap:4px;min-width:0;}
            .gs26-recorded-camera video,.gs26-recorded-gap{width:100%;aspect-ratio:16/9;background:#080d15;border-radius:8px;}
            .gs26-recorded-gap{display:flex;align-items:center;justify-content:center;color:#a9b7c4;font-size:14px;}
            .gs26-recorded-camera figcaption,.gs26-recorded-empty{font-size:13px;color:#a9b7c4;margin:0;overflow-wrap:anywhere;}
            .gs26-recorded-voice{list-style:none;margin:0;padding:0;display:flex;flex-direction:column;gap:4px;}
            .gs26-recorded-voice audio{display:none;}
        "#} }
        section { class: "gs26-recorded-media", "aria-label": "Recorded media of this replay", "data-testid": "recorded-media", {children} }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn element_ids_are_safe_and_distinct() {
        let clip = |path: &str| Clip { kind: Kind::Voice, stream: "crew".into(), file: "f".into(), path: path.into(), start_ms: 0, end_ms: 1, media_type: "audio/wav".into(), body: Default::default() };
        assert_eq!(element_id(&clip("media/voice/crew/1-7.wav")), "gs26-rec-media-voice-crew-1-7-wav");
        assert_ne!(element_id(&clip("media/video/a/1.mp4")), element_id(&clip("media/video/b/1.mp4")));
    }

    #[test]
    fn the_sync_script_carries_every_target() {
        let s = media::Sync { offset_s: 1.5, rate: 2.0, play: true, tolerance_s: 0.5, muted: true };
        let script = sync_script(&[("gs26-rec-a".into(), s)]);
        assert!(script.contains(r#""id":"gs26-rec-a""#) && script.contains(r#""at":1.5"#) && script.contains(r#""rate":2.0"#));
        assert!(!script.contains("fetch") && !script.contains("http"), "the script only drives elements");
    }

    #[test]
    fn absent_media_says_why() {
        assert!(absent(Kind::Video, None).contains("“Camera video” ticked"));
        let media = Media { clips: Vec::new(), video: serde_json::json!({"requested": true, "recorded": true, "skipped": [{}]}), voice: serde_json::json!({"requested": true, "recorded": false}) };
        assert!(absent(Kind::Video, Some(&media)).contains("size cap (1 clips"));
        assert!(absent(Kind::Voice, Some(&media)).contains("does not record crew voice"));
    }

    #[test]
    fn utc_labels() {
        assert_eq!(utc(1_767_225_630_000), "00:00:30 UTC");
    }
}
