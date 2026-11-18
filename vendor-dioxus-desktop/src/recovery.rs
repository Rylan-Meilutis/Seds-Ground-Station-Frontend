//! Reloading the app document when the platform kills or blanks the web content.
//!
//! WebKit (iOS, macOS, Linux), WebView2 and the Android WebView run page content
//! in a separate process that the OS may end, typically while the app is in the
//! background. The webview then shows only its background colour. Recovery loads
//! the app document again; when it initializes, the window gets a fresh virtual
//! dom on the same route (see `WebviewInstance::remount_fresh_dom`).

use crate::ipc::UserWindowEvent;
use dioxus_core::VirtualDom;
use dioxus_history::{History, MemoryHistory};
use std::cell::RefCell;
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, Ordering};
use tao::event_loop::EventLoopProxy;

/// Builds the app's virtual dom again with its root contexts.
pub(crate) type DomFactory = Rc<dyn Fn() -> VirtualDom>;

thread_local! {
    static DOM_FACTORY: RefCell<Option<DomFactory>> = const { RefCell::new(None) };
    static PROXY: RefCell<Option<EventLoopProxy<UserWindowEvent>>> = const { RefCell::new(None) };
}

pub(crate) fn set_dom_factory(factory: DomFactory) {
    DOM_FACTORY.with(|slot| *slot.borrow_mut() = Some(factory));
}

pub(crate) fn dom_factory() -> Option<DomFactory> {
    DOM_FACTORY.with(|slot| slot.borrow().clone())
}

pub(crate) fn set_proxy(proxy: EventLoopProxy<UserWindowEvent>) {
    PROXY.with(|slot| *slot.borrow_mut() = Some(proxy));
}

/// Asks every window to reload its app document because its web content died
/// or came back blank. Call on the main thread (platform callbacks such as
/// WebKit's and WebView2's process-failure signals arrive there). Returns false
/// before the app's event loop exists.
pub fn request_webview_reload(reason: &str) -> bool {
    PROXY.with(|slot| {
        slot.borrow().as_ref().is_some_and(|proxy| {
            proxy
                .send_event(UserWindowEvent::RecoverWebview {
                    id: None,
                    reason: reason.to_string(),
                })
                .is_ok()
        })
    })
}

/// The line logged for every recovery.
pub(crate) fn recovery_log_line(reason: &str) -> String {
    format!("[webview] content process terminated, reloading ({reason})")
}

/// Reload in place when the webview still knows its document; load the app
/// root when the content process died before a URL was committed.
pub(crate) fn needs_root_load(current_url: &str) -> bool {
    let url = current_url.trim();
    url.is_empty() || url == "about:blank"
}

/// Navigation policy for the app document itself: the first load, then only
/// a reload that recovery armed (once per arming).
pub(crate) fn allow_app_document_navigation(loaded_before: bool, reload_armed: &AtomicBool) -> bool {
    !loaded_before || reload_armed.swap(false, Ordering::SeqCst)
}

/// History for the fresh dom after a reload: it opens on the route the old dom showed.
pub(crate) fn carry_route(old: &dyn History) -> Rc<dyn History> {
    Rc::new(MemoryHistory::with_initial_path(old.current_route()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_app_document_reloads_only_when_recovery_armed_it() {
        let armed = AtomicBool::new(false);
        assert!(allow_app_document_navigation(false, &armed), "first load");
        assert!(!allow_app_document_navigation(true, &armed), "page-initiated reload stays blocked");
        armed.store(true, Ordering::SeqCst);
        assert!(allow_app_document_navigation(true, &armed), "armed recovery reload");
        assert!(!allow_app_document_navigation(true, &armed), "the arming is used up");
    }

    #[test]
    fn the_remounted_app_opens_on_the_route_it_showed() {
        let old = MemoryHistory::default();
        old.push("/settings".to_string());
        assert_eq!(carry_route(&old).current_route(), "/settings");
        assert_eq!(carry_route(&MemoryHistory::default()).current_route(), "/");
    }

    #[test]
    fn blank_or_missing_urls_load_the_root() {
        assert!(needs_root_load(""));
        assert!(needs_root_load("about:blank"));
        assert!(!needs_root_load("dioxus://index.html/"));
        assert!(!needs_root_load("http://dioxus.index.html/"));
    }

    #[test]
    fn every_recovery_logs_the_same_prefix() {
        assert!(recovery_log_line("x").starts_with("[webview] content process terminated, reloading"));
    }
}
