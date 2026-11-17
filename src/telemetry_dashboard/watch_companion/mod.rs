//! Read-only Apple Watch / Wear OS companion: settings, account sync and the
//! phone-side publisher. See docs/watch-companion.md.
pub(crate) mod account_sync;
pub(crate) mod bridge;
// Reference implementation for the watch ports; the phone only formats ages.
#[cfg_attr(not(test), allow(dead_code))]
pub(crate) mod geodesy;
pub(crate) mod protocol;
mod publisher;
pub(crate) mod registry;
pub(crate) mod settings;
mod settings_section;
pub(crate) mod snapshot;

#[cfg(not(target_arch = "wasm32"))]
pub(super) use publisher::use_demo_publisher;
pub(super) use publisher::use_live_publisher;
pub(super) use settings_section::WatchSettingsSection;

use account_sync::LocalRecord;
use dioxus::prelude::*;
use settings::{Editor, WatchSettings};
use std::sync::atomic::{AtomicBool, Ordering};

/// The single local copy of the watch settings and its sync metadata.
static WATCH_RECORD: GlobalSignal<LocalRecord> = Signal::global(|| {
    LocalRecord::parse(&super::persist::get_string(account_sync::STORAGE_KEY).unwrap_or_default())
});

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum SyncState {
    LocalOnly,
    Pending,
    Synced,
    Error,
}

impl SyncState {
    pub fn wire(self) -> &'static str {
        match self {
            SyncState::LocalOnly => "local_only",
            SyncState::Pending => "pending",
            SyncState::Synced => "synced",
            SyncState::Error => "error",
        }
    }
    pub fn label(self) -> &'static str {
        match self {
            SyncState::LocalOnly => "Saved on this device; not synced to your account",
            SyncState::Pending => "Saving to your account…",
            SyncState::Synced => "Synced to your account",
            SyncState::Error => "Account sync failed; will retry",
        }
    }
}

static WATCH_SYNC: GlobalSignal<SyncState> = Signal::global(|| SyncState::LocalOnly);
static SYNC_REQUESTED: AtomicBool = AtomicBool::new(true);
static PUBLISH_REQUESTED: AtomicBool = AtomicBool::new(true);

pub(crate) fn current_settings() -> WatchSettings {
    WATCH_RECORD.peek().settings.clone()
}

fn store_record(record: LocalRecord) {
    if let Ok(raw) = serde_json::to_string(&record) {
        super::persist::set_string(account_sync::STORAGE_KEY, &raw);
    }
    *WATCH_RECORD.write() = record;
}

/// Applies an edit made on the phone or received from the watch.
fn apply_edit(edit: WatchSettings, by: Editor) -> bool {
    let mut record = WATCH_RECORD.peek().clone();
    if !record.apply_edit(edit, by, super::current_wallclock_ms()) {
        return false;
    }
    store_record(record);
    if *WATCH_SYNC.peek() != SyncState::LocalOnly {
        *WATCH_SYNC.write() = SyncState::Pending;
    }
    SYNC_REQUESTED.store(true, Ordering::Release);
    PUBLISH_REQUESTED.store(true, Ordering::Release);
    true
}

/// Phone-side edit helper for the Settings section.
pub(crate) fn edit_settings(f: impl FnOnce(&mut WatchSettings)) {
    if !watch_settings_available() {
        return;
    }
    let mut next = current_settings();
    f(&mut next);
    apply_edit(next, Editor::Phone);
}

/// The username of a signed-in, non-anonymous session.
fn signed_in_user(session: Option<&crate::auth::StoredAuthSession>) -> Option<&str> {
    let session = session?;
    if session.session.anonymous {
        return None;
    }
    session.session.username.as_deref().filter(|u| !u.is_empty())
}

/// `base|username` for a signed-in, non-anonymous session.
fn account_key() -> Option<String> {
    let session = crate::auth::current_session();
    let user = signed_in_user(session.as_ref())?;
    Some(format!("{}|{}", super::UrlConfig::base_http(), user))
}

/// This build is a native phone app that ships a watch companion bridge
/// (iOS WatchConnectivity, Android Wear Data Layer). False for the web/wasm
/// frontend and for desktop builds.
pub(crate) const COMPANION_PLATFORM: bool = cfg!(any(target_os = "ios", target_os = "android"));

/// Watch settings exist only for a signed-in user of a native phone app whose
/// watch has the companion app: a paired watch with the GS app installed.
/// Everyone else gets no Watch settings UI, no account sync of the watch
/// document and the default settings in snapshots.
pub(crate) fn settings_eligible(
    companion_platform: bool,
    signed_in: bool,
    link: bridge::LinkState,
) -> bool {
    companion_platform && signed_in && link.supported && link.paired && link.installed
}

/// Current eligibility. Skips the native bridge query where there is none.
pub(crate) fn watch_settings_available() -> bool {
    COMPANION_PLATFORM
        && settings_eligible(
            COMPANION_PLATFORM,
            account_key().is_some(),
            bridge::link_state(),
        )
}

/// The settings to send to the watch: the user's own for an eligible user,
/// the defaults for everyone else (persisted settings stay on the device but
/// are neither sent nor applied).
pub(crate) fn effective_settings(eligible: bool) -> WatchSettings {
    if eligible {
        current_settings()
    } else {
        WatchSettings::default()
    }
}

/// Live eligibility for the Settings screen. Polls the bridge and session on
/// companion platforms only; elsewhere it is constantly false.
pub(super) fn use_watch_settings_available() -> Signal<bool> {
    let mut available = use_signal(watch_settings_available);
    use_future(move || async move {
        if !COMPANION_PLATFORM {
            return;
        }
        loop {
            sleep_ms(2_000).await;
            let next = watch_settings_available();
            if *available.peek() != next {
                available.set(next);
            }
        }
    });
    available
}

