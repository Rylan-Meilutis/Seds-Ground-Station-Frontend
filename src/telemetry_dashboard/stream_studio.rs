use super::{UrlConfig, http_get_json, http_post_json, live_stream_tab::BroadcastState};
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
    let mut message = use_signal(String::new);
    let mut busy = use_signal(|| false);
    let mut accounts = use_signal(Vec::<Account>::new);
    let admin = auth::current_status()
        .roles
        .iter()
        .any(|r| r == "stream_admin");
    use_future(move || async move {
        if admin {
            match http_get_json::<Vec<Account>>("/api/stream-roles").await {
                Ok(v) => accounts.set(v),
                Err(e) => message.set(e),
            }
        }
    });
    let url = if program_url.starts_with('/') {
        format!("{}{}", UrlConfig::base_http(), program_url)
    } else {
        program_url
    };
    rsx! {section {style:"width:100%;padding:16px;box-sizing:border-box;border:1px solid #344454;border-radius:12px;background:#101923;color:#e4ebf0;",
      h3 {style:"margin:0 0 12px;","Broadcast studio"}
      p {style:"font-size:12px;color:#a9b7c4;","Your cameras below are LIVE previews. Viewers receive the delayed program shown here. A camera cut takes effect on the current delayed timeline."}
      div {style:"display:flex;align-items:center;gap:12px;flex-wrap:wrap;",
       label {"Audience delay (3–60 seconds) " input {r#type:"number",min:"3",max:"60",value:"{delay}",disabled:*busy.read(),oninput:move|e|{if let Ok(v)=e.value().parse(){delay.set(v);}}}}
       button {disabled:*busy.read()||!(3..=60).contains(&*delay.read()),onclick:move |_|{
         let mut next=broadcast.clone();next.delay_seconds=*delay.read();busy.set(true);
         spawn(async move{match http_post_json::<BroadcastState,BroadcastState>("/api/live_streams/control",&next).await{Ok(_)=>message.set("Delay saved. Audience buffers are rebuilding.".into()),Err(e)=>message.set(e)}busy.set(false);});
       },"Apply delay"}
      }
      iframe {src:url,title:"Delayed audience program monitor",allow:"autoplay; fullscreen",style:"width:100%;height:280px;border:0;margin-top:12px;background:#080d15;"}
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
