//! Publishes snapshots to the watch and handles the watch's three message
//! types. Mounted inside the live dashboard (follows the connected station)
//! and inside the offline preview (demo mode).
use super::protocol::{Account, Clock, Mode, Station, WatchMessage};
use super::settings::{Editor, WatchSettings};
use super::snapshot::{self, AntennaStatus, Inputs};
use super::{PUBLISH_REQUESTED, SYNC_REQUESTED, WATCH_SYNC, bridge, registry, sleep_ms};
use crate::telemetry_dashboard::types::BoardStatusEntry;
use crate::telemetry_dashboard::{
    LaunchClockKind, LaunchClockMsg, NetworkTimeSync, compensated_network_time_ms,
    current_wallclock_ms, latest_telemetry_row, launch_clock_tminus_remaining_ms,
    launch_clock_tplus_elapsed_ms, monotonic_now_ms, telemetry_row_received_ms,
};
use dioxus::prelude::*;
use std::sync::Mutex;
use std::sync::atomic::Ordering;

/// Foreground pings from the watch keep the fast rate for this long.
const ACTIVE_HOLD_MS: f64 = 30_000.0;
const ACCOUNT_REFRESH_MS: f64 = 60_000.0;
const LOOP_MS: u64 = 500;

/// Last antenna status and the monotonic time it arrived, kept so a
/// `no_station` snapshot can still carry the last known targets.
static LAST_ANTENNA: Mutex<Option<(AntennaStatus, f64)>> = Mutex::new(None);

fn account_state() -> Account {
    Account {
        signed_in: super::account_key().is_some(),
        sync: WATCH_SYNC.peek().wire(),
    }
}

#[derive(Default)]
struct Rate {
    active_until: f64,
    last_sent: f64,
    force: bool,
}

impl Rate {
    /// Handles queued watch messages; only settings edits change state.
    fn handle_incoming(&mut self, now: f64) {
        for raw in bridge::take_incoming(16) {
            match WatchMessage::parse(&raw) {
                Some(WatchMessage::Hello) => {
                    self.active_until = now + ACTIVE_HOLD_MS;
                    self.force = true;
                }
                Some(WatchMessage::Activity { foreground, .. }) => {
                    self.active_until = if foreground {
                        now + ACTIVE_HOLD_MS
                    } else {
                        0.0
                    };
                    self.force = true;
                }
                Some(WatchMessage::Settings { settings }) => {
                    if let Some(mut edit) = WatchSettings::from_value(settings) {
                        edit.updated_by = Editor::Watch;
                        super::apply_edit(edit, Editor::Watch);
                    }
                    // Echo the authoritative copy either way.
                    self.force = true;
                }
                None => {}
            }
        }
    }

    fn active(&self, now: f64, link: bridge::LinkState) -> bool {
        link.reachable && now < self.active_until
    }

    fn due(&mut self, now: f64, active: bool, settings: &WatchSettings) -> bool {
        let interval = if active {
            settings.power.active_interval_ms()
        } else {
            settings.power.idle_interval_ms()
        } as f64;
        let requested = PUBLISH_REQUESTED.swap(false, Ordering::AcqRel);
        if self.force || requested || now - self.last_sent >= interval {
            self.force = false;
            self.last_sent = now;
            true
        } else {
            false
        }
    }
}

fn publish(snapshot: &super::protocol::Snapshot, urgent: bool) {
    if let Ok(json) = serde_json::to_string(snapshot)
        && json.len() <= super::protocol::MAX_MESSAGE_BYTES
    {
        bridge::publish(&json, urgent);
    }
}

fn telemetry_sample(dt: &str, sender: Option<&str>, index: usize) -> Option<snapshot::Sample> {
    let row = latest_telemetry_row(dt, sender)?;
    let value = row.values.get(index).copied().flatten()?;
    Some((value as f64, telemetry_row_received_ms(&row)))
}

