//! What the station's voice page needs to know about the app it runs in, and
//! the crew-voice audio output on phones.
//!
//! The voice page (`station_ui/scripts/voice.js`) is a station document in an
//! iframe. It cannot tell a native app from a browser tab on its own, so the
//! dashboard sends `app` (see [`page_info`]) with each `gs26-session` message:
//! the page then names the right setting when the microphone is refused and
//! never says "this browser" inside an app.
//!
//! **Audio output** (iOS and Android). The operator chooses on the voice page
//! between **Phone** (the default: like a call, the earpiece or whatever
//! headset is connected) and **Speaker** (the loudspeaker, even with a headset
//! connected). The page posts `{"type":"gs26-audio-output","choice","active"}`
//! to the dashboard ([`page_message`]); the choice is kept per device in
//! `voice_audio_output.json` beside the app's settings and applied natively:
//!
//! * iOS: WebKit switches the audio session to play-and-record while the
//!   microphone is open. `VoiceAudioShim.m` reports every route change,
//!   interruption, media-services reset and return to the foreground;
//!   [`ios_port_override`] answers with the port override that keeps the
//!   chosen output (`overrideOutputAudioPort(.speaker)` for Speaker, `.none`
//!   to undo it for Phone), so a headset arriving or an interruption ending
//!   never silently changes the choice. Each report goes to the debug log as
//!   `[voice-audio] ...`.
//! * Android: `VoiceAudioBridge.kt` puts the audio mode in
//!   `MODE_IN_COMMUNICATION` while voice is joined and selects the
//!   communication device [`android_pick`] chooses (`setCommunicationDevice`
//!   on API 31+, `setSpeakerphoneOn` below), again after every device change.
//!
//! When the chosen output is not available the page says what plays instead
//! (`audio_output.note`). The watch voice relay through the phone plays no
//! audio on the phone (the phone's own voice page is held while its watch
//! talks), so it has nothing to route.

use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicU64, Ordering};

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
    if native_output_control(platform()) {
        let state = output_snapshot();
        info["audio_output"] = output_state(state.choice, state.route.as_deref(), state.available);
    }
    info
}

// --- Audio output choice ---------------------------------------------------

/// Where crew voice plays on a phone.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum OutputChoice {
    /// Like a call: the earpiece, or the connected headset (wired, USB,
    /// Bluetooth, hearing aid). The default: private, and what a phone does
    /// for a call.
    #[default]
    Phone,
    /// The loudspeaker, even with a headset connected.
    Speaker,
}

/// Phones choose the output natively; elsewhere the page uses the web
/// view's own output selection (`setSinkId`) when it has one.
pub(crate) fn native_output_control(platform: &str) -> bool {
    matches!(platform, "ios" | "android")
}

/// The `audio_output` object for the page.
pub(crate) fn output_state(choice: OutputChoice, route: Option<&str>, available: bool) -> serde_json::Value {
    let note = (!available).then(|| {
        let playing = route.unwrap_or("another output");
        match choice {
            OutputChoice::Speaker => format!("The speaker is not available; playing through {playing}."),
            OutputChoice::Phone => format!("The earpiece or headset is not available; playing through {playing}."),
        }
    });
    serde_json::json!({
        "control": "native",
        "options": ["phone", "speaker"],
        "choice": choice,
        "route": route,
        "available": available,
        "note": note,
    })
}

#[derive(Clone, Debug, Default)]
struct OutputState {
    loaded: bool,
    choice: OutputChoice,
    /// Voice is joined with the microphone open on this page.
    #[cfg_attr(target_arch = "wasm32", allow(dead_code))]
    active: bool,
    /// What is playing now, in words (`earpiece`, `speaker`, `Bluetooth` ...).
    route: Option<String>,
    available: bool,
}

static OUTPUT: std::sync::Mutex<OutputState> =
    std::sync::Mutex::new(OutputState { loaded: false, choice: OutputChoice::Phone, active: false, route: None, available: true });
static REVISION: AtomicU64 = AtomicU64::new(0);

/// Bumped whenever the page should get a fresh `audio_output`.
pub(crate) fn revision() -> u64 {
    REVISION.load(Ordering::Acquire)
}

fn bump() {
    REVISION.fetch_add(1, Ordering::AcqRel);
}

#[cfg(not(target_arch = "wasm32"))]
fn choice_path() -> std::path::PathBuf {
    #[cfg(target_os = "android")]
    if let Some(dir) = crate::native_storage::android_files_dir() {
        return dir.join("gs26").join("voice_audio_output.json");
    }
    crate::native_storage::fallback_data_dir().join("gs26").join("voice_audio_output.json")
}

