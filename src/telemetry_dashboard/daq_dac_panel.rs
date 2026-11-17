//! DAQ signal generator / ADC validation. Drives the DAQ's DAC1 outputs on
//! P10 through the GS (`/api/daq/dac*`), plots expected against measured for
//! a looped-back input and runs the DC sweep. The GS owns gating, clamping
//! and the keepalive lease; this view only requests and reports.
use super::daq_dac_model::{
    CHANNEL_LABELS, ConfigDraft, DacChannel, DacInput, DacStatus, Polarity, SweepReport,
    SweepStatus, Validation, ValidationAnalysis, Waveform, accepted_reply, clamped_text,
    config_body, daq_state_text, disable_block_reason, disable_body, disable_reason_text,
    enable_block_reason, fmt_opt, operator_error, plot_geometry, range_hint, result_text,
    status_age_text, sweep_body, sweep_progress_text, validation_body,
};
use super::data_chart::{SeriesSwatch, series_color};
use super::layout::ThemeConfig;
use dioxus::prelude::*;
use serde_json::Value;

const COMMAND: &str = "DaqDacControl";
const PLOT_W: f64 = 640.0;
const PLOT_H: f64 = 220.0;
const PLOT_PAD: f64 = 12.0;

async fn sleep_ms(ms: u32) {
    #[cfg(target_arch = "wasm32")]
    gloo_timers::future::TimeoutFuture::new(ms).await;
    #[cfg(not(target_arch = "wasm32"))]
    tokio::time::sleep(std::time::Duration::from_millis(ms as u64)).await;
}

/// Posts with this view's ordinary owner; a response for a retired view is dropped.
fn post_json(
    path: &'static str,
    body: Value,
    mut done: impl FnMut(Result<Value, String>) + 'static,
) {
    let owner = match super::recording_download::ExportOwner::capture_ordinary_selected(|| true) {
        Ok(owner) => owner,
        Err(error) => {
            done(Err(error));
            return;
        }
    };
    let request =
        super::station_settings_request::post_captured::<_, Value>(path, &body, Ok(owner.clone()));
    spawn(async move {
        let result = request.await;
        if !owner.is_current() {
            return;
        }
        done(accepted_reply(result));
    });
}

fn rrls() -> bool {
    crate::auth::current_status().station_role.as_deref() == Some("rrls")
}

fn button_style(theme: &ThemeConfig) -> String {
    format!(
        "min-height:44px; padding:8px 14px; border-radius:8px; border:1px solid {}; background:{}; color:{}; font:inherit; cursor:pointer;",
        theme.border, theme.button_background, theme.text_primary
    )
}

fn danger_button_style(theme: &ThemeConfig) -> String {
    format!(
        "min-height:44px; padding:8px 14px; border-radius:8px; border:2px solid {}; background:{}; color:{}; font:inherit; font-weight:700; cursor:pointer;",
        theme.error_border, theme.error_background, theme.error_text
    )
}

fn field_style(theme: &ThemeConfig) -> String {
    format!(
        "padding:8px; border-radius:6px; background:{}; color:{}; border:1px solid {}; font:inherit; max-width:100%; box-sizing:border-box; min-width:0;",
        theme.button_background, theme.text_primary, theme.border
    )
}

