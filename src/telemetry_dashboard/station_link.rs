//! Which station the dashboard talks to. Chosen once, when a connection
//! starts, never per request or per row:
//!
//! - **Remote** (the default): HTTP to `{base}/api/...` and the `/ws`
//!   WebSocket of a Ground Station.
//! - **Local**: a station answered on this device
//!   ([`local_station::LocalStation`]): demo mode, or a replay file or bundle.
//!   The offline entry route installs it for as long as it is mounted.
//!
//! Every HTTP helper asks [`local`] before it builds a network request, and
//! the WebSocket supervisor asks once when it starts. With no local station
//! installed that is one thread-local read per HTTP request and nothing at
//! all per telemetry row: the live socket and ingest path are the unchanged
//! remote code. While a local station is installed the base URL is
//! [`LOCAL_BASE`], which no HTTP client or WebSocket can reach, so a call
//! site that missed the local answer fails instead of sending.
use super::local_station::LocalStation;
use std::cell::RefCell;
use std::rc::Rc;

/// The base URL while a local station is selected: not routable, so nothing
/// can leave the device by accident.
pub(crate) const LOCAL_BASE: &str = "local-station://this-device";

thread_local! {
    static LOCAL: RefCell<Option<Rc<LocalStation>>> = const { RefCell::new(None) };
}

/// The local station, when one is selected instead of a Ground Station.
pub(crate) fn local() -> Option<Rc<LocalStation>> {
    LOCAL.with(|slot| slot.borrow().clone())
}

pub(crate) fn is_local() -> bool {
    LOCAL.with(|slot| slot.borrow().is_some())
}

/// Selects `station` until the guard drops (the offline entry route's
/// lifetime). Dropping it returns to the remote station and frees the
/// local station once its last task has ended.
pub(crate) struct LocalSelection(());

impl LocalSelection {
    pub(crate) fn install(station: Rc<LocalStation>) -> Self {
        install_tiles(Some(station.session.tiles()));
        LOCAL.with(|slot| *slot.borrow_mut() = Some(station));
        super::persist::begin_scratch();
        #[cfg(target_arch = "wasm32")]
        super::js_eval(BROWSER_SCRATCH_ON);
        Self(())
    }
}

impl Drop for LocalSelection {
    fn drop(&mut self) {
        LOCAL.with(|slot| slot.borrow_mut().take());
        install_tiles(None);
        super::persist::end_scratch();
        #[cfg(target_arch = "wasm32")]
        super::js_eval(BROWSER_SCRATCH_OFF);
    }
}

/// In the browser, scripts (the map, the debug log, settings sync) write
/// `localStorage` directly. While a local station is selected their writes
/// land in a page-memory overlay that reads see first, so the station's own
/// preferences and caches in this browser are left exactly as they were.
#[cfg(target_arch = "wasm32")]
const BROWSER_SCRATCH_ON: &str = r#"(()=>{const P=Storage.prototype;if(P.__gs26Scratch)return;
const o={setItem:P.setItem,getItem:P.getItem,removeItem:P.removeItem,clear:P.clear};const m=new Map();
P.__gs26Scratch={o,m};
P.setItem=function(k,v){if(this===window.localStorage){m.set(String(k),String(v));return;}return o.setItem.call(this,k,v);};
P.getItem=function(k){if(this===window.localStorage&&m.has(String(k)))return m.get(String(k));return o.getItem.call(this,k);};
P.removeItem=function(k){if(this===window.localStorage){m.set(String(k),null);return;}return o.removeItem.call(this,k);};
P.clear=function(){if(this===window.localStorage)return;return o.clear.call(this);};})()"#;
#[cfg(target_arch = "wasm32")]
const BROWSER_SCRATCH_OFF: &str = r#"(()=>{const P=Storage.prototype;const s=P.__gs26Scratch;if(!s)return;Object.assign(P,s.o);delete P.__gs26Scratch;})()"#;

/// Map tiles of the selected local station, "z/x/y" -> JPEG bytes. The map
/// draws only these while a local station is selected and fetches nothing.
pub(crate) type Tiles = Vec<(String, Rc<Vec<u8>>)>;

#[cfg(target_arch = "wasm32")]
fn install_tiles(tiles: Option<Tiles>) {
    let Some(window) = web_sys::window() else { return };
    let key = wasm_bindgen::JsValue::from_str("__gs26LocalTiles");
    match tiles {
        Some(tiles) => {
            let map = js_sys::Map::new();
            for (coord, bytes) in tiles {
                map.set(&coord.as_str().into(), &js_sys::Uint8Array::from(bytes.as_slice()));
            }
            let _ = js_sys::Reflect::set(&window, &key, &map);
        }
        None => {
            let _ = js_sys::Reflect::delete_property(&window, &key);
        }
    }
}

/// Native webviews load tiles through the app's own protocol handler
/// (`main.rs`), on another thread; it serves from this copy instead of the
/// station while a local station is selected.
#[cfg(not(target_arch = "wasm32"))]
static NATIVE_TILES: std::sync::Mutex<Option<std::collections::HashMap<String, Vec<u8>>>> = std::sync::Mutex::new(None);

#[cfg(not(target_arch = "wasm32"))]
fn install_tiles(tiles: Option<Tiles>) {
    if let Ok(mut slot) = NATIVE_TILES.lock() {
        *slot = tiles.map(|list| list.into_iter().map(|(k, v)| (k, v.as_ref().clone())).collect());
    }
}

/// `Some(tile or None)` while a local station is selected: the protocol
/// handler must then answer from it and never contact a station.
#[cfg(not(target_arch = "wasm32"))]
pub(crate) fn native_local_tile(z: u32, x: u32, y: u32) -> Option<Option<Vec<u8>>> {
    let slot = NATIVE_TILES.lock().ok()?;
    slot.as_ref().map(|tiles| tiles.get(&format!("{z}/{x}/{y}")).cloned())
}

/// A local station's answer to one HTTP request.
pub(crate) struct Reply {
    pub status: u16,
    pub body: Vec<u8>,
    /// Media type; downloads in the browser keep it.
    #[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]
    pub content_type: String,
}

impl Reply {
    pub(crate) fn text(&self) -> String {
        String::from_utf8_lossy(&self.body).into_owned()
    }
    pub(crate) fn ok(&self) -> bool {
        (200..300).contains(&self.status)
    }
}

/// The local station's answer to `method path` (path with query), or `None`
/// when the remote station is selected.
pub(crate) fn request(method: &str, path: &str, body: Option<&[u8]>) -> Option<Reply> {
    let station = local()?;
    Some(station.request(method, path, body))
}

/// [`request`] for a URL built from the base URL (auth and probes): only
/// URLs under [`LOCAL_BASE`] are answered locally.
pub(crate) fn request_url(method: &str, url: &str, body: Option<&[u8]>) -> Option<Reply> {
    let path = url.strip_prefix(LOCAL_BASE)?;
    let station = local()?;
    Some(station.request(method, if path.is_empty() { "/" } else { path }, body))
}
