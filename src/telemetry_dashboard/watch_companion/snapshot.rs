//! Builds watch snapshots from phone state. `build` is pure so the exact
//! output is unit tested; the publisher gathers the inputs.
use super::geodesy;
use super::protocol::*;
use super::registry::{self, Quantity, Source};
use super::settings::WatchSettings;
use serde::Deserialize;
use std::collections::BTreeMap;

/// One fix from `GET /api/antenna/status`.
#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
pub struct StationFix {
    pub latitude: f64,
    pub longitude: f64,
    #[serde(default)]
    pub altitude_m: Option<f64>,
    #[serde(default)]
    pub age_ms: u64,
    #[serde(default)]
    pub stale: bool,
    #[serde(default)]
    pub manual: bool,
    #[serde(default)]
    pub source: String,
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
pub struct AntennaStatus {
    #[serde(default)]
    pub rocket: Option<StationFix>,
    #[serde(default)]
    pub ground_station: Option<StationFix>,
    #[serde(default)]
    pub rrls: Option<StationFix>,
}

/// A telemetry sample: value and wall-clock receive time.
pub type Sample = (f64, i64);

pub struct Inputs<'a> {
    pub mode: Mode,
    pub seq: u64,
    pub now_ms: i64,
    pub station: Station,
    pub account: Account,
    pub flight_state: &'a str,
    pub clock: Option<Clock>,
    /// `(seen, total)`
    pub boards: Option<(usize, usize)>,
    pub telemetry: &'a dyn Fn(&str, Option<&str>, usize) -> Option<Sample>,
    /// Antenna status plus how long ago (ms) the phone received it.
    pub antenna: Option<(&'a AntennaStatus, i64)>,
    /// Rocket position from the phone's own telemetry store, as a fallback
    /// when antenna status is unavailable: `(lat, lon, alt, received_ms)`.
    pub rocket_from_telemetry: Option<(f64, f64, Option<f64>, i64)>,
    pub settings: &'a WatchSettings,
}

pub const LINK_TIMEOUT_S: u64 = 15;

fn fmt_number(value: f64, precision: usize) -> String {
    let text = format!("{value:.precision$}");
    // Avoid "-0".
    if text
        .trim_start_matches('-')
        .chars()
        .all(|c| c == '0' || c == '.')
    {
        text.trim_start_matches('-').to_string()
    } else {
        text
    }
}

pub fn clock_text(clock: &Clock) -> String {
    let s = clock.ms.max(0) / 1000;
    match clock.kind {
        "tminus" => format!("T- {:02}:{:02}", s / 60, s % 60),
        "tplus" => format!("T+ {:02}:{:02}", s / 60, s % 60),
        _ => "Idle".into(),
    }
}

fn unknown(label: &'static str) -> ItemValue {
    ItemValue {
        label,
        text: "--".into(),
        value: None,
        unit: "",
        age_ms: None,
        stale: false,
        level: Level::Unknown,
        clock: None,
    }
}

fn item_value(def: &registry::ItemDef, input: &Inputs<'_>, rocket: Option<&Target>) -> ItemValue {
    let metric = input.settings.metric();
    match def.source {
        Source::FlightState => {
            if input.flight_state.is_empty() {
                return unknown(def.label);
            }
            ItemValue {
                text: super::super::types::display_flight_state(input.flight_state),
                level: Level::Normal,
                age_ms: Some(0),
                ..unknown(def.label)
            }
        }
        Source::LaunchClock => match &input.clock {
            Some(clock) => ItemValue {
                text: clock_text(clock),
                level: Level::Normal,
                age_ms: Some(0),
                clock: Some(clock.clone()),
                ..unknown(def.label)
            },
            None => unknown(def.label),
        },
        Source::StationLink => {
            let level = match input.station.link {
                "connected" => Level::Normal,
                "reconnecting" => Level::Warn,
                _ => Level::Alert,
            };
            let text = match (input.station.link, input.station.telemetry_age_ms) {
                ("connected", Some(age)) => format!("OK · {}", geodesy::format_age(age)),
                ("connected", None) => "OK".into(),
                ("reconnecting", _) => "Reconnecting".into(),
                _ => "Offline".into(),
            };
            ItemValue {
                text,
                level,
                age_ms: Some(0),
                ..unknown(def.label)
            }
        }
        Source::Boards => match input.boards {
            Some((seen, total)) if total > 0 => ItemValue {
                text: format!("{seen}/{total}"),
                value: Some(seen as f64),
                level: if seen == total {
                    Level::Normal
                } else {
                    Level::Warn
                },
                age_ms: Some(0),
                ..unknown(def.label)
            },
            _ => unknown(def.label),
        },
        Source::RocketFixAge => match rocket {
            Some(t) => ItemValue {
                text: geodesy::format_age(t.age_ms),
                value: Some(t.age_ms as f64 / 1000.0),
                unit: "s",
                age_ms: Some(t.age_ms),
                stale: t.stale,
                level: if t.stale { Level::Warn } else { Level::Normal },
                ..unknown(def.label)
            },
            None => unknown(def.label),
        },
        Source::Telemetry {
            channels,
            quantity,
            precision,
            stale_after_ms,
        } => {
            let newest = channels
                .iter()
                .filter_map(|(dt, sender, index)| (input.telemetry)(dt, *sender, *index))
                .filter(|(v, _)| v.is_finite())
                .max_by_key(|(_, received)| *received);
            let Some((raw, received)) = newest else {
                return unknown(def.label);
            };
            let (value, unit) = quantity.display(raw, metric);
            let age = input.now_ms.saturating_sub(received).max(0);
            let stale = age > stale_after_ms;
            ItemValue {
                label: def.label,
                text: fmt_number(
                    value,
                    if quantity == Quantity::Count {
                        0
                    } else {
                        precision
                    },
                ),
                value: Some(value),
                unit,
                age_ms: Some(age),
                stale,
                level: if stale { Level::Warn } else { Level::Normal },
                clock: None,
            }
        }
    }
}

fn station_target(
    id: &'static str,
    label: &str,
    fix: &StationFix,
    extra_age_ms: i64,
    stale_after_ms: i64,
) -> Option<Target> {
    if !geodesy::LatLon::new(fix.latitude, fix.longitude).valid() {
        return None;
    }
    // Saved positions do not age; live fixes age from the backend's report.
    let age_ms = if fix.manual {
        0
    } else {
        (fix.age_ms as i64).saturating_add(extra_age_ms.max(0))
    };
    Some(Target {
        id,
        label: if fix.manual {
            format!("{label} (saved)")
        } else {
            label.to_string()
        },
        lat: fix.latitude,
        lon: fix.longitude,
        alt_m: fix.altitude_m.filter(|a| a.is_finite()),
        age_ms,
        stale: !fix.manual && (fix.stale || age_ms > stale_after_ms),
        source: if fix.source.is_empty() {
            label.to_string()
        } else {
            fix.source.clone()
        },
        fallback: fix.manual,
        demo_offset: None,
    })
}

pub fn targets(input: &Inputs<'_>) -> Vec<Target> {
    let stale_after_ms = input.settings.compass.stale_after_s as i64 * 1000;
    let mut out = Vec::new();
    for (id, label) in registry::TARGETS {
        if !input.settings.compass.targets.iter().any(|t| t == id) {
            continue;
        }
        let from_antenna = input.antenna.and_then(|(status, extra)| {
            let fix = match *id {
                "rocket" => status.rocket.as_ref(),
                "ground_station" => status.ground_station.as_ref(),
                "rrls" => status.rrls.as_ref(),
                _ => None,
            }?;
            station_target(id, label, fix, extra, stale_after_ms)
        });
        let target = from_antenna.or_else(|| {
            if *id != "rocket" {
                return None;
            }
            let (lat, lon, alt, received) = input.rocket_from_telemetry?;
            let fix = StationFix {
                latitude: lat,
                longitude: lon,
                altitude_m: alt,
                age_ms: 0,
                stale: false,
                manual: false,
                source: "Rocket GPS (phone telemetry)".into(),
            };
            station_target(
                id,
                label,
                &fix,
                input.now_ms.saturating_sub(received),
                stale_after_ms,
            )
        });
        out.extend(target);
    }
    out
}

pub fn build(input: &Inputs<'_>) -> Snapshot {
    let targets = targets(input);
    let rocket = targets.iter().find(|t| t.id == "rocket");
    let items: BTreeMap<_, _> = registry::ITEMS
        .iter()
        .map(|def| (def.id, item_value(def, input, rocket)))
        .collect();
    Snapshot {
        v: PROTOCOL_VERSION,
        seq: input.seq,
        mode: input.mode,
        station: input.station.clone(),
        account: input.account.clone(),
        items,
        pages: resolved_pages(input.settings),
        targets,
        settings: input.settings.clone(),
        catalog: catalog(),
        link_timeout_s: LINK_TIMEOUT_S,
    }
}

#[cfg(any(test, not(target_arch = "wasm32")))]
/// Example data for the offline preview. Every value is plainly fake and
/// the targets sit at fixed offsets from wherever the watch is.
pub fn demo(seq: u64, now_ms: i64, settings: &WatchSettings, account: Account) -> Snapshot {
    let t = (now_ms / 1000).rem_euclid(600);
    let demo_values = move |dt: &str, _: Option<&str>, index: usize| -> Option<Sample> {
        let v = match (dt, index) {
            ("DESCENT_STATE", 2) => 1500.0 - (t as f64) * 2.4,
            ("DESCENT_STATE", 3) => -6.1,
            ("GPS_DATA", 2) => 1480.0 - (t as f64) * 2.4,
            ("GPS_SATELLITE_NUMBER", 0) => 11.0,
            ("FUEL_TANK_PRESSURE", 0) => 12.0,
            ("COMBUSTION_CHAMBER_PRESSURE", 0) => 0.0,
            ("LOADCELL_WEIGHT_KG", 0) => 0.4,
            ("LOADCELL_FILL_PERCENT", 0) => 0.0,
            ("BATTERY_VOLTAGE", 0) => 15.9,
            _ => return None,
        };
        Some((v, now_ms - 400))
    };
    let input = Inputs {
        mode: Mode::Demo,
        seq,
        now_ms,
        station: Station {
            name: "Demo station".into(),
            role: "primary".into(),
            link: "connected",
            telemetry_age_ms: Some(400),
        },
        account,
        flight_state: "Descent",
        clock: Some(Clock {
            kind: "tplus",
            ms: 95_000 + t * 1000,
        }),
        boards: Some((6, 6)),
        telemetry: &demo_values,
        antenna: None,
        rocket_from_telemetry: None,
        settings,
    };
    let mut snapshot = build(&input);
    let offsets = [
        ("rocket", "Rocket (demo)", 640.0, 410.0, 3_000),
        ("ground_station", "Ground station (demo)", -350.0, -900.0, 0),
        ("rrls", "RRLS (demo)", -1200.0, 300.0, 0),
    ];
    snapshot.targets = offsets
        .iter()
        .filter(|(id, ..)| settings.compass.targets.iter().any(|t| t == id))
        .map(|(id, label, n, e, age)| Target {
            id,
            label: (*label).into(),
            lat: 0.0,
            lon: 0.0,
            alt_m: None,
            age_ms: *age,
            stale: false,
            source: "Example data".into(),
            fallback: false,
            demo_offset: Some(DemoOffset {
                north_m: *n,
                east_m: *e,
            }),
        })
        .collect();
    if let Some(item) = snapshot.items.get_mut("rocket_fix_age") {
        item.text = geodesy::format_age(3_000);
        item.level = Level::Normal;
    }
    snapshot
}

#[cfg(test)]
mod tests {
    use super::*;

