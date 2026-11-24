//! The motor and flight review of a custom window, offline: the station's
//! `GET /api/flight_review/recording/{id}?from_ms&to_ms` and its
//! `.../motor_export` / `.../flight_export` (as the station sends them with
//! `raster=false`), computed here from the bundle's
//! `derived/review_inputs.json` (and `models/hybrid_motor/*.json` for the
//! burn report's predicted curve).
//!
//! The review is the station's, ported from GroundStation26
//! `backend/src/flight_review*` and `burn_report*`:
//!
//! | Here | GroundStation26 `backend/src/` | |
//! | --- | --- | --- |
//! | `zip.rs`, `plot.rs` | `flight_review/zip.rs`, `flight_review/plot.rs` | copies |
//! | `thrust.rs`, `merge.rs` | `burn_report/thrust.rs`, `burn_report/merge.rs` | copies |
//! | `hybrid.rs` | `apogee_planner/hybrid.rs` up to "committed model files" | copy |
//! | `burn.rs` | `burn_report.rs`: types, inputs, `compute`, `predict` | port (model from the bundle) |
//! | `data.rs`, `motor.rs`, `flight.rs`, `export.rs` | `flight_review/{data,motor,flight,export}.rs` | ports |
//! | this file | `flight_review.rs`: `load` (custom range), the export windows | port |
//!
//! `scripts/check_review_port.py` checks the copies against a
//! GroundStation26 checkout; the golden tests (`local_station/tests.rs`,
//! GroundStation26 `contract_fixtures` `replay-bundle/review/`) check that
//! every answer equals the station's byte for byte.
#[allow(dead_code)]
mod burn;
#[allow(dead_code)]
mod data;
#[allow(dead_code)]
mod export;
#[allow(dead_code)]
mod flight;
#[allow(dead_code, clippy::all)]
mod hybrid;
#[allow(dead_code)]
mod merge;
#[allow(dead_code)]
mod motor;
#[allow(dead_code)]
mod plot;
#[allow(dead_code)]
mod thrust;
#[allow(dead_code)]
mod zip;

use data::{Data, Row};
use merge::{Channel, Sample, Source};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::cell::OnceCell;
use std::collections::BTreeMap;
use std::rc::Rc;

/// GroundStation26 `recording_replay::sources::THRUST_TYPE`.
pub(crate) const THRUST_TYPE: &str = "BURN_REPORT_THRUST";

/// GroundStation26 `recording_replay::sources::sd_sender`.
pub(crate) fn sd_sender(board: &str) -> String {
    format!("{board}_SD")
}

pub(crate) const INPUTS_PATH: &str = "derived/review_inputs.json";
const MODEL_DIR: &str = "models/hybrid_motor";
const NEED_STATION: u16 = 501;

/// GroundStation26 `flight_review::RecordingReview` (the same fields, in the same order).
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct RecordingReview {
    pub id: String,
    pub source_kind: String,
    pub label: String,
    pub mode: Option<String>,
    pub provenance: Option<String>,
    pub integrated_ground_test: bool,
    pub simulated: bool,
    pub from_ms: Option<i64>,
    pub to_ms: Option<i64>,
    pub data_from_ms: Option<i64>,
    pub data_to_ms: Option<i64>,
    pub motor: Option<motor::MotorView>,
    pub flight: Option<flight::FlightSummary>,
}

/// What a bundle carries for its recording's review: the raw entries, parsed
/// on the first custom-window request.
pub(crate) struct Bundle {
    inputs: Vec<u8>,
    model: Option<[Vec<u8>; 3]>,
    parsed: OnceCell<Result<Rc<Inputs>, String>>,
}

impl Bundle {
    /// From a bundle's verified entries; `None` when it has no review inputs.
    pub(crate) fn from_files(files: &BTreeMap<String, Vec<u8>>) -> Option<Self> {
        let model = |name: &str| files.get(&format!("{MODEL_DIR}/{name}")).cloned();
        Some(Self {
            inputs: files.get(INPUTS_PATH)?.clone(),
            model: match (model("hybrid_motor.json"), model("sweet_v6_cea.json"), model("sweet_v6_grain.json")) {
                (Some(spec), Some(cea), Some(grain)) => Some([spec, cea, grain]),
                _ => None,
            },
            parsed: OnceCell::new(),
        })
    }

    fn inputs(&self) -> Result<Rc<Inputs>, (u16, String)> {
        self.parsed
            .get_or_init(|| Inputs::parse(&self.inputs, self.model.as_ref()).map(Rc::new))
            .clone()
            .map_err(|e| (NEED_STATION, format!("The bundle's review inputs cannot be read ({e}); a custom window needs the station.")))
    }
}

/// `derived/review_inputs.json`, parsed.
pub(crate) struct Inputs {
    id: String,
    source_kind: String,
    label: Option<String>,
    mode: Option<String>,
    provenance: Option<String>,
    simulated: bool,
    planner: Option<motor::Predicted>,
    station_software: String,
    data: Data,
    extra_daq: Vec<(Channel, Sample)>,
    model: Option<Rc<burn::Model>>,
}

