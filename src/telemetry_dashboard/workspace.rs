//! A second visible tab, sharing the dashboard's existing data and command owners.
use super::*;
use std::{
    cell::{Cell, RefCell},
    rc::Rc,
};

// Same width rule on every platform, including iPad landscape and iPad
// Split View; phones stay single-pane.
const MIN_WIDTH: u32 = 1024;
static VIEWPORT_ID: AtomicU64 = AtomicU64::new(1);

#[derive(Clone, Serialize, Deserialize, PartialEq)]
struct Preference {
    enabled: bool,
    primary: String,
    secondary: String,
}
impl Default for Preference {
    fn default() -> Self {
        // Split view is a mode the operator turns on; off is a single pane.
        Self {
            enabled: false,
            primary: "data".into(),
            secondary: "actions".into(),
        }
    }
}

fn secondary_allowed(tab: MainTab, primary: MainTab) -> bool {
    match tab {
        // Editors and singleton media stay primary. Next to Actions, State
        // leaves its ground controls out so there is one command surface.
        MainTab::Actions => matches!(
            primary,
            MainTab::State
                | MainTab::Checklists
                | MainTab::Data
                | MainTab::ConnectionStatus
                | MainTab::Detailed
                | MainTab::NetworkTopology
                | MainTab::Map
                | MainTab::Vehicle
                | MainTab::Messages
                | MainTab::Notifications
                | MainTab::Warnings
                | MainTab::Errors
        ),
        MainTab::State
        | MainTab::Checklists
        | MainTab::Data
        | MainTab::ConnectionStatus
        | MainTab::Detailed
        | MainTab::NetworkTopology
        | MainTab::Map
        | MainTab::Vehicle
        | MainTab::Messages
        | MainTab::Notifications
        | MainTab::Warnings
        | MainTab::Errors
        | MainTab::DataExport
        | MainTab::SequenceHistory => true,
        _ => false,
    }
}

/// Moving the second pane's view into the first swaps the panes, so choosing
/// that view never drops what was already showing, such as Actions.
fn secondary_choice(previous: &[MainTab], primary: MainTab, saved: &str) -> String {
    match previous {
        [first, second] if *second == primary && *first != primary => _main_tab_to_str(*first).into(),
        _ => saved.into(),
    }
}

fn resolve(primary: MainTab, secondary: &str, available: &[MainTab], split: bool) -> Vec<MainTab> {
    let primary = if available.contains(&primary) {
        primary
    } else {
        available.first().copied().unwrap_or(MainTab::State)
    };
    let mut tabs = vec![primary];
    if split
        && !matches!(
            primary,
            MainTab::Mission | MainTab::StreamManager | MainTab::CrewVoice | MainTab::Media
        )
    {
        let mut candidates = available
            .iter()
            .copied()
            .filter(|tab| *tab != primary && secondary_allowed(*tab, primary));
        let selected = candidates
            .clone()
            .find(|tab| _main_tab_to_str(*tab) == secondary)
            .or_else(|| candidates.clone().find(|tab| *tab == MainTab::Data))
            .or_else(|| candidates.clone().find(|tab| *tab == MainTab::Actions))
            .or_else(|| candidates.next());
        if let Some(tab) = selected {
            tabs.push(tab);
        }
    }
    tabs
}

fn storage_key() -> String {
    // Same client storage boundary as customized tabs, with an independent
    // station + account namespace. It conveys no command or server authority.
    let account = auth::current_status()
        .username
        .unwrap_or_else(|| "anonymous".into());
    let station = normalize_base_url(UrlConfig::base_http());
    format!(
        "gs26_wide_workspace_v1:{}",
        serde_json::json!([station, account])
    )
}
fn load(key: &str) -> Preference {
    persist::get_string(key)
        .and_then(|value| serde_json::from_str(&value).ok())
        .unwrap_or_default()
}

#[derive(Clone, Copy, PartialEq)]
pub(super) struct Workspace {
    pub tabs: Memo<Vec<MainTab>>,
    wide: Signal<bool>,
    preference: Signal<Preference>,
    primary: Signal<MainTab>,
}
impl Workspace {
    pub fn contains(self, tab: MainTab) -> bool {
        self.tabs.read().contains(&tab)
    }
}