fn rocket_from_telemetry() -> Option<(f64, f64, Option<f64>, i64)> {
    ["GPS_DATA", "GPS", "ROCKET_GPS"]
        .iter()
        .filter_map(|dt| latest_telemetry_row(dt, None))
        .filter_map(|row| {
            let lat = row.values.first().copied().flatten()? as f64;
            let lon = row.values.get(1).copied().flatten()? as f64;
            let alt = row.values.get(2).copied().flatten().map(f64::from);
            Some((lat, lon, alt, telemetry_row_received_ms(&row)))
        })
        .max_by_key(|(.., received)| *received)
}

fn newest_telemetry_ms() -> Option<i64> {
    registry::ITEMS
        .iter()
        .filter_map(|item| match item.source {
            registry::Source::Telemetry { channels, .. } => Some(channels),
            _ => None,
        })
        .flatten()
        .filter_map(|(dt, sender, _)| {
            latest_telemetry_row(dt, *sender).map(|row| telemetry_row_received_ms(&row))
        })
        .max()
}

fn clock_from(msg: Option<&LaunchClockMsg>, network_now: Option<i64>) -> Option<Clock> {
    let msg = msg?;
    let now = network_now.unwrap_or_else(current_wallclock_ms);
    Some(match msg.kind {
        LaunchClockKind::Idle => Clock {
            kind: "idle",
            ms: 0,
        },
        LaunchClockKind::TMinus => Clock {
            kind: "tminus",
            ms: launch_clock_tminus_remaining_ms(msg, now)?,
        },
        LaunchClockKind::TPlus => Clock {
            kind: "tplus",
            ms: launch_clock_tplus_elapsed_ms(msg, Some(now), None)?,
        },
    })
}

fn station(ws_connected: bool) -> Station {
    let status = crate::auth::current_status();
    let telemetry_age_ms =
        newest_telemetry_ms().map(|t| current_wallclock_ms().saturating_sub(t).max(0));
    Station {
        name: crate::auth::station_name(),
        role: status.station_role.unwrap_or_else(|| "primary".into()),
        link: if ws_connected {
            "connected"
        } else if crate::telemetry_dashboard::DASHBOARD_HAS_CONNECTED.load(Ordering::Relaxed) {
            "reconnecting"
        } else {
            "offline"
        },
        telemetry_age_ms,
    }
}

/// Snapshot sent when the publisher unmounts: the phone left the station
/// (sign-out, switching stations, connection page). Keeps last targets.
fn publish_no_station() {
    let settings = super::current_settings();
    let none = |_: &str, _: Option<&str>, _: usize| None;
    let antenna = LAST_ANTENNA.lock().ok().and_then(|g| g.clone());
    let now = monotonic_now_ms();
    let input = Inputs {
        mode: Mode::NoStation,
        seq: 0,
        now_ms: current_wallclock_ms(),
        station: Station {
            name: String::new(),
            role: "unknown".into(),
            link: "offline",
            telemetry_age_ms: None,
        },
        account: account_state(),
        flight_state: "",
        clock: None,
        boards: None,
        telemetry: &none,
        antenna: antenna.as_ref().map(|(s, at)| (s, (now - at) as i64)),
        rocket_from_telemetry: None,
        settings: &settings,
    };
    publish(&snapshot::build(&input), false);
}