/// A value as the inputs carry it (over range: "OVR" / "-OVR").
fn value(v: &Value) -> Option<f64> {
    match v {
        Value::String(s) if s == "OVR" => Some(f64::INFINITY),
        Value::String(s) if s == "-OVR" => Some(f64::NEG_INFINITY),
        other => other.as_f64(),
    }
}

impl Inputs {
    fn parse(bytes: &[u8], model: Option<&[Vec<u8>; 3]>) -> Result<Self, String> {
        let v: Value = serde_json::from_slice(bytes).map_err(|e| e.to_string())?;
        if v["format"] != "seds-gs-review-inputs" || v["version"].as_u64() != Some(1) {
            return Err("unknown format".into());
        }
        let text = |key: &str| v[key].as_str().map(str::to_owned);
        let names = |key: &str| -> Vec<String> { v[key].as_array().into_iter().flatten().map(|n| n.as_str().unwrap_or_default().to_owned()).collect() };
        let (types, senders) = (names("types"), names("senders"));
        let mut data = Data::default();
        for row in v["rows"].as_array().into_iter().flatten() {
            let index = |i: usize, list: &[String]| row[i].as_u64().and_then(|n| list.get(n as usize)).cloned();
            let (Some(t_ms), Some(data_type), Some(sender)) = (row[0].as_i64(), index(1, &types), index(2, &senders)) else {
                return Err("bad row".into());
            };
            data.rows.push(Row {
                t_ms,
                data_type,
                sender,
                values: row[3].as_array().into_iter().flatten().map(value).collect(),
                payload: row.get(4).and_then(Value::as_str).map(str::to_owned),
            });
        }
        for m in v["messages"].as_array().into_iter().flatten() {
            data.messages.push((m[0].as_i64().ok_or("bad message")?, m[1].as_str().unwrap_or_default().to_owned()));
        }
        for s in v["station_states"].as_array().into_iter().flatten() {
            let code = s[1].as_u64().and_then(|c| u8::try_from(c).ok()).ok_or("bad state")?;
            data.station_states.push((s[0].as_i64().ok_or("bad state")?, code));
        }
        let mut extra_daq = Vec::new();
        for d in v["extra_daq"].as_array().into_iter().flatten() {
            let channel = Channel::ALL.into_iter().find(|c| Some(motor::channel_key(*c)) == d[0].as_str()).ok_or("bad DAQ row")?;
            let (Some(t_ms), Some(value)) = (d[1].as_i64(), value(&d[2])) else { return Err("bad DAQ row".into()) };
            extra_daq.push((channel, Sample { t_ms, value, source: Source::DaqSd }));
        }
        let model = match model {
            Some([spec, cea, grain]) => {
                let text = |b: &[u8]| String::from_utf8_lossy(b).into_owned();
                Some(Rc::new(burn::Model::from_json(&text(spec), &text(cea), &text(grain))?))
            }
            None => None,
        };
        Ok(Self {
            id: text("id").unwrap_or_default(),
            source_kind: text("source_kind").unwrap_or_default(),
            label: text("label"),
            mode: text("mode"),
            provenance: text("provenance"),
            simulated: v["simulated"].as_bool().unwrap_or(false),
            planner: serde_json::from_value(v["planner_prediction"].clone()).ok().flatten(),
            station_software: text("station_software").unwrap_or_default(),
            data,
            extra_daq,
            model,
        })
    }
}

/// GroundStation26 `flight_review::Loaded`.
struct Loaded {
    data: Data,
    review: RecordingReview,
    extra_daq: Vec<(Channel, Sample)>,
}

/// GroundStation26 `flight_review::load` for a requested range (a saved burn
/// report is used only without one; bundles hold no fire-archive sources).
fn load(inputs: &Inputs, from_ms: Option<i64>, to_ms: Option<i64>, now_ms: i64) -> Result<Loaded, (u16, String)> {
    let mut data = inputs.data.clone();
    data.clip(from_ms, to_ms);
    let mut review = RecordingReview {
        id: inputs.id.clone(),
        source_kind: inputs.source_kind.clone(),
        label: inputs.id.clone(),
        from_ms,
        to_ms,
        ..RecordingReview::default()
    };
    let (mode, igt, provenance) = flight::context(&data);
    review.mode = mode;
    review.integrated_ground_test = igt;
    review.provenance = provenance;
    if let Some(label) = &inputs.label {
        review.label = label.clone();
    }
    review.mode = review.mode.or(inputs.mode.clone());
    review.provenance = review.provenance.or(inputs.provenance.clone());
    let mut extra_daq = inputs.extra_daq.clone();
    extra_daq.retain(|(_, s)| from_ms.is_none_or(|f| s.t_ms >= f) && to_ms.is_none_or(|t| s.t_ms <= t));
    review.simulated = inputs.simulated
        || igt
        || review.mode.as_deref() == Some("hitl")
        || review.provenance.as_deref().is_some_and(|p| p != "hardware");
    review.data_from_ms = data.first_ms();
    review.data_to_ms = data.last_ms();
    burn::use_model(inputs.model.clone());
    review.motor = motor::view(
        &data,
        motor::Context {
            id: format!("review.{}", inputs.id),
            label: review.label.clone(),
            mode: review.mode.clone().unwrap_or_default(),
            simulated: review.simulated,
            created_ms: now_ms,
            saved: None,
            plan: inputs.planner.as_ref(),
            extra_daq: extra_daq.clone(),
            archive: None,
        },
    );
    let missing = burn::model_was_missing();
    burn::use_model(None);
    if missing {
        return Err((NEED_STATION, "This bundle has no hybrid motor model for the burn's predicted curve; this window needs the station.".into()));
    }
    review.flight = flight::compute(&data, review.mode.as_deref(), review.provenance.as_deref());
    if let Some(f) = &mut review.flight {
        f.simulated |= review.simulated;
    }
    data.sort();
    Ok(Loaded { data, review, extra_daq })
}

