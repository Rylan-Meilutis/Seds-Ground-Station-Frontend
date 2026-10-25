//! Programme control for a selected saved recording, separate from camera clips.
use super::{
    http_get_json, http_post_json,
    live_stream_tab::{BroadcastState, LiveStreamConfig, RecordedReplayPlan},
    vehicle_replay::{Recording, Snapshot},
};
use dioxus::prelude::*;
use serde::Deserialize;

#[derive(Clone, Deserialize)]
struct SceneClock {
    plan: RecordedReplayPlan,
    replay_position_ms: u64,
}
#[derive(Clone)]
enum Action {
    Select(String),
    Stop,
    TogglePause,
    Rewind,
    Speed(f64),
    Loop(bool),
    FollowSaved(bool),
    Seek(u64),
    Refresh,
}
fn same_selection(a: &RecordedReplayPlan, b: &RecordedReplayPlan) -> bool {
    a.recording_id == b.recording_id
        && a.upper_id == b.upper_id
        && a.start_ms == b.start_ms
        && a.end_ms == b.end_ms
}
fn same_or_grown_selection(a: &RecordedReplayPlan, b: &RecordedReplayPlan) -> bool {
    same_selection(a, b)
        || (a.follow_saved
            && a.recording_id == b.recording_id
            && a.start_ms == b.start_ms
            && b.upper_id >= a.upper_id
            && b.end_ms >= a.end_ms)
}
fn encoded(value: &str) -> String {
    const HEX: &[u8] = b"0123456789ABCDEF";
    let mut out = String::new();
    for b in value.bytes() {
        if b.is_ascii_alphanumeric() || b"-._~".contains(&b) {
            out.push(b as char);
        } else {
            out.push('%');
            out.push(HEX[(b >> 4) as usize] as char);
            out.push(HEX[(b & 15) as usize] as char);
        }
    }
    out
}
async fn update(expected: BroadcastState, action: Action) -> Result<BroadcastState, String> {
    let current = http_get_json::<LiveStreamConfig>("/api/live_streams")
        .await?
        .broadcast;
    if current.revision != expected.revision {
        return Err("Broadcast changed. Review the current controls before trying again.".into());
    }
    let mut next = current.clone();
    match action {
        Action::Select(_) | Action::Refresh => {
            let refresh = matches!(&action, Action::Refresh);
            let prior = if refresh {
                Some(http_get_json::<SceneClock>("/api/media-assets/recorded-vehicle/state").await?)
            } else {
                None
            };
            let id = match action {
                Action::Select(id) => id,
                _ => current
                    .recorded_replay
                    .as_ref()
                    .map(|p| p.recording_id.clone())
                    .ok_or("Select a recording first")?,
            };
            let saved = http_get_json::<Snapshot>(&format!(
                "/api/recordings/{}/replay/flight",
                encoded(&id)
            ))
            .await?;
            let (Some(start), Some(end)) = (saved.start_ms, saved.end_ms) else {
                return Err("This recording has no saved flight samples yet".into());
            };
            if start < 0 || end < start {
                return Err("This recording has invalid time bounds".into());
            }
            let old = prior
                .as_ref()
                .map(|c| &c.plan)
                .filter(|p| p.recording_id == id);
            if refresh
                && !current
                    .recorded_replay
                    .as_ref()
                    .zip(old)
                    .is_some_and(|(a, b)| same_or_grown_selection(a, b))
            {
                return Err("The selected recording changed. Review the current replay.".into());
            }
            let plan = RecordedReplayPlan {
                recording_id: id,
                upper_id: saved.upper_id,
                start_ms: start,
                end_ms: end,
                position_ms: prior
                    .as_ref()
                    .map(|c| {
                        c.plan
                            .start_ms
                            .saturating_add(c.replay_position_ms as i64)
                            .saturating_sub(start)
                            .max(0) as u64
                    })
                    .unwrap_or(0)
                    .min((end - start) as u64),
                anchor_server_ms: 0,
                speed: old.map(|p| p.speed).unwrap_or(1.),
                paused: old.map(|p| p.paused).unwrap_or(false),
                loop_playback: old.map(|p| p.loop_playback).unwrap_or(true),
                follow_saved: old.is_some_and(|p| p.follow_saved),
            };
            next.recorded_replay = Some(plan);
            next.replay = None;
            next.layout = "recorded-replay".into();
        }
        Action::Stop => {
            next.recorded_replay = None;
            if next.layout == "recorded-replay" {
                next.layout = "hero".into();
            }
        }
        _ => {
            let clock =
                http_get_json::<SceneClock>("/api/media-assets/recorded-vehicle/state").await?;
            let plan = next
                .recorded_replay
                .as_mut()
                .ok_or("Select a recording first")?;
            if !same_or_grown_selection(plan, &clock.plan) {
                return Err("The selected recording changed. Review the current replay.".into());
            }
            *plan = clock.plan.clone();
            plan.position_ms = clock.replay_position_ms;
            plan.anchor_server_ms = 0;
            match action {
                Action::TogglePause => plan.paused = !plan.paused,
                Action::Rewind => {
                    plan.position_ms = 0;
                    plan.paused = true;
                }
                Action::Speed(value) => plan.speed = value.clamp(0.25, 4.),
                Action::Loop(value) => plan.loop_playback = value,
                Action::FollowSaved(value) => plan.follow_saved = value,
                Action::Seek(value) => {
                    plan.position_ms = value.min(plan.end_ms.saturating_sub(plan.start_ms) as u64);
                    plan.paused = true;
                }
                _ => {}
            }
        }
    }
    http_post_json::<BroadcastState, BroadcastState>(
        "/api/media-assets/recorded-vehicle/control",
        &next,
    )
    .await
}
fn dispatch(
    action: Action,
    mut base: Signal<BroadcastState>,
    mut busy: Signal<bool>,
    mut message: Signal<String>,
) {
    if *busy.read() {
        return;
    }
    busy.set(true);
    let expected = base.read().clone();
    spawn(async move {
        match update(expected, action).await {
            Ok(value) => {
                message.set(
                    if value.recorded_replay.is_some() {
                        "Recorded flight replay saved"
                    } else {
                        "Recorded flight replay stopped"
                    }
                    .into(),
                );
                base.set(value);
            }
            Err(error) => {
                message.set(error);
                if let Ok(config) = http_get_json::<LiveStreamConfig>("/api/live_streams").await {
                    base.set(config.broadcast);
                }
            }
        }
        busy.set(false);
    });
}
#[component]
pub(super) fn RecordedReplayControls(broadcast: BroadcastState) -> Element {
    let mut selected = use_signal(|| {
        broadcast
            .recorded_replay
            .as_ref()
            .map(|p| p.recording_id.clone())
            .unwrap_or_default()
    });
    let mut base = use_signal(|| broadcast.clone());
    let busy = use_signal(|| false);
    let message = use_signal(String::new);
    let mut seek = use_signal(|| None::<u64>);
    let recordings =
        use_resource(|| async { http_get_json::<Vec<Recording>>("/api/recordings").await });
    use_effect(move || {
        if broadcast.revision >= base.read().revision && broadcast != *base.read() {
            base.set(broadcast.clone());
        }
    });
    let mut clock = use_resource(move || {
        let key = base.read().recorded_replay.clone();
        async move {
            if key.is_none() {
                return Ok(None);
            }
            http_get_json::<SceneClock>("/api/media-assets/recorded-vehicle/state")
                .await
                .map(Some)
        }
    });
    use_future(move || async move {
        loop {
            #[cfg(target_arch = "wasm32")]
            gloo_timers::future::TimeoutFuture::new(1000).await;
            #[cfg(not(target_arch = "wasm32"))]
            tokio::time::sleep(std::time::Duration::from_millis(1000)).await;
            if base.read().recorded_replay.is_some() {
                clock.restart();
            }
        }
    });
    let current = base.read().clone();
    let plan = current.recorded_replay.clone();
    let position = clock
        .read()
        .as_ref()
        .and_then(|v| v.as_ref().ok())
        .and_then(|v| v.as_ref())
        .filter(|v| {
            plan.as_ref()
                .is_some_and(|p| same_or_grown_selection(p, &v.plan))
        })
        .map(|v| v.replay_position_ms)
        .unwrap_or_else(|| plan.as_ref().map(|p| p.position_ms).unwrap_or(0));
    let options = recordings
        .read()
        .as_ref()
        .and_then(|v| v.as_ref().ok())
        .cloned()
        .unwrap_or_default();
    rsx! {section {class:"gs26-recorded-replay-controls",style:"padding:16px;border:1px solid #344454;border-radius:12px;display:grid;gap:12px;",
        h3 {style:"margin:0;","Recorded flight replay"}
        div {style:"display:flex;align-items:end;gap:10px;flex-wrap:wrap;",
            label {style:"display:grid;gap:6px;max-width:28rem;min-width:0;","Recording"
                select {"aria-label":"Flight replay recording",value:"{selected}",disabled:*busy.read(),onchange:move|e|selected.set(e.value()),
                    option {value:"","Select saved flight…"}
                    for recording in options {option {value:"{recording.id}","{recording.id}",if recording.active{" · recording"}}}
                }
            }
            button {class:"gs26-button",disabled:*busy.read()||selected.read().is_empty(),onclick:move|_|dispatch(Action::Select(selected.read().clone()),base,busy,message),"Show 3D replay"}
        }
        if let Some(p)=plan {
            div {style:"display:flex;gap:8px;flex-wrap:wrap;align-items:center;",
                button {class:"gs26-button",disabled:*busy.read(),onclick:move|_|dispatch(Action::TogglePause,base,busy,message),if p.paused{"Play recorded flight"}else{"Pause recorded flight"}}
                button {class:"gs26-button",disabled:*busy.read(),onclick:move|_|dispatch(Action::Rewind,base,busy,message),"Rewind flight"}
                label {"Replay speed" select {"aria-label":"Recorded flight speed",value:"{p.speed}",disabled:*busy.read(),onchange:move|e|if let Ok(v)=e.value().parse(){dispatch(Action::Speed(v),base,busy,message);},
                    option {value:"0.25","¼×"}option {value:"0.5","½×"}option {value:"1","1×"}option {value:"2","2×"}option {value:"4","4×"}
                }}
                label {style:"display:flex;gap:7px;align-items:center;min-height:44px;",input {r#type:"checkbox",checked:p.loop_playback,disabled:*busy.read(),onchange:move|e|dispatch(Action::Loop(e.checked()),base,busy,message)},"Loop recorded flight"}
                label {style:"display:flex;gap:7px;align-items:center;min-height:44px;",input {r#type:"checkbox",checked:p.follow_saved,disabled:*busy.read(),onchange:move|e|dispatch(Action::FollowSaved(e.checked()),base,busy,message)},"Follow newly saved data"}
                button {class:"gs26-button",disabled:*busy.read(),onclick:move|_|dispatch(Action::Refresh,base,busy,message),"Refresh saved snapshot"}
                button {class:"gs26-button",disabled:*busy.read(),onclick:move|_|dispatch(Action::Stop,base,busy,message),"Stop flight replay"}
            }
            label {style:"display:grid;gap:6px;","Saved replay position"
                input {r#type:"range","aria-label":"Broadcast replay position",min:"0",max:"{p.end_ms.saturating_sub(p.start_ms)}",step:"100",value:"{seek.read().unwrap_or(position)}",disabled:*busy.read(),style:"min-height:44px;width:100%;",oninput:move|e|if let Ok(v)=e.value().parse(){seek.set(Some(v));},onchange:move|e|{if let Ok(v)=e.value().parse(){dispatch(Action::Seek(v),base,busy,message);}seek.set(None);}}
            }
            span {style:"font-variant-numeric:tabular-nums;font-size:13px;",{format!("Recorded {:.1} / {:.1} s",position as f64/1000.,p.end_ms.saturating_sub(p.start_ms) as f64/1000.)}}
        }
        if !message.read().is_empty(){p {role:"status",style:"margin:0;","{message}"}}
        details {summary {style:"min-height:44px;display:flex;align-items:center;cursor:pointer;","About flight replay"}
            p {style:"max-width:40rem;","Uses saved samples only. Active recordings are frozen at selection. Refresh manually or enable Follow newly saved data for bounded updates from this saved flight. Camera clips and live recovery positions remain separate."}
        }
    }}
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn selectionidentityincludesrecordingandsnapshot() {
        let a = RecordedReplayPlan {
            recording_id: "one.db".into(),
            upper_id: 10,
            start_ms: 0,
            end_ms: 1000,
            position_ms: 0,
            anchor_server_ms: 0,
            speed: 1.,
            paused: true,
            loop_playback: false,
            follow_saved: false,
        };
        let mut b = a.clone();
        b.recording_id = "two.db".into();
        assert!(!same_selection(&a, &b));
        b = a.clone();
        b.upper_id = 11;
        assert!(!same_selection(&a, &b));
        assert!(!same_or_grown_selection(&a, &b));
        let mut following = a.clone();
        following.follow_saved = true;
        b.end_ms = 2000;
        assert!(same_or_grown_selection(&following, &b));
        b.recording_id = "another.db".into();
        assert!(!same_or_grown_selection(&following, &b));
        assert_eq!(encoded("file name/#"), "file%20name%2F%23");
    }
}
