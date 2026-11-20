//! Power off / reboot of the station this app is connected to (administrators).
use super::neutral_request_owner;
use dioxus::prelude::*;
use serde::Deserialize;

#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
pub(super) struct Blocker {
    pub reason: String,
    #[serde(default)]
    pub forceable: bool,
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
pub(super) struct PowerStatus {
    pub station_name: String,
    pub supported: bool,
    #[serde(default)]
    pub unsupported_reason: Option<String>,
    #[serde(default)]
    pub blocker: Option<Blocker>,
    #[serde(default)]
    pub in_progress: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Action {
    PowerOff,
    Reboot,
}
impl Action {
    pub(super) fn id(self) -> &'static str {
        match self {
            Self::PowerOff => "power_off",
            Self::Reboot => "reboot",
        }
    }
    pub(super) fn button(self, station: &str) -> String {
        match self {
            Self::PowerOff => format!("Power off {station}"),
            Self::Reboot => format!("Reboot {station}"),
        }
    }
    /// What the confirm dialog says will happen.
    pub(super) fn consequence(self, station: &str) -> String {
        let tail = match self {
            Self::PowerOff => "then the host shuts down. Someone must be at the station to switch it back on.",
            Self::Reboot => "then the host restarts. The station is offline for about a minute and recording resumes automatically.",
        };
        format!(
            "{station} will close every event capture (saving its manifest), close the current rolling recording segment, tell every connected client, and {tail} This applies only to {station}, the station this app is connected to."
        )
    }
}

#[component]
pub(super) fn StationPowerAdmin() -> Element {
    let authority = neutral_request_owner::use_factory();
    let mut status = use_signal(|| None::<PowerStatus>);
    let mut confirm = use_signal(|| None::<Action>);
    let mut force = use_signal(|| false);
    let mut message = use_signal(String::new);
    let mut busy = use_signal(|| false);
    let mut refresh = use_signal(|| 0u32);
    use_effect(move || {
        let _ = refresh();
        authority.spawn(async move {
            match authority.get::<PowerStatus>("/api/system/power").await {
                Ok(next) => status.set(Some(next)),
                Err(error) => message.set(format!("Could not read station power status: {error}")),
            }
        });
    });
    let Some(current) = status.read().clone() else {
        return rsx! { section { class: "admin-power", h2 { "Station power" } p { role: "status", "{message}" } } };
    };
    let station = current.station_name.clone();
    let blocked = current.blocker.clone();
    let can_request = current.supported && !current.in_progress && !busy();
    let mut send = move |action: Action, forced: bool| {
        busy.set(true);
        let mut body = serde_json::json!({"action": action.id(), "confirm": true});
        if forced {
            body["force"] = "FORCE".into();
        }
        authority.spawn(async move {
            match authority.post::<_, serde_json::Value>("/api/system/power", &body).await {
                Ok(reply) => message.set(reply["message"].as_str().unwrap_or("Request accepted.").to_owned()),
                Err(error) => message.set(format!("Refused: {error}")),
            }
            confirm.set(None);
            force.set(false);
            busy.set(false);
            refresh += 1;
        });
    };
    rsx! {
        section { class: "admin-power", "aria-labelledby": "admin-power-title",
            h2 { id: "admin-power-title", "Station power — {station}" }
            p { class: "admin-hint", "Cleanly powers off or reboots the {station} host. Recordings are closed first. Refused from Armed through landing, during a fill or station sequence, and while a launch is being captured." }
            if !current.supported {
                p { role: "status", {current.unsupported_reason.clone().unwrap_or_else(|| "Not supported here.".into())} }
            }
            if let Some(b) = blocked.clone() {
                p { role: "alert", class: "admin-error", "Not safe now: {b.reason}" }
            }
            if current.in_progress { p { role: "status", "A power request is in progress." } }
            div { style: "display:flex;gap:8px;flex-wrap:wrap;",
                for action in [Action::Reboot, Action::PowerOff] {
                    button {
                        key: "{action.id()}",
                        disabled: !can_request || blocked.as_ref().is_some_and(|b| !b.forceable),
                        onclick: move |_| confirm.set(Some(action)),
                        {action.button(&station)}
                    }
                }
            }
            if let Some(action) = confirm() {
                div { role: "alertdialog", "aria-modal": "true", "aria-labelledby": "admin-power-confirm",
                    class: "admin-power-confirm",
                    h3 { id: "admin-power-confirm", {format!("{}?", action.button(&station))} }
                    p { {action.consequence(&station)} }
                    if let Some(b) = blocked.clone().filter(|b| b.forceable) {
                        p { role: "alert", "The station reports this is not safe: {b.reason}" }
                        label {
                            input { r#type: "checkbox", checked: force(), onchange: move |e| force.set(e.checked()) }
                            "FORCE anyway. I accept that this interrupts the activity above; the override is logged with my name."
                        }
                    }
                    div { style: "display:flex;gap:8px;",
                        button {
                            disabled: busy() || (blocked.is_some() && !force()),
                            onclick: move |_| send(action, force()),
                            {format!("Yes, {}", action.button(&station).to_lowercase())}
                        }
                        button { onclick: move |_| { confirm.set(None); force.set(false); }, "Cancel" }
                    }
                }
            }
            p { role: "status", "{message}" }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn confirm_copy_names_the_station_and_the_consequences() {
        let text = Action::PowerOff.consequence("RRLS");
        assert!(text.contains("RRLS will close every event capture"));
        assert!(text.contains("applies only to RRLS"));
        assert!(Action::Reboot.consequence("Ground Station").contains("recording resumes automatically"));
        assert_eq!(Action::Reboot.button("Ground Station"), "Reboot Ground Station");
        assert_eq!(Action::PowerOff.id(), "power_off");
    }

    #[test]
    fn power_status_decodes_unsupported_and_blocked_states() {
        let status: PowerStatus = serde_json::from_value(serde_json::json!({
            "station_name": "Ground Station", "supported": false, "mechanism": "unsupported",
            "unsupported_reason": "Not supported here: the host power helper is not installed",
            "blocker": {"reason": "The vehicle is Armed.", "forceable": false}, "in_progress": false
        }))
        .unwrap();
        assert!(!status.supported);
        assert!(!status.blocker.unwrap().forceable);
    }

    #[test]
    fn panel_renders_admin_buttons_and_the_confirm_dialog() {
        #[component]
        fn Harness() -> Element {
            let station = "Ground Station";
            rsx! {
                for action in [Action::Reboot, Action::PowerOff] { button { {action.button(station)} } }
                p { {Action::Reboot.consequence(station)} }
            }
        }
        let mut dom = VirtualDom::new(Harness);
        dom.rebuild_in_place();
        let html = dioxus::ssr::render(&dom);
        assert!(html.contains("Reboot Ground Station") && html.contains("Power off Ground Station"));
    }
}