fn checked(inputs: &Bundle, from_ms: Option<i64>, to_ms: Option<i64>, now_ms: i64) -> Result<Loaded, (u16, String)> {
    if from_ms.zip(to_ms).is_some_and(|(a, b)| b <= a) {
        return Err((400, "The range must end after it starts.".into()));
    }
    load(&*inputs.inputs()?, from_ms, to_ms, now_ms)
}

/// `GET /api/flight_review/recording/{id}?from_ms&to_ms`: the review JSON.
pub(crate) fn summary(inputs: &Bundle, from_ms: Option<i64>, to_ms: Option<i64>, now_ms: i64) -> Result<Vec<u8>, (u16, String)> {
    let loaded = checked(inputs, from_ms, to_ms, now_ms)?;
    serde_json::to_vec(&loaded.review).map_err(|e| (500, e.to_string()))
}

/// GroundStation26 `flight_review::provenance`.
fn provenance(inputs: &Inputs, review: &RecordingReview, from_ms: Option<i64>, to_ms: Option<i64>, rows: usize, now_ms: i64) -> export::Provenance {
    export::Provenance {
        source_id: review.id.clone(),
        source_kind: review.source_kind.clone(),
        label: review.label.clone(),
        mode: review.mode.clone(),
        data_provenance: review.provenance.clone(),
        simulated: review.simulated,
        integrated_ground_test: review.integrated_ground_test,
        from_ms,
        to_ms,
        generated_ms: now_ms,
        station_software: inputs.station_software.clone(),
        telemetry_rows: rows,
        daq_sd_rows: 0,
        notes: Vec::new(),
    }
}

/// `.../motor_export` or `.../flight_export` for a range: (file name, ZIP).
pub(crate) fn export(bundle: &Bundle, kind: &str, from_ms: Option<i64>, to_ms: Option<i64>, now_ms: i64) -> Result<(String, Vec<u8>), (u16, String)> {
    let mut loaded = checked(bundle, from_ms, to_ms, now_ms)?;
    let inputs = bundle.inputs()?;
    let (mut from, mut to) = (loaded.review.from_ms, loaded.review.to_ms);
    if kind == "motor_export" {
        let Some(view) = loaded.review.motor.clone() else {
            return Err((404, "No motor data (load cells, chamber or tank PT) in this recording or range.".into()));
        };
        // GroundStation26 `flight_review::motor_export_bytes`.
        if from.is_none() && to.is_none() {
            (from, to) = (view.window_from_ms, view.window_to_ms);
            loaded.data.clip(from, to);
            loaded.extra_daq.retain(|(_, s)| from.is_none_or(|f| s.t_ms >= f) && to.is_none_or(|t| s.t_ms <= t));
        }
        let prov = provenance(&inputs, &loaded.review, from, to, loaded.data.rows.len(), now_ms);
        let name = format!("motor-{}.zip", export::file_stem(&loaded.review.label));
        burn::use_model(inputs.model.clone());
        let bytes = export::motor_bundle(&loaded.data, &loaded.extra_daq, &view, prov);
        burn::use_model(None);
        Ok((name, bytes))
    } else {
        let Some(summary) = loaded.review.flight.clone() else {
            return Err((404, "No flight data (FC phases, altitude or velocity) in this recording or range.".into()));
        };
        // GroundStation26 `flight_review::flight_export_bytes`.
        if from.is_none() && to.is_none() {
            from = summary.launch_accepted_ms.or(summary.t0_ms).or(summary.liftoff_ms).map(|t| t - 30_000);
            to = summary.landed_ms.map(|t| t + 60_000);
            loaded.data.clip(from, to);
        }
        let prov = provenance(&inputs, &loaded.review, from, to, loaded.data.rows.len(), now_ms);
        let name = format!("flight-{}.zip", export::file_stem(&loaded.review.label));
        Ok((name, export::flight_bundle(&loaded.data, &summary, prov)))
    }
}
