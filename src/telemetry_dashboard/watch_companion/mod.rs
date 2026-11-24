//! Apple Watch / Wear OS companion: settings, account sync, the phone-side
//! publisher and the direct-link provisioning. See docs/watch-companion.md.
pub(crate) mod account_sync;
pub(crate) mod actions;
pub(crate) mod bridge;
pub(crate) mod direct;
// Reference implementation for the watch ports; the phone only formats ages.
#[cfg_attr(not(test), allow(dead_code))]
pub(crate) mod geodesy;
// The web frontend has no watch bridge: only its stub runtime is used there.
#[cfg_attr(target_arch = "wasm32", allow(dead_code))]
pub(crate) mod map_sync;
// The phone plans and sends the tile set; the view math (visible tiles,
// eviction, auto-fit) is the reference for the watch ports.
#[cfg_attr(not(test), allow(dead_code))]
pub(crate) mod map_tiles;
pub(crate) mod phone_status;
pub(crate) mod protocol;
mod publisher;
pub(crate) mod registry;
// The browser build has no watch: only the phone's relay step runs there.
#[cfg_attr(target_arch = "wasm32", allow(dead_code))]
pub(crate) mod search_relay;
pub(crate) mod settings;
mod settings_section;
pub(crate) mod snapshot;
// Native phone apps only: the browser build has no watch to relay for.
#[cfg(not(target_arch = "wasm32"))]
pub(crate) mod voice_relay;

#[cfg(not(target_arch = "wasm32"))]
pub(super) use publisher::use_demo_publisher;
pub(super) use publisher::use_live_publisher;
pub(crate) use phone_status::{PhonePhase, set_phone_phase};
pub(crate) use publisher::use_status_publisher;
pub(super) use settings_section::{WatchLinkDiagnostics, WatchSettingsSection};

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

/// One diagnostics line for Settings: the bridge bits and why the Watch
/// tab is shown or hidden.
pub(crate) fn watch_link_diagnostics() -> String {
    let link = bridge::link_state();
    let signed_in = account_key().is_some();
    format!(
        "{} · signed in: {} · Watch settings {}",
        link.debug_text(),
        if signed_in { "yes" } else { "no" },
        phone_status::eligibility_reason(COMPANION_PLATFORM, signed_in, link)
    )
}

