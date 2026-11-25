//! What the station's voice page needs to know about the app it runs in, and
//! the iOS audio-session guard for crew voice.
//!
//! The voice page (`station_ui/scripts/voice.js`) is a station document in an
//! iframe. It cannot tell a native app from a browser tab on its own, so the
//! dashboard sends `app` (see [`page_info`]) with each `gs26-session` message:
//! the page then names the right setting when the microphone is refused and
//! never says "this browser" inside an app.
//!
//! iOS: WebKit switches the app's audio session to play-and-record when the
//! microphone opens. With no headset, play-and-record plays through the
//! receiver (the earpiece at the top of the phone) unless the speaker is
//! chosen, so crew voice is nearly inaudible at arm's length. `VoiceAudioShim.m`
//! reports every route change, interruption and media-services reset here;
//! [`speaker_override_wanted`] decides whether to move output to the speaker,
//! and each report goes to the debug log (`[voice-audio] ...`) for on-device
//! diagnosis. Headsets, Bluetooth and AirPlay routes are never overridden.

use serde::Deserialize;

/// Platform name the voice page understands.
pub(crate) const fn platform() -> &'static str {
    if cfg!(target_os = "ios") {
        "ios"
    } else if cfg!(target_os = "android") {
        "android"
    } else if cfg!(target_os = "macos") {
        "macos"
    } else if cfg!(target_os = "windows") {
        "windows"
    } else if cfg!(target_arch = "wasm32") {
        "web"
    } else {
        "linux"
    }
}

/// The `app` object of the `gs26-session` message.
pub(crate) fn page_info() -> serde_json::Value {
    let mut info = serde_json::json!({ "platform": platform() });
    if let Some(permission) = latest().and_then(|s| s.record_permission) {
        info["record_permission"] = permission.into();
    }
    info
}

/// One report from `VoiceAudioShim.m` (AVAudioSession state after an event).
#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
#[serde(default)]
pub(crate) struct SessionReport {
    /// `install`, `route`, `interruption`, `reset`, `active` or `poll`.
    pub event: String,
    /// Route-change reason or interruption type, when the event has one.
    pub detail: String,
    /// `AVAudioSessionCategory...` raw value, e.g. `AVAudioSessionCategoryPlayAndRecord`.
    pub category: String,
    pub mode: String,
    /// Output port types, e.g. `Receiver`, `Speaker`, `Headphones`, `BluetoothHFP`.
    pub outputs: Vec<String>,
    pub inputs: Vec<String>,
    /// `granted`, `denied` or `undetermined`.
    pub record_permission: Option<String>,
    pub sample_rate: f64,
    /// True after this app moved output to the speaker.
    pub speaker_override: bool,
}

const PLAY_AND_RECORD: &str = "AVAudioSessionCategoryPlayAndRecord";
const RECEIVER: &str = "Receiver";

/// Output goes to the speaker when the microphone is open and the only
/// output is the receiver. Any other route (wired, Bluetooth, AirPlay,
/// CarPlay, already the speaker) is what the operator chose and stays.
pub(crate) fn speaker_override_wanted(report: &SessionReport) -> bool {
    report.category == PLAY_AND_RECORD && report.outputs.len() == 1 && report.outputs[0] == RECEIVER
}

/// One debug-log line per report.
pub(crate) fn log_line(report: &SessionReport) -> String {
    let detail = if report.detail.is_empty() { String::new() } else { format!(" ({})", report.detail) };
    let category = report.category.strip_prefix("AVAudioSessionCategory").unwrap_or(&report.category);
    let mode = report.mode.strip_prefix("AVAudioSessionMode").unwrap_or(&report.mode);
    format!(
        "[voice-audio] {}{detail}: category {} mode {} out [{}] in [{}] {:.0} Hz mic {}{}",
        report.event,
        if category.is_empty() { "?" } else { category },
        if mode.is_empty() { "?" } else { mode },
        report.outputs.join(","),
        report.inputs.join(","),
        report.sample_rate,
        report.record_permission.as_deref().unwrap_or("?"),
        if report.speaker_override { " speaker-override" } else { "" },
    )
}

static LATEST: std::sync::Mutex<Option<SessionReport>> = std::sync::Mutex::new(None);

fn latest() -> Option<SessionReport> {
    LATEST.lock().ok().and_then(|l| l.clone())
}

