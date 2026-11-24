//! The report studio's `preview`, `csv`, `daq_csv` and `daq_tsv` formats,
//! ported from the station's worker (GroundStation26
//! `backend/src/recording_report.py`) for one recording read from a replay
//! bundle. CSV and DAQ files are byte-identical to the station's; the
//! preview is the same JSON. Function names follow the Python ones.
use super::py::{Py, repr_float};
use super::{HistoryRow, Row};
use std::collections::{HashMap, HashSet};

/// An SD-resolved recording's row provenance by row id (`read_rows`'s
/// `provenance`); `None` for other recordings.
type Provenance = Option<HashMap<i64, Py>>;

fn origin<'a>(provenance: &'a Provenance, row_id: i64) -> Option<&'a Py> {
    provenance.as_ref()?.get(&row_id)
}

fn is_sd(origin: &Py) -> bool {
    origin.get("origin").and_then(Py::as_str) == Some("sd")
}

/// `derived_sd`: an SD value computed from several records (boxcar, paired
/// supply), so not one physical SD record to copy verbatim.
fn derived_sd(origin: &Py) -> bool {
    origin.get("mapping").filter(|m| m.is_dict()).and_then(|m| m.get("derived")).and_then(Py::as_str).is_some_and(|d| d == "boxcar" || d == "pair")
}

/// The dict without `fields` (the provenance the exports write).
fn without_fields(origin: &Py) -> Py {
    match origin {
        Py::Dict(items) => Py::Dict(items.iter().filter(|(k, _)| k != "fields").cloned().collect()),
        other => other.clone(),
    }
}

/// `x not in (None, "")`.
fn present(value: Option<&Py>) -> bool {
    !matches!(value, None | Some(Py::None)) && value.and_then(Py::as_str) != Some("")
}

const MAX_ROWS: usize = 200_000;
const MAX_SERIES: usize = 128;
const EQUATION: &str = "y = f(raw) - f(zero_raw) if tare exists, else f(raw); linear: f(x)=m*x+b; polyN: descending a..e coefficients evaluated at (x-x0)";
const VALVE_CHANNELS: [&str; 7] = ["Pilot valve", "Normally-open valve", "Dump valve", "Igniter", "Nitrogen valve", "Nitrous valve", "Plumbing"];
const DAQ_COLUMNS: [&str; 13] = [
    "timestamp_ms", "monotonic_ms", "sensor", "value", "raw_adc_code", "raw_value", "calibrated_value", "time_source",
    "adc_temperature_c", "adc_temperature_code", "uptime_ticks", "uptime_frequency_hz", "record_sequence",
];

/// A calibrated type and the raw type it is computed from.
fn raw_type(kind: &str) -> Option<&'static str> {
    match kind {
        "LOADCELL_WEIGHT_KG" => Some("KG1000"),
        "LOADCELL_50_WEIGHT_KG" => Some("KG50"),
        "PRESSURE_TRANSDUCER_CALIBRATED" => Some("OX_TANK_PRESSURE"),
        _ => None,
    }
}

fn daq_sd_sensor(sender: &str, kind: &str, index: usize) -> Option<&'static str> {
    match (sender, kind, index) {
        ("DAQ", "KG1000", 0) => Some("kg1000_v"),
        ("DAQ", "KG50", 0) => Some("kg50_v"),
        ("DAQ", "DAQ_POWER_MONITOR", 2) => Some("input_voltage_v"),
        ("DAQ", "DAQ_POWER_MONITOR", 3) => Some("input_current_a"),
        _ => None,
    }
}

/// A report failure: the station's HTTP status and message.
pub(crate) type Failure = (u16, String);

fn fail<T>(message: &str) -> Result<T, Failure> {
    Err((422, message.to_owned()))
}

#[derive(Clone)]
struct Sample<'a> {
    t: i64,
    value: Option<Py>,
    source: Option<i64>,
    recording: &'a str,
    row_id: i64,
}

struct Stream<'a> {
    key: String,
    sender: String,
    kind: String,
    index: usize,
    samples: Vec<Sample<'a>>,
}

/// Python's `sum()` (3.12+): exact int sums, then Neumaier-compensated
/// float addition; ints met after the first float are added plainly.
enum Num {
    Int(i64),
    Float(f64),
}

fn num(value: &Py) -> Num {
    match value {
        Py::Bool(b) => Num::Int(i64::from(*b)),
        Py::Int(i) => Num::Int(*i),
        Py::Float(f) => Num::Float(*f),
        _ => Num::Float(f64::NAN),
    }
}

fn py_sum(items: impl Iterator<Item = Num>) -> Num {
    let mut items = items.peekable();
    let mut int = 0i64;
    let mut f = loop {
        match items.next() {
            None => return Num::Int(int),
            Some(Num::Int(i)) => int = int.wrapping_add(i),
            Some(Num::Float(x)) => break int as f64 + x,
        }
    };
    let mut c = 0.0f64;
    for item in items {
        match item {
            Num::Float(x) => {
                let t = f + x;
                if f.abs() >= x.abs() {
                    c += (f - t) + x;
                } else {
                    c += (x - t) + f;
                }
                f = t;
            }
            Num::Int(i) => f += i as f64,
        }
    }
    if c != 0.0 && c.is_finite() {
        f += c;
    }
    Num::Float(f)
}

