//! The demo scenario: a generic liquid/hybrid rocket on the pad, filled,
//! armed, counted down and flown, then landed. It loops every
//! [`LOOP_MS`]. The flight is the IGT `nominal_10k` mock flight from
//! GroundStation26 (`test-data/igt/nominal_10k`, about 10,000 ft AGL),
//! thinned to 2 Hz in `flight_10k.csv`; the ground phases are synthetic.
//!
//! Everything is served in the station's own shapes: the layout below is a
//! valid `GET /api/layout` (the dashboard, data tabs and actions come from it,
//! not from code in the workspace), the rows are `TelemetryRow`s and the
//! boards are `GET /api/boards` entries. Data types, senders and labels are
//! examples; another vehicle's station supplies its own.
use super::{Alert, Board, CalibrationCurve, Mode, Recording, Sample, Session, SessionParts, Source};
use serde_json::{Value, json};

/// One scenario pass: idle, fill, armed, countdown, flight, landed.
pub(crate) const LOOP_MS: i64 = 380_000;
/// The workspace opens partway through the fill, so launch is a minute away.
pub(crate) const OPEN_AT_MS: i64 = 92_000;
/// Flight file time 0 on the scenario clock (ignition is at file time 10 s).
const FLIGHT_OFFSET_MS: i64 = 130_000;
const IGNITION_MS: i64 = 140_000;
const BURNOUT_MS: i64 = 147_300;
const FLIGHT_CSV: &str = include_str!("flight_10k.csv");

struct FlightRow {
    t_ms: i64,
    alt_m: f32,
    vspeed: f32,
    accel: f32,
    lat: f32,
    lon: f32,
    pressure_pa: f32,
    temp_c: f32,
}

fn flight_rows() -> Vec<FlightRow> {
    FLIGHT_CSV
        .lines()
        .filter(|l| !l.starts_with('#') && !l.trim().is_empty())
        .filter_map(|line| {
            let v: Vec<f32> = line.split(',').filter_map(|x| x.trim().parse().ok()).collect();
            (v.len() == 8).then(|| FlightRow { t_ms: v[0] as i64, alt_m: v[1], vspeed: v[2], accel: v[3], lat: v[4], lon: v[5], pressure_pa: v[6], temp_c: v[7] })
        })
        .collect()
}

/// Deterministic noise in [-1, 1].
fn noise(t: i64, salt: u64) -> f32 {
    let mut x = (t as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15) ^ salt.wrapping_mul(0xC2B2_AE3D_27D4_EB4F);
    x ^= x >> 29;
    x = x.wrapping_mul(0xBF58_476D_1CE4_E5B9);
    x ^= x >> 32;
    (x % 20_001) as f32 / 10_000.0 - 1.0
}

fn ramp(t: i64, from: i64, to: i64) -> f32 {
    ((t - from) as f32 / (to - from) as f32).clamp(0.0, 1.0)
}

pub(crate) fn state_timeline() -> Vec<(i64, String)> {
    [
        (0, "Idle"),
        (20_000, "PreFill"),
        (30_000, "NitrogenFill"),
        (45_000, "NitrousFill"),
        (110_000, "Armed"),
        (130_000, "Launch"),
        (140_150, "Ascent"),
        (147_300, "Coast"),
        (166_600, "Descent"),
        (334_000, "Landed"),
    ]
    .into_iter()
    .map(|(t, s)| (t, s.to_string()))
    .collect()
}

/// Valve outputs (1 = actuated) in `VALVE_STATE` order:
/// Pilot, Vent, Dump, Igniter, Nitrogen, Nitrous, Fill lines (1 = retracted).
fn valves(t: i64) -> [f32; 7] {
    let fill = (30_000..110_000).contains(&t);
    let vent = !fill && t < 115_000 || (60_000..63_000).contains(&t) || (85_000..87_500).contains(&t);
    [
        f32::from((IGNITION_MS..BURNOUT_MS).contains(&t)),
        f32::from(vent),
        0.0,
        f32::from((139_500..141_000).contains(&t)),
        f32::from((30_000..45_000).contains(&t)),
        f32::from((45_000..104_000).contains(&t)),
        f32::from(t >= 118_000),
    ]
}

