//! Watch settings: one document edited on the phone or the watch, kept as a
//! single local copy on the phone and synced to the signed-in account.
//! Conflicts resolve last-writer-wins on `updated_ms`, arbitrated by the
//! account revision when signed in (see `account_sync`).
use super::registry::{self, PageKind};
use serde::{Deserialize, Serialize};

pub const SETTINGS_VERSION: u32 = 1;
pub const MIN_INTERVAL_S: u32 = 2;
pub const MAX_INTERVAL_S: u32 = 60;
pub const MIN_STALE_S: u32 = 5;
pub const MAX_STALE_S: u32 = 600;
pub const MAX_CYCLE_ITEMS: usize = 12;
/// Upper bound on the serialized document; the backend enforces the same.
pub const MAX_SETTINGS_BYTES: usize = 8 * 1024;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PageSetting {
    pub id: String,
    pub enabled: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct CycleSetting {
    pub items: Vec<String>,
    pub interval_s: u32,
    /// Start cycling as soon as the cycle page is shown.
    pub autostart: bool,
}

impl Default for CycleSetting {
    fn default() -> Self {
        Self {
            items: [
                "flight_state",
                "t_clock",
                "altitude",
                "velocity",
                "tank_pressure",
            ]
            .map(String::from)
            .to_vec(),
            interval_s: 5,
            autostart: true,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum North {
    #[default]
    True,
    Magnetic,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct CompassSetting {
    pub north: North,
    /// A target fix older than this is shown as stale.
    pub stale_after_s: u32,
    pub targets: Vec<String>,
}

impl Default for CompassSetting {
    fn default() -> Self {
        Self {
            north: North::True,
            stale_after_s: 30,
            targets: registry::TARGETS
                .iter()
                .map(|(id, _)| (*id).to_string())
                .collect(),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum Units {
    #[default]
    Metric,
    Imperial,
}

/// Update cadence. `Recovery` trades battery for the fastest compass data.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum Power {
    Saver,
    #[default]
    Balanced,
    Recovery,
}

impl Power {
    /// Phone→watch snapshot interval while the watch app is in the foreground.
    pub fn active_interval_ms(self) -> u64 {
        match self {
            Power::Saver => 5_000,
            Power::Balanced => 2_000,
            Power::Recovery => 1_000,
        }
    }
    /// Interval while the watch app is backgrounded (latest-state only).
    pub fn idle_interval_ms(self) -> u64 {
        match self {
            Power::Saver => 60_000,
            Power::Balanced => 30_000,
            Power::Recovery => 10_000,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum Editor {
    #[default]
    Phone,
    Watch,
    Account,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct WatchSettings {
    pub version: u32,
    pub updated_ms: i64,
    pub updated_by: Editor,
    /// Display order; every registry page appears exactly once.
    pub pages: Vec<PageSetting>,
    pub cycle: CycleSetting,
    pub compass: CompassSetting,
    pub units: Units,
    pub power: Power,
}

impl Default for WatchSettings {
    fn default() -> Self {
        Self {
            version: SETTINGS_VERSION,
            updated_ms: 0,
            updated_by: Editor::Phone,
            pages: registry::PAGES
                .iter()
                .map(|p| PageSetting {
                    id: p.id.into(),
                    enabled: p.default_enabled,
                })
                .collect(),
            cycle: CycleSetting::default(),
            compass: CompassSetting::default(),
            units: Units::Metric,
            power: Power::Balanced,
        }
    }
}

fn dedupe_known(ids: &mut Vec<String>, known: impl Fn(&str) -> bool, max: usize) {
    let mut seen = std::collections::HashSet::new();
    ids.retain(|id| known(id) && seen.insert(id.clone()));
    ids.truncate(max);
}

impl WatchSettings {
    /// Makes any decoded document valid against this build's registry:
    /// drops unknown/duplicate ids, appends new registry pages, clamps ranges.
    /// Unknown ids from a newer app are dropped rather than preserved; the
    /// registry is phone-owned so the newest phone defines the vocabulary.
    pub fn sanitize(mut self) -> Self {
        self.version = SETTINGS_VERSION;
        let mut seen = std::collections::HashSet::new();
        self.pages
            .retain(|p| registry::page(&p.id).is_some() && seen.insert(p.id.clone()));
        for p in registry::PAGES {
            if !self.pages.iter().any(|s| s.id == p.id) {
                self.pages.push(PageSetting {
                    id: p.id.into(),
                    enabled: p.default_enabled,
                });
            }
        }
        if !self.pages.iter().any(|p| p.enabled) {
            // A watch with no pages is useless; fall back to the compass.
            if let Some(p) = self.pages.iter_mut().find(|p| p.id == "compass") {
                p.enabled = true;
            }
        }
        dedupe_known(
            &mut self.cycle.items,
            |id| registry::item(id).is_some(),
            MAX_CYCLE_ITEMS,
        );
        if self.cycle.items.is_empty() {
            self.cycle.items = CycleSetting::default().items;
        }
        self.cycle.interval_s = self.cycle.interval_s.clamp(MIN_INTERVAL_S, MAX_INTERVAL_S);
        self.compass.stale_after_s = self.compass.stale_after_s.clamp(MIN_STALE_S, MAX_STALE_S);
        dedupe_known(
            &mut self.compass.targets,
            registry::target_known,
            registry::TARGETS.len(),
        );
        if self.compass.targets.is_empty() {
            self.compass.targets = CompassSetting::default().targets;
        }
        self.updated_ms = self.updated_ms.max(0);
        self
    }

    /// Parses untrusted JSON.
    #[cfg(test)]
    pub fn parse(json: &str) -> Option<Self> {
        if json.len() > MAX_SETTINGS_BYTES {
            return None;
        }
        serde_json::from_str::<WatchSettings>(json)
            .ok()
            .map(Self::sanitize)
    }

    pub fn from_value(value: serde_json::Value) -> Option<Self> {
        let bytes = serde_json::to_vec(&value).ok()?;
        if bytes.len() > MAX_SETTINGS_BYTES {
            return None;
        }
        serde_json::from_value::<WatchSettings>(value)
            .ok()
            .map(Self::sanitize)
    }

    /// Same user-visible content, ignoring edit metadata.
    pub fn same_content(&self, other: &Self) -> bool {
        let mut a = self.clone();
        let mut b = other.clone();
        a.updated_ms = 0;
        b.updated_ms = 0;
        a.updated_by = Editor::Phone;
        b.updated_by = Editor::Phone;
        a == b
    }

    /// Stamps an edit. The time never goes backwards relative to the previous
    /// document so a slow clock on one device cannot lose its own edit.
    pub fn stamped(mut self, previous_ms: i64, now_ms: i64, by: Editor) -> Self {
        self.updated_ms = now_ms.max(previous_ms.saturating_add(1));
        self.updated_by = by;
        self
    }

    /// Last-writer-wins: true when `self` should replace `other`.
    pub fn newer_than(&self, other: &Self) -> bool {
        self.updated_ms > other.updated_ms
    }

    pub fn enabled_pages(&self) -> impl Iterator<Item = &'static registry::PageDef> + '_ {
        self.pages
            .iter()
            .filter(|p| p.enabled)
            .filter_map(|p| registry::page(&p.id))
    }

    pub fn compass_enabled(&self) -> bool {
        self.enabled_pages().any(|p| p.kind == PageKind::Compass)
    }

    pub fn metric(&self) -> bool {
        self.units == Units::Metric
    }

    // --- Editing helpers shared by the phone settings section. ---

    pub fn move_page(&mut self, id: &str, up: bool) {
        move_in(&mut self.pages, |p| p.id == id, up);
    }

    pub fn toggle_page(&mut self, id: &str) {
        if let Some(p) = self.pages.iter_mut().find(|p| p.id == id) {
            p.enabled = !p.enabled;
        }
    }

    pub fn toggle_cycle_item(&mut self, id: &str) {
        if let Some(pos) = self.cycle.items.iter().position(|i| i == id) {
            if self.cycle.items.len() > 1 {
                self.cycle.items.remove(pos);
            }
        } else if self.cycle.items.len() < MAX_CYCLE_ITEMS && registry::item(id).is_some() {
            self.cycle.items.push(id.into());
        }
    }

    pub fn move_cycle_item(&mut self, id: &str, up: bool) {
        move_in(&mut self.cycle.items, |i| i == id, up);
    }

    pub fn toggle_target(&mut self, id: &str) {
        if let Some(pos) = self.compass.targets.iter().position(|t| t == id) {
            if self.compass.targets.len() > 1 {
                self.compass.targets.remove(pos);
            }
        } else if registry::target_known(id) {
            self.compass.targets.push(id.into());
        }
    }
}

fn move_in<T>(list: &mut [T], pred: impl Fn(&T) -> bool, up: bool) {
    let Some(i) = list.iter().position(pred) else {
        return;
    };
    if up && i > 0 {
        list.swap(i, i - 1);
    } else if !up && i + 1 < list.len() {
        list.swap(i, i + 1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_are_already_sanitized() {
        let d = WatchSettings::default();
        assert_eq!(d.clone().sanitize(), d);
        assert!(serde_json::to_vec(&d).unwrap().len() < MAX_SETTINGS_BYTES);
    }

    #[test]
    fn sanitize_drops_unknown_duplicates_and_clamps() {
        let raw = r#"{"pages":[{"id":"compass","enabled":true},{"id":"nope","enabled":true},{"id":"compass","enabled":false}],
            "cycle":{"items":["altitude","altitude","bogus","velocity"],"interval_s":0},
            "compass":{"north":"magnetic","stale_after_s":99999,"targets":["rrls","x"]},
            "units":"imperial","power":"recovery","future_field":1}"#;
        let s = WatchSettings::parse(raw).unwrap();
        assert_eq!(
            s.pages[0],
            PageSetting {
                id: "compass".into(),
                enabled: true
            }
        );
        assert_eq!(s.pages.len(), registry::PAGES.len());
        assert_eq!(s.cycle.items, vec!["altitude", "velocity"]);
        assert_eq!(s.cycle.interval_s, MIN_INTERVAL_S);
        assert_eq!(s.compass.stale_after_s, MAX_STALE_S);
        assert_eq!(s.compass.targets, vec!["rrls"]);
        assert_eq!(s.compass.north, North::Magnetic);
        assert_eq!(s.units, Units::Imperial);
        assert_eq!(s.power, Power::Recovery);
    }

    #[test]
    fn empty_or_hostile_input_is_rejected_or_repaired() {
        assert!(WatchSettings::parse("not json").is_none());
        assert!(WatchSettings::parse(&"x".repeat(MAX_SETTINGS_BYTES + 1)).is_none());
        let s =
            WatchSettings::parse(r#"{"pages":[],"cycle":{"items":[]},"compass":{"targets":[]}}"#)
                .unwrap();
        assert!(s.pages.iter().any(|p| p.enabled));
        assert!(!s.cycle.items.is_empty());
        assert!(!s.compass.targets.is_empty());
        let none_enabled = WatchSettings::parse(
            &serde_json::json!({"pages": registry::PAGES.iter().map(|p| serde_json::json!({"id":p.id,"enabled":false})).collect::<Vec<_>>()}).to_string(),
        )
        .unwrap();
        assert_eq!(
            none_enabled
                .enabled_pages()
                .map(|p| p.id)
                .collect::<Vec<_>>(),
            vec!["compass"]
        );
    }

    #[test]
    fn stamping_is_monotonic_and_lww_compares_time() {
        let a = WatchSettings::default().stamped(0, 1_000, Editor::Phone);
        let b = a.clone().stamped(a.updated_ms, 500, Editor::Watch); // watch clock behind
        assert_eq!(b.updated_ms, 1_001);
        assert!(b.newer_than(&a));
        assert!(!a.newer_than(&b));
        assert!(a.same_content(&b));
    }

    #[test]
    fn editing_helpers_keep_invariants() {
        let mut s = WatchSettings::default();
        s.move_page("flight", true); // already first
        assert_eq!(s.pages[0].id, "flight");
        s.move_page("compass", true);
        assert_eq!(s.pages[s.pages.len() - 2].id, "compass");
        s.cycle.items = vec!["altitude".into()];
        s.toggle_cycle_item("altitude"); // never empties
        assert_eq!(s.cycle.items, vec!["altitude"]);
        s.toggle_cycle_item("velocity");
        s.move_cycle_item("velocity", true);
        assert_eq!(s.cycle.items, vec!["velocity", "altitude"]);
        s.toggle_cycle_item("unknown");
        assert_eq!(s.cycle.items.len(), 2);
        s.compass.targets = vec!["rocket".into()];
        s.toggle_target("rocket");
        assert_eq!(s.compass.targets, vec!["rocket"]);
    }
}