fn to_f64(n: &Num) -> f64 {
    match n {
        Num::Int(i) => *i as f64,
        Num::Float(f) => *f,
    }
}

fn mean(items: &[Number]) -> f64 {
    to_f64(&py_sum(items.iter().map(|v| v.num()))) / items.len() as f64
}

/// A recorded number as Python holds it (bool, int or float).
#[derive(Clone, Copy)]
enum Number {
    Int(i64),
    Float(f64),
}
impl Number {
    fn of(value: &Py) -> Number {
        match num(value) {
            Num::Int(i) => Number::Int(i),
            Num::Float(f) => Number::Float(f),
        }
    }
    fn num(self) -> Num {
        match self {
            Number::Int(i) => Num::Int(i),
            Number::Float(f) => Num::Float(f),
        }
    }
    fn f(self) -> f64 {
        match self {
            Number::Int(i) => i as f64,
            Number::Float(f) => f,
        }
    }
}

fn key_of(sender: &str, kind: &str, index: usize) -> String {
    Py::List(vec![Py::str(sender), Py::str(kind), Py::Int(index as i64)]).dumps(true, false)
}

fn channel_label(stream: &Stream) -> String {
    if stream.kind == "VALVE_STATE" && stream.index < VALVE_CHANNELS.len() {
        return VALVE_CHANNELS[stream.index].to_owned();
    }
    format!("{} / {} / {}", stream.sender, stream.kind, stream.index)
}

fn state_label(stream: &Stream, value: Option<&Py>, mapping: Option<&Py>) -> String {
    if stream.kind != "VALVE_STATE" || stream.index >= VALVE_CHANNELS.len() {
        return String::new();
    }
    let word = |name: &str| mapping.and_then(|m| m.get(name)).filter(|v| v.truthy()).map(Py::py_str);
    let Some(bit) = value.and_then(Py::number).filter(|v| *v == 0.0 || *v == 1.0) else {
        return word("unknown_label").unwrap_or_else(|| "Unknown".into());
    };
    let labels = match stream.index {
        3 => ("Off", "On"),
        6 => ("Extended", "Retracted"),
        _ => ("Closed", "Open"),
    };
    word(if bit == 1.0 { "true_label" } else { "false_label" }).unwrap_or_else(|| if bit == 1.0 { labels.1 } else { labels.0 }.into())
}

/// Calibration history by (recording, sender, sensor): consecutive equal
/// snapshots collapse, as the worker reads them.
type History = HashMap<(String, String, String), Vec<(i64, Py)>>;

fn history(recording: &str, rows: &[HistoryRow]) -> Result<History, Failure> {
    let mut out: History = HashMap::new();
    for row in rows {
        let snapshot = Py::parse(&row.config_json).map_err(|e| (422, e))?;
        let entries = out.entry((recording.to_owned(), row.sender_id.clone(), row.sensor_id.clone())).or_default();
        if entries.last().is_none_or(|(_, last)| !last.py_eq(&snapshot)) {
            entries.push((row.timestamp_ms, snapshot));
        }
    }
    Ok(out)
}

fn missing_key(key: &str) -> Failure {
    (422, format!("'{key}'"))
}

fn fill_source_kg1000(snapshot: &Py) -> bool {
    snapshot.get("fill_targets").and_then(|t| t.get("fill_source")).and_then(Py::as_str) == Some("kg1000_absolute")
}

