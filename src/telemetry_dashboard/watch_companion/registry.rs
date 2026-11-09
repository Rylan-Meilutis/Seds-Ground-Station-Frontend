//! Registry of watch items and pages. Adding a telemetry item or an `items`
//! page here is enough: the phone sends the resolved catalog to the watch and
//! the watch apps render any `items` page generically. Only a new page *kind*
//! needs watch code (unknown kinds show an "update the watch app" card).

/// Where an item's value comes from on the phone.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Source {
    FlightState,
    LaunchClock,
    StationLink,
    Boards,
    RocketFixAge,
    /// Newest of several telemetry channels: `(data_type, sender, index)`.
    Telemetry {
        channels: &'static [(&'static str, Option<&'static str>, usize)],
        quantity: Quantity,
        precision: usize,
        stale_after_ms: i64,
    },
}

/// Physical quantity, used for unit conversion when the watch is imperial.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Quantity {
    Length,
    Speed,
    Pressure(&'static str),
    Mass,
    Percent,
    Voltage,
    Count,
}

impl Quantity {
    /// Converts an SI-ish value into display value and unit.
    pub fn display(self, value: f64, metric: bool) -> (f64, &'static str) {
        match self {
            Quantity::Length if metric => (value, "m"),
            Quantity::Length => (value / 0.3048, "ft"),
            Quantity::Speed if metric => (value, "m/s"),
            Quantity::Speed => (value / 0.3048, "ft/s"),
            Quantity::Pressure(unit) => (value, unit),
            Quantity::Mass => (value, "kg"),
            Quantity::Percent => (value, "%"),
            Quantity::Voltage => (value, "V"),
            Quantity::Count => (value, ""),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ItemDef {
    pub id: &'static str,
    pub label: &'static str,
    pub group: &'static str,
    pub source: Source,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PageKind {
    Items,
    Cycle,
    Compass,
}

impl PageKind {
    pub fn wire(self) -> &'static str {
        match self {
            PageKind::Items => "items",
            PageKind::Cycle => "cycle",
            PageKind::Compass => "compass",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PageDef {
    pub id: &'static str,
    pub title: &'static str,
    pub kind: PageKind,
    pub items: &'static [&'static str],
    pub default_enabled: bool,
}

const FAST: i64 = 10_000;

pub const ITEMS: &[ItemDef] = &[
    ItemDef {
        // One operator state ("In flight · Descent"); the id stays for saved settings.
        id: "flight_state",
        label: "State",
        group: "Flight",
        source: Source::FlightState,
    },
    ItemDef {
        id: "t_clock",
        label: "T-clock",
        group: "Flight",
        source: Source::LaunchClock,
    },
    ItemDef {
        id: "altitude",
        label: "Altitude",
        group: "Trajectory",
        source: Source::Telemetry {
            channels: &[
                ("ASCENT_STATE", None, 4),
                ("DESCENT_STATE", None, 2),
                ("BAROMETER_DATA", None, 2),
            ],
            quantity: Quantity::Length,
            precision: 0,
            stale_after_ms: FAST,
        },
    },
    ItemDef {
        id: "velocity",
        label: "Velocity",
        group: "Trajectory",
        source: Source::Telemetry {
            channels: &[("ASCENT_STATE", None, 5), ("DESCENT_STATE", None, 3)],
            quantity: Quantity::Speed,
            precision: 1,
            stale_after_ms: FAST,
        },
    },
    ItemDef {
        id: "gps_altitude",
        label: "GPS altitude",
        group: "Trajectory",
        source: Source::Telemetry {
            channels: &[
                ("GPS_DATA", None, 2),
                ("GPS", None, 2),
                ("ROCKET_GPS", None, 2),
            ],
            quantity: Quantity::Length,
            precision: 0,
            stale_after_ms: 30_000,
        },
    },
    ItemDef {
        id: "gps_sats",
        label: "GPS satellites",
        group: "Trajectory",
        source: Source::Telemetry {
            channels: &[("GPS_SATELLITE_NUMBER", None, 0)],
            quantity: Quantity::Count,
            precision: 0,
            stale_after_ms: 30_000,
        },
    },
    ItemDef {
        id: "rocket_fix_age",
        label: "Rocket fix age",
        group: "Trajectory",
        source: Source::RocketFixAge,
    },
    ItemDef {
        id: "tank_pressure",
        label: "Tank pressure",
        group: "Propulsion",
        source: Source::Telemetry {
            channels: &[("FUEL_TANK_PRESSURE", None, 0)],
            quantity: Quantity::Pressure("psi"),
            precision: 0,
            stale_after_ms: FAST,
        },
    },
    ItemDef {
        id: "chamber_pressure",
        label: "Chamber pressure",
        group: "Propulsion",
        source: Source::Telemetry {
            channels: &[("COMBUSTION_CHAMBER_PRESSURE", None, 0)],
            quantity: Quantity::Pressure("psig"),
            precision: 0,
            stale_after_ms: FAST,
        },
    },
    ItemDef {
        id: "loadcell_mass",
        label: "Load cell mass",
        group: "Propulsion",
        source: Source::Telemetry {
            channels: &[("LOADCELL_WEIGHT_KG", None, 0)],
            quantity: Quantity::Mass,
            precision: 1,
            stale_after_ms: FAST,
        },
    },
    ItemDef {
        id: "fill_percent",
        label: "Fill level",
        group: "Propulsion",
        source: Source::Telemetry {
            channels: &[("LOADCELL_FILL_PERCENT", None, 0)],
            quantity: Quantity::Percent,
            precision: 0,
            stale_after_ms: FAST,
        },
    },
    ItemDef {
        id: "station_link",
        label: "Station link",
        group: "Health",
        source: Source::StationLink,
    },
    ItemDef {
        id: "boards",
        label: "Boards seen",
        group: "Health",
        source: Source::Boards,
    },
    ItemDef {
        id: "av_battery",
        label: "AV battery",
        group: "Health",
        source: Source::Telemetry {
            channels: &[("BATTERY_VOLTAGE", Some("PB"), 0)],
            quantity: Quantity::Voltage,
            precision: 2,
            stale_after_ms: 30_000,
        },
    },
];

pub const PAGES: &[PageDef] = &[
    PageDef {
        id: "flight",
        title: "Flight",
        kind: PageKind::Items,
        items: &["flight_state", "t_clock", "altitude", "velocity"],
        default_enabled: true,
    },
    PageDef {
        id: "propulsion",
        title: "Propulsion",
        kind: PageKind::Items,
        items: &[
            "tank_pressure",
            "chamber_pressure",
            "loadcell_mass",
            "fill_percent",
        ],
        default_enabled: true,
    },
    PageDef {
        id: "trajectory",
        title: "Trajectory",
        kind: PageKind::Items,
        items: &["altitude", "velocity", "gps_altitude", "gps_sats"],
        default_enabled: true,
    },
    PageDef {
        id: "health",
        title: "Health",
        kind: PageKind::Items,
        items: &["station_link", "boards", "av_battery", "rocket_fix_age"],
        default_enabled: true,
    },
    PageDef {
        id: "cycle",
        title: "Cycle",
        kind: PageKind::Cycle,
        items: &[],
        default_enabled: true,
    },
    PageDef {
        id: "compass",
        title: "Recovery",
        kind: PageKind::Compass,
        items: &[],
        default_enabled: true,
    },
];

/// Compass targets the phone can supply, in default order.
pub const TARGETS: &[(&str, &str)] = &[
    ("rocket", "Rocket"),
    ("ground_station", "Ground station"),
    ("rrls", "RRLS"),
];

pub fn item(id: &str) -> Option<&'static ItemDef> {
    ITEMS.iter().find(|i| i.id == id)
}

pub fn page(id: &str) -> Option<&'static PageDef> {
    PAGES.iter().find(|p| p.id == id)
}

pub fn target_known(id: &str) -> bool {
    TARGETS.iter().any(|(t, _)| *t == id)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn ids_are_unique_and_pages_reference_known_items() {
        let mut seen = HashSet::new();
        for i in ITEMS {
            assert!(seen.insert(i.id), "duplicate item {}", i.id);
            assert!(i.id.len() <= 32 && i.id.bytes().all(|b| b.is_ascii_lowercase() || b == b'_'));
        }
        let mut seen = HashSet::new();
        for p in PAGES {
            assert!(seen.insert(p.id), "duplicate page {}", p.id);
            for i in p.items {
                assert!(
                    item(i).is_some(),
                    "page {} references unknown item {i}",
                    p.id
                );
            }
            assert_eq!(p.kind == PageKind::Items, !p.items.is_empty());
        }
    }

    #[test]
    fn imperial_converts_lengths_and_speeds_only() {
        let (v, u) = Quantity::Length.display(100.0, false);
        assert!((v - 328.084).abs() < 1e-3 && u == "ft");
        let (v, u) = Quantity::Speed.display(3.048, false);
        assert!((v - 10.0).abs() < 1e-9 && u == "ft/s");
        assert_eq!(Quantity::Mass.display(5.0, false), (5.0, "kg"));
        assert_eq!(Quantity::Pressure("psi").display(5.0, true), (5.0, "psi"));
    }
}
