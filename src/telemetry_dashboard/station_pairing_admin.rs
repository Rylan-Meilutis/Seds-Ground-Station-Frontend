//! Pair the primary GroundStation and the RRLS from the UI, over their radio link.
//! Both admins compare a six-digit code; nothing is saved until both confirm.
use super::neutral_request_owner;
use dioxus::prelude::*;
use serde::Deserialize;

#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
struct Peer {
    id: String,
    #[serde(default)]
    urls: Vec<String>,
}
#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
struct Exchange {
    phase: String,
    code: Option<String>,
    expires_in_ms: u64,
    local_confirmed: bool,
    peer_confirmed: bool,
    peer_url: Option<String>,
    message: String,
}
#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
struct PrimaryLink {
    connected: bool,
    available: bool,
    #[serde(default)]
    reason: String,
}
#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
struct Status {
    role: String,
    paired: bool,
    source: Option<String>,
    error: Option<String>,
    #[serde(default)]
    peers: Vec<Peer>,
    fingerprint: Option<String>,
    restart_required: bool,
    can_manage: bool,
    radio_available: bool,
    unpair_available: bool,
    exchange: Option<Exchange>,
    primary_link: Option<PrimaryLink>,
}

fn other(role: &str) -> &'static str {
    if role == "rrls" { "primary GroundStation" } else { "RRLS" }
}
fn window_open(status: &Status) -> bool {
    status
        .exchange
        .as_ref()
        .is_some_and(|e| matches!(e.phase.as_str(), "waiting" | "compare" | "awaiting_peer"))
}
/// The headline state and its explanation, in plain terms.
fn summary(status: &Status) -> (String, String) {
    if let Some(error) = &status.error {
        return (
            "Not paired: the pairing is invalid".into(),
            format!("{error}. Login and normal operation run unpaired."),
        );
    }
    if !status.paired {
        return (
            "Not paired".into(),
            format!(
                "Without pairing the {} cannot send signed commands, sync settings or hand over sign-ins.",
                if status.role == "rrls" { "RRLS" } else { "pair" }
            ),
        );
    }
    let peer = status
        .peers
        .first()
        .map(|p| {
            let url = p.urls.first().map(|u| format!(" at {u}")).unwrap_or_default();
            format!("{}{url}", if p.id == "rrls" { "RRLS" } else { "primary GroundStation" })
        })
        .unwrap_or_else(|| other(&status.role).into());
    let mut detail = format!(
        "Paired with the {peer}. Identity {}.",
        status.fingerprint.as_deref().unwrap_or("unknown")
    );
    if status.source.as_deref() == Some("environment") {
        detail.push_str(" Set by GS_STATION_PAIRING_FILE.");
    }
    if let Some(link) = &status.primary_link {
        detail.push_str(if link.available {
            " The primary accepts signed requests."
        } else if link.connected {
            " The primary is heard but signed control is not available yet."
        } else {
            " The primary is not heard right now."
        });
    }
    if status.restart_required {
        detail.push_str(" Restart this station's service to activate the change.");
    }
    ("Paired".into(), detail)
}

fn send(
    authority: neutral_request_owner::Factory,
    mut status: Signal<Option<Status>>,
    mut message: Signal<String>,
    mut busy: Signal<bool>,
    path: &'static str,
    body: serde_json::Value,
    done: &'static str,
) {
    busy.set(true);
    authority.spawn(async move {
        match authority.post::<serde_json::Value, Status>(path, &body).await {
            Ok(value) => {
                status.set(Some(value));
                message.set(done.into());
            }
            Err(error) => message.set(error),
        }
        busy.set(false);
    });
}