pub(super) fn use_workspace(
    mut primary: Signal<MainTab>,
    available: Vec<MainTab>,
    streamer: bool,
) -> Workspace {
    let key = storage_key();
    let mut loaded_key = use_signal(|| key.clone());
    let mut preference = use_signal(|| load(&key));
    let mut applied_key = use_signal(|| None::<String>);
    let mut wide = use_signal(|| false);
    let viewport_id = use_hook(|| VIEWPORT_ID.fetch_add(1, Ordering::Relaxed));
    use_future(move || async move {
        let mut script = document::eval(&format!(
            r#"
            (() => {{
                const media = window.matchMedia('(min-width: {MIN_WIDTH}px)');
                const changed = () => dioxus.send(media.matches);
                const key = '__gs26_workspace_{viewport_id}';
                window[key] = () => {{ media.removeEventListener('change', changed); delete window[key]; }};
                media.addEventListener('change', changed);
                changed();
            }})();
        "#
        ));
        while let Ok(value) = script.recv::<bool>().await {
            set_signal_if_changed(&mut wide, value);
        }
    });
    use_drop(move || js_eval(&format!("window.__gs26_workspace_{viewport_id}?.();")));

    use_effect(use_reactive((&key,), move |(key,)| {
        if *loaded_key.peek() != key {
            preference.set(load(&key));
            loaded_key.set(key);
            applied_key.set(None);
        }
    }));
    use_effect(use_reactive((&key, &available), move |(key, available)| {
        if !available.is_empty() && !available.contains(&*primary.peek()) {
            primary.set(available[0]);
        }
        // Restore the saved left-hand tab only while split view is on, so a
        // window or iPad crossing the width threshold never changes the tab.
        if !available.is_empty()
            && *wide.read()
            && preference.read().enabled
            && *loaded_key.read() == key
            && applied_key.read().as_ref() != Some(&key)
        {
            let selected = _main_tab_from_str(&preference.peek().primary);
            if available.contains(&selected) {
                primary.set(selected);
            }
            applied_key.set(Some(key));
        }
    }));
    let last_primary = use_hook(|| Rc::new(Cell::new(None::<MainTab>)));
    use_effect(use_reactive((&key,), move |(key,)| {
        let selected = *primary.read();
        let mut next = preference.read().clone();
        if *wide.read() && *loaded_key.read() == key && applied_key.read().as_ref() == Some(&key) {
            next.primary = _main_tab_to_str(selected).into();
            if let Some(previous) = last_primary.replace(Some(selected))
                && previous != selected
                && next.secondary == _main_tab_to_str(selected)
            {
                next.secondary = _main_tab_to_str(previous).into();
            }
            if let Ok(value) = serde_json::to_string(&next) {
                persist::set_string(&key, &value);
            }
            if *preference.peek() != next {
                preference.set(next);
            }
        }
    }));
    let previous_tabs = use_hook(|| Rc::new(RefCell::new(Vec::<MainTab>::new())));
    let tabs = use_memo(use_reactive(
        (&available, &streamer, &key),
        move |(available, streamer, key)| {
            let next = if streamer {
                vec![MainTab::Mission]
            } else if *loaded_key.read() != key {
                resolve(*primary.read(), "data", &available, false)
            } else {
                let choice = preference.read();
                let selected = *primary.read();
                let secondary = secondary_choice(&previous_tabs.borrow(), selected, &choice.secondary);
                resolve(
                    selected,
                    &secondary,
                    &available,
                    *wide.read() && choice.enabled,
                )
            };
            if *previous_tabs.borrow() != next {
                // Run BEFORE publishing the changed view/DOM, never from a
                // later effect that could consume a fresh Stop pressed in the
                // newly rendered layout. Keep the original gesture tombstone.
                if let Ok(mut pending) = PENDING_COMMAND_PRESS.lock()
                    && let Some(press) = pending.as_mut()
                    && !press.owner.as_ref().is_some_and(CommandOwner::is_emergency)
                {
                    press.consumed = true;
                }
                *previous_tabs.borrow_mut() = next.clone();
            }
            next
        },
    ));
    Workspace {
        tabs,
        wide,
        preference,
        primary,
    }
}

#[component]
pub(super) fn WorkspaceToolbar(
    workspace: Workspace,
    available: Vec<MainTab>,
    labels: Vec<String>,
) -> Element {
    let tabs = workspace.tabs.read();
    let primary = tabs[0];
    let secondary = tabs.get(1).copied();
    let mut preference = workspace.preference;
    let mut selected_primary = workspace.primary;
    let views = available.into_iter().zip(labels).collect::<Vec<_>>();
    rsx! {
        if *workspace.wide.read() && !matches!(primary, MainTab::Mission | MainTab::StreamManager | MainTab::CrewVoice | MainTab::Media) {
            div { class: "gs26-workspace-tools", "aria-label": "Workspace layout", "data-split": secondary.is_some().to_string(),
                button { r#type: "button", class: "gs26-split-toggle", "aria-pressed": secondary.is_some().to_string(),
                    title: if secondary.is_some() { "Show one tab" } else { "Show two tabs side by side" },
                    onclick: move |_| { let mut next = preference.read().clone(); next.enabled = !next.enabled; preference.set(next); },
                    span { class: "gs26-split-icon", "aria-hidden": "true" }
                    "Split view"
                }
                if let Some(secondary) = secondary {
                    label { span { "Left" }
                        select { "aria-label": "First workspace pane",
                            onchange: move |event| selected_primary.set(_main_tab_from_str(&event.value())),
                            for (tab, label) in views.iter().cloned() {
                                option { key: "{_main_tab_to_str(tab)}", value: _main_tab_to_str(tab), selected: tab == primary, "{label}" }
                            }
                        }
                    }
                    label { span { "Right" }
                        select { "aria-label": "Second workspace pane",
                            onchange: move |event| {
                                let chosen = _main_tab_from_str(&event.value());
                                if chosen == primary {
                                    // Same tab on both sides: swap them. The
                                    // primary-change effect moves the old
                                    // primary into the second pane.
                                    selected_primary.set(secondary);
                                } else {
                                    let mut next = preference.read().clone();
                                    next.secondary = event.value();
                                    preference.set(next);
                                }
                            },
                            for (tab, label) in views.iter().cloned().filter(|(tab, _)| *tab == primary || secondary_allowed(*tab, primary)) {
                                option { key: "{_main_tab_to_str(tab)}", value: _main_tab_to_str(tab), selected: tab == secondary, "{label}" }
                            }
                        }
                    }
                    button { r#type: "button", class: "gs26-split-close", "aria-label": "Close second pane", onclick: move |_| { let mut next = preference.read().clone(); next.enabled = false; preference.set(next); }, "×" }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests;