/// Hook for the live dashboard: runs for the dashboard's lifetime, so it
/// follows whichever station the phone is connected to and survives
/// full-screen tools (antenna aiming, admin) that replace the main view.
pub(in crate::telemetry_dashboard) fn use_live_publisher(
    flight_state: Signal<String>,
    launch_clock: Signal<Option<LaunchClockMsg>>,
    board_status: Signal<Vec<BoardStatusEntry>>,
    network_time: Signal<Option<NetworkTimeSync>>,
) {
    use_drop(publish_no_station);
    use_future(move || async move {
        bridge::start();
        SYNC_REQUESTED.store(true, Ordering::Release);
        let mut rate = Rate {
            force: true,
            ..Rate::default()
        };
        let mut seq = 0u64;
        let mut last_sync = f64::NEG_INFINITY;
        let mut last_antenna_poll = f64::NEG_INFINITY;
        loop {
            let now = monotonic_now_ms();
            rate.handle_incoming(now);
            if SYNC_REQUESTED.swap(false, Ordering::AcqRel) || now - last_sync >= ACCOUNT_REFRESH_MS
            {
                last_sync = now;
                super::sync_once().await;
            }
            let link = bridge::link_state();
            if link.supported && link.paired {
                let settings = super::current_settings();
                let active = rate.active(now, link);
                let antenna_interval = match (settings.compass_enabled(), active) {
                    (true, true) => settings.power.active_interval_ms().max(1_000) as f64,
                    (true, false) => settings.power.idle_interval_ms().max(10_000) as f64,
                    (false, _) => 30_000.0,
                };
                if now - last_antenna_poll >= antenna_interval {
                    last_antenna_poll = now;
                    if let Ok(status) = crate::telemetry_dashboard::http_get_json::<AntennaStatus>(
                        "/api/antenna/status",
                    )
                    .await
                        && let Ok(mut slot) = LAST_ANTENNA.lock()
                    {
                        *slot = Some((status, monotonic_now_ms()));
                    }
                }
                if rate.due(monotonic_now_ms(), active, &settings) {
                    seq += 1;
                    let antenna = LAST_ANTENNA.lock().ok().and_then(|g| g.clone());
                    let view = crate::telemetry_dashboard::mission_phase::StateView::resolve(
                        crate::telemetry_dashboard::mission_phase::MISSION_PHASE.peek().as_ref(),
                        &flight_state.peek(),
                        &crate::telemetry_dashboard::mission_phase::FLIGHT_STATE_DETAIL.peek(),
                    );
                    let flight = if view.phase.is_some() || view.detail.is_some() { view.text() } else { String::new() };
                    let clock = clock_from(
                        launch_clock.peek().as_ref(),
                        (*network_time.peek()).map(compensated_network_time_ms),
                    );
                    let boards = {
                        let list = board_status.peek();
                        (!list.is_empty())
                            .then(|| (list.iter().filter(|b| b.seen).count(), list.len()))
                    };
                    let telemetry = telemetry_sample;
                    let input = Inputs {
                        mode: Mode::Live,
                        seq,
                        now_ms: current_wallclock_ms(),
                        station: station(*crate::telemetry_dashboard::WS_CONNECTED_SIGNAL.peek()),
                        account: account_state(),
                        flight_state: &flight,
                        clock,
                        boards,
                        telemetry: &telemetry,
                        antenna: antenna
                            .as_ref()
                            .map(|(s, at)| (s, (monotonic_now_ms() - at) as i64)),
                        rocket_from_telemetry: rocket_from_telemetry(),
                        settings: &settings,
                    };
                    publish(&snapshot::build(&input), active);
                }
            }
            sleep_ms(LOOP_MS).await;
        }
    });
}

/// Hook for the offline preview: example data, marked demo, no station traffic.
#[cfg(not(target_arch = "wasm32"))]
pub(in crate::telemetry_dashboard) fn use_demo_publisher() {
    use_drop(publish_no_station);
    use_future(move || async move {
        bridge::start();
        let mut rate = Rate {
            force: true,
            ..Rate::default()
        };
        let mut seq = 0u64;
        loop {
            let now = monotonic_now_ms();
            // Watch edits still apply locally; account sync waits for a
            // real station session.
            rate.handle_incoming(now);
            let link = bridge::link_state();
            if link.supported && link.paired {
                let settings = super::current_settings();
                let active = rate.active(now, link);
                if rate.due(now, active, &settings) {
                    seq += 1;
                    let account = Account {
                        signed_in: false,
                        sync: super::SyncState::LocalOnly.wire(),
                    };
                    publish(
                        &snapshot::demo(seq, current_wallclock_ms(), &settings, account),
                        active,
                    );
                }
            }
            sleep_ms(LOOP_MS).await;
        }
    });
}