/// One reconciliation round with the account. Never blocks the UI thread
/// for long: at most three GET/POST pairs.
async fn sync_once() {
    use account_sync::*;
    // Only an eligible user's watch document is read from or pushed to the
    // account (never from the web frontend or without the companion app).
    let account = account_key().filter(|_| watch_settings_available());
    let Some(account) = account else {
        *WATCH_SYNC.write() = SyncState::LocalOnly;
        return;
    };
    for _ in 0..3 {
        let server = match super::http_get_json::<ServerDocument>(ENDPOINT).await {
            Ok(doc) => doc,
            Err(error) => {
                *WATCH_SYNC.write() = if is_unsupported(&error) {
                    SyncState::LocalOnly
                } else {
                    SyncState::Error
                };
                return;
            }
        };
        if account_key().as_deref() != Some(account.as_str()) {
            return; // Signed out or switched while waiting.
        }
        let local = WATCH_RECORD.peek().clone();
        let mut record = local.clone();
        match reconcile(&local, &account, &server) {
            Plan::InSync => {}
            Plan::Adopt(settings) => {
                record.settings = settings;
                PUBLISH_REQUESTED.store(true, Ordering::Release);
            }
            Plan::Push(expected) => {
                let update = ServerUpdate {
                    expected_revision: expected,
                    settings: &local.settings,
                };
                match super::http_post_json::<_, ServerDocument>(ENDPOINT, &update).await {
                    Ok(doc) => {
                        let current = WATCH_RECORD.peek().clone();
                        record = current.clone();
                        record.revision = doc.revision;
                        record.account = Some(account);
                        // An edit made during the request stays pending.
                        record.pending = !current.settings.same_content(&local.settings);
                        let state = if record.pending {
                            SyncState::Pending
                        } else {
                            SyncState::Synced
                        };
                        store_record(record);
                        *WATCH_SYNC.write() = state;
                        return;
                    }
                    Err(error) if is_conflict(&error) => continue,
                    Err(error) => {
                        *WATCH_SYNC.write() = if is_unsupported(&error) {
                            SyncState::LocalOnly
                        } else {
                            SyncState::Error
                        };
                        return;
                    }
                }
            }
        }
        // Adopt / InSync: only if nothing was edited meanwhile.
        if WATCH_RECORD.peek().settings != local.settings {
            SYNC_REQUESTED.store(true, Ordering::Release);
            return;
        }
        record.revision = server.revision;
        record.account = Some(account);
        record.pending = false;
        store_record(record);
        *WATCH_SYNC.write() = SyncState::Synced;
        return;
    }
    *WATCH_SYNC.write() = SyncState::Error;
}

async fn sleep_ms(ms: u64) {
    #[cfg(target_arch = "wasm32")]
    gloo_timers::future::TimeoutFuture::new(ms as u32).await;
    #[cfg(not(target_arch = "wasm32"))]
    tokio::time::sleep(std::time::Duration::from_millis(ms)).await;
}

#[cfg(test)]
mod eligibility_tests {
    use super::bridge::LinkState;
    use super::*;
    use crate::auth::{SessionStatus, StoredAuthSession};

    fn session(username: Option<&str>, anonymous: bool) -> StoredAuthSession {
        StoredAuthSession {
            token: "t".into(),
            session: SessionStatus {
                authenticated: true,
                anonymous,
                username: username.map(str::to_owned),
                ..SessionStatus::default()
            },
            remember_me: false,
        }
    }

    #[test]
    fn signed_in_means_a_named_non_anonymous_session() {
        assert_eq!(signed_in_user(None), None);
        assert_eq!(signed_in_user(Some(&session(Some("ada"), false))), Some("ada"));
        assert_eq!(signed_in_user(Some(&session(Some("ada"), true))), None);
        assert_eq!(signed_in_user(Some(&session(Some(""), false))), None);
        assert_eq!(signed_in_user(Some(&session(None, false))), None);
    }

    /// Every combination of platform, sign-in and watch link state: only a
    /// signed-in user on a companion platform with the watch app installed
    /// on a paired watch is eligible. Reachability does not matter.
    #[test]
    fn gating_matrix() {
        for platform in [false, true] {
            for signed_in in [false, true] {
                for bits in 0..16 {
                    let link = LinkState::from_bits(bits);
                    let expected =
                        platform && signed_in && link.supported && link.paired && link.installed;
                    assert_eq!(
                        settings_eligible(platform, signed_in, link),
                        expected,
                        "platform={platform} signed_in={signed_in} bits={bits}"
                    );
                }
            }
        }
        let installed = LinkState::from_bits(1 | 2 | 4);
        assert!(settings_eligible(true, true, installed));
        assert!(settings_eligible(true, true, LinkState::from_bits(15)));
        // Web/desktop, signed out, no paired watch, watch app missing.
        assert!(!settings_eligible(false, true, LinkState::from_bits(15)));
        assert!(!settings_eligible(true, false, LinkState::from_bits(15)));
        assert!(!settings_eligible(true, true, LinkState::from_bits(1)));
        assert!(!settings_eligible(true, true, LinkState::from_bits(1 | 2 | 8)));
    }

    #[test]
    fn this_build_platform() {
        assert_eq!(
            COMPANION_PLATFORM,
            cfg!(any(target_os = "ios", target_os = "android"))
        );
        // Host (desktop) and web builds never offer watch settings.
        #[cfg(not(any(target_os = "ios", target_os = "android")))]
        assert!(!watch_settings_available());
    }

    #[test]
    fn ineligible_users_get_defaults_not_persisted_settings() {
        assert_eq!(effective_settings(false), WatchSettings::default());
    }
}
