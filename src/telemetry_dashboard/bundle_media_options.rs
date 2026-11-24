//! What a replay bundle download adds beyond the data: recorded camera video
//! and crew voice, each with a size cap (`?video=1&video_max_mb=N`,
//! `?voice=1&voice_max_mb=N`; `docs/replay-bundle.md` "Recorded media").
use dioxus::prelude::*;

/// The station's caps (MiB): default and largest it accepts.
pub(crate) const VIDEO_MB: (u32, u32) = (32, 64);
pub(crate) const VOICE_MB: (u32, u32) = (8, 16);

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct BundleMedia {
    pub video: bool,
    pub video_mb: u32,
    pub voice: bool,
    pub voice_mb: u32,
}

impl Default for BundleMedia {
    fn default() -> Self {
        Self { video: false, video_mb: VIDEO_MB.0, voice: false, voice_mb: VOICE_MB.0 }
    }
}

impl BundleMedia {
    /// The bundle route's query for these choices ("" when none).
    pub(crate) fn query(&self) -> String {
        let mut parts = Vec::new();
        if self.video {
            parts.push(format!("video=1&video_max_mb={}", self.video_mb.clamp(1, VIDEO_MB.1)));
        }
        if self.voice {
            parts.push(format!("voice=1&voice_max_mb={}", self.voice_mb.clamp(1, VOICE_MB.1)));
        }
        if parts.is_empty() { String::new() } else { format!("?{}", parts.join("&")) }
    }
}

/// Checkboxes and caps beside "Download replay bundle".
#[component]
pub(crate) fn BundleMediaToggles(choice: Signal<BundleMedia>) -> Element {
    let current = choice();
    let cap = |value: String, max: u32, default: u32| value.trim().parse::<u32>().map_or(default, |v| v.clamp(1, max));
    rsx! {
        fieldset { class: "gs26-bundle-media", "data-testid": "bundle-media-options",
            style: "display:flex;flex-wrap:wrap;gap:6px 14px;align-items:center;border:0;padding:0;margin:4px 0;font-size:13px;min-width:0;",
            legend { style: "padding:0;font-weight:600;", "Replay bundles also include" }
            label { style: "display:flex;gap:6px;align-items:center;",
                input { r#type: "checkbox", checked: current.video, "aria-label": "Camera video",
                    onchange: move |e| choice.with_mut(|c| c.video = e.checked()) }
                "Camera video, up to"
                input { r#type: "number", min: "1", max: "{VIDEO_MB.1}", value: "{current.video_mb}", disabled: !current.video, style: "width:4.5em;", "aria-label": "Camera video cap (MiB)",
                    onchange: move |e| choice.with_mut(|c| c.video_mb = cap(e.value(), VIDEO_MB.1, VIDEO_MB.0)) }
                "MiB"
            }
            label { style: "display:flex;gap:6px;align-items:center;",
                input { r#type: "checkbox", checked: current.voice, "aria-label": "Crew voice",
                    onchange: move |e| choice.with_mut(|c| c.voice = e.checked()) }
                "Crew voice, up to"
                input { r#type: "number", min: "1", max: "{VOICE_MB.1}", value: "{current.voice_mb}", disabled: !current.voice, style: "width:4.5em;", "aria-label": "Crew voice cap (MiB)",
                    onchange: move |e| choice.with_mut(|c| c.voice_mb = cap(e.value(), VOICE_MB.1, VOICE_MB.0)) }
                "MiB"
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_query_names_only_what_was_ticked() {
        assert_eq!(BundleMedia::default().query(), "");
        let both = BundleMedia { video: true, video_mb: 500, voice: true, voice_mb: 4 };
        assert_eq!(both.query(), "?video=1&video_max_mb=64&voice=1&voice_max_mb=4", "caps are clamped to the station's");
        assert_eq!(BundleMedia { voice: true, ..Default::default() }.query(), "?voice=1&voice_max_mb=8");
    }
}
