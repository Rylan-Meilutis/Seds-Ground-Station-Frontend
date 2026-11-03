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
    ContinueFillSequence,
    IgniterSequence,
}
impl Kind {
    pub fn has_gse_safe_cancel(self) -> bool {
        matches!(
            self,
            Self::ValveSelfTest | Self::NitrogenTest | Self::Fill | Self::Turbocharge
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

pub const AUTO_CLOSE_MS: u64 = 4000;
#[derive(Clone, Debug, PartialEq)]
pub struct Panel {
    pub snapshot: Snapshot,
    pub observed_at_ms: u64,
    pub minimized: bool,
    pub visible: bool,
    pub cancel_pending: bool,
    pub historical: bool,
    manual_minimize: bool,
    auto_close_at_ms: Option<u64>,
}
impl Panel {
    pub fn age_ms(&self, now: u64) -> Option<u64> {
        now.checked_sub(self.observed_at_ms)
            .map(|age| age.saturating_add(self.snapshot.observed_age_ms))
    }
    pub fn fresh(&self, now: u64) -> bool {
        self.snapshot.reporting_error.is_none()
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
    panels: Vec<Panel>,
    results: VecDeque<Snapshot>,
    notices: VecDeque<ResultNotice>,
    pub unavailable: bool,
    pub result_error: Option<String>,
    requested_details: VecDeque<RunKey>,
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
            self.accept(epoch, snapshot, observed_at, now)?;
        }
        for snapshot in envelope.terminal {
            if snapshot.key.reset_epoch == envelope.reset_epoch
                && snapshot.observed_age_ms < AUTO_CLOSE_MS
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
        self.unavailable = envelope.reporting_error.is_some();
        Ok(())
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
    pub fn cached_result(&self, key: &RunKey) -> Option<&Snapshot> {
        self.results
            .iter()
            .find(|s| &s.key == key && s.details_complete)
    }
    pub fn set_source(&mut self, epoch: u64) {
        if epoch != self.source_epoch {
            self.source_epoch = epoch;
            self.panels.clear();
            self.results.clear();
            self.notices.clear();
            self.unavailable = false;
            self.expected_scope = None;
            self.result_error = None;
            self.requested_details.clear();
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
            self.result_error = None;
            self.requested_details.clear();
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
            // dismissed result or restarts a terminal auto-close deadline.
            let first_terminal =
                snapshot.outcome.terminal() && !self.panels[i].snapshot.outcome.terminal();
            let panel = &mut self.panels[i];
            if first_terminal {
                panel.auto_close_at_ms = Some(now.saturating_add(AUTO_CLOSE_MS));
                panel.cancel_pending = false;
            }
            panel.snapshot = snapshot.clone();
            panel.observed_at_ms = observed_at;
        } else {
            if self.panels.len() >= MAX_ACTIVE {
                if let Some(old) = self
                    .panels
                    .iter()
                    .position(|p| p.snapshot.outcome.terminal())
                {
                    self.panels.remove(old);
                } else {
                    return Err(Error::NoCapacity);
                }
            }
            self.panels.push(Panel {
                auto_close_at_ms: snapshot
                    .outcome
                    .terminal()
                    .then_some(now.saturating_add(AUTO_CLOSE_MS)),
                snapshot: snapshot.clone(),
                observed_at_ms: observed_at,
                minimized: !self.on_actions,
                visible: true,
                cancel_pending: false,
                historical: false,
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
        if let Some(panel) = self.panels.iter_mut().find(|p| &p.snapshot.key == key) {
            panel.minimized = false;
            panel.manual_minimize = false;
            panel.visible = true;
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
    pub fn tick(&mut self, now: u64) {
        for panel in &mut self.panels {
            if panel.snapshot.outcome.terminal()
                && panel
                    .auto_close_at_ms
                    .is_some_and(|deadline| now >= deadline)
            {
                panel.visible = false;
            }
        }
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
        panel.auto_close_at_ms = None;
        self.result_error = None;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn run() -> Snapshot {
        serde_json::from_value(serde_json::json!({"version":1,"key":{"backend_instance":"11111111111111111111111111111111","reset_epoch":"2222222222222222","run_id":"33333333333333333333333333333333"},"revision":1,"kind":"nitrogen_test","outcome":"running","origin_client":"origin-a","title":"Nitrogen test","message":"Watching tank pressure","current_step":"raise-0","next_step":"hold-0","steps":[{"id":"raise-0","label":"Raise pressure","state":"watching","watch":[{"id":"tank-pt","label":"Tank pressure","target":50.0,"observed":42.0,"unit":"psi","age_ms":0,"valid_for_ms":2000,"evidence":"measured"}]},{"id":"hold-0","label":"Verify hold","state":"planned","watch":[]}],"observed_age_ms":0,"valid_for_ms":2000,"cancel_allowed":true})).unwrap()
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
    fn terminal_autoclose_is_not_restarted_by_polling_and_results_reopen() {
        let mut value = run();
        let mut m = Model::default();
        m.accept(0, value.clone(), 0, 0).unwrap();
        value.revision = 2;
        value.outcome = Outcome::Cancelled;
        value.cancel_allowed = false;
        value.notification_id = Some(9);
        value.notification_timestamp_ms = Some(1000);
        m.accept(0, value.clone(), 1000, 1000).unwrap();
        assert_eq!(m.notices().len(), 1);
        assert!(m.notices()[0].warning);
        m.accept(0, value.clone(), 4000, 4000).unwrap();
        assert_eq!(m.notices().len(), 1);
        m.tick(5000);
        assert!(!m.panels()[0].visible);
        m.open_result(0, value.clone(), 9000).unwrap();
        m.tick(20000);
        assert!(m.panels()[0].visible && m.panels()[0].historical);
        assert!(m.dismiss(&value.key));
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
        assert_eq!(model.needed_result_details(), vec![full.key.clone()]);
        assert!(model.needed_result_details().is_empty());
        assert!(model.cached_result(&full.key).is_none());
        model.accept(5, full.clone(), 1000, 1000).unwrap();
        assert!(model.panels()[0].snapshot.details_complete);
        assert_eq!(model.cached_result(&full.key).unwrap().steps.len(), 1);
        model.tick(4000);
        assert!(
            !model.panels()[0].visible,
            "detail arrival must not restart completion auto-close"
        );
        model
            .accept_envelope(5, envelope(summary), 4000, 4000)
            .unwrap();
        assert!(model.panels()[0].snapshot.details_complete);
        assert!(!model.panels()[0].visible);
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
}
