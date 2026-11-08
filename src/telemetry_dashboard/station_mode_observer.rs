//! One auth-owned mode observer survives ordinary admission/epoch retirement.
use super::{recording_download::ExportOwner, station_mode_contract::Snapshot};
use dioxus::prelude::*;
use std::{
    rc::Rc,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};
#[derive(Clone)]
pub(super) struct Observation {
    pub value: Snapshot,
    pub received: f64,
    pub owner: Rc<ExportOwner>,
    pub verified: bool,
}
impl Observation {
    pub fn current(&self, now: f64) -> bool {
        super::station_policy_context::observation_is_current(
            &self.value,
            self.received,
            now,
            self.verified,
            self.verified && self.owner.is_current(),
        )
    }
}
pub(super) static OBSERVED: GlobalSignal<Option<Observation>> = Signal::global(|| None);
pub(super) static CLOCK: GlobalSignal<f64> = Signal::global(|| 0.);
pub(super) static VIEW_KEY: GlobalSignal<String> = Signal::global(|| "mode-unknown:legacy".into());
pub(super) static ERROR: GlobalSignal<String> = Signal::global(String::new);
pub(super) static REFRESH: GlobalSignal<u64> = Signal::global(|| 0);
// Deliberately retain the ORIGINAL owner through ordinary closure and unavailable
// mode probes. A new account cannot acquire this proof from the same epoch text.
static APPLIED_OWNER: GlobalSignal<
    Option<(super::station_policy_context::AppliedStamp, Rc<ExportOwner>)>,
> = Signal::global(|| None);
pub(super) fn applied_owner_current(stamp: &super::station_policy_context::AppliedStamp) -> bool {
    APPLIED_OWNER
        .peek()
        .as_ref()
        .is_some_and(|(original, owner)| original == stamp && owner.is_current())
}
fn retire_applied_owner_if_needed() {
    // A new UI runtime may have no local Rc proof while process-static Context
    // still retains an old installed stamp. Never attach a new owner to that old
    // token: retire its generation before accepting a fresh observation.
    let retired = APPLIED_OWNER.peek().as_ref().map_or_else(
        || super::station_policy_context::applied_stamp().is_some(),
        |(_, owner)| !owner.is_current(),
    );
    if retired {
        *APPLIED_OWNER.write() = None;
        super::station_policy_context::retire_applied_owner();
    }
}
pub(super) fn owner(alive: Arc<AtomicBool>) -> Result<Rc<ExportOwner>, String> {
    ExportOwner::capture_observer_selected(move || alive.load(Ordering::Acquire))
}
/// View-only key. Retirement is enforced by the independent irreversible lease
/// generation; remounting drafts cannot resurrect an old operation.
pub(super) fn view_key() -> String {
    VIEW_KEY.read().clone()
}
fn key_for(observed: &Option<Observation>, now: f64) -> String {
    observed.as_ref().map_or_else(
        || {
            format!(
                "mode-unknown:{}",
                super::station_policy_context::view_generation()
            )
        },
        |o| {
            serde_json::to_string(&(
                &o.value.backend_instance,
                &o.value.authority_id,
                &o.value.principal_context,
                &o.value.policy_epoch,
                o.value.admissions_open,
                o.current(now),
                super::station_policy_context::view_generation(),
            ))
            .unwrap_or_else(|_| "mode-unavailable".into())
        },
    )
}

fn publish_view_key(now: f64) {
    let next = key_for(&OBSERVED.peek(), now);
    if *VIEW_KEY.peek() != next {
        *VIEW_KEY.write() = next;
    }
}

/// Admit an actual owned mode-GET completion on the UI runtime. Native fixtures
/// exercise this same production path after real loopback transport; no trusted
/// seal setter bypasses original auth/request-ticket or before-banner clearing.
/// The return value means "completion handled", never "mode applied".
pub(super) fn admit_owned_response(
    origin: &str,
    captured: Rc<ExportOwner>,
    ticket: &super::station_policy_context::ObservationTicket,
    started: f64,
    result: Result<serde_json::Value, String>,
) -> bool {
    if !captured.is_current()
        || captured.base() != origin
        || !super::station_policy_context::observation_current(ticket, origin)
    {
        return false;
    }
    // An authenticated literal support declaration can only
    // tighten the fallback floor, even when other fields fail
    // schema validation. Stale-owner replies never reach this.
    if let Ok(raw) = &result
        && raw["epoch_fencing_supported"].as_bool() == Some(true)
    {
        let _ = super::station_policy_context::declare_supported(
            origin,
            raw["authority_id"].as_str(),
        );
    }
    if let Ok(raw) = &result
        && raw["control_origin_required"].as_bool() == Some(true) {
        let _ = super::station_policy_context::declare_control_origin_required(origin, raw["authority_id"].as_str());
    }
    match result.and_then(Snapshot::decode) {
        Ok(value) => {
            retire_applied_owner_if_needed();
            let now = super::monotonic_now_ms();
            let published = super::station_policy_context::publish(
                origin,
                value.clone(),
                started,
                now,
            );
            if let Err(error) = published {
                super::synchronize_mode_ingress_context();
                if let Some(previous) = OBSERVED.write().as_mut() {
                    previous.verified = false;
                }
                *ERROR.write() = error;
            } else {
                if value.authority_current
                    && value.fresh(started, now)
                    && value.epoch_fencing_supported
                    && value.policy_epoch.is_some()
                    && value.current.is_some()
                    && let Some(stamp) =
                        super::station_policy_context::applied_stamp()
                {
                    let retained = APPLIED_OWNER.peek().as_ref().is_some_and(
                        |(original, owner)| {
                            *original == stamp && owner.is_current()
                        },
                    );
                    if !retained {
                        *APPLIED_OWNER.write() =
                            Some((stamp, captured.clone()));
                    }
                }
                // Clear/fence current live facts BEFORE displaying the
                // newly installed mode. Hook owns actual queue/reducer reset.
                super::synchronize_mode_ingress_context();
                *OBSERVED.write() = Some(Observation {
                    value,
                    received: started,
                    owner: captured,
                    verified: true,
                });
                *ERROR.write() = String::new();
            }
        }
        Err(error) => {
            let _ = super::station_policy_context::unavailable(origin);
            super::synchronize_mode_ingress_context();
            if let Some(previous) = OBSERVED.write().as_mut() {
                previous.verified = false;
            }
            *ERROR.write() = error;
        }
    }
    publish_view_key(super::monotonic_now_ms());
    true
}

