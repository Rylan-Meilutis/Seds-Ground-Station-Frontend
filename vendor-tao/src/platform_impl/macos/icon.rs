// Copyright 2014-2021 The winit contributors
// Copyright 2021-2023 Tauri Programme within The Commons Conservancy
// SPDX-License-Identifier: Apache-2.0

use crate::icon::{BadIcon, RgbaIcon};

#[derive(Debug, Clone)]
pub struct PlatformIcon;

impl PlatformIcon {
  pub fn from_rgba(rgba: Vec<u8>, width: u32, height: u32) -> Result<Self, BadIcon> {
    // AppKit does not use per-window icons; retain validation of the public API.
    RgbaIcon::from_rgba(rgba, width, height)?;
    Ok(PlatformIcon)
  }
}
