//! The station's export routes answered from a replay bundle, with no
//! station (`docs/replay-bundle.md`, "Exports offline"). Pure functions: no
//! I/O, no UI.
//!
//! | Route | Offline |
//! | --- | --- |
//! | `GET /api/recordings/{id}/csv` | `data/telemetry.csv`, unchanged |
//! | `GET /api/recordings/csv?start_ms&end_ms` | the rows of `data/telemetry.csv` in the range, behind a `recording` column (this recording only) |
//! | `GET /api/recordings/report?request=` | `preview`, `csv`, `daq_csv`, `daq_tsv` computed here ([`report`]); `xlsx`/`pdf` need the station |
//! | `GET /api/flight_review/recording/{id}/motor_export`, `.../flight_export` | `exports/*.zip` from the bundle, for the default window only |
//!
//! Each answer is byte for byte the station's for the same request
//! (GroundStation26 `contract_fixtures` writes the station's answers to
//! `replay-bundle/exports/`; `local_station/tests.rs` compares).
mod py;
mod report;

use serde_json::Value;
use std::collections::BTreeMap;
use std::rc::Rc;

/// A file the station would send for an export request.
#[derive(Debug, PartialEq)]
pub(crate) struct ExportFile {
    pub bytes: Vec<u8>,
    pub content_type: &'static str,
    /// The `Content-Disposition` file name; empty for an inline answer (the report preview).
    pub filename: String,
}

/// One `calibration_history` row of the recording (`derived/calibration_history.json`).
#[derive(Clone, Debug)]
pub(crate) struct HistoryRow {
    pub timestamp_ms: i64,
    pub sender_id: String,
    pub sensor_id: String,
    /// The stored JSON text, parsed when a report needs it.
    pub config_json: String,
}

/// A motor or flight review export the bundle holds (`exports/index.json`).
#[derive(Clone, Debug)]
pub(crate) struct BundledZip {
    pub filename: String,
    pub from_ms: Option<i64>,
    pub to_ms: Option<i64>,
    pub bytes: Vec<u8>,
}

/// What a bundle supplies for its recording's exports.
pub(crate) struct ExportInputs {
    /// The recording id (`manifest.json` `source.recording`).
    pub recording: String,
    /// `data/telemetry.csv`: the station's `GET /api/recordings/{id}/csv`.
    pub csv: Rc<Vec<u8>>,
    /// `derived/calibration_history.json` (empty when the recording has none).
    pub calibration_history: Vec<HistoryRow>,
    /// `config/report_context.json`, the report studio's export-time context.
    pub report_context: Option<Value>,
    /// `exports/index.json` was present: an export it does not list does not
    /// apply to the recording (rather than being unknown to an older bundle).
    pub export_index: bool,
    /// By kind (`motor_export`, `flight_export`).
    pub zips: BTreeMap<String, BundledZip>,
}

impl ExportInputs {
    /// From a bundle's verified entries and its `data/telemetry.csv`.
    pub(crate) fn from_bundle(recording: &str, csv: Rc<Vec<u8>>, files: &BTreeMap<String, Vec<u8>>) -> Self {
        let json = |path: &str| files.get(path).and_then(|b| serde_json::from_slice::<Value>(b).ok());
        let calibration_history = json("derived/calibration_history.json")
            .and_then(|v| v.as_array().cloned())
            .unwrap_or_default()
            .iter()
            .filter_map(|row| {
                Some(HistoryRow {
                    timestamp_ms: row["timestamp_ms"].as_i64()?,
                    sender_id: row["sender_id"].as_str().unwrap_or_default().to_owned(),
                    sensor_id: row["sensor_id"].as_str().unwrap_or_default().to_owned(),
                    config_json: row["config_json"].as_str().unwrap_or("null").to_owned(),
                })
            })
            .collect();
        let index = json("exports/index.json");
        let mut zips = BTreeMap::new();
        for (kind, entry) in index.as_ref().and_then(Value::as_object).into_iter().flatten() {
            if let Some(bytes) = entry["path"].as_str().and_then(|p| files.get(p)) {
                zips.insert(
                    kind.clone(),
                    BundledZip {
                        filename: entry["filename"].as_str().map_or_else(|| format!("{kind}.zip"), str::to_owned),
                        from_ms: entry["from_ms"].as_i64(),
                        to_ms: entry["to_ms"].as_i64(),
                        bytes: bytes.clone(),
                    },
                );
            }
        }
        Self { recording: recording.to_owned(), csv, calibration_history, report_context: json("config/report_context.json"), export_index: index.is_some(), zips }
    }
}