const FULL_KG: f32 = 10.5;

fn ox_mass(t: i64) -> f32 {
    if t < BURNOUT_MS {
        FULL_KG * ramp(t, 45_000, 104_000) * (1.0 - ramp(t, IGNITION_MS, BURNOUT_MS))
    } else {
        0.0
    }
}

fn tank_psi(t: i64) -> f32 {
    let n2 = 290.0 * ramp(t, 30_000, 45_000);
    let filled = n2 + 455.0 * ramp(t, 45_000, 104_000);
    let vent_dips = if (60_000..63_000).contains(&t) || (85_000..87_500).contains(&t) { -40.0 } else { 0.0 };
    let pressure = if t < IGNITION_MS { filled + vent_dips } else { filled * (1.0 - 0.82 * ramp(t, IGNITION_MS, BURNOUT_MS)) * (1.0 - ramp(t, BURNOUT_MS, BURNOUT_MS + 8_000)) };
    if t < 30_000 { 0.4 } else { pressure.max(0.0) }
}

fn chamber_psi(t: i64) -> f32 {
    if !(IGNITION_MS..BURNOUT_MS + 600).contains(&t) {
        return 0.0;
    }
    let rise = ramp(t, IGNITION_MS, IGNITION_MS + 400);
    let fall = 1.0 - ramp(t, BURNOUT_MS - 1_500, BURNOUT_MS + 600);
    (420.0 - 60.0 * ramp(t, IGNITION_MS, BURNOUT_MS)) * rise * fall
}

