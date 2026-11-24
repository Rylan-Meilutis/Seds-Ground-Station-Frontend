//! ADC over-range display.
//!
//! The backend sets bit `i` of a telemetry row's `over_range` when `values[i]`
//! came from a saturated DAQ ADC channel (that value arrives as null), and bit
//! `i` of `over_range_low` when the saturation was below −full scale. Rows
//! derived from DAQ channels (load-cell weight, fill percent, chamber
//! pressure, ...) carry the bits of their source.
//!
//! An over-range value is never a number and never the previous number: every
//! view shows the text [`OverRange::text`] in the `gs-ovr` style with the
//! [`OverRange::title`] tooltip, and charts break the line there and mark the
//! plot edge (see `data_chart`). A null value without a bit keeps each view's
//! usual no-data rendering.
use dioxus::prelude::*;

pub(crate) const OVR_CLASS: &str = "gs-ovr";

/// Which side of the ADC's range the reading saturated on.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum OverRange {
    /// Above +full scale (or an unsigned channel's top).
    High,
    /// Below −full scale.
    Low,
}

impl OverRange {
    /// The state of channel `index` from a row's bit masks.
    pub(crate) fn from_bits(over_range: u32, over_range_low: u32, index: usize) -> Option<Self> {
        let bit = 1u32.checked_shl(u32::try_from(index).ok()?)?;
        if over_range & bit == 0 {
            None
        } else if over_range_low & bit != 0 {
            Some(Self::Low)
        } else {
            Some(Self::High)
        }
    }

    pub(crate) const fn text(self) -> &'static str {
        match self {
            Self::High => "OVR",
            Self::Low => "−OVR",
        }
    }

    pub(crate) const fn title(self) -> &'static str {
        match self {
            Self::High => "ADC over range: reading is not valid",
            Self::Low => "ADC over range (below −full scale): reading is not valid",
        }
    }

    /// Recognises a value string produced by [`OverRange::text`], so shared
    /// value widgets that only receive text can style it.
    pub(crate) fn from_text(text: &str) -> Option<Self> {
        match text.trim() {
            "OVR" => Some(Self::High),
            "−OVR" => Some(Self::Low),
            _ => None,
        }
    }
}

/// One channel as a view should show it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum Reading {
    Value(f32),
    OverRange(OverRange),
    Missing,
}

impl Reading {
    /// A channel of a row: over range wins over any value the row carries.
    pub(crate) fn of(values: &[Option<f32>], over_range: u32, over_range_low: u32, index: usize) -> Self {
        if let Some(state) = OverRange::from_bits(over_range, over_range_low, index) {
            return Self::OverRange(state);
        }
        match values.get(index).copied().flatten() {
            Some(value) => Self::Value(value),
            None => Self::Missing,
        }
    }

    pub(crate) fn value(self) -> Option<f32> {
        match self {
            Self::Value(value) => Some(value),
            _ => None,
        }
    }

    pub(crate) fn over_range(self) -> Option<OverRange> {
        match self {
            Self::OverRange(state) => Some(state),
            _ => None,
        }
    }

    /// Formats the reading: `format` for a value, the OVR text when over
    /// range, `missing` otherwise (each view keeps its own no-data text).
    #[cfg(test)]
    pub(crate) fn display(self, format: impl FnOnce(f32) -> String, missing: &str) -> String {
        match self {
            Self::Value(value) => format(value),
            Self::OverRange(state) => state.text().to_string(),
            Self::Missing => missing.to_string(),
        }
    }
}

/// Text for a value slot: the OVR text in the over-range style with its
/// tooltip, any other text unchanged.
#[component]
pub(crate) fn ValueText(text: String) -> Element {
    match OverRange::from_text(&text) {
        Some(state) => rsx! {
            span { class: OVR_CLASS, title: state.title(), "aria-label": state.title(), "data-over-range": if state == OverRange::Low { "low" } else { "high" }, "{text}" }
        },
        None => rsx! { "{text}" },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bits_select_high_low_and_ignore_clear_or_out_of_range_indices() {
        assert_eq!(OverRange::from_bits(0b101, 0b100, 0), Some(OverRange::High));
        assert_eq!(OverRange::from_bits(0b101, 0b100, 1), None);
        assert_eq!(OverRange::from_bits(0b101, 0b100, 2), Some(OverRange::Low));
        // A low bit without the over-range bit is not over range.
        assert_eq!(OverRange::from_bits(0, 0b1, 0), None);
        assert_eq!(OverRange::from_bits(u32::MAX, 0, 31), Some(OverRange::High));
        assert_eq!(OverRange::from_bits(u32::MAX, 0, 32), None);
        assert_eq!(OverRange::from_bits(u32::MAX, 0, usize::MAX), None);
    }

    #[test]
    fn ovr_formatting_is_text_never_a_number_and_null_keeps_no_data() {
        let fmt = |v: f32| format!("{v:.2} kg");
        // High and low over range.
        let high = Reading::of(&[None], 1, 0, 0);
        let low = Reading::of(&[None], 1, 1, 0);
        assert_eq!(high.display(fmt, "-"), "OVR");
        assert_eq!(low.display(fmt, "-"), "−OVR");
        assert_eq!(high.value(), None);
        assert_eq!(low.over_range(), Some(OverRange::Low));
        // Even if a value were present, the bit wins: never a stale number.
        assert_eq!(Reading::of(&[Some(12.0)], 1, 0, 0).display(fmt, "-"), "OVR");
        // Null without a flag stays each view's no-data text.
        assert_eq!(Reading::of(&[None], 0, 0, 0).display(fmt, "-"), "-");
        assert_eq!(Reading::of(&[None], 0, 0, 0).display(fmt, "—"), "—");
        assert_eq!(Reading::of(&[], 0, 0, 3), Reading::Missing);
        assert_eq!(Reading::of(&[Some(1.5)], 0b10, 0, 0).display(fmt, "-"), "1.50 kg");
        assert_eq!(OverRange::from_text(" OVR "), Some(OverRange::High));
        assert_eq!(OverRange::from_text("−OVR"), Some(OverRange::Low));
        assert_eq!(OverRange::from_text("1.0"), None);
        assert_eq!(OverRange::High.title(), "ADC over range: reading is not valid");
    }

    #[test]
    fn value_text_styles_only_ovr() {
        fn render(text: &str) -> String {
            let text = text.to_string();
            let mut dom = VirtualDom::new_with_props(ValueText, ValueTextProps { text });
            dom.rebuild_in_place();
            dioxus::ssr::render(&dom)
        }
        let ovr = render("OVR");
        assert!(ovr.contains("class=\"gs-ovr\""), "{ovr}");
        assert!(ovr.contains("ADC over range: reading is not valid"), "{ovr}");
        assert!(render("−OVR").contains("data-over-range=\"low\""));
        let plain = render("12.0 kg");
        assert!(!plain.contains("gs-ovr") && plain.contains("12.0 kg"), "{plain}");
    }
}