type Answer = Result<ExportFile, (u16, String)>;

const NEED_STATION: u16 = 501;

/// The answer to an export request, `None` when `path_and_query` is not an
/// export route. `path_and_query` is the request as the UI makes it, for
/// example `/api/recordings/csv?start_ms=1&end_ms=2`.
pub(crate) fn answer(inputs: &ExportInputs, path_and_query: &str) -> Option<Answer> {
    let (path, query) = path_and_query.split_once('?').unwrap_or((path_and_query, ""));
    let segments: Vec<String> = path.trim_start_matches('/').split('/').map(|s| decode(s, false)).collect();
    let segments: Vec<&str> = segments.iter().map(String::as_str).collect();
    let query = parse_query(query);
    match segments[..] {
        ["api", "recordings", "csv"] => Some(range_csv(inputs, &query)),
        ["api", "recordings", "report"] => Some(report_answer(inputs, &query)),
        ["api", "recordings", id, "csv"] => Some(recording_csv(inputs, id)),
        ["api", "flight_review", "recording", id, kind @ ("motor_export" | "flight_export")] => Some(review_export(inputs, id, kind, &query)),
        _ => None,
    }
}

/// Percent-decoding (`+` is a space in a query, as the station's form decoder reads it).
fn decode(text: &str, plus_is_space: bool) -> String {
    let bytes = text.as_bytes();
    let hex = |b: u8| (b as char).to_digit(16);
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'%' if i + 2 < bytes.len() => match (hex(bytes[i + 1]), hex(bytes[i + 2])) {
                (Some(h), Some(l)) => {
                    out.push((h * 16 + l) as u8);
                    i += 3;
                    continue;
                }
                _ => out.push(b'%'),
            },
            b'+' if plus_is_space => out.push(b' '),
            b => out.push(b),
        }
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn parse_query(query: &str) -> BTreeMap<String, String> {
    let mut out = BTreeMap::new();
    for pair in query.split('&').filter(|p| !p.is_empty()) {
        let (k, v) = pair.split_once('=').unwrap_or((pair, ""));
        out.entry(decode(k, true)).or_insert_with(|| decode(v, true));
    }
    out
}

fn not_found() -> (u16, String) {
    (404, "Recording not found or invalid recording name. Refresh the recording list.".into())
}

fn recording_csv(inputs: &ExportInputs, id: &str) -> Answer {
    if id != inputs.recording {
        return Err(not_found());
    }
    Ok(ExportFile { bytes: inputs.csv.to_vec(), content_type: "text/csv; charset=utf-8", filename: format!("{}.csv", id.trim_end_matches(".db")) })
}

/// One record of the recording CSV: each cell's raw text (quotes kept).
fn records(csv: &[u8]) -> Vec<Vec<&[u8]>> {
    let mut out = Vec::new();
    let mut record = Vec::new();
    let (mut start, mut quoted, mut i) = (0, false, 0);
    while i < csv.len() {
        match csv[i] {
            b'"' => quoted = !quoted,
            b',' if !quoted => {
                record.push(&csv[start..i]);
                start = i + 1;
            }
            b'\r' if !quoted && csv.get(i + 1) == Some(&b'\n') => {
                record.push(&csv[start..i]);
                out.push(std::mem::take(&mut record));
                i += 2;
                start = i;
                continue;
            }
            _ => {}
        }
        i += 1;
    }
    if start < csv.len() {
        record.push(&csv[start..]);
        out.push(record);
    }
    out
}