fn regression(stream: &Stream, streams: &[Stream], history: &History, has_history: bool) -> Result<Py, Failure> {
    let (sender, kind, index) = (&stream.sender, stream.kind.as_str(), stream.index);
    let sensor = match kind {
        "LOADCELL_FILL_PERCENT" => Some("KG50"),
        "PRESSURE_TRANSDUCER_CALIBRATED" => Some("IADC"),
        other => raw_type(other),
    };
    if let Some(sensor) = sensor
        && has_history
    {
        let mut epochs: Vec<(String, Py)> = Vec::new();
        let mut missing = 0i64;
        let mut indexes: HashMap<&str, Vec<(i64, Py)>> = HashMap::new();
        for sample in &stream.samples {
            let records = indexes.entry(sample.recording).or_insert_with(|| {
                let get = |s: &str| history.get(&(sample.recording.to_owned(), sender.clone(), s.to_owned())).cloned().unwrap_or_default();
                if kind == "LOADCELL_FILL_PERCENT" {
                    let mut merged: Vec<(i64, Py)> = ["KG50", "KG1000"]
                        .into_iter()
                        .flat_map(|source| get(source).into_iter().filter(move |e| source == if fill_source_kg1000(&e.1) { "KG1000" } else { "KG50" }))
                        .collect();
                    merged.sort_by_key(|e| e.0);
                    merged
                } else {
                    get(sensor)
                }
            });
            let at = records.partition_point(|e| e.0 <= sample.t);
            let Some(i) = at.checked_sub(1) else {
                missing += 1;
                continue;
            };
            let (stamp, snapshot) = &records[i];
            let cfg = snapshot.get("calibration").ok_or_else(|| missing_key("calibration"))?;
            let sample_sensor = if kind == "LOADCELL_FILL_PERCENT" {
                if fill_source_kg1000(snapshot) { "KG1000" } else { "KG50" }
            } else {
                sensor
            };
            let name = match sample_sensor {
                "KG1000" => "ch1",
                "KG50" => "kg50",
                _ => "iadc",
            };
            let channel = if name == "kg50" {
                cfg.get("extra_channels")
                    .and_then(|extra| extra.get("kg50"))
                    .cloned()
                    .unwrap_or_else(|| Py::dict(vec![("linear", Py::dict(vec![("m", Py::Int(1)), ("b", Py::Int(0))]))]))
            } else {
                Py::dict(vec![
                    ("linear", cfg.get(name).cloned().ok_or_else(|| missing_key(name))?),
                    ("fit", cfg.get(&format!("{name}_fit")).cloned().unwrap_or(Py::None)),
                    ("zero_raw", cfg.get(&format!("{name}_zero_raw")).cloned().unwrap_or(Py::None)),
                    ("points", cfg.get(&format!("points_{name}")).cloned().unwrap_or(Py::List(Vec::new()))),
                ])
            };
            let signature = Py::List(vec![Py::str(sample.recording), Py::Int(*stamp), channel.clone()]).dumps(false, true);
            let position = match epochs.iter().position(|(s, _)| *s == signature) {
                Some(p) => p,
                None => {
                    epochs.push((
                        signature,
                        Py::dict(vec![
                            ("recording", Py::str(sample.recording)),
                            ("first_sample_ms", Py::Int(sample.t)),
                            ("last_sample_ms", Py::Int(sample.t)),
                            ("channel", channel),
                            ("equation", Py::str(EQUATION)),
                            ("samples", Py::Int(0)),
                        ]),
                    ));
                    epochs.len() - 1
                }
            };
            let entry = &mut epochs[position].1;
            entry.set("last_sample_ms", Py::Int(sample.t));
            let count = match entry.get("samples") {
                Some(Py::Int(n)) => *n,
                _ => 0,
            };
            entry.set("samples", Py::Int(count + 1));
            if kind == "LOADCELL_FILL_PERCENT" {
                entry.set("fill_targets", snapshot.get("fill_targets").cloned().unwrap_or(Py::None));
                entry.set("flight_state", snapshot.get("flight_state").cloned().unwrap_or(Py::None));
                entry.set("fill_source", Py::str(sample_sensor));
                let source = if sample_sensor == "KG1000" { "abs(calibrated_kg)" } else { "calibrated_kg" };
                entry.set("postprocess", Py::str(format!("clamp(100 * {source} / active_target_kg, 0, 100)")));
            }
        }
        if !epochs.is_empty() {
            return Ok(Py::dict(vec![
                ("status", Py::str(if missing == 0 { "recorded_calibration" } else { "partially_recorded_calibration" })),
                ("epochs", Py::List(epochs.into_iter().map(|(_, e)| e).collect())),
                ("samples_without_metadata", Py::Int(missing)),
            ]));
        }
    }
    let Some(raw) = raw_type(kind) else {
        if kind.contains("CALIBRAT") || kind == "LOADCELL_FILL_PERCENT" {
            return Ok(Py::dict(vec![("status", Py::str("unavailable")), ("reason", Py::str("No historical regression recorded for this derived channel."))]));
        }
        return Ok(Py::dict(vec![("status", Py::str("not_applicable"))]));
    };
    let raw_key = key_of(sender, raw, index);
    let mut lookup: HashMap<(i64, Option<i64>, &str), Option<&Py>> = HashMap::new();
    if let Some(raw_stream) = streams.iter().find(|s| s.key == raw_key) {
        for s in &raw_stream.samples {
            lookup.insert((s.t, s.source, s.recording), s.value.as_ref());
        }
    }
    let pairs: Vec<(Number, Number)> = stream
        .samples
        .iter()
        .filter_map(|s| {
            let y = s.value.as_ref()?;
            let x = (*lookup.get(&(s.t, s.source, s.recording))?)?;
            Some((Number::of(x), Number::of(y)))
        })
        .collect();
    let unavailable = |reason: &str| Ok(Py::dict(vec![("status", Py::str("unavailable")), ("reason", Py::str(reason))]));
    if pairs.len() < 3 {
        return unavailable("Insufficient matched raw/calibrated samples; current calibration is not substituted.");
    }
    let xs: Vec<Number> = pairs.iter().map(|p| p.0).collect();
    let ys: Vec<Number> = pairs.iter().map(|p| p.1).collect();
    let (mx, my) = (mean(&xs), mean(&ys));
    let xx = to_f64(&py_sum(xs.iter().map(|x| {
        let d = x.f() - mx;
        Num::Float(d * d)
    })));
    if xx < 1e-24 {
        return unavailable("Raw samples have insufficient variation.");
    }
    let slope = to_f64(&py_sum(pairs.iter().map(|(x, y)| Num::Float((x.f() - mx) * (y.f() - my))))) / xx;
    let intercept = my - slope * mx;
    let residual = pairs.iter().map(|(x, y)| (y.f() - (slope * x.f() + intercept)).abs()).fold(f64::NEG_INFINITY, |a, b| if b > a { b } else { a });
    let largest = ys.iter().map(|y| y.f().abs()).fold(f64::NEG_INFINITY, |a, b| if b > a { b } else { a });
    let tolerance = if largest * 2e-6 > 1e-5 { largest * 2e-6 } else { 1e-5 };
    if residual > tolerance {
        return unavailable("Samples do not support one linear regression (possibly calibration changes or a nonlinear fit). Select a narrower interval.");
    }
    Ok(Py::dict(vec![
        ("status", Py::str("inferred_from_recorded_pairs")),
        ("equation", Py::str("y = slope * x + intercept")),
        ("raw_type", Py::str(raw)),
        ("slope", Py::Float(slope)),
        ("intercept", Py::Float(intercept)),
        ("pair_count", Py::Int(pairs.len() as i64)),
        ("max_residual", Py::Float(residual)),
        ("note", Py::str("Reconstructed mapping, not a saved original calibration.")),
    ]))
}