pub(crate) fn layout() -> Value {
    let summary = |ty: &str, items: Value| json!({"kind": "summary", "data_type": ty, "items": items});
    let fmt = |precision: u32, suffix: &str| json!({"kind": "number", "precision": precision, "suffix": suffix});
    let batteries = json!({"kind": "summary", "data_type": "BATTERY_VOLTAGE", "items": [
        {"label": "Avionics battery (PB)", "index": 0, "sender_id": "PB", "formatter": fmt(2, " V")},
        {"label": "Valve board (VB)", "index": 0, "sender_id": "VB", "formatter": fmt(2, " V")},
        {"label": "Fill box (GB)", "index": 0, "sender_id": "GB", "formatter": fmt(2, " V")}
    ]});
    let valves = json!({"kind": "valve_state", "data_type": "VALVE_STATE", "valves": [
        {"label": "Main", "index": 0}, {"label": "Vent", "index": 1}, {"label": "Dump", "index": 2},
        {"label": "Igniter", "index": 3}, {"label": "Nitrogen", "index": 4}, {"label": "Nitrous", "index": 5},
        {"label": "Fill lines", "index": 6, "true_label": "Retracted", "false_label": "Connected"}
    ]});
    let fill = json!([
        summary("OX_TANK_PRESSURE", json!([{"label": "Tank pressure", "index": 0, "formatter": fmt(0, " psi")}])),
        summary("LOADCELL_50_WEIGHT_KG", json!([{"label": "Oxidizer mass", "index": 0, "formatter": fmt(2, " kg")}])),
        summary("LOADCELL_FILL_PERCENT", json!([{"label": "Fill level", "index": 0, "formatter": fmt(0, " %")}])),
        {"kind": "chart", "chart_title": "Fill", "chart_series": [
            {"data_type": "OX_TANK_PRESSURE", "index": 0, "label": "Tank pressure (psi)"},
            {"data_type": "LOADCELL_FILL_PERCENT", "index": 0, "label": "Fill level (%)"}
        ]}
    ]);
    let flight = json!([
        summary("BAROMETER_DATA", json!([{"label": "Altitude AGL", "index": 0, "formatter": fmt(0, " m")}])),
        summary("VEHICLE_SPEED", json!([{"label": "Vertical speed", "index": 0, "formatter": fmt(1, " m/s")}])),
        summary("ACCEL_DATA", json!([{"label": "Acceleration", "index": 2, "formatter": fmt(1, " m/s²")}])),
        summary("GPS_SATELLITE_NUMBER", json!([{"label": "GPS satellites", "index": 0, "formatter": {"kind": "integer"}}])),
        {"kind": "chart", "chart_title": "Altitude and speed", "chart_series": [
            {"data_type": "BAROMETER_DATA", "index": 0, "label": "Altitude (m)"},
            {"data_type": "VEHICLE_SPEED", "index": 0, "label": "Vertical speed (m/s)"}
        ]},
        {"kind": "map"}
    ]);
    let tab = |id: &str, label: &str, channels: Value, formatters: Value| json!({"id": id, "label": label, "channels": channels, "channel_formatters": formatters, "chart": {"enabled": true}});
    json!({
        "version": 1,
        "branding": {"app_name": "Demo station", "dashboard_title": "Demo rocket"},
        "main_tabs": ["state", "data", "connection-status", "map", "actions"],
        "connection_tab": {"sections": [{"kind": "board_status", "title": "Boards"}]},
        "network_tab": {"enabled": true, "title": "Network", "expected_boards": ["FC", "RF", "PB", "VB", "GB", "AB", "DAQ"]},
        "actions_tab": {"disable_actions_by_default": false, "show_flight_setup": false, "show_fill_targets": false, "actions": [
            {"label": "Launch", "cmd": "Launch", "border": "#22c55e", "bg": "#022c22", "fg": "#bbf7d0", "group": "Flight controls"},
            {"label": "Fill lines", "cmd": "RetractPlumbing", "border": "#eab308", "bg": "#1f2937", "fg": "#fef9c3", "group": "Flight controls"},
            {"label": "Vent", "cmd": "NormallyOpen", "border": "#60a5fa", "bg": "#0b1220", "fg": "#bfdbfe", "group": "Manual GSE valves"},
            {"label": "Dump", "cmd": "Dump", "border": "#ef4444", "bg": "#450a0a", "fg": "#fecaca", "group": "Manual GSE valves"},
            {"label": "Nitrogen", "cmd": "Nitrogen", "border": "#60a5fa", "bg": "#0b1220", "fg": "#bfdbfe", "group": "Manual GSE valves"},
            {"label": "Nitrous", "cmd": "Nitrous", "border": "#60a5fa", "bg": "#0b1220", "fg": "#bfdbfe", "group": "Manual GSE valves"},
            {"label": "Abort", "cmd": "Abort", "border": "#ef4444", "bg": "#450a0a", "fg": "#fecaca", "group": "Flight controls"}
        ]},
        "data_tab": {
            "sender_split_data_types": ["BATTERY_VOLTAGE"],
            "tabs": [
                tab("OX_TANK_PRESSURE", "Tank pressure", json!(["Pressure"]), json!([fmt(0, " psi")])),
                tab("PRESSURE_TRANSDUCER_CALIBRATED", "Tank transducer (VB)", json!(["Pressure"]), json!([fmt(0, " psi")])),
                tab("LOADCELL_50_WEIGHT_KG", "Fill loadcell", json!(["Mass"]), json!([fmt(2, " kg")])),
                tab("LOADCELL_FILL_PERCENT", "Fill level", json!(["Level"]), json!([fmt(0, " %")])),
                tab("COMBUSTION_CHAMBER_PRESSURE", "Chamber pressure", json!(["Pressure"]), json!([fmt(0, " psig")])),
                tab("BAROMETER_DATA", "Barometer", json!(["Altitude", "Pressure", "Temperature"]), json!([fmt(0, " m"), fmt(0, " Pa"), fmt(1, " °C")])),
                tab("VEHICLE_SPEED", "Vertical speed", json!(["Speed"]), json!([fmt(1, " m/s")])),
                tab("ACCEL_DATA", "Acceleration", json!(["X", "Y", "Z"]), json!([fmt(1, " m/s²"), fmt(1, " m/s²"), fmt(1, " m/s²")])),
                tab("GPS_DATA", "GPS", json!(["Latitude", "Longitude", "Altitude MSL"]), json!([fmt(6, "°"), fmt(6, "°"), fmt(0, " m")])),
                tab("GPS_SATELLITE_NUMBER", "GPS satellites", json!(["Satellites"]), json!([{"kind": "integer"}])),
                tab("BATTERY_VOLTAGE", "Battery voltage", json!(["Voltage"]), json!([fmt(2, " V")])),
                tab("RADIO_LINK", "Radio link", json!(["RSSI", "SNR"]), json!([fmt(0, " dBm"), fmt(1, " dB")])),
                tab("VALVE_STATE", "Valve state", json!(["Main", "Vent", "Dump", "Igniter", "Nitrogen", "Nitrous", "Fill lines"]), json!([]))
            ]
        },
        "state_tab": {"states": [
            {"states": ["Startup", "Idle"], "sections": [
                {"title": "Connected boards", "widgets": [{"kind": "board_status"}]},
                {"title": "Batteries", "widgets": [batteries.clone()]},
                {"title": "Valves", "widgets": [valves.clone()]}
            ]},
            {"states": ["PreFill", "FillTest", "NitrogenFill", "NitrousFill", "Aborted"], "sections": [
                {"title": "Fill", "widgets": fill},
                {"title": "Valves", "widgets": [valves.clone()]},
                {"title": "Actions", "widgets": [{"kind": "actions", "actions": ["NormallyOpen", "Dump", "Nitrogen", "Nitrous", "RetractPlumbing"]}]},
                {"title": "Batteries", "widgets": [batteries.clone()]}
            ]},
            {"states": ["Armed", "Launch"], "sections": [
                {"title": "Pad", "widgets": [
                    summary("OX_TANK_PRESSURE", json!([{"label": "Tank pressure", "index": 0, "formatter": fmt(0, " psi")}])),
                    summary("LOADCELL_50_WEIGHT_KG", json!([{"label": "Oxidizer mass", "index": 0, "formatter": fmt(2, " kg")}])),
                    summary("COMBUSTION_CHAMBER_PRESSURE", json!([{"label": "Chamber pressure", "index": 0, "formatter": fmt(0, " psig")}])),
                    {"kind": "chart", "chart_title": "Tank and chamber", "chart_series": [
                        {"data_type": "OX_TANK_PRESSURE", "index": 0, "label": "Tank (psi)"},
                        {"data_type": "COMBUSTION_CHAMBER_PRESSURE", "index": 0, "label": "Chamber (psig)"}
                    ]}
                ]},
                {"title": "Valves", "widgets": [valves.clone()]},
                {"title": "Actions", "widgets": [{"kind": "actions", "actions": ["Launch", "Abort"]}]},
                {"title": "Batteries", "widgets": [batteries.clone()]}
            ]},
            {"states": ["Ascent", "Coast", "Apogee", "Descent", "Landed", "Recovery"], "sections": [
                {"title": "Flight", "widgets": flight},
                {"title": "Batteries", "widgets": [batteries]}
            ]}
        ]}
    })
}