#[component]
pub(super) fn DaqDacPanel(theme: ThemeConfig) -> Element {
    let expanded = use_signal(|| false);
    let mut status = use_signal(|| None::<DacStatus>);
    let mut status_error = use_signal(|| None::<String>);
    let mut validation = use_signal(|| None::<Validation>);
    let mut validation_error = use_signal(|| None::<String>);
    let mut sweep = use_signal(|| None::<SweepStatus>);
    let mut drafts = use_signal(|| [ConfigDraft::default(), ConfigDraft::default()]);
    let mut drafts_loaded = use_signal(|| [false, false]);
    let mut target = use_signal(String::new);
    let mut val_channel = use_signal(|| 0_u8);
    let mut attenuation = use_signal(|| "1.0".to_string());
    let mut polarity = use_signal(|| Polarity::SMinus);
    let mut target_loaded = use_signal(|| false);

    // Status drives the active banner even while collapsed; validation and
    // sweep are read only while the panel is open.
    use_future(move || async move {
        loop {
            let open = *expanded.peek();
            match super::http_get_json::<DacStatus>("/api/daq/dac").await {
                Ok(next) => {
                    let mut loaded = *drafts_loaded.peek();
                    let mut next_drafts = drafts.peek().clone();
                    for channel in &next.channels {
                        let i = channel.channel as usize;
                        if i < 2 && !loaded[i] {
                            next_drafts[i] = ConfigDraft::from_channel(channel);
                            loaded[i] = true;
                        }
                    }
                    if loaded != *drafts_loaded.peek() {
                        drafts.set(next_drafts);
                        drafts_loaded.set(loaded);
                    }
                    status.set(Some(next));
                    status_error.set(None);
                }
                Err(error) => status_error.set(Some(operator_error(&error))),
            }
            if open {
                match super::http_get_json::<Validation>("/api/daq/dac/validation").await {
                    Ok(next) => {
                        if !*target_loaded.peek() {
                            target.set(next.target.clone().unwrap_or_default());
                            if let Some(channel) = next.channel.filter(|c| *c < 2) {
                                val_channel.set(channel);
                            }
                            if let Some(k) = next.attenuation.filter(|k| k.is_finite() && *k > 0.0)
                            {
                                attenuation.set(format!("{k}"));
                            }
                            if let Some(p) = next.load_cell_polarity.as_deref() {
                                polarity.set(Polarity::from_wire(p));
                            }
                            target_loaded.set(true);
                        }
                        validation.set(Some(next));
                        validation_error.set(None);
                    }
                    Err(error) => validation_error.set(Some(operator_error(&error))),
                }
                if let Ok(next) = super::http_get_json::<SweepStatus>("/api/daq/dac/sweep").await {
                    sweep.set(Some(next));
                }
            }
            sleep_ms(if open { 500 } else { 2000 }).await;
        }
    });

    let state = status.read().clone();
    let permitted = crate::auth::can_send_command(COMMAND);
    let rrls = rrls();
    let enable_block = enable_block_reason(state.as_ref(), permitted, rrls);
    let disable_block = disable_block_reason(permitted, rrls);
    let any_active = state.as_ref().is_some_and(|s| s.any_active);
    let banner_message = use_signal(String::new);
    let is_open = expanded();
    let border = if any_active {
        theme.error_border.clone()
    } else {
        theme.border_soft.clone()
    };

    rsx! {
        section { class: "gs26-daq-dac-panel", "aria-label": "DAQ signal generator and ADC validation",
            style: "padding:12px; border:1px solid {border}; border-radius:10px; background:{theme.panel_background}; color:{theme.text_primary}; display:flex; flex-direction:column; gap:10px; min-width:0; overflow-wrap:anywhere;",
            if any_active {
                ActiveBanner {
                    theme: theme.clone(),
                    channels: state.as_ref().map(|s| s.channels.clone()).unwrap_or_default(),
                    disable_block: disable_block.clone(),
                    message: banner_message,
                }
            }
            div { style: "display:flex; align-items:center; justify-content:space-between; gap:8px; flex-wrap:wrap;",
                h3 { style: "margin:0; font-size:16px;", "Signal generator / ADC validation" }
                button { r#type: "button", style: "{button_style(&theme)}",
                    "aria-expanded": if is_open { "true" } else { "false" },
                    onclick: move |_| { let mut expanded = expanded; let next = !expanded(); expanded.set(next); },
                    if is_open { "Hide" } else { "Show" }
                }
            }
            if let Some(error) = status_error() {
                p { role: "alert", style: "margin:0; color:{theme.error_text};", "DAQ DAC status unavailable: {error}" }
            }
            if is_open {
                if let Some(state) = state.clone() {
                    StatusSummary { theme: theme.clone(), state: state.clone(), enable_block: enable_block.clone() }
                    div { style: "display:grid; grid-template-columns:repeat(auto-fit, minmax(min(100%, 320px), 1fr)); gap:10px;",
                        for index in 0..2_usize {
                            ChannelCard {
                                key: "{index}",
                                theme: theme.clone(),
                                index,
                                channel: state.channels.iter().find(|c| c.channel as usize == index).cloned(),
                                connector_range: state.connector_range_v,
                                drafts,
                                enable_block: enable_block.clone(),
                                disable_block: disable_block.clone(),
                            }
                        }
                    }
                    ValidationSection {
                        theme: theme.clone(),
                        inputs: state.inputs.clone(),
                        validation: validation(),
                        validation_error: validation_error(),
                        sweep: sweep(),
                        target, val_channel, attenuation, polarity,
                        enable_block: enable_block.clone(),
                        disable_block: disable_block.clone(),
                    }
                } else if status_error().is_none() {
                    p { role: "status", style: "margin:0; color:{theme.text_secondary};", "Loading DAQ DAC status…" }
                }
            }
        }
    }
}

#[component]
fn ActiveBanner(
    theme: ThemeConfig,
    channels: Vec<DacChannel>,
    disable_block: Option<String>,
    message: Signal<String>,
) -> Element {
    let mut message = message;
    let active: Vec<String> = channels
        .iter()
        .filter(|c| c.active)
        .map(|c| {
            let label = CHANNEL_LABELS
                .get(c.channel as usize)
                .copied()
                .unwrap_or(c.label.as_str());
            format!(
                "{label}: {}",
                Waveform::from_wire(&c.waveform)
                    .map(Waveform::label)
                    .unwrap_or(c.waveform.as_str())
            )
        })
        .collect();
    rsx! {
        div { role: "alert", class: "gs26-daq-dac-active",
            style: "padding:12px; border-radius:8px; border:2px solid {theme.error_border}; background:{theme.error_background}; color:{theme.error_text}; display:flex; flex-direction:column; gap:6px;",
            strong { style: "font-size:18px; letter-spacing:0.06em;", "DAC OUTPUT ACTIVE" }
            for line in active {
                span { "{line}" }
            }
            div { style: "display:flex; gap:8px; flex-wrap:wrap; align-items:center;",
                button { r#type: "button", style: "{danger_button_style(&theme)}",
                    disabled: disable_block.is_some(),
                    onclick: move |_| {
                        message.set("Disabling all DAC outputs…".into());
                        post_json("/api/daq/dac/disable", disable_body(None), move |result| {
                            message.set(match result {
                                Ok(_) => "Disable sent for all DAC outputs.".into(),
                                Err(error) => format!("Disable failed: {error}"),
                            });
                        });
                    },
                    "Disable all outputs"
                }
                if let Some(reason) = &disable_block {
                    span { style: "font-size:12px;", "Disable unavailable: {reason}" }
                }
            }
            if !message().is_empty() {
                span { role: "status", "{message}" }
            }
        }
    }
}