fn cmp_num(a: &Py, b: &Py) -> std::cmp::Ordering {
    a.number().unwrap_or(f64::NAN).partial_cmp(&b.number().unwrap_or(f64::NAN)).unwrap_or(std::cmp::Ordering::Equal)
}

fn preview_points(samples: &[Sample]) -> Py {
    let width = ((samples.len() as f64 / (600.0 / 5.0)).ceil() as usize).max(1);
    let mut points = Vec::new();
    for group in samples.chunks(width) {
        let mut indexes = vec![0, group.len() - 1];
        if let Some(missing) = group.iter().position(|s| s.value.is_none()) {
            indexes.push(missing);
        }
        let valid: Vec<(usize, &Py)> = group.iter().enumerate().filter_map(|(i, s)| s.value.as_ref().map(|v| (i, v))).collect();
        if let Some(first) = valid.first() {
            let mut low = *first;
            let mut high = *first;
            for &(i, v) in &valid[1..] {
                if cmp_num(v, low.1).is_lt() {
                    low = (i, v);
                }
                if cmp_num(v, high.1).is_gt() {
                    high = (i, v);
                }
            }
            indexes.push(low.0);
            indexes.push(high.0);
        }
        indexes.sort_unstable();
        indexes.dedup();
        for i in indexes {
            points.push(Py::List(vec![Py::Int(group[i].t), group[i].value.clone().unwrap_or(Py::None)]));
        }
    }
    Py::List(points)
}

fn data_kind(kind: &str) -> &'static str {
    if raw_type(kind).is_some() || kind == "LOADCELL_FILL_PERCENT" {
        "recorded_derived"
    } else if matches!(kind, "KG1000" | "KG50" | "OX_TANK_PRESSURE" | "IADC" | "COMBUSTION_CHAMBER_VOLTAGE") {
        "recorded_raw_input"
    } else if kind == "VALVE_STATE" {
        "recorded_state"
    } else {
        "recorded_unclassified"
    }
}

/// The station context the request carries (`config/report_context.json`).
pub(crate) struct Context {
    pub valve_mappings: Py,
    pub channel_metadata: Py,
    pub diagnostic_streams: Py,
}

fn recorded_diagnostic_info(stream: &Stream, context: &Context, versions: &HashMap<(i64, String), Py>) -> Option<Py> {
    if !stream.kind.starts_with("REC_") || stream.samples.is_empty() {
        return None;
    }
    let one = Py::Int(1);
    if !stream.samples.iter().all(|s| versions.get(&(s.row_id, s.recording.to_owned())).is_some_and(|v| v.py_eq(&one))) {
        return None;
    }
    let schema = &context.diagnostic_streams;
    if !schema.get("version").is_some_and(|v| v.py_eq(&one)) {
        return None;
    }
    let found = schema.get("streams")?.as_list()?.iter().find(|s| s.get("data_type").and_then(Py::as_str) == Some(stream.kind.as_str()))?;
    let channels = found.get("channels").and_then(Py::as_list).unwrap_or(&[]);
    let label = channels.get(stream.index)?.clone();
    let units = found.get("units").and_then(Py::as_list).unwrap_or(&[]);
    let unit = units.get(stream.index).cloned().unwrap_or_else(|| found.get("unit").cloned().unwrap_or(Py::None));
    Some(Py::dict(vec![
        ("channel_label", label),
        ("unit", unit),
        ("unit_source", Py::str("recorded_diagnostic_schema_v1")),
        ("precision", Py::str("counter CSV integers exact; graph projections approximate")),
    ]))
}