    fn input<'a>(
        settings: &'a WatchSettings,
        telemetry: &'a dyn Fn(&str, Option<&str>, usize) -> Option<Sample>,
        antenna: Option<(&'a AntennaStatus, i64)>,
    ) -> Inputs<'a> {
        Inputs {
            mode: Mode::Live,
            seq: 7,
            now_ms: 100_000,
            station: Station {
                name: "RRLS".into(),
                role: "rrls".into(),
                link: "connected",
                telemetry_age_ms: Some(500),
            },
            account: Account {
                signed_in: true,
                sync: "synced",
            },
            flight_state: "Descent",
            clock: Some(Clock {
                kind: "tplus",
                ms: 83_000,
            }),
            boards: Some((5, 6)),
            telemetry,
            antenna,
            rocket_from_telemetry: None,
            settings,
        }
    }

    #[test]
    fn newest_channel_wins_and_units_follow_settings() {
        let telemetry = |dt: &str, _: Option<&str>, i: usize| match (dt, i) {
            ("ASCENT_STATE", 4) => Some((900.0, 90_000)),
            ("DESCENT_STATE", 2) => Some((1000.0, 99_000)),
            ("BAROMETER_DATA", 2) => Some((f64::NAN, 99_900)),
            ("FUEL_TANK_PRESSURE", 0) => Some((-0.2, 50_000)),
            _ => None,
        };
        let mut settings = WatchSettings::default();
        let s = build(&input(&settings, &telemetry, None));
        let alt = &s.items["altitude"];
        assert_eq!(
            (alt.text.as_str(), alt.unit, alt.age_ms, alt.stale),
            ("1000", "m", Some(1000), false)
        );
        let tank = &s.items["tank_pressure"];
        assert_eq!(
            (tank.text.as_str(), tank.stale, tank.level),
            ("0", true, Level::Warn)
        );
        assert_eq!(s.items["velocity"].level, Level::Unknown);
        assert_eq!(s.items["boards"].text, "5/6");
        assert_eq!(s.items["t_clock"].text, "T+ 01:23");
        assert_eq!(s.items["station_link"].text, "OK · 0s");
        settings.units = super::super::settings::Units::Imperial;
        let s = build(&input(&settings, &telemetry, None));
        assert_eq!(
            (s.items["altitude"].text.as_str(), s.items["altitude"].unit),
            ("3281", "ft")
        );
    }