#[component]
fn StatusSummary(theme: ThemeConfig, state: DacStatus, enable_block: Option<String>) -> Element {
    let age_color = if state.status_fresh {
        theme.text_secondary.clone()
    } else {
        theme.warning_text.clone()
    };
    let gs_state = state
        .flight_state
        .clone()
        .unwrap_or_else(|| "unknown".into());
    let range = state.connector_range_v.map(|[lo, hi]| {
        let pin = state
            .dac_pin_range_v
            .map(|[a, b]| format!(" (DAC pin {a:.2} to {b:.2} V × buffer gain {})", fmt_opt(state.buffer_gain, 1)))
            .unwrap_or_default();
        format!("Reachable at P10: {lo:.2} to {hi:.2} V{pin}. Values are P10 connector volts, before any external attenuator.")
    });
    rsx! {
        div { style: "display:flex; flex-direction:column; gap:4px; font-size:13px;",
            span { "GS flight state: {gs_state} · DAQ sees: {daq_state_text(state.daq_flight_state)}" }
            span { role: if state.status_fresh { "status" } else { "alert" }, style: "color:{age_color};", "{status_age_text(&state)}" }
            if let Some(range) = range {
                span { style: "color:{theme.text_secondary};", "{range}" }
            }
            if let Some(reason) = enable_block {
                p { role: "status",
                    style: "margin:4px 0 0; padding:8px; border-radius:6px; border:1px solid {theme.warning_border}; background:{theme.warning_background}; color:{theme.warning_text};",
                    "Output changes unavailable: {reason} Disable stays available."
                }
            }
        }
    }
}

