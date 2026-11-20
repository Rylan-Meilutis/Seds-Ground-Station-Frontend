//! Publishes snapshots to the watch and handles the watch's three message
//! types (none of which reaches a station command; the watch sends its page
//! actions to the GS itself with its watch token). Mounted inside the live dashboard (follows the connected station)
//! and inside the offline preview (demo mode). The app-wide status publisher
//! covers everything before that: the Connect screen, the session check,
//! sign-in and connection failures.
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
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};

/// Foreground pings from the watch keep the fast rate for this long.
const ACTIVE_HOLD_MS: f64 = 30_000.0;
const ACCOUNT_REFRESH_MS: f64 = 60_000.0;
const LOOP_MS: u64 = 500;
/// How often the dashboard publisher checks the direct-link token.
const DIRECT_TICK_MS: f64 = 2_000.0;
/// How often the dashboard publisher checks the watch map tile set.
const MAP_TICK_MS: f64 = 15_000.0;

/// Last antenna status and the monotonic time it arrived, kept so a
/// `no_station` snapshot can still carry the last known targets.
static LAST_ANTENNA: Mutex<Option<(AntennaStatus, f64)>> = Mutex::new(None);

/// Snapshot sequence shared by every publisher, so the watch never sees it
/// go backwards when the phone moves between the Connect screen and a
/// dashboard (it drops an older seq that arrives right after a newer one).
static SEQ: AtomicU64 = AtomicU64::new(0);

fn next_seq() -> u64 {
    SEQ.fetch_add(1, Ordering::AcqRel) + 1
}

/// Mounted dashboard/demo publishers; while one runs, the status publisher
/// stays quiet.
static SCREEN_PUBLISHERS: AtomicUsize = AtomicUsize::new(0);

/// Counts a dashboard/demo publisher for its lifetime.
struct ScreenPublisher;

impl ScreenPublisher {
    fn mount() -> Self {
        SCREEN_PUBLISHERS.fetch_add(1, Ordering::AcqRel);
        ScreenPublisher
    }
}

impl Drop for ScreenPublisher {
    fn drop(&mut self) {
        SCREEN_PUBLISHERS.fetch_sub(1, Ordering::AcqRel);
        PUBLISH_REQUESTED.store(true, Ordering::Release);
    }
}

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
    /// Handles queued watch messages; only settings edits change state, and
    /// only for a user eligible for watch settings (others are echoed the
    /// defaults without anything being stored).
    fn handle_incoming(&mut self, now: f64, eligible: bool) {
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
                    if eligible && let Some(mut edit) = WatchSettings::from_value(settings) {
                        edit.updated_by = Editor::Watch;
                        super::apply_edit(edit, Editor::Watch);
                    }
                    // Echo the authoritative copy either way.
                    self.force = true;
                }
                Some(WatchMessage::Tiles { set, have }) => {
                    if eligible {
                        super::map_sync::on_report(set, have);
                    }
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
        link: station_link(
            ws_connected,
            crate::telemetry_dashboard::DASHBOARD_HAS_CONNECTED.load(Ordering::Relaxed),
        ),
        address: super::phone_status::station_address(&crate::telemetry_dashboard::UrlConfig::base_http()),
        telemetry_age_ms,
    }
}

/// Dashboard link: the first connection is `connecting`, later outages
/// `reconnecting`.
fn station_link(ws_connected: bool, has_connected: bool) -> &'static str {
    match (ws_connected, has_connected) {
        (true, _) => "connected",
        (false, true) => "reconnecting",
        (false, false) => "connecting",
    }
}

/// Snapshot for the watch while no dashboard is mounted: the phone's
/// connection phase (Connect screen, connecting, sign-in, unreachable) plus
/// the last known targets.
fn no_station_snapshot(
    phase: &super::PhonePhase,
    settings: &WatchSettings,
    account: Account,
    catalog: &[super::actions::CatalogAction],
) -> super::protocol::Snapshot {
    let none = |_: &str, _: Option<&str>, _: usize| None;
    let antenna = LAST_ANTENNA.lock().ok().and_then(|g| g.clone());

    let now = monotonic_now_ms();
    let input = Inputs {
        mode: Mode::NoStation,
        seq: next_seq(),
        now_ms: current_wallclock_ms(),
        station: super::phone_status::station_for_phase(phase),
        account,
        flight_state: "",
        clock: None,
        boards: None,
        telemetry: &none,
        antenna: antenna.as_ref().map(|(s, at)| (s, (now - at) as i64)),
        rocket_from_telemetry: None,
        settings,
        catalog,
        policy: None,
    };
    snapshot::build(&input)
}