#[component]
pub(super) fn StationPairingAdmin() -> Element {
    let authority = neutral_request_owner::use_factory();
    let mut status = use_signal(|| None::<Status>);
    let mut message = use_signal(String::new);
    let busy = use_signal(|| false);
    let mut own_url = use_signal(super::UrlConfig::base_http);
    let mut code = use_signal(String::new);
    let mut confirm_unpair = use_signal(|| false);
    use_future(move || {
        authority.run(async move {
            loop {
                match authority.get::<Status>("/api/station/pairing").await {
                    Ok(value) => {
                        let open = window_open(&value);
                        status.set(Some(value));
                        super::sequence_progress_host::delay(if open { 1_000 } else { 5_000 }).await;
                    }
                    Err(error) => {
                        message.set(error);
                        super::sequence_progress_host::delay(5_000).await;
                    }
                }
            }
        })
    });
    let post = move |path: &'static str, body: serde_json::Value, done: &'static str| {
        send(authority, status, message, busy, path, body, done)
    };
    let Some(current) = status.read().clone() else {
        return rsx! {section {class:"station-positions-admin","aria-label":"Station pairing", h2 {"Station pairing"} p {"Loading…"} p {role:"status","{message}"}}};
    };
    let (headline, detail) = summary(&current);
    let open = window_open(&current);
    let peer_name = other(&current.role);
    let exchange = current.exchange.clone();
    rsx! {
        section {class:"station-positions-admin station-pairing-admin","aria-label":"Station pairing",
            h2 {"Station pairing"}
            p {strong {"{headline}"}}
            p {class:"station-location-hint","{detail}"}
            if let Some(e) = exchange.as_ref() {
                div {role:"status",class:"station-pairing-exchange",
                    p {"{e.message}"}
                    if let Some(code_text) = e.code.as_ref() {
                        p {class:"station-pairing-code","aria-label":"Verification code","{code_text}"}
                        p {class:"station-location-hint",
                            "Check that the {peer_name} shows exactly this code. "
                            if e.peer_confirmed {"The other station has confirmed. "}
                        }
                    }
                    if open {p {class:"station-location-hint","Window closes in {e.expires_in_ms / 1000} s."}}
                }
            }
            if current.can_manage {
                if !current.paired && current.error.is_none() && !open {
                    if current.radio_available {
                        label {"This station's address, as apps reach it"
                            input {r#type:"url",value:own_url.read().clone(),disabled:*busy.read(),oninput:move|e|own_url.set(e.value())}}
                        div {class:"station-location-actions",
                            button {disabled:*busy.read(),onclick:move |_|{
                                let url=own_url.read().trim().to_string();
                                code.set(String::new());
                                post("/api/station/pairing/start",serde_json::json!({"own_url":url}),"Pairing window open.");
                            },"Start pairing with the {peer_name}"}
                        }
                        p {class:"station-location-hint","Start pairing on both stations within five minutes. They talk over the radio, so the RRLS must hear the primary."}
                    } else {
                        p {class:"station-location-hint","Pairing from the UI needs the GroundStation–RRLS radio role on this station."}
                    }
                }
                if open {
                    if exchange.as_ref().is_some_and(|e|e.code.is_some() && !e.local_confirmed) {
                        label {"Type the code you compared"
                            input {inputmode:"numeric",autocomplete:"off",maxlength:"7",value:code.read().clone(),disabled:*busy.read(),oninput:move|e|code.set(e.value())}}
                    }
                    div {class:"station-location-actions",
                        if exchange.as_ref().is_some_and(|e|e.code.is_some() && !e.local_confirmed) {
                            button {disabled:*busy.read(),onclick:move |_|{
                                let typed=code.read().clone();
                                post("/api/station/pairing/confirm",serde_json::json!({"code":typed}),"Code confirmed on this station.");
                            },"Codes match: confirm"}
                        }
                        button {disabled:*busy.read(),onclick:move |_|post("/api/station/pairing/cancel",serde_json::json!({}),"Pairing cancelled; nothing was saved on this station."),"Cancel"}
                    }
                }
                if current.unpair_available {
                    div {class:"station-location-actions",
                        if *confirm_unpair.read() {
                            p {class:"station-location-hint","Unpairing deletes this station's identity. Signed control, sync and portable sign-ins stop; sessions signed with it must sign in again. Unpair the {peer_name} too."}
                            button {disabled:*busy.read(),onclick:move |_|{confirm_unpair.set(false);post("/api/station/pairing/unpair",serde_json::json!({}),"Unpaired. Restart this station's service to stop paired features.");},"Unpair this station"}
                            button {disabled:*busy.read(),onclick:move |_|confirm_unpair.set(false),"Keep pairing"}
                        } else {
                            button {disabled:*busy.read(),onclick:move |_|confirm_unpair.set(true),"Unpair…"}
                        }
                    }
                }
            } else if !current.paired {
                p {class:"station-location-hint","An application administrator can pair this station."}
            }
            p {role:"status","{message}"}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn parse(json: &str) -> Status {
        serde_json::from_str(json).unwrap()
    }
    #[test]
    fn states_are_named_plainly_and_broken_pairing_is_not_paired() {
        let unpaired = parse(r#"{"role":"rrls","paired":false,"source":null,"error":null,"peers":[],"fingerprint":null,"restart_required":false,"can_manage":true,"radio_available":true,"unpair_available":false,"exchange":null,"primary_link":{"connected":true,"available":false,"reason":""}}"#);
        assert_eq!(summary(&unpaired).0, "Not paired");
        assert!(!window_open(&unpaired));
        let broken = parse(r#"{"role":"primary","paired":false,"source":"ui","error":"invalid station pairing identity","peers":[],"fingerprint":null,"restart_required":false,"can_manage":true,"radio_available":true,"unpair_available":true,"exchange":null,"primary_link":null}"#);
        let (head, detail) = summary(&broken);
        assert!(head.starts_with("Not paired"));
        assert!(detail.contains("run unpaired"));
        let paired = parse(r#"{"role":"primary","paired":true,"source":"ui","error":null,"peers":[{"id":"rrls","role":"rrls","urls":["http://rrls.local:3000"]}],"fingerprint":"AB12-CD34-EF56","restart_required":true,"can_manage":true,"radio_available":true,"unpair_available":true,"exchange":{"phase":"saved","code":"123 456","expires_in_ms":1000,"local_confirmed":true,"peer_confirmed":true,"peer_url":"http://rrls.local:3000","message":"Paired."},"primary_link":null}"#);
        let (head, detail) = summary(&paired);
        assert_eq!(head, "Paired");
        assert!(detail.contains("RRLS at http://rrls.local:3000"));
        assert!(detail.contains("AB12-CD34-EF56"));
        assert!(detail.contains("Restart"));
        assert!(!window_open(&paired), "a saved exchange no longer offers confirm or cancel");
        let comparing = parse(r#"{"role":"rrls","paired":false,"source":null,"error":null,"peers":[],"fingerprint":null,"restart_required":false,"can_manage":true,"radio_available":true,"unpair_available":false,"exchange":{"phase":"compare","code":"123 456","expires_in_ms":250000,"local_confirmed":false,"peer_confirmed":false,"peer_url":"https://gs.local","message":"Compare"},"primary_link":null}"#);
        assert!(window_open(&comparing));
        assert_eq!(other(&comparing.role), "primary GroundStation");
    }
}
