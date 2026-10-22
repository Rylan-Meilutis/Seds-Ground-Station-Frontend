//! GroundStation-owned checklist progress and administrator-managed templates.
use super::{http_get_json, http_post_json, layout::ThemeConfig};
use dioxus::prelude::*;
use serde::{Deserialize, Serialize};
#[derive(Clone, Default, Serialize, Deserialize, PartialEq)]
struct Item {
    id: String,
    text: String,
    checked: bool,
    #[serde(default)]
    complete_on: Option<String>,
    #[serde(default)]
    completion_source: Option<String>,
    completed_by: Option<String>,
    completed_at_ms: Option<i64>,
}
#[derive(Clone, Default, Serialize, Deserialize, PartialEq)]
struct Checklist {
    id: String,
    title: String,
    required_before_launch: bool,
    items: Vec<Item>,
}
#[derive(Clone, Default, Serialize, Deserialize, PartialEq)]
struct Document {
    revision: u64,
    enforce_before_launch: bool,
    checklists: Vec<Checklist>,
}
#[derive(Serialize)]
struct Check {
    checklist_id: String,
    item_id: String,
    checked: bool,
}
fn id() -> Result<String, String> {
    let mut bytes = [0u8; 16];
    getrandom::fill(&mut bytes).map_err(|_| "Cannot generate a checklist ID".to_string())?;
    Ok(bytes.iter().map(|b| format!("{b:02x}")).collect())
}
#[component]
pub(super) fn ChecklistsTab(theme: ThemeConfig) -> Element {
    let mut document = use_signal(Document::default);
    let mut loaded = use_signal(|| false);
    let mut selected = use_signal(String::new);
    let mut message = use_signal(String::new);
    let mut busy = use_signal(|| false);
    let mut draft = use_signal(|| None::<Document>);
    let admin = crate::auth::current_status()
        .roles
        .iter()
        .any(|role| role == "admin");
    use_future(move || async move {
        loop {
            match http_get_json::<Document>("/api/checklists").await {
                Ok(value) => {
                    if !*loaded.read() || value.revision >= document.read().revision {
                        document.set(value);
                    }
                    loaded.set(true);
                }
                Err(error) => {
                    message.set(error);
                    loaded.set(false);
                }
            }
            #[cfg(target_arch = "wasm32")]
            gloo_timers::future::TimeoutFuture::new(1000).await;
            #[cfg(not(target_arch = "wasm32"))]
            tokio::time::sleep(std::time::Duration::from_secs(1)).await;
        }
    });
    let current = {
        let snapshot = document.read();
        snapshot
            .checklists
            .iter()
            .find(|c| c.id == *selected.read())
            .or_else(|| snapshot.checklists.first())
            .cloned()
    };
    let missing = document
        .read()
        .checklists
        .iter()
        .filter(|c| c.required_before_launch && c.items.iter().any(|i| !i.checked))
        .count();
    rsx! {
     style {{include_str!("checklists.css")}}
     main {class:"gs26-checklists",style:"color:{theme.text_primary};--checklist-panel:{theme.panel_background};--checklist-border:{theme.button_border};","aria-label":"Shared checklists",
      header {div {h2 {"Checklists"}p {role:"status",if !*loaded.read(){{format!("Connecting to {}…", crate::auth::station_name())}}else if document.read().enforce_before_launch{if missing==0{"Required checklists complete"}else{"{missing} required checklists incomplete — launch locked"}}else{"Shared progress · launch enforcement off"}}}
       if admin && draft.read().is_none(){button {disabled:!*loaded.read(),onclick:move |_|{draft.set(Some(document.read().clone()));message.set(String::new());},"Manage checklists"}}
      }
      if !message.read().is_empty(){p {role:"status","{message}"}}
      if let Some(edit)=draft.read().clone(){
       section {class:"gs26-checklist-editor","aria-label":"Checklist administration",
        h3 {"Checklist administration"}
        label {input {r#type:"checkbox",checked:edit.enforce_before_launch,disabled:*busy.read(),onchange:move|e|{if let Some(d)=draft.write().as_mut(){d.enforce_before_launch=e.checked();}}}"Require selected checklists before launch"}
        p {"Changing text or its test link resets completion. Manual checks do not run valves or bypass interlocks."}
        for (index,list) in edit.checklists.iter().enumerate(){
         article {key:"{list.id}",
          label {"Checklist name" input {value:list.title.clone(),maxlength:"120",disabled:*busy.read(),oninput:move|e|{if let Some(d)=draft.write().as_mut(){d.checklists[index].title=e.value();}}}}
          label {input {r#type:"checkbox",checked:list.required_before_launch,disabled:*busy.read(),onchange:move|e|{if let Some(d)=draft.write().as_mut(){d.checklists[index].required_before_launch=e.checked();}}}"Required before launch"}
          for (item_index,item) in list.items.iter().enumerate(){
           div {class:"gs26-checklist-edit-item",key:"{item.id}",
            input {"aria-label":"Checklist item",value:item.text.clone(),maxlength:"500",disabled:*busy.read(),oninput:move|e|{if let Some(d)=draft.write().as_mut(){d.checklists[index].items[item_index].text=e.value();}}}
            label {"Complete after"
             select {"aria-label":"Complete after",value:item.complete_on.clone().unwrap_or_default(),disabled:*busy.read(),onchange:move |e|{if let Some(d)=draft.write().as_mut(){let value=e.value();d.checklists[index].items[item_index].complete_on=(!value.is_empty()).then_some(value);}},
              option {value:"","Manual check"}
              option {value:"valve_self_test","Valve self-test passes"}
              option {value:"nitrogen_test","Nitrogen test passes"}
              option {value:"fill_target","Fill target reached"}
             }
            }
            button {disabled:*busy.read(),onclick:move |_|{if let Some(d)=draft.write().as_mut(){d.checklists[index].items.remove(item_index);}},"Remove item"}
           }
          }
          div {class:"gs26-checklist-tools",
           button {disabled:*busy.read(),onclick:move |_|{match id(){Ok(id)=>if let Some(d)=draft.write().as_mut(){d.checklists[index].items.push(Item{id,text:"New item".into(),..Default::default()})},Err(e)=>message.set(e)}},"Add item"}
           button {disabled:*busy.read(),onclick:move |_|{if let Some(d)=draft.write().as_mut(){d.checklists.remove(index);}},"Remove checklist"}
          }
         }
        }
        div {class:"gs26-checklist-tools",
         button {disabled:*busy.read(),onclick:move |_|{match id(){Ok(id)=>if let Some(d)=draft.write().as_mut(){d.checklists.push(Checklist{id,title:"New checklist".into(),..Default::default()})},Err(e)=>message.set(e)}},"Add checklist"}
         button {disabled:*busy.read(),onclick:move |_|{let Some(request)=draft.read().clone()else{return};busy.set(true);spawn(async move{match http_post_json::<Document,Document>("/api/checklists",&request).await{Ok(value)=>{document.set(value);draft.set(None);message.set("Checklists saved".into());},Err(error)=>message.set(error)}busy.set(false);});},"Save changes"}
         button {disabled:*busy.read(),onclick:move |_|{draft.set(None);message.set(String::new());},"Cancel"}
        }
       }
      }else{
       div {class:"gs26-checklist-layout",
        nav {"aria-label":"Checklists",
         for list in &document.read().checklists{
          {let list_id=list.id.clone();let active=current.as_ref().is_some_and(|c|c.id==list.id);let complete=list.items.iter().filter(|i|i.checked).count();rsx!{
           button {key:"{list.id}","aria-current":if active{"page"}else{"false"},onclick:move |_|selected.set(list_id.clone()),span {"{list.title}"}small {"{complete}/{list.items.len()}"}if list.required_before_launch{small {"Required"}}}
          }}
         }
        }
        if let Some(list)=current{
         section {h3 {"{list.title}"}
          if list.items.is_empty(){p {"No items in this checklist."}}
          for item in &list.items{
           {let list_id=list.id.clone();let item_id=item.id.clone();rsx!{
            label {class:"gs26-checklist-item",key:"{item.id}",
             input {r#type:"checkbox",checked:item.checked,disabled:*busy.read()||!*loaded.read(),onchange:move|event|{
              let request=Check{checklist_id:list_id.clone(),item_id:item_id.clone(),checked:event.checked()};busy.set(true);
              spawn(async move{match http_post_json::<Check,Document>("/api/checklists/check",&request).await{Ok(value)=>{if value.revision>=document.read().revision{document.set(value);}message.set(String::new());},Err(error)=>message.set(error)}busy.set(false);});
             }}
             div {span {"{item.text}"}
              if item.completion_source.as_deref()==Some("automated_sequence"){small {"Test passed · saved automatically"}}
              else if item.completion_source.as_deref()==Some("manual_override"){if let Some(actor)=&item.completed_by{small {"Manual override by {actor}"}}}
              else if let Some(actor)=&item.completed_by{small {"Completed by {actor}"}}
              if !item.checked && item.complete_on.is_some(){small {"Linked test completes this item automatically. A manual check records an override."}}
             }
            }
           }}
          }
          p {class:"gs26-checklist-note",{format!("Progress is saved by {} and shared with signed-in devices.", crate::auth::station_name())}}
         }
        }
       }
      }
     }
    }
}
