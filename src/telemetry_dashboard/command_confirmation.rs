//! In-app confirmation works in native webviews without platform JavaScript dialogs.
use dioxus::prelude::*;

#[derive(Clone)]
struct PendingCommand {
    command: String,
    message: &'static str,
    dispatch: fn(&str),
    station: String,
    session: Option<String>,
}
static PENDING: GlobalSignal<Option<PendingCommand>> = Signal::global(|| None);

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
        *PENDING.write() = Some(PendingCommand {
            command: command.into(),
            message,
            dispatch,
            station: super::UrlConfig::base_http(),
            session: crate::auth::current_token(),
        });
    }
}

fn cancel_pending() {
    let cancelled = PENDING.write().take().is_some();
    if cancelled {
        // The press opened a dialog; it never dispatched a command. Allow an
        // immediate, deliberate retry after Cancel or Escape instead of retaining
        // the activation debounce from that abandoned press.
        super::clear_command_feedback_latches();
    }
}

#[component]
pub(super) fn CommandConfirmation() -> Element {
    use_drop(cancel_pending);
    let pending = PENDING.read().clone();
    let Some(pending) = pending else {
        return rsx! {};
    };
    rsx! {
        div {
            style: "position:fixed;inset:0;z-index:200000;background:#000a;display:flex;align-items:center;justify-content:center;padding:20px;box-sizing:border-box;",
            onkeydown: move |event| { if event.key() == Key::Escape { cancel_pending(); } },
            div {
                role: "alertdialog", "aria-modal": "true", "aria-labelledby": "gs26-command-confirm-title", "aria-describedby": "gs26-command-confirm-message",
                style: "width:min(100%,480px);max-height:90vh;overflow:auto;background:var(--gs26-panel-background,#142534);color:var(--gs26-app-text,#e5e7eb);border:1px solid var(--gs26-border,#526579);border-radius:14px;padding:22px;box-sizing:border-box;box-shadow:0 12px 60px #0008;",
                h2 { id: "gs26-command-confirm-title", style: "margin:0 0 14px;font-size:20px;", "Confirm reset" }
                p { id: "gs26-command-confirm-message", style: "line-height:1.5;", "{pending.message}" }
                div { style: "display:flex;justify-content:flex-end;gap:12px;margin-top:22px;",
                    button {
                        autofocus: true,
                        // The autofocus attribute is not reapplied consistently when
                        // a webview mounts a second modal. Focus after every mount so
                        // Escape reaches this dialog rather than the underlying page.
                        onmounted: move |event| async move { let _ = event.data().set_focus(true).await; },
                        style: "min-height:44px;padding:10px 18px;border-radius:8px;cursor:pointer;",
                        onclick: move |_| { cancel_pending(); }, "Cancel"
                    }
                    button {
                        style: "min-height:44px;padding:10px 18px;border-radius:8px;cursor:pointer;",
                        onclick: move |_| {
                            // Consume before dispatch: repeated clicks cannot submit twice.
                            // The existing dispatch rechecks permissions, connection and action policy.
                            let pending = PENDING.write().take();
                            if let Some(pending) = pending {
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
