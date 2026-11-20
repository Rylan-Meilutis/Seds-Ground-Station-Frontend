//! Watch settings: one document edited on the phone or the watch, kept as a
//! single local copy on the phone and synced to the signed-in account.
//! Conflicts resolve last-writer-wins on `updated_ms`, arbitrated by the
//! account revision when signed in (see `account_sync`).
//!
//! Version 2 adds per-page `items` (which rows an `items` page shows, in
//! order) and `actions` (at most two opt-in action buttons per page) plus
//! `direct_link`. A version 1 document migrates to the same layout it had:
//! every page gets its registry rows and no actions.
use super::registry::{self, PageKind};
use serde::{Deserialize, Serialize};

pub const SETTINGS_VERSION: u32 = 2;
pub const MIN_INTERVAL_S: u32 = 2;
pub const MAX_INTERVAL_S: u32 = 60;
pub const MIN_STALE_S: u32 = 5;
pub const MAX_STALE_S: u32 = 600;
pub const MAX_CYCLE_ITEMS: usize = 12;
/// Rows on one `items` page.
pub const MAX_PAGE_ITEMS: usize = 8;
/// Action buttons on one page. Mirrors the GS `watch::MAX_ACTIONS_PER_PAGE`.
pub const MAX_PAGE_ACTIONS: usize = 2;
/// Upper bound on the serialized document; the backend enforces the same.
pub const MAX_SETTINGS_BYTES: usize = 8 * 1024;
/// Map background tile set size cap, MB.
pub const MIN_MAP_CAP_MB: u32 = 1;
pub const MAX_MAP_CAP_MB: u32 = 5;
pub const DEFAULT_MAP_CAP_MB: u32 = 3;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PageSetting {
    pub id: String,
    pub enabled: bool,
    /// `items` pages: the rows to show, in order (registry item ids).
    /// Empty for other page kinds. Missing in a version 1 document, which
    /// `sanitize` fills with the page's registry rows.
    #[serde(default)]
    pub items: Vec<String>,
    /// Opt-in action buttons (GS command names), at most
    /// [`MAX_PAGE_ACTIONS`]. Off by default. Only commands the GS lists as
    /// watch-eligible and permitted for the account are ever shown; see
    /// `actions::permitted`.
    #[serde(default)]
    pub actions: Vec<String>,
}

impl PageSetting {
    /// The page as it was before version 2: registry rows, no actions.
    pub fn default_for(def: &registry::PageDef) -> Self {
        Self {
            id: def.id.into(),
            enabled: def.default_enabled,
            items: def.items.iter().map(|i| (*i).to_string()).collect(),
            actions: Vec::new(),
        }
    }
}

/// A GS command name as the watch may carry it: short ASCII identifier.
pub fn command_name_valid(cmd: &str) -> bool {
    (1..=64).contains(&cmd.len()) && cmd.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_')
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

/// Map background under the compass (docs/watch-companion.md "Map
/// background"): station map tiles around the GS, the rocket and the RRLS,
/// sent by the phone and cached on the watch.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct MapSetting {
    pub enabled: bool,
    /// Size cap of the tile set the phone sends, and of the watch's tile
    /// cache, in MB.
    pub cap_mb: u32,
}

impl Default for MapSetting {
    fn default() -> Self {
        Self {
            enabled: true,
            cap_mb: DEFAULT_MAP_CAP_MB,
        }
    }
}

impl MapSetting {
    #[cfg_attr(target_arch = "wasm32", allow(dead_code))]
    pub fn cap_bytes(&self) -> u64 {
        u64::from(self.cap_mb) * 1024 * 1024
    }
}

/// Crew voice on the watch (docs/watch-companion.md "Voice").
pub const MAX_VOICE_VOLUME: u32 = 150;
pub const DEFAULT_VOICE_VOLUME: u32 = 100;

/// Which channel the watch joins. `CrewRadio` also relays its push-to-talk
/// over the recovery radio when the station offers it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum VoiceChannel {
    CrewRadio,
    /// Also any value this build does not know (`serde(other)` must be last).
    #[default]
    #[serde(other)]
    Crew,
}

/// Press-and-hold or tap-to-toggle.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum PttMode {
    Toggle,
    #[default]
    #[serde(other)]
    Hold,
}

/// Where the watch plays and records voice.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum AudioRoute {
    /// The watch speaker and microphone even with a headset connected.
    Speaker,
    /// Prefer a Bluetooth headset (AirPods etc.).
    Bluetooth,
    /// A connected Bluetooth headset, else the watch.
    #[default]
    #[serde(other)]
    Auto,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct VoiceSetting {
    /// Ask the station for crew voice in the watch token and offer voice on
    /// the watch. The station grants it only to accounts with the voice
    /// transmit permission.
    pub enabled: bool,
    pub channel: VoiceChannel,
    pub ptt: PttMode,
    /// Tap on push-to-talk start and stop.
    pub haptics: bool,
    /// Tap when someone else starts talking.
    pub talker_haptics: bool,
    pub route: AudioRoute,
    /// Playback volume, percent (0–150); the crown changes it on the watch.
    pub volume: u32,
}

