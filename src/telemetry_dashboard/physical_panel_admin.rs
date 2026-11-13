//! Administrator setting "Physical panel": whether the dual-meaning panel
//! buttons drive individual valves or run the GSE sequences. The station
//! decides what each button sends; this page shows and changes the choice.
use super::neutral_request_owner;
use dioxus::prelude::*;
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
enum PanelMode {
    Valves,
    Sequences,
}
impl PanelMode {
    fn label(self) -> &'static str {
        match self {
            PanelMode::Valves => "Individual valve control",
            PanelMode::Sequences => "Sequence control",
        }
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
struct ButtonRow {
    physical: String,
    pin: u8,
    valve_label: String,
    sequence_label: String,
    current_command: String,
    sequence_command: String,
    disabled_reason: Option<String>,
}
#[derive(Clone, Debug, Deserialize, PartialEq)]
struct FixedRow {
    physical: String,
    pin: u8,
    function: String,
}
#[derive(Clone, Debug, Deserialize, PartialEq)]
struct Status {
    panel_mode: Option<PanelMode>,
    effective_mode: PanelMode,
    station_default: PanelMode,
    #[serde(default)]
    buttons: Vec<ButtonRow>,
    #[serde(default)]
    fixed: Vec<FixedRow>,
    launch_only_note: Option<String>,
    change_blocked_reason: Option<String>,
    can_change: bool,
}
#[derive(Serialize)]
struct Update {
    panel_mode: Option<PanelMode>,
}
#[derive(Clone, Debug, Deserialize, PartialEq)]
struct PressInput {
    physical: String,
    pin: u8,
    pressed: Option<bool>,
}
#[derive(Clone, Debug, Deserialize, PartialEq)]
struct PressTest {
    gpio_available: bool,
    message: Option<String>,
    #[serde(default)]
    inputs: Vec<PressInput>,
}

/// Select value for a choice; "" follows the station default.
fn choice_value(mode: Option<PanelMode>) -> &'static str {
    match mode {
        None => "",
        Some(PanelMode::Valves) => "valves",
        Some(PanelMode::Sequences) => "sequences",
    }
}
fn parse_choice(value: &str) -> Option<PanelMode> {
    match value {
        "valves" => Some(PanelMode::Valves),
        "sequences" => Some(PanelMode::Sequences),
        _ => None,
    }
}

/// The inline reason the switch cannot be used now, if any.
fn switch_block(status: &Status) -> Option<&str> {
    status.change_blocked_reason.as_deref().or_else(|| {
        (!status.can_change).then_some("Change the physical panel on the primary GroundStation.")
    })
}

/// Names of the inputs pressed now, for the live indicator.
fn pressed_names(test: &PressTest) -> Vec<String> {
    test.inputs
        .iter()
        .filter(|input| input.pressed == Some(true))
        .map(|input| format!("{} (GPIO {})", input.physical, input.pin))
        .collect()
}

