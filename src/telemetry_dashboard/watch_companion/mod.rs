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
            SyncState::LocalOnly => "Saved on this device (sign in to sync to your account)",
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
    let mut next = current_settings();
    f(&mut next);
    apply_edit(next, Editor::Phone);
}

/// `base|username` for a signed-in, non-anonymous session.
fn account_key() -> Option<String> {
    let session = crate::auth::current_session()?;
    if session.session.anonymous {
        return None;
    }
    let user = session.session.username.filter(|u| !u.is_empty())?;
    Some(format!("{}|{}", super::UrlConfig::base_http(), user))
}

/// One reconciliation round with the account. Never blocks the UI thread
/// for long: at most three GET/POST pairs.
async fn sync_once() {
    use account_sync::*;
    let Some(account) = account_key() else {
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