/// Snapshot sent when a dashboard publisher unmounts: the phone left the
/// station (sign-out, switching stations, connection page).
fn publish_no_station() {
    let eligible = super::watch_settings_available();
    let settings = super::effective_settings(eligible);
    // Keep the page actions: the watch may still reach the station itself.
    let catalog = if eligible { super::catalog_actions() } else { Vec::new() };
    publish(
        &no_station_snapshot(&super::phone_status::phone_phase(), &settings, account_state(), &catalog),
        false,
    );
}

/// App-wide hook (mounted once at the root on the phone apps): starts the
/// watch session at launch, logs every watch link change, and while no
/// dashboard or demo publisher is mounted keeps the watch told what the
/// phone is doing (choosing a station, connecting to <address>, sign-in
/// needed, unreachable), answering the watch's hello/activity at once.
pub(crate) fn use_status_publisher() {
    // Crew voice relay for a watch that cannot reach the station itself.
    #[cfg(not(target_arch = "wasm32"))]
    use_future(move || async move {
        if super::COMPANION_PLATFORM {
            super::voice_relay::run().await;
        }
    });
    use_future(move || async move {
        if !super::COMPANION_PLATFORM {
            return;
        }
        bridge::start();
        let mut rate = Rate {
            force: true,
            ..Rate::default()
        };
        let mut last_bits = None;
        let mut last_phase = None;
        loop {
            let now = monotonic_now_ms();
            let link = bridge::link_state();
            let signed_in = super::account_key().is_some();
            let eligible = super::settings_eligible(super::COMPANION_PLATFORM, signed_in, link);
            if last_bits != Some((link.bits, signed_in)) {
                last_bits = Some((link.bits, signed_in));
                let line = format!("[watch] link {} · {}", link.summary(), super::watch_link_diagnostics());
                eprintln!("{line}");
                crate::debug_log::append(&line);
            }
            if SCREEN_PUBLISHERS.load(Ordering::Acquire) == 0 {
                super::direct_check_account();
                let phase = super::phone_status::phone_phase();
                if last_phase.as_ref() != Some(&phase) {
                    last_phase = Some(phase.clone());
                    rate.force = true;
                }
                rate.handle_incoming(now, eligible);
                if link.supported && link.paired {
                    let settings = super::effective_settings(eligible);
                    let active = rate.active(now, link);
                    if rate.due(now, active, &settings) {
                        let catalog = if eligible { super::catalog_actions() } else { Vec::new() };
                        publish(&no_station_snapshot(&phase, &settings, account_state(), &catalog), active);
                    }
                }
            } else {
                // A dashboard publisher owns the watch; resend on return.
                last_phase = None;
            }
            sleep_ms(LOOP_MS).await;
        }
    });
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
    let _mounted = use_hook(|| std::rc::Rc::new(ScreenPublisher::mount()));
    use_future(move || async move {
        bridge::start();
        SYNC_REQUESTED.store(true, Ordering::Release);
        let mut rate = Rate {
            force: true,
            ..Rate::default()
        };
        let mut last_sync = f64::NEG_INFINITY;
        let mut last_direct = f64::NEG_INFINITY;
        let mut last_map = f64::NEG_INFINITY;
        let mut last_antenna_poll = f64::NEG_INFINITY;
        let mut was_eligible = false;
        loop {
            let now = monotonic_now_ms();
            let link = bridge::link_state();
            let eligible = super::COMPANION_PLATFORM
                && super::settings_eligible(
                    super::COMPANION_PLATFORM,
                    super::account_key().is_some(),
                    link,
                );
            if eligible != was_eligible {
                // Became eligible (signed in, watch app installed): sync now.
                // Lost eligibility: resend so the watch drops the settings.
                was_eligible = eligible;
                SYNC_REQUESTED.store(true, Ordering::Release);
                PUBLISH_REQUESTED.store(true, Ordering::Release);
            }
            rate.handle_incoming(now, eligible);
            if SYNC_REQUESTED.swap(false, Ordering::AcqRel) || now - last_sync >= ACCOUNT_REFRESH_MS
            {
                last_sync = now;
                super::sync_once().await;
                super::refresh_action_catalog().await;
            }
            if now - last_direct >= DIRECT_TICK_MS {
                last_direct = now;
                super::direct_tick(eligible).await;
            }
            if link.supported && link.paired {
                let settings = super::effective_settings(eligible);
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
                    let seq = next_seq();
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
                    let catalog = if eligible { super::catalog_actions() } else { Vec::new() };
                    let policy = crate::telemetry_dashboard::ACTION_POLICY_SIGNAL.peek().clone();
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
                        catalog: &catalog,
                        policy: Some(&policy),
                    };
                    let snapshot = snapshot::build(&input);
                    super::map_sync::note_targets(&snapshot.targets);
                    publish(&snapshot, active);
                }
                if now - last_map >= MAP_TICK_MS {
                    last_map = now;
                    super::map_sync::tick(eligible, link, &settings).await;
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
    let _mounted = use_hook(|| std::rc::Rc::new(ScreenPublisher::mount()));
    use_future(move || async move {
        bridge::start();
        let mut rate = Rate {
            force: true,
            ..Rate::default()
        };
        loop {
            let now = monotonic_now_ms();
            // The offline preview has no signed-in user, so the watch gets
            // the default settings and its edits are not stored.
            rate.handle_incoming(now, false);
            let link = bridge::link_state();
            if link.supported && link.paired {
                let settings = super::effective_settings(false);
                let active = rate.active(now, link);
                if rate.due(now, active, &settings) {
                    let seq = next_seq();
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::telemetry_dashboard::watch_companion::PhonePhase;

    fn signed_out() -> Account {
        Account {
            signed_in: false,
            sync: "local_only",
        }
    }

    #[test]
    fn dashboard_link_distinguishes_first_connection_from_reconnects() {
        assert_eq!(station_link(true, false), "connected");
        assert_eq!(station_link(true, true), "connected");
        assert_eq!(station_link(false, false), "connecting");
        assert_eq!(station_link(false, true), "reconnecting");
    }

    /// The watch must learn what the phone is doing before any station
    /// connection completes, with a sequence that never goes backwards.
    #[test]
    fn connection_state_snapshots_before_the_dashboard() {
        let settings = WatchSettings::default();
        let connecting = no_station_snapshot(
            &PhonePhase::Connecting {
                base: "https://gs.local:3000/".into(),
            },
            &settings,
            signed_out(),
            &[],
        );
        let value = serde_json::to_value(&connecting).unwrap();
        assert_eq!(value["mode"], "no_station");
        assert_eq!(value["station"]["link"], "connecting");
        assert_eq!(value["station"]["address"], "gs.local:3000");
        // Pages and settings travel too, so the watch can lay out its pages.
        assert!(!value["pages"].as_array().unwrap().is_empty());
        let sign_in = no_station_snapshot(
            &PhonePhase::SignInRequired {
                base: "https://gs.local:3000".into(),
            },
            &settings,
            signed_out(),
            &[],
        );
        assert_eq!(serde_json::to_value(&sign_in).unwrap()["station"]["link"], "sign_in");
        assert!(sign_in.seq > connecting.seq);
        let choosing = no_station_snapshot(&PhonePhase::ChoosingStation, &settings, signed_out(), &[]);
        let value = serde_json::to_value(&choosing).unwrap();
        assert_eq!(value["station"]["link"], "offline");
        assert_eq!(value["station"]["address"], "");
        assert!(choosing.seq > sign_in.seq);
    }

    #[test]
    fn screen_publishers_are_counted_for_their_lifetime() {
        let before = SCREEN_PUBLISHERS.load(Ordering::Acquire);
        let mounted = ScreenPublisher::mount();
        assert_eq!(SCREEN_PUBLISHERS.load(Ordering::Acquire), before + 1);
        PUBLISH_REQUESTED.store(false, Ordering::Release);
        drop(mounted);
        assert_eq!(SCREEN_PUBLISHERS.load(Ordering::Acquire), before);
        // The status publisher takes over at once.
        assert!(PUBLISH_REQUESTED.load(Ordering::Acquire));
    }
}