#[component]
pub(super) fn StationModeObserver() -> Element {
    let alive = super::recording_download::use_export_lifetime();
    let poll_alive = alive.clone();
    use_future(move || {
        let alive = poll_alive.clone();
        async move {
            loop {
                if !alive.load(Ordering::Acquire) {
                    return;
                }
                // Fresh observer auth owner per request, captured before its future.
                // Neither an epoch change nor a retired ordinary Settings draft owns it.
                let captured = owner(alive.clone());
                let origin = super::UrlConfig::base_http();
                let started = super::monotonic_now_ms();
                let ticket = super::station_policy_context::observation_ticket(&origin);
                if let Ok(captured) = captured {
                    let result =
                        super::station_mode_transport::capabilities(Ok(captured.clone())).await;
                    if !alive.load(Ordering::Acquire) {
                        return;
                    }
                    if let Ok(ticket) = ticket {
                        admit_owned_response(&origin, captured, &ticket, started, result);
                    }
                    // A stale completion writes nothing, including no invalidation.
                }
                let refresh = *REFRESH.peek();
                for _ in 0..20 {
                    if !alive.load(Ordering::Acquire) {
                        return;
                    }
                    #[cfg(target_arch = "wasm32")]
                    gloo_timers::future::TimeoutFuture::new(100).await;
                    #[cfg(not(target_arch = "wasm32"))]
                    tokio::time::sleep(std::time::Duration::from_millis(100)).await;
                    if *REFRESH.peek() != refresh {
                        break;
                    }
                }
            }
        }
    });
    use_future(move || {
        let alive = alive.clone();
        async move {
            loop {
                if !alive.load(Ordering::Acquire) {
                    return;
                }
                let now = super::monotonic_now_ms();
                retire_applied_owner_if_needed();
                let _ = super::station_policy_context::tick(now);
                super::synchronize_mode_ingress_context();
                *CLOCK.write() = now;
                // Retire accepted display based on current ownership, independently of
                // any late response. Retain same-owner expired observations as last known.
                let retired = OBSERVED
                    .peek()
                    .as_ref()
                    .is_some_and(|o| !o.owner.is_current());
                if retired {
                    *OBSERVED.write() = None;
                }
                publish_view_key(now);
                #[cfg(target_arch = "wasm32")]
                gloo_timers::future::TimeoutFuture::new(100).await;
                #[cfg(not(target_arch = "wasm32"))]
                tokio::time::sleep(std::time::Duration::from_millis(100)).await;
            }
        }
    });
    let current = OBSERVED.read().clone();
    let now = *CLOCK.read();
    let simulation_reported = current
        .as_ref()
        .is_some_and(|o| o.value.current.as_ref().is_some_and(|c| c.simulation));
    let unknown_execution = current
        .as_ref()
        .map_or_else(super::station_policy_context::epoch_fencing_required, |o| {
            o.value.current.is_none() || !o.current(now)
        });
    rsx! {
        if simulation_reported {
            if let Some(o) = current {
                div {role:"status",class:"gs26-simulation-domain",style:"padding:8px 12px;background:#503b12;color:#fff2c6;font:inherit;",
                    if o.current(now) {"SIMULATION — backend-reported simulated telemetry and actions. "}
                    else {"Last reported SIMULATION; current execution domain is unverified. "}
                    "{o.value.current.as_ref().map_or(\"Unknown\",|c|c.output_domain.as_str())}"
                }
            }
        } else if unknown_execution {
            div {role:"status",class:"gs26-execution-domain-unverified",style:"padding:8px 12px;background:#503b12;color:#fff2c6;font:inherit;",
                "Execution domain is unverified. No hardware domain is assumed; last-known data does not establish the current output domain."
            }
        }
    }
}
