//! Local watch-settings store plus sync with the signed-in account
//! (`/api/account/client-settings/watch`). The account copy is the source of
//! truth; an unsynced local edit only wins when it is newer (LWW).
use super::settings::{Editor, WatchSettings};
use serde::{Deserialize, Serialize};

pub const STORAGE_KEY: &str = "watch_settings_v1";
pub const ENDPOINT: &str = "/api/account/client-settings/watch";

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct LocalRecord {
    pub settings: WatchSettings,
    /// `base|username` of the account this copy last synced with.
    pub account: Option<String>,
    /// Account revision the local copy is based on.
    pub revision: u64,
    /// Local edit not yet stored in the account.
    pub pending: bool,
}

impl LocalRecord {
    pub fn parse(raw: &str) -> Self {
        serde_json::from_str::<LocalRecord>(raw)
            .map(|mut r| {
                r.settings = r.settings.sanitize();
                r
            })
            .unwrap_or_default()
    }

    /// Applies an edit from the phone or the watch. Returns false when the
    /// edit is older than the current copy or changes nothing.
    pub fn apply_edit(&mut self, edit: WatchSettings, by: Editor, now_ms: i64) -> bool {
        let edit = edit.sanitize();
        match by {
            // Watch edits carry their own stamp; keep LWW across devices.
            Editor::Watch if !edit.newer_than(&self.settings) => return false,
            _ => {}
        }
        if edit.same_content(&self.settings) {
            if by == Editor::Watch {
                // Take the newer stamp so the echo confirms the watch's edit.
                self.settings.updated_ms = edit.updated_ms;
                return true;
            }
            return false;
        }
        let stamped = match by {
            Editor::Watch => edit,
            _ => edit.stamped(self.settings.updated_ms, now_ms, by),
        };
        self.settings = stamped;
        self.pending = true;
        true
    }
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
pub struct ServerDocument {
    pub revision: u64,
    #[serde(default)]
    pub settings: Option<serde_json::Value>,
}

#[derive(Serialize)]
pub struct ServerUpdate<'a> {
    pub expected_revision: u64,
    pub settings: &'a WatchSettings,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Plan {
    /// Replace the local copy with the account copy.
    Adopt(WatchSettings),
    /// Store the local copy in the account at this expected revision.
    Push(u64),
    /// Already identical; record the revision.
    InSync,
}

/// Decides how to reconcile the local copy with the account document.
pub fn reconcile(local: &LocalRecord, account: &str, server: &ServerDocument) -> Plan {
    let remote = server.settings.clone().and_then(WatchSettings::from_value);
    let Some(remote) = remote else {
        // Nothing (or nothing usable) stored yet: seed it from this phone.
        return Plan::Push(server.revision);
    };
    if remote.same_content(&local.settings) {
        return Plan::InSync;
    }
    // A pending edit made while signed out, or under this account, may win
    // if it is newer. Edits that belonged to another account never do.
    let edit_is_ours = local.account.is_none() || local.account.as_deref() == Some(account);
    if local.pending && edit_is_ours && local.settings.newer_than(&remote) {
        Plan::Push(server.revision)
    } else {
        let mut adopted = remote;
        adopted.updated_by = Editor::Account;
        Plan::Adopt(adopted)
    }
}

/// HTTP errors that mean "this station or session cannot sync" rather than
/// a transient failure.
pub fn is_unsupported(error: &str) -> bool {
    ["HTTP 401", "HTTP 403", "HTTP 404", "HTTP 405"]
        .iter()
        .any(|code| error.starts_with(code))
}