/// Commands the demo station's action policy lists beside the layout's
/// buttons: the close halves of the supply valves, as a station lists them.
pub(crate) const PAIRED_COMMANDS: &[&str] = &["NitrogenClose", "NitrousClose"];

/// The demo's pad checklists (`GET /api/checklists`).
pub(crate) fn checklists() -> Value {
    let item = |id: &str, text: &str| json!({"id": id, "text": text, "checked": false, "completed_by": null, "completed_at_ms": null});
    json!({"revision": 1, "enforce_before_launch": false, "checklists": [
        {"id": "pad", "title": "Pad checks (demo)", "required_before_launch": false, "items": [
            item("pt", "Pressure transducer reading checked"),
            item("lines", "Gas lines and connections inspected"),
            item("comms", "Communications and abort procedure checked")
        ]}
    ]})
}

fn boards() -> Vec<Board> {
    [("FC", "Flight Computer"), ("RF", "RF Board"), ("PB", "Power Board"), ("VB", "Valve Board"), ("GB", "Gateway Board"), ("AB", "Actuator Board"), ("DAQ", "DAQ Board")]
        .into_iter()
        .map(|(sender, label)| Board { sender: sender.into(), label: label.into() })
        .collect()
}

fn push(out: &mut Vec<Sample>, t_ms: i64, sender: &str, data_type: &str, values: &[f32]) {
    out.push(Sample { t_ms, sender: sender.into(), data_type: data_type.into(), values: values.iter().map(|v| Some(*v)).collect() });
}