/// The text of a quoted CSV cell.
fn unquote(cell: &[u8]) -> String {
    let text = String::from_utf8_lossy(cell);
    match text.strip_prefix('"').and_then(|t| t.strip_suffix('"')) {
        Some(inner) => inner.replace("\"\"", "\""),
        None => text.into_owned(),
    }
}

/// The original value of a station text cell (`text_cell`): unquoted, with
/// the spreadsheet-formula guard (a leading `'`) removed.
fn untext(cell: &[u8]) -> String {
    let text = unquote(cell);
    match text.strip_prefix('\'') {
        Some(rest) if rest.trim_start().starts_with(['=', '+', '-', '@']) => rest.to_owned(),
        _ => text,
    }
}

/// A row of the recording CSV.
pub(crate) struct Row {
    pub id: i64,
    pub received: i64,
    pub source: Option<i64>,
    pub sender: String,
    pub kind: String,
    /// `values_json` (over-range values as "OVR"/"-OVR").
    pub values: String,
    pub payload: String,
}

const HEADER: &[u8] = b"id,received_timestamp_ms,source_timestamp_ms,sender_id,data_type,values_json,payload_json";

/// The rows of a receipt-time recording CSV with each record's raw cells, or
/// the reason it cannot be used (an SD-resolved recording's CSV is on the
/// resolved timeline, not the receipt rows these exports read).
fn rows(csv: &[u8]) -> Result<Vec<(Row, Vec<&[u8]>)>, (u16, String)> {
    let mut records = records(csv).into_iter();
    let header = records.next().unwrap_or_default().join(&b","[..]);
    if !header.starts_with(HEADER) || header.windows(16).any(|w| w == b"canonical_utc_ms") {
        return Err((NEED_STATION, "This recording is SD-resolved; its time-range CSV and reports need the station.".into()));
    }
    let int = |cell: &[u8]| std::str::from_utf8(cell).ok().and_then(|t| t.parse::<i64>().ok());
    Ok(records
        .filter(|cells| cells.len() >= 7)
        .filter_map(|cells| {
            let row = Row {
                id: int(cells[0])?,
                received: int(cells[1])?,
                source: int(cells[2]),
                sender: untext(cells[3]),
                kind: untext(cells[4]),
                values: unquote(cells[5]),
                payload: unquote(cells[6]),
            };
            Some((row, cells))
        })
        .collect())
}

/// `"text"` with quotes doubled and formulas guarded, as the station writes text cells.
fn text_cell(value: &str) -> String {
    let formula = value.trim_start().starts_with(['=', '+', '-', '@']);
    format!("\"{}{}\"", if formula { "'" } else { "" }, value.replace('"', "\"\""))
}

fn range_csv(inputs: &ExportInputs, query: &BTreeMap<String, String>) -> Answer {
    let number = |name: &str| query.get(name).and_then(|v| v.parse::<i64>().ok());
    let (Some(start), Some(end)) = (number("start_ms"), number("end_ms")) else {
        return Err((400, "Failed to deserialize query string: start_ms and end_ms must be whole milliseconds.".into()));
    };
    if start < 0 || end <= start {
        return Err((400, "Choose an end time later than the start time.".into()));
    }
    let rows = rows(&inputs.csv)?;
    let recording = text_cell(&inputs.recording);
    let mut out = b"recording,id,received_timestamp_ms,source_timestamp_ms,sender_id,data_type,values_json,payload_json\r\n".to_vec();
    let mut any = false;
    for (_, cells) in rows.iter().filter(|(r, _)| (start..end).contains(&r.received)) {
        any = true;
        out.extend_from_slice(recording.as_bytes());
        for cell in &cells[..7] {
            out.push(b',');
            out.extend_from_slice(cell);
        }
        out.extend_from_slice(b"\r\n");
    }
    if !any {
        return Err((404, "No recorded telemetry matches this time range. Check the GroundStation clock or download an individual session.".into()));
    }
    Ok(ExportFile { bytes: out, content_type: "text/csv; charset=utf-8", filename: format!("telemetry-{start}-{end}.csv") })
}

