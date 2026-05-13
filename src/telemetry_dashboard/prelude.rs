//! Shared imports for small dashboard UI modules.
//!
//! Keep this focused. Domain-heavy modules should still import their own layout
//! and data types explicitly so dependencies stay visible.

pub(crate) use super::translate_text;
pub(crate) use dioxus::prelude::*;
pub(crate) use dioxus_signals::Signal;

pub(crate) use super::layout::ThemeConfig;