fn samples() -> Vec<Sample> {
    let flight = flight_rows();
    let pad = flight.first().map(|r| (r.lat, r.lon, r.alt_m, r.pressure_pa, r.temp_c)).unwrap_or((31.758, -102.046, 0.0, 91_077.0, 30.0));
    let mut out = Vec::with_capacity(24_000);
    // Ground equipment at 5 Hz, valves and batteries at 1 Hz.
    for t in (0..LOOP_MS).step_by(200) {
        let mass = ox_mass(t) + 0.02 * noise(t, 1);
        push(&mut out, t, "DAQ", "LOADCELL_50_WEIGHT_KG", &[mass]);
        push(&mut out, t, "DAQ", "LOADCELL_FILL_PERCENT", &[(100.0 * mass / FULL_KG).clamp(0.0, 100.0)]);
        let tank = tank_psi(t) + 1.5 * noise(t, 2);
        push(&mut out, t, "VB", "OX_TANK_PRESSURE", &[tank]);
        push(&mut out, t, "VB", "PRESSURE_TRANSDUCER_CALIBRATED", &[tank]);
        push(&mut out, t, "DAQ", "COMBUSTION_CHAMBER_PRESSURE", &[(chamber_psi(t) + 2.0 * noise(t, 3)).max(0.0)]);
        if t % 1_000 == 0 {
            push(&mut out, t, "VB", "VALVE_STATE", &valves(t));
            let minutes = t as f32 / 60_000.0;
            // The avionics battery sags under load and ends near its warning level.
            push(&mut out, t, "PB", "BATTERY_VOLTAGE", &[7.92 - 0.09 * minutes + 0.01 * noise(t, 4)]);
            push(&mut out, t, "VB", "BATTERY_VOLTAGE", &[14.9 - 0.03 * minutes + 0.01 * noise(t, 5)]);
            push(&mut out, t, "GB", "BATTERY_VOLTAGE", &[14.7 - 0.02 * minutes + 0.01 * noise(t, 6)]);
            push(&mut out, t, "AB", "BATTERY_VOLTAGE", &[14.6 - 0.02 * minutes + 0.01 * noise(t, 7)]);
            let range_km = flight.iter().find(|r| r.t_ms + FLIGHT_OFFSET_MS >= t).map(|r| r.alt_m / 1000.0).unwrap_or(0.0);
            push(&mut out, t, "RF", "RADIO_LINK", &[-71.0 - 6.0 * range_km + 2.0 * noise(t, 8), 9.5 - 1.5 * range_km + 0.5 * noise(t, 9)]);
        }
        // On the pad the avionics report at 2 Hz until the flight file starts.
        if t < FLIGHT_OFFSET_MS && t % 500 == 0 {
            push(&mut out, t, "FC", "GPS_DATA", &[pad.0 + 0.000_002 * noise(t, 10), pad.1 + 0.000_002 * noise(t, 11), 890.0 + noise(t, 12)]);
            push(&mut out, t, "FC", "BAROMETER_DATA", &[pad.2 + 0.2 * noise(t, 13), pad.3, pad.4]);
            push(&mut out, t, "FC", "ACCEL_DATA", &[0.05 * noise(t, 14), 0.05 * noise(t, 15), 9.81]);
            push(&mut out, t, "FC", "VEHICLE_SPEED", &[0.0]);
            push(&mut out, t, "FC", "GPS_SATELLITE_NUMBER", &[11.0]);
        }
    }
    for r in &flight {
        let t = r.t_ms + FLIGHT_OFFSET_MS;
        if t >= LOOP_MS {
            break;
        }
        push(&mut out, t, "FC", "GPS_DATA", &[r.lat, r.lon, r.alt_m + 890.0]);
        push(&mut out, t, "FC", "BAROMETER_DATA", &[r.alt_m, r.pressure_pa, r.temp_c]);
        push(&mut out, t, "FC", "ACCEL_DATA", &[0.3 * noise(t, 16), 0.3 * noise(t, 17), r.accel]);
        push(&mut out, t, "FC", "VEHICLE_SPEED", &[r.vspeed]);
        push(&mut out, t, "FC", "GPS_SATELLITE_NUMBER", &[if r.alt_m > 2_500.0 { 7.0 } else { 10.0 }]);
    }
    out
}