#[component]
fn ChannelCard(
    theme: ThemeConfig,
    index: usize,
    channel: Option<DacChannel>,
    connector_range: Option<[f64; 2]>,
    drafts: Signal<[ConfigDraft; 2]>,
    enable_block: Option<String>,
    disable_block: Option<String>,
) -> Element {
    let mut drafts = drafts;
    let mut message = use_signal(String::new);
    let mut busy = use_signal(|| false);
    let draft = drafts.read()[index].clone();
    let fields = draft.waveform.fields();
    let channel_id = index as u8;
    let label = CHANNEL_LABELS[index];
    let active = channel.as_ref().is_some_and(|c| c.active);
    let hint = range_hint(&draft, connector_range);
    let field = field_style(&theme);
    let set = move |apply: fn(&mut ConfigDraft, String)| {
        move |event: FormEvent| {
            let mut drafts = drafts;
            let mut all = drafts.write();
            apply(&mut all[index], event.value());
        }
    };
    let enable_disabled = enable_block.is_some() || busy();
    let applied = channel.clone().unwrap_or_default();
    let applied_wave = Waveform::from_wire(&applied.waveform);
    let card_border = if active {
        theme.error_border.clone()
    } else {
        theme.border.clone()
    };
    rsx! {
        div { style: "padding:10px; border-radius:8px; border:1px solid {card_border}; background:{theme.panel_background_alt}; display:flex; flex-direction:column; gap:8px; min-width:0;",
            div { style: "display:flex; justify-content:space-between; gap:8px; flex-wrap:wrap;",
                strong { "{label}" }
                if active {
                    span { style: "color:{theme.error_text}; font-weight:700;", "ACTIVE" }
                } else {
                    span { style: "color:{theme.text_secondary};", "off" }
                }
            }
            if channel.is_some() {
                dl { style: "display:grid; grid-template-columns:auto 1fr; gap:2px 10px; margin:0; font-size:12px; color:{theme.text_secondary};",
                    dt { "Applied" }
                    dd { style: "margin:0;",
                        {
                            let wave = applied_wave.map(Waveform::label).unwrap_or(applied.waveform.as_str());
                            let detail = match applied_wave {
                                Some(Waveform::Dc) | None => format!("{wave} {:.3} V", applied.offset_v),
                                Some(Waveform::Staircase) => format!("{wave} {:.3} ± {:.3} V, {} steps × {} ms", applied.offset_v, applied.amplitude_v, applied.steps, applied.dwell_ms),
                                Some(_) => format!("{wave} {:.3} ± {:.3} V at {:.3} Hz", applied.offset_v, applied.amplitude_v, applied.frequency_hz),
                            };
                            rsx! { "{detail}" }
                        }
                    }
                    dt { "Output / readback" }
                    dd { style: "margin:0;", "{fmt_opt(applied.output_v, 3)} V / {fmt_opt(applied.readback_v, 3)} V" }
                    dt { "Last result" }
                    dd { style: "margin:0;", "{result_text(&applied.last_result)}" }
                    dt { "Last off reason" }
                    dd { style: "margin:0;", "{disable_reason_text(&applied.disable_reason)}" }
                    dt { "Sequence" }
                    dd { style: "margin:0;", "{applied.sequence}" }
                }
                if let Some(text) = clamped_text(&applied.clamped) {
                    span { role: "status", style: "font-size:12px; color:{theme.warning_text};", "{text}" }
                }
            } else {
                span { style: "font-size:12px; color:{theme.text_muted};", "Not reported by the DAQ yet." }
            }
            div { style: "display:grid; grid-template-columns:repeat(auto-fit, minmax(min(100%, 130px), 1fr)); gap:6px;",
                label { style: "display:flex; flex-direction:column; gap:2px; font-size:12px;",
                    "Waveform"
                    select { style: "{field}", disabled: enable_disabled,
                        value: "{draft.waveform.wire()}",
                        onchange: move |event| {
                            if let Some(w) = Waveform::from_wire(&event.value()) {
                                drafts.write()[index].waveform = w;
                            }
                        },
                        for w in Waveform::ALL {
                            option { value: "{w.wire()}", selected: w == draft.waveform, "{w.label()}" }
                        }
                    }
                }
                if fields.offset {
                    label { style: "display:flex; flex-direction:column; gap:2px; font-size:12px;",
                        if draft.waveform == Waveform::Dc { "Level (V)" } else { "Offset / centre (V)" }
                        input { r#type: "number", step: "any", style: "{field}", disabled: enable_disabled,
                            value: "{draft.offset_v}", oninput: set(|d, v| d.offset_v = v) }
                    }
                }
                if fields.amplitude {
                    label { style: "display:flex; flex-direction:column; gap:2px; font-size:12px;",
                        "Amplitude, peak (V)"
                        input { r#type: "number", step: "any", min: "0", style: "{field}", disabled: enable_disabled,
                            value: "{draft.amplitude_v}", oninput: set(|d, v| d.amplitude_v = v) }
                    }
                }
                if fields.frequency {
                    label { style: "display:flex; flex-direction:column; gap:2px; font-size:12px;",
                        "Frequency (Hz)"
                        input { r#type: "number", step: "any", min: "0.01", max: "1000", style: "{field}", disabled: enable_disabled,
                            value: "{draft.frequency_hz}", oninput: set(|d, v| d.frequency_hz = v) }
                    }
                }
                if fields.steps {
                    label { style: "display:flex; flex-direction:column; gap:2px; font-size:12px;",
                        "Steps (2–32)"
                        input { r#type: "number", step: "1", min: "2", max: "32", style: "{field}", disabled: enable_disabled,
                            value: "{draft.steps}", oninput: set(|d, v| d.steps = v) }
                    }
                }
                if fields.dwell {
                    label { style: "display:flex; flex-direction:column; gap:2px; font-size:12px;",
                        "Dwell per step (ms)"
                        input { r#type: "number", step: "1", min: "10", max: "20000", style: "{field}", disabled: enable_disabled,
                            value: "{draft.dwell_ms}", oninput: set(|d, v| d.dwell_ms = v) }
                    }
                }
            }
            if let Some(hint) = hint {
                span { style: "font-size:12px; color:{theme.warning_text};", "{hint}" }
            }
            div { style: "display:flex; gap:8px; flex-wrap:wrap;",
                button { r#type: "button", style: "{button_style(&theme)}", disabled: enable_disabled,
                    onclick: move |_| {
                        if busy() { return; }
                        let body = match config_body(channel_id, &drafts.peek()[index]) {
                            Ok(body) => body,
                            Err(error) => { message.set(error); return; }
                        };
                        busy.set(true);
                        message.set("Sending configuration…".into());
                        post_json("/api/daq/dac/config", body, move |result| {
                            message.set(match result {
                                Ok(reply) => {
                                    let mut text = reply.get("message").and_then(Value::as_str)
                                        .map(str::to_owned)
                                        .unwrap_or_else(|| "Configuration accepted; the GS keeps it alive.".into());
                                    let clamped: Vec<String> = reply.get("clamped").and_then(Value::as_array)
                                        .map(|a| a.iter().filter_map(|v| v.as_str().map(str::to_owned)).collect())
                                        .unwrap_or_default();
                                    if let Some(c) = clamped_text(&clamped) {
                                        text = format!("{text} GS {c}.");
                                    }
                                    text
                                }
                                Err(error) => format!("Refused: {error}"),
                            });
                            busy.set(false);
                        });
                    },
                    if active { "Apply" } else { "Enable output" }
                }
                button { r#type: "button", style: "{danger_button_style(&theme)}", disabled: disable_block.is_some(),
                    onclick: move |_| {
                        message.set("Disabling…".into());
                        post_json("/api/daq/dac/disable", disable_body(Some(channel_id)), move |result| {
                            message.set(match result {
                                Ok(_) => "Disable sent.".into(),
                                Err(error) => format!("Disable failed: {error}"),
                            });
                        });
                    },
                    "Disable"
                }
            }
            if let Some(reason) = &disable_block {
                span { style: "font-size:12px; color:{theme.text_secondary};", "Disable unavailable: {reason}" }
            }
            if !message().is_empty() {
                span { role: "status", style: "font-size:13px;", "{message}" }
            }
        }
    }
}

fn input_summary(input: &DacInput) -> String {
    let unit = if input.unit.is_empty() {
        "V"
    } else {
        input.unit.as_str()
    };
    let range = input
        .valid_range
        .map(|[lo, hi]| format!("Valid {lo} to {hi} {unit}"))
        .unwrap_or_else(|| "Valid range not reported".into());
    let rate = input
        .stream_hz
        .map(|hz| format!(" · stream about {hz} Hz"))
        .unwrap_or_default();
    format!("{range}{rate}")
}

