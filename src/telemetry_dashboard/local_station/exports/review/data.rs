//! Port of GroundStation26 `backend/src/flight_review/data.rs`: the rows one
//! review reads. Offline they come from the bundle's
//! `derived/review_inputs.json` (the station's `read_pool` of the whole
//! source) and are clipped to the window as the station clips its read.

/// One telemetry row on network time (`source_timestamp_ms` when the row has
/// one, as the burn report reads it).
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct Row {
    pub t_ms: i64,
    pub data_type: String,
    pub sender: String,
    pub values: Vec<Option<f64>>,
    pub payload: Option<String>,
}

impl Row {
    pub(crate) fn v(&self, i: usize) -> Option<f64> {
        self.values.get(i).copied().flatten().filter(|v| v.is_finite())
    }
    pub(crate) fn from_fc(&self) -> bool {
        canonical_sender_id(self.sender.trim()) == "FC"
    }
}

/// Everything one review reads: telemetry rows (time order), the message log
/// (FC messages and the station's deployment notices) and the station's own
/// flight-state changes.
#[derive(Clone, Debug, Default)]
pub(crate) struct Data {
    pub rows: Vec<Row>,
    pub messages: Vec<(i64, String)>,
    pub station_states: Vec<(i64, u8)>,
}

impl Data {
    pub(crate) fn of_type<'a>(&'a self, data_type: &'a str) -> impl Iterator<Item = &'a Row> + 'a {
        self.rows.iter().filter(move |r| r.data_type == data_type)
    }
    pub(crate) fn sort(&mut self) {
        self.rows.sort_by_key(|r| r.t_ms);
        self.messages.sort_by_key(|m| m.0);
        self.station_states.sort_by_key(|s| s.0);
    }
    pub(crate) fn first_ms(&self) -> Option<i64> {
        self.rows.first().map(|r| r.t_ms)
    }
    pub(crate) fn last_ms(&self) -> Option<i64> {
        self.rows.last().map(|r| r.t_ms)
    }
    /// The recording's context: the first GS_STATION_MODE row.
    pub(crate) fn station_mode(&self) -> Option<serde_json::Value> {
        self.of_type("GS_STATION_MODE")
            .find_map(|r| r.payload.as_deref().and_then(|p| serde_json::from_str(p).ok()))
    }
    /// Keeps only what falls in `[from, to]`.
    pub(crate) fn clip(&mut self, from_ms: Option<i64>, to_ms: Option<i64>) {
        let keep = |t: i64| from_ms.is_none_or(|f| t >= f) && to_ms.is_none_or(|e| t <= e);
        // The station-mode context rows are kept whatever their time.
        self.rows.retain(|r| keep(r.t_ms) || r.data_type == "GS_STATION_MODE");
        self.messages.retain(|m| keep(m.0));
        self.station_states.retain(|s| keep(s.0));
    }
}


/// GroundStation26 `types::canonical_sender_id`: the boards' legacy sender ids.
pub(crate) fn canonical_sender_id(sender: &str) -> &str {
    match sender {
        "GW" => "GB",
        "DAQB" => "DAQ",
        _ => sender,
    }
}
