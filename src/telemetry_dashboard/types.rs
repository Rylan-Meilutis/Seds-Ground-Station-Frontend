use once_cell::sync::Lazy;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

pub type FlightState = String;

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
}

impl BoardStatusEntry {
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
    pub endpoints: Vec<String>,
    #[serde(default = "default_true")]
    pub show_in_details: bool,
    pub detail: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Eq, PartialEq, Hash)]
pub struct NetworkTopologyLink {
    pub source: String,
    pub target: String,
    pub label: Option<String>,
    pub status: NetworkTopologyStatus,
}

#[derive(Debug, Clone, Serialize, Deserialize, Eq, PartialEq, Default, Hash)]
pub struct NetworkTopologyMsg {
    pub generated_ms: u64,
    #[serde(default)]
    pub simulated: bool,
    #[serde(default)]
    pub nodes: Vec<NetworkTopologyNode>,
    #[serde(default)]
    pub links: Vec<NetworkTopologyLink>,
}

fn default_true() -> bool {
    true
}

#[cfg(test)]
mod tests {
    use super::display_flight_state;

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
}

impl TelemetryRow {
    pub fn refresh_interned_ids(&mut self) {
        self.data_type_id = intern_telemetry_text(&self.data_type);
        self.sender_id_id = intern_telemetry_text(&self.sender_id);
    }

    pub fn interned_data_type_id(&self) -> TelemetryTextId {
        if self.data_type_id.is_empty() && !self.data_type.is_empty() {
            return intern_telemetry_text(&self.data_type);
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