#[component]
fn ValidationSection(
    theme: ThemeConfig,
    inputs: Vec<DacInput>,
    validation: Option<Validation>,
    validation_error: Option<String>,
    sweep: Option<SweepStatus>,
    target: Signal<String>,
    val_channel: Signal<u8>,
    attenuation: Signal<String>,
    polarity: Signal<Polarity>,
    enable_block: Option<String>,
    disable_block: Option<String>,
) -> Element {
    let (mut target, mut val_channel, mut attenuation, mut polarity) =
        (target, val_channel, attenuation, polarity);
    let mut message = use_signal(String::new);
    let mut sweep_message = use_signal(String::new);
    let mut sweep_steps = use_signal(String::new);
    let mut sweep_dwell = use_signal(String::new);
    let selected_id = target();
    let selected = inputs.iter().find(|i| i.id == selected_id).cloned();
    let unit = selected
        .as_ref()
        .map(|i| {
            if i.unit.is_empty() {
                "V".to_string()
            } else {
                i.unit.clone()
            }
        })
        .unwrap_or_else(|| "V".into());
    let field = field_style(&theme);
    // Choosing what to compare against never drives the output.
    let select_disabled = disable_block.is_some();
    let running = sweep.as_ref().is_some_and(SweepStatus::running);
    let sweep_block = enable_block
        .clone()
        .or_else(|| running.then(|| "A sweep is already running.".into()))
        .or_else(|| {
            selected
                .is_none()
                .then(|| "Choose a target input first.".into())
        });
    let active_target = validation.as_ref().and_then(|v| v.target.clone());
    rsx! {
        div { style: "display:flex; flex-direction:column; gap:8px; padding-top:8px; border-top:1px solid {theme.border_soft};",
            h4 { style: "margin:0; font-size:14px;", "ADC validation" }
            div { style: "display:grid; grid-template-columns:repeat(auto-fit, minmax(min(100%, 180px), 1fr)); gap:6px;",
                label { style: "display:flex; flex-direction:column; gap:2px; font-size:12px;",
                    "Target input"
                    select { style: "{field}", disabled: select_disabled, value: "{selected_id}",
                        onchange: move |event| target.set(event.value()),
                        option { value: "", selected: selected_id.is_empty(), "Choose an input…" }
                        for input in inputs.iter() {
                            option { value: "{input.id}", selected: input.id == selected_id, "{input.label}" }
                        }
                    }
                }
                label { style: "display:flex; flex-direction:column; gap:2px; font-size:12px;",
                    "DAC channel"
                    select { style: "{field}", disabled: select_disabled, value: "{val_channel()}",
                        onchange: move |event| val_channel.set(event.value().parse().unwrap_or(0)),
                        for (i, label) in CHANNEL_LABELS.iter().enumerate() {
                            option { value: "{i}", selected: val_channel() as usize == i, "{label}" }
                        }
                    }
                }
                label { style: "display:flex; flex-direction:column; gap:2px; font-size:12px;",
                    "External attenuation k (input V = P10 V × k)"
                    input { r#type: "number", step: "any", min: "0", style: "{field}", disabled: select_disabled,
                        value: "{attenuation}", oninput: move |event| attenuation.set(event.value()) }
                }
                if selected.as_ref().is_some_and(DacInput::is_load_cell) {
                    label { style: "display:flex; flex-direction:column; gap:2px; font-size:12px;",
                        "Load-cell wiring"
                        select { style: "{field}", disabled: select_disabled, value: "{polarity().wire()}",
                            onchange: move |event| polarity.set(Polarity::from_wire(&event.value())),
                            option { value: "s_minus", selected: polarity() == Polarity::SMinus, "Cable on S− (pin 4), S+ to GND" }
                            option { value: "s_plus", selected: polarity() == Polarity::SPlus, "Cable on S+ (pin 3), S− to GND (gain −100.9)" }
                        }
                    }
                }
            }
            if let Some(input) = &selected {
                div { style: "font-size:12px; color:{theme.text_secondary}; display:flex; flex-direction:column; gap:2px;",
                    span { "{input_summary(input)}" }
                    if !input.model.is_empty() {
                        span { "Model: {input.model}" }
                    }
                }
                if input.needs_attenuator {
                    p { role: "alert",
                        style: "margin:0; padding:8px; border-radius:6px; border:1px solid {theme.warning_border}; background:{theme.warning_background}; color:{theme.warning_text};",
                        "This input needs an external attenuator: the DAC cannot go low enough to stay in its valid range. Fit one (about 1/100 for load cells) and enter its ratio as k."
                    }
                }
            }
            div { style: "display:flex; gap:8px; flex-wrap:wrap;",
                button { r#type: "button", style: "{button_style(&theme)}", disabled: select_disabled || selected.is_none(),
                    onclick: move |_| {
                        let id = target.peek().clone();
                        let body = match validation_body(Some(&id), *val_channel.peek(), &attenuation.peek(), *polarity.peek()) {
                            Ok(body) => body,
                            Err(error) => { message.set(error); return; }
                        };
                        message.set("Selecting validation target…".into());
                        post_json("/api/daq/dac/validation", body, move |result| message.set(match result {
                            Ok(_) => "Validation target set.".into(),
                            Err(error) => format!("Refused: {error}"),
                        }));
                    },
                    if active_target.is_some() { "Update validation" } else { "Start validation" }
                }
                button { r#type: "button", style: "{button_style(&theme)}", disabled: select_disabled || active_target.is_none(),
                    onclick: move |_| {
                        post_json("/api/daq/dac/validation", serde_json::json!({"target": null}), move |result| message.set(match result {
                            Ok(_) => "Validation stopped.".into(),
                            Err(error) => format!("Refused: {error}"),
                        }));
                    },
                    "Stop validation"
                }
            }
            if let Some(reason) = &disable_block {
                span { style: "font-size:12px; color:{theme.text_secondary};", "Validation selection unavailable: {reason}" }
            }
            if !message().is_empty() {
                span { role: "status", style: "font-size:13px;", "{message}" }
            }
            if let Some(error) = validation_error {
                p { role: "alert", style: "margin:0; color:{theme.error_text};", "Validation data unavailable: {error}" }
            }
            if let Some(v) = validation.clone().filter(|v| v.target.is_some()) {
                ValidationView { theme: theme.clone(), validation: v, unit: unit.clone() }
            }
            h4 { style: "margin:8px 0 0; font-size:14px;", "DC sweep" }
            span { style: "font-size:12px; color:{theme.text_secondary};",
                "Steps DC levels across the target's valid range (5–95 %, limited to what the DAC reaches), waits for the DAQ to confirm each level, then fits measured against expected. Uses the target, channel and k above."
            }
            div { style: "display:grid; grid-template-columns:repeat(auto-fit, minmax(min(100%, 150px), 1fr)); gap:6px;",
                label { style: "display:flex; flex-direction:column; gap:2px; font-size:12px;",
                    "Steps (blank = default)"
                    input { r#type: "number", step: "1", min: "2", style: "{field}", disabled: sweep_block.is_some(),
                        value: "{sweep_steps}", oninput: move |event| sweep_steps.set(event.value()) }
                }
                label { style: "display:flex; flex-direction:column; gap:2px; font-size:12px;",
                    "Dwell per step, ms (blank = default)"
                    input { r#type: "number", step: "1", min: "100", style: "{field}", disabled: sweep_block.is_some(),
                        value: "{sweep_dwell}", oninput: move |event| sweep_dwell.set(event.value()) }
                }
            }
            div { style: "display:flex; gap:8px; flex-wrap:wrap; align-items:center;",
                button { r#type: "button", style: "{button_style(&theme)}", disabled: sweep_block.is_some(),
                    onclick: move |_| {
                        let body = match sweep_body(&target.peek(), *val_channel.peek(), &attenuation.peek(), *polarity.peek(), &sweep_steps.peek(), &sweep_dwell.peek()) {
                            Ok(body) => body,
                            Err(error) => { sweep_message.set(error); return; }
                        };
                        sweep_message.set("Starting sweep…".into());
                        post_json("/api/daq/dac/sweep", body, move |result| sweep_message.set(match result {
                            Ok(_) => "Sweep started.".into(),
                            Err(error) => format!("Refused: {error}"),
                        }));
                    },
                    "Run sweep"
                }
                if running {
                    button { r#type: "button", style: "{danger_button_style(&theme)}", disabled: disable_block.is_some(),
                        onclick: move |_| {
                            post_json("/api/daq/dac/sweep/cancel", serde_json::json!({}), move |result| sweep_message.set(match result {
                                Ok(_) => "Cancel sent.".into(),
                                Err(error) => format!("Cancel failed: {error}"),
                            }));
                        },
                        "Cancel sweep"
                    }
                }
            }
            if let Some(reason) = &sweep_block {
                span { style: "font-size:12px; color:{theme.text_secondary};", "Sweep unavailable: {reason}" }
            }
            if !sweep_message().is_empty() {
                span { role: "status", style: "font-size:13px;", "{sweep_message}" }
            }
            if let Some(s) = sweep {
                span { role: "status", style: "font-size:13px;", "{sweep_progress_text(&s)}" }
                if let Some(step) = s.progress.as_ref().filter(|p| s.running() && p.steps > 0) {
                    progress { style: "width:100%;", value: "{step.step}", max: "{step.steps}" }
                }
                if let Some(text) = s.message.as_ref().filter(|m| !m.is_empty()) {
                    span { style: if s.state == "failed" { format!("font-size:13px; color:{};", theme.error_text) } else { format!("font-size:13px; color:{};", theme.text_secondary) }, "{text}" }
                }
                if let Some(report) = s.report.clone() {
                    SweepReportView { theme: theme.clone(), report, unit: unit.clone() }
                }
            }
        }
    }
}

