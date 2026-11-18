//! Phone ⇄ watch wire format (version 1). See docs/watch-companion.md.
//! The phone renders values to text so watch apps stay generic; ages are
//! relative to the send instant so watches never compare clocks.
use super::registry;
use super::settings::WatchSettings;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

pub const PROTOCOL_VERSION: u32 = 1;
/// Largest message either side will accept (WatchConnectivity and the Wear
/// Data Layer both allow far more; this keeps radio time short).
pub const MAX_MESSAGE_BYTES: usize = 32 * 1024;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Mode {
    /// Connected to a real GroundStation or RRLS.
    Live,
    /// Phone is showing the offline preview; every value is example data.
    Demo,
    /// Phone app is running but not connected to any station.
    NoStation,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Level {
    Normal,
    Warn,
    Alert,
    Unknown,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Station {
    pub name: String,
    /// `primary` (GroundStation), `rrls`, or `unknown`.
    pub role: String,
    /// `connected`, `connecting` (first connection in progress),
    /// `reconnecting`, `sign_in` (the station needs a login on the phone) or
    /// `offline`.
    pub link: &'static str,
    /// Host (and port) of the station the phone uses; empty when none is
    /// chosen.
    pub address: String,
    pub telemetry_age_ms: Option<i64>,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Clock {
    /// `idle`, `tminus`, or `tplus`.
    pub kind: &'static str,
    /// Remaining (tminus) or elapsed (tplus) milliseconds at send time.
    pub ms: i64,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct ItemValue {
    pub label: &'static str,
    pub text: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub value: Option<f64>,
    pub unit: &'static str,
    pub age_ms: Option<i64>,
    pub stale: bool,
    pub level: Level,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub clock: Option<Clock>,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct DemoOffset {
    pub north_m: f64,
    pub east_m: f64,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Target {
    pub id: &'static str,
    pub label: String,
    pub lat: f64,
    pub lon: f64,
    pub alt_m: Option<f64>,
    pub age_ms: i64,
    pub stale: bool,
    pub source: String,
    /// Saved/manual position rather than a live GPS fix.
    pub fallback: bool,
    /// Demo only: place the target relative to the watch's own position.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub demo_offset: Option<DemoOffset>,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Page {
    pub id: &'static str,
    pub kind: &'static str,
    pub title: &'static str,
    /// Rows of an `items` page, in the user's order.
    pub items: Vec<String>,
    /// Opt-in action buttons (at most two), already filtered to what the GS
    /// offers this account on the watch.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub actions: Vec<PageAction>,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct PageAction {
    pub cmd: String,
    pub label: String,
    /// Abort: one tap plus confirm, never disabled on the watch.
    pub abort: bool,
}

/// The GS's enable state for one page action.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct ActionState {
    pub enabled: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct CatalogItem {
    pub id: &'static str,
    pub label: &'static str,
    pub group: &'static str,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct CatalogTarget {
    pub id: &'static str,
    pub label: &'static str,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Catalog {
    pub items: Vec<CatalogItem>,
    pub pages: Vec<Page>,
    pub targets: Vec<CatalogTarget>,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Account {
    pub signed_in: bool,
    /// `synced`, `pending`, `local_only`, or `error`.
    pub sync: &'static str,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Snapshot {
    pub v: u32,
    pub seq: u64,
    pub mode: Mode,
    pub station: Station,
    pub account: Account,
    pub items: BTreeMap<&'static str, ItemValue>,
    pub pages: Vec<Page>,
    pub targets: Vec<Target>,
    pub settings: WatchSettings,
    pub catalog: Catalog,
    /// Enable state of every action on `pages`, from the phone's action
    /// policy. Absent when no page has actions.
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub actions: BTreeMap<String, ActionState>,
    /// Seconds after which the watch should treat the phone link as lost.
    pub link_timeout_s: u64,
}

pub fn catalog() -> Catalog {
    Catalog {
        items: registry::ITEMS
            .iter()
            .map(|i| CatalogItem {
                id: i.id,
                label: i.label,
                group: i.group,
            })
            .collect(),
        pages: registry::PAGES.iter().map(page).collect(),
        targets: registry::TARGETS
            .iter()
            .map(|(id, label)| CatalogTarget { id, label })
            .collect(),
    }
}

pub fn page(p: &'static registry::PageDef) -> Page {
    Page {
        id: p.id,
        kind: p.kind.wire(),
        title: p.title,
        items: p.items.iter().map(|i| (*i).to_string()).collect(),
        actions: Vec::new(),
    }
}

/// Enabled pages in the user's order with their configured rows and the
/// configured actions that `catalog` (the GS's watch action list for this
/// account) offers. An empty catalog shows no actions at all.
pub fn resolved_pages(
    settings: &WatchSettings,
    catalog: &[super::actions::CatalogAction],
) -> Vec<Page> {
    settings
        .pages
        .iter()
        .filter(|s| s.enabled)
        .filter_map(|s| {
            let def = registry::page(&s.id)?;
            let mut out = page(def);
            if def.kind == registry::PageKind::Items && !s.items.is_empty() {
                out.items = s.items.clone();
            }
            out.actions = super::actions::permitted(&s.actions, catalog)
                .into_iter()
                .map(|a| PageAction {
                    cmd: a.cmd.clone(),
                    label: a.label.clone(),
                    abort: a.cmd == "Abort",
                })
                .collect();
            Some(out)
        })
        .collect()
}

/// Messages the watch may send. Anything else is ignored. There is no
/// variant that can reach a station command path.
#[derive(Clone, Debug, PartialEq, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum WatchMessage {
    /// Ask for a full snapshot now (watch app opened).
    Hello,
    /// Watch app foreground state; raises or lowers the update rate.
    Activity {
        #[serde(default)]
        foreground: bool,
        #[serde(default)]
        page: Option<String>,
    },
    /// A settings edit made on the watch.
    Settings { settings: serde_json::Value },
}

impl WatchMessage {
    pub fn parse(json: &str) -> Option<Self> {
        if json.len() > MAX_MESSAGE_BYTES {
            return None;
        }
        let value: serde_json::Value = serde_json::from_str(json).ok()?;
        if value.get("v").and_then(|v| v.as_u64()) != Some(PROTOCOL_VERSION as u64) {
            return None;
        }
        serde_json::from_value(value).ok()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn watch_messages_parse_strictly() {
        assert_eq!(
            WatchMessage::parse(r#"{"v":1,"type":"hello"}"#),
            Some(WatchMessage::Hello)
        );
        assert_eq!(
            WatchMessage::parse(r#"{"v":1,"type":"activity","foreground":true,"page":"compass"}"#),
            Some(WatchMessage::Activity {
                foreground: true,
                page: Some("compass".into())
            })
        );
        assert!(matches!(
            WatchMessage::parse(r#"{"v":1,"type":"settings","settings":{"units":"imperial"}}"#),
            Some(WatchMessage::Settings { .. })
        ));
        // Wrong version, unknown types and command-shaped payloads are dropped.
        assert_eq!(WatchMessage::parse(r#"{"v":2,"type":"hello"}"#), None);
        assert_eq!(
            WatchMessage::parse(r#"{"v":1,"type":"command","cmd":"Launch"}"#),
            None
        );
        assert_eq!(WatchMessage::parse(r#"{"type":"hello"}"#), None);
        assert_eq!(
            WatchMessage::parse(&format!(
                r#"{{"v":1,"type":"hello","pad":"{}"}}"#,
                "x".repeat(MAX_MESSAGE_BYTES)
            )),
            None
        );
    }

    #[test]
    fn catalog_and_pages_follow_settings_order() {
        let mut s = WatchSettings::default();
        s.move_page("compass", true);
        s.toggle_page("propulsion");
        let pages = resolved_pages(&s, &[]);
        assert!(pages.iter().all(|p| p.id != "propulsion"));
        let ids: Vec<_> = pages.iter().map(|p| p.id).collect();
        assert_eq!(
            ids,
            vec!["flight", "trajectory", "health", "compass", "cycle"]
        );
        assert_eq!(catalog().items.len(), registry::ITEMS.len());
    }
}
