//! Versioned observation DTO and UI-only sequence state. No command transport lives here.
use serde::{Deserialize, Serialize};
use std::collections::VecDeque;

pub const VERSION: u16 = 1;
pub const MAX_ACTIVE: usize = 4;
pub const MAX_TERMINAL: usize = 32;
pub const MAX_RESULT_LINKS: usize = 200;
pub const MAX_STEPS: usize = 160;
pub const MAX_WATCH: usize = 8;
pub const MAX_ACTIVE_BYTES: usize = 32 * 1024;
pub const MAX_RESULT_BYTES: usize = 128 * 1024;
pub const MAX_MEMORY_BYTES: usize = 8 * 1024 * 1024;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RunKey {
    pub backend_instance: String,
    pub reset_epoch: String,
    pub run_id: String,
}
impl RunKey {
    pub fn valid(&self) -> bool {
        fn hex(s: &str, n: usize) -> bool {
            s.len() == n && s.bytes().all(|c| c.is_ascii_hexdigit())
        }
        hex(&self.backend_instance, 32) && hex(&self.reset_epoch, 16) && hex(&self.run_id, 32)
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    ValveSelfTest,
    NitrogenTest,
    Fill,
    Turbocharge,
    ColdFlow,
    ContinueFillSequence,
    IgniterSequence,
}
impl Kind {
    pub fn has_gse_safe_cancel(self) -> bool {
        matches!(
            self,
            Self::ValveSelfTest
                | Self::NitrogenTest
                | Self::Fill
                | Self::Turbocharge
                | Self::ColdFlow
        )
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Outcome {
    Running,
    Paused,
    Cancelling,
    Passed,
    Failed,
    Cancelled,
    Interrupted,
    Unknown,
}
impl Outcome {
    pub fn terminal(self) -> bool {
        matches!(
            self,
            Self::Passed | Self::Failed | Self::Cancelled | Self::Interrupted
        )
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StepState {
    Planned,
    Current,
    Watching,
    Passed,
    Failed,
    Skipped,
    Unknown,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Evidence {
    Planned,
    Commanded,
    ElectricalConfirmed,
    BoardReported,
    Measured,
    Unknown,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Value {
    Boolean(bool),
    Number(f64),
}
impl Value {
    fn finite(&self) -> bool {
        !matches!(self, Self::Number(v) if !v.is_finite())
    }
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Watch {
    pub id: String,
    pub label: String,
    pub target: Option<Value>,
    pub observed: Option<Value>,
    pub unit: Option<String>,
    pub age_ms: Option<u64>,
    pub valid_for_ms: u64,
    pub evidence: Evidence,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Step {
    pub id: String,
    pub label: String,
    pub state: StepState,
    #[serde(default)]
    pub watch: Vec<Watch>,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Snapshot {
    pub version: u16,
    pub key: RunKey,
    pub revision: u64,
    pub kind: Kind,
    pub outcome: Outcome,
    pub origin_client: Option<String>,
    pub title: String,
    pub message: String,
    pub current_step: Option<String>,
    pub next_step: Option<String>,
    pub steps: Vec<Step>,
    pub observed_age_ms: u64,
    pub valid_for_ms: u64,
    #[serde(default)]
    pub cancel_allowed: bool,
    #[serde(default)]
    pub cold_flow_release_allowed: bool,
    #[serde(default)]
    pub notification_id: Option<u64>,
    #[serde(default)]
    pub notification_timestamp_ms: Option<i64>,
    #[serde(default)]
    pub reporting_error: Option<String>,
    #[serde(default)]
    pub watch_history_truncated: bool,
    #[serde(default = "complete_details")]
    pub details_complete: bool,
}
fn complete_details() -> bool {
    true
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Error {
    Invalid,
    Bounds,
    Stale,
    Conflict,
    NoCapacity,
    NotCurrent,
    Permission,
    Unavailable,
}
impl Snapshot {
    /// Compact authoritative terminal header; no synthetic step states.
    pub fn compact_result(&self) -> Self {
        Self {
            version: self.version,
            key: self.key.clone(),
            revision: self.revision,
            kind: self.kind,
            outcome: self.outcome,
            origin_client: self.origin_client.clone(),
            title: self.title.clone(),
            message: self.message.clone(),
            current_step: None,
            next_step: None,
            steps: Vec::new(),
            observed_age_ms: self.observed_age_ms,
            valid_for_ms: self.valid_for_ms,
            cancel_allowed: false,
            cold_flow_release_allowed: false,
            notification_id: self.notification_id,
            notification_timestamp_ms: self.notification_timestamp_ms,
            reporting_error: self.reporting_error.clone(),
            watch_history_truncated: self.watch_history_truncated,
            details_complete: false,
        }
    }
    fn memory_bytes(&self) -> usize {
        let mut bytes = std::mem::size_of::<Self>()
            + self.key.backend_instance.capacity()
            + self.key.reset_epoch.capacity()
            + self.key.run_id.capacity()
            + self.title.capacity()
            + self.message.capacity()
            + self.steps.capacity() * std::mem::size_of::<Step>();
        for value in [
            &self.origin_client,
            &self.current_step,
            &self.next_step,
            &self.reporting_error,
        ]
        .into_iter()
        .flatten()
        {
            bytes += value.capacity();
        }
        for step in &self.steps {
            bytes += step.id.capacity()
                + step.label.capacity()
                + step.watch.capacity() * std::mem::size_of::<Watch>();
            for watch in &step.watch {
                bytes += watch.id.capacity()
                    + watch.label.capacity()
                    + watch.unit.as_ref().map_or(0, String::capacity);
            }
        }
        bytes
    }
    pub fn validate(&self) -> Result<(), Error> {
        if !self.details_complete
            && (!self.outcome.terminal()
                || !self.steps.is_empty()
                || self.current_step.is_some()
                || self.next_step.is_some()
                || self.cancel_allowed)
        {
            return Err(Error::Invalid);
        }

        let label = |s: &str| !s.is_empty() && s.len() <= 96;
        if self.version != VERSION
            || !self.key.valid()
            || self.revision == 0
            || !label(&self.title)
            || self.message.len() > 512
            || self.steps.len() > MAX_STEPS
            || self.valid_for_ms == 0
            || self.valid_for_ms > 2000
            || self.origin_client.as_ref().is_some_and(|s| s.len() > 128)
            || self.reporting_error.as_ref().is_some_and(|s| s.len() > 512)
        {
            return Err(Error::Invalid);
        }
        let mut ids = std::collections::HashSet::new();
        for step in &self.steps {
            if !label(&step.id)
                || !label(&step.label)
                || !ids.insert(&step.id)
                || step.watch.len() > MAX_WATCH
            {
                return Err(Error::Invalid);
            }
            for watch in &step.watch {
                if !label(&watch.id)
                    || !label(&watch.label)
                    || watch.valid_for_ms > 2000
                    || watch.unit.as_ref().is_some_and(|s| s.len() > 24)
                    || watch.target.as_ref().is_some_and(|v| !v.finite())
                    || watch.observed.as_ref().is_some_and(|v| !v.finite())
                {
                    return Err(Error::Invalid);
                }
                if watch.observed.is_some()
                    && (watch.evidence == Evidence::Unknown
                        || watch.valid_for_ms == 0
                        || watch.age_ms.is_none_or(|age| age > watch.valid_for_ms))
                {
                    return Err(Error::Invalid);
                }
                if matches!(
                    watch.evidence,
                    Evidence::Measured | Evidence::ElectricalConfirmed | Evidence::BoardReported
                ) && (watch.observed.is_none()
                    || watch.valid_for_ms == 0
                    || watch.age_ms.is_none_or(|age| age > watch.valid_for_ms))
                {
                    return Err(Error::Invalid);
                }
            }
        }
        for id in [&self.current_step, &self.next_step].into_iter().flatten() {
            if !ids.contains(id) {
                return Err(Error::Invalid);
            }
        }
        if self.cold_flow_release_allowed
            && (self.kind != Kind::ColdFlow || self.outcome != Outcome::Running)
        {
            return Err(Error::Invalid);
        }
        if self.outcome.terminal() && self.cancel_allowed {
            return Err(Error::Invalid);
        }
        if self.cancel_allowed && !self.kind.has_gse_safe_cancel() {
            return Err(Error::Invalid);
        }
        let bytes = serde_json::to_vec(self).map_err(|_| Error::Invalid)?.len();
        let limit = if self.outcome.terminal() {
            MAX_RESULT_BYTES
        } else {
            MAX_ACTIVE_BYTES
        };
        if bytes > limit || self.memory_bytes() > MAX_MEMORY_BYTES {
            return Err(Error::Bounds);
        }
        Ok(())
    }
}

/// Live server admission for one run control; `reason` explains a refusal.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Gate {
    #[serde(default)]
    pub allowed: bool,
    #[serde(default)]
    pub reason: Option<String>,
}
/// Station-provided Pause/Resume/Cancel admission for the current run.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RunControls {
    pub key: RunKey,
    #[serde(default)]
    pub pause: Gate,
    #[serde(default)]
    pub resume: Gate,
    #[serde(default)]
    pub cancel: Gate,
}
impl RunControls {
    fn valid(&self) -> bool {
        self.key.valid()
            && [&self.pause, &self.resume, &self.cancel]
                .iter()
                .all(|gate| gate.reason.as_ref().is_none_or(|r| r.len() <= 512))
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Control {
    Pause,
    Resume,
}
// Only limits unsolicited surfacing of old results; open views never expire.
const RECENT_RESULT_WINDOW_MS: u64 = 4000;
#[derive(Clone, Debug, PartialEq)]
pub struct Panel {
    pub snapshot: Snapshot,
    pub observed_at_ms: u64,
    pub minimized: bool,
    pub visible: bool,
    pub cancel_pending: bool,
    /// A Pause or Resume request awaiting the station's next revision.
    pub control_pending: Option<Control>,
    pub historical: bool,
    pub applied_retired: bool,
    pub loading_steps: Vec<(String, String)>,
    manual_minimize: bool,
}
impl Panel {
    pub fn age_ms(&self, now: u64) -> Option<u64> {
        now.checked_sub(self.observed_at_ms)
            .map(|age| age.saturating_add(self.snapshot.observed_age_ms))
    }
    pub fn fresh(&self, now: u64) -> bool {
        !self.applied_retired
            && self.snapshot.reporting_error.is_none()
            && self
                .age_ms(now)
                .is_some_and(|age| age <= self.snapshot.valid_for_ms)
    }
    pub fn observed_value<'a>(&self, watch: &'a Watch, now: u64) -> Option<&'a Value> {
        let age = self.age_ms(now)?;
        if !self.fresh(now) || watch.evidence == Evidence::Unknown {
            return None;
        }
        if watch
            .age_ms
            .is_none_or(|own| own.saturating_add(age) > watch.valid_for_ms)
        {
            return None;
        }
        watch.observed.as_ref()
    }
    pub fn can_release(&self, now: u64, permission: bool) -> bool {
        permission
            && self.fresh(now)
            && !self.cancel_pending
            && !self.historical
            && self.snapshot.kind == Kind::ColdFlow
            && self.snapshot.outcome == Outcome::Running
            && self.snapshot.cold_flow_release_allowed
    }
    pub fn can_cancel(&self, now: u64, permission: bool, api_version: Option<u16>) -> bool {
        permission
            && api_version == Some(VERSION)
            && self.fresh(now)
            && !self.cancel_pending
            && !self.historical
            && self.snapshot.cancel_allowed
            && self.snapshot.kind.has_gse_safe_cancel()
            && matches!(self.snapshot.outcome, Outcome::Running | Outcome::Paused)
    }
    /// Pause while running, Resume while paused; both need live station admission.
    pub fn can_control(
        &self,
        control: Control,
        now: u64,
        permission: bool,
        gates: Option<&RunControls>,
    ) -> bool {
        let (outcome, gate) = match control {
            Control::Pause => (Outcome::Running, gates.map(|g| &g.pause)),
            Control::Resume => (Outcome::Paused, gates.map(|g| &g.resume)),
        };
        permission
            && self.fresh(now)
            && !self.cancel_pending
            && self.control_pending.is_none()
            && !self.historical
            && self.snapshot.outcome == outcome
            && gate.is_some_and(|gate| gate.allowed)
    }
}
#[derive(Clone, Debug, Default, PartialEq, Deserialize, Serialize)]
pub struct BoardDisplay {
    pub connected: bool,
    pub known: bool,
    pub boot: Option<String>,
    pub epoch: Option<u32>,
    pub outputs: Option<u32>,
    #[serde(default)]
    pub outputs_age_ms: Option<u64>,
}
pub type BoardDisplayMap = std::collections::BTreeMap<String, BoardDisplay>;
impl BoardDisplay {
    fn valid(&self) -> bool {
        self.boot
            .as_ref()
            .is_none_or(|boot| boot.len() == 16 && boot.bytes().all(|b| b.is_ascii_hexdigit()))
            && (!self.known
                || (self.connected
                    && self.boot.as_ref().is_some_and(|boot| {
                        boot.len() == 16
                            && boot.bytes().all(|b| b.is_ascii_hexdigit())
                            && boot != "0000000000000000"
                            && boot != "ffffffffffffffff"
                    })
                    && self.epoch.is_some_and(|epoch| epoch > 0)
                    && self.outputs.is_some()
                    && self.outputs_age_ms.is_some_and(|age| age != u64::MAX)))
    }
}
pub type OutputDisplayMap = std::collections::BTreeMap<usize, bool>;
#[derive(Clone)]
struct OutputReceipt {
    boot: String,
    epoch: u32,
    value: bool,
    received_at: u64,
    source_age_ms: u64,
}
#[derive(Default)]
struct OutputCache {
    receipts: std::collections::BTreeMap<usize, OutputReceipt>,
    generations: std::collections::BTreeMap<String, (String, u32)>,
    fences: std::collections::BTreeMap<String, u64>,
    query_fences: std::collections::BTreeMap<String, u64>,
    last_received: Option<u64>,
}
fn output_source(index: usize) -> Option<(&'static str, u32)> {
    Some(match index {
        0 => ("VB", 1),
        1 => ("VB", 2),
        2 => ("VB", 4),
        3 => ("AB", 2),
        4 => ("AB", 4),
        _ => return None,
    })
}
fn output_index(watch: &Watch) -> Option<usize> {
    let index = watch
        .id
        .strip_prefix("board-output-")
        .or_else(|| watch.id.strip_prefix("cold-valve-"))?
        .parse()
        .ok()?;
    output_source(index)?;
    Some(index)
}
pub fn board_output_metadata_present(watch: &Watch, boards: &BoardDisplayMap) -> bool {
    output_index(watch)
        .and_then(output_source)
        .is_some_and(|(board, _)| boards.contains_key(board))
}
pub fn board_output_display(watch: &Watch, outputs: &OutputDisplayMap) -> Option<Value> {
    outputs
        .get(&output_index(watch)?)
        .copied()
        .map(Value::Boolean)
}
impl OutputCache {
    fn accepts_clock(&self, received: u64) -> bool {
        self.last_received.is_none_or(|old| received >= old)
    }
    fn clear_board(&mut self, board: &str) {
        self.receipts
            .retain(|index, _| output_source(*index).is_some_and(|(source, _)| source != board));
    }
    fn offer(&mut self, index: usize, row: &BoardDisplay, value: bool, age: u64, received: u64) {
        if age == u64::MAX {
            return;
        }
        let Some(boot) = row.boot.as_ref() else {
            return;
        };
        let Some(epoch) = row.epoch else {
            return;
        };
        if let Some(old) = self.receipts.get(&index) {
            let old_age = received
                .checked_sub(old.received_at)
                .and_then(|elapsed| old.source_age_ms.checked_add(elapsed));
            if old.boot == *boot
                && old.epoch == epoch
                && old_age.is_some_and(|old_age| age >= old_age)
            {
                return;
            }
        }
        self.receipts.insert(
            index,
            OutputReceipt {
                boot: boot.clone(),
                epoch,
                value,
                received_at: received,
                source_age_ms: age,
            },
        );
    }
    /// Fixed capacity: two generations and five Boolean output receipts. Neither
    /// duplicate polls nor producer expiry can renew an original source stamp.
    fn observe(&mut self, boards: &BoardDisplayMap, panels: &[Panel], received: u64) -> bool {
        if !self.accepts_clock(received) {
            self.receipts.clear();
            return false;
        }
        for board in ["AB", "VB"] {
            let row = boards
                .get(board)
                .filter(|row| row.valid() && row.connected && row.known);
            let Some(row) = row else {
                self.clear_board(board);
                self.generations.remove(board);
                self.fences.insert(board.into(), received);
                self.query_fences.insert(board.into(), received);
                continue;
            };
            let generation = (row.boot.clone().unwrap(), row.epoch.unwrap());
            if self
                .generations
                .get(board)
                .is_some_and(|old| old != &generation)
            {
                self.clear_board(board);
                self.fences.insert(board.into(), received);
            }
            self.generations.insert(board.into(), generation);
            for index in 0..5 {
                let (source, bit) = output_source(index).unwrap();
                if source == board
                    && self.query_fences.get(board).is_none_or(|fence| {
                        received
                            .checked_sub(*fence)
                            .is_some_and(|elapsed| row.outputs_age_ms.unwrap() < elapsed)
                    })
                {
                    self.offer(
                        index,
                        row,
                        row.outputs.unwrap() & bit != 0,
                        row.outputs_age_ms.unwrap(),
                        received,
                    );
                }
            }
        }
        for panel in panels
            .iter()
            .filter(|panel| !panel.historical && !panel.snapshot.outcome.terminal())
        {
            let current = panel
                .snapshot
                .current_step
                .as_ref()
                .and_then(|id| panel.snapshot.steps.iter().find(|step| &step.id == id));
            for watch in current.into_iter().flat_map(|step| &step.watch) {
                if watch.evidence != Evidence::BoardReported {
                    continue;
                }
                let Some(index) = output_index(watch) else {
                    continue;
                };
                let (board, _) = output_source(index).unwrap();
                let Some(row) = boards
                    .get(board)
                    .filter(|row| row.valid() && row.connected && row.known)
                else {
                    continue;
                };
                let Some(Value::Boolean(value)) = panel.observed_value(watch, received) else {
                    continue;
                };
                let Some(age) = panel
                    .age_ms(received)
                    .and_then(|age| watch.age_ms.and_then(|own| age.checked_add(own)))
                else {
                    continue;
                };
                if self.fences.get(board).is_some_and(|fence| {
                    received
                        .checked_sub(*fence)
                        .is_none_or(|elapsed| age >= elapsed)
                }) {
                    continue;
                }
                self.offer(index, row, *value, age, received);
            }
        }
        self.last_received = Some(received);
        true
    }
    fn values(&self) -> OutputDisplayMap {
        self.receipts
            .iter()
            .map(|(index, row)| (*index, row.value))
            .collect()
    }
}
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Envelope {
    pub version: u16,
    pub backend_instance: String,
    pub reset_epoch: String,
    pub cancel_api_version: u16,
    pub active: Vec<Snapshot>,
    pub terminal: Vec<Snapshot>,
    pub reporting_error: Option<String>,
    #[serde(default)]
    pub result_links: Vec<ResultNotice>,
    #[serde(default)]
    pub board_display: BoardDisplayMap,
    #[serde(default)]
    pub controls: Vec<RunControls>,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResultNotice {
    pub key: RunKey,
    pub notification_id: u64,
    pub timestamp_ms: i64,
    pub warning: bool,
}
#[derive(Default)]
pub struct Model {
    source_epoch: u64,
    on_actions: bool,
    expected_scope: Option<(String, String)>,
    pub board_display: BoardDisplayMap,
    output_cache: OutputCache,
    panels: Vec<Panel>,
    results: VecDeque<Snapshot>,
    notices: VecDeque<ResultNotice>,
    pub unavailable: bool,
    pub result_error: Option<String>,
    requested_details: VecDeque<RunKey>,
    release_pending: Vec<RunKey>,
    controls: Vec<RunControls>,
}
impl Model {
    /// A single authenticated station observation. Validate the whole envelope
    /// before applying any part; a wrong source cannot clear the current context.
    pub fn accept_envelope(
        &mut self,
        epoch: u64,
        envelope: Envelope,
        observed_at: u64,
        now: u64,
    ) -> Result<(), Error> {
        if epoch != self.source_epoch {
            return Err(Error::Stale);
        }
        if !self.output_cache.accepts_clock(observed_at) {
            self.output_cache.receipts.clear();
            self.unavailable = true;
            return Err(Error::Stale);
        }
        let scope = RunKey {
            backend_instance: envelope.backend_instance.clone(),
            reset_epoch: envelope.reset_epoch.clone(),
            run_id: "0".repeat(32),
        };
        if envelope.version != VERSION
            || envelope.cancel_api_version != VERSION
            || !scope.valid()
            || envelope.active.len() > MAX_ACTIVE
            || envelope.terminal.len() > MAX_TERMINAL
            || envelope.result_links.len() > MAX_RESULT_LINKS
            || envelope.board_display.len() > 2
            || envelope.controls.len() > MAX_ACTIVE
            || envelope.controls.iter().any(|c| {
                !c.valid() || c.key.backend_instance != envelope.backend_instance
            })
            || envelope
                .board_display
                .iter()
                .any(|(id, row)| !matches!(id.as_str(), "AB" | "VB") || !row.valid())
        {
            self.unavailable = true;
            return Err(Error::Invalid);
        }
        let mut keys = Vec::with_capacity(MAX_ACTIVE + MAX_TERMINAL);
        for snapshot in envelope.active.iter().chain(&envelope.terminal) {
            snapshot.validate()?;
            if snapshot.key.backend_instance != envelope.backend_instance
                || keys.contains(&snapshot.key)
            {
                self.unavailable = true;
                return Err(Error::Conflict);
            }
            keys.push(snapshot.key.clone());
        }
        if envelope
            .active
            .iter()
            .any(|s| s.outcome.terminal() || s.key.reset_epoch != envelope.reset_epoch)
            || envelope.terminal.iter().any(|s| !s.outcome.terminal())
        {
            self.unavailable = true;
            return Err(Error::Invalid);
        }
        let mut link_ids = Vec::with_capacity(envelope.result_links.len());
        for link in &envelope.result_links {
            if !link.key.valid()
                || link.key.backend_instance != envelope.backend_instance
                || link.notification_id == 0
                || link_ids.contains(&(link.notification_id, link.timestamp_ms))
            {
                self.unavailable = true;
                return Err(Error::Invalid);
            }
            link_ids.push((link.notification_id, link.timestamp_ms));
        }
        self.set_scope(
            epoch,
            envelope.backend_instance,
            envelope.reset_epoch.clone(),
        );
        for snapshot in envelope.active {
            let key = snapshot.key.clone();
            self.accept(epoch, snapshot, observed_at, now)?;
            // Only the header-validated CURRENT envelope can re-establish an
            // active projection. Immutable result/history reads cannot.
            if let Some(panel) = self.panels.iter_mut().find(|panel| panel.snapshot.key == key) {
                panel.applied_retired = false;
            }
        }
        for snapshot in envelope.terminal {
            if snapshot.key.reset_epoch == envelope.reset_epoch
                && (self
                    .panels
                    .iter()
                    .any(|panel| panel.snapshot.key == snapshot.key)
                    || snapshot.observed_age_ms < RECENT_RESULT_WINDOW_MS)
            {
                self.accept(epoch, snapshot, observed_at, now)?;
            } else {
                self.catalog(snapshot);
            }
        }
        for link in envelope.result_links {
            if !self.notices.iter().any(|old| {
                old.key == link.key
                    && old.notification_id == link.notification_id
                    && old.timestamp_ms == link.timestamp_ms
            }) {
                self.notices.push_back(link);
            }
        }
        while self.notices.len() > MAX_RESULT_LINKS {
            self.notices.pop_front();
        }
        if !self
            .output_cache
            .observe(&envelope.board_display, &self.panels, observed_at)
        {
            self.unavailable = true;
            return Err(Error::Stale);
        }
        self.board_display = envelope.board_display;
        self.controls = envelope.controls;
        self.unavailable = envelope.reporting_error.is_some();
        Ok(())
    }
    /// The station's latest live Pause/Resume/Cancel admission for a run.
    pub fn controls(&self, key: &RunKey) -> Option<&RunControls> {
        if self.unavailable {
            return None;
        }
        self.controls.iter().find(|c| &c.key == key)
    }
    pub fn can_control(&self, key: &RunKey, control: Control, now: u64, permission: bool) -> bool {
        let gates = self.controls(key);
        self.panels
            .iter()
            .any(|p| &p.snapshot.key == key && p.can_control(control, now, permission, gates))
    }
    pub fn request_control(
        &mut self,
        key: &RunKey,
        control: Control,
        now: u64,
        permission: bool,
    ) -> Result<RunKey, Error> {
        if self.unavailable {
            return Err(Error::Unavailable);
        }
        if !self.can_control(key, control, now, permission) {
            return Err(Error::Permission);
        }
        let panel = self
            .panels
            .iter_mut()
            .find(|p| &p.snapshot.key == key)
            .ok_or(Error::NotCurrent)?;
        panel.control_pending = Some(control);
        // Run-key intent only; the station re-checks every gate.
        Ok(panel.snapshot.key.clone())
    }
    pub fn control_failed(&mut self, key: &RunKey) {
        if let Some(panel) = self.panels.iter_mut().find(|p| &p.snapshot.key == key) {
            panel.control_pending = None;
        }
    }
    fn catalog(&mut self, mut snapshot: Snapshot) {
        if !snapshot.details_complete
            && let Some(full) = self
                .results
                .iter()
                .find(|s| s.key == snapshot.key && s.details_complete)
        {
            let mut retained = full.clone();
            retained.observed_age_ms = snapshot.observed_age_ms;
            retained.notification_id = snapshot.notification_id.or(retained.notification_id);
            retained.notification_timestamp_ms = snapshot
                .notification_timestamp_ms
                .or(retained.notification_timestamp_ms);
            snapshot = retained;
        }
        if let Some(old) = self.results.iter().position(|s| s.key == snapshot.key) {
            self.results.remove(old);
        }
        if let (Some(id), Some(time)) =
            (snapshot.notification_id, snapshot.notification_timestamp_ms)
            && !self
                .notices
                .iter()
                .any(|n| n.key == snapshot.key && n.notification_id == id && n.timestamp_ms == time)
        {
            self.notices.push_back(ResultNotice {
                key: snapshot.key.clone(),
                notification_id: id,
                timestamp_ms: time,
                warning: snapshot.outcome != Outcome::Passed,
            });
            while self.notices.len() > MAX_RESULT_LINKS {
                self.notices.pop_front();
            }
        }
        self.results.push_back(snapshot);
        while self.results.len() > MAX_TERMINAL {
            self.results.pop_front();
        }
    }
    pub fn needed_result_details(&mut self) -> Vec<RunKey> {
        let keys = self
            .panels
            .iter()
            .filter(|panel| {
                panel.visible
                    && panel.snapshot.outcome.terminal()
                    && !panel.snapshot.details_complete
                    && !self.requested_details.contains(&panel.snapshot.key)
            })
            .map(|p| p.snapshot.key.clone())
            .collect::<Vec<_>>();
        for key in &keys {
            self.requested_details.push_back(key.clone());
        }
        while self.requested_details.len() > MAX_TERMINAL {
            self.requested_details.pop_front();
        }
        keys
    }
    pub fn history(&self) -> Vec<(RunKey, String, Outcome, Option<i64>)> {
        let mut rows = Vec::new();
        for snapshot in self
            .panels
            .iter()
            .map(|panel| &panel.snapshot)
            .chain(self.results.iter().rev())
        {
            if !rows.iter().any(|(key, _, _, _)| key == &snapshot.key) {
                rows.push((
                    snapshot.key.clone(),
                    snapshot.title.clone(),
                    snapshot.outcome,
                    snapshot.notification_timestamp_ms,
                ));
            }
        }
        rows
    }
    pub fn cached_result(&self, key: &RunKey) -> Option<&Snapshot> {
        self.results
            .iter()
            .find(|s| &s.key == key && s.details_complete)
    }
    /// Retire current facts, not the station/transport or immutable results.
    /// Preserve original ages and payloads; no synthetic time can make old
    /// progress fresh. A new current envelope must explicitly establish it.
    pub fn retire_applied_context(&mut self) {
        for panel in &mut self.panels {
            if !panel.snapshot.outcome.terminal() { panel.applied_retired = true; }
            panel.cancel_pending = false;
            panel.control_pending = None;
        }
        self.controls.clear();
        self.board_display.clear();
        self.output_cache = OutputCache::default();
        self.release_pending.clear();
        self.unavailable = true;
    }
    pub fn retire_source(&mut self) {
        self.panels.clear();
        self.results.clear();
        self.notices.clear();
        self.expected_scope = None;
        self.board_display.clear();
        self.output_cache = OutputCache::default();
        self.result_error = None;
        self.requested_details.clear();
        self.release_pending.clear();
        self.controls.clear();
        self.unavailable = true;
    }
    pub fn set_source(&mut self, epoch: u64) {
        if epoch != self.source_epoch {
            self.source_epoch = epoch;
            self.panels.clear();
            self.results.clear();
            self.notices.clear();
            self.unavailable = false;
            self.expected_scope = None;
            self.board_display.clear();
            self.output_cache = OutputCache::default();
            self.result_error = None;
            self.requested_details.clear();
            self.release_pending.clear();
            self.controls.clear();
        }
    }
    pub fn set_scope(&mut self, epoch: u64, backend_instance: String, reset_epoch: String) {
        self.set_source(epoch);
        if self.expected_scope.as_ref() != Some(&(backend_instance.clone(), reset_epoch.clone())) {
            self.panels.clear();
            self.results.clear();
            self.notices.clear();
            self.unavailable = false;
            self.expected_scope = Some((backend_instance, reset_epoch));
            self.board_display.clear();
            self.output_cache = OutputCache::default();
            self.result_error = None;
            self.requested_details.clear();
            self.release_pending.clear();
            self.controls.clear();
        }
    }
    pub fn set_surface(&mut self, actions: bool) {
        if self.on_actions == actions {
            return;
        }
        self.on_actions = actions;
        for panel in &mut self.panels {
            if !panel.historical {
                panel.minimized = !actions || panel.manual_minimize;
            }
        }
    }
    pub fn output_display(&self) -> OutputDisplayMap {
        self.output_cache.values()
    }
    pub fn panels(&self) -> &[Panel] {
        &self.panels
    }
    pub fn notices(&self) -> &VecDeque<ResultNotice> {
        &self.notices
    }
    pub fn accept(
        &mut self,
        epoch: u64,
        snapshot: Snapshot,
        observed_at: u64,
        now: u64,
    ) -> Result<(), Error> {
        self.accept_inner(epoch, snapshot, observed_at, now, false)
    }
    fn accept_inner(
        &mut self,
        epoch: u64,
        mut snapshot: Snapshot,
        observed_at: u64,
        now: u64,
        historical: bool,
    ) -> Result<(), Error> {
        if epoch != self.source_epoch {
            return Err(Error::Stale);
        }
        if self
            .expected_scope
            .as_ref()
            .is_some_and(|(backend, reset)| {
                backend != &snapshot.key.backend_instance
                    || (!historical && reset != &snapshot.key.reset_epoch)
            })
        {
            return Err(Error::Stale);
        }
        snapshot.validate()?;
        if observed_at > now {
            self.unavailable = true;
            return Err(Error::Invalid);
        }
        let index = self
            .panels
            .iter()
            .position(|p| p.snapshot.key == snapshot.key);
        if let Some(i) = index {
            if snapshot.revision < self.panels[i].snapshot.revision {
                return Err(Error::Stale);
            }
            if snapshot.kind != self.panels[i].snapshot.kind
                || snapshot.origin_client != self.panels[i].snapshot.origin_client
            {
                self.unavailable = true;
                return Err(Error::Conflict);
            }
            if snapshot.revision == self.panels[i].snapshot.revision {
                let mut previous = self.panels[i].snapshot.clone();
                let mut incoming = snapshot.clone();
                previous.observed_age_ms = 0;
                incoming.observed_age_ms = 0;
                if previous.notification_id.is_none() {
                    previous.notification_id = incoming.notification_id;
                }
                if previous.notification_timestamp_ms.is_none() {
                    previous.notification_timestamp_ms = incoming.notification_timestamp_ms;
                }
                // Binding is monotone: an earlier in-flight response may lack
                // the already observed actual notification, but cannot erase it.
                if incoming.notification_id.is_none() {
                    incoming.notification_id = previous.notification_id;
                    snapshot.notification_id = previous.notification_id;
                }
                if incoming.notification_timestamp_ms.is_none() {
                    incoming.notification_timestamp_ms = previous.notification_timestamp_ms;
                    snapshot.notification_timestamp_ms = previous.notification_timestamp_ms;
                }
                if !previous.details_complete || !incoming.details_complete {
                    if previous.compact_result() != incoming.compact_result() {
                        self.unavailable = true;
                        return Err(Error::Conflict);
                    }
                    if previous.details_complete && !incoming.details_complete {
                        let age = snapshot.observed_age_ms;
                        snapshot = previous.clone();
                        snapshot.observed_age_ms = age;
                    }
                } else if previous != incoming {
                    self.unavailable = true;
                    return Err(Error::Conflict);
                }
            }
            // Same-revision polling may update envelope age; it never reopens a
            // dismissed result or an automatically closed live result.
            let first_terminal =
                snapshot.outcome.terminal() && !self.panels[i].snapshot.outcome.terminal();
            let panel = &mut self.panels[i];
            if !snapshot.details_complete && panel.snapshot.details_complete {
                panel.loading_steps = panel
                    .snapshot
                    .steps
                    .iter()
                    .map(|step| (step.id.clone(), step.label.clone()))
                    .collect();
            } else if snapshot.details_complete {
                panel.loading_steps.clear();
            }
            if snapshot.revision > panel.snapshot.revision {
                // The station has published its response to the request.
                panel.control_pending = None;
            }
            if first_terminal {
                panel.cancel_pending = false;
                // Completion closes live progress; recorded results reopen only
                // through an explicit notification/history intent.
                panel.visible = false;
            }
            panel.snapshot = snapshot.clone();
            panel.observed_at_ms = observed_at;
        } else {
            if self.panels.len() >= MAX_ACTIVE + MAX_TERMINAL {
                if let Some(old) = self
                    .panels
                    .iter()
                    .position(|p| p.snapshot.outcome.terminal() && !p.visible)
                {
                    self.panels.remove(old);
                } else {
                    return Err(Error::NoCapacity);
                }
            }
            self.panels.push(Panel {
                snapshot: snapshot.clone(),
                observed_at_ms: observed_at,
                minimized: !self.on_actions,
                visible: !snapshot.outcome.terminal(),
                cancel_pending: false,
                control_pending: None,
                historical: false,
                applied_retired: false,
                loading_steps: Vec::new(),
                manual_minimize: false,
            });
        }
        if snapshot.outcome.terminal() {
            self.catalog(snapshot);
        }
        self.unavailable = false;
        Ok(())
    }
    pub fn minimize(&mut self, key: &RunKey) {
        if let Some(panel) = self.panels.iter_mut().find(|p| &p.snapshot.key == key) {
            panel.minimized = true;
            panel.manual_minimize = true;
        }
    }
    pub fn restore(&mut self, key: &RunKey) {
        if let Some(index) = self
            .panels
            .iter()
            .position(|panel| &panel.snapshot.key == key)
        {
            let mut panel = self.panels.remove(index);
            panel.minimized = false;
            panel.manual_minimize = false;
            panel.visible = true;
            self.panels.push(panel);
        }
    }
    /// Running, paused, cancelling and unknown are never dismissed as finished.
    pub fn dismiss(&mut self, key: &RunKey) -> bool {
        if let Some(panel) = self.panels.iter_mut().find(|p| &p.snapshot.key == key) {
            if panel.snapshot.outcome.terminal() {
                panel.visible = false;
                return true;
            }
            panel.minimized = true;
            panel.manual_minimize = true;
        }
        false
    }
    pub fn can_release(&self, key: &RunKey, now: u64, permission: bool) -> bool {
        !self.unavailable
            && !self.release_pending.contains(key)
            && self
                .panels
                .iter()
                .any(|p| &p.snapshot.key == key && p.can_release(now, permission))
    }
    pub fn request_release(
        &mut self,
        key: &RunKey,
        now: u64,
        permission: bool,
    ) -> Result<RunKey, Error> {
        if !self.can_release(key, now, permission) {
            return Err(Error::Permission);
        }
        if self.release_pending.len() >= MAX_ACTIVE {
            return Err(Error::NoCapacity);
        }
        self.release_pending.push(key.clone());
        Ok(key.clone())
    }
    pub fn release_failed(&mut self, key: &RunKey) {
        self.release_pending.retain(|k| k != key);
    }
    pub fn request_cancel(
        &mut self,
        key: &RunKey,
        now: u64,
        permission: bool,
        api_version: Option<u16>,
    ) -> Result<RunKey, Error> {
        if self.unavailable {
            return Err(Error::Unavailable);
        }
        let panel = self
            .panels
            .iter_mut()
            .find(|p| &p.snapshot.key == key)
            .ok_or(Error::NotCurrent)?;
        if !panel.can_cancel(now, permission, api_version) {
            return Err(Error::Permission);
        }
        panel.cancel_pending = true;
        // This is a run-key intent only. There is no generic command fallback.
        Ok(panel.snapshot.key.clone())
    }
    pub fn cancel_failed(&mut self, key: &RunKey) {
        if let Some(panel) = self.panels.iter_mut().find(|p| &p.snapshot.key == key) {
            panel.cancel_pending = false;
        }
    }
    pub fn tick(&mut self, _now: u64) {
        let panels = &self.panels;
        self.release_pending.retain(|key| {
            panels
                .iter()
                .any(|p| &p.snapshot.key == key && p.snapshot.cold_flow_release_allowed)
        });
    }
    pub fn open_result(&mut self, epoch: u64, snapshot: Snapshot, now: u64) -> Result<(), Error> {
        if epoch != self.source_epoch {
            return Err(Error::Stale);
        }
        if !snapshot.outcome.terminal() || !snapshot.details_complete {
            return Err(Error::Invalid);
        }
        self.accept_inner(epoch, snapshot.clone(), now, now, true)?;
        let panel = self
            .panels
            .iter_mut()
            .find(|p| p.snapshot.key == snapshot.key)
            .ok_or(Error::Unavailable)?;
        panel.historical = true;
        panel.visible = true;
        panel.minimized = false;
        self.restore(&snapshot.key);
        self.result_error = None;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    pub(super) fn run() -> Snapshot {
        serde_json::from_value(serde_json::json!({"version":1,"key":{"backend_instance":"11111111111111111111111111111111","reset_epoch":"2222222222222222","run_id":"33333333333333333333333333333333"},"revision":1,"kind":"nitrogen_test","outcome":"running","origin_client":"origin-a","title":"Nitrogen test","message":"Watching tank pressure","current_step":"raise-0","next_step":"hold-0","steps":[{"id":"raise-0","label":"Raise pressure","state":"watching","watch":[{"id":"tank-pt","label":"Tank pressure","target":50.0,"observed":42.0,"unit":"psi","age_ms":0,"valid_for_ms":2000,"evidence":"measured"}]},{"id":"hold-0","label":"Verify hold","state":"planned","watch":[]}],"observed_age_ms":0,"valid_for_ms":2000,"cancel_allowed":true})).unwrap()
    }
    #[test]
    fn output_display_is_separate_from_revision_proofs_and_never_refreshes_sensors() {
        let snapshot = run();
        let mut header:Envelope=serde_json::from_value(serde_json::json!({"version":1,"backend_instance":snapshot.key.backend_instance,"reset_epoch":snapshot.key.reset_epoch,"cancel_api_version":1,"active":[snapshot.clone()],"terminal":[],"reporting_error":null,"board_display":{"VB":{"connected":true,"known":true,"boot":"0000000000000002","epoch":3,"outputs":5,"outputs_age_ms":0}}})).unwrap();
        let mut model = Model::default();
        model.accept_envelope(0, header.clone(), 0, 0).unwrap();
        let mut output = snapshot.steps[0].watch[0].clone();
        output.id = "board-output-0".into();
        assert_eq!(
            board_output_display(&output, &model.output_display()),
            Some(Value::Boolean(true))
        );
        assert_eq!(
            board_output_display(&snapshot.steps[0].watch[0], &model.output_display()),
            None,
            "measured pressure never uses board heartbeat/output authority"
        );
        header.board_display.get_mut("VB").unwrap().outputs = Some(0);
        model
            .accept_envelope(0, header.clone(), 30000, 30000)
            .unwrap();
        assert_eq!(model.panels()[0].snapshot.revision, 1);
        assert_eq!(
            board_output_display(&output, &model.output_display()),
            Some(Value::Boolean(false)),
            "mutable display does not falsify frozen watch revisions"
        );
        assert!(
            model.panels()[0]
                .observed_value(&snapshot.steps[0].watch[0], 33000)
                .is_none(),
            "old sensor receipt remains expired"
        );
        header.board_display.get_mut("VB").unwrap().connected = false;
        header.board_display.get_mut("VB").unwrap().known = false;
        model.accept_envelope(0, header, 31000, 31000).unwrap();
        assert_eq!(board_output_display(&output, &model.output_display()), None);
        assert!(
            board_output_metadata_present(&output, &model.board_display),
            "explicit disconnect suppresses fallback to any still-fresh producer watch"
        );
        model.retire_source();
        assert!(model.board_display.is_empty());
    }
    #[test]
    fn display_metadata_schema_is_bounded_and_missing_old_fields_are_strict() {
        let snapshot = run();
        let make = |display: serde_json::Value| {
            serde_json::from_value::<Envelope>(serde_json::json!({"version":1,"backend_instance":snapshot.key.backend_instance,"reset_epoch":snapshot.key.reset_epoch,"cancel_api_version":1,"active":[snapshot.clone()],"terminal":[],"reporting_error":null,"board_display":display})).unwrap()
        };
        let mut model = Model::default();
        model
            .accept_envelope(0, make(serde_json::json!({})), 0, 0)
            .unwrap();
        assert!(model.board_display.is_empty());
        for display in [
            serde_json::json!({"FC":{"connected":true,"known":false,"boot":null,"epoch":null,"outputs":null}}),
            serde_json::json!({"AB":{"connected":false,"known":true,"boot":"0000000000000001","epoch":1,"outputs":0}}),
            serde_json::json!({"AB":{"connected":true,"known":false,"boot":"x".repeat(4096),"epoch":null,"outputs":null}}),
        ] {
            assert!(
                Model::default()
                    .accept_envelope(0, make(display), 0, 0)
                    .is_err()
            );
        }
    }
    fn display_boards(value: bool, age: u64, boot: &str) -> BoardDisplayMap {
        [(
            "VB".into(),
            BoardDisplay {
                connected: true,
                known: true,
                boot: Some(boot.into()),
                epoch: Some(3),
                outputs: Some(u32::from(value)),
                outputs_age_ms: Some(age),
            },
        )]
        .into_iter()
        .collect()
    }
    fn display_panel(value: bool, age: u64, received: u64) -> Panel {
        let mut snapshot = run();
        snapshot.observed_age_ms = age;
        let watch = &mut snapshot.steps[0].watch[0];
        watch.id = "board-output-0".into();
        watch.observed = Some(Value::Boolean(value));
        watch.evidence = Evidence::BoardReported;
        watch.unit = None;
        let mut model = Model::default();
        model.accept(0, snapshot, received, received).unwrap();
        model.panels()[0].clone()
    }
    #[test]
    fn latest_output_source_wins_in_both_directions_without_duplicate_restamping() {
        let mut cache = OutputCache::default();
        let panel = display_panel(true, 0, 100);
        cache.observe(
            &display_boards(false, 10000, "0000000000000002"),
            std::slice::from_ref(&panel),
            100,
        );
        assert_eq!(cache.values().get(&0), Some(&true));
        cache.observe(
            &display_boards(false, 11000, "0000000000000002"),
            std::slice::from_ref(&panel),
            1100,
        );
        assert_eq!(cache.receipts[&0].received_at, 100);
        assert_eq!(cache.receipts[&0].source_age_ms, 0);
        cache.observe(
            &display_boards(false, 13000, "0000000000000002"),
            &[panel],
            3100,
        );
        assert_eq!(
            cache.values().get(&0),
            Some(&true),
            "producer expiry cannot resurrect an older QUERY mask"
        );
        cache.observe(
            &display_boards(false, 0, "0000000000000002"),
            &[display_panel(true, 1500, 4100)],
            4100,
        );
        assert_eq!(
            cache.values().get(&0),
            Some(&false),
            "new QUERY supersedes an older still-fresh producer receipt"
        );
        cache.observe(
            &display_boards(false, 1000, "0000000000000002"),
            &[display_panel(true, 0, 5100)],
            5100,
        );
        assert_eq!(cache.values().get(&0), Some(&true));
        assert!(cache.receipts.len() <= 5);
    }
    #[test]
    fn disconnect_and_boot_fences_never_resurrect_prechange_producer_or_query_outputs() {
        let mut cache = OutputCache::default();
        let boot = "0000000000000002";
        cache.observe(
            &display_boards(false, 10000, boot),
            &[display_panel(true, 0, 100)],
            100,
        );
        cache.observe(
            &[("VB".into(), BoardDisplay::default())]
                .into_iter()
                .collect(),
            &[],
            1100,
        );
        assert!(cache.values().is_empty());
        cache.observe(
            &display_boards(false, 12000, boot),
            &[display_panel(true, 1500, 2100)],
            2100,
        );
        assert!(
            cache.values().is_empty(),
            "nothing from before disconnect returns on reconnect"
        );
        cache.observe(&display_boards(false, 0, boot), &[], 3100);
        assert_eq!(cache.values().get(&0), Some(&false));
        cache.observe(
            &display_boards(false, 0, "0000000000000004"),
            &[display_panel(true, 500, 4100)],
            4100,
        );
        assert_eq!(cache.values().get(&0), Some(&false));
        cache.observe(
            &display_boards(false, 1000, "0000000000000004"),
            &[display_panel(true, 1500, 5100)],
            5100,
        );
        assert_eq!(cache.values().get(&0), Some(&false));
        cache.observe(
            &display_boards(false, 2000, "0000000000000004"),
            &[display_panel(true, 0, 6100)],
            6100,
        );
        assert_eq!(cache.values().get(&0), Some(&true));
    }
    #[test]
    fn output_cache_missing_invalid_clock_sensor_and_historical_sources_fail_closed() {
        let mut cache = OutputCache::default();
        let boards = display_boards(false, 10000, "0000000000000002");
        let mut panel = display_panel(true, 0, 100);
        panel.snapshot.steps[0].watch[0].evidence = Evidence::Measured;
        cache.observe(&boards, std::slice::from_ref(&panel), 100);
        assert_eq!(
            cache.values().get(&0),
            Some(&false),
            "measured sensor cannot supply display state"
        );
        panel.snapshot.steps[0].watch[0].evidence = Evidence::BoardReported;
        panel.historical = true;
        cache.observe(&boards, std::slice::from_ref(&panel), 200);
        assert_eq!(cache.values().get(&0), Some(&false));
        panel.historical = false;
        panel.snapshot.outcome = Outcome::Passed;
        cache.observe(&boards, &[panel], 300);
        assert_eq!(cache.values().get(&0), Some(&false));
        assert!(!cache.observe(&boards, &[], 299));
        assert!(
            cache.values().is_empty(),
            "clock rollback erases display receipts"
        );
        cache.observe(&BoardDisplayMap::new(), &[], 400);
        assert!(cache.values().is_empty());
        let mut invalid = boards;
        invalid.get_mut("VB").unwrap().outputs_age_ms = None;
        cache.observe(&invalid, &[], 500);
        assert!(cache.values().is_empty());
    }
    #[test]
    fn every_actions_client_observes_identical_steps_and_origin_minimizes_elsewhere() {
        let value = run();
        let mut origin = Model::default();
        let mut observer = Model::default();
        for m in [&mut origin, &mut observer] {
            m.set_source(1);
            m.set_surface(true);
            m.accept(1, value.clone(), 100, 100).unwrap();
        }
        assert_eq!(origin.panels()[0].snapshot, observer.panels()[0].snapshot);
        assert!(!origin.panels()[0].minimized && !observer.panels()[0].minimized);
        origin.set_surface(false);
        assert!(origin.panels()[0].minimized);
        assert!(!origin.dismiss(&value.key));
        assert!(origin.panels()[0].visible);
        origin.set_surface(true);
        assert!(origin.panels()[0].minimized); // Explicit minimize choice persists.
        origin.restore(&value.key);
        assert!(!origin.panels()[0].minimized);
    }
    #[test]
    fn cancellation_intent_is_not_completion_and_old_server_has_no_command_fallback() {
        let value = run();
        let mut m = Model::default();
        m.accept(0, value.clone(), 0, 0).unwrap();
        assert_eq!(
            m.request_cancel(&value.key, 100, true, None),
            Err(Error::Permission)
        );
        assert_eq!(
            m.request_cancel(&value.key, 100, false, Some(1)),
            Err(Error::Permission)
        );
        assert_eq!(
            m.request_cancel(&value.key, 100, true, Some(1)),
            Ok(value.key.clone())
        );
        assert!(m.panels()[0].cancel_pending);
        assert_eq!(m.panels()[0].snapshot.outcome, Outcome::Running);
        assert!(m.notices().is_empty());
        m.tick(9000);
        assert!(m.panels()[0].visible);
        m.cancel_failed(&value.key);
        assert!(!m.panels()[0].cancel_pending);
        assert_eq!(
            m.request_cancel(&value.key, 2100, true, Some(1)),
            Err(Error::Permission)
        );
    }
    #[test]
    fn stale_or_missing_evidence_is_unknown_not_zero() {
        let value = run();
        let mut m = Model::default();
        m.accept(0, value.clone(), 0, 0).unwrap();
        let p = &m.panels()[0];
        assert_eq!(
            p.observed_value(&p.snapshot.steps[0].watch[0], 100),
            Some(&Value::Number(42.))
        );
        assert_eq!(p.observed_value(&p.snapshot.steps[0].watch[0], 2001), None);
        assert!(!p.fresh(2001));
        assert!(!p.fresh(0u64.wrapping_sub(1)));
    }
    #[test]
    fn compact_completion_retains_only_labels_until_authoritative_details_arrive() {
        let mut live = run();
        live.current_step = Some("step".into());
        live.next_step = None;
        live.steps = vec![Step {
            id: "step".into(),
            label: "Recorded step".into(),
            state: StepState::Watching,
            watch: vec![],
        }];
        let mut model = Model::default();
        model.accept(0, live.clone(), 0, 0).unwrap();
        let mut full = live;
        full.revision += 1;
        full.outcome = Outcome::Passed;
        full.cancel_allowed = false;
        full.current_step = None;
        full.next_step = None;
        full.steps[0].state = StepState::Passed;
        model.accept(0, full.compact_result(), 100, 100).unwrap();
        assert!(model.panels()[0].snapshot.steps.is_empty());
        assert_eq!(
            model.panels()[0].loading_steps,
            vec![("step".into(), "Recorded step".into())]
        );
        model.tick(15000);
        assert!(!model.panels()[0].visible);
        model.accept(0, full, 16000, 16000).unwrap();
        assert!(!model.panels()[0].visible, "detail arrival cannot reopen a completed live panel");
        assert!(model.panels()[0].loading_steps.is_empty());
        assert_eq!(model.panels()[0].snapshot.steps[0].state, StepState::Passed);
    }
    #[test]
    fn terminal_live_progress_closes_once_and_notification_result_stays_reopened() {
        for outcome in [Outcome::Passed, Outcome::Failed, Outcome::Cancelled, Outcome::Interrupted] {
            let mut value = run();
            let mut model = Model::default();
            model.set_surface(true);
            model.accept(0, value.clone(), 0, 0).unwrap();
            assert!(model.panels()[0].visible);
            assert!(!model.dismiss(&value.key), "running progress cannot be dismissed");
            model.restore(&value.key);
            value.revision += 1;
            value.outcome = outcome;
            value.cancel_allowed = false;
            value.current_step = None;
            value.next_step = None;
            value.notification_id = Some(9);
            value.notification_timestamp_ms = Some(1000);
            model.accept(0, value.clone(), 1000, 1000).unwrap();
            assert!(!model.panels()[0].visible, "{outcome:?} must close live progress");
            assert_eq!(model.notices().len(), 1);
            assert_eq!(model.notices()[0].warning, outcome != Outcome::Passed);
            assert_eq!(model.cached_result(&value.key), Some(&value));
            model.accept(0, value.clone(), 4000, 4000).unwrap();
            model.set_surface(false);
            model.set_surface(true);
            model.tick(15000);
            assert!(!model.panels()[0].visible, "polls/navigation cannot reopen a result");
            model.open_result(0, value.clone(), 16000).unwrap();
            assert!(model.panels()[0].visible && model.panels()[0].historical);
            model.accept(0, value.clone(), 17000, 17000).unwrap();
            model.tick(20000);
            assert!(model.panels()[0].visible, "an explicitly reopened result stays open");
            assert!(model.dismiss(&value.key));
            model.accept(0, value.clone(), 21000, 21000).unwrap();
            assert!(!model.panels()[0].visible, "polls cannot undo explicit close");
            model.open_result(0, value, 22000).unwrap();
            assert!(model.panels()[0].visible);
        }
    }
    #[test]
    fn first_observed_terminal_is_catalogued_without_covering_running_progress() {
        let mut value = run();
        value.revision = 2;
        value.outcome = Outcome::Passed;
        value.cancel_allowed = false;
        value.current_step = None;
        value.next_step = None;
        value.notification_id = Some(11);
        value.notification_timestamp_ms = Some(100);
        let mut model = Model::default();
        model.set_surface(true);
        model.accept(0, value.clone(), 100, 100).unwrap();
        assert!(!model.panels()[0].visible);
        assert_eq!(model.notices().len(), 1);
        let mut live = run();
        live.key.run_id = "4".repeat(32);
        model.accept(0, live, 101, 101).unwrap();
        assert_eq!(model.panels().iter().filter(|p| p.visible).count(), 1);
        model.open_result(0, value, 102).unwrap();
        assert_eq!(model.panels().iter().filter(|p| p.visible).count(), 2);
    }
    #[test]
    fn backend_auth_epoch_and_reset_scope_reject_stale_callbacks() {
        let value = run();
        let mut m = Model::default();
        m.set_scope(
            1,
            value.key.backend_instance.clone(),
            value.key.reset_epoch.clone(),
        );
        m.accept(1, value.clone(), 0, 0).unwrap();
        m.set_source(2);
        assert!(m.panels().is_empty());
        assert_eq!(m.accept(1, value.clone(), 0, 0), Err(Error::Stale));
        let mut other = value.clone();
        other.key.reset_epoch = "4444444444444444".into();
        m.set_scope(
            2,
            value.key.backend_instance.clone(),
            value.key.reset_epoch.clone(),
        );
        assert_eq!(m.accept(2, other, 0, 0), Err(Error::Stale));
        assert!(serde_json::from_value::<Kind>(serde_json::json!("launch")).is_err());
    }
    #[test]
    fn same_revision_conflicting_steps_cannot_overwrite_server_history() {
        let value = run();
        let mut m = Model::default();
        m.accept(0, value.clone(), 0, 0).unwrap();
        let mut conflict = value.clone();
        conflict.steps[0].state = StepState::Passed;
        assert_eq!(m.accept(0, conflict, 1, 1), Err(Error::Conflict));
        assert!(m.unavailable);
        assert_eq!(m.panels()[0].snapshot.steps[0].state, StepState::Watching);
    }
}

#[cfg(test)]
mod envelope_tests {
    use super::*;
    fn terminal() -> Snapshot {
        serde_json::from_value(serde_json::json!({"version":1,"key":{"backend_instance":"11111111111111111111111111111111","reset_epoch":"2222222222222222","run_id":"33333333333333333333333333333333"},"revision":2,"kind":"fill","outcome":"cancelled","origin_client":null,"title":"Fill","message":"Safe cancellation confirmed","current_step":null,"next_step":null,"steps":[],"observed_age_ms":8000,"valid_for_ms":2000,"cancel_allowed":false,"notification_id":7,"notification_timestamp_ms":123})).unwrap()
    }
    fn envelope(value: Snapshot) -> Envelope {
        Envelope {
            version: 1,
            backend_instance: value.key.backend_instance.clone(),
            reset_epoch: value.key.reset_epoch.clone(),
            cancel_api_version: 1,
            active: vec![],
            terminal: vec![value],
            reporting_error: None,
            result_links: Vec::new(),
            board_display: Default::default(),
            controls: Vec::new(),
        }
    }
    #[test]
    fn old_results_bind_notifications_without_reopening_on_every_poll() {
        let value = terminal();
        let mut model = Model::default();
        model.set_source(5);
        model
            .accept_envelope(5, envelope(value.clone()), 100, 100)
            .unwrap();
        assert!(model.panels().is_empty());
        assert_eq!(model.notices()[0].notification_id, 7);
        model.open_result(5, value.clone(), 200).unwrap();
        assert!(model.panels()[0].historical);
        model.dismiss(&value.key);
        model.accept_envelope(5, envelope(value), 300, 300).unwrap();
        assert!(!model.panels()[0].visible);
        assert_eq!(model.notices().len(), 1);
    }
    #[test]
    fn compact_result_fetches_once_upgrades_without_fake_steps_or_new_close_deadline() {
        let mut full = terminal();
        full.observed_age_ms = 0;
        full.steps = vec![Step {
            id: "actual-step".into(),
            label: "Actual completed step".into(),
            state: StepState::Passed,
            watch: vec![],
        }];
        let summary = full.compact_result();
        let mut model = Model::default();
        model.set_source(5);
        model.set_surface(true);
        model
            .accept_envelope(5, envelope(summary.clone()), 0, 0)
            .unwrap();
        assert!(!model.panels()[0].snapshot.details_complete);
        assert!(model.panels()[0].snapshot.steps.is_empty());
        assert!(!model.panels()[0].visible);
        assert!(model.needed_result_details().is_empty());
        assert!(model.cached_result(&full.key).is_none());
        // The notification/history GET supplies full immutable details.
        model.open_result(5, full.clone(), 1000).unwrap();
        assert!(model.panels()[0].snapshot.details_complete);
        assert_eq!(model.cached_result(&full.key).unwrap().steps.len(), 1);
        model.tick(4000);
        assert!(
            model.panels()[0].visible,
            "detail arrival must not close the operator's open view"
        );
        model
            .accept_envelope(5, envelope(summary), 4000, 4000)
            .unwrap();
        assert!(model.panels()[0].snapshot.details_complete);
        assert!(model.panels()[0].visible);
        assert!(model.needed_result_details().is_empty());
        full.steps[0].state = StepState::Failed;
        assert_eq!(model.accept(5, full, 4000, 4000), Err(Error::Conflict));
    }
    #[test]
    fn old_notification_link_survives_without_inventing_pruned_result_details() {
        let full = terminal();
        let mut header = envelope(full.compact_result());
        header.terminal.clear();
        header.result_links = vec![ResultNotice {
            key: full.key.clone(),
            notification_id: 7,
            timestamp_ms: 123,
            warning: true,
        }];
        let mut model = Model::default();
        model.set_source(5);
        model.accept_envelope(5, header, 100, 100).unwrap();
        assert_eq!(model.notices()[0].key, full.key);
        assert!(model.cached_result(&full.key).is_none());
        assert!(model.panels().is_empty());
        assert!(
            model.needed_result_details().is_empty(),
            "old links are fetched only on explicit reopen"
        );
    }
    #[test]
    fn malformed_envelope_cannot_replace_source_scope_or_create_launch() {
        let value = terminal();
        let mut model = Model::default();
        model.set_source(5);
        let mut malformed = envelope(value.clone());
        malformed.backend_instance = "bad".into();
        assert_eq!(
            model.accept_envelope(5, malformed, 100, 100),
            Err(Error::Invalid)
        );
        assert!(model.panels().is_empty());
        assert!(model.expected_scope.is_none());
        assert!(serde_json::from_value::<Kind>(serde_json::json!("launch")).is_err());
        assert_eq!(
            model.accept_envelope(4, envelope(value), 100, 100),
            Err(Error::Stale)
        );
    }
    #[test]
    fn cold_flow_release_is_server_flag_permission_source_and_run_bound_without_launch_fallback() {
        let mut snapshot = super::tests::run();
        snapshot.kind = Kind::ColdFlow;
        snapshot.title = "Auto Cold Flow".into();
        snapshot.cold_flow_release_allowed = true;
        let key = snapshot.key.clone();
        let mut model = Model::default();
        model.set_surface(true);
        model.accept(0, snapshot.clone(), 100, 100).unwrap();
        assert!(model.request_release(&key, 101, false).is_err());
        assert_eq!(model.request_release(&key, 101, true).unwrap(), key);
        assert!(model.request_release(&key, 101, true).is_err());
        assert!(
            model.panels()[0].can_cancel(101, true, Some(1)),
            "safe cancel stays available while release is pending"
        );
        let mut stale = snapshot.clone();
        stale.revision = 0;
        stale.cold_flow_release_allowed = false;
        assert!(model.accept(0, stale, 101, 101).is_err());
        assert!(!model.can_release(&key, 101, true));
        snapshot.revision = 2;
        snapshot.cold_flow_release_allowed = false;
        model.accept(0, snapshot.clone(), 102, 102).unwrap();
        model.tick(102);
        assert!(!model.can_release(&key, 102, true));
        snapshot.revision = 3;
        snapshot.cold_flow_release_allowed = true;
        model.accept(0, snapshot, 103, 103).unwrap();
        assert!(!model.can_release(&key, 2104, true));
        model.set_source(1);
        assert!(!model.can_release(&key, 104, true));
        let old = super::tests::run();
        let mut model = Model::default();
        model.accept(0, old.clone(), 100, 100).unwrap();
        assert!(!model.can_release(&old.key, 101, true));
    }
}

#[cfg(test)]
mod applied_context_tests {
    use super::*;
    #[test]
    fn applied_retirement_preserves_history_and_age_but_only_current_envelope_restores_live_facts() {
        let live=super::tests::run();
        let mut value=serde_json::to_value(&live).unwrap();
        value["key"]["run_id"]="44444444444444444444444444444444".into();
        value["outcome"]="passed".into(); value["current_step"]=serde_json::Value::Null;
        value["next_step"]=serde_json::Value::Null; value["steps"]=serde_json::json!([]); value["cancel_allowed"]=false.into();
        let terminal:Snapshot=serde_json::from_value(value).unwrap();
        let mut envelope=Envelope {controls:Vec::new(),version:1,backend_instance:live.key.backend_instance.clone(),reset_epoch:live.key.reset_epoch.clone(),
            cancel_api_version:1,active:vec![live.clone()],terminal:vec![terminal.clone()],reporting_error:None,result_links:Vec::new(),board_display:Default::default()};
        let mut model=Model::default();model.accept_envelope(0,envelope.clone(),10,10).unwrap();
        let original=model.panels()[0].clone();let source=model.source_epoch;let scope=model.expected_scope.clone();
        assert!(original.can_cancel(20,true,Some(1)));
        model.retire_applied_context();
        assert_eq!(model.source_epoch,source);assert_eq!(model.expected_scope,scope);
        assert_eq!(model.panels()[0].snapshot,original.snapshot);
        assert_eq!(model.panels()[0].observed_at_ms,original.observed_at_ms,"mode clearing never rewrites capture/receipt age");
        assert_eq!(model.cached_result(&terminal.key),Some(&terminal),"immutable result survives mode change");
        assert!(!model.panels()[0].fresh(20));assert!(!model.panels()[0].can_cancel(20,true,Some(1)));
        model.accept(0,live.clone(),21,21).unwrap();
        assert!(!model.panels()[0].fresh(21),"generic/result acceptance cannot rehabilitate a retired live projection");
        envelope.terminal.clear();model.accept_envelope(0,envelope,22,22).unwrap();
        assert!(model.panels()[0].fresh(22));assert!(model.panels()[0].can_cancel(22,true,Some(1)));
        assert_eq!(model.cached_result(&terminal.key),Some(&terminal));
    }
}

#[cfg(test)]
mod control_tests {
    use super::*;
    fn envelope(value: Snapshot) -> Envelope {
        Envelope {
            version: 1,
            backend_instance: value.key.backend_instance.clone(),
            reset_epoch: value.key.reset_epoch.clone(),
            cancel_api_version: 1,
            active: vec![],
            terminal: vec![value],
            reporting_error: None,
            result_links: Vec::new(),
            board_display: Default::default(),
            controls: Vec::new(),
        }
    }
    fn running(revision: u64, outcome: &str) -> Snapshot {
        serde_json::from_value(serde_json::json!({"version":1,"key":{"backend_instance":"11111111111111111111111111111111","reset_epoch":"2222222222222222","run_id":"44444444444444444444444444444444"},"revision":revision,"kind":"nitrogen_test","outcome":outcome,"origin_client":null,"title":"Nitrogen test","message":"Watching","current_step":null,"next_step":null,"steps":[],"observed_age_ms":0,"valid_for_ms":2000,"cancel_allowed":true})).unwrap()
    }
    fn live(value: Snapshot, pause: bool, resume: bool, reason: &str) -> Envelope {
        let mut header = envelope(value.clone());
        header.terminal.clear();
        header.controls = vec![RunControls {
            key: value.key.clone(),
            pause: Gate { allowed: pause, reason: (!pause).then(|| reason.into()) },
            resume: Gate { allowed: resume, reason: (!resume).then(|| reason.into()) },
            cancel: Gate { allowed: true, reason: None },
        }];
        header.active = vec![value];
        header
    }
    #[test]
    fn pause_and_resume_need_live_station_admission_and_clear_on_next_revision() {
        let mut model = Model::default();
        let value = running(1, "running");
        let key = value.key.clone();
        model.accept_envelope(0, live(value, true, false, "Already paused"), 10, 10).unwrap();
        assert!(model.can_control(&key, Control::Pause, 10, true));
        assert!(!model.can_control(&key, Control::Pause, 10, false), "permission");
        assert!(!model.can_control(&key, Control::Resume, 10, true), "not paused");
        assert!(!model.can_control(&key, Control::Pause, 5000, true), "stale");
        assert_eq!(model.request_control(&key, Control::Pause, 10, true), Ok(key.clone()));
        assert!(!model.can_control(&key, Control::Pause, 10, true), "one request at a time");
        model.control_failed(&key);
        assert!(model.can_control(&key, Control::Pause, 10, true));
        model.request_control(&key, Control::Pause, 10, true).unwrap();
        let paused = running(2, "paused");
        model.accept_envelope(0, live(paused, false, true, "Already paused"), 20, 20).unwrap();
        assert_eq!(model.panels()[0].control_pending, None);
        assert!(model.can_control(&key, Control::Resume, 20, true));
        assert!(!model.can_control(&key, Control::Pause, 20, true));
        assert_eq!(model.controls(&key).unwrap().pause.reason.as_deref(), Some("Already paused"));
        // Without the station's live gate nothing is enabled.
        let mut silent = envelope(running(3, "paused"));
        silent.terminal.clear();
        silent.active = vec![running(3, "paused")];
        model.accept_envelope(0, silent, 30, 30).unwrap();
        assert!(model.controls(&key).is_none());
        assert!(!model.can_control(&key, Control::Resume, 30, true));
        // Old stations omit the field entirely.
        let mut old = serde_json::to_value(live(running(4, "paused"), true, true, "")).unwrap();
        old.as_object_mut().unwrap().remove("controls");
        let old: Envelope = serde_json::from_value(old).unwrap();
        model.accept_envelope(0, old, 40, 40).unwrap();
        assert!(!model.can_control(&key, Control::Resume, 40, true));
    }
    #[test]
    fn controls_for_another_station_or_oversized_reasons_reject_the_envelope() {
        let mut model = Model::default();
        let mut header = live(running(1, "running"), true, false, "x");
        header.controls[0].key.backend_instance = "5".repeat(32);
        assert_eq!(model.accept_envelope(0, header, 10, 10), Err(Error::Invalid));
        let header = live(running(1, "running"), true, false, &"x".repeat(600));
        assert_eq!(model.accept_envelope(0, header, 10, 10), Err(Error::Invalid));
    }
    #[test]
    fn cancelled_run_closes_its_live_modal() {
        let mut model = Model::default();
        model.set_surface(true);
        let value = running(1, "paused");
        let key = value.key.clone();
        model.accept_envelope(0, live(value, false, true, "Already paused"), 10, 10).unwrap();
        assert!(model.panels()[0].visible);
        model.request_cancel(&key, 10, true, Some(1)).unwrap();
        let mut done = running(2, "cancelled");
        done.cancel_allowed = false;
        let mut header = envelope(done);
        header.controls.clear();
        model.accept_envelope(0, header, 20, 20).unwrap();
        assert!(!model.panels()[0].visible, "cancel auto-closes the modal");
        assert!(model.controls(&key).is_none());
    }
}
