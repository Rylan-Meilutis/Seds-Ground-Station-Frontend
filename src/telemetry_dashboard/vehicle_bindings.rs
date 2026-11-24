//! Vehicle-specific bindings a station names in its layout (`/api/layout`,
//! docs/backend-contract.md section 7): the GPS fix types, names of controls
//! that are not layout buttons, the fill-target fluids and the watch items'
//! channels. [`configure`] sets them whenever a layout is applied (live,
//! cached, demo or replay); anything the layout leaves out keeps the
//! built-in value, which is what a station from before these fields meant.
//! Nothing here is fetched: the layout already carries it.

use super::layout::{ControlLabel, FillTargetFluidSpec, GpsLayout, LayoutConfig, WatchItemBinding};
use std::sync::{PoisonError, RwLock};

/// GPS fix types when the layout names none, in preference order.
pub(crate) const BUILT_IN_GPS_TYPES: [&str; 3] = ["GPS_DATA", "GPS", "ROCKET_GPS"];
/// Fill-target fluids (`/api/fill_targets` keys) when the layout names none.
const BUILT_IN_FLUIDS: [(&str, &str); 2] = [("nitrogen", "Nitrogen"), ("nitrous", "Nitrous")];

struct Bindings {
    gps: Option<GpsLayout>,
    controls: Vec<ControlLabel>,
    watch_items: Vec<WatchItemBinding>,
}

static BINDINGS: RwLock<Bindings> =
    RwLock::new(Bindings { gps: None, controls: Vec::new(), watch_items: Vec::new() });

/// Adopts `layout`'s bindings (each absent one reverts to built-in).
pub(crate) fn configure(layout: &LayoutConfig) {
    let mut bindings = BINDINGS.write().unwrap_or_else(PoisonError::into_inner);
    bindings.gps = layout.gps.clone();
    bindings.controls = layout.actions_tab.controls.clone();
    bindings.watch_items = layout.watch_items.clone();
}

fn read<T>(f: impl FnOnce(&Bindings) -> T) -> T {
    f(&BINDINGS.read().unwrap_or_else(PoisonError::into_inner))
}

/// Where a GPS fix sits in its row.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct GpsIndices {
    pub lat: usize,
    pub lon: usize,
    pub alt: usize,
}

const BUILT_IN_GPS_INDICES: GpsIndices = GpsIndices { lat: 0, lon: 1, alt: 2 };

/// A layout GPS binding with at least one named type, else none.
fn named_gps(gps: Option<&GpsLayout>) -> Option<&GpsLayout> {
    gps.filter(|gps| gps.data_types.iter().any(|t| !t.trim().is_empty()))
}

fn gps_types_of(gps: Option<&GpsLayout>) -> Vec<String> {
    match named_gps(gps) {
        Some(gps) => gps.data_types.iter().map(|t| t.trim()).filter(|t| !t.is_empty()).map(str::to_owned).collect(),
        None => BUILT_IN_GPS_TYPES.iter().map(|t| (*t).to_owned()).collect(),
    }
}

fn gps_indices_of(gps: Option<&GpsLayout>) -> GpsIndices {
    named_gps(gps).map_or(BUILT_IN_GPS_INDICES, |gps| GpsIndices { lat: gps.lat_index, lon: gps.lon_index, alt: gps.alt_index })
}

/// The data types carrying a GPS fix, in preference order.
pub(crate) fn gps_types() -> Vec<String> {
    read(|b| gps_types_of(b.gps.as_ref()))
}

pub(crate) fn is_gps_type(data_type: &str) -> bool {
    read(|b| match named_gps(b.gps.as_ref()) {
        Some(gps) => gps.data_types.iter().any(|t| t.trim() == data_type),
        None => BUILT_IN_GPS_TYPES.contains(&data_type),
    })
}

pub(crate) fn gps_indices() -> GpsIndices {
    read(|b| gps_indices_of(b.gps.as_ref()))
}

fn control_label_in(controls: &[ControlLabel], cmd: &str) -> Option<String> {
    controls.iter().find(|c| c.cmd == cmd).map(|c| c.label.trim()).filter(|l| !l.is_empty()).map(str::to_owned)
}

/// The layout's name for a control that is not a layout button.
pub(crate) fn control_label(cmd: &str) -> Option<String> {
    read(|b| control_label_in(&b.controls, cmd))
}

/// `(id, label)` of each fill-target fluid to edit, in order: the layout's
/// list, else the built-in one.
pub(crate) fn fluids_of(fluids: &[FillTargetFluidSpec]) -> Vec<(String, String)> {
    let named: Vec<_> = fluids
        .iter()
        .filter(|f| !f.id.trim().is_empty())
        .map(|f| {
            let label = if f.label.trim().is_empty() { f.id.trim() } else { f.label.trim() };
            (f.id.trim().to_owned(), label.to_owned())
        })
        .collect();
    if named.is_empty() {
        BUILT_IN_FLUIDS.iter().map(|(id, label)| ((*id).to_owned(), (*label).to_owned())).collect()
    } else {
        named
    }
}

