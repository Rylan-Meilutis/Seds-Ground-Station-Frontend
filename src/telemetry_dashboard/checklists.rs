//! GroundStation-owned checklist progress and administrator-managed templates.
use super::{http_get_json, http_post_json, layout::ThemeConfig};
use dioxus::prelude::*;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::sync::Arc;
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
#[derive(Clone, Default, Serialize, Deserialize, PartialEq)]
struct Revision {
    clock: BTreeMap<String, u64>,
    value: serde_json::Value,
}
#[derive(Clone, Default, Serialize, Deserialize, PartialEq)]
struct History {
    heads: Vec<Revision>,
}
#[derive(Clone, Default, Deserialize, PartialEq)]
struct SyncStatus {
    enabled: bool,
    ready: bool,
    durability_pending: bool,
    definition_pending: bool,
    error: Option<String>,
    pending_reason: Option<String>,
    conflicts: BTreeMap<String, History>,
    conflict_count: usize,
    qualification_binding_valid: bool,
}
#[derive(Serialize)]
struct Resolve {
    key: String,
    expected: History,
    value: serde_json::Value,
}
fn concise(text: &str) -> String {
    let mut result = text.chars().take(160).collect::<String>();
    if text.chars().count() > 160 {
        result.push('…');
    }
    result
}
fn version_summary(value: &serde_json::Value) -> String {
    if value.is_null() {
        return "Removed".into();
    }
    if let Some(checked) = value.get("checked").and_then(|value| value.as_bool()) {
        return if checked {
            value
                .get("completed_by")
                .and_then(|value| value.as_str())
                .map(|actor| format!("Complete · {}", concise(actor)))
                .unwrap_or_else(|| "Complete".into())
        } else {
            "Not complete".into()
        };
    }
    if let Some(text) = value
        .get("text")
        .or_else(|| value.get("title"))
        .and_then(|value| value.as_str())
    {
        return concise(text);
    }
    if let Some(enforce) = value
        .get("enforce_before_launch")
        .and_then(|value| value.as_bool())
    {
        return if enforce {
            "Required checks enforced".into()
        } else {
            "Launch enforcement off".into()
        };
    }
    "Checklist version".into()
}
fn conflict_label(key: &str, history: &History) -> String {
    if key == "checklists/policy" {
        return "Launch checklist policy".into();
    }
    let value = history
        .heads
        .iter()
        .map(|head| &head.value)
        .find(|value| !value.is_null());
    if let Some(value) = value {
        let list = value
            .get("list_id")
            .or_else(|| value.get("id"))
            .and_then(|value| value.as_str())
            .unwrap_or("Checklist");
        let item = value
            .get("item_id")
            .or_else(|| value.get("text"))
            .and_then(|value| value.as_str());
        return concise(
            &item
                .map(|item| format!("{list} · {item}"))
                .unwrap_or_else(|| list.into()),
        );
    }
    "Removed checklist".into()
}
fn item_conflicted(sync: &SyncStatus, list: &str, item: &str) -> bool {
    sync.conflicts.iter().any(|(key, history)| {
        key.starts_with("checklists/progress/")
            && history.heads.iter().any(|head| {
                head.value.get("list_id").and_then(|value| value.as_str()) == Some(list)
                    && head.value.get("item_id").and_then(|value| value.as_str()) == Some(item)
            })
    })
}
fn item_complete(sync: &SyncStatus, list: &str, item: &Item) -> bool {
    item.checked && !item_conflicted(sync, list, &item.id)
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
    let mut sync_state = use_signal(SyncStatus::default);
    let admin = crate::auth::current_status()
        .roles
        .iter()
        .any(|role| role == "admin");
    use_future(move || async move {
        let mut ticks = 0u64;
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
            if ticks.is_multiple_of(5) {
                if let Ok(value) = http_get_json::<SyncStatus>("/api/checklists/sync").await {
                    if value != *sync_state.read() {
                        sync_state.set(value);
                    }
                }
            }
            ticks = ticks.wrapping_add(1);
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
        .filter(|c| {
            c.required_before_launch
                && c.items.iter().any(|i| !item_complete(&sync_state.read(), &c.id, i))
        })
        .count();
    rsx! {
     style {{include_str!("checklists.css")}}
     main {class:"gs26-checklists",style:"color:{theme.text_primary};--checklist-panel:{theme.panel_background};--checklist-border:{theme.button_border};","aria-label":"Shared checklists",
      header {div {h2 {"Checklists"}p {role:"status",if !*loaded.read(){{format!("Connecting to {}…", crate::auth::station_name())}}else if document.read().enforce_before_launch{if missing==0{"Required checklists complete"}else{"{missing} required checklists incomplete — launch locked"}}else{"Shared progress · launch enforcement off"}}}
       if admin && draft.read().is_none(){button {disabled:!*loaded.read(),onclick:move |_|{draft.set(Some(document.read().clone()));message.set(String::new());},"Manage checklists"}}
      }
      if !message.read().is_empty(){p {role:"status","{message}"}}
      if sync_state.read().enabled {
       section {class:"gs26-checklist-sync","aria-label":"Paired checklist synchronization",
        div {class:"gs26-checklist-sync-heading",h3 {"Station sync"}span {class:"gs26-checklist-sync-badge",if !sync_state.read().ready{"Preparing"}else if sync_state.read().conflict_count>0{"Review needed"}else if sync_state.read().definition_pending||sync_state.read().durability_pending{"Pending"}else{"Shared"}}}
        if !sync_state.read().ready {p {role:"status","Preparing durable checklists. Checks will be available shortly."}}
        else if sync_state.read().durability_pending {p {role:"status","Saving checklist state. Sequence starts remain locked until storage is ready."}}
        else if sync_state.read().definition_pending {p {role:"status","Template updates are waiting for a complete reviewed version, Idle and cleared qualification. Progress stays editable."}}
        else if !sync_state.read().qualification_binding_valid {p {role:"status","Reset local qualification before using this checklist template for a procedure."}}
        if sync_state.read().definition_pending {
         if let Some(reason)=sync_state.read().pending_reason.as_ref(){
          p {class:"gs26-checklist-sync-note",role:"status","aria-label":"Pending checklist update details","{concise(reason)}"}
         }
        }
        if let Some(error)=sync_state.read().error.as_ref(){p {class:"gs26-checklist-sync-note",role:"status","{concise(error)}"}}
        if sync_state.read().conflict_count>0 {
         p {role:"status",if admin{"Choose the version to keep. Conflicting progress does not count as complete."}else{"An administrator must review conflicting updates before those items can count as complete."}}
         if admin {
          for (key,history) in &sync_state.read().conflicts {
           {let key=key.clone();let expected=Arc::new(history.clone());let label=conflict_label(&key,history);rsx!{
            article {class:"gs26-checklist-conflict",key:"{key}",h4 {"{label}"}
             div {class:"gs26-checklist-versions",
              for (index,head) in history.heads.iter().enumerate(){
               {let key=key.clone();let expected=expected.clone();let value=head.value.clone();let summary=version_summary(&value);let stations=concise(&head.clock.keys().cloned().collect::<Vec<_>>().join(", "));rsx!{
                button {disabled:*busy.read()||!sync_state.read().ready||sync_state.read().durability_pending,onclick:move |_|{
                 let request=Resolve{key:key.clone(),expected:(*expected).clone(),value:value.clone()};busy.set(true);
                 spawn(async move{match http_post_json::<Resolve,Document>("/api/checklists/resolve",&request).await{Ok(value)=>{document.set(value);message.set("Reviewed version saved".into());if let Ok(status)=http_get_json::<SyncStatus>("/api/checklists/sync").await{sync_state.set(status);}},Err(error)=>message.set(error)}busy.set(false);});
                },strong {"Use version {index+1}"}span {"{summary}"}small {"{stations}"}}
               }}
              }
             }
            }
           }}
          }
          if sync_state.read().conflict_count>sync_state.read().conflicts.len(){p {class:"gs26-checklist-sync-note","Additional conflicts will appear after these are resolved."}}
         }
        }
       }
      }
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
          {let list_id=list.id.clone();let active=current.as_ref().is_some_and(|c|c.id==list.id);let complete=list.items.iter().filter(|i|item_complete(&sync_state.read(),&list.id,i)).count();rsx!{
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
             input {r#type:"checkbox",checked:item_complete(&sync_state.read(),&list.id,item),disabled:*busy.read()||!*loaded.read()||(sync_state.read().enabled&&(!sync_state.read().ready||sync_state.read().durability_pending))||item_conflicted(&sync_state.read(),&list.id,&item.id),onchange:move|event|{
              let request=Check{checklist_id:list_id.clone(),item_id:item_id.clone(),checked:event.checked()};busy.set(true);
              spawn(async move{match http_post_json::<Check,Document>("/api/checklists/check",&request).await{Ok(value)=>{if value.revision>=document.read().revision{document.set(value);}message.set(String::new());},Err(error)=>message.set(error)}busy.set(false);});
             }}
             div {span {"{item.text}"}
              if item.completion_source.as_deref()==Some("automated_sequence"){small {"Test passed · saved automatically"}}
              else if item.completion_source.as_deref()==Some("manual_override"){if let Some(actor)=&item.completed_by{small {"Manual override by {actor}"}}}
              else if let Some(actor)=&item.completed_by{small {"Completed by {actor}"}}
              if item_conflicted(&sync_state.read(),&list.id,&item.id){small {"Conflicting shared updates · administrator review needed"}}
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

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn reviewed_versions_keep_receipt_and_causal_context() {
        let receipt = serde_json::json!({"list_id":"setup","item_id":"test","checked":true,"completion_source":"automated_sequence","completed_by":"sequence engine","completed_at_ms":123});
        let history = History {
            heads: vec![
                Revision {
                    clock: BTreeMap::from([("rrls".into(), 3)]),
                    value: receipt.clone(),
                },
                Revision {
                    clock: BTreeMap::from([("gs".into(), 4)]),
                    value: serde_json::json!({"list_id":"setup","item_id":"test","checked":false}),
                },
            ],
        };
        let request = Resolve {
            key: "checklists/progress/example".into(),
            expected: history.clone(),
            value: receipt.clone(),
        };
        let wire = serde_json::to_value(request).unwrap();
        assert_eq!(wire["value"], receipt);
        assert_eq!(wire["expected"]["heads"].as_array().unwrap().len(), 2);
        let status = SyncStatus {
            enabled: true,
            ready: true,
            conflict_count: 1,
            conflicts: BTreeMap::from([("checklists/progress/example".into(), history.clone())]),
            ..Default::default()
        };
        assert!(item_conflicted(&status, "setup", "test"));
        let retained = Item { id: "test".into(), checked: true, ..Default::default() };
        assert!(!item_complete(&status, "setup", &retained), "retained local check cannot satisfy a shared progress conflict");
        assert!(item_complete(&SyncStatus::default(), "setup", &retained));
        assert!(!item_conflicted(&status, "setup", "other"));
        assert_eq!(
            conflict_label("checklists/progress/example", &history),
            "setup · test"
        );
        assert!(version_summary(&receipt).contains("sequence engine"));
        assert_eq!(version_summary(&serde_json::Value::Null), "Removed");
    }
    #[test]
    fn conflict_preview_is_bounded_unicode_and_plain_text() {
        let text = "é".repeat(300);
        assert_eq!(concise(&text).chars().count(), 161);
        assert_eq!(
            version_summary(&serde_json::json!({"text":"<button>review</button>"})),
            "<button>review</button>"
        );
    }
}
