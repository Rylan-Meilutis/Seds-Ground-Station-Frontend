//! Explicit condition state is independent of historical event acknowledgments.
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, BTreeSet},
    sync::Arc,
};
const MAX_RECORDS: usize = 1018;
#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Severity {
    Warning,
    Error,
}
#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ConditionState {
    Active,
    Resolved,
    Unknown,
}
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct Record {
    pub key: String,
    pub occurrence_id: String,
    pub authority: String,
    pub severity: Severity,
    pub source: String,
    pub message: String,
    pub state: ConditionState,
    pub raised_at_ms: i64,
    pub updated_at_ms: i64,
    pub resolved_at_ms: Option<i64>,
}
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct KnownCondition {
    pub key: String,
    pub source: String,
}
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq, Hash)]
pub struct ConditionLink {
    pub condition_occurrence_id: String,
    pub condition_authority: String,
    pub condition_key: String,
    pub condition_source: String,
    pub condition_snapshot_authority: String,
    pub condition_revision: u64,
}
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct Snapshot {
    pub version: u32,
    pub authority: String,
    pub revision: u64,
    pub complete: bool,
    pub evaluated: Vec<KnownCondition>,
    pub awaiting_evidence: Vec<String>,
    pub persistence_error: Option<String>,
    pub records: Vec<Record>,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RowTone {
    Warning,
    Error,
    Unknown,
    Resolved,
}
impl Record {
    /// No acknowledgment parameter: reviewing an active condition cannot resolve it.
    pub fn tone(&self) -> RowTone {
        match self.state {
            ConditionState::Active => match self.severity {
                Severity::Warning => RowTone::Warning,
                Severity::Error => RowTone::Error,
            },
            ConditionState::Unknown => RowTone::Unknown,
            ConditionState::Resolved => RowTone::Resolved,
        }
    }
    pub fn state_label(&self) -> &'static str {
        match self.state {
            ConditionState::Active => "Active",
            ConditionState::Resolved => "Resolved",
            ConditionState::Unknown => "State unknown",
        }
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Ingest {
    Accepted,
    IgnoredStale,
    Invalid,
}
#[derive(Clone, Default)]
pub struct View {
    connection: u64,
    connected: bool,
    snapshot: Option<Arc<Snapshot>>,
    by_occurrence: Arc<BTreeMap<String, usize>>,
    evaluated: Arc<BTreeMap<String, String>>,
    retired: BTreeSet<String>,
    invalid: bool,
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Summary {
    pub active_errors: usize,
    pub active_warnings: usize,
    pub unknown: usize,
    pub unknown_errors: usize,
    pub unknown_warnings: usize,
    pub snapshot_current: bool,
    pub nominal: bool,
}
impl View {
    /// A new station/transport scope invalidates the old authority immediately.
    /// Pass this token with async HTTP/WS results to fence stale completions.
    pub fn begin_connection(&mut self) -> u64 {
        self.connection = self
            .connection
            .checked_add(1)
            .expect("condition connection identity exhausted");
        self.connected = true;
        self.snapshot = None;
        self.by_occurrence = Arc::new(BTreeMap::new());
        self.evaluated = Arc::new(BTreeMap::new());
        self.retired.clear();
        self.invalid = false;
        self.connection
    }
    /// Retire current-domain facts without inventing a new transport token.
    /// The preserved socket can deliver the next matching-epoch snapshot.
    pub fn retire_applied_context(&mut self) {
        self.snapshot = None;
        self.by_occurrence = Arc::new(BTreeMap::new());
        self.evaluated = Arc::new(BTreeMap::new());
        self.retired.clear();
        self.invalid = true;
    }
    pub fn disconnect(&mut self, connection: u64) {
        if connection == self.connection {
            self.connected = false;
        }
    }
    pub fn invalidate(&mut self, connection: u64) {
        if connection == self.connection && self.connected {
            self.invalid = true;
        }
    }
    pub fn snapshot(&self) -> Option<&Snapshot> {
        self.snapshot.as_deref()
    }
    /// Preserve the existing event-only fallback until this transport has
    /// actually supplied typed history. Known incomplete/invalid history is
    /// uncertainty, never proof that a severity has no recorded conditions.
    pub fn may_show_empty_history(&self, severity: Severity) -> bool {
        match self.snapshot() {
            None => !self.invalid,
            Some(snapshot) => self.summary(false, false).snapshot_current
                && !snapshot.records.iter().any(|record| record.severity == severity),
        }
    }
    pub fn typed_history_unconfirmed(&self) -> bool {
        (self.snapshot.is_some() || self.invalid)
            && !self.summary(false, false).snapshot_current
    }
    pub fn ingest(&mut self, connection: u64, next: Snapshot) -> Ingest {
        if connection != self.connection
            || !self.connected
            || self.retired.contains(&next.authority)
        {
            return Ingest::IgnoredStale;
        }
        if !valid(&next) {
            self.invalid = true;
            return Ingest::Invalid;
        }
        if let Some(current) = &self.snapshot {
            if next.authority == current.authority {
                if next.revision < current.revision {
                    return Ingest::IgnoredStale;
                }
                if next.revision == current.revision && next != **current {
                    self.invalid = true;
                    return Ingest::Invalid;
                }
            } else {
                // A transport is not expected to see many backend restarts.
                // Fail closed instead of forgetting an old authority and
                // accepting a delayed stale snapshot after unbounded churn.
                if self.retired.len() >= 8 {
                    self.invalid = true;
                    return Ingest::Invalid;
                }
                self.retired.insert(current.authority.clone());
            }
        }
        self.by_occurrence = Arc::new(
            next.records
                .iter()
                .enumerate()
                .map(|(index, record)| (record.occurrence_id.clone(), index))
                .collect(),
        );
        self.evaluated = Arc::new(
            next.evaluated
                .iter()
                .map(|known| (known.key.clone(), known.source.clone()))
                .collect(),
        );
        self.snapshot = Some(Arc::new(next));
        self.invalid = false;
        Ingest::Accepted
    }
    pub fn event_projection(
        &self,
        link: Option<&ConditionLink>,
        acknowledged: bool,
    ) -> EventProjection {
        let Some(link) = link else {
            return EventProjection::Legacy {
                attention: !acknowledged,
            };
        };
        if let Some(snapshot) = &self.snapshot {
            if let Some(record) = self
                .by_occurrence
                .get(&link.condition_occurrence_id)
                .and_then(|index| snapshot.records.get(*index))
            {
                if record.authority == link.condition_authority
                    && record.key == link.condition_key
                    && record.source == link.condition_source
                {
                    return EventProjection::Represented;
                }
                return EventProjection::Unverified {
                    attention: true,
                    unknown: true,
                };
            }
            let current = self.connected
                && !self.invalid
                && snapshot.complete
                && snapshot.authority == link.condition_snapshot_authority
                && snapshot.revision >= link.condition_revision;
            let source_known = self
                .evaluated
                .get(&link.condition_key)
                .is_some_and(|source| source == &link.condition_source);
            let identity_bound = !link.condition_authority.is_empty()
                && link
                    .condition_occurrence_id
                    .starts_with(&format!("{}:", link.condition_authority));
            return EventProjection::MissingHistory {
                attention: !current || !source_known || !identity_bound,
                unknown: !current || !source_known || !identity_bound,
            };
        }
        EventProjection::MissingHistory {
            attention: true,
            unknown: true,
        }
    }
    pub fn summary(&self, board_abort_active: bool, unreviewed_events: bool) -> Summary {
        let mut result = Summary::default();
        if let Some(snapshot) = &self.snapshot {
            result.snapshot_current = self.connected && !self.invalid && snapshot.complete;
            for record in &snapshot.records {
                match record.state {
                    ConditionState::Active => match record.severity {
                        Severity::Warning => result.active_warnings += 1,
                        Severity::Error => result.active_errors += 1,
                    },
                    ConditionState::Unknown => {
                        result.unknown += 1;
                        match record.severity {
                            Severity::Error => result.unknown_errors += 1,
                            Severity::Warning => result.unknown_warnings += 1,
                        }
                    }
                    ConditionState::Resolved => {}
                }
            }
            for key in &snapshot.awaiting_evidence {
                if !snapshot
                    .records
                    .iter()
                    .any(|record| record.key == *key && record.state == ConditionState::Unknown)
                {
                    result.unknown += 1;
                    result.unknown_warnings += 1;
                }
            }
        }
        result.nominal = result.snapshot_current
            && result.active_errors == 0
            && result.active_warnings == 0
            && result.unknown == 0
            && !board_abort_active
            && !unreviewed_events;
        result
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EventProjection {
    Legacy {
        attention: bool,
    },
    Represented,
    /// Show as a historical condition event with details unavailable, NEVER Resolved.
    MissingHistory {
        attention: bool,
        unknown: bool,
    },
    Unverified {
        attention: bool,
        unknown: bool,
    },
}
impl EventProjection {
    pub fn history_gray(self, acknowledged: bool) -> bool {
        match self {
            Self::Legacy { .. } => acknowledged,
            Self::MissingHistory { unknown, .. } | Self::Unverified { unknown, .. } => !unknown,
            Self::Represented => false,
        }
    }
    pub fn event_label(self, acknowledged: bool) -> &'static str {
        match self {
            Self::Legacy { .. } => {
                if acknowledged {
                    "Event · Acknowledged"
                } else {
                    "Event"
                }
            }
            Self::MissingHistory { unknown: false, .. } => {
                "Historical condition event · details unavailable"
            }
            Self::MissingHistory { unknown: true, .. } | Self::Unverified { .. } => {
                "Condition event · state unconfirmed"
            }
            Self::Represented => "Condition event",
        }
    }
    pub fn attention(self) -> bool {
        match self {
            Self::Represented => false,
            Self::Legacy { attention }
            | Self::MissingHistory { attention, .. }
            | Self::Unverified { attention, .. } => attention,
        }
    }
    pub fn unknown(self) -> bool {
        match self {
            Self::MissingHistory { unknown, .. } | Self::Unverified { unknown, .. } => unknown,
            _ => false,
        }
    }
}
fn valid(snapshot: &Snapshot) -> bool {
    if snapshot.version != 1
        || snapshot.authority.is_empty()
        || snapshot.authority.len() > 128
        || snapshot.records.len() > MAX_RECORDS
        || snapshot.awaiting_evidence.len() > 18
        || snapshot.evaluated.len() > 18
        || (snapshot.complete
            && (!snapshot.awaiting_evidence.is_empty() || snapshot.persistence_error.is_some()))
    {
        return false;
    }
    let mut evaluated = BTreeSet::new();
    if snapshot.evaluated.iter().any(|known| {
        known.key.is_empty()
            || known.key.len() > 128
            || known.source.is_empty()
            || known.source.len() > 128
            || !evaluated.insert(&known.key)
    }) {
        return false;
    }
    let mut ids = BTreeSet::new();
    let mut active_keys = BTreeSet::new();
    for record in &snapshot.records {
        if record.key.is_empty()
            || record.key.len() > 128
            || record.occurrence_id.is_empty()
            || record.occurrence_id.len() > 160
            || record.authority.is_empty()
            || record.authority.len() > 128
            || !record
                .occurrence_id
                .starts_with(&format!("{}:", record.authority))
            || record.message.len() > 4096
            || record.source.len() > 128
            || !ids.insert(&record.occurrence_id)
            || (record.state == ConditionState::Resolved) != record.resolved_at_ms.is_some()
            || (record.state != ConditionState::Resolved && !active_keys.insert(&record.key))
        {
            return false;
        }
    }
    true
}

/// History rows newest first, so the latest event is at the top of the list.
pub(crate) fn newest_first<'a>(
    events: impl Iterator<Item = &'a super::AlertMsg>,
) -> Vec<super::AlertMsg> {
    let mut events = events.cloned().collect::<Vec<_>>();
    events.sort_by_key(|event| std::cmp::Reverse(event.timestamp_ms));
    events
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn history_rows_list_the_newest_first() {
        let alert = |timestamp_ms| super::super::AlertMsg {
            timestamp_ms,
            message: String::new(),
            condition: None,
        };
        let events = [alert(10), alert(30), alert(20)];
        let order = newest_first(events.iter())
            .iter()
            .map(|event| event.timestamp_ms)
            .collect::<Vec<_>>();
        assert_eq!(order, [30, 20, 10]);
    }
    fn record(state: ConditionState) -> Record {
        Record {
            key: "board_offline:DAQ".into(),
            occurrence_id: "epoch:1".into(),
            authority: "epoch".into(),
            severity: Severity::Error,
            source: "DAQ".into(),
            message: "Board offline".into(),
            state,
            raised_at_ms: 1,
            updated_at_ms: 2,
            resolved_at_ms: (state == ConditionState::Resolved).then_some(2),
        }
    }
    fn snapshot(state: Option<ConditionState>) -> Snapshot {
        Snapshot {
            version: 1,
            authority: "epoch".into(),
            revision: 1,
            complete: true,
            evaluated: Vec::new(),
            awaiting_evidence: Vec::new(),
            persistence_error: None,
            records: state.map(record).into_iter().collect(),
        }
    }
    #[test]
    fn acknowledgments_cannot_dim_active_conditions_or_hide_abort_and_resolved_history_stays_separate()
     {
        let mut view = View::default();
        let token = view.begin_connection();
        view.ingest(token, snapshot(Some(ConditionState::Active)));
        assert_eq!(view.snapshot().unwrap().records[0].tone(), RowTone::Error);
        assert!(
            !view.summary(false, false).nominal,
            "acknowledged events cannot clear an active condition"
        );
        let mut resolved = snapshot(Some(ConditionState::Resolved));
        resolved.revision = 2;
        view.ingest(token, resolved);
        assert_eq!(
            view.snapshot().unwrap().records[0].tone(),
            RowTone::Resolved
        );
        assert_eq!(
            view.snapshot().unwrap().records[0].state_label(),
            "Resolved"
        );
        assert!(view.summary(false, false).nominal);
        assert!(!view.summary(true, false).nominal);
        assert!(!view.summary(false, true).nominal);
    }
    #[test]
    fn stale_authority_incomplete_unknown_and_disconnected_snapshots_never_claim_nominal() {
        let mut view = View::default();
        assert!(!view.summary(false, false).nominal);
        let first = view.begin_connection();
        view.ingest(first, snapshot(None));
        assert!(view.summary(false, false).nominal);
        let second = view.begin_connection();
        assert_eq!(view.ingest(first, snapshot(None)), Ingest::IgnoredStale);
        assert!(!view.summary(false, false).nominal);
        let mut incomplete = snapshot(None);
        incomplete.complete = false;
        incomplete.persistence_error = Some("Journal failed".into());
        view.ingest(second, incomplete);
        assert!(!view.summary(false, false).nominal);
        let mut unknown = snapshot(Some(ConditionState::Unknown));
        unknown.revision = 2;
        view.ingest(second, unknown);
        assert_eq!(view.summary(false, false).unknown, 1);
        assert!(!view.summary(false, false).nominal);
        view.disconnect(second);
        assert!(!view.summary(false, false).snapshot_current);
    }
    #[test]
    fn recurrence_and_authority_replay_cannot_be_hidden_by_historical_state() {
        let mut view = View::default();
        let token = view.begin_connection();
        view.ingest(token, snapshot(Some(ConditionState::Resolved)));
        let mut recurring = snapshot(Some(ConditionState::Active));
        recurring.revision = 2;
        recurring.records[0].occurrence_id = "epoch:2".into();
        view.ingest(token, recurring);
        assert_eq!(view.summary(false, false).active_errors, 1);
        assert_eq!(
            view.ingest(token, snapshot(Some(ConditionState::Resolved))),
            Ingest::IgnoredStale
        );
        assert_eq!(view.summary(false, false).active_errors, 1);
        let mut next = snapshot(None);
        next.authority = "new-epoch".into();
        next.complete = false;
        view.ingest(token, next);
        assert_eq!(view.ingest(token, snapshot(None)), Ingest::IgnoredStale);
        assert!(!view.summary(false, false).nominal);
    }
    #[test]
    fn empty_history_requires_complete_current_typed_evidence_but_preserves_legacy_fallback() {
        let mut view = View::default();
        assert!(view.may_show_empty_history(Severity::Error));
        assert!(view.may_show_empty_history(Severity::Warning));
        let token = view.begin_connection();
        assert!(view.may_show_empty_history(Severity::Error));
        view.invalidate(token);
        assert!(view.typed_history_unconfirmed());
        assert!(!view.may_show_empty_history(Severity::Error));
        assert!(!view.may_show_empty_history(Severity::Warning));
        let mut pending = snapshot(None);
        pending.complete = false;
        view.ingest(token, pending);
        assert!(view.typed_history_unconfirmed());
        assert!(!view.may_show_empty_history(Severity::Error));
        assert!(!view.may_show_empty_history(Severity::Warning));
        let mut empty = snapshot(None);
        empty.revision = 2;
        view.ingest(token, empty);
        assert!(view.may_show_empty_history(Severity::Error));
        assert!(view.may_show_empty_history(Severity::Warning));
        for (index, state) in [ConditionState::Active, ConditionState::Unknown, ConditionState::Resolved].into_iter().enumerate() {
            let mut row = snapshot(Some(state));
            row.revision = 3 + index as u64;
            view.ingest(token, row);
            assert!(!view.may_show_empty_history(Severity::Error));
            assert!(view.may_show_empty_history(Severity::Warning));
        }
        view.disconnect(token);
        assert!(view.typed_history_unconfirmed());
        assert!(!view.may_show_empty_history(Severity::Warning));
    }
    #[test]
    fn malformed_same_revision_and_conflicting_active_keys_fail_closed() {
        let mut view = View::default();
        let token = view.begin_connection();
        view.ingest(token, snapshot(None));
        assert_eq!(
            view.ingest(token, snapshot(Some(ConditionState::Active))),
            Ingest::Invalid
        );
        assert!(!view.summary(false, false).nominal);
        let mut duplicate = snapshot(Some(ConditionState::Active));
        duplicate.revision = 2;
        let mut other = duplicate.records[0].clone();
        other.occurrence_id = "epoch:3".into();
        duplicate.records.push(other);
        assert_eq!(view.ingest(token, duplicate), Ingest::Invalid);
        assert!(!view.summary(false, false).nominal);
    }
}