fn alerts() -> Vec<Alert> {
    vec![
        Alert { t_ms: 25_000, warning: true, message: "Fill box battery (GB) below 14.8 V (demo warning).".into() },
        Alert { t_ms: 61_000, warning: true, message: "Tank pressure dip while venting (demo warning).".into() },
        Alert { t_ms: 160_000, warning: true, message: "Radio link margin low: RSSI below −85 dBm (demo warning).".into() },
        Alert { t_ms: 300_000, warning: true, message: "Avionics battery (PB) below 7.5 V (demo warning).".into() },
    ]
}

fn calibration() -> Vec<CalibrationCurve> {
    let points = [(0.512, 0.0), (0.874, 5.0), (1.236, 10.0), (1.598, 15.0), (1.961, 20.0)];
    vec![CalibrationCurve {
        label: "Fill loadcell (KG50)".into(),
        raw_unit: "V".into(),
        unit: "kg".into(),
        points: points.iter().map(|(r, e)| (*r as f32, *e as f32)).collect(),
        slope: 13.80,
        offset: -7.07,
    }]
}

fn recordings() -> Vec<Recording> {
    vec![
        Recording { id: "demo-flight.db".into(), label: "Demo flight (this scenario)".into(), kind: "capture".into(), bytes: 18_400_000, start_ms: 0, end_ms: LOOP_MS },
    ]
}

pub(crate) fn session(epoch_ms: i64) -> Session {
    let mut session = Session::new(SessionParts {
        source: Source::Demo,
        title: "Demo rocket".into(),
        mode: Mode::Flight,
        provenance: "Demo".into(),
        layout: layout(),
        boards: boards(),
        samples: samples(),
        states: state_timeline(),
        alerts: alerts(),
        calibration: calibration(),
        recordings: recordings(),
        replay_document: None,
        vehicle_geometry: None,
        notes: vec!["Flight profile: GroundStation26 IGT nominal_10k mock flight; fill and pad data are synthetic.".into()],
        epoch_ms,
    });
    session.replay_document = Some(replay_document(&session));
    session
}

/// The demo as a portable replay document, for the 3D player.
pub(crate) fn replay_document(session: &Session) -> Value {
    let samples: Vec<Value> = session
        .samples
        .iter()
        .filter(|s| matches!(s.data_type.as_str(), "VALVE_STATE" | "GPS_DATA" | "BAROMETER_DATA" | "LOADCELL_FILL_PERCENT" | "OX_TANK_PRESSURE"))
        .enumerate()
        .map(|(id, s)| json!({"id": id + 1, "timestamp_ms": s.t_ms, "sender_id": s.sender, "data_type": s.data_type, "values": s.values}))
        .collect();
    json!({
        "format": "seds-replay", "version": 1, "recording": "demo-flight.db", "mode": "flight",
        "configuration_provenance": "Demo scenario generated on this device",
        "schema": {"types": []},
        "layout": session.layout,
        "view_config": {"stages": [], "ground_systems": [
            {"id": "ox", "label": "Oxidizer tank", "kind": "tank", "unit": "%", "min": 0, "max": 100, "binding": {"data_type": "LOADCELL_FILL_PERCENT", "index": 0}}
        ]},
        "valve_mappings": [
            {"label": "Main", "data_type": "VALVE_STATE", "index": 0},
            {"label": "Vent", "data_type": "VALVE_STATE", "index": 1},
            {"label": "Dump", "data_type": "VALVE_STATE", "index": 2}
        ],
        "samples": samples,
    })
}
