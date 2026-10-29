use super::{http_get_json, http_post_json, live_stream_tab::BroadcastState};
use crate::auth;
use dioxus::prelude::*;
use serde::{Deserialize, Serialize};

fn camera_tiles(ids:&[String],template:&str,split:f64)->Vec<super::live_stream_tab::CameraTile>{
    let ids=&ids[..ids.len().min(8)];let n=ids.len();
    ids.iter().enumerate().filter_map(|(i,id)|{
        if template=="featured"&&i>0{return None;}
        let (x,y,width,height)=if template=="featured"||n==1{(0.,0.,100.,100.)}
        else if template=="main-side"{if i==0{(0.,0.,split,100.)}else{(split,(i-1)as f64*100./(n-1)as f64,100.-split,100./(n-1)as f64)}}
        else{let cols=if n<=4{2}else{3};let rows=n.div_ceil(cols);((i%cols)as f64*100./cols as f64,(i/cols)as f64*100./rows as f64,100./cols as f64,100./rows as f64)};
        Some(super::live_stream_tab::CameraTile{stream_id:id.clone(),x,y,width,height})
    }).collect()
}

#[derive(Clone,Default,Deserialize)]
struct TranscodeHost {id:String,name:String,passed:bool,online:bool,capacity:usize,encoding_ms:u64,upload_bps:u64,download_bps:u64}
#[derive(Clone,Default,Deserialize)]
struct TranscodeHosts {selected:Option<String>,hosts:Vec<TranscodeHost>}
#[component]
fn TranscodingHosts()->Element{
    let mut data=use_signal(TranscodeHosts::default);let mut message=use_signal(String::new);let mut busy=use_signal(||false);let mut available=use_signal(||false);
    use_future(move||async move{loop{match http_get_json::<TranscodeHosts>("/api/video/transcode/hosts").await{Ok(value)=>{data.set(value);available.set(true);},Err(_)=>{available.set(false);}}
        #[cfg(target_arch="wasm32")]gloo_timers::future::TimeoutFuture::new(3000).await;
        #[cfg(not(target_arch="wasm32"))]tokio::time::sleep(std::time::Duration::from_secs(3)).await;
    }});
    rsx!{section {"aria-label":"Transcoding hosts",h3 {"Transcoding host"}
        p {"Run the GroundStation Video Transcoder helper on another computer. It must pass encoding and upload/download tests before it can be selected."}
        label {"Use host" select {value:data().selected.unwrap_or_default(),disabled:busy()||!available(),onchange:move|e|{
            let id=e.value();busy.set(true);spawn(async move{let body=serde_json::json!({"host_id":if id.is_empty(){None}else{Some(id)}});match http_post_json::<serde_json::Value,serde_json::Value>("/api/video/transcode/hosts",&body).await{Ok(_)=>{message.set("Transcoding selection saved".into());if let Ok(next)=http_get_json::<TranscodeHosts>("/api/video/transcode/hosts").await{data.set(next);}},Err(error)=>message.set(error)}busy.set(false);});
        },option {value:"","Original camera feeds (no remote transcoding)"}
         for host in &data().hosts{option {value:host.id.clone(),disabled:!host.passed||!host.online,"{host.name} · {host.capacity} camera(s)"}}
        }}
        for host in &data().hosts{p {"{host.name}: " if host.passed {"qualified"}else{"test required"} if !host.online {" · offline / not offered"} " · encode {host.encoding_ms} ms · ↑{host.upload_bps / 1_000_000} / ↓{host.download_bps / 1_000_000} Mb/s"}}
        if data().selected.is_some()&&!data().hosts.iter().any(|h|Some(&h.id)==data().selected.as_ref()&&h.online){p {role:"status","Selected host is unavailable. Using original feeds."}}
        p {role:"status","{message}"}
    }}
}