/// Handles one shim report: log it, remember it, and say whether the shim
/// should move output to the speaker.
#[cfg_attr(not(target_os = "ios"), allow(dead_code))]
pub(crate) fn handle_report(json: &str) -> bool {
    let Ok(report) = serde_json::from_str::<SessionReport>(json) else {
        crate::debug_log::append("[voice-audio] unreadable audio session report");
        return false;
    };
    crate::debug_log::append(&log_line(&report));
    let wanted = speaker_override_wanted(&report);
    if let Ok(mut l) = LATEST.lock() {
        *l = Some(report);
    }
    wanted
}

#[cfg(target_os = "ios")]
mod platform_ios {
    use std::ffi::{CStr, c_char};

    unsafe extern "C" {
        fn gs26_voice_audio_install(callback: extern "C" fn(*const c_char) -> i32);
    }

    /// Called by the shim on the main thread with a JSON report; a non-zero
    /// return asks it to move output to the speaker.
    extern "C" fn on_report(json: *const c_char) -> i32 {
        if json.is_null() {
            return 0;
        }
        let json = unsafe { CStr::from_ptr(json) }.to_string_lossy();
        i32::from(super::handle_report(&json))
    }

    pub(super) fn install() {
        unsafe { gs26_voice_audio_install(on_report) };
    }
}

/// Starts the iOS audio-session guard (idempotent; nothing elsewhere).
#[cfg(not(target_arch = "wasm32"))]
pub(crate) fn install() {
    #[cfg(target_os = "ios")]
    {
        static ONCE: std::sync::Once = std::sync::Once::new();
        ONCE.call_once(platform_ios::install);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn report(category: &str, outputs: &[&str]) -> SessionReport {
        SessionReport {
            event: "route".into(),
            category: category.into(),
            outputs: outputs.iter().map(|s| s.to_string()).collect(),
            ..SessionReport::default()
        }
    }

    #[test]
    fn receiver_only_play_and_record_moves_to_the_speaker() {
        assert!(speaker_override_wanted(&report(PLAY_AND_RECORD, &["Receiver"])));
    }

    #[test]
    fn chosen_routes_and_playback_categories_are_left_alone() {
        for outputs in [&["Speaker"][..], &["Headphones"], &["BluetoothHFP"], &["BluetoothA2DPOutput"], &["AirPlay"], &["Receiver", "AirPlay"], &[]] {
            assert!(!speaker_override_wanted(&report(PLAY_AND_RECORD, outputs)), "{outputs:?}");
        }
        for category in ["AVAudioSessionCategorySoloAmbient", "AVAudioSessionCategoryAmbient", "AVAudioSessionCategoryPlayback", ""] {
            assert!(!speaker_override_wanted(&report(category, &["Receiver"])), "{category}");
        }
    }

    #[test]
    fn shim_reports_parse_with_missing_fields_and_log_compactly() {
        let parsed: SessionReport = serde_json::from_str(
            r#"{"event":"route","detail":"CategoryChange","category":"AVAudioSessionCategoryPlayAndRecord","mode":"AVAudioSessionModeVideoChat","outputs":["Receiver"],"inputs":["MicrophoneBuiltIn"],"record_permission":"granted","sample_rate":48000,"extra":1}"#,
        )
        .unwrap();
        assert!(speaker_override_wanted(&parsed));
        assert_eq!(
            log_line(&parsed),
            "[voice-audio] route (CategoryChange): category PlayAndRecord mode VideoChat out [Receiver] in [MicrophoneBuiltIn] 48000 Hz mic granted"
        );
        let empty: SessionReport = serde_json::from_str("{}").unwrap();
        assert!(!speaker_override_wanted(&empty));
        assert!(log_line(&empty).contains("category ? mode ?"));
    }

    #[test]
    fn page_info_names_the_platform_and_the_microphone_permission() {
        assert_eq!(page_info()["platform"], platform());
        assert!(handle_report(r#"{"event":"poll","category":"AVAudioSessionCategoryPlayAndRecord","outputs":["Receiver"],"record_permission":"denied"}"#));
        assert_eq!(page_info()["record_permission"], "denied");
        assert!(!handle_report("not json"));
        if let Ok(mut l) = LATEST.lock() {
            *l = None;
        }
    }

    #[test]
    fn host_platform_name_matches_the_build() {
        let expected = if cfg!(target_os = "macos") { "macos" } else if cfg!(target_os = "windows") { "windows" } else { "linux" };
        assert_eq!(platform(), expected);
    }
}
