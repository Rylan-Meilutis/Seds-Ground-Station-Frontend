use super::board_recovery_guard::ConnectionBinding;
use super::igniter_sense_model;
use super::layout::ThemeConfig;
use super::*;

pub(super) fn with_tab(mut layout: super::layout::DataTabLayout) -> super::layout::DataTabLayout {
    // Persisted/custom layouts created before this app feature remain usable.
    if !layout
        .tabs
        .iter()
        .any(|tab| tab.id == "IGNITER_SENSE_SIGNAL")
    {
        let tab = serde_json::from_str(include_str!("igniter_sense_tab.json"))
            .expect("checked-in read-only igniter tab must be valid");
        layout.tabs.push(tab);
    }
    // Older server layouts exposed a permanently empty amperes channel. Keep
    // the measured ADC/voltage cards and show capability in the status panel;
    // do not silently rewrite custom channels or infer a current conversion.
    for tab in &mut layout.tabs {
        if tab.id != "IGNITER_SENSE_SIGNAL" {
            continue;
        }
        if unsupported_current_channel(&tab.channels) {
            tab.channels.truncate(2);
            if let Some(formatters) = &mut tab.channel_formatters {
                formatters.truncate(2);
            }
        }
        for subtab in tab.subtabs.iter_mut().flatten() {
            if subtab
                .data_type
                .as_deref()
                .is_some_and(|kind| kind != "IGNITER_SENSE_SIGNAL")
            {
                continue;
            }
            if let Some(channels) = &mut subtab.channels
                && unsupported_current_channel(channels)
            {
                channels.truncate(2);
                if let Some(formatters) = &mut subtab.channel_formatters {
                    formatters.truncate(2);
                }
            }
        }
    }
    layout
}
fn unsupported_current_channel(channels: &[String]) -> bool {
    channels == ["ADC counts", "Signal voltage", "Current (unavailable)"]
}

#[component]
pub(super) fn IgniterSensePanel(theme: ThemeConfig) -> Element {
    let _render = *TELEMETRY_RENDER_EPOCH.read();
    let epoch = *WS_EPOCH.read();
    let binding = ConnectionBinding::current();
    let mut timer = use_signal(|| 0_u32);
    let _tick = *timer.read();
    let mut session = use_signal(|| (epoch, binding.clone(), monotonic_now_ms()));
    use_effect(move || {
        let epoch = *WS_EPOCH.read();
        let binding = ConnectionBinding::current();
        if session.peek().0 != epoch || session.peek().1 != binding {
            session.set((epoch, binding, monotonic_now_ms()));
        }
    });
    use_future(move || async move {
        loop {
            #[cfg(target_arch = "wasm32")]
            gloo_timers::future::TimeoutFuture::new(250).await;
            #[cfg(not(target_arch = "wasm32"))]
            tokio::time::sleep(std::time::Duration::from_millis(250)).await;
            let next = timer.peek().wrapping_add(1);
            timer.set(next);
        }
    });
    let status_id = intern_telemetry_text("IGNITER_SENSE_STATUS");
    let signal_id = intern_telemetry_text("IGNITER_SENSE_SIGNAL");
    let sender = intern_telemetry_text("AB");
    let (status, signal) = LATEST_TELEMETRY
        .lock()
        .ok()
        .map(|latest| {
            (
                latest
                    .get(&LatestTelemetryKey::new(status_id, sender))
                    .cloned(),
                latest
                    .get(&LatestTelemetryKey::new(signal_id, sender))
                    .cloned(),
            )
        })
        .unwrap_or_default();
    let now = monotonic_now_ms();
    let owned = session.peek();
    let elapsed = status
        .as_ref()
        .and_then(|row| row.live_received_mono_ms)
        .filter(|received| owned.0 == epoch && owned.1 == binding && *received as f64 >= owned.2)
        .and_then(|received| now.is_finite().then_some(now - received as f64))
        .filter(|age| age.is_finite() && *age >= 0.0)
        .map(|age| age.ceil() as u64);
    let data = igniter_sense_model::view(
        status
            .as_ref()
            .map(|row| row.values.as_ref())
            .unwrap_or_default(),
        signal
            .as_ref()
            .map(|row| row.values.as_ref())
            .unwrap_or_default(),
        elapsed,
        u64::try_from(current_wallclock_ms()).ok(),
    );
    let capability = igniter_sense_model::capability(
        status
            .as_ref()
            .map(|row| row.values.as_ref())
            .unwrap_or_default(),
    );
    rsx! {
        section {
            "data-testid":"igniter-sense-panel",
            style:"padding:12px;border:1px solid {theme.border};border-radius:10px;background:{theme.panel_background};color:{theme.text_primary};font:inherit;",
            strong {"Igniter current sense"}
            p {style:"margin:6px 0;color:{theme.text_secondary};","{capability}"}
            div {role:"status","{data.label}"}
            div {style:"display:flex;gap:20px;flex-wrap:wrap;margin-top:8px;",
                span {"ADC: {data.counts}"}
                span {"Signal: {data.volts}"}
                span {"Current: unavailable"}
            }
            p {style:"font-size:0.85em;color:{theme.text_muted};margin-bottom:0;","Measured signal is separate from the ignition command and continuity. It does not confirm firing."}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn igniter_sense_runtime_tab_is_read_only_and_not_duplicated() {
        let empty = super::super::layout::DataTabLayout {
            tabs: Vec::new(),
            sender_split_data_types: Vec::new(),
            default_display_filter: None,
        };
        let layout = with_tab(empty);
        assert_eq!(layout.tabs.len(), 1);
        assert_eq!(layout.tabs[0].channels.len(), 2);
        let sub = layout.tabs[0].subtabs.as_ref().unwrap();
        assert_eq!(sub[0].sender_id.as_deref(), Some("AB"));
        assert_eq!(with_tab(layout).tabs.len(), 1);
    }
    #[test]
    fn old_unavailable_ampere_slot_is_removed_without_changing_custom_channels() {
        let mut tab: super::super::layout::DataTabSpec =
            serde_json::from_str(include_str!("igniter_sense_tab.json")).unwrap();
        tab.channels.push("Current (unavailable)".into());
        tab.subtabs.as_mut().unwrap()[0]
            .channels
            .as_mut()
            .unwrap()
            .push("Current (unavailable)".into());
        let layout = super::super::layout::DataTabLayout {
            tabs: vec![tab.clone()],
            sender_split_data_types: vec![],
            default_display_filter: None,
        };
        let migrated = with_tab(layout.clone());
        assert_eq!(
            migrated.tabs[0].channels,
            vec!["ADC counts", "Signal voltage"]
        );
        assert_eq!(
            migrated.tabs[0].subtabs.as_ref().unwrap()[0]
                .channels
                .as_ref()
                .unwrap()
                .len(),
            2
        );
        tab.channels[2] = "Custom input".into();
        let custom = with_tab(super::super::layout::DataTabLayout {
            tabs: vec![tab.clone()],
            ..layout
        });
        assert_eq!(custom.tabs[0].channels, tab.channels);
    }
    #[test]
    fn igniter_sense_native_panel_mounts_without_a_browser() {
        fn root() -> Element {
            rsx! {IgniterSensePanel {theme:ThemeConfig::default()}}
        }
        let mut dom = VirtualDom::new(root);
        dom.rebuild_in_place();
    }
}
