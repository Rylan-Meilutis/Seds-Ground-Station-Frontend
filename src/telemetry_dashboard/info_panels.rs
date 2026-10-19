//! Personal ordering and visibility for informational panels, independent of viewport size.
use super::persist;
use dioxus::prelude::*;
use serde::{Deserialize, Serialize};

#[derive(Clone, PartialEq)]
pub(super) struct InfoPanel {
    pub id: String,
    pub title: String,
    pub category: String,
    pub content: Element,
}
impl InfoPanel {
    pub fn new(id: &str, title: &str, category: &str, content: Element) -> Self {
        Self {
            id: id.into(),
            title: title.into(),
            category: category.into(),
            content,
        }
    }
}
#[derive(Clone, Default, Serialize, Deserialize)]
struct Preferences {
    #[serde(default)]
    order: Vec<String>,
    #[serde(default)]
    hidden: Vec<String>,
}
fn ordered_ids(panels: &[InfoPanel], preferences: &Preferences) -> Vec<String> {
    let mut ids = Vec::new();
    for id in preferences.order.iter().chain(panels.iter().map(|p| &p.id)) {
        if panels.iter().any(|p| &p.id == id) && !ids.contains(id) {
            ids.push(id.clone());
        }
    }
    ids
}
fn save(key: &str, value: &Preferences) {
    if let Ok(raw) = serde_json::to_string(value) {
        persist::set_string(key, &raw);
    }
}
#[component]
pub(super) fn InfoPanels(
    page: String,
    panels: Vec<InfoPanel>,
    #[props(default)] wide: bool,
) -> Element {
    let key = format!(
        "{}_panels_v1_{page}",
        super::personal_dashboard_customization_key()
    );
    let initial = key.clone();
    let mut preferences = use_signal(move || {
        persist::get_string(&initial)
            .and_then(|raw| serde_json::from_str::<Preferences>(&raw).ok())
            .unwrap_or_default()
    });
    let mut editing = use_signal(|| false);
    let mut selected = use_signal(|| "Overview".to_string());
    let mut categories = Vec::new();
    for panel in &panels {
        if !categories.contains(&panel.category) {
            categories.push(panel.category.clone());
        }
    }
    let active = if categories.contains(&selected.read()) {
        selected.read().clone()
    } else {
        categories.first().cloned().unwrap_or_default()
    };
    let order = ordered_ids(&panels, &preferences.read());
    rsx! {
        style {{include_str!("info_panels.css")}}
        section {class:"gs26-info-panels","data-wide":wide.to_string(),"aria-label":"Information panels",
            div {class:"gs26-info-toolbar",
                nav {"aria-label":"Information categories",if categories.len()>1 {for category in categories {
                    button {"aria-pressed":(active==category).to_string(),onclick:move |_|selected.set(category.clone()),"{category}"}
                }}}
                button {"aria-expanded":editing.read().to_string(),onclick:move |_|{let next=!*editing.read();editing.set(next);},if *editing.read(){"Done arranging"}else{"Arrange panels"}}
            }
            if *editing.read() {
                div {class:"gs26-panel-editor",
                    div {class:"gs26-panel-editor-heading",span {"Saved for your account on this device."}
                        button {onclick:{let key=key.clone();move |_|{let next=Preferences::default();save(&key,&next);preferences.set(next);}},"Reset panels"}
                    }
                    for (index,id) in order.iter().enumerate() {
                        if let Some(panel)=panels.iter().find(|panel|&panel.id==id) {
                            {let id=id.clone();let ids=order.clone();let name=panel.title.clone();let category=panel.category.clone();let key=key.clone();
                            rsx! {div {class:"gs26-panel-editor-row",
                                label {input {r#type:"checkbox",checked:!preferences.read().hidden.contains(&id),onchange:{let id=id.clone();let key=key.clone();move |e|{let mut next=preferences.read().clone();next.hidden.retain(|old|old!=&id);if !e.checked(){next.hidden.push(id.clone());}save(&key,&next);preferences.set(next);}}} span {"{name}" small {"{category}"}}}
                                button {"aria-label":"Move {name} up",disabled:index==0,onclick:{let ids=ids.clone();let key=key.clone();move |_|{let mut next=preferences.read().clone();next.order=ids.clone();next.order.swap(index,index-1);save(&key,&next);preferences.set(next);}},"↑"}
                                button {"aria-label":"Move {name} down",disabled:index+1==ids.len(),onclick:move |_|{let mut next=preferences.read().clone();next.order=ids.clone();next.order.swap(index,index+1);save(&key,&next);preferences.set(next);},"↓"}
                            }}}
                        }
                    }
                }
            }
            div {class:"gs26-info-masonry",
                for id in &order {
                    if let Some(panel)=panels.iter().find(|panel|&panel.id==id && panel.category==active && !preferences.read().hidden.contains(id)) {
                        article {key:"{id}",class:"gs26-info-panel","data-panel-id":"{id}","aria-label":"{panel.title}",{panel.content.clone()}}
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
    fn new_runtime_panels_append_and_duplicates_do_not_hide_information() {
        let panels = vec![
            InfoPanel::new("a", "A", "Overview", rsx! {}),
            InfoPanel::new("b", "B", "Overview", rsx! {}),
            InfoPanel::new("new", "New", "Network", rsx! {}),
        ];
        let prefs = Preferences {
            order: vec!["b".into(), "gone".into(), "b".into(), "a".into()],
            hidden: vec![],
        };
        assert_eq!(ordered_ids(&panels, &prefs), vec!["b", "a", "new"]);
    }
}