#[component]
fn LayoutDesigner(broadcast:BroadcastState,streams:Vec<super::live_stream_tab::LiveStreamSpec>)->Element{
    let mut order=use_signal(||{let mut ids:Vec<_>=broadcast.tiles.iter().map(|t|t.stream_id.clone()).collect();for s in &streams{if !ids.contains(&s.id){ids.push(s.id.clone());}}ids.truncate(8);ids});
    let mut template=use_signal(||"grid".to_owned());let mut split=use_signal(||65.0);
    let mut base=use_signal(||broadcast.clone());let mut busy=use_signal(||false);let mut message=use_signal(String::new);
    let mut preview=use_signal(||true);let tiles=camera_tiles(&order(),&template(),split());
    rsx!{section {class:"gs26-layout-designer","aria-label":"Camera layout designer",
        h3 {"Live camera layout"}
        p {"Arrange the tiles, choose a layout and resize the main camera. Save to fade the audience programme into this layout, including camera replays."}
        label {"Layout" select {"aria-label":"Camera layout",value:template(),onchange:move|e|template.set(e.value()),option {value:"grid","Grid"}option {value:"main-side","Main camera and side tiles"}option {value:"featured","Featured camera"}}}
        if template()=="main-side"{label {"Main camera width" input {"aria-label":"Main camera width",r#type:"range",min:"35",max:"75",step:"1",value:split().to_string(),oninput:move|e|{if let Ok(v)=e.value().parse(){split.set(v);}}}}}
        label {input {r#type:"checkbox",checked:preview(),onchange:move|e|preview.set(e.checked())}" Live previews"}
        div {style:"position:relative;aspect-ratio:16/9;background:#050b12;min-height:200px;",
            for (index,tile) in tiles.iter().enumerate(){
                {let feed=streams.iter().find(|s|s.id==tile.stream_id);let label=feed.map(|s|s.label.clone()).unwrap_or_else(||tile.stream_id.clone());let source=feed.map(|s|s.url.clone()).unwrap_or_default();
                rsx!{article {key:"{tile.stream_id}",style:format!("position:absolute;box-sizing:border-box;border:2px solid #65778c;overflow:hidden;left:{}%;top:{}%;width:{}%;height:{}%;",tile.x,tile.y,tile.width,tile.height),
                    if preview()&&!source.is_empty(){iframe {src:super::live_stream_tab::media_url(&source),title:"{label} preview",allow:"autoplay",style:"position:absolute;inset:0;width:100%;height:100%;border:0;pointer-events:none;"}}
                    div {style:"position:absolute;left:0;right:0;bottom:0;padding:4px;display:flex;gap:4px;align-items:center;background:#07111ee6;",
                        span {style:"overflow:hidden;text-overflow:ellipsis;white-space:nowrap;flex:1;","{label}"}
                        button {r#type:"button",disabled:busy()||index==0,"aria-label":format!("Move {} earlier",label),onclick:move|_|{order.write().swap(index,index-1);},"←"}
                        button {r#type:"button",disabled:busy()||index+1>=order().len(),"aria-label":format!("Move {} later",label),onclick:move|_|{order.write().swap(index,index+1);},"→"}
                    }
                }}}
            }
        }
        button {disabled:busy()||tiles.is_empty(),onclick:move|_|{
            let mut next=base();next.layout="tiles".into();next.tiles=camera_tiles(&order(),&template(),split());next.hidden_stream_ids.retain(|id|!next.tiles.iter().any(|t|&t.stream_id==id));busy.set(true);
            spawn(async move{match http_post_json::<BroadcastState,BroadcastState>("/api/live_streams/control",&next).await{Ok(saved)=>{base.set(saved);message.set("Layout saved".into());},Err(e)=>message.set(e)}busy.set(false);});
        },"Save camera layout"}
        button {disabled:busy(),onclick:move|_|{base.set(broadcast.clone());order.set(streams.iter().map(|s|s.id.clone()).take(8).collect());message.set("Current camera list loaded".into());},"Reload cameras"}
        p {role:"status","{message}"}
    }}
}
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
pub(super) fn StreamStudio(
    broadcast: BroadcastState,
    program_url: String,
    streams: Vec<super::live_stream_tab::LiveStreamSpec>,
) -> Element {
    let delay = use_signal(|| broadcast.delay_seconds);
    let mut comms_audio = use_signal(|| broadcast.comms_audio_enabled);
    let mut commentary_audio = use_signal(|| broadcast.commentary_audio_enabled);
    let mut audio_desk = use_signal(|| false);
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
      style {{include_str!("stream_studio.css")}}
      h3 {style:"margin:0 0 12px;","Broadcast studio"}

      div {style:"display:flex;align-items:center;gap:12px;flex-wrap:wrap;",
       label {"Mission label " input {value:"{label}",disabled:*busy.read(),oninput:move|e|label.set(e.value())}}

      }
      fieldset {class:"gs26-audience-audio",
       legend {"Audience audio"}
       label { input {r#type:"checkbox",checked:*comms_audio.read(),disabled:*busy.read(),"aria-describedby":"audience-audio-help",onchange:move|e|comms_audio.set(e.checked())} " Include crew voice (VC)" }
       label {input {r#type:"checkbox",checked:commentary_audio(),disabled:busy(),onchange:move|e|commentary_audio.set(e.checked())}" Include programme commentary microphones"}
       p {id:"audience-audio-help","Viewers can listen without joining the call."}
      }
      div {
       button {disabled:*busy.read()||!(3..=60).contains(&*delay.read()),onclick:move |_|{
         let mut next=broadcast.clone();next.delay_seconds = *delay.read();next.label=label.read().clone();next.comms_audio_enabled = *comms_audio.read();next.commentary_audio_enabled=commentary_audio();busy.set(true);
         spawn(async move{match http_post_json::<BroadcastState,BroadcastState>("/api/live_streams/control",&next).await{Ok(_)=>message.set("Broadcast settings saved.".into()),Err(e)=>message.set(e)}busy.set(false);});
       },"Apply broadcast settings"}
      }
      div {class:"gs26-studio-actions",button {aria_expanded:audio_desk().to_string(),onclick:move|_|audio_desk.toggle(),if audio_desk(){"Close audio desk"}else{"Open audio desk"}}}
      if audio_desk(){div {style:"height:360px;min-height:300px;",super::media_tool_tab::MediaToolTab {id:"gs26-studio-audio",path:"/studio-audio?embedded=1",title:"Broadcast audio desk",visible:true}}}
      details {summary {"OBS browser source"}
        p {"Use this address as an OBS Browser Source for the clean programme, without app navigation or playback controls."}
        input {r#type:"url",readonly:true,"aria-label":"OBS browser source URL",value:format!("{}{}&clean=1",super::UrlConfig::base_http().trim_end_matches('/'),program_url)}
        a {href:format!("{}{}&clean=1",super::UrlConfig::base_http().trim_end_matches('/'),program_url),target:"_blank",rel:"noopener noreferrer","Open clean feed"}
      }
      LayoutDesigner {broadcast:broadcast.clone(),streams:streams.clone()}
      TranscodingHosts {}
      InterviewEditor {broadcast:broadcast.clone(),streams}
      super::stream_rundown::RundownEditor {broadcast:broadcast.clone()}
      super::recorded_replay_controls::RecordedReplayControls {broadcast:broadcast.clone()}
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
    saved
        .as_ref()
        .filter(|s| s.revision > current.revision)
        .unwrap_or(current)
        .clone()
}
async fn save_replay(
    mut request: BroadcastState,
    plan: Option<super::live_stream_tab::ReplayPlan>,
    mut busy: Signal<bool>,
    mut message: Signal<String>,
    mut saved: Signal<Option<BroadcastState>>,
    mut draft: Signal<super::live_stream_tab::ReplayPlan>,
) {
    request.replay = plan;
    busy.set(true);
    message.set("Preparing buffered cameras…".into());
    match http_post_json::<BroadcastState, BroadcastState>("/api/live_streams/control", &request)
        .await
    {
        Ok(value) => {
            if let Some(plan) = &value.replay {
                draft.set(plan.clone());
            }
            message.set(
                if value.replay.is_some() {
                    "Replay on air"
                } else {
                    "Live program restored"
                }
                .into(),
            );
            saved.set(Some(value));
        }
        Err(e) => message.set(e),
    }
    busy.set(false);
}
#[component]
pub(super) fn ReplayControls(
    broadcast: BroadcastState,
    streams: Vec<super::live_stream_tab::LiveStreamSpec>,
) -> Element {
    use super::live_stream_tab::{ReplayCamera, ReplayPlan};
    let mut draft = use_signal(|| {
        broadcast.replay.clone().unwrap_or(ReplayPlan {
            id: String::new(),
            duration_seconds: 10,
            speed: 0.5,
            cameras: streams
                .first()
                .map(|s| {
                    vec![ReplayCamera {
                        id: s.id.clone(),
                        x: 50.,
                        y: 50.,
                        zoom: 1.,
                    }]
                })
                .unwrap_or_default(),
            started_at_ms: 0,
            source_end_ms: 0,
        })
    });
    let busy = use_signal(|| false);
    let message = use_signal(String::new);
    let saved = use_signal(|| None::<BroadcastState>);
    let current = freshest_broadcast(&broadcast, &saved.read());
    let can_apply = current.replay.is_some();
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
        let current = BroadcastState {
            revision: 4,
            ..Default::default()
        };
        assert_eq!(
            freshest_broadcast(
                &current,
                &Some(BroadcastState {
                    revision: 3,
                    ..Default::default()
                })
            )
            .revision,
            4
        );
        assert_eq!(
            freshest_broadcast(
                &current,
                &Some(BroadcastState {
                    revision: 5,
                    ..Default::default()
                })
            )
            .revision,
            5
        );
    }
}

#[component]
fn InterviewEditor(
    broadcast: BroadcastState,
    streams: Vec<super::live_stream_tab::LiveStreamSpec>,
) -> Element {
    let mut base = use_signal(|| broadcast.clone());
    let mut main = use_signal(|| broadcast.interview.main_stream_id.clone());
    let mut pip = use_signal(|| broadcast.interview.pip_stream_id.clone());
    let mut main_audio = use_signal(|| broadcast.interview.main_audio);
    let mut pip_audio = use_signal(|| broadcast.interview.pip_audio);
    let mut busy = use_signal(|| false);
    let mut message = use_signal(String::new);
    rsx! {details {class:"gs26-interview-editor",summary {"Interview scene"}
        p {class:"gs26-studio-help","Choose a camera/participant feed for the main view and another feed for the corner. Share a browser camera from Cameras to add a participant."}
        div {class:"gs26-studio-fields",
            label {"Interview camera / participant" select {value:main(),onchange:move|e|main.set(e.value()),option {value:"","Choose feed"}for s in &streams {option {value:s.id.clone(),"{s.label}"}}}}
            label {"Picture-in-picture camera" select {value:pip(),onchange:move|e|pip.set(e.value()),option {value:"","Choose feed"}for s in &streams {option {value:s.id.clone(),"{s.label}"}}}}
        }
        label {input {r#type:"checkbox",checked:main_audio(),onchange:move|e|main_audio.set(e.checked())}" Include interview-feed audio"}
        label {input {r#type:"checkbox",checked:pip_audio(),onchange:move|e|pip_audio.set(e.checked())}" Include picture-in-picture audio"}
        p {class:"gs26-studio-help","Feed audio, commentary and crew-voice inclusion are separate choices. Use headphones when monitoring comms."}
        div {class:"gs26-studio-actions",
            button {disabled:busy()||main().is_empty()||pip().is_empty()||main()==pip(),onclick:move|_|{
                let mut request=base();request.layout="interview".into();request.interview=super::live_stream_tab::InterviewScene{main_stream_id:main(),pip_stream_id:pip(),main_audio:main_audio(),pip_audio:pip_audio()};busy.set(true);
                spawn(async move{match http_post_json::<BroadcastState,BroadcastState>("/api/live_streams/control",&request).await{Ok(saved)=>{base.set(saved);message.set("Interview scene saved and selected".into());},Err(e)=>message.set(e)}busy.set(false);});
            },"Take interview on air"}
            button {disabled:busy(),onclick:move|_|{busy.set(true);spawn(async move{match http_get_json::<super::live_stream_tab::LiveStreamConfig>("/api/live_streams").await{Ok(cfg)=>{main.set(cfg.broadcast.interview.main_stream_id.clone());pip.set(cfg.broadcast.interview.pip_stream_id.clone());main_audio.set(cfg.broadcast.interview.main_audio);pip_audio.set(cfg.broadcast.interview.pip_audio);base.set(cfg.broadcast);message.set("Interview settings reloaded".into());},Err(e)=>message.set(e)}busy.set(false);});},"Reload interview settings"}
        }
        if !message().is_empty(){p {role:"status","{message}"}}
    }}
}

#[cfg(test)]
mod layout_tests {
    use super::*;
    #[test]
    fn presets_keep_reordered_cameras_inside_canvas_without_overlap() {
        for count in 1..=8 {
            let ids:Vec<_>=(0..count).rev().map(|i|format!("camera-{i}")).collect();
            for preset in ["grid","main-side","featured"] {
                for split in [35.0,65.0,75.0] {
                    let tiles=camera_tiles(&ids,preset,split);
                    assert_eq!(tiles[0].stream_id,ids[0]);
                    assert_eq!(tiles.len(),if preset=="featured"{1}else{count});
                    for (i,a) in tiles.iter().enumerate() {
                        assert!(a.x>=0.0&&a.y>=0.0&&a.width>=10.0&&a.height>=10.0&&a.x+a.width<=100.001&&a.y+a.height<=100.001);
                        for b in &tiles[i+1..] {
                            let intersection=(a.x+a.width).min(b.x+b.width)-a.x.max(b.x);
                            let vertical=(a.y+a.height).min(b.y+b.height)-a.y.max(b.y);
                            assert!(intersection<0.001||vertical<0.001);
                        }
                    }
                }
            }
        }
    }
}