/// The saved choice; Phone when nothing (or nothing readable) is saved.
#[cfg_attr(target_arch = "wasm32", allow(dead_code))]
pub(crate) fn read_choice_from(path: &std::path::Path) -> OutputChoice {
    std::fs::read(path)
        .ok()
        .and_then(|bytes| serde_json::from_slice::<serde_json::Value>(&bytes).ok())
        .and_then(|value| serde_json::from_value(value["choice"].clone()).ok())
        .unwrap_or_default()
}

#[cfg(not(target_arch = "wasm32"))]
pub(crate) fn write_choice_to(path: &std::path::Path, choice: OutputChoice) {
    let Ok(bytes) = serde_json::to_vec(&serde_json::json!({ "choice": choice })) else { return };
    if let Err(error) = crate::native_storage::atomic_write(path, &bytes) {
        crate::native_storage::report_error("voice audio output write", &error);
    }
}

fn output_snapshot() -> OutputState {
    let Ok(mut state) = OUTPUT.lock() else { return OutputState::default() };
    if !state.loaded {
        state.loaded = true;
        #[cfg(not(target_arch = "wasm32"))]
        {
            state.choice = read_choice_from(&choice_path());
        }
    }
    state.clone()
}

/// The page's `{"type":"gs26-audio-output","choice":"phone"|"speaker","active":bool}`:
/// remember a new choice and apply the output now.
#[cfg_attr(target_arch = "wasm32", allow(dead_code))]
pub(crate) fn page_message(message: &serde_json::Value) {
    if message["type"] != "gs26-audio-output" {
        return;
    }
    let previous = output_snapshot();
    let choice = serde_json::from_value::<OutputChoice>(message["choice"].clone()).unwrap_or(previous.choice);
    let active = message["active"].as_bool().unwrap_or(previous.active);
    if let Ok(mut state) = OUTPUT.lock() {
        state.choice = choice;
        state.active = active;
    }
    if choice != previous.choice {
        #[cfg(not(target_arch = "wasm32"))]
        write_choice_to(&choice_path(), choice);
        crate::debug_log::append(&format!("[voice-audio] output choice: {}", choice_name(choice)));
    }
    if choice != previous.choice || active != previous.active {
        apply();
        bump();
    }
}

fn choice_name(choice: OutputChoice) -> &'static str {
    match choice {
        OutputChoice::Phone => "phone",
        OutputChoice::Speaker => "speaker",
    }
}

fn set_route(route: Option<String>, available: bool) {
    let changed = OUTPUT.lock().is_ok_and(|mut state| {
        let changed = state.route != route || state.available != available;
        state.route = route;
        state.available = available;
        changed
    });
    if changed {
        bump();
    }
}

/// Applies the saved choice on this platform (nothing outside iOS/Android).
#[cfg_attr(target_arch = "wasm32", allow(dead_code))]
fn apply() {
    #[cfg(target_os = "ios")]
    platform_ios::refresh();
    #[cfg(target_os = "android")]
    {
        let state = output_snapshot();
        platform_android::apply(state.choice, state.active);
    }
}

// --- Android ---------------------------------------------------------------

/// `AudioDeviceInfo.TYPE_*` values the choice uses.
#[cfg_attr(not(target_os = "android"), allow(dead_code))]
pub(crate) mod android_type {
    pub const EARPIECE: i32 = 1;
    pub const SPEAKER: i32 = 2;
    pub const WIRED_HEADSET: i32 = 3;
    pub const WIRED_HEADPHONES: i32 = 4;
    pub const BLUETOOTH_SCO: i32 = 7;
    pub const USB_DEVICE: i32 = 11;
    pub const USB_HEADSET: i32 = 22;
    pub const HEARING_AID: i32 = 23;
    pub const BLE_HEADSET: i32 = 26;
}

/// The communication device for `choice` among `available` (device types),
/// or None when the choice cannot be met. Phone prefers a headset the
/// operator connected (Bluetooth, hearing aid, USB, wired) over the
/// earpiece, as a call does.
#[cfg_attr(not(target_os = "android"), allow(dead_code))]
pub(crate) fn android_pick(choice: OutputChoice, available: &[i32]) -> Option<i32> {
    use android_type::*;
    let order: &[i32] = match choice {
        OutputChoice::Speaker => &[SPEAKER],
        OutputChoice::Phone => &[BLE_HEADSET, BLUETOOTH_SCO, HEARING_AID, USB_HEADSET, WIRED_HEADSET, WIRED_HEADPHONES, USB_DEVICE, EARPIECE],
    };
    order.iter().copied().find(|t| available.contains(t))
}

