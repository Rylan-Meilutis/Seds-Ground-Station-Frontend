use once_cell::sync::Lazy;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

pub type FlightState = String;

/// The valve board's oxidiser tank pressure transducer (psi).
pub const OX_TANK_PRESSURE: &str = "OX_TANK_PRESSURE";
/// The same PT's former name. Rows, layouts and dashboards saved before the
/// rename (local caches, offline replay files) still carry it; the client maps
/// it to [`OX_TANK_PRESSURE`] on row ingress and when saved state is loaded.
pub const LEGACY_FUEL_TANK_PRESSURE: &str = "FUEL_TANK_PRESSURE";

/// Maps a legacy telemetry type name to its canonical name; any other name is
/// returned unchanged.
pub fn canonical_data_type(name: &str) -> &str {
    if name == LEGACY_FUEL_TANK_PRESSURE {
        OX_TANK_PRESSURE
    } else {
        name
    }
}

/// Renames legacy telemetry type names (and the matching human labels) in a
/// saved layout, view config or replay file. Whole strings only, in values and
/// object keys. Returns whether anything changed. Mirrors the backend's
/// `layout::canonicalize_legacy_type_names`.
pub fn canonicalize_legacy_type_names(value: &mut serde_json::Value) -> bool {
    fn renamed(text: &str) -> Option<&'static str> {
        match text {
            LEGACY_FUEL_TANK_PRESSURE => Some(OX_TANK_PRESSURE),
            "Fuel Tank Pressure" => Some("Ox Tank Pressure"),
            "Fuel tank pressure" => Some("Ox tank pressure"),
            "FUEL TANK PRESSURE" => Some("OX TANK PRESSURE"),
            _ => None,
        }
    }
    fn walk(v: &mut serde_json::Value, changed: &mut bool) {
        match v {
            serde_json::Value::String(text) => {
                if let Some(new) = renamed(text) {
                    *text = new.to_string();
                    *changed = true;
                }
            }
            serde_json::Value::Object(map) => {
                let legacy: Vec<String> = map.keys().filter(|k| renamed(k).is_some()).cloned().collect();
                for key in legacy {
                    let new = renamed(&key).unwrap().to_string();
                    if let Some(item) = map.remove(&key) {
                        map.entry(new).or_insert(item);
                        *changed = true;
                    }
                }
                map.values_mut().for_each(|v| walk(v, changed));
            }
            serde_json::Value::Array(items) => items.iter_mut().for_each(|v| walk(v, changed)),
            _ => {}
        }
    }
    let mut changed = false;
    walk(value, &mut changed);
    changed
}

#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default, Serialize, Deserialize,
)]
pub struct TelemetryTextId(u32);

impl TelemetryTextId {
    pub const EMPTY: Self = Self(0);

    pub fn is_empty(self) -> bool {
        self.0 == 0
    }
}

#[derive(Default)]
struct TelemetryTextInterner {
    by_text: HashMap<Arc<str>, TelemetryTextId>,
    by_id: Vec<Arc<str>>,
}

impl TelemetryTextInterner {
    fn intern(&mut self, value: &str) -> TelemetryTextId {
        if value.is_empty() {
            return TelemetryTextId::EMPTY;
        }
        if let Some(id) = self.by_text.get(value) {
            return *id;
        }
        let text: Arc<str> = Arc::from(value);
        let id = TelemetryTextId((self.by_id.len() as u32).saturating_add(1));
        self.by_id.push(text.clone());
        self.by_text.insert(text, id);
        id
    }

    fn resolve(&self, id: TelemetryTextId) -> Arc<str> {
        if id.is_empty() {
            return Arc::from("");
        }
        self.by_id
            .get(id.0.saturating_sub(1) as usize)
            .cloned()
            .unwrap_or_else(|| Arc::from(""))
    }
}