pub fn is_conflict(error: &str) -> bool {
    error.starts_with("HTTP 409")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::telemetry_dashboard::watch_companion::settings::Units;

    fn settings(units: Units, updated_ms: i64) -> WatchSettings {
        WatchSettings {
            units,
            updated_ms,
            ..WatchSettings::default()
        }
    }
    fn server(revision: u64, s: Option<&WatchSettings>) -> ServerDocument {
        ServerDocument {
            revision,
            settings: s.map(|s| serde_json::to_value(s).unwrap()),
        }
    }

    #[test]
    fn empty_account_is_seeded_from_the_phone() {
        let local = LocalRecord::default();
        assert_eq!(reconcile(&local, "a", &server(0, None)), Plan::Push(0));
        // Unusable documents are overwritten at their revision.
        let bad = ServerDocument {
            revision: 3,
            settings: Some(serde_json::json!({"pages": "nope"})),
        };
        assert_eq!(reconcile(&local, "a", &bad), Plan::Push(3));
    }

    #[test]
    fn account_wins_unless_a_newer_local_edit_is_pending() {
        let remote = settings(Units::Imperial, 2_000);
        let mut local = LocalRecord {
            settings: settings(Units::Metric, 1_000),
            account: Some("a".into()),
            revision: 1,
            pending: false,
        };
        assert!(
            matches!(reconcile(&local, "a", &server(2, Some(&remote))), Plan::Adopt(s) if s.units == Units::Imperial && s.updated_by == Editor::Account)
        );
        local.pending = true; // older pending edit still loses
        assert!(matches!(
            reconcile(&local, "a", &server(2, Some(&remote))),
            Plan::Adopt(_)
        ));
        local.settings.updated_ms = 3_000; // newer pending edit wins
        assert_eq!(
            reconcile(&local, "a", &server(2, Some(&remote))),
            Plan::Push(2)
        );
        // ...but never an edit that belonged to a different account.
        local.account = Some("b".into());
        assert!(matches!(
            reconcile(&local, "a", &server(2, Some(&remote))),
            Plan::Adopt(_)
        ));
        // Signed-out edits may be carried into the account.
        local.account = None;
        assert_eq!(
            reconcile(&local, "a", &server(2, Some(&remote))),
            Plan::Push(2)
        );
    }

    #[test]
    fn identical_content_is_in_sync_regardless_of_stamps() {
        let local = LocalRecord {
            settings: settings(Units::Metric, 5),
            ..LocalRecord::default()
        };
        assert_eq!(
            reconcile(&local, "a", &server(9, Some(&settings(Units::Metric, 99)))),
            Plan::InSync
        );
    }

    #[test]
    fn edits_apply_with_lww_for_watch_and_monotonic_stamps_for_phone() {
        let mut r = LocalRecord {
            settings: settings(Units::Metric, 1_000),
            ..LocalRecord::default()
        };
        assert!(
            !r.apply_edit(settings(Units::Metric, 0), Editor::Phone, 2_000),
            "no-op edit"
        );
        assert!(r.apply_edit(settings(Units::Imperial, 0), Editor::Phone, 500));
        assert_eq!(
            (r.settings.updated_ms, r.pending, r.settings.updated_by),
            (1_001, true, Editor::Phone)
        );
        let mut stale_watch = settings(Units::Metric, 900);
        stale_watch.updated_by = Editor::Watch;
        assert!(!r.apply_edit(stale_watch.clone(), Editor::Watch, 3_000));
        stale_watch.updated_ms = 4_000;
        assert!(r.apply_edit(stale_watch.clone(), Editor::Watch, 3_000));
        assert_eq!(
            (r.settings.units, r.settings.updated_ms),
            (Units::Metric, 4_000)
        );
        // A newer watch edit with identical content still advances the stamp.
        stale_watch.updated_ms = 5_000;
        assert!(r.apply_edit(stale_watch, Editor::Watch, 3_000));
        assert_eq!(r.settings.updated_ms, 5_000);
    }

    #[test]
    fn records_round_trip_and_tolerate_garbage() {
        let r = LocalRecord {
            account: Some("x|y".into()),
            revision: 4,
            pending: true,
            ..LocalRecord::default()
        };
        assert_eq!(LocalRecord::parse(&serde_json::to_string(&r).unwrap()), r);
        assert_eq!(LocalRecord::parse("{{{"), LocalRecord::default());
        assert!(is_unsupported("HTTP 404 Not Found: x") && !is_unsupported("HTTP 500"));
        assert!(is_conflict("HTTP 409 Conflict: {}"));
    }
}