/// Device types from `VoiceAudioBridge.devices` (bit n set = type n present).
#[cfg_attr(not(target_os = "android"), allow(dead_code))]
pub(crate) fn android_types_from_mask(mask: i32) -> Vec<i32> {
    (0..31).filter(|bit| mask & (1 << bit) != 0).collect()
}

#[cfg_attr(not(target_os = "android"), allow(dead_code))]
pub(crate) fn android_label(device_type: i32) -> &'static str {
    use android_type::*;
    match device_type {
        EARPIECE => "the earpiece",
        SPEAKER => "the speaker",
        WIRED_HEADSET | WIRED_HEADPHONES => "the wired headset",
        BLUETOOTH_SCO | BLE_HEADSET => "the Bluetooth headset",
        HEARING_AID => "the hearing aid",
        USB_DEVICE | USB_HEADSET => "the USB headset",
        _ => "another output",
    }
}

#[cfg(target_os = "android")]
mod platform_android {
    use super::*;
    use ::jni::objects::{JClass, JValue};
    use ::jni::EnvUnowned;
    use crate::telemetry_dashboard::proximity_native::call_class;

    const CLASS: &str = "com.ubseds.gs26.VoiceAudioBridge";

    fn int(method: &str, sig: &str, args: &[JValue<'_>]) -> Option<i32> {
        call_class(CLASS, method, sig, args, |_, v| v.i().ok())
    }
    fn boolean(method: &str, sig: &str, args: &[JValue<'_>]) -> Option<bool> {
        call_class(CLASS, method, sig, args, |_, v| v.z().ok())
    }

    pub(super) fn apply(choice: OutputChoice, active: bool) {
        if !active {
            let _ = call_class(CLASS, "release", "(Landroid/content/Context;)V", &[], |_, _| Some(()));
            set_route(None, true);
            return;
        }
        let mask = int("devices", "(Landroid/content/Context;)I", &[]).unwrap_or(-1);
        let (route, available) = if mask < 0 {
            // API 30 and below: speakerphone on or off.
            let speaker = choice == OutputChoice::Speaker;
            let ok = boolean("legacy", "(Landroid/content/Context;Z)Z", &[JValue::Bool(speaker)]).unwrap_or(false);
            (if speaker { "the speaker" } else { "the earpiece or headset" }, ok)
        } else {
            match android_pick(choice, &android_types_from_mask(mask)) {
                Some(device) => {
                    let ok = boolean("select", "(Landroid/content/Context;I)Z", &[JValue::Int(device)]).unwrap_or(false);
                    (android_label(device), ok)
                }
                None => (android_label(int("current", "(Landroid/content/Context;)I", &[]).unwrap_or(0)), false),
            }
        };
        crate::debug_log::append(&format!(
            "[voice-audio] android output {}: {route}{}",
            choice_name(choice),
            if available { "" } else { " (choice unavailable)" }
        ));
        set_route(Some(route.to_string()), available);
    }

    /// An audio device came or went (`AudioDeviceCallback`): keep the choice.
    #[unsafe(no_mangle)]
    pub extern "system" fn Java_com_ubseds_gs26_VoiceAudioBridge_nativeOnDevicesChanged(
        _env: EnvUnowned<'_>,
        _class: JClass<'_>,
    ) {
        let state = output_snapshot();
        if state.active {
            // Off the callback thread: JNI calls back into the bridge.
            let _ = std::thread::Builder::new()
                .name("gs26-voice-audio".into())
                .spawn(move || apply(state.choice, state.active));
        }
    }
}

// --- iOS -------------------------------------------------------------------

/// What the iOS shim does with the output after a report.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(i32)]
pub(crate) enum PortOverride {
    /// Nothing to change.
    Leave = 0,
    /// `overrideOutputAudioPort(.speaker)`.
    Speaker = 1,
    /// `overrideOutputAudioPort(.none)`: back to the earpiece or headset.
    Clear = 2,
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
const SPEAKER: &str = "Speaker";

/// The override that keeps `choice` after `report`. Only while WebKit has
/// the microphone open (play-and-record); other categories are playback the
/// app does not route. Re-run after every route change and interruption, so
/// a headset arriving or an interruption ending never changes the choice.
pub(crate) fn ios_port_override(choice: OutputChoice, report: &SessionReport) -> PortOverride {
    if report.category != PLAY_AND_RECORD {
        return PortOverride::Leave;
    }
    match choice {
        OutputChoice::Speaker if !(report.outputs.len() == 1 && report.outputs[0] == SPEAKER) => PortOverride::Speaker,
        OutputChoice::Phone if report.speaker_override => PortOverride::Clear,
        _ => PortOverride::Leave,
    }
}

/// What plays now, in words, and whether it is what `choice` asks for.
/// Outside play-and-record there is no route to report.
pub(crate) fn ios_route(choice: OutputChoice, report: &SessionReport) -> (Option<String>, bool) {
    if report.category != PLAY_AND_RECORD || report.outputs.is_empty() {
        return (None, true);
    }
    let label = |port: &str| match port {
        RECEIVER => "the earpiece",
        SPEAKER => "the speaker",
        "Headphones" | "HeadsetMic" => "the wired headset",
        "BluetoothHFP" | "BluetoothA2DPOutput" | "BluetoothLE" => "the Bluetooth headset",
        "AirPlay" => "AirPlay",
        "CarAudio" => "the car",
        "USBAudio" => "the USB headset",
        "HDMI" => "HDMI",
        _ => "another output",
    };
    let on_speaker = report.outputs.iter().any(|p| p == SPEAKER);
    let available = match choice {
        OutputChoice::Speaker => on_speaker,
        OutputChoice::Phone => !on_speaker,
    };
    (Some(label(&report.outputs[0]).to_string()), available)
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

/// Handles one shim report: log it, remember it, and say which port
/// override keeps the operator's choice.
#[cfg_attr(not(target_os = "ios"), allow(dead_code))]
pub(crate) fn handle_report(json: &str) -> PortOverride {
    let Ok(report) = serde_json::from_str::<SessionReport>(json) else {
        crate::debug_log::append("[voice-audio] unreadable audio session report");
        return PortOverride::Leave;
    };
    let choice = output_snapshot().choice;
    let action = ios_port_override(choice, &report);
    crate::debug_log::append(&format!(
        "{} choice {}{}",
        log_line(&report),
        choice_name(choice),
        match action {
            PortOverride::Leave => "",
            PortOverride::Speaker => " -> speaker",
            PortOverride::Clear => " -> earpiece/headset",
        }
    ));
    // A pending override changes the route; the next report says where it went.
    if action == PortOverride::Leave {
        let (route, available) = ios_route(choice, &report);
        set_route(route, available);
    }
    if let Ok(mut l) = LATEST.lock() {
        *l = Some(report);
    }
    action
}

#[cfg(target_os = "ios")]
mod platform_ios {
    use std::ffi::{CStr, c_char};

    unsafe extern "C" {
        fn gs26_voice_audio_install(callback: extern "C" fn(*const c_char) -> i32);
        fn gs26_voice_audio_refresh();
    }

    /// Called by the shim on the main thread with a JSON report; returns the
    /// `PortOverride` to apply (0 leave, 1 speaker, 2 none).
    extern "C" fn on_report(json: *const c_char) -> i32 {
        if json.is_null() {
            return 0;
        }
        let json = unsafe { CStr::from_ptr(json) }.to_string_lossy();
        super::handle_report(&json) as i32
    }

    pub(super) fn install() {
        unsafe { gs26_voice_audio_install(on_report) };
    }

    /// The choice changed: the shim reports again and applies the answer.
    pub(super) fn refresh() {
        unsafe { gs26_voice_audio_refresh() };
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
    fn overridden(mut r: SessionReport) -> SessionReport {
        r.speaker_override = true;
        r
    }
    use OutputChoice::{Phone, Speaker};

    #[test]
    fn speaker_choice_forces_the_loudspeaker_even_with_a_headset() {
        for outputs in [&["Receiver"][..], &["Headphones"], &["BluetoothHFP"], &["USBAudio"], &[]] {
            assert_eq!(ios_port_override(Speaker, &report(PLAY_AND_RECORD, outputs)), PortOverride::Speaker, "{outputs:?}");
        }
        assert_eq!(ios_port_override(Speaker, &overridden(report(PLAY_AND_RECORD, &["Speaker"]))), PortOverride::Leave);
    }

    #[test]
    fn phone_choice_is_the_earpiece_or_headset_and_undoes_only_its_own_override() {
        for outputs in [&["Receiver"][..], &["Headphones"], &["BluetoothHFP"], &["Speaker"]] {
            assert_eq!(ios_port_override(Phone, &report(PLAY_AND_RECORD, outputs)), PortOverride::Leave, "{outputs:?}");
        }
        assert_eq!(ios_port_override(Phone, &overridden(report(PLAY_AND_RECORD, &["Speaker"]))), PortOverride::Clear);
    }

    #[test]
    fn the_choice_is_reapplied_after_interruptions_route_changes_and_resets() {
        // An interruption (call, Siri) ends and WebKit reopens play-and-record
        // on the receiver; a headset arrives; media services reset. Each
        // report gets the answer that keeps the operator's choice.
        let mut ended = report(PLAY_AND_RECORD, &["Receiver"]);
        ended.event = "interruption".into();
        ended.detail = "ended".into();
        assert_eq!(ios_port_override(Speaker, &ended), PortOverride::Speaker);
        assert_eq!(ios_port_override(Phone, &ended), PortOverride::Leave);
        let mut headset = report(PLAY_AND_RECORD, &["BluetoothHFP"]);
        headset.detail = "NewDeviceAvailable".into();
        assert_eq!(ios_port_override(Speaker, &headset), PortOverride::Speaker, "a new headset does not take the speaker's place");
        assert_eq!(ios_port_override(Phone, &headset), PortOverride::Leave, "phone mode plays on the headset, like a call");
        let mut reset = report(PLAY_AND_RECORD, &["Receiver"]);
        reset.event = "reset".into();
        assert_eq!(ios_port_override(Speaker, &reset), PortOverride::Speaker);
        // Playback categories (the microphone closed) are never routed.
        for category in ["AVAudioSessionCategorySoloAmbient", "AVAudioSessionCategoryAmbient", "AVAudioSessionCategoryPlayback", ""] {
            assert_eq!(ios_port_override(Speaker, &report(category, &["Receiver"])), PortOverride::Leave, "{category}");
            assert_eq!(ios_port_override(Phone, &overridden(report(category, &["Speaker"]))), PortOverride::Leave, "{category}");
        }
    }

    #[test]
    fn ios_route_says_what_plays_and_whether_it_is_the_choice() {
        assert_eq!(ios_route(Phone, &report(PLAY_AND_RECORD, &["Receiver"])), (Some("the earpiece".into()), true));
        assert_eq!(ios_route(Phone, &report(PLAY_AND_RECORD, &["BluetoothHFP"])), (Some("the Bluetooth headset".into()), true));
        assert_eq!(ios_route(Phone, &report(PLAY_AND_RECORD, &["Speaker"])), (Some("the speaker".into()), false));
        assert_eq!(ios_route(Speaker, &report(PLAY_AND_RECORD, &["Speaker"])), (Some("the speaker".into()), true));
        assert_eq!(ios_route(Speaker, &report(PLAY_AND_RECORD, &["AirPlay"])), (Some("AirPlay".into()), false));
        assert_eq!(ios_route(Speaker, &report("AVAudioSessionCategoryPlayback", &["Receiver"])), (None, true));
    }

    #[test]
    fn android_picks_the_called_for_device() {
        use android_type::*;
        let all = [EARPIECE, SPEAKER, WIRED_HEADSET, BLUETOOTH_SCO];
        assert_eq!(android_pick(Speaker, &all), Some(SPEAKER));
        assert_eq!(android_pick(Phone, &all), Some(BLUETOOTH_SCO), "a connected headset, like a call");
        assert_eq!(android_pick(Phone, &[EARPIECE, SPEAKER, WIRED_HEADSET]), Some(WIRED_HEADSET));
        assert_eq!(android_pick(Phone, &[EARPIECE, SPEAKER]), Some(EARPIECE));
        assert_eq!(android_pick(Phone, &[SPEAKER, BLE_HEADSET, USB_HEADSET]), Some(BLE_HEADSET));
        // A tablet with no earpiece cannot play "phone"; a choice that
        // cannot be met is reported, never silently swapped.
        assert_eq!(android_pick(Phone, &[SPEAKER]), None);
        assert_eq!(android_pick(Speaker, &[EARPIECE, WIRED_HEADSET]), None);
        assert_eq!(android_types_from_mask((1 << EARPIECE) | (1 << SPEAKER) | (1 << BLE_HEADSET)), vec![EARPIECE, SPEAKER, BLE_HEADSET]);
        assert_eq!(android_types_from_mask(0), Vec::<i32>::new());
        assert_eq!(android_label(BLUETOOTH_SCO), "the Bluetooth headset");
    }

    #[test]
    fn the_choice_persists_per_device_and_defaults_to_phone() {
        let dir = std::env::temp_dir().join(format!("gs26-voice-output-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let path = dir.join("gs26").join("voice_audio_output.json");
        assert_eq!(read_choice_from(&path), Phone, "nothing saved");
        write_choice_to(&path, Speaker);
        assert_eq!(read_choice_from(&path), Speaker);
        write_choice_to(&path, Phone);
        assert_eq!(read_choice_from(&path), Phone);
        std::fs::write(&path, b"{not json").unwrap();
        assert_eq!(read_choice_from(&path), Phone, "an unreadable file falls back to the default");
        std::fs::write(&path, br#"{"choice":"tannoy"}"#).unwrap();
        assert_eq!(read_choice_from(&path), Phone);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn page_state_offers_both_outputs_and_explains_an_unavailable_choice() {
        assert!(native_output_control("ios") && native_output_control("android"));
        for desktop in ["macos", "linux", "windows", "web"] {
            assert!(!native_output_control(desktop), "{desktop} uses the web view's output selection");
        }
        let ok = output_state(Speaker, Some("the speaker"), true);
        assert_eq!(ok["control"], "native");
        assert_eq!(ok["options"], serde_json::json!(["phone", "speaker"]));
        assert_eq!(ok["choice"], "speaker");
        assert!(ok["note"].is_null());
        let missing = output_state(Speaker, Some("AirPlay"), false);
        assert_eq!(missing["note"], "The speaker is not available; playing through AirPlay.");
        let phone = output_state(Phone, Some("the speaker"), false);
        assert_eq!(phone["note"], "The earpiece or headset is not available; playing through the speaker.");
        assert_eq!(output_state(Phone, None, true)["route"], serde_json::Value::Null);
    }

    #[test]
    fn page_messages_set_the_choice_and_bump_the_revision() {
        let before = revision();
        page_message(&serde_json::json!({"type": "gs26-audio-output", "choice": "speaker", "active": true}));
        assert_eq!(output_snapshot().choice, Speaker);
        assert!(output_snapshot().active);
        assert!(revision() > before);
        let same = revision();
        page_message(&serde_json::json!({"type": "gs26-audio-output", "choice": "speaker", "active": true}));
        assert_eq!(revision(), same, "nothing changed");
        page_message(&serde_json::json!({"type": "other", "choice": "phone"}));
        page_message(&serde_json::json!({"type": "gs26-audio-output", "choice": "loud"}));
        assert_eq!(output_snapshot().choice, Speaker, "unknown messages and choices are ignored");
        page_message(&serde_json::json!({"type": "gs26-audio-output", "choice": "phone", "active": false}));
        assert_eq!(output_snapshot().choice, Phone);
        assert!(!output_snapshot().active);
    }

    #[test]
    fn shim_reports_parse_with_missing_fields_and_log_compactly() {
        let parsed: SessionReport = serde_json::from_str(
            r#"{"event":"route","detail":"CategoryChange","category":"AVAudioSessionCategoryPlayAndRecord","mode":"AVAudioSessionModeVideoChat","outputs":["Receiver"],"inputs":["MicrophoneBuiltIn"],"record_permission":"granted","sample_rate":48000,"extra":1}"#,
        )
        .unwrap();
        assert_eq!(ios_port_override(Speaker, &parsed), PortOverride::Speaker);
        assert_eq!(
            log_line(&parsed),
            "[voice-audio] route (CategoryChange): category PlayAndRecord mode VideoChat out [Receiver] in [MicrophoneBuiltIn] 48000 Hz mic granted"
        );
        let empty: SessionReport = serde_json::from_str("{}").unwrap();
        assert_eq!(ios_port_override(Speaker, &empty), PortOverride::Leave);
        assert!(log_line(&empty).contains("category ? mode ?"));
    }

    #[test]
    fn page_info_names_the_platform_and_the_microphone_permission() {
        assert_eq!(page_info()["platform"], platform());
        // Desktop hosts choose the output in the web view, not natively.
        assert!(page_info().get("audio_output").is_none());
        handle_report(r#"{"event":"poll","category":"AVAudioSessionCategoryPlayAndRecord","outputs":["Receiver"],"record_permission":"denied"}"#);
        assert_eq!(page_info()["record_permission"], "denied");
        assert_eq!(handle_report("not json"), PortOverride::Leave);
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
