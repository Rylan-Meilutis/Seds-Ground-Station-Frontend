//! Revision belongs to the actual auth mutation owner; credentials are never logged.
use std::sync::atomic::{AtomicU64, Ordering};

pub(super) struct Revision(AtomicU64);
#[derive(Clone)]
pub(crate) struct Identity {
    revision: u64,
    token: Option<String>,
}
impl Identity {
    pub(crate) fn token(&self) -> Option<&str> {
        self.token.as_deref()
    }
}
impl Revision {
    pub(super) const fn new() -> Self {
        Self(AtomicU64::new(1))
    }
    /// Caller holds the existing CURRENT_SESSION mutation mutex.
    pub(super) fn invalidate(&self) {
        let mut old = self.0.load(Ordering::SeqCst);
        while let Some(next) = old.checked_add(1) {
            match self
                .0
                .compare_exchange(old, next, Ordering::SeqCst, Ordering::SeqCst)
            {
                Ok(_) => return,
                Err(current) => old = current,
            }
        }
    }
    /// Snapshot and validation use that same session mutex, not an observer.
    pub(super) fn capture(&self, token: Option<String>) -> Option<Identity> {
        let revision = self.0.load(Ordering::SeqCst);
        (revision != u64::MAX).then_some(Identity { revision, token })
    }
    pub(super) fn matches(&self, identity: &Identity, token: Option<&str>) -> bool {
        identity.revision != u64::MAX
            && self.0.load(Ordering::SeqCst) == identity.revision
            && identity.token.as_deref() == token
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn credential_aba_retires_original_identity() {
        let revision = Revision::new();
        let first = revision.capture(Some("fixture-a".into())).unwrap();
        assert!(revision.matches(&first, Some("fixture-a")));
        revision.invalidate();
        let other = revision.capture(Some("fixture-b".into())).unwrap();
        revision.invalidate();
        assert!(!revision.matches(&first, Some("fixture-a")));
        assert!(!revision.matches(&other, Some("fixture-b")));
        let current = revision.capture(Some("fixture-a".into())).unwrap();
        assert_eq!(current.token(), Some("fixture-a"));
        assert!(revision.matches(&current, Some("fixture-a")));
    }
    #[test]
    fn anonymous_and_exhausted_identities_fail_closed() {
        let revision = Revision::new();
        let first = revision.capture(None).unwrap();
        assert!(revision.matches(&first, None));
        revision.invalidate();
        assert!(!revision.matches(&first, None));
        revision.0.store(u64::MAX, Ordering::SeqCst);
        revision.invalidate();
        assert!(revision.capture(None).is_none());
        assert!(!revision.matches(&first, None));
    }
}