fn channel_info(stream: &Stream, context: &Context, versions: &HashMap<(i64, String), Py>, provenance: &Provenance) -> Py {
    let mut units: Vec<&Py> = Vec::new();
    for entry in context.channel_metadata.as_list().unwrap_or(&[]) {
        let matches = entry.get("data_type").and_then(Py::as_str) == Some(stream.kind.as_str())
            && entry.get("index").is_some_and(|i| i.py_eq(&Py::Int(stream.index as i64)))
            && entry.get("sender_id").is_none_or(|s| matches!(s, Py::None) || s.as_str() == Some(stream.sender.as_str()));
        if matches && let Some(unit) = entry.get("unit").filter(|u| u.truthy()) && !units.iter().any(|u| u.py_eq(unit)) {
            units.push(unit);
        }
    }
    // Units an SD import's manifest names for these samples win over the layout's.
    let mut recorded: Vec<Py> = Vec::new();
    for sample in &stream.samples {
        if let Some(origin) = origin(provenance, sample.row_id).filter(|o| is_sd(o))
            && let Some(unit) = origin.get("mapping").filter(|m| m.is_dict()).and_then(|m| m.get("units")).filter(|u| u.as_str().is_some())
            && !recorded.iter().any(|u| u.py_eq(unit))
        {
            recorded.push(unit.clone());
        }
    }
    let from_sd = !recorded.is_empty();
    let units: Vec<&Py> = if from_sd { recorded.iter().collect() } else { units };
    let single = units.len() == 1;
    let mut info = Py::dict(vec![
        ("sender", Py::str(&stream.sender)),
        ("data_type", Py::str(&stream.kind)),
        ("index", Py::Int(stream.index as i64)),
        ("unit", if single { units[0].clone() } else { Py::None }),
        ("unit_source", Py::str(if !single { "unknown" } else if from_sd { "recorded_sd_manifest" } else { "export_station_layout" })),
        ("value_kind", Py::str(data_kind(&stream.kind))),
    ]);
    if let Some(diagnostic) = recorded_diagnostic_info(stream, context, versions) {
        info.update(diagnostic);
    }
    info
}

fn csv_cell(text: &str) -> String {
    if text.contains([',', '"', '\r', '\n']) { format!("\"{}\"", text.replace('"', "\"\"")) } else { text.to_owned() }
}

fn safe(text: &str) -> String {
    if text.starts_with(['=', '+', '-', '@']) { format!("'{text}") } else { text.to_owned() }
}

fn cell(value: Option<&Py>) -> String {
    value.map(Py::py_str).unwrap_or_default()
}

fn export_csv(streams: &[&Stream], metadata: &[Py], provenance: &Provenance) -> Result<Vec<u8>, Failure> {
    let size: usize = streams.iter().zip(metadata).map(|(s, m)| m.dumps(false, false).chars().count() * s.samples.len()).sum();
    if size > 48_000_000 {
        return fail("Regression metadata makes this CSV too large. Narrow the selection or use Excel, which stores regressions separately.");
    }
    let mut out = String::from("recording,row_id,received_timestamp_ms,source_timestamp_ms,sender,data_type,channel,value,regression_json,channel_label,state_label\r\n");
    for (stream, meta) in streams.iter().zip(metadata) {
        let json = meta.dumps(true, false);
        let label = meta.get("channel_label").map(Py::py_str).unwrap_or_else(|| channel_label(stream));
        for s in &stream.samples {
            // A resolved row: the time it was received (none for SD rows) and its provenance.
            let origin = origin(provenance, s.row_id);
            let received = match origin.map(|o| o.get("received_timestamp_ms")) {
                None => s.t.to_string(),
                Some(None | Some(Py::None)) => String::new(),
                Some(Some(value)) => csv_cell(&value.py_str()),
            };
            let with_origin;
            let json = match origin {
                Some(origin) => {
                    let mut meta = meta.clone();
                    meta.set("sample_provenance", without_fields(origin));
                    with_origin = meta.dumps(true, false);
                    &with_origin
                }
                None => &json,
            };
            let cells = [
                csv_cell(s.recording),
                s.row_id.to_string(),
                received,
                s.source.map(|v| v.to_string()).unwrap_or_default(),
                csv_cell(&safe(&stream.sender)),
                csv_cell(&safe(&stream.kind)),
                stream.index.to_string(),
                cell(s.value.as_ref()),
                csv_cell(json),
                csv_cell(&safe(&label)),
                csv_cell(&safe(&state_label(stream, s.value.as_ref(), meta.get("boolean_labels")))),
            ];
            out.push_str(&cells.join(","));
            out.push_str("\r\n");
        }
    }
    Ok(out.into_bytes())
}

/// The DAQ firmware's nine-significant-digit binary32 text, without
/// quantizing a value that is not exactly binary32.
fn daq_float(value: &Py) -> String {
    let x = match value {
        Py::Float(x) => *x,
        other => return other.py_str(),
    };
    let single = x as f32;
    if single.is_infinite() || f64::from(single) != x {
        return repr_float(x);
    }
    let sign = if x.is_sign_negative() { "-" } else { "" };
    if x == 0.0 {
        return format!("{sign}0");
    }
    let (mut normalized, mut exponent) = (x.abs(), 0i32);
    while normalized >= 10.0 {
        normalized /= 10.0;
        exponent += 1;
    }
    while normalized < 1.0 {
        normalized *= 10.0;
        exponent -= 1;
    }
    let mut digits = (normalized * 100_000_000.0 + 0.5) as i64;
    if digits == 1_000_000_000 {
        digits = 100_000_000;
        exponent += 1;
    }
    format!("{sign}{}.{:08}e{}{:02}", digits / 100_000_000, digits % 100_000_000, if exponent < 0 { '-' } else { '+' }, exponent.abs())
}