#[component]
fn ValidationView(theme: ThemeConfig, validation: Validation, unit: String) -> Element {
    let geometry = plot_geometry(&validation.series, PLOT_W, PLOT_H, PLOT_PAD);
    let measured_color = series_color(0);
    let expected_color = series_color(1);
    let analysis = validation.analysis.clone().unwrap_or_default();
    rsx! {
        div { style: "display:flex; flex-direction:column; gap:6px;",
            div { style: "display:flex; gap:12px; flex-wrap:wrap; align-items:center; font-size:12px;",
                span { style: "display:inline-flex; gap:4px; align-items:center;", SeriesSwatch { index: 0 } "Measured ({unit})" }
                span { style: "display:inline-flex; gap:4px; align-items:center;", SeriesSwatch { index: 1 } "Expected ({unit})" }
            }
            if let Some(g) = geometry {
                svg { view_box: "0 0 {PLOT_W} {PLOT_H}", role: "img",
                    "aria-label": "Expected against measured for the validation target",
                    style: "width:100%; height:220px; display:block; background:{theme.panel_background_alt}; border:1px solid {theme.border_soft}; border-radius:8px;",
                    path { d: "{g.expected}", fill: "none", stroke: "{expected_color}", "stroke-width": "2", "stroke-dasharray": "6 4" }
                    path { d: "{g.measured}", fill: "none", stroke: "{measured_color}", "stroke-width": "2" }
                    text { x: "6", y: "14", fill: "{theme.text_muted}", "font-size": "11", {format!("{:.4} {unit}", g.y_max)} }
                    text { x: "6", y: "{PLOT_H - 4.0}", fill: "{theme.text_muted}", "font-size": "11", {format!("{:.4} {unit}", g.y_min)} }
                    text { x: "{PLOT_W - 90.0}", y: "{PLOT_H - 4.0}", fill: "{theme.text_muted}", "font-size": "11", {format!("last {:.0} s", g.span_s)} }
                }
            } else {
                span { style: "font-size:12px; color:{theme.text_muted};", "Waiting for samples…" }
            }
            AnalysisGrid { theme: theme.clone(), analysis, unit: unit.clone() }
            for warning in validation.warnings.iter() {
                span { role: "alert", style: "font-size:13px; color:{theme.warning_text};", "⚠ {warning}" }
            }
        }
    }
}