static TELEMETRY_TEXT_INTERNER: Lazy<Mutex<TelemetryTextInterner>> =
    Lazy::new(|| Mutex::new(TelemetryTextInterner::default()));

pub fn intern_telemetry_text(value: &str) -> TelemetryTextId {
    TELEMETRY_TEXT_INTERNER
        .lock()
        .ok()
        .map(|mut interner| interner.intern(value))
        .unwrap_or(TelemetryTextId::EMPTY)
}

pub fn resolve_telemetry_text(id: TelemetryTextId) -> Arc<str> {
    TELEMETRY_TEXT_INTERNER
        .lock()
        .ok()
        .map(|interner| interner.resolve(id))
        .unwrap_or_else(|| Arc::from(""))
}

pub fn display_flight_state(state: &str) -> String {
    let mut out = String::with_capacity(state.len() + 4);
    let mut prev: Option<char> = None;
    let mut chars = state.chars().peekable();

    while let Some(ch) = chars.next() {
        if matches!(ch, '_' | '-') {
            if !out.ends_with(' ') && !out.is_empty() {
                out.push(' ');
            }
            prev = Some(' ');
            continue;
        }

        let next = chars.peek().copied();
        let needs_space = match (prev, next) {
            (Some(p), _) if p.is_lowercase() && ch.is_uppercase() => true,
            (Some(p), _) if p.is_ascii_digit() && ch.is_alphabetic() => true,
            (Some(p), Some(n)) if p.is_uppercase() && ch.is_uppercase() && n.is_lowercase() => true,
            _ => false,
        };

        if needs_space && !out.ends_with(' ') {
            out.push(' ');
        }
        out.push(ch);
        prev = Some(ch);
    }

    out.trim().to_string()
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct BoardStatusEntry {
    pub board: String,
    #[serde(default)]
    pub board_label: String,
    pub sender_id: String,
    pub seen: bool,
    #[serde(default)]
    pub packet_count: u64,
    pub last_seen_ms: Option<u64>,
    pub age_ms: Option<u64>,
    /// Local receipt anchor, never a server timestamp or a wire field.
    #[serde(skip, default = "board_status_received_ms")]
    pub received_mono_ms: i64,
}
fn board_status_received_ms() -> i64 {
    super::monotonic_now_ms() as i64
}

#[cfg(test)]
mod board_age_tests {
    use super::*;
    #[test]
    fn snapshot_age_advances_between_packets_without_changing_on_clone() {
        let mut entry: BoardStatusEntry = serde_json::from_str(
            r#"{"board":"FC","sender_id":"FC","seen":true,"last_seen_ms":100,"age_ms":0}"#,
        )
        .unwrap();
        entry.received_mono_ms = 1000;
        assert_eq!(entry.current_age_ms(1000), Some(0));
        assert_eq!(entry.current_age_ms(1100), Some(100));
        assert_eq!(entry.clone().current_age_ms(1500), Some(500));
        assert_eq!(entry.current_age_ms(900), Some(0));
        entry.age_ms = Some(250);
        entry.received_mono_ms = 2000;
        assert_eq!(entry.current_age_ms(2100), Some(350));
        assert!(
            serde_json::to_value(&entry)
                .unwrap()
                .get("received_mono_ms")
                .is_none()
        );
        entry.age_ms = None;
        assert_eq!(entry.current_age_ms(9999), None);
    }
}

impl BoardStatusEntry {
    pub fn current_age_ms(&self, now_mono_ms: i64) -> Option<u64> {
        self.age_ms.map(|age| {
            age.saturating_add(now_mono_ms.saturating_sub(self.received_mono_ms).max(0) as u64)
        })
    }

    pub fn display_name(&self) -> &str {
        if self.board_label.trim().is_empty() {
            &self.board
        } else {
            &self.board_label
        }
    }

    pub fn from_sender_id(sender_id: &str) -> Option<Self> {
        if sender_id.trim().is_empty() {
            return None;
        }

        Some(Self {
            board: sender_id.to_string(),
            board_label: display_flight_state(sender_id),
            sender_id: sender_id.to_string(),
            seen: false,
            packet_count: 0,
            last_seen_ms: None,
            age_ms: None,
            received_mono_ms: board_status_received_ms(),
        })
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BoardStatusMsg {
    pub boards: Vec<BoardStatusEntry>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, Eq, PartialEq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum NetworkTopologyNodeKind {
    Router,
    Endpoint,
    Side,
    Board,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, Eq, PartialEq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum NetworkTopologyStatus {
    Online,
    Offline,
    Simulated,
}

impl NetworkTopologyStatus {
    pub fn merged(self, other: Self) -> Self {
        use NetworkTopologyStatus::{Offline, Online, Simulated};

        match (self, other) {
            (Offline, _) | (_, Offline) => Offline,
            (Simulated, _) | (_, Simulated) => Simulated,
            _ => Online,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Eq, PartialEq, Hash)]
pub struct NetworkTopologyNode {
    pub id: String,
    pub label: String,
    pub kind: NetworkTopologyNodeKind,
    pub status: NetworkTopologyStatus,
    pub group: String,
    pub sender_id: Option<String>,
    #[serde(default)]
    pub last_seen_ms: Option<u64>,
    #[serde(default)]
    pub age_ms: Option<u64>,
    #[serde(skip, default = "board_status_received_ms")]
    pub received_mono_ms: i64,
    #[serde(default)]
    pub endpoints: Vec<String>,
    #[serde(default = "default_true")]
    pub show_in_details: bool,
    pub detail: Option<String>,
    #[serde(default)]
    pub stats: Option<NetworkTopologyStats>,
}

impl NetworkTopologyNode {
    pub fn current_age_ms(&self, now_mono_ms: i64) -> Option<u64> {
        self.age_ms.map(|age| {
            age.saturating_add(now_mono_ms.saturating_sub(self.received_mono_ms).max(0) as u64)
        })
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, Eq, PartialEq, Default, Hash)]
pub struct NetworkTopologyStats {
    #[serde(default)]
    pub packets_sent: u64,
    #[serde(default)]
    pub packets_received: u64,
    #[serde(default)]
    pub bytes_sent: u64,
    #[serde(default)]
    pub bytes_received: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, Eq, PartialEq, Hash)]
pub struct NetworkTopologyLink {
    pub source: String,
    pub target: String,
    pub label: Option<String>,
    pub status: NetworkTopologyStatus,
}

#[derive(Debug, Clone, Serialize, Deserialize, Eq, PartialEq, Default, Hash)]
pub struct LaunchLogStatus {
    pub session: u64,
    pub deadline_utc_ms: u64,
    pub revision: u64,
    pub flags: u32,
    pub received_age_ms: u64,
    pub matches_expected: Option<bool>,
    pub expected_session: Option<u64>,
    pub expected_deadline_utc_ms: Option<u64>,
    pub expected_revision: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Eq, PartialEq, Default, Hash)]
pub struct BoardDiagnostics {
    #[serde(default)]
    pub launch_log: Option<LaunchLogStatus>,
    /// Internal MCU die temperature, not ambient. Absent on older firmware.
    #[serde(default)]
    pub die_temperature_centi_c: Option<i32>,
    #[serde(default)]
    pub die_temperature_sample_age_ms: u32,
    pub version: u32,
    pub received_age_ms: u64,
    pub uptime_ticks: u64,
    pub uptime_frequency_hz: u32,
    pub reset_flags: u32,
    pub watchdog_missing_mask: u32,
    /// RF/PB router services that failed to start and are being retried,
    /// e.g. "OTA storage unavailable". Empty on older backends/boards.
    #[serde(default)]
    pub service_faults: Vec<String>,
    pub heap_live_bytes: Option<u32>,
    pub heap_peak_bytes: Option<u32>,
    pub heap_region_bytes: Option<u32>,
    pub allocation_failures: Option<u32>,
    pub admission_refusals: u32,
    pub can_rx_drops: Option<u32>,
    pub can_tx_rejected: Option<u32>,
    pub uart_rx_drops: Option<u32>,
    pub tx_queue_high_water: Option<u32>,
    pub sd_accepted_records: Option<u32>,
    pub sd_processed_records: Option<u32>,
    pub sd_rejected_records: Option<u32>,
    pub sd_flushed_records: Option<u32>,
    pub sd_write_errors: Option<u32>,
    pub sd_max_write_ms: Option<u32>,
    /// Freshness of the separate DAQ SD-status report, independent of board health.
    #[serde(default)]
    pub sd_status_received_age_ms: Option<u64>,
    #[serde(default)]
    pub sd_flags: Option<u32>,
    #[serde(default)]
    pub sd_last_write_age_ms: Option<u32>,
    #[serde(default)]
    pub sd_last_flush_age_ms: Option<u32>,
    #[serde(default)]
    pub sd_init_stage: Option<u32>,
    #[serde(default)]
    pub sd_retry_count: Option<u32>,
    #[serde(default)]
    pub sd_retry_requested_token: Option<u32>,
    #[serde(default)]
    pub sd_retry_completed_token: Option<u32>,
    #[serde(default)]
    pub sd_init_failures: Option<u32>,
    pub adc_overruns: Option<u32>,
    pub diagnostic_send_failures: u32,
    /// DAQ live input faults. Absent on older backends, older DAQ firmware and other boards.
    #[serde(default)]
    pub daq_input: Option<DaqInputHealthStatus>,
    /// GB/PB LTC2990 power monitors. Absent on older backends/firmware and other boards.
    #[serde(default)]
    pub power_monitors: Option<PowerMonitorHealth>,
    /// AB Hall current-sense trust. Absent on older backends/firmware and other boards.
    #[serde(default)]
    pub current_sense: Option<CurrentSenseStatus>,
}

/// GB/PB power monitors (backend `PowerMonitorHealth`, BOARD_HEALTH "LTC1").
#[derive(Debug, Clone, Serialize, Deserialize, Eq, PartialEq, Default, Hash)]
pub struct PowerMonitorHealth {
    pub voltage_present: bool,
    pub current_present: bool,
    /// Current I2C retry delay, ms; 0 when both monitors answer.
    pub retry_delay_ms: u32,
    pub retries: u32,
    /// GB only; PB has no bus-clear counters.
    #[serde(default)]
    pub bus_clears: Option<u32>,
    #[serde(default)]
    pub failed_bus_clears: Option<u32>,
}

/// AB Hall current-sense trust (backend `CurrentSenseStatus`, BOARD_HEALTH "CSN1").
#[derive(Debug, Clone, Serialize, Deserialize, Eq, PartialEq, Default, Hash)]
pub struct CurrentSenseStatus {
    #[serde(default)]
    pub untrusted: Vec<String>,
    #[serde(default)]
    pub mismatch_abort_enabled: bool,
    #[serde(default)]
    pub mismatch_latched: Vec<String>,
}

/// DAQ input-health block (backend daq_input_health.rs). Millivolts; `None`
/// means no reading in the DAQ's last report interval.
#[derive(Debug, Clone, Serialize, Deserialize, Eq, PartialEq, Default, Hash)]
pub struct DaqInputHealthStatus {
    pub received_age_ms: u64,
    #[serde(default)]
    pub faults: Vec<DaqInputFaultStatus>,
    #[serde(default)]
    pub kg50_p6: bool,
    #[serde(default)]
    pub vmain_mv: Option<u16>,
    #[serde(default)]
    pub vmain_min_mv: Option<u16>,
    #[serde(default)]
    pub chamber_mv: Option<u16>,
    #[serde(default)]
    pub chamber_min_mv: Option<u16>,
    #[serde(default)]
    pub p6_imbalance_max_mv: Option<u16>,
    #[serde(default)]
    pub p6_common_mv: Option<u16>,
    #[serde(default)]
    pub supply_low_mv: u16,
    #[serde(default)]
    pub chamber_floor_mv: u16,
    /// DAQ ADC channels over range now or within the backend's recent window.
    /// Older backends omit it.
    #[serde(default)]
    pub adc_channels: Vec<DaqAdcChannelStatus>,
    /// The DAQ's measured +3V3A (STM32 VDDA from VREFINT), volts.
    #[serde(default)]
    pub vdda_v: Option<Volts>,
    /// Highest VDDA in the last report interval, volts.
    #[serde(default)]
    pub vdda_max_v: Option<Volts>,
    /// The MCP3564R reference the DAQ applies, volts.
    #[serde(default)]
    pub mcp_vref_v: Option<Volts>,
    /// Where the DAQ's reference came from: "station" | "flash" | "default".
    #[serde(default)]
    pub mcp_vref_source: Option<String>,
}

/// One DAQ ADC channel's over-range state (backend daq_input_health.rs).
#[derive(Debug, Clone, Serialize, Deserialize, Eq, PartialEq, Default, Hash)]
pub struct DaqAdcChannelStatus {
    pub key: String,
    pub label: String,
    #[serde(default)]
    pub bit: u8,
    #[serde(default)]
    pub over_range_now: bool,
    /// Below −full scale.
    #[serde(default)]
    pub low: bool,
    #[serde(default)]
    pub latched: bool,
    #[serde(default)]
    pub count_total: u64,
    /// Over-range samples in the last 60 s.
    #[serde(default)]
    pub count_recent: u64,
    /// Start of the current episode (UTC ms).
    #[serde(default)]
    pub first_over_range_ms: Option<i64>,
    #[serde(default)]
    pub last_over_range_ms: Option<i64>,
}

/// A voltage in a hashable status block: compared and hashed by bit pattern.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, Default)]
#[serde(transparent)]
pub struct Volts(pub f32);

impl PartialEq for Volts {
    fn eq(&self, other: &Self) -> bool {
        self.0.to_bits() == other.0.to_bits()
    }
}
impl Eq for Volts {}
impl std::hash::Hash for Volts {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.0.to_bits().hash(state);
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Eq, PartialEq, Default, Hash)]
pub struct DaqInputFaultStatus {
    pub key: String,
    pub label: String,
    pub active: bool,
    pub latched: bool,
    pub episodes: u16,
    #[serde(default)]
    pub advice: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Eq, PartialEq, Default, Hash)]
pub struct NetworkBoardHealthEntry {
    /// Host CPU/package sensor; unsupported platforms report unavailable.
    #[serde(default)]
    pub cpu_temperature_centi_c: Option<i32>,
    #[serde(default)]
    pub cpu_temperature_age_ms: u64,
    #[serde(default)]
    pub diagnostics: Option<BoardDiagnostics>,
    pub sender_id: String,
    pub healthy: bool,
    pub age_ms: u64,
    pub last_seen_ms: u64,
    pub disconnects: u64,
    pub reconnects: u64,
    pub rx_observations: u64,
    pub observed_ms: u64,
    pub offline_ms: u64,
    pub longest_outage_ms: u64,
    pub current_outage_ms: u64,
    /// Board network time minus the station's network time at receipt (in the
    /// packet's time-base epoch), from the least-delayed recent packet.
    #[serde(default)]
    pub clock_offset_ms: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Eq, PartialEq, Default, Hash)]
pub struct NetworkTopologyMsg {
    #[serde(default)]
    pub health: Vec<NetworkBoardHealthEntry>,
    pub generated_ms: u64,
    #[serde(default)]
    pub simulated: bool,
    #[serde(default)]
    pub nodes: Vec<NetworkTopologyNode>,
    #[serde(default)]
    pub links: Vec<NetworkTopologyLink>,
    #[serde(default)]
    pub traffic: Option<NetworkTrafficSnapshot>,
}

fn default_true() -> bool {
    true
}

#[cfg(test)]
mod tests {
    use super::display_flight_state;

    #[test]
    fn legacy_fuel_tank_pressure_rows_ingest_as_ox_tank_pressure() {
        let mut row: super::TelemetryRow = serde_json::from_value(serde_json::json!({
            "timestamp_ms": 1, "data_type": "FUEL_TANK_PRESSURE", "sender_id": "VB", "values": [512.0]
        }))
        .unwrap();
        assert_eq!(row.interned_data_type_id(), super::intern_telemetry_text("OX_TANK_PRESSURE"));
        row.refresh_interned_ids();
        assert_eq!(row.data_type, "OX_TANK_PRESSURE");
        assert_eq!(row.interned_data_type_id(), super::intern_telemetry_text("OX_TANK_PRESSURE"));
        assert_eq!(super::canonical_data_type("KG1000"), "KG1000");
    }

    #[test]
    fn saved_layouts_rename_the_legacy_ox_tank_type_and_label() {
        let mut layout = serde_json::json!({"data_tab": {"tabs": [
            {"id": "FUEL_TANK_PRESSURE", "label": "Fuel Tank Pressure", "channels": ["Tank Pressure"]},
            {"id": "FUEL_FLOW", "label": "Fuel flow (FUEL_TANK_PRESSURE text)"}
        ]}, "per_type": {"FUEL_TANK_PRESSURE": {"min": 0}}});
        assert!(super::canonicalize_legacy_type_names(&mut layout));
        assert_eq!(layout["data_tab"]["tabs"][0]["id"], "OX_TANK_PRESSURE");
        assert_eq!(layout["data_tab"]["tabs"][0]["label"], "Ox Tank Pressure");
        assert_eq!(layout["data_tab"]["tabs"][1]["id"], "FUEL_FLOW");
        assert_eq!(layout["data_tab"]["tabs"][1]["label"], "Fuel flow (FUEL_TANK_PRESSURE text)");
        assert_eq!(layout["per_type"]["OX_TANK_PRESSURE"]["min"], 0);
        assert!(!super::canonicalize_legacy_type_names(&mut layout));
    }

    #[test]
    fn telemetry_rows_decode_with_and_without_over_range_bits() {
        use super::TelemetryRow;
        use crate::telemetry_dashboard::over_range::{OverRange, Reading};
        let old: TelemetryRow = serde_json::from_str(
            r#"{"timestamp_ms":1,"data_type":"KG1000","sender_id":"DAQ","values":[0.5,null]}"#,
        )
        .unwrap();
        assert_eq!((old.over_range, old.over_range_low), (0, 0));
        assert_eq!(old.reading(0), Reading::Value(0.5));
        assert_eq!(old.reading(1), Reading::Missing, "null without a flag stays no data");
        // Zero bits are omitted again, so cached rows keep their old shape.
        assert!(!serde_json::to_string(&old).unwrap().contains("over_range"));

        let new: TelemetryRow = serde_json::from_str(
            r#"{"timestamp_ms":1,"data_type":"LOADCELL_WEIGHT_KG","sender_id":"DAQ","values":[null,null,3.0],"over_range":3,"over_range_low":2}"#,
        )
        .unwrap();
        assert_eq!(new.over_range_at(0), Some(OverRange::High));
        assert_eq!(new.over_range_at(1), Some(OverRange::Low));
        assert_eq!(new.over_range_at(2), None);
        assert_eq!(new.reading(2), Reading::Value(3.0));
        // The persisted telemetry cache round-trips the bits.
        let cached: TelemetryRow = serde_json::from_str(&serde_json::to_string(&new).unwrap()).unwrap();
        assert_eq!((cached.over_range, cached.over_range_low), (3, 2));
    }

    #[test]
    fn displays_camel_case_flight_states_with_spaces() {
        assert_eq!(display_flight_state("FillTest"), "Fill Test");
        assert_eq!(display_flight_state("PadIdle"), "Pad Idle");
        assert_eq!(display_flight_state("MECOState"), "MECO State");
    }

    #[test]
    fn displays_delimited_flight_states_with_spaces() {
        assert_eq!(display_flight_state("fill_test"), "fill test");
        assert_eq!(display_flight_state("Fill-Test"), "Fill Test");
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TelemetryRow {
    /// Canonical device UTC only when the backend has explicit source evidence.
    #[serde(default,skip_serializing_if="Option::is_none")]
    pub source_timestamp_ms:Option<i64>,
    pub timestamp_ms: i64,
    #[serde(default)]
    pub received_timestamp_ms: i64,
    pub data_type: String,
    #[serde(skip)]
    pub(crate) data_type_id: TelemetryTextId,
    #[serde(default)]
    pub sender_id: String,
    #[serde(skip)]
    pub(crate) sender_id_id: TelemetryTextId,
    pub values: Vec<Option<f32>>,
    /// Bit i set: `values[i]` is ADC over range (that value is null). The
    /// backend omits the field when no channel is over range.
    #[serde(default, skip_serializing_if = "is_zero_u32")]
    pub over_range: u32,
    /// Bit i set: that over-range value was below −full scale.
    #[serde(default, skip_serializing_if = "is_zero_u32")]
    pub over_range_low: u32,
}

fn is_zero_u32(value: &u32) -> bool {
    *value == 0
}

impl TelemetryRow {
    /// Channel `index` as a view shows it: over range, a value or missing.
    #[cfg(test)]
    pub(crate) fn reading(&self, index: usize) -> super::over_range::Reading {
        super::over_range::Reading::of(&self.values, self.over_range, self.over_range_low, index)
    }

    pub(crate) fn over_range_at(&self, index: usize) -> Option<super::over_range::OverRange> {
        super::over_range::OverRange::from_bits(self.over_range, self.over_range_low, index)
    }

    /// Row ingress: every live, history, cached and replayed row passes here,
    /// so legacy type names are canonicalized before anything keys on them.
    pub fn refresh_interned_ids(&mut self) {
        if self.data_type == LEGACY_FUEL_TANK_PRESSURE {
            self.data_type = OX_TANK_PRESSURE.to_string();
        }
        self.data_type_id = intern_telemetry_text(&self.data_type);
        self.sender_id_id = intern_telemetry_text(&self.sender_id);
    }

    pub fn interned_data_type_id(&self) -> TelemetryTextId {
        if self.data_type_id.is_empty() && !self.data_type.is_empty() {
            return intern_telemetry_text(canonical_data_type(&self.data_type));
        }
        self.data_type_id
    }

    pub fn interned_sender_id(&self) -> TelemetryTextId {
        if self.sender_id_id.is_empty() && !self.sender_id.is_empty() {
            return intern_telemetry_text(&self.sender_id);
        }
        self.sender_id_id
    }
}

/// Ground Station router counters, sampled before WebSocket batching.
#[derive(Debug, Clone, Serialize, Deserialize, Eq, PartialEq, Default, Hash)]
pub struct NetworkTrafficSnapshot {
    pub interval_ms: u64,
    pub sides: Vec<NetworkTrafficSide>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Eq, PartialEq, Default, Hash)]
pub struct NetworkTrafficSide {
    pub side_id: usize,
    pub name: String,
    pub ingress_enabled: bool,
    pub egress_enabled: bool,
    pub totals: NetworkTopologyStats,
    /// Absent until two samples exist, or if the side/counters reset.
    pub delta: Option<NetworkTopologyStats>,
}