    #[test]
    fn targets_use_antenna_status_with_saved_fallback_labels_and_staleness() {
        let none = |_: &str, _: Option<&str>, _: usize| None;
        let status = AntennaStatus {
            rocket: Some(StationFix {
                latitude: 32.99,
                longitude: -106.97,
                altitude_m: Some(1400.0),
                age_ms: 25_000,
                stale: false,
                manual: false,
                source: "FC GPS".into(),
            }),
            ground_station: Some(StationFix {
                latitude: 32.98,
                longitude: -106.98,
                altitude_m: None,
                age_ms: 0,
                stale: false,
                manual: true,
                source: "Saved GroundStation (manual)".into(),
            }),
            rrls: Some(StationFix {
                latitude: 999.0,
                longitude: 0.0,
                ..StationFix::default()
            }),
        };
        let settings = WatchSettings::default();
        let s = build(&input(&settings, &none, Some((&status, 6_000))));
        assert_eq!(s.targets.len(), 2, "invalid RRLS fix is dropped");
        let rocket = &s.targets[0];
        assert_eq!(
            (rocket.age_ms, rocket.stale, rocket.fallback),
            (31_000, true, false)
        );
        let gs = &s.targets[1];
        assert_eq!(
            (gs.label.as_str(), gs.fallback, gs.stale, gs.age_ms),
            ("Ground station (saved)", true, false, 0)
        );
        assert_eq!(s.items["rocket_fix_age"].text, "31s");
        let json = serde_json::to_value(&s).unwrap();
        assert_eq!(json["mode"], "live");
        assert_eq!(json["targets"][1]["fallback"], true);
        assert!(json["targets"][0].get("demo_offset").is_none());
    }