#[component]
fn AnalysisGrid(theme: ThemeConfig, analysis: ValidationAnalysis, unit: String) -> Element {
    let rows = [
        ("Samples", analysis.samples.to_string()),
        (
            "Offset error",
            format!("{} {unit}", fmt_opt(analysis.offset_error, 4)),
        ),
        (
            "Gain error",
            format!("{} %", fmt_opt(analysis.gain_error_pct, 2)),
        ),
        (
            "Noise RMS",
            format!("{} {unit}", fmt_opt(analysis.noise_rms, 4)),
        ),
        (
            "Amplitude (meas / exp)",
            format!(
                "{} / {} {unit}",
                fmt_opt(analysis.measured_amplitude, 4),
                fmt_opt(analysis.expected_amplitude, 4)
            ),
        ),
        (
            "Frequency (meas / exp)",
            format!(
                "{} / {} Hz",
                fmt_opt(analysis.measured_frequency_hz, 3),
                fmt_opt(analysis.expected_frequency_hz, 3)
            ),
        ),
    ];
    rsx! {
        dl { style: "display:grid; grid-template-columns:auto 1fr; gap:2px 10px; margin:0; font-size:12px;",
            for (name, value) in rows {
                dt { style: "color:{theme.text_secondary};", "{name}" }
                dd { style: "margin:0; font-variant-numeric:tabular-nums;", "{value}" }
            }
        }
    }
}

