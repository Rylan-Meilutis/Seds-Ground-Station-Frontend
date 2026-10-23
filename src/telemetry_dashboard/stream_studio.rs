use super::{http_get_json, http_post_json, live_stream_tab::BroadcastState};
use crate::auth;
use dioxus::prelude::*;
use serde::{Deserialize, Serialize};
#[derive(Clone, Deserialize)]
struct Account {
    username: String,
    roles: Vec<String>,
    disabled: bool,
}
#[derive(Serialize)]
struct Grant {
    username: String,
    stream_master: bool,
}
#[component]
pub(super) fn StreamStudio(broadcast: BroadcastState, program_url: String) -> Element {
    let mut delay = use_signal(|| broadcast.delay_seconds);
    let mut comms_audio = use_signal(|| broadcast.comms_audio_enabled);
    let mut label = use_signal(|| broadcast.label.clone());
    let mut message = use_signal(String::new);
    let mut busy = use_signal(|| false);
    let mut accounts = use_signal(Vec::<Account>::new);
    let admin = auth::current_status()
        .roles
        .iter()
        .any(|r| r == "stream_admin" || r == "admin");
    use_future(move || async move {
        if admin {
            match http_get_json::<Vec<Account>>("/api/stream-roles").await {
                Ok(v) => accounts.set(v),
                Err(e) => message.set(e),
            }
        }
    });
    rsx! {section {class:"gs26-stream-studio",style:"width:100%;padding:16px;box-sizing:border-box;border:1px solid #344454;border-radius:12px;background:#101923;color:#e4ebf0;",
      h3 {style:"margin:0 0 12px;","Broadcast studio"}
      p {style:"font-size:12px;color:#a9b7c4;","The broadcast below is the same buffered program for everyone. Camera changes, telemetry and crew comms use its delayed timeline."}
      div {style:"display:flex;align-items:center;gap:12px;flex-wrap:wrap;",
       label {"Mission label " input {value:"{label}",disabled:*busy.read(),oninput:move|e|label.set(e.value())}}
       label {"Audience delay (3–60 seconds) " input {r#type:"number",min:"3",max:"60",value:"{delay}",disabled:*busy.read(),oninput:move|e|{if let Ok(v)=e.value().parse(){delay.set(v);}}}}

      }
      fieldset {class:"gs26-audience-audio",
       legend {"Audience audio"}
       label { input {r#type:"checkbox",checked:*comms_audio.read(),disabled:*busy.read(),"aria-describedby":"audience-audio-help",onchange:move|e|comms_audio.set(e.checked())} " Include crew voice (VC)" }
       p {id:"audience-audio-help","Voice follows the audience delay. Viewers can listen without joining the call."}
      }
      div {
       button {disabled:*busy.read()||!(3..=60).contains(&*delay.read()),onclick:move |_|{
         let mut next=broadcast.clone();next.delay_seconds = *delay.read();next.label=label.read().clone();next.comms_audio_enabled = *comms_audio.read();busy.set(true);
         spawn(async move{match http_post_json::<BroadcastState,BroadcastState>("/api/live_streams/control",&next).await{Ok(_)=>message.set("Broadcast settings saved. Crew audio follows the audience delay.".into()),Err(e)=>message.set(e)}busy.set(false);});
       },"Apply broadcast settings"}
      }
      if admin {details {summary {"Stream manager roles"}
       p {"Assigning stream master grants broadcast control only, not valve or rocket commands. Stream administrators retain management access."}
       for account in accounts.read().iter() {
         {let username=account.username.clone();let enabled=account.roles.iter().any(|r|r=="stream_master");let administrator=account.roles.iter().any(|r|r=="stream_admin");
          rsx!{label {style:"display:block;padding:8px;",
           input {r#type:"checkbox",checked:enabled,disabled:account.disabled||administrator||*busy.read(),onchange:move|e|{
             let grant=Grant{username:username.clone(),stream_master:e.checked()};busy.set(true);
             spawn(async move{match http_post_json::<Grant,serde_json::Value>("/api/stream-roles",&grant).await{Ok(_)=>{message.set("Stream role saved".into());if let Ok(v)=http_get_json::<Vec<Account>>("/api/stream-roles").await{accounts.set(v);}},Err(e)=>message.set(e)}busy.set(false);});
           }} "{account.username}" if administrator {" — administrator"}
          }}
         }
       }
      }}
      p {role:"status",style:"font-size:12px;","{message}"}
    }}
}

fn freshest_broadcast(current: &BroadcastState, saved: &Option<BroadcastState>) -> BroadcastState {
    saved.as_ref().filter(|s|s.revision>current.revision).unwrap_or(current).clone()
}
async fn save_replay(mut request: BroadcastState, plan: Option<super::live_stream_tab::ReplayPlan>, mut busy: Signal<bool>, mut message: Signal<String>, mut saved: Signal<Option<BroadcastState>>, mut draft: Signal<super::live_stream_tab::ReplayPlan>) {
    request.replay=plan;busy.set(true);message.set("Preparing buffered cameras…".into());
    match http_post_json::<BroadcastState,BroadcastState>("/api/live_streams/control",&request).await {
        Ok(value)=>{if let Some(plan)=&value.replay{draft.set(plan.clone());}message.set(if value.replay.is_some(){"Replay on air"}else{"Live program restored"}.into());saved.set(Some(value));},
        Err(e)=>message.set(e),
    }
    busy.set(false);
}
#[component]
pub(super) fn ReplayControls(broadcast: BroadcastState, streams: Vec<super::live_stream_tab::LiveStreamSpec>) -> Element {
    use super::live_stream_tab::{ReplayPlan,ReplayCamera};
    let mut draft=use_signal(||broadcast.replay.clone().unwrap_or(ReplayPlan{id:String::new(),duration_seconds:10,speed:0.5,cameras:streams.first().map(|s|vec![ReplayCamera{id:s.id.clone(),x:50.,y:50.,zoom:1.}]).unwrap_or_default(),started_at_ms:0,source_end_ms:0}));
    let busy=use_signal(||false);let message=use_signal(String::new);let saved=use_signal(||None::<BroadcastState>);
    let current=freshest_broadcast(&broadcast,&saved.read());
    let can_apply=current.replay.is_some();
    rsx! {
        section {class:"gs26-replay-controls",style:"padding:16px;border:1px solid #344454;border-radius:12px;display:grid;gap:12px;",
            h3 {style:"margin:0;","Instant replay"}
            div {style:"display:flex;gap:12px;flex-wrap:wrap;align-items:end;",
                label {"Last seconds" input {r#type:"number",min:"1",max:"20",value:"{draft.read().duration_seconds}",disabled:*busy.read(),oninput:move|e|if let Ok(v)=e.value().parse(){draft.write().duration_seconds=v;}}}
                label {"Playback" select {"aria-label":"Playback",value:"{draft.read().speed}",disabled:*busy.read(),onchange:move|e|if let Ok(v)=e.value().parse(){draft.write().speed=v;},
                    option {value:"1","Normal · 1×"} option {value:"0.5","Slow · ½×"} option {value:"0.25","Slow · ¼×"}
                }}
            }
            div {style:"display:flex;gap:12px;flex-wrap:wrap;",
                for stream in &streams {
                    label {style:"display:flex;flex-direction:row;gap:8px;align-items:center;min-height:44px;",
                        input {r#type:"checkbox",checked:draft.read().cameras.iter().any(|c|c.id==stream.id),disabled:*busy.read(),onchange:{let id=stream.id.clone();move|e:Event<FormData>|{let mut p=draft.write();if e.checked(){if p.cameras.len()<8 && !p.cameras.iter().any(|c|c.id==id){p.cameras.push(ReplayCamera{id:id.clone(),x:50.,y:50.,zoom:1.});}}else{p.cameras.retain(|c|c.id!=id);}}}}
                        "{stream.label}"
                    }
                }
            }
            for (i,camera) in draft.read().cameras.clone().into_iter().enumerate() {
                div {style:"display:flex;gap:10px;flex-wrap:wrap;align-items:center;padding:8px;border:1px solid #344454;border-radius:10px;",
                    strong {"{camera.id}"}
                    button {r#type:"button",disabled:*busy.read()||i==0,onclick:move |_|draft.write().cameras.swap(i,i-1),"Move earlier"}
                    button {r#type:"button",disabled:*busy.read()||i+1>=draft.read().cameras.len(),onclick:move |_|draft.write().cameras.swap(i,i+1),"Move later"}
                    label {"Zoom" input {r#type:"range",min:"1",max:"3",step:"0.05",value:"{camera.zoom}",disabled:*busy.read(),oninput:move|e|if let Ok(v)=e.value().parse(){draft.write().cameras[i].zoom=v;}}}
                    label {"Horizontal" input {r#type:"range",min:"0",max:"100",value:"{camera.x}",disabled:*busy.read(),oninput:move|e|if let Ok(v)=e.value().parse(){draft.write().cameras[i].x=v;}}}
                    label {"Vertical" input {r#type:"range",min:"0",max:"100",value:"{camera.y}",disabled:*busy.read(),oninput:move|e|if let Ok(v)=e.value().parse(){draft.write().cameras[i].y=v;}}}
                }
            }
            div {style:"display:flex;gap:10px;flex-wrap:wrap;",
                button {r#type:"button",disabled:*busy.read()||draft.read().cameras.is_empty()||!(1..=20).contains(&draft.read().duration_seconds),onclick:{let current=current.clone();move |_|{let mut p=draft.read().clone();p.id.clear();p.started_at_ms=0;p.source_end_ms=0;spawn(save_replay(current.clone(),Some(p),busy,message,saved,draft));}},"Play replay"}
                button {r#type:"button",disabled:*busy.read()||!can_apply,onclick:{let current=current.clone();move |_|{if let Some(mut p)=current.replay.clone(){p.cameras=draft.read().cameras.clone();spawn(save_replay(current.clone(),Some(p),busy,message,saved,draft));}}},"Apply replay framing"}
                button {r#type:"button",disabled:*busy.read()||!can_apply,onclick:{let current=current.clone();move |_|{spawn(save_replay(current.clone(),None,busy,message,saved,draft));}},"Return to live"}
            }
            p {role:"status",style:"margin:0;","{message}"}
            details {summary {"Replay help"} p {"Select up to eight cameras, including cameras hidden from the live program. Replay uses the last 1–20 seconds of the audience timeline and returns to live automatically. Frame and order changes apply only to the replay. Start a new replay to add a camera or change its duration or speed. Crew voice is muted during replay."}}
        }
    }
}

#[cfg(test)]
mod replay_tests {
    use super::*;
    #[test]
    fn stale_control_response_cannot_replace_newer_broadcast() {
        let current=BroadcastState{revision:4,..Default::default()};
        assert_eq!(freshest_broadcast(&current,&Some(BroadcastState{revision:3,..Default::default()})).revision,4);
        assert_eq!(freshest_broadcast(&current,&Some(BroadcastState{revision:5,..Default::default()})).revision,5);
    }
}