/// One telemetry channel: `(data_type, sender, index)`.
pub(crate) type Channel = (String, Option<String>, usize);

fn watch_channels_in(items: &[WatchItemBinding], id: &str) -> Option<Vec<Channel>> {
    items
        .iter()
        .find(|item| item.id == id)
        .map(|item| item.channels.iter().map(|c| (c.data_type.clone(), c.sender_id.clone(), c.index)).collect())
}

/// The layout's channels for watch item `id`, if it rebinds that item.
pub(crate) fn watch_channels(id: &str) -> Option<Vec<Channel>> {
    read(|b| watch_channels_in(&b.watch_items, id))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn gps(types: &[&str], indices: (usize, usize, usize)) -> GpsLayout {
        GpsLayout {
            data_types: types.iter().map(|t| (*t).to_owned()).collect(),
            lat_index: indices.0,
            lon_index: indices.1,
            alt_index: indices.2,
        }
    }

    #[test]
    fn gps_binding_comes_from_the_layout_with_the_built_in_types_as_fallback() {
        assert_eq!(gps_types_of(None), ["GPS_DATA", "GPS", "ROCKET_GPS"]);
        assert_eq!(gps_indices_of(None), GpsIndices { lat: 0, lon: 1, alt: 2 });
        let other = gps(&["NAV_FIX"], (1, 2, 0));
        assert_eq!(gps_types_of(Some(&other)), ["NAV_FIX"]);
        assert_eq!(gps_indices_of(Some(&other)), GpsIndices { lat: 1, lon: 2, alt: 0 });
        // A binding naming no type is no binding.
        let empty = gps(&[" "], (4, 5, 6));
        assert_eq!(gps_types_of(Some(&empty)), BUILT_IN_GPS_TYPES);
        assert_eq!(gps_indices_of(Some(&empty)), BUILT_IN_GPS_INDICES);
    }

    #[test]
    fn control_labels_and_fluids_fall_back_to_built_in() {
        let controls = vec![ControlLabel { cmd: "ToggleChecklistInterlock".into(), label: "Checklists".into() }];
        assert_eq!(control_label_in(&controls, "ToggleChecklistInterlock").as_deref(), Some("Checklists"));
        assert_eq!(control_label_in(&controls, "ToggleSupplyInterlock"), None);
        assert_eq!(fluids_of(&[]), [("nitrogen".to_owned(), "Nitrogen".to_owned()), ("nitrous".to_owned(), "Nitrous".to_owned())]);
        let lox = vec![FillTargetFluidSpec { id: "lox".into(), label: "".into() }];
        assert_eq!(fluids_of(&lox), [("lox".to_owned(), "lox".to_owned())]);
    }

    /// The SEDS layout (GroundStation26 `backend/layout/layout.json`, as the
    /// replay-export contract fixture carries it) names exactly the built-in
    /// values, so a SEDS station looks the same with or without the fields.
    #[test]
    fn the_seds_layout_names_the_built_in_values() {
        let file: serde_json::Value =
            serde_json::from_str(include_str!("../../tests/fixtures/backend-contract/replay-export/flight.json")).unwrap();
        let layout: LayoutConfig = serde_json::from_value(file["layout"].clone()).unwrap();
        assert!(layout.gps.is_some() && !layout.watch_items.is_empty() && !layout.actions_tab.controls.is_empty());
        assert_eq!(gps_types_of(layout.gps.as_ref()), BUILT_IN_GPS_TYPES);
        assert_eq!(gps_indices_of(layout.gps.as_ref()), BUILT_IN_GPS_INDICES);
        assert_eq!(fluids_of(&layout.actions_tab.fill_target_fluids), fluids_of(&[]));
        let labels: Vec<_> = ["TogglePlumbingDelayInterlock", "ToggleChecklistInterlock", "ToggleSupplyInterlock", "ToggleBoardStateProofInterlock"]
            .iter()
            .map(|cmd| control_label_in(&layout.actions_tab.controls, cmd).unwrap())
            .collect();
        assert_eq!(labels, ["Retraction minimum 45 seconds", "Checklist enforcement", "Nitrogen/nitrous one at a time", "Board state checks"]);
        for item in super::super::watch_companion::registry::ITEMS {
            if let super::super::watch_companion::registry::Source::Telemetry { channels, .. } = item.source {
                let built_in: Vec<Channel> = channels.iter().map(|(dt, s, i)| ((*dt).to_owned(), s.map(str::to_owned), *i)).collect();
                assert_eq!(watch_channels_in(&layout.watch_items, item.id), Some(built_in), "{}", item.id);
            }
        }
    }
}