fn report_answer(inputs: &ExportInputs, query: &BTreeMap<String, String>) -> Answer {
    let Some(text) = query.get("request") else {
        return Err((400, "Failed to deserialize query string: missing field `request`".into()));
    };
    if text.len() > 24_000 {
        return Err((400, "Report selection is too large.".into()));
    }
    let request: Value = match serde_json::from_str(text) {
        Ok(value @ Value::Object(_)) => value,
        _ => return Err((400, "Invalid report selection.".into())),
    };
    let format = request["format"].as_str().unwrap_or("preview");
    let (content_type, filename) = match format {
        "preview" => ("application/json", String::new()),
        "csv" => ("text/csv; charset=utf-8", "telemetry-report.csv".to_owned()),
        "daq_csv" => ("text/csv; charset=utf-8", "telemetry-daq-sd.csv".to_owned()),
        "daq_tsv" => ("text/tab-separated-values; charset=utf-8", "telemetry-daq-sd.tsv".to_owned()),
        "xlsx" | "pdf" => return Err((NEED_STATION, "Excel and PDF reports need the station. Offline, choose CSV or a DAQ SD layout.".into())),
        _ => return Err((400, "Choose CSV, DAQ SD CSV/TSV, Excel or PDF.".into())),
    };
    let selected = request["recording"].as_str().unwrap_or("");
    if !selected.is_empty() && selected != inputs.recording {
        return Err((404, "Recording not found. Refresh the recording list.".into()));
    }
    let rows: Vec<Row> = rows(&inputs.csv)?.into_iter().map(|(row, _)| row).collect();
    let context = report::context(inputs.report_context.as_ref());
    let bytes = report::run(&inputs.recording, &rows, &inputs.calibration_history, &context, &request, format)?;
    Ok(ExportFile { bytes, content_type, filename })
}

/// The UI's whole-second rendering of a window (`flight_review_view::ExportControls`).
fn form_window(from: i64, to: i64) -> (i64, i64) {
    (from.div_euclid(1000) * 1000, (to + 999).div_euclid(1000) * 1000)
}

fn review_export(inputs: &ExportInputs, id: &str, kind: &str, query: &BTreeMap<String, String>) -> Answer {
    if id != inputs.recording {
        return Err((404, "Recording not found. Refresh the recording list.".into()));
    }
    let Some(zip) = inputs.zips.get(kind) else {
        return Err(match (inputs.export_index, kind) {
            (true, "motor_export") => (404, "No motor data (load cells, chamber or tank PT) in this recording or range.".into()),
            (true, _) => (404, "No flight data (FC phases, altitude or velocity) in this recording or range.".into()),
            (false, _) => (NEED_STATION, "This bundle holds no review exports; export from the station.".into()),
        });
    };
    let number = |name: &str| query.get(name).and_then(|v| v.parse::<i64>().ok());
    let window = (number("from_ms"), number("to_ms"));
    let default = match (zip.from_ms, zip.to_ms) {
        (Some(from), Some(to)) => window == (Some(from), Some(to)) || window == {
            let (a, b) = form_window(from, to);
            (Some(a), Some(b))
        },
        _ => false,
    };
    if window != (None, None) && !default {
        return Err((NEED_STATION, "Offline, the bundle holds the export of the default window only; a custom window needs the station.".into()));
    }
    Ok(ExportFile { bytes: zip.bytes.clone(), content_type: "application/zip", filename: zip.filename.clone() })
}
