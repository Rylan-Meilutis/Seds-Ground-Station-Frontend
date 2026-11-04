//! In-app confirmation works in native webviews without platform JavaScript dialogs.
use dioxus::prelude::*;

#[derive(Clone)]
struct PendingCommand {
    identity: std::sync::Arc<()>,
    command: String,
    message: &'static str,
    dispatch: fn(&str),
    station: String,
    session: Option<String>,
}
static PENDING: GlobalSignal<Option<PendingCommand>> = Signal::global(|| None);

// This is UI burst protection only, never a command/permission deadline. Keep
// just the former dialog hit area after consuming its command, so the second
// click cannot activate a different control newly exposed underneath. This
// bounds protection to the existing activation interval, not arbitrary OS
// double-click settings. The rest of the screen, including Stop, stays usable.
#[derive(Clone)]
struct RetiredDialog {
    identity: std::sync::Arc<()>,
    message: &'static str,
    retired_ms: f64,
}
static RETIRED: GlobalSignal<Option<RetiredDialog>> = Signal::global(|| None);
fn retire_dialog(pending: &PendingCommand) {
    *RETIRED.write() = Some(RetiredDialog {
        identity: pending.identity.clone(), message: pending.message,
        retired_ms: super::monotonic_now_ms(),
    });
}
fn remaining_burst_ms(retired_ms: f64, now_ms: f64) -> u32 {
    let elapsed = now_ms - retired_ms;
    if elapsed.is_finite() && elapsed >= 0.0 {
        (super::COMMAND_ACTIVATION_DEDUP_MS - elapsed).clamp(0.0, super::COMMAND_ACTIVATION_DEDUP_MS).ceil() as u32
    } else {
        super::COMMAND_ACTIVATION_DEDUP_MS as u32
    }
}
fn same_retirement(a: &RetiredDialog, b: &RetiredDialog) -> bool {
    std::sync::Arc::ptr_eq(&a.identity, &b.identity)
}

pub(super) fn request(command: &str, dispatch: fn(&str)) {
    let message = match command {
        "ResetFlightState" => {
            "Reset the shared flight state to Idle? This does not close valves or stop an active physical sequence. Confirm the system is safe first."
        }
        "ResetTClock" => {
            "Reset the displayed T-clock? Flight state and physical board sequences will not be changed."
        }
        _ => {
            dispatch(command);
            return;
        }
    };
    if PENDING.peek().is_none() {
        *RETIRED.write() = None;
        *PENDING.write() = Some(PendingCommand {
            identity: std::sync::Arc::new(()),
            command: command.into(),
            message,
            dispatch,
            station: super::UrlConfig::base_http(),
            session: crate::auth::current_token(),
        });
    }
}

fn cancel_pending() {
    let cancelled = PENDING.write().take();
    if let Some(cancelled) = cancelled {
        retire_dialog(&cancelled);
        // The press opened a dialog; it never dispatched a command. Allow an
        // immediate, deliberate retry after Cancel or Escape instead of retaining
        // the activation debounce from that abandoned press.
        super::clear_command_feedback_latches();
    }
}