/// Live eligibility for the Settings screen. Polls the bridge and session on
/// companion platforms only; elsewhere it is constantly false.
pub(super) fn use_watch_settings_available() -> Signal<bool> {
    let mut available = use_signal(watch_settings_available);
    use_future(move || async move {
        if !COMPANION_PLATFORM {
            return;
        }
        // Settings can be opened before any publisher has started the
        // session (e.g. from the Connect screen); activation is idempotent.
        bridge::start();
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

/// The GS's watch action list for the signed-in account (`account_key`).
static ACTION_CATALOG: std::sync::Mutex<Option<(String, actions::ActionCatalog)>> =
    std::sync::Mutex::new(None);

/// Watch token held for the direct link, and when issuing last failed.
struct DirectState {
    held: Option<direct::Held>,
    last_failure_ms: Option<i64>,
    /// Last provisioning JSON sent to the watch.
    sent: Option<String>,
}

static DIRECT: std::sync::Mutex<DirectState> = std::sync::Mutex::new(DirectState {
    held: None,
    last_failure_ms: None,
    sent: None,
});

/// The cached catalog for the current account; empty for anyone else.
pub(crate) fn catalog_actions() -> Vec<actions::CatalogAction> {
    let key = account_key();
    ACTION_CATALOG
        .lock()
        .ok()
        .and_then(|g| g.clone())
        .filter(|(account, _)| Some(account) == key.as_ref())
        .map(|(_, c)| c.actions)
        .unwrap_or_default()
}

/// Full cached catalog (with exclusions) for the Settings section.
pub(crate) fn action_catalog() -> Option<actions::ActionCatalog> {
    let key = account_key()?;
    ACTION_CATALOG
        .lock()
        .ok()
        .and_then(|g| g.clone())
        .filter(|(account, _)| *account == key)
        .map(|(_, c)| c)
}

/// Reads `GET /api/watch/actions` for an eligible user. A station without
/// the endpoint offers no actions.
async fn refresh_action_catalog() {
    let Some(account) = account_key().filter(|_| watch_settings_available()) else {
        if let Ok(mut slot) = ACTION_CATALOG.lock() {
            *slot = None;
        }
        return;
    };
    let catalog = match super::http_get_json::<actions::ActionCatalog>(actions::CATALOG_ENDPOINT).await {
        Ok(catalog) => catalog.sanitized(),
        Err(error) if account_sync::is_unsupported(&error) => actions::ActionCatalog::default(),
        Err(_) => return, // keep the last list through a transient failure
    };
    if account_key().as_deref() != Some(account.as_str()) {
        return;
    }
    if let Ok(mut slot) = ACTION_CATALOG.lock() {
        if slot.as_ref().map(|(_, c)| c) != Some(&catalog) {
            PUBLISH_REQUESTED.store(true, Ordering::Release);
        }
        *slot = Some((account, catalog));
    }
}

fn direct_device_id() -> String {
    if let Some(id) = super::persist::get_string(direct::DEVICE_ID_KEY).filter(|id| !id.is_empty()) {
        return id;
    }
    let mut random = [0u8; 16];
    let _ = getrandom::fill(&mut random);
    let id = direct::new_device_id(u128::from_le_bytes(random));
    super::persist::set_string(direct::DEVICE_ID_KEY, &id);
    id
}

/// What the direct link needs now: an eligible, signed-in user connected to
/// a station with "Direct link" on.
fn direct_want(eligible: bool) -> Option<direct::Want> {
    let settings = current_settings();
    if !eligible || !settings.direct_link {
        return None;
    }
    let account = account_key()?;
    let session = crate::auth::current_session();
    let username = signed_in_user(session.as_ref())?.to_string();
    Some(direct::Want {
        account,
        base: super::UrlConfig::base_http(),
        username,
        station: crate::auth::station_name(),
        skip_tls: super::UrlConfig::_skip_tls_verify(),
        commands: actions::token_commands(&settings, &catalog_actions()),
        voice: settings.voice_wanted(),
    })
}

/// The watch token this phone holds now (the voice relay uses it).
#[cfg_attr(target_arch = "wasm32", allow(dead_code))]
fn direct_held() -> Option<direct::Held> {
    DIRECT.lock().ok().and_then(|state| state.held.clone())
}

/// Sends the provisioning message when it changed.
fn send_provision(held: Option<&direct::Held>, state: &mut DirectState) {
    let Ok(json) = serde_json::to_string(&direct::provision(held)) else {
        return;
    };
    if state.sent.as_deref() != Some(json.as_str()) {
        bridge::provision(&json);
        state.sent = Some(json);
    }
}

/// Issues, refreshes or drops the watch token. Runs in the dashboard
/// publisher, where the phone has a session with the station.
async fn direct_tick(eligible: bool) {
    let want = direct_want(eligible);
    let now = super::current_wallclock_ms();
    let (step, held) = {
        let Ok(state) = DIRECT.lock() else { return };
        (
            direct::plan(state.held.as_ref(), want.as_ref(), now, state.last_failure_ms),
            state.held.clone(),
        )
    };
    match step {
        direct::Step::Keep => {
            if let Ok(mut state) = DIRECT.lock() {
                let held = state.held.clone();
                send_provision(held.as_ref(), &mut state);
            }
        }
        direct::Step::Issue => {
            let Some(want) = want else { return };
            let device_id = direct_device_id();
            let request = direct::IssueRequest {
                device_id: &device_id,
                commands: &want.commands,
                voice: want.voice,
            };
            let result = super::http_post_json::<_, direct::Grant>(direct::TOKEN_ENDPOINT, &request).await;
            let Ok(mut state) = DIRECT.lock() else { return };
            match result {
                // Still the same station and account after the request.
                Ok(grant) if direct_want(eligible).is_some_and(|w| w.account == want.account) => {
                    state.held = Some(direct::Held { want, grant });
                    state.last_failure_ms = None;
                }
                Ok(_) => {}
                Err(error) => {
                    state.last_failure_ms = Some(now);
                    let line = format!("[watch] direct link token: {error}");
                    eprintln!("{line}");
                    crate::debug_log::append(&line);
                    if account_sync::is_unsupported(&error) {
                        // Station without watch tokens: the watch stays phone-only.
                        state.held = None;
                    }
                }
            }
            let held = state.held.clone();
            send_provision(held.as_ref(), &mut state);
        }
        direct::Step::Drop => {
            if let Some(held) = held
                && account_key().as_deref() == Some(held.want.account.as_str())
            {
                // Still signed in (direct link turned off, watch removed):
                // revoke on the GS. Sign-out revokes it there by itself.
                let request = direct::RevokeRequest {
                    device_id: &held.grant.device_id,
                };
                let _ = super::http_post_json::<_, serde_json::Value>(direct::REVOKE_ENDPOINT, &request).await;
            }
            if let Ok(mut state) = DIRECT.lock() {
                state.held = None;
                send_provision(None, &mut state);
            }
        }
    }
}

/// Without a dashboard: forget the token once the user signed out or the
/// account changed, and tell the watch.
fn direct_check_account() {
    let key = account_key();
    let Ok(mut state) = DIRECT.lock() else { return };
    let stale = state
        .held
        .as_ref()
        .is_some_and(|held| key.as_deref() != Some(held.want.account.as_str()));
    if stale || (key.is_none() && state.sent.is_none()) {
        state.held = None;
        send_provision(None, &mut state);
    }
}

/// Settings line: whether the watch can reach the station by itself.
pub(crate) fn direct_status_text() -> String {
    let Ok(state) = DIRECT.lock() else {
        return String::new();
    };
    match &state.held {
        Some(held) => {
            let mut actions = if held.grant.commands.is_empty() {
                "read-only".to_string()
            } else {
                format!("actions: {}", held.grant.commands.join(", "))
            };
            if held.grant.voice {
                actions.push_str(", crew voice");
            } else if held.want.voice {
                actions.push_str(", no voice permission");
            }
            format!(
                "Watch can reach {} directly ({actions})",
                direct_address(&held.want.base)
            )
        }
        None if state.last_failure_ms.is_some() => {
            "Direct link unavailable: the station did not issue a watch token".into()
        }
        None => "Direct link not set up (needs a signed-in station connection)".into(),
    }
}

fn direct_address(base: &str) -> String {
    phone_status::station_address(base)
}

/// Revokes this phone's watch token now (Settings button). A new one is
/// issued on the next round if the direct link stays on.
pub(crate) async fn revoke_direct_link() {
    let held = DIRECT.lock().ok().and_then(|s| s.held.clone());
    if let Some(held) = held {
        let request = direct::RevokeRequest {
            device_id: &held.grant.device_id,
        };
        let _ = super::http_post_json::<_, serde_json::Value>(direct::REVOKE_ENDPOINT, &request).await;
    }
    if let Ok(mut state) = DIRECT.lock() {
        state.held = None;
        // Do not reissue at once.
        state.last_failure_ms = Some(super::current_wallclock_ms());
        send_provision(None, &mut state);
    }
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
