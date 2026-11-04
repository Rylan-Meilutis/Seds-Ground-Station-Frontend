//! Client confirmation projection. Backend permissions/interlocks remain authoritative.
const FRESH_MS: f64 = 1_500.0;
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct Ticket {
    pub serial: u64,
    pub source: u64,
    pub desired: bool,
}
#[derive(Clone, Debug, PartialEq)]
pub(super) enum Notice {
    Empty,
    Pending,
    Accepted,
    Locked,
    Consumed,
    Unavailable,
    OwnerChanged,
    Exhausted,
    Failed(String),
}
impl Notice {
    pub fn text(&self) -> &str {
        match self {
            Self::Empty => "",
            Self::Pending => "Saving isolation confirmation…",
            Self::Accepted => "Self-test confirmation accepted. Backend interlocks still apply.",
            Self::Locked => "Self-test locked.",
            Self::Consumed => {
                "Confirmation consumed by the last run. Confirm isolation again before retrying self-test."
            }
            Self::Unavailable => {
                "Current isolation confirmation is unavailable. Confirm isolation again before self-test."
            }
            Self::OwnerChanged => {
                "Station, connection or signed-in account changed. Confirm isolation again before self-test."
            }
            Self::Exhausted => {
                "Confirmation request limit reached. Reopen Actions before confirming isolation."
            }
            Self::Failed(error) => error,
        }
    }
}
#[derive(Clone, Debug, PartialEq)]
pub(super) struct Model {
    serial: u64,
    source: u64,
    pending: Option<Ticket>,
    confirmed: Option<bool>,
    floor: Option<f64>,
    last_observation: Option<f64>,
    evidence: Option<f64>,
    pub notice: Notice,
}
impl Model {
    pub fn new(source: u64) -> Self {
        Self {
            serial: 0,
            source,
            pending: None,
            confirmed: None,
            floor: None,
            last_observation: None,
            evidence: None,
            notice: Notice::Empty,
        }
    }
    pub fn confirmed(&self) -> bool {
        self.confirmed == Some(true)
    }
    pub fn busy(&self) -> bool {
        self.pending.is_some()
    }
    pub fn exhausted(&self) -> bool {
        self.serial == u64::MAX
    }
    pub fn owns(&self, ticket: Ticket) -> bool {
        self.serial == ticket.serial && self.source == ticket.source
    }
    pub fn retire(&mut self, source: u64, now: f64) {
        self.source = source;
        self.pending = None;
        self.confirmed = None;
        self.evidence = None;
        self.floor = now.is_finite().then_some(now);
        self.last_observation = None;
        self.notice = Notice::OwnerChanged;
    }
    pub fn begin(&mut self, desired: bool, source: u64, now: f64) -> Option<Ticket> {
        if self.busy() {
            return None;
        }
        if self.source != source {
            self.retire(source, now);
        }
        let Some(serial) = self.serial.checked_add(1).filter(|v| *v != u64::MAX) else {
            self.serial = u64::MAX;
            self.confirmed = None;
            self.evidence = None;
            self.notice = Notice::Exhausted;
            return None;
        };
        self.serial = serial;
        let ticket = Ticket {
            serial,
            source,
            desired,
        };
        self.pending = Some(ticket);
        self.confirmed = None;
        self.evidence = None;
        self.notice = Notice::Pending;
        Some(ticket)
    }
    pub fn finish(&mut self, ticket: Ticket, result: Result<(), String>, now: f64) -> bool {
        if self.pending != Some(ticket) || !self.owns(ticket) {
            return false;
        }
        self.pending = None;
        if !now.is_finite() || self.floor.is_some_and(|floor| now < floor) {
            self.confirmed = None;
            self.evidence = None;
            self.notice = Notice::Unavailable;
            return true;
        }
        self.floor = Some(now);
        self.evidence = Some(now);
        match result {
            Ok(()) => {
                self.confirmed = Some(ticket.desired);
                self.notice = if ticket.desired {
                    Notice::Accepted
                } else {
                    Notice::Locked
                };
            }
            Err(error) => {
                self.confirmed = None;
                self.evidence = None;
                self.notice = Notice::Failed(error);
            }
        }
        true
    }
    pub fn observe(
        &mut self,
        current_source: u64,
        source: u64,
        requested: f64,
        now: f64,
        value: Option<bool>,
    ) {
        if self.source != current_source {
            self.retire(current_source, now);
        }
        if self.busy() {
            return;
        }
        if self
            .evidence
            .is_some_and(|at| !now.is_finite() || now < at || now - at > FRESH_MS)
        {
            self.confirmed = None;
            self.evidence = None;
            self.notice = Notice::Unavailable;
        }
        if self.exhausted() {
            return;
        }
        if source != current_source || !requested.is_finite() || !now.is_finite() || requested > now
        {
            self.confirmed = None;
            self.evidence = None;
            if !self.exhausted() {
                self.notice = Notice::Unavailable;
            }
            return;
        }
        // A retained observation is not a new read when a request stops being busy.
        // Equal times cannot establish that a GET started after this completion.
        if self.floor.is_some_and(|floor| requested <= floor)
            || self.last_observation.is_some_and(|last| requested <= last)
        {
            return;
        }
        if now - requested > FRESH_MS || value.is_none() {
            self.confirmed = None;
            self.evidence = None;
            if !self.exhausted() {
                self.notice = Notice::Unavailable;
            }
            return;
        }
        self.last_observation = Some(requested);
        self.evidence = Some(requested);
        let next = value.unwrap();
        if self.confirmed == Some(true) && !next {
            self.notice = Notice::Consumed;
        } else if next && self.notice != Notice::Accepted {
            self.notice = Notice::Empty;
        } else if !next && !matches!(self.notice, Notice::Locked | Notice::Consumed) {
            self.notice = Notice::Locked;
        }
        self.confirmed = Some(next);
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn accepted_request_rejects_cached_and_held_old_false_then_fresh_false_consumes() {
        let mut m = Model::new(4);
        m.observe(4, 4, 10., 11., Some(false));
        let t = m.begin(true, 4, 12.).unwrap();
        assert!(m.finish(t, Ok(()), 20.));
        m.observe(4, 4, 10., 21., Some(false));
        assert!(m.confirmed());
        assert_eq!(m.notice, Notice::Accepted);
        m.observe(4, 4, 19., 22., Some(false));
        assert!(m.confirmed());
        m.observe(4, 4, 20., 23., Some(false));
        assert!(m.confirmed());
        m.observe(4, 4, 21., 24., Some(false));
        assert!(!m.confirmed());
        assert_eq!(m.notice, Notice::Consumed);
    }
    #[test]
    fn duplicate_ticks_do_not_refresh_confirmation_or_consume_post() {
        let mut m = Model::new(1);
        let t = m.begin(true, 1, 1.).unwrap();
        m.finish(t, Ok(()), 2.);
        for now in [3., 4., 5.] {
            m.observe(1, 1, 1., now, Some(false));
            assert!(m.confirmed());
        }
        m.observe(1, 1, 1., 1503., Some(false));
        assert!(!m.confirmed());
        assert_eq!(m.notice, Notice::Unavailable);
    }
    #[test]
    fn busy_and_serial_keep_original_request_one_shot() {
        let mut m = Model::new(1);
        let a = m.begin(true, 1, 1.).unwrap();
        assert!(m.begin(false, 1, 2.).is_none());
        m.observe(1, 1, 3., 4., Some(true));
        assert!(!m.confirmed());
        assert!(m.finish(a, Ok(()), 5.));
        let b = m.begin(false, 1, 6.).unwrap();
        assert!(!m.finish(a, Ok(()), 7.));
        assert!(m.finish(b, Ok(()), 8.));
        assert!(!m.confirmed());
        assert_eq!(m.notice, Notice::Locked);
    }
    #[test]
    fn invalid_metadata_and_owner_retirement_never_reconfirm_cached_value() {
        for requested in [f64::NAN, f64::INFINITY, 12., -2000.] {
            let mut m = Model::new(1);
            m.observe(1, 1, requested, 10., Some(true));
            assert!(!m.confirmed());
        }
        let mut m = Model::new(1);
        m.observe(1, 1, 1., 2., Some(true));
        m.retire(1, 3.);
        m.observe(1, 1, 2., 4., Some(true));
        assert!(!m.confirmed());
        m.observe(1, 1, 4., 5., Some(true));
        assert!(m.confirmed());
        m.observe(2, 1, 5., 6., Some(true));
        assert!(!m.confirmed());
    }
    #[test]
    fn failed_and_cross_source_completions_never_apply() {
        let mut m = Model::new(1);
        let t = m.begin(true, 1, 1.).unwrap();
        m.retire(2, 2.);
        assert!(!m.finish(t, Ok(()), 3.));
        let t = m.begin(true, 2, 4.).unwrap();
        m.finish(t, Err("rejected".into()), 5.);
        m.observe(2, 2, 4., 6., Some(true));
        assert!(!m.confirmed());
        m.observe(2, 2, 6., 7., Some(false));
        assert!(!m.confirmed());
    }
    #[test]
    fn exhausted_serial_does_not_wrap_or_send() {
        let mut m = Model::new(1);
        m.serial = u64::MAX - 1;
        assert!(m.begin(true, 1, 1.).is_none());
        assert!(m.exhausted());
        assert!(!m.confirmed());
        assert_eq!(m.notice, Notice::Exhausted);
    }
}