#[component]
pub(super) fn PhysicalPanelAdmin() -> Element {
    let authority = neutral_request_owner::use_factory();
    let mut status = use_signal(|| None::<Status>);
    let mut message = use_signal(String::new);
    let mut busy = use_signal(|| false);
    let mut testing = use_signal(|| false);
    let mut press = use_signal(|| None::<PressTest>);
    use_future(move || {
        authority.run(async move {
            loop {
                match authority.get::<Status>("/api/admin/physical-panel").await {
                    Ok(value) => status.set(Some(value)),
                    Err(error) => message.set(error),
                }
                super::sequence_progress_host::delay(3_000).await;
            }
        })
    });
    use_future(move || {
        authority.run(async move {
            loop {
                if *testing.peek() {
                    match authority.get::<PressTest>("/api/admin/physical-panel/press-test").await {
                        Ok(value) => press.set(Some(value)),
                        Err(error) => press.set(Some(PressTest { gpio_available: false, message: Some(error), inputs: Vec::new() })),
                    }
                    super::sequence_progress_host::delay(250).await;
                } else {
                    super::sequence_progress_host::delay(500).await;
                }
            }
        })
    });
    let Some(current) = status.read().clone() else {
        return rsx! {section {class:"station-positions-admin","aria-label":"Physical panel", h2 {"Physical panel"} p {"Loading…"} p {role:"status","{message}"}}};
    };
    let block = switch_block(&current).map(str::to_owned);
    let disabled = *busy.read() || block.is_some();
    let default_label = format!("Station default ({})", current.station_default.label());
    let selected = choice_value(current.panel_mode);
    let press_now = press.read().clone();
    rsx! {
        section {class:"station-positions-admin","aria-label":"Physical panel",id:"gs26-physical-panel",
            h2 {"Physical panel"}
            label {class:"admin-field",
                strong {"Panel buttons"}
                select {value:"{selected}",disabled,onchange:move |e|{
                    let wanted=parse_choice(&e.value());
                    if !authority.current(){return;}
                    busy.set(true);
                    authority.spawn(async move{
                        let (mut status, mut message, mut busy) = (status, message, busy);
                        match authority.post::<Update,Status>("/api/admin/physical-panel",&Update{panel_mode:wanted}).await {
                            Ok(value)=>{
                                message.set(format!("Panel buttons now use {}.", value.effective_mode.label().to_lowercase()));
                                status.set(Some(value));
                            }
                            Err(error)=>message.set(format!("Could not change the panel buttons. {error}")),
                        }
                        busy.set(false);
                    });
                },
                    option {value:"",selected:selected.is_empty(),"{default_label}"}
                    option {value:"valves",selected:selected=="valves","Individual valve control"}
                    option {value:"sequences",selected:selected=="sequences","Sequence control"}
                }
            }
            if let Some(reason)=block.as_ref() {
                p {class:"admin-note admin-error",role:"note","{reason}"}
            }
            p {class:"station-location-hint",
                "Now: " strong {"{current.effective_mode.label()}"}
                ". Applies in every station mode. Each press passes the same checks as the on-screen button for the command it sends. Changing it is refused while a sequence runs or the station is armed or flying."
            }
            if let Some(note)=current.launch_only_note.as_ref() {
                p {class:"admin-note",role:"note","{note}"}
            }
            table {class:"admin-panel-table",
                thead {tr {th {"Button"} th {"GPIO"} th {"Individual valve control"} th {"Sequence control"} th {"Sends now"}}}
                tbody {
                    for row in current.buttons.iter() {
                        tr {key:"{row.pin}",
                            td {"{row.physical}"} td {"{row.pin}"} td {"{row.valve_label}"} td {"{row.sequence_label}"}
                            td {
                                strong {if row.current_command==row.sequence_command {"{row.sequence_label}"} else {"{row.valve_label}"}}
                                if let Some(reason)=row.disabled_reason.as_ref() {
                                    span {class:"admin-note admin-error",style:"display:block;",role:"note","{reason}"}
                                }
                            }
                        }
                    }
                }
            }
            p {class:"station-location-hint", strong {"Never changed: "}
                {current.fixed.iter().map(|f| format!("{} (GPIO {}) — {}", f.physical, f.pin, f.function)).collect::<Vec<_>>().join("; ")}
            }
            label {style:"display:flex;gap:.6rem;align-items:center;min-height:44px;",
                input {r#type:"checkbox",checked:*testing.read(),style:"min-width:20px;min-height:20px;",onchange:move |e|{
                    testing.set(e.checked());
                    if !e.checked(){press.set(None);}
                }}
                span {"Press test: show which panel input is pressed"}
            }
            if *testing.read() {
                {match press_now {
                    None => rsx!{p {role:"status","Reading the panel…"}},
                    Some(test) if !test.gpio_available => rsx!{p {role:"status",{test.message.clone().unwrap_or_else(|| "No GPIO on this host.".into())}}},
                    Some(test) => {
                        let names = pressed_names(&test);
                        rsx!{p {role:"status","aria-live":"polite",
                            if names.is_empty() {"Nothing pressed."} else {strong {"Pressed: {names.join(\", \")}"}}
                        }}
                    }
                }}
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
    const BASE: &str = r#"{"panel_mode":null,"effective_mode":"sequences","station_default":"sequences","buttons":[{"physical":"Pilot","pin":13,"valve_command":"Pilot","valve_label":"Pilot valve","sequence_command":"ValveSelfTest","sequence_label":"Valve self-test","current_command":"ValveSelfTest","disabled_reason":null}],"fixed":[{"physical":"Abort","pin":7,"function":"Abort"}],"launch_only_note":null,"change_blocked_reason":null,"can_change":true,"updated_by":null,"updated_at_ms":null}"#;

    #[test]
    fn choice_round_trips_and_default_is_empty() {
        for mode in [None, Some(PanelMode::Valves), Some(PanelMode::Sequences)] {
            assert_eq!(parse_choice(choice_value(mode)), mode);
        }
        assert_eq!(serde_json::to_string(&Update { panel_mode: None }).unwrap(), r#"{"panel_mode":null}"#);
        assert_eq!(serde_json::to_string(&Update { panel_mode: Some(PanelMode::Valves) }).unwrap(), r#"{"panel_mode":"valves"}"#);
    }
    #[test]
    fn blocked_reason_is_shown_inline() {
        let idle = parse(BASE);
        assert_eq!(switch_block(&idle), None);
        let armed = Status { change_blocked_reason: Some("The station is Armed.".into()), ..idle.clone() };
        assert_eq!(switch_block(&armed), Some("The station is Armed."));
        let rrls = Status { can_change: false, ..idle };
        assert!(switch_block(&rrls).is_some_and(|r| r.contains("primary")));
    }
    #[test]
    fn press_indicator_lists_pressed_inputs() {
        let test: PressTest = serde_json::from_str(r#"{"gpio_available":true,"message":null,"inputs":[{"physical":"Pilot","pin":13,"pressed":true},{"physical":"Dump","pin":12,"pressed":false},{"physical":"Abort","pin":7,"pressed":null}]}"#).unwrap();
        assert_eq!(pressed_names(&test), ["Pilot (GPIO 13)"]);
    }
}