fn daq_cell(text: &str, delimiter: char, force: bool) -> String {
    if force || text.contains([delimiter, '"', '\r', '\n']) { format!("\"{}\"", text.replace('"', "\"\"")) } else { text.to_owned() }
}

/// `daq_record`: one SD record's thirteen cells, quoted as the firmware writes them.
fn daq_record(fields: &Py, delimiter: char) -> String {
    let raw_adc = present(fields.get("raw_adc_code"));
    let cells: Vec<String> = DAQ_COLUMNS
        .iter()
        .map(|name| {
            let value = fields.get(name);
            let text = match value {
                None | Some(Py::None) => String::new(),
                Some(v) => v.py_str(),
            };
            let force = (*name == "value" && !raw_adc) || (matches!(*name, "raw_value" | "calibrated_value") && !raw_adc && present(value));
            daq_cell(&text, delimiter, force)
        })
        .collect();
    let mut out = cells.join(&delimiter.to_string());
    out.push_str("\r\n");
    out
}

/// `export_daq_sd_rows` for one resolved SD sample, without the header line.
fn daq_sd_row(origin: &Py, delimiter: char) -> Result<String, Failure> {
    let fields = origin.get("fields");
    let complete = match fields {
        Some(Py::Dict(items)) => {
            let keys: HashSet<&str> = items.iter().map(|(k, _)| k.as_str()).collect();
            keys == DAQ_COLUMNS.iter().copied().collect::<HashSet<&str>>()
        }
        _ => false,
    };
    if !complete {
        return fail("Imported SD sample does not contain the actual thirteen-column schema.");
    }
    Ok(format!("# sd_sample={}\r\n{}", without_fields(origin).dumps(true, false), daq_record(fields.unwrap_or(&Py::None), delimiter)))
}

fn export_daq(streams: &[&Stream], metadata: &[Py], context: &Context, versions: &HashMap<(i64, String), Py>, delimiter: char, provenance: &Provenance, all: &[Stream]) -> Result<Vec<u8>, Failure> {
    let d = delimiter.to_string();
    let mut out = DAQ_COLUMNS.join(&d);
    out.push_str("\r\n");
    let export = Py::dict(vec![
        ("layout", Py::str("seds-sd-csv-13-v1")),
        ("origin", Py::str("groundstation_recording")),
        ("clock_policy", Py::str("source UTC only when calendar-valid; otherwise unknown")),
        (
            "unavailable_fields",
            Py::List(
                ["monotonic_ms", "raw_adc_code", "raw_value", "calibrated_value", "adc_temperature_c", "adc_temperature_code", "uptime_ticks", "uptime_frequency_hz", "record_sequence"]
                    .into_iter()
                    .map(Py::str)
                    .collect(),
            ),
        ),
        ("precision", Py::str("firmware nine-digit binary32; higher recorded precision retained")),
        ("missing_values", Py::str("omitted; counts retained in channel metadata")),
    ]);
    out.push_str(&format!("# export={}\r\n", export.dumps(true, false)));
    let mut selected: Vec<(i64, &str, i64, &str, usize, &Py, Option<i64>)> = Vec::new();
    for (n, (stream, meta)) in streams.iter().zip(metadata).enumerate() {
        let mut info = channel_info(stream, context, versions, provenance);
        info.set("key", Py::str(&stream.key));
        info.set("samples", Py::Int(stream.samples.len() as i64));
        info.set("missing_samples", Py::Int(stream.samples.iter().filter(|s| s.value.is_none()).count() as i64));
        info.set("regression", meta.clone());
        out.push_str(&format!("# channel={}\r\n", info.dumps(true, false)));
        for s in &stream.samples {
            if let Some(value) = &s.value {
                selected.push((s.t, s.recording, s.row_id, stream.key.as_str(), n, value, s.source));
            }
        }
    }
    selected.sort_by(|a, b| (a.0, a.1, a.2, a.3).cmp(&(b.0, b.1, b.2, b.3)));
    // A physical SD record is written whole: every channel mapped from it must be selected.
    let mut physical: HashMap<String, HashSet<&str>> = HashMap::new();
    if provenance.as_ref().is_some_and(|p| !p.is_empty()) {
        for candidate in all {
            for s in &candidate.samples {
                if let Some(origin) = origin(provenance, s.row_id).filter(|o| is_sd(o) && !derived_sd(o)) {
                    let identity = origin.get("sd_sample_id").unwrap_or(&Py::None).dumps(true, false);
                    physical.entry(identity).or_default().insert(candidate.key.as_str());
                }
            }
        }
    }
    let chosen: HashSet<&str> = streams.iter().map(|s| s.key.as_str()).collect();
    let mut seen_sd: HashSet<String> = HashSet::new();
    const UTC: std::ops::Range<i64> = 315_532_800_000..4_354_819_200_000;
    for (received, recording, row_id, key, n, value, source) in selected {
        let stream = streams[n];
        if let Some(origin) = origin(provenance, row_id).filter(|o| is_sd(o) && !derived_sd(o)) {
            let sample_id = origin.get("sd_sample_id").filter(|id| !matches!(id, Py::None));
            let (Some(sample_id), true) = (sample_id, origin.get("fields").is_some_and(Py::is_dict)) else {
                return fail("Resolved SD recording lacks its original thirteen fields; no substitute was produced.");
            };
            let identity = sample_id.dumps(true, false);
            if !physical.get(&identity).is_none_or(|required| required.is_subset(&chosen)) {
                return fail("DAQ SD layout preserves whole physical SD records. Select all channels mapped from the chosen SD record, or use selected-channel CSV.");
            }
            if seen_sd.insert(identity) {
                out.push_str(&daq_sd_row(origin, delimiter)?);
            }
            continue;
        }
        let utc = source.filter(|s| UTC.contains(s));
        let sensor = daq_sd_sensor(&stream.sender, &stream.kind, stream.index).map_or_else(|| key.to_owned(), str::to_owned);
        let provenance = Py::dict(vec![
            ("recording", Py::str(recording)),
            ("row_id", Py::Int(row_id)),
            ("channel", Py::str(key)),
            ("received_timestamp_ms", Py::Int(received)),
            ("source_timestamp_ms", source.map_or(Py::None, Py::Int)),
            ("source_age_ms", utc.filter(|_| UTC.contains(&received)).map_or(Py::None, |s| Py::Int(received - s))),
            ("state_label", Py::str(state_label(stream, Some(value), metadata[n].get("boolean_labels")))),
        ]);
        out.push_str(&format!("# recorded_sample={}\r\n", provenance.dumps(true, false)));
        let fields: [(&str, String); 4] = [
            ("timestamp_ms", utc.map(|u| u.to_string()).unwrap_or_default()),
            ("sensor", sensor),
            ("value", daq_float(value)),
            ("time_source", if utc.is_some() { "network" } else { "unknown" }.to_owned()),
        ];
        let record: Vec<String> = DAQ_COLUMNS
            .iter()
            .map(|name| {
                let text = fields.iter().find(|(f, _)| f == name).map(|(_, v)| v.as_str()).unwrap_or("");
                daq_cell(text, delimiter, *name == "value")
            })
            .collect();
        out.push_str(&record.join(&d));
        out.push_str("\r\n");
    }
    Ok(out.into_bytes())
}

