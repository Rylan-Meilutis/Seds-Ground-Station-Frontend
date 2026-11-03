// Exact Config/NitrogenStep definitions and Default from published GS8c08a70258270b1c0d85b534ef10436081fdd2fc.
use serde::{Deserialize, Serialize};
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct NitrogenStep {
    pub target_psi: f32,
    pub settle_ms: u64,
    pub hold_ms: u64,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(default, deny_unknown_fields)]
pub struct Config {
    pub nitrogen_target_psi: f32,
    pub nitrogen_steps: Vec<NitrogenStep>,
    pub fill_overfill_kg: f32,
    pub fill_tolerance_kg: f32,
    pub fill_settle_ms: u64,
    /// Explicit additional nitrogen mass allowance, in calibrated kg.
    /// None disables Turbocharge; this is not liquid overfill tolerance.
    pub turbocharge_max_added_mass_kg: Option<f32>,
    pub pressure_step_psi: f32,
    pub pressure_ceiling_psi: Option<f32>,
    pub maximum_zero_offset_psi: Option<f32>,
    pub dry_self_test_confirmed: bool,
    pub grouped_panel: bool,
}
impl Default for Config {
    fn default() -> Self {
        Self {
            nitrogen_target_psi: 120.0,
            nitrogen_steps: Vec::new(),
            fill_overfill_kg: 0.05,
            fill_tolerance_kg: 0.07,
            fill_settle_ms: 3000,
            turbocharge_max_added_mass_kg: None,
            pressure_step_psi: 50.0,
            pressure_ceiling_psi: None,
            maximum_zero_offset_psi: None,
            dry_self_test_confirmed: false,
            grouped_panel: true,
        }
    }
}