#[component]
fn SweepReportView(theme: ThemeConfig, report: SweepReport, unit: String) -> Element {
    let (border, background, text, verdict) = if report.pass {
        (
            theme.info_accent.clone(),
            theme.info_background.clone(),
            theme.success_text.clone(),
            "SWEEP PASSED",
        )
    } else {
        (
            theme.error_border.clone(),
            theme.error_background.clone(),
            theme.error_text.clone(),
            "SWEEP FAILED",
        )
    };
    let cell = format!(
        "padding:4px 6px; border-bottom:1px solid {}; text-align:right; font-variant-numeric:tabular-nums;",
        theme.border_soft
    );
    let head = format!(
        "padding:4px 6px; border-bottom:1px solid {}; text-align:right; color:{};",
        theme.border, theme.text_secondary
    );
    rsx! {
        div { style: "display:flex; flex-direction:column; gap:8px;",
            div { role: "status", style: "padding:10px; border-radius:8px; border:2px solid {border}; background:{background}; color:{text}; font-weight:700;",
                "{verdict}"
                span { style: "font-weight:400; margin-left:8px;",
                    "offset {fmt_opt(report.offset_error, 4)} {unit} · gain {fmt_opt(report.gain_error_pct, 2)} % · nonlinearity {fmt_opt(report.max_nonlinearity_pct_fs, 3)} % FS · noise {fmt_opt(report.noise_rms, 4)} {unit}"
                }
            }
            div { style: "overflow-x:auto;",
                table { style: "border-collapse:collapse; font-size:12px; min-width:100%;",
                    thead { tr {
                        th { style: "{head} text-align:left;", "Check" }
                        th { style: "{head}", "Value" }
                        th { style: "{head}", "Limit" }
                        th { style: "{head}", "Result" }
                    } }
                    tbody {
                        for check in report.checks.iter() {
                            tr {
                                td { style: "{cell} text-align:left;", "{check.name}" }
                                td { style: "{cell}", "{fmt_opt(check.value, 4)}" }
                                td { style: "{cell}", "{fmt_opt(check.limit, 4)}" }
                                td { style: if check.pass { format!("{cell} color:{}; font-weight:700;", theme.success_text) } else { format!("{cell} color:{}; font-weight:700;", theme.error_text) },
                                    if check.pass { "pass" } else { "FAIL" } }
                            }
                        }
                    }
                }
            }
            div { style: "overflow-x:auto;",
                table { style: "border-collapse:collapse; font-size:12px; min-width:100%;",
                    thead { tr {
                        th { style: "{head}", "DAC pin (V)" }
                        th { style: "{head}", "Expected ({unit})" }
                        th { style: "{head}", "Measured mean" }
                        th { style: "{head}", "Std" }
                        th { style: "{head}", "Readback (V)" }
                        th { style: "{head}", "Samples" }
                    } }
                    tbody {
                        for point in report.points.iter() {
                            tr {
                                td { style: "{cell}", "{fmt_opt(point.dac_pin_v, 3)}" }
                                td { style: "{cell}", "{fmt_opt(point.expected, 4)}" }
                                td { style: "{cell}", "{fmt_opt(point.measured_mean, 4)}" }
                                td { style: "{cell}", "{fmt_opt(point.measured_std, 4)}" }
                                td { style: "{cell}", "{fmt_opt(point.readback_v, 3)}" }
                                td { style: "{cell}", "{point.samples}" }
                            }
                        }
                    }
                }
            }
            if let Some(path) = report.saved_path.as_ref() {
                span { style: "font-size:12px; color:{theme.text_secondary};", "Saved: {path}" }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn status() -> DacStatus {
        serde_json::from_value(serde_json::json!({
            "allowed": false, "refusal": "Flight state is Fill; DAC output is allowed only in Startup or Idle.",
            "flight_state": "Fill", "daq_flight_state": 3, "status_age_ms": 420, "status_fresh": true,
            "any_active": true, "buffer_gain": 2.0, "dac_pin_range_v": [0.2, 3.1], "connector_range_v": [0.4, 6.2],
            "channels": [{"channel": 0, "active": true, "waveform": "staircase", "amplitude_v": 1.0,
                "offset_v": 2.0, "steps": 8, "dwell_ms": 500, "clamped": ["amplitude"], "output_v": 2.5,
                "readback_v": 2.49, "last_result": "ok", "disable_reason": "none", "sequence": 17}],
            "inputs": [{"id": "loadcell_kg50", "label": "50 kg AMP2 (P8)", "unit": "V",
                "valid_range": [0.0, 2.4], "stream_hz": 40.0, "needs_attenuator": true,
                "model": "reported = 0.600 + 100.9 x V_in"}]
        }))
        .unwrap()
    }

    fn render(root: fn() -> Element) -> String {
        let mut dom = VirtualDom::new(root);
        dom.rebuild_in_place();
        dioxus::ssr::render(&dom)
    }

    #[test]
    fn active_banner_and_refusal_render_with_disable_still_enabled() {
        let html = render(|| {
            let state = status();
            let message = use_signal(String::new);
            let block = enable_block_reason(Some(&state), true, false);
            rsx! {
                ActiveBanner { theme: ThemeConfig::default(), channels: state.channels.clone(), disable_block: None, message }
                StatusSummary { theme: ThemeConfig::default(), state, enable_block: block }
            }
        });
        assert!(html.contains("DAC OUTPUT ACTIVE"));
        assert!(html.contains("P10 pin 2: Staircase"), "{html}");
        assert!(html.contains("Flight state is Fill"));
        assert!(html.contains("Reachable at P10: 0.40 to 6.20 V"));
        assert!(html.contains("DAQ status 0.4 s old"));
        let disable = html.split("Disable all outputs").next().unwrap();
        assert!(
            !disable
                .rsplit("<button")
                .next()
                .unwrap()
                .contains("disabled")
        );
    }

    #[test]
    fn refused_channel_greys_enable_and_shows_only_staircase_fields() {
        let html = render(|| {
            let state = status();
            let mut drafts = use_signal(|| [ConfigDraft::default(), ConfigDraft::default()]);
            use_hook(|| drafts.write()[0] = ConfigDraft::from_channel(&state.channels[0]));
            rsx! {
                ChannelCard {
                    theme: ThemeConfig::default(), index: 0, channel: state.channels[0].clone(),
                    connector_range: state.connector_range_v, drafts,
                    enable_block: enable_block_reason(Some(&state), true, false), disable_block: None,
                }
            }
        });
        assert!(html.contains("Steps (2–32)") && html.contains("Dwell per step"));
        assert!(!html.contains("Frequency (Hz)"));
        assert!(html.contains("Clamped: amplitude"));
        let apply = html.split(">Apply<").next().unwrap();
        assert!(apply.rsplit("<button").next().unwrap().contains("disabled"));
        let disable = html.split(">Disable<").next().unwrap();
        assert!(
            !disable
                .rsplit("<button")
                .next()
                .unwrap()
                .contains("disabled")
        );
    }

    #[test]
    fn load_cell_target_shows_polarity_attenuator_warning_plot_and_report() {
        let html = render(|| {
            let state = status();
            let target = use_signal(|| "loadcell_kg50".to_string());
            let val_channel = use_signal(|| 0_u8);
            let attenuation = use_signal(|| "0.01".to_string());
            let polarity = use_signal(|| Polarity::SMinus);
            let validation: Validation = serde_json::from_value(serde_json::json!({
                "target": "loadcell_kg50", "channel": 0, "attenuation": 0.01,
                "warnings": ["Frequency 20 Hz is above a quarter of the 40 Hz stream rate."],
                "series": {"t_ms": [0, 500, 1000], "measured": [0.6, 0.7, null], "expected": [0.6, 0.71, 0.8]},
                "analysis": {"samples": 3, "offset_error": 0.003, "gain_error_pct": null, "noise_rms": 0.001}
            }))
            .unwrap();
            let sweep: SweepStatus = serde_json::from_value(serde_json::json!({
                "state": "done", "progress": {"step": 11, "steps": 11},
                "report": {"points": [{"dac_pin_v": 0.3, "expected": 0.6, "measured_mean": 0.601,
                    "measured_std": 0.001, "readback_v": 0.299, "samples": 10}],
                    "checks": [{"name": "gain", "value": 2.0, "limit": 1.0, "pass": false}],
                    "pass": false, "saved_path": "recordings/r/daq_dac_sweep_1.json"}
            }))
            .unwrap();
            rsx! {
                ValidationSection {
                    theme: ThemeConfig::default(), inputs: state.inputs.clone(),
                    validation: Some(validation), validation_error: None, sweep: Some(sweep),
                    target, val_channel, attenuation, polarity,
                    enable_block: None, disable_block: None,
                }
            }
        });
        assert!(html.contains("Load-cell wiring"));
        assert!(html.contains("needs an external attenuator"));
        assert!(html.contains("Valid 0 to 2.4 V · stream about 40 Hz"));
        assert!(html.contains("reported = 0.600 + 100.9 x V_in"));
        assert!(html.contains("<path"));
        assert!(html.contains("above a quarter"));
        assert!(html.contains("SWEEP FAILED") && html.contains("FAIL"));
        assert!(html.contains("recordings/r/daq_dac_sweep_1.json"));
    }
}