/// One report of the recording's `rows` (in recording order). `request` is
/// the report studio's selection; `format` one of preview/csv/daq_csv/daq_tsv.
pub(crate) fn run(recording: &str, rows: &[Row], history_rows: &[HistoryRow], context: &Context, request: &serde_json::Value, format: &str) -> Result<Vec<u8>, Failure> {
    let (start, end) = (request.get("start_ms").and_then(serde_json::Value::as_f64), request.get("end_ms").and_then(serde_json::Value::as_f64));
    if let (Some(start), Some(end)) = (start, end)
        && end <= start
    {
        return fail("End time must be after start time.");
    }
    let history = history(recording, history_rows)?;
    let mut rows: Vec<&Row> = rows.iter().filter(|r| start.is_none_or(|s| r.received as f64 >= s) && end.is_none_or(|e| (r.received as f64) < e)).collect();
    if rows.len() > MAX_ROWS {
        return fail("Selection exceeds 200,000 recorded rows. Choose a recording or narrower time window; no rows were silently dropped.");
    }
    if rows.is_empty() {
        return fail("No recorded samples in this selection.");
    }
    rows.sort_by_key(|r| (r.received, r.id));
    // An SD-resolved recording: every row names its source (read_rows).
    let provenance: Provenance = if rows.iter().any(|r| r.provenance.is_some()) {
        let mut map = HashMap::with_capacity(rows.len());
        for row in &rows {
            let info = Py::parse(row.provenance.as_deref().unwrap_or("null")).map_err(|e| (422, e))?;
            if !info.is_dict() || !matches!(info.get("origin").and_then(Py::as_str), Some("sd" | "network")) {
                return fail("Resolved recording has invalid source provenance; no export was produced.");
            }
            map.insert(row.id, info);
        }
        Some(map)
    } else {
        None
    };
    let mut versions: HashMap<(i64, String), Py> = HashMap::new();
    for row in &rows {
        if row.kind.starts_with("REC_")
            && let Ok(decoded) = Py::parse(if row.payload.is_empty() { "null" } else { &row.payload })
            && decoded.is_dict()
        {
            let context = decoded.get("context").unwrap_or(&decoded);
            if context.is_dict() {
                versions.insert((row.id, recording.to_owned()), context.get("version").cloned().unwrap_or(Py::None));
            }
        }
    }
    // channels(rows)
    let mut streams: Vec<Stream> = Vec::new();
    let mut positions: HashMap<String, usize> = HashMap::new();
    let mut count = 0usize;
    for row in &rows {
        let values = Py::parse(if row.values.is_empty() { "[]" } else { &row.values }).map_err(|e| (422, e))?;
        let Py::List(values) = values else { return fail("A recorded row's values are not a list.") };
        for (index, value) in values.into_iter().enumerate() {
            count += 1;
            if count > MAX_ROWS {
                return fail("Selection exceeds 200,000 channel samples. Choose a narrower time window; no data was silently dropped.");
            }
            let value = match value {
                Py::Bool(_) | Py::Int(_) => Some(value),
                Py::Float(f) if f.is_finite() => Some(value),
                _ => None,
            };
            let key = key_of(&row.sender, &row.kind, index);
            let at = *positions.entry(key.clone()).or_insert_with(|| {
                streams.push(Stream { key, sender: row.sender.clone(), kind: row.kind.clone(), index, samples: Vec::new() });
                streams.len() - 1
            });
            streams[at].samples.push(Sample { t: row.received, value, source: row.source, recording, row_id: row.id });
            if streams.len() > MAX_SERIES {
                return fail("Too many channels for one report. Select a smaller recording.");
            }
        }
    }
    // selected_streams(request, streams)
    let chosen: Vec<&Stream> = match request.get("channels") {
        None | Some(serde_json::Value::Null) => streams.iter().collect(),
        Some(serde_json::Value::Array(keys)) if !keys.is_empty() => {
            let mut chosen: Vec<&Stream> = Vec::new();
            for key in keys {
                let Some(&at) = key.as_str().and_then(|k| positions.get(k)) else {
                    return fail("A selected channel has no data in this interval. Refresh the preview or change the selection.");
                };
                if !chosen.iter().any(|s| s.key == streams[at].key) {
                    chosen.push(&streams[at]);
                }
            }
            chosen
        }
        Some(_) => return fail("Select at least one channel, or choose all data."),
    };
    let has_history = !history.is_empty();
    let mut metadata = Vec::new();
    for stream in &chosen {
        let mut info = regression(stream, &streams, &history, has_history)?;
        if let Some(diagnostic) = recorded_diagnostic_info(stream, context, &versions) {
            info.update(diagnostic);
        }
        if stream.kind == "VALVE_STATE" {
            let mapping = context
                .valve_mappings
                .as_list()
                .unwrap_or(&[])
                .iter()
                .find(|m| m.is_dict() && m.get("index").is_some_and(|i| i.py_eq(&Py::Int(stream.index as i64))))
                .cloned()
                .unwrap_or(Py::Dict(Vec::new()));
            info.set("channel_label", mapping.get("label").filter(|l| l.truthy()).cloned().unwrap_or_else(|| Py::str(channel_label(stream))));
            let labels = ["true_label", "false_label", "unknown_label"].into_iter().filter_map(|name| mapping.get(name).filter(|v| v.as_str().is_some()).map(|v| (name, v.clone()))).collect();
            info.set("boolean_labels", Py::dict(labels));
        }
        metadata.push(info);
    }
    match format {
        "preview" => {
            let channels = chosen
                .iter()
                .zip(&metadata)
                .map(|(stream, meta)| {
                    let mut channel = Py::dict(vec![
                        ("key", Py::str(&stream.key)),
                        ("label", meta.get("channel_label").cloned().unwrap_or_else(|| Py::str(channel_label(stream)))),
                        ("count", Py::Int(stream.samples.len() as i64)),
                        ("points", preview_points(&stream.samples)),
                        ("regression", meta.clone()),
                        ("finite_count", Py::Int(stream.samples.iter().filter(|s| s.value.is_some()).count() as i64)),
                    ]);
                    channel.update(channel_info(stream, context, &versions, &provenance));
                    channel
                })
                .collect();
            let preview = Py::dict(vec![
                ("start_ms", Py::Int(rows[0].received)),
                ("end_ms", Py::Int(rows[rows.len() - 1].received + 1)),
                ("recorded_rows", Py::Int(rows.len() as i64)),
                ("channels", Py::List(channels)),
            ]);
            Ok(preview.dumps(false, false).into_bytes())
        }
        "csv" => export_csv(&chosen, &metadata, &provenance),
        "daq_csv" | "daq_tsv" => {
            if !request.get("recording").and_then(serde_json::Value::as_str).is_some_and(|r| !r.is_empty()) {
                return fail("Select one recording before choosing DAQ SD layout.");
            }
            export_daq(&chosen, &metadata, context, &versions, if format == "daq_tsv" { '\t' } else { ',' }, &provenance, &streams)
        }
        _ => fail("Choose CSV, DAQ SD CSV/TSV, Excel or PDF."),
    }
}

/// The context from `config/report_context.json` (empty when absent).
pub(crate) fn context(document: Option<&serde_json::Value>) -> Context {
    let get = |key: &str, empty: Py| document.and_then(|d| d.get(key)).map(Py::from_json).unwrap_or(empty);
    Context {
        valve_mappings: get("valve_mappings", Py::List(Vec::new())),
        channel_metadata: get("channel_metadata", Py::List(Vec::new())),
        diagnostic_streams: get("recording_diagnostic_streams", Py::Dict(Vec::new())),
    }
}