#[component]
pub(super) fn CommandConfirmation() -> Element {
    use_drop(cancel_pending);
    let mut cancel_node = use_signal(|| None::<std::rc::Rc<MountedData>>);
    use_effect(move || {
        let pending = PENDING.read().clone();
        let mounted = cancel_node.read().clone();
        if let (Some(pending), Some(mounted)) = (pending, mounted) {
            spawn(async move {
                // Reused DOM does not fire onmounted again. Focus only the
                // current request's Cancel control after its pending render.
                let current = PENDING.peek().as_ref().is_some_and(|current| {
                    std::sync::Arc::ptr_eq(&current.identity, &pending.identity)
                });
                if current { let _ = mounted.set_focus(true).await; }
            });
        }
    });
    // Reactive scope cancellation never grants dispatch. A remount retains the
    // inert geometry only for the original interval's remaining time. Each
    // cleanup is fenced by its exact identity so it cannot erase a new dialog.
    let _cleanup = use_resource(move || {
        let retired = RETIRED.read().clone();
        async move {
            if let Some(retired) = retired {
                let wait_ms = remaining_burst_ms(retired.retired_ms, super::monotonic_now_ms());
                #[cfg(target_arch = "wasm32")]
                gloo_timers::future::TimeoutFuture::new(wait_ms).await;
                #[cfg(not(target_arch = "wasm32"))]
                tokio::time::sleep(std::time::Duration::from_millis(u64::from(wait_ms))).await;
                let current = RETIRED.peek().as_ref().is_some_and(|current| same_retirement(current, &retired));
                if current { *RETIRED.write() = None; }
            }
        }
    });
    let pending = PENDING.read().clone();
    let retired = RETIRED.read().clone();
    let message = match (&pending, &retired) {
        (Some(pending), _) => pending.message,
        (None, Some(retired)) => retired.message,
        (None, None) => return rsx! {},
    };
    let inert = pending.is_none();
    let pointer_events = if inert { "none" } else { "auto" };
    let backdrop = if inert { "transparent" } else { "#000a" };
    rsx! {
        div {
            style: "position:fixed;inset:0;z-index:200000;background:{backdrop};pointer-events:{pointer_events};display:flex;align-items:center;justify-content:center;padding:20px;box-sizing:border-box;",
            onkeydown: move |event| { if event.key() == Key::Escape { cancel_pending(); } },
            div {
                class: "gs26-modal",
                "data-confirmation-retired": if inert { "true" } else { "false" },
                role: "alertdialog", "aria-modal": !inert, "aria-labelledby": "gs26-command-confirm-title", "aria-describedby": "gs26-command-confirm-message",
                style: "pointer-events:auto;width:min(100%,480px);max-height:90vh;overflow:auto;background:var(--gs26-panel-background,#142534);color:var(--gs26-app-text,#e5e7eb);border:1px solid var(--gs26-border,#526579);border-radius:14px;padding:22px;box-sizing:border-box;box-shadow:0 12px 60px #0008;",
                h2 { id: "gs26-command-confirm-title", style: "margin:0 0 14px;font-size:20px;", "Confirm reset" }
                p { id: "gs26-command-confirm-message", style: "line-height:1.5;", "{message}" }
                div { style: "display:flex;justify-content:flex-end;gap:12px;margin-top:22px;",
                    button {
                        disabled: inert,
                        autofocus: !inert,
                        // The autofocus attribute is not reapplied consistently when
                        // a webview mounts a second modal. Focus after every mount so
                        // Escape reaches this dialog rather than the underlying page.
                        onmounted: move |event| { cancel_node.set(Some(event.data())); },
                        style: "min-height:44px;padding:10px 18px;border-radius:8px;cursor:pointer;",
                        onclick: move |_| { cancel_pending(); }, "Cancel"
                    }
                    button {
                        class: "gs26-modal-danger",
                        disabled: inert,
                        style: "min-height:44px;padding:10px 18px;border-radius:8px;cursor:pointer;",
                        onclick: move |_| {
                            // Consume before dispatch: repeated clicks cannot submit twice.
                            // The existing dispatch rechecks permissions, connection and action policy.
                            let pending = PENDING.write().take();
                            if let Some(pending) = pending {
                                retire_dialog(&pending);
                                if pending.station == super::UrlConfig::base_http() && pending.session == crate::auth::current_token() {
                                    (pending.dispatch)(&pending.command);
                                } else {
                                    *super::COMMAND_SEND_ERROR.write() = Some("Connection changed. Review the current station and confirm the reset again.".into());
                                }
                            }
                        }, "Confirm reset"
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod retired_dialog_tests {
    use super::*;

    #[test]
    fn burst_budget_never_renews_a_valid_original_retirement() {
        assert_eq!(remaining_burst_ms(100.0, 100.0), 450);
        assert_eq!(remaining_burst_ms(100.0, 549.0), 1);
        assert_eq!(remaining_burst_ms(100.0, 550.0), 0);
        assert_eq!(remaining_burst_ms(100.0, 900.0), 0);
    }

    #[test]
    fn invalid_ui_clock_is_bounded_without_command_authority() {
        assert_eq!(remaining_burst_ms(f64::NAN, 100.0), 450);
        assert_eq!(remaining_burst_ms(100.0, 99.0), 450);
    }

    #[test]
    fn retirement_identity_cannot_clear_an_equal_message_successor() {
        let first = RetiredDialog { identity: std::sync::Arc::new(()), message: "same", retired_ms: 100.0 };
        let held = first.clone();
        let next = RetiredDialog { identity: std::sync::Arc::new(()), message: "same", retired_ms: 100.0 };
        assert!(same_retirement(&first, &held));
        assert!(!same_retirement(&first, &next));
    }
}
