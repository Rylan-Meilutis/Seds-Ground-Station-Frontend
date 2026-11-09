//! Operator flow on the backend's real HITL board abort snapshot
//! (`board-abort/status-hitl`, GroundStation26 contract fixture): Dismiss every
//! board's Abort on the Errors tab, then reload the app. The dismissals must
//! stay in effect on the Errors and Connection Status tabs until a board
//! aborts again.
use super::*;
use crate::backend_contract::decode;

const STORAGE_KEY: &str = "gs_dismissed_board_aborts";

fn errors_app() -> Element {
    let errors = use_signal(Vec::<AlertMsg>::new);
    rsx! {
        ErrorsTab { errors, ack_timestamp_ms: 0, theme: layout::ThemeConfig::default(), on_ack: move |_| {}, on_history_cleared: move |_| {} }
    }
}
fn both_tabs_app() -> Element {
    let errors = use_signal(Vec::<AlertMsg>::new);
    let boards = use_signal(Vec::<BoardStatusEntry>::new);
    let network_topology = use_signal(NetworkTopologyMsg::default);
    rsx! {
        ErrorsTab { errors, ack_timestamp_ms: 0, theme: layout::ThemeConfig::default(), on_ack: move |_| {}, on_history_cleared: move |_| {} }
        connection_status_tab::ConnectionStatusTab {
            boards, network_topology, ws_connected: true, expected_boards: Vec::new(),
            layout: layout::ConnectionTabLayout { sections: Vec::new() }, title: "Connection Status".to_string(), theme: layout::ThemeConfig::default()
        }
    }
}
/// A fresh app (new runtime, so global signals start over from storage) showing `snapshot`.
fn launch(app: fn() -> Element, snapshot: &BoardAbortSnapshot) -> (VirtualDom, Vec<dioxus::core::ElementId>) {
    let mut dom = VirtualDom::new(app);
    dom.in_scope(ScopeId::ROOT, || *BOARD_ABORT_SIGNAL.write() = snapshot.clone());
    let clicks = dom
        .rebuild_to_vec()
        .edits
        .iter()
        .filter_map(|edit| match edit {
            dioxus::core::Mutation::NewEventListener { name, id } if name == "click" => Some(*id),
            _ => None,
        })
        .collect();
    (dom, clicks)
}
fn click(dom: &VirtualDom, id: dioxus::core::ElementId) {
    dioxus::html::set_event_converter(Box::new(dioxus::html::SerializedHtmlEventConverter));
    let payload = dioxus::html::PlatformEventData::new(Box::new(dioxus::html::SerializedMouseData::default()));
    dom.runtime().handle_event("click", Event::new(std::rc::Rc::new(payload) as std::rc::Rc<dyn std::any::Any>, true), id);
}
fn shown(html: &str, snapshot: &BoardAbortSnapshot) -> Vec<String> {
    snapshot
        .active_errors
        .iter()
        .filter(|e| html.contains(&format!("Active: {}", e.board_label)) || html.contains(&format!("Active ERROR: {}", e.board_label)))
        .map(|e| e.board.clone())
        .collect()
}

#[test]
fn hitl_dismissals_cover_every_board_and_survive_a_reload_on_errors_and_connection_status() {
    let saved = persist::get_string(STORAGE_KEY);
    let snapshot: BoardAbortSnapshot = decode("board-abort/status-hitl");
    assert_eq!(snapshot.active_errors.len(), 7, "the backend reports every board's Abort");

    let (mut dom, clicks) = launch(errors_app, &snapshot);
    let html = dioxus::ssr::render(&dom);
    assert_eq!(html.matches(">Dismiss<").count(), 7, "HITL offers Dismiss on every board's Abort: {html}");
    // Every click handler on the tab; the Dismiss buttons are among them, the
    // others (Acknowledge, Clear history) do nothing that outlives the app.
    for id in clicks {
        click(&dom, id);
    }
    dom.render_immediate_to_vec();
    assert!(shown(&dioxus::ssr::render(&dom), &snapshot).is_empty(), "dismissed on the Errors tab");
    drop(dom);

    // Reload: a new app reads the dismissals back from storage.
    let (dom, _) = launch(both_tabs_app, &snapshot);
    let html = dioxus::ssr::render(&dom);
    let still = shown(&html, &snapshot);
    // A later Abort (new abort time) shows again.
    let mut again = snapshot.clone();
    again.active_errors[0].timestamp_ms += 1;
    let (dom2, _) = launch(errors_app, &again);
    let reshown = shown(&dioxus::ssr::render(&dom2), &again);
    drop((dom, dom2));
    persist::set_string(STORAGE_KEY, saved.as_deref().unwrap_or("[]"));
    assert!(still.is_empty(), "dismissed aborts reappeared after a reload on the Errors or Connection Status tab: {still:?}");
    assert_eq!(reshown, vec![again.active_errors[0].board.clone()]);
}