impl Default for VoiceSetting {
    fn default() -> Self {
        Self {
            enabled: true,
            channel: VoiceChannel::Crew,
            ptt: PttMode::Hold,
            haptics: true,
            talker_haptics: false,
            route: AudioRoute::Auto,
            volume: DEFAULT_VOICE_VOLUME,
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
    /// Let the watch read the station itself (with a scoped watch token)
    /// while the phone app is suspended or out of reach.
    pub direct_link: bool,
    /// Map background under the compass.
    pub map: MapSetting,
    /// Crew voice (push-to-talk) on the watch.
    pub voice: VoiceSetting,
}

impl Default for WatchSettings {
    fn default() -> Self {
        Self {
            version: SETTINGS_VERSION,
            updated_ms: 0,
            updated_by: Editor::Phone,
            pages: registry::PAGES.iter().map(PageSetting::default_for).collect(),
            cycle: CycleSetting::default(),
            compass: CompassSetting::default(),
            units: Units::Metric,
            power: Power::Balanced,
            direct_link: true,
            map: MapSetting::default(),
            voice: VoiceSetting::default(),
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
                self.pages.push(PageSetting::default_for(p));
            }
        }
        for page in &mut self.pages {
            let Some(def) = registry::page(&page.id) else {
                continue;
            };
            if def.kind == PageKind::Items {
                dedupe_known(&mut page.items, |id| registry::item(id).is_some(), MAX_PAGE_ITEMS);
                if page.items.is_empty() {
                    // Version 1 documents (and an emptied page) get the
                    // page's registry rows: the layout they always had.
                    page.items = PageSetting::default_for(def).items;
                }
            } else {
                page.items.clear();
            }
            dedupe_known(&mut page.actions, command_name_valid, MAX_PAGE_ACTIONS);
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
        self.map.cap_mb = self.map.cap_mb.clamp(MIN_MAP_CAP_MB, MAX_MAP_CAP_MB);
        self.voice.volume = self.voice.volume.min(MAX_VOICE_VOLUME);
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

    /// Crew voice is wanted on the watch: switched on and on an enabled page.
    pub fn voice_wanted(&self) -> bool {
        self.voice.enabled && self.enabled_pages().any(|p| p.kind == PageKind::Voice)
    }

    pub fn compass_enabled(&self) -> bool {
        self.enabled_pages().any(|p| p.kind == PageKind::Compass)
    }

    pub fn metric(&self) -> bool {
        self.units == Units::Metric
    }

    /// Every action configured on an enabled page, deduplicated in page
    /// order (before filtering against the GS catalog).
    #[cfg(test)]
    pub fn configured_actions(&self) -> Vec<String> {
        let mut out: Vec<String> = Vec::new();
        for page in self.pages.iter().filter(|p| p.enabled) {
            for cmd in &page.actions {
                if !out.contains(cmd) {
                    out.push(cmd.clone());
                }
            }
        }
        out
    }

    #[cfg(test)]
    pub fn page_setting(&self, id: &str) -> Option<&PageSetting> {
        self.pages.iter().find(|p| p.id == id)
    }

    /// Adds or removes a row on an `items` page; never empties the page,
    /// never exceeds [`MAX_PAGE_ITEMS`].
    pub fn toggle_page_item(&mut self, page: &str, item: &str) {
        let Some(p) = self.pages.iter_mut().find(|p| p.id == page) else {
            return;
        };
        if registry::page(page).map(|d| d.kind) != Some(PageKind::Items) {
            return;
        }
        if let Some(pos) = p.items.iter().position(|i| i == item) {
            if p.items.len() > 1 {
                p.items.remove(pos);
            }
        } else if p.items.len() < MAX_PAGE_ITEMS && registry::item(item).is_some() {
            p.items.push(item.into());
        }
    }

    pub fn move_page_item(&mut self, page: &str, item: &str, up: bool) {
        if let Some(p) = self.pages.iter_mut().find(|p| p.id == page) {
            move_in(&mut p.items, |i| i == item, up);
        }
    }

    /// Adds or removes an action button on a page. Adding is refused once
    /// the page has [`MAX_PAGE_ACTIONS`]; returns whether anything changed.
    pub fn toggle_page_action(&mut self, page: &str, cmd: &str) -> bool {
        let Some(p) = self.pages.iter_mut().find(|p| p.id == page) else {
            return false;
        };
        if let Some(pos) = p.actions.iter().position(|c| c == cmd) {
            p.actions.remove(pos);
            true
        } else if p.actions.len() < MAX_PAGE_ACTIONS && command_name_valid(cmd) {
            p.actions.push(cmd.into());
            true
        } else {
            false
        }
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
                enabled: true,
                items: vec![],
                actions: vec![],
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
        // Documents without a map section get the default (on, 3 MB).
        assert_eq!(s.map, MapSetting::default());
        let s = WatchSettings::parse(r#"{"map":{"enabled":false,"cap_mb":99}}"#).unwrap();
        assert!(!s.map.enabled);
        assert_eq!(s.map.cap_mb, MAX_MAP_CAP_MB);
        let s = WatchSettings::parse(r#"{"map":{"cap_mb":0}}"#).unwrap();
        assert!(s.map.enabled);
        assert_eq!(s.map.cap_bytes(), 1024 * 1024);
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
    fn voice_settings_default_parse_and_clamp() {
        let d = WatchSettings::default();
        assert!(d.voice.enabled && d.voice_wanted());
        assert_eq!(d.voice.ptt, PttMode::Hold);
        assert_eq!(d.voice.volume, DEFAULT_VOICE_VOLUME);
        assert_eq!(d.pages.last().unwrap().id, "voice");
        // Unknown values from a newer watch fall back instead of failing
        // the whole document; volume is clamped.
        let s = WatchSettings::parse(
            r#"{"voice":{"enabled":true,"channel":"crew_radio","ptt":"toggle","haptics":false,
                "talker_haptics":true,"route":"bluetooth","volume":999}}"#,
        )
        .unwrap();
        assert_eq!(s.voice.channel, VoiceChannel::CrewRadio);
        assert_eq!(s.voice.ptt, PttMode::Toggle);
        assert_eq!(s.voice.route, AudioRoute::Bluetooth);
        assert!(!s.voice.haptics && s.voice.talker_haptics);
        assert_eq!(s.voice.volume, MAX_VOICE_VOLUME);
        let s = WatchSettings::parse(r#"{"voice":{"channel":"moon","ptt":"shout","route":"laser"}}"#).unwrap();
        assert_eq!(s.voice, VoiceSetting::default());
        // A document from before voice gets the voice page (appended, on) and defaults.
        let s = WatchSettings::parse(V1_DEFAULT).unwrap();
        assert_eq!(s.voice, VoiceSetting::default());
        assert!(s.page_setting("voice").unwrap().enabled);
        // Voice is wanted only while on and on an enabled page.
        let mut s = WatchSettings::default();
        s.toggle_page("voice");
        assert!(!s.voice_wanted());
        s.toggle_page("voice");
        s.voice.enabled = false;
        assert!(!s.voice_wanted());
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

    /// The settings document the phone stored before page configuration
    /// (version 1): no `items`, `actions` or `direct_link`.
    const V1_DEFAULT: &str = r#"{"version":1,"updated_ms":1730000000000,"updated_by":"phone",
        "pages":[{"id":"flight","enabled":true},{"id":"propulsion","enabled":true},{"id":"trajectory","enabled":true},
                 {"id":"health","enabled":true},{"id":"cycle","enabled":true},{"id":"compass","enabled":true}],
        "cycle":{"items":["flight_state","t_clock","altitude","velocity","tank_pressure"],"interval_s":5,"autostart":true},
        "compass":{"north":"true","stale_after_s":30,"targets":["rocket","ground_station","rrls"]},
        "units":"metric","power":"balanced"}"#;

    #[test]
    fn version_1_documents_migrate_to_the_layout_they_had() {
        let migrated = WatchSettings::parse(V1_DEFAULT).unwrap();
        let mut expected = WatchSettings::default();
        expected.updated_ms = 1_730_000_000_000;
        assert_eq!(migrated, expected);
        assert_eq!(migrated.version, 2);
        assert!(migrated.direct_link);
        // Items pages carry the registry rows they always showed; no actions.
        for page in &migrated.pages {
            let def = registry::page(&page.id).unwrap();
            assert_eq!(page.items, def.items.iter().map(|i| i.to_string()).collect::<Vec<_>>());
            assert!(page.actions.is_empty());
        }
        // The resolved pages equal the version 1 layout, plus the voice page
        // that later builds append.
        let pages = super::super::protocol::resolved_pages(&migrated, &[]);
        let ids: Vec<_> = pages.iter().map(|p| p.id).collect();
        assert_eq!(ids, vec!["flight", "propulsion", "trajectory", "health", "cycle", "compass", "voice"]);
        assert_eq!(pages[0].items, vec!["flight_state", "t_clock", "altitude", "velocity"]);
        // A disabled v1 page stays disabled.
        let disabled = WatchSettings::parse(&V1_DEFAULT.replace(
            r#"{"id":"health","enabled":true}"#,
            r#"{"id":"health","enabled":false}"#,
        ))
        .unwrap();
        assert!(!disabled.page_setting("health").unwrap().enabled);
    }

    #[test]
    fn page_items_and_actions_are_validated() {
        let raw = r#"{"pages":[
            {"id":"flight","enabled":true,"items":["altitude","altitude","bogus","velocity"],"actions":["Abort","Dump","Nitrogen","Abort"]},
            {"id":"cycle","enabled":true,"items":["altitude"],"actions":["bad name!","Dump"]},
            {"id":"propulsion","enabled":true,"items":[]}]}"#;
        let s = WatchSettings::parse(raw).unwrap();
        let flight = s.page_setting("flight").unwrap();
        assert_eq!(flight.items, vec!["altitude", "velocity"]);
        // Two actions per page at most; duplicates and invalid names dropped.
        assert_eq!(flight.actions, vec!["Abort", "Dump"]);
        let cycle = s.page_setting("cycle").unwrap();
        assert!(cycle.items.is_empty(), "only items pages carry rows");
        assert_eq!(cycle.actions, vec!["Dump"]);
        // An emptied items page falls back to its registry rows.
        assert_eq!(s.page_setting("propulsion").unwrap().items.len(), 4);
        let many: Vec<_> = registry::ITEMS.iter().map(|i| i.id).collect();
        let s = WatchSettings::parse(&serde_json::json!({"pages":[{"id":"health","enabled":true,"items":many}]}).to_string()).unwrap();
        assert_eq!(s.page_setting("health").unwrap().items.len(), MAX_PAGE_ITEMS);
        assert_eq!(s.configured_actions(), Vec::<String>::new());
        // Every page full of actions still fits the account document.
        let mut full = WatchSettings::default();
        for page in &mut full.pages {
            page.actions = vec!["ToggleAvBayUnderglow".into(), "ToggleFlightBuzzer".into()];
            if !page.items.is_empty() {
                page.items = many.iter().take(MAX_PAGE_ITEMS).map(|i| i.to_string()).collect();
            }
        }
        assert!(serde_json::to_vec(&full).unwrap().len() < MAX_SETTINGS_BYTES / 2);
    }

    #[test]
    fn two_action_limit_is_enforced_by_the_editor() {
        let mut s = WatchSettings::default();
        assert!(s.toggle_page_action("flight", "Abort"));
        assert!(s.toggle_page_action("flight", "Dump"));
        assert!(!s.toggle_page_action("flight", "Nitrogen"), "third action refused");
        assert_eq!(s.page_setting("flight").unwrap().actions, vec!["Abort", "Dump"]);
        assert!(s.toggle_page_action("flight", "Abort"));
        assert!(s.toggle_page_action("flight", "Nitrogen"));
        assert_eq!(s.page_setting("flight").unwrap().actions, vec!["Dump", "Nitrogen"]);
        assert!(!s.toggle_page_action("flight", "not valid"));
        assert!(!s.toggle_page_action("nope", "Abort"));
        assert!(s.toggle_page_action("compass", "Dump"));
        // Disabled pages contribute no actions.
        s.toggle_page("compass");
        assert_eq!(s.configured_actions(), vec!["Dump", "Nitrogen"]);
    }

    #[test]
    fn page_item_editing_keeps_invariants() {
        let mut s = WatchSettings::default();
        s.toggle_page_item("flight", "tank_pressure");
        s.move_page_item("flight", "tank_pressure", true);
        assert_eq!(
            s.page_setting("flight").unwrap().items,
            vec!["flight_state", "t_clock", "altitude", "tank_pressure", "velocity"]
        );
        for id in ["flight_state", "t_clock", "altitude", "tank_pressure", "velocity"] {
            s.toggle_page_item("flight", id);
        }
        assert_eq!(s.page_setting("flight").unwrap().items, vec!["velocity"], "never empties");
        s.toggle_page_item("flight", "bogus");
        s.toggle_page_item("cycle", "altitude");
        assert_eq!(s.page_setting("flight").unwrap().items, vec!["velocity"]);
        assert!(s.page_setting("cycle").unwrap().items.is_empty());
        for item in registry::ITEMS {
            s.toggle_page_item("flight", item.id);
        }
        assert!(s.page_setting("flight").unwrap().items.len() <= MAX_PAGE_ITEMS);
    }

    #[test]
    fn editing_helpers_keep_invariants() {
        let mut s = WatchSettings::default();
        s.move_page("flight", true); // already first
        assert_eq!(s.pages[0].id, "flight");
        s.move_page("compass", true);
        assert_eq!(s.pages[s.pages.len() - 3].id, "compass");
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