    #[test]
    fn rocket_falls_back_to_phone_telemetry_and_targets_follow_settings() {
        let none = |_: &str, _: Option<&str>, _: usize| None;
        let mut settings = WatchSettings::default();
        let mut i = input(&settings, &none, None);
        i.rocket_from_telemetry = Some((32.99, -106.97, None, 98_000));
        let s = build(&i);
        assert_eq!(s.targets.len(), 1);
        assert_eq!((s.targets[0].age_ms, s.targets[0].stale), (2_000, false));
        settings.compass.targets = vec!["ground_station".into()];
        let mut i = input(&settings, &none, None);
        i.rocket_from_telemetry = Some((32.99, -106.97, None, 98_000));
        assert!(build(&i).targets.is_empty());
    }

    #[test]
    fn demo_is_marked_and_offsets_targets() {
        let s = demo(
            1,
            1_000_000,
            &WatchSettings::default(),
            Account {
                signed_in: false,
                sync: "local_only",
            },
        );
        assert_eq!(s.mode, Mode::Demo);
        assert!(
            s.targets
                .iter()
                .all(|t| t.demo_offset.is_some() && t.label.contains("demo"))
        );
        assert_eq!(s.items["flight_state"].text, "Descent");
        assert!(serde_json::to_vec(&s).unwrap().len() < MAX_MESSAGE_BYTES);
    }

    #[test]
    fn clock_text_formats() {
        assert_eq!(
            clock_text(&Clock {
                kind: "tminus",
                ms: 9_500
            }),
            "T- 00:09"
        );
        assert_eq!(
            clock_text(&Clock {
                kind: "tminus",
                ms: -5
            }),
            "T- 00:00"
        );
        assert_eq!(
            clock_text(&Clock {
                kind: "idle",
                ms: 0
            }),
            "Idle"
        );
    }
}
