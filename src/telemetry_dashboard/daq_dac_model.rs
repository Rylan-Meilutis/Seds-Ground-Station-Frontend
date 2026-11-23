//! Pure model for the DAQ signal generator / ADC validation panel.
//! Wire shapes follow the GS HTTP API (`/api/daq/dac*`). Volts are connector
//! volts at P10 unless a field says otherwise.
use serde::Deserialize;
use serde_json::{Value, json};

pub(super) const CHANNEL_LABELS: [&str; 2] = [
    "DAC1 OUT1 (PA4) -> P10 pin 2",
    "DAC1 OUT2 (PA5) -> P10 pin 5",
];

#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
pub(super) struct DacChannel {
    #[serde(default)]
    pub channel: u8,
    #[serde(default)]
    pub label: String,
    #[serde(default)]
    pub desired: Option<Value>,
    #[serde(default)]
    pub active: bool,
    #[serde(default)]
    pub waveform: String,
    #[serde(default)]
    pub amplitude_v: f64,
    #[serde(default)]
    pub offset_v: f64,
    #[serde(default)]
    pub frequency_hz: f64,
    #[serde(default)]
    pub steps: u32,
    #[serde(default)]
    pub dwell_ms: u32,
    #[serde(default)]
    pub clamped: Vec<String>,
    #[serde(default)]
    pub output_v: Option<f64>,
    #[serde(default)]
    pub readback_v: Option<f64>,
    #[serde(default)]
    pub last_result: String,
    #[serde(default)]
    pub disable_reason: String,
    #[serde(default)]
    pub sequence: u32,
    #[serde(default)]
    pub update_hz: f64,
    #[serde(default)]
    pub samples: u32,
    #[serde(default)]
    pub phase_deg: f64,
    #[serde(default)]
    pub duty_pct: Option<f64>,
    #[serde(default)]
    pub ramp_down: bool,
    #[serde(default)]
    pub log_sweep: bool,
    #[serde(default)]
    pub table_len: u32,
    #[serde(default)]
    pub table_crc: String,
    #[serde(default)]
    pub table_crc_ok: Option<bool>,
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
pub(super) struct DacTable {
    #[serde(default)]
    pub state: String,
    #[serde(default)]
    pub len: u32,
    #[serde(default)]
    pub shape_crc: String,
    #[serde(default)]
    pub computed_crc: String,
    #[serde(default)]
    pub chunks_received: u32,
    #[serde(default)]
    pub chunk_count: u32,
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
pub(super) struct DacUpload {
    #[serde(default)]
    pub state: String,
    #[serde(default)]
    pub channel: Option<u8>,
    #[serde(default)]
    pub phase: Option<String>,
    #[serde(default)]
    pub chunks_confirmed: u32,
    #[serde(default)]
    pub chunk_count: u32,
    #[serde(default)]
    pub resent_chunks: u32,
    #[serde(default)]
    pub shape_crc: Option<String>,
    #[serde(default)]
    pub message: Option<String>,
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
pub(super) struct DacInput {
    pub id: String,
    #[serde(default)]
    pub label: String,
    #[serde(default)]
    pub unit: String,
    #[serde(default)]
    pub valid_range: Option<[f64; 2]>,
    #[serde(default)]
    pub stream_hz: Option<f64>,
    #[serde(default)]
    pub needs_attenuator: bool,
    #[serde(default)]
    pub model: String,
}

impl DacInput {
    pub(super) fn is_load_cell(&self) -> bool {
        is_load_cell_target(&self.id)
    }
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
pub(super) struct DacStatus {
    #[serde(default)]
    pub allowed: bool,
    #[serde(default)]
    pub refusal: Option<String>,
    #[serde(default)]
    pub flight_state: Option<String>,
    #[serde(default)]
    pub daq_flight_state: Option<u8>,
    #[serde(default)]
    pub status_age_ms: Option<u64>,
    #[serde(default)]
    pub status_fresh: bool,
    #[serde(default)]
    pub any_active: bool,
    #[serde(default)]
    pub buffer_gain: Option<f64>,
    #[serde(default)]
    pub dac_pin_range_v: Option<[f64; 2]>,
    #[serde(default)]
    pub connector_range_v: Option<[f64; 2]>,
    #[serde(default)]
    pub channels: Vec<DacChannel>,
    #[serde(default)]
    pub inputs: Vec<DacInput>,
    #[serde(default)]
    pub table: Option<DacTable>,
    #[serde(default)]
    pub upload: Option<DacUpload>,
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
pub(super) struct ValidationSeries {
    #[serde(default)]
    pub t_ms: Vec<f64>,
    #[serde(default)]
    pub measured: Vec<Option<f64>>,
    #[serde(default)]
    pub expected: Vec<Option<f64>>,
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
pub(super) struct ValidationAnalysis {
    #[serde(default)]
    pub samples: u64,
    #[serde(default)]
    pub offset_error: Option<f64>,
    #[serde(default)]
    pub gain_error_pct: Option<f64>,
    #[serde(default)]
    pub noise_rms: Option<f64>,
    #[serde(default)]
    pub measured_amplitude: Option<f64>,
    #[serde(default)]
    pub expected_amplitude: Option<f64>,
    #[serde(default)]
    pub measured_frequency_hz: Option<f64>,
    #[serde(default)]
    pub expected_frequency_hz: Option<f64>,
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
pub(super) struct Validation {
    #[serde(default)]
    pub target: Option<String>,
    #[serde(default)]
    pub channel: Option<u8>,
    #[serde(default)]
    pub attenuation: Option<f64>,
    #[serde(default)]
    pub load_cell_polarity: Option<String>,
    #[serde(default)]
    pub warnings: Vec<String>,
    #[serde(default)]
    pub series: ValidationSeries,
    #[serde(default)]
    pub analysis: Option<ValidationAnalysis>,
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
pub(super) struct SweepProgress {
    #[serde(default)]
    pub step: u32,
    #[serde(default)]
    pub steps: u32,
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
pub(super) struct SweepPoint {
    #[serde(default)]
    pub dac_pin_v: Option<f64>,
    #[serde(default)]
    pub expected: Option<f64>,
    #[serde(default)]
    pub measured_mean: Option<f64>,
    #[serde(default)]
    pub measured_std: Option<f64>,
    #[serde(default)]
    pub readback_v: Option<f64>,
    #[serde(default)]
    pub samples: u64,
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
pub(super) struct SweepCheck {
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub value: Option<f64>,
    #[serde(default)]
    pub limit: Option<f64>,
    #[serde(default)]
    pub pass: bool,
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
pub(super) struct SweepReport {
    #[serde(default)]
    pub target: Option<String>,
    #[serde(default)]
    pub channel: Option<u8>,
    #[serde(default)]
    pub attenuation: Option<f64>,
    #[serde(default)]
    pub points: Vec<SweepPoint>,
    #[serde(default)]
    pub offset_error: Option<f64>,
    #[serde(default)]
    pub gain_error_pct: Option<f64>,
    #[serde(default)]
    pub max_nonlinearity_pct_fs: Option<f64>,
    #[serde(default)]
    pub noise_rms: Option<f64>,
    #[serde(default)]
    pub checks: Vec<SweepCheck>,
    #[serde(default)]
    pub pass: bool,
    #[serde(default)]
    pub saved_path: Option<String>,
    /// Frequency-response preset only.
    #[serde(default)]
    pub frequency_points: Vec<FrequencyPoint>,
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
pub(super) struct FrequencyPoint {
    #[serde(default)]
    pub frequency_hz: Option<f64>,
    #[serde(default)]
    pub expected_amplitude: Option<f64>,
    #[serde(default)]
    pub measured_amplitude: Option<f64>,
    #[serde(default)]
    pub gain_db: Option<f64>,
    #[serde(default)]
    pub samples: u64,
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
pub(super) struct SweepStatus {
    #[serde(default)]
    pub state: String,
    #[serde(default)]
    pub progress: Option<SweepProgress>,
    #[serde(default)]
    pub message: Option<String>,
    #[serde(default)]
    pub report: Option<SweepReport>,
}

impl SweepStatus {
    pub(super) fn running(&self) -> bool {
        self.state == "running"
    }
}

/// "running · step 3 of 11", or the terminal state.
pub(super) fn sweep_progress_text(status: &SweepStatus) -> String {
    let state = match status.state.as_str() {
        "" | "idle" => "No sweep running",
        "running" => "Sweep running",
        "done" => "Sweep finished",
        "failed" => "Sweep failed",
        "cancelled" => "Sweep cancelled",
        other => other,
    };
    match (&status.progress, status.running()) {
        (Some(progress), true) if progress.steps > 0 => {
            format!("{state} · step {} of {}", progress.step, progress.steps)
        }
        _ => state.to_string(),
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Waveform {
    Dc,
    Square,
    Sine,
    Triangle,
    Sawtooth,
    Staircase,
    Chirp,
    Noise,
    Arbitrary,
}

impl Waveform {
    pub(super) const ALL: [Waveform; 9] = [
        Waveform::Dc,
        Waveform::Sine,
        Waveform::Square,
        Waveform::Triangle,
        Waveform::Sawtooth,
        Waveform::Staircase,
        Waveform::Chirp,
        Waveform::Noise,
        Waveform::Arbitrary,
    ];
    pub(super) fn wire(self) -> &'static str {
        match self {
            Waveform::Dc => "dc",
            Waveform::Square => "square",
            Waveform::Sine => "sine",
            Waveform::Triangle => "triangle",
            Waveform::Sawtooth => "sawtooth",
            Waveform::Staircase => "staircase",
            Waveform::Chirp => "chirp",
            Waveform::Noise => "noise",
            Waveform::Arbitrary => "arbitrary",
        }
    }
    pub(super) fn label(self) -> &'static str {
        match self {
            Waveform::Dc => "DC",
            Waveform::Square => "Square",
            Waveform::Sine => "Sine",
            Waveform::Triangle => "Triangle",
            Waveform::Sawtooth => "Sawtooth / ramp",
            Waveform::Staircase => "Staircase",
            Waveform::Chirp => "Chirp (frequency sweep)",
            Waveform::Noise => "Noise",
            Waveform::Arbitrary => "Custom (equation / WAV)",
        }
    }
    pub(super) fn from_wire(value: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|w| w.wire() == value)
    }
    /// Which inputs the operator fills for this waveform.
    pub(super) fn fields(self) -> WaveFields {
        let periodic = matches!(
            self,
            Waveform::Square | Waveform::Sine | Waveform::Triangle | Waveform::Sawtooth
        );
        WaveFields {
            amplitude: self != Waveform::Dc,
            offset: true,
            frequency: !matches!(self, Waveform::Dc | Waveform::Staircase),
            steps: self == Waveform::Staircase,
            dwell: self == Waveform::Staircase,
            phase: periodic || matches!(self, Waveform::Chirp | Waveform::Arbitrary),
            duty: self == Waveform::Square,
            ramp: self == Waveform::Sawtooth,
            chirp: self == Waveform::Chirp,
            custom: self == Waveform::Arbitrary,
        }
    }
    /// What the frequency field means for this waveform.
    pub(super) fn frequency_label(self) -> &'static str {
        match self {
            Waveform::Chirp => "Start frequency (Hz)",
            Waveform::Noise => "Sample rate (S/s)",
            Waveform::Arbitrary => "Frequency, periods/s (Hz)",
            _ => "Frequency (Hz)",
        }
    }
    pub(super) fn frequency_max(self) -> f64 {
        if self == Waveform::Noise { 200_000.0 } else { 10_000.0 }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct WaveFields {
    pub amplitude: bool,
    pub offset: bool,
    pub frequency: bool,
    pub steps: bool,
    pub dwell: bool,
    pub phase: bool,
    pub duty: bool,
    pub ramp: bool,
    pub chirp: bool,
    pub custom: bool,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) enum CustomKind {
    #[default]
    Expression,
    Wav,
}

/// The custom-shape part of a draft. The shape is normalized (-1..1); the
/// amplitude, offset, phase and frequency fields scale and time it.
#[derive(Clone, Debug, PartialEq)]
pub(super) struct CustomDraft {
    pub kind: CustomKind,
    pub expression: String,
    /// Shape length; blank lets the GS choose.
    pub samples: String,
    pub wav_name: String,
    pub wav_base64: String,
    pub wav_normalize: bool,
    /// WAV playback rate (S/s); blank uses the file's rate.
    pub wav_rate: String,
}

impl Default for CustomDraft {
    fn default() -> Self {
        Self {
            kind: CustomKind::Expression,
            expression: "0.8*sin(2*pi*x) + 0.2*sign(sin(2*pi*5*x))".into(),
            samples: String::new(),
            wav_name: String::new(),
            wav_base64: String::new(),
            wav_normalize: true,
            wav_rate: String::new(),
        }
    }
}

/// Operator-entered configuration for one channel, kept as typed text.
#[derive(Clone, Debug, PartialEq)]
pub(super) struct ConfigDraft {
    pub waveform: Waveform,
    pub amplitude_v: String,
    pub offset_v: String,
    pub frequency_hz: String,
    pub steps: String,
    pub dwell_ms: String,
    pub phase_deg: String,
    pub duty_pct: String,
    pub ramp_down: bool,
    pub frequency_stop_hz: String,
    pub sweep_s: String,
    pub log_sweep: bool,
    pub custom: CustomDraft,
}

impl Default for ConfigDraft {
    fn default() -> Self {
        Self {
            waveform: Waveform::Dc,
            amplitude_v: "1.0".into(),
            offset_v: "2.0".into(),
            frequency_hz: "0.5".into(),
            steps: "8".into(),
            dwell_ms: "1000".into(),
            phase_deg: "0".into(),
            duty_pct: "50".into(),
            ramp_down: false,
            frequency_stop_hz: "100".into(),
            sweep_s: "10".into(),
            log_sweep: true,
            custom: CustomDraft::default(),
        }
    }
}

fn number_text(value: f64) -> String {
    let text = format!("{value:.4}");
    let text = text.trim_end_matches('0').trim_end_matches('.');
    if text.is_empty() || text == "-" {
        "0".into()
    } else {
        text.to_string()
    }
}

impl ConfigDraft {
    /// Start from what the GS keeps alive, else from the applied values.
    pub(super) fn from_channel(channel: &DacChannel) -> Self {
        let mut draft = Self::default();
        let num = |source: &Value, key: &str| source.get(key).and_then(Value::as_f64);
        if let Some(desired) = channel.desired.as_ref().filter(|v| v.is_object()) {
            if let Some(w) = desired
                .get("waveform")
                .and_then(Value::as_str)
                .and_then(Waveform::from_wire)
            {
                draft.waveform = w;
            }
            if let Some(v) = num(desired, "amplitude_v") {
                draft.amplitude_v = number_text(v);
            }
            if let Some(v) = num(desired, "offset_v") {
                draft.offset_v = number_text(v);
            }
            if let Some(v) = num(desired, "frequency_hz") {
                draft.frequency_hz = number_text(v);
            }
            if let Some(v) = num(desired, "steps") {
                draft.steps = number_text(v.round());
            }
            if let Some(v) = num(desired, "dwell_ms") {
                draft.dwell_ms = number_text(v.round());
            }
            if let Some(v) = num(desired, "phase_deg") {
                draft.phase_deg = number_text(v);
            }
            if let Some(v) = num(desired, "duty_pct") {
                draft.duty_pct = number_text(v);
            }
            if let Some(v) = num(desired, "frequency_stop_hz") {
                draft.frequency_stop_hz = number_text(v);
            }
            if let Some(v) = num(desired, "sweep_s") {
                draft.sweep_s = number_text(v);
            }
            draft.ramp_down = desired.get("ramp_down").and_then(Value::as_bool).unwrap_or(false);
            if let Some(v) = desired.get("log_sweep").and_then(Value::as_bool) {
                draft.log_sweep = v;
            }
            if let Some(custom) = desired.get("custom").filter(|c| c.is_object()) {
                match custom.get("kind").and_then(Value::as_str) {
                    Some("expression") => {
                        draft.custom.kind = CustomKind::Expression;
                        if let Some(e) = custom.get("expression").and_then(Value::as_str) {
                            draft.custom.expression = e.to_string();
                        }
                    }
                    // The station keeps no WAV data; re-open the file to change it.
                    Some("wav") => draft.custom.kind = CustomKind::Wav,
                    _ => {}
                }
            }
        } else if channel.active {
            if let Some(w) = Waveform::from_wire(&channel.waveform) {
                draft.waveform = w;
            }
            draft.amplitude_v = number_text(channel.amplitude_v);
            draft.offset_v = number_text(channel.offset_v);
            if channel.frequency_hz > 0.0 {
                draft.frequency_hz = number_text(channel.frequency_hz);
            }
            if channel.steps >= 2 {
                draft.steps = channel.steps.to_string();
            }
            if channel.dwell_ms > 0 {
                draft.dwell_ms = channel.dwell_ms.to_string();
            }
        }
        draft
    }
}

fn parse_finite(text: &str, name: &str) -> Result<f64, String> {
    let value: f64 = text
        .trim()
        .parse()
        .map_err(|_| format!("{name} must be a number"))?;
    if value.is_finite() {
        Ok(value)
    } else {
        Err(format!("{name} must be a number"))
    }
}

fn parse_whole(text: &str, name: &str, min: u32, max: u32) -> Result<u32, String> {
    let value: u32 = text
        .trim()
        .parse()
        .map_err(|_| format!("{name} must be a whole number"))?;
    if (min..=max).contains(&value) {
        Ok(value)
    } else {
        Err(format!("{name} must be {min}..{max}"))
    }
}

/// Body for `POST /api/daq/dac/config` (and `/preview`). Range clamping is
/// the GS's job (it reports `clamped`); this only rejects text that is not a
/// usable number. Every key the waveform uses is sent.
pub(super) fn config_body(channel: u8, draft: &ConfigDraft) -> Result<Value, String> {
    let w = draft.waveform;
    let fields = w.fields();
    let offset = parse_finite(&draft.offset_v, "Offset")?;
    let amplitude = if fields.amplitude {
        let value = parse_finite(&draft.amplitude_v, "Amplitude")?;
        if value < 0.0 {
            return Err("Amplitude must not be negative".into());
        }
        value
    } else {
        0.0
    };
    // A WAV clip may leave the frequency blank: its playback rate sets it.
    let wav_without_rate = fields.custom
        && draft.custom.kind == CustomKind::Wav
        && draft.frequency_hz.trim().is_empty();
    let frequency = if fields.frequency && !wav_without_rate {
        let value = parse_finite(&draft.frequency_hz, "Frequency")?;
        if value <= 0.0 {
            return Err("Frequency must be above 0 Hz".into());
        }
        Some(value)
    } else {
        None
    };
    let mut body = json!({
        "channel": channel,
        "waveform": w.wire(),
        "amplitude_v": amplitude,
        "offset_v": offset,
    });
    match frequency {
        Some(f) => body["frequency_hz"] = json!(f),
        None if !fields.custom => body["frequency_hz"] = json!(0.0),
        None => {}
    }
    if !fields.custom {
        body["steps"] = json!(if fields.steps { parse_whole(&draft.steps, "Steps", 2, 256)? } else { 2 });
        body["dwell_ms"] =
            json!(if fields.dwell { parse_whole(&draft.dwell_ms, "Dwell", 10, 20_000)? } else { 1000 });
    }
    if fields.phase {
        body["phase_deg"] = json!(parse_finite(&draft.phase_deg, "Phase")?);
    }
    if fields.duty {
        let duty = parse_finite(&draft.duty_pct, "Duty")?;
        if !(1.0..=99.0).contains(&duty) {
            return Err("Duty must be 1..99 %".into());
        }
        body["duty_pct"] = json!(duty);
    }
    if fields.ramp && draft.ramp_down {
        body["ramp_down"] = json!(true);
    }
    if fields.chirp {
        let stop = parse_finite(&draft.frequency_stop_hz, "Stop frequency")?;
        if stop <= 0.0 {
            return Err("Stop frequency must be above 0 Hz".into());
        }
        let sweep = parse_finite(&draft.sweep_s, "Sweep time")?;
        if sweep <= 0.0 {
            return Err("Sweep time must be above 0 s".into());
        }
        body["frequency_stop_hz"] = json!(stop);
        body["sweep_s"] = json!(sweep);
        if draft.log_sweep {
            body["log_sweep"] = json!(true);
        }
    }
    if fields.custom {
        body["custom"] = custom_body(&draft.custom)?;
    }
    Ok(body)
}

fn custom_body(custom: &CustomDraft) -> Result<Value, String> {
    let samples = if custom.samples.trim().is_empty() {
        None
    } else {
        Some(parse_whole(&custom.samples, "Shape samples", 16, 512)?)
    };
    let mut body = match custom.kind {
        CustomKind::Expression => {
            let expression = custom.expression.trim();
            if expression.is_empty() {
                return Err("Enter an equation f(x), x in [0, 1)".into());
            }
            if expression.chars().count() > 512 {
                return Err("The equation is longer than 512 characters".into());
            }
            json!({ "kind": "expression", "expression": expression })
        }
        CustomKind::Wav => {
            if custom.wav_base64.is_empty() {
                return Err("Open a WAV file first".into());
            }
            let mut body = json!({
                "kind": "wav",
                "data_base64": custom.wav_base64,
                "normalize": custom.wav_normalize,
            });
            if !custom.wav_rate.trim().is_empty() {
                let rate = parse_finite(&custom.wav_rate, "Playback rate")?;
                if !(1.0..=200_000.0).contains(&rate) {
                    return Err("Playback rate must be 1..200000 S/s".into());
                }
                body["playback_rate_hz"] = json!(rate);
            }
            body
        }
    };
    if let Some(n) = samples {
        body["samples"] = json!(n);
    }
    Ok(body)
}

/// Local note when the requested span leaves the reachable connector range.
/// The GS still decides and reports what it clamped.
pub(super) fn range_hint(draft: &ConfigDraft, range: Option<[f64; 2]>) -> Option<String> {
    let [lo, hi] = range?;
    let offset = parse_finite(&draft.offset_v, "Offset").ok()?;
    let amplitude = if draft.waveform.fields().amplitude {
        parse_finite(&draft.amplitude_v, "Amplitude").ok()?.abs()
    } else {
        0.0
    };
    let (min, max) = (offset - amplitude, offset + amplitude);
    (min < lo - 1e-9 || max > hi + 1e-9).then(|| {
        format!(
            "Requested {} to {} V is outside the reachable {} to {} V at P10; the GS will clamp it.",
            number_text(min),
            number_text(max),
            number_text(lo),
            number_text(hi)
        )
    })
}

pub(super) fn disable_body(channel: Option<u8>) -> Value {
    match channel {
        Some(channel) => json!({ "channel": channel }),
        None => json!({ "channel": "all" }),
    }
}

pub(super) fn is_load_cell_target(id: &str) -> bool {
    id.starts_with("loadcell")
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Polarity {
    SMinus,
    SPlus,
}

impl Polarity {
    pub(super) fn wire(self) -> &'static str {
        match self {
            Polarity::SMinus => "s_minus",
            Polarity::SPlus => "s_plus",
        }
    }
    pub(super) fn from_wire(value: &str) -> Self {
        if value == "s_plus" {
            Polarity::SPlus
        } else {
            Polarity::SMinus
        }
    }
}

pub(super) fn parse_attenuation(text: &str) -> Result<f64, String> {
    let value = parse_finite(text, "Attenuation")?;
    if value > 0.0 {
        Ok(value)
    } else {
        Err("Attenuation must be above 0".into())
    }
}

/// Body for `POST /api/daq/dac/validation`. `None` turns validation off.
pub(super) fn validation_body(
    target: Option<&str>,
    channel: u8,
    attenuation: &str,
    polarity: Polarity,
) -> Result<Value, String> {
    let Some(target) = target.filter(|t| !t.is_empty()) else {
        return Ok(json!({ "target": null }));
    };
    let mut body = json!({
        "target": target,
        "channel": channel,
        "attenuation": parse_attenuation(attenuation)?,
    });
    if is_load_cell_target(target) {
        body["load_cell_polarity"] = json!(polarity.wire());
    }
    Ok(body)
}

/// Body for `POST /api/daq/dac/sweep`. Blank steps/dwell use the GS default.
pub(super) fn sweep_body(
    target: &str,
    channel: u8,
    attenuation: &str,
    polarity: Polarity,
    steps: &str,
    dwell_ms: &str,
    preset: SweepPreset,
) -> Result<Value, String> {
    if target.is_empty() {
        return Err("Choose a target input first".into());
    }
    let mut body = validation_body(Some(target), channel, attenuation, polarity)?;
    body["preset"] = json!(preset.wire());
    if !steps.trim().is_empty() {
        body["steps"] = json!(parse_whole(steps, "Sweep steps", 2, 101)?);
    }
    if !dwell_ms.trim().is_empty() {
        body["dwell_ms"] = json!(parse_whole(dwell_ms, "Sweep dwell", 100, 60_000)?);
    }
    Ok(body)
}

/// Validation sweep presets.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) enum SweepPreset {
    /// DC steps across the input's range, with a line fit.
    #[default]
    Linearity,
    /// Log-spaced sines up to a quarter of the stream rate, gain per point.
    FrequencyResponse,
}

impl SweepPreset {
    pub(super) const ALL: [SweepPreset; 2] = [SweepPreset::Linearity, SweepPreset::FrequencyResponse];
    pub(super) fn wire(self) -> &'static str {
        match self {
            SweepPreset::Linearity => "linearity",
            SweepPreset::FrequencyResponse => "frequency_response",
        }
    }
    pub(super) fn label(self) -> &'static str {
        match self {
            SweepPreset::Linearity => "Step linearity (DC steps)",
            SweepPreset::FrequencyResponse => "Frequency response (sine steps)",
        }
    }
    pub(super) fn from_wire(value: &str) -> Self {
        Self::ALL.into_iter().find(|p| p.wire() == value).unwrap_or_default()
    }
}

/// Why enabling/configuring/sweeping is unavailable, or `None` when it may go.
/// Disable and cancel are not gated by this.
pub(super) fn enable_block_reason(
    status: Option<&DacStatus>,
    permitted: bool,
    rrls: bool,
) -> Option<String> {
    if rrls {
        return Some("RRLS is read-only.".into());
    }
    if !permitted {
        return Some("Your account cannot send DaqDacControl.".into());
    }
    let Some(status) = status else {
        return Some("DAQ DAC status has not loaded yet.".into());
    };
    if !status.allowed {
        return Some(
            status
                .refusal
                .clone()
                .filter(|r| !r.trim().is_empty())
                .unwrap_or_else(|| "DAC output is not allowed now.".into()),
        );
    }
    if !status.status_fresh {
        return Some("DAQ DAC status is stale.".into());
    }
    None
}

/// Disable/cancel stay usable unless the station cannot accept writes at all.
pub(super) fn disable_block_reason(permitted: bool, rrls: bool) -> Option<String> {
    if rrls {
        Some("RRLS is read-only.".into())
    } else if !permitted {
        Some("Your account cannot send DaqDacControl.".into())
    } else {
        None
    }
}

pub(super) fn status_age_text(status: &DacStatus) -> String {
    match status.status_age_ms {
        None => "No DAQ DAC status received".into(),
        Some(age) => {
            let age = if age < 10_000 {
                format!("{:.1} s", age as f64 / 1000.0)
            } else {
                format!("{} s", age / 1000)
            };
            if status.status_fresh {
                format!("DAQ status {age} old")
            } else {
                format!("DAQ status STALE ({age} old)")
            }
        }
    }
}

pub(super) fn daq_state_text(state: Option<u8>) -> String {
    match state {
        None => "no status".into(),
        Some(0) => "Startup (0)".into(),
        Some(1) => "Idle (1)".into(),
        Some(0xFF) => "unknown".into(),
        Some(other) => format!("state {other}"),
    }
}

pub(super) fn clamped_text(clamped: &[String]) -> Option<String> {
    (!clamped.is_empty()).then(|| format!("Clamped: {}", clamped.join(", ")))
}

pub(super) fn result_text(value: &str) -> &str {
    match value {
        "" | "ok" => "ok",
        "refused_state" => "refused (flight state)",
        "refused_unknown_state" => "refused (state unknown)",
        "bad_request" => "bad request",
        "hardware_error" => "hardware error",
        "not_ready" => "not ready",
        other => other,
    }
}

pub(super) fn disable_reason_text(value: &str) -> &str {
    match value {
        "" | "none" => "none",
        "commanded" => "commanded off",
        "flight_state" => "flight state left Startup/Idle",
        "abort" => "abort",
        "lease_expired" => "lease expired (GS link loss)",
        "hardware_error" => "hardware error",
        other => other,
    }
}

pub(super) fn fmt_opt(value: Option<f64>, precision: usize) -> String {
    match value {
        Some(v) if v.is_finite() => format!("{v:.precision$}"),
        _ => "—".into(),
    }
}

/// The operator-readable reason from a failed request. The request helpers
/// format failures as "HTTP 409: {body}" or "Export request failed (409): {body}";
/// bodies are `{"error": "..."}` and may be cut short.
pub(super) fn operator_error(raw: &str) -> String {
    let Some(start) = raw.find('{') else {
        return raw.to_string();
    };
    let body = &raw[start..];
    if let Ok(value) = serde_json::from_str::<Value>(body)
        && let Some(error) = value.get("error").and_then(Value::as_str)
    {
        return error.to_string();
    }
    // Truncated JSON: take the string after "error":" up to its closing quote.
    if let Some(at) = body.find("\"error\"") {
        let rest = body[at + 7..]
            .trim_start()
            .trim_start_matches(':')
            .trim_start();
        if let Some(rest) = rest.strip_prefix('"') {
            let mut out = String::new();
            let mut chars = rest.chars();
            while let Some(c) = chars.next() {
                match c {
                    '\\' => {
                        if let Some(next) = chars.next() {
                            out.push(next);
                        }
                    }
                    '"' => return out,
                    c => out.push(c),
                }
            }
            return format!("{out}…");
        }
    }
    raw.to_string()
}

/// A 2xx reply with no body still means the station accepted the request.
pub(super) fn accepted_reply(result: Result<Value, String>) -> Result<Value, String> {
    match result {
        Err(error)
            if !error.starts_with("HTTP ")
                && error.starts_with("EOF while parsing a value at line 1 column 0") =>
        {
            Ok(Value::Null)
        }
        other => other.map_err(|error| operator_error(&error)),
    }
}

/// SVG paths for the expected-vs-measured plot.
#[derive(Clone, Debug, PartialEq)]
pub(super) struct PlotGeometry {
    pub measured: String,
    pub expected: String,
    pub y_min: f64,
    pub y_max: f64,
    pub span_s: f64,
}

pub(super) fn plot_geometry(
    series: &ValidationSeries,
    width: f64,
    height: f64,
    pad: f64,
) -> Option<PlotGeometry> {
    let n = series.t_ms.len();
    if n < 2 {
        return None;
    }
    let value = |values: &[Option<f64>], i: usize| {
        values.get(i).copied().flatten().filter(|v| v.is_finite())
    };
    let mut y_min = f64::INFINITY;
    let mut y_max = f64::NEG_INFINITY;
    for i in 0..n {
        for v in [value(&series.measured, i), value(&series.expected, i)]
            .into_iter()
            .flatten()
        {
            y_min = y_min.min(v);
            y_max = y_max.max(v);
        }
    }
    if !y_min.is_finite() {
        return None;
    }
    if (y_max - y_min).abs() < 1e-9 {
        let pad_v = y_min.abs().max(1.0) * 0.05;
        y_min -= pad_v;
        y_max += pad_v;
    }
    let t0 = series.t_ms[0];
    let t1 = series.t_ms[n - 1];
    let span = (t1 - t0).max(1.0);
    let x = |i: usize| pad + (series.t_ms[i] - t0) / span * (width - 2.0 * pad);
    let y = |v: f64| height - pad - (v - y_min) / (y_max - y_min) * (height - 2.0 * pad);
    let path = |values: &[Option<f64>]| {
        let mut d = String::new();
        let mut pen_down = false;
        for i in 0..n {
            match value(values, i) {
                Some(v) => {
                    d.push_str(&format!(
                        "{}{:.1},{:.1} ",
                        if pen_down { "L" } else { "M" },
                        x(i),
                        y(v)
                    ));
                    pen_down = true;
                }
                None => pen_down = false,
            }
        }
        d.trim_end().to_string()
    };
    Some(PlotGeometry {
        measured: path(&series.measured),
        expected: path(&series.expected),
        y_min,
        y_max,
        span_s: span / 1000.0,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const STATUS: &str = r#"{
      "allowed": false,
      "refusal": "Flight state is Fill; DAC output is allowed only in Startup or Idle.",
      "flight_state": "Fill", "daq_flight_state": 3, "status_age_ms": 420, "status_fresh": true,
      "any_active": true, "buffer_gain": 2.0, "dac_pin_range_v": [0.2, 3.1],
      "connector_range_v": [0.4, 6.2],
      "channels": [{"channel": 0, "label": "DAC1 OUT1 (PA4) -> P10 pin 2",
        "desired": {"channel": 0, "waveform": "sine", "amplitude_v": 1.0, "offset_v": 2.0,
                    "frequency_hz": 0.5, "steps": 8, "dwell_ms": 1000},
        "active": true, "waveform": "sine", "amplitude_v": 1.0, "offset_v": 2.0,
        "frequency_hz": 0.5, "steps": 0, "dwell_ms": 0, "clamped": ["amplitude"],
        "output_v": 2.5, "readback_v": null, "last_result": "ok",
        "disable_reason": "none", "sequence": 17}],
      "inputs": [{"id": "mcp_ch2", "label": "MCP CH2 (P6 pin 3)", "unit": "V",
        "valid_range": [0.0, 2.4], "stream_hz": 4.0, "needs_attenuator": false,
        "model": "reported = V_in"},
        {"id": "loadcell_kg1000", "label": "1000 kg", "unit": "V", "valid_range": [0.0, 2.4],
         "stream_hz": 40.0, "needs_attenuator": true, "model": "0.218 + 100.9 x V_in"}]
    }"#;

    fn status() -> DacStatus {
        serde_json::from_str(STATUS).unwrap()
    }

    #[test]
    fn contract_status_is_parsed() {
        let s = status();
        assert!(s.any_active && !s.allowed && s.status_fresh);
        assert_eq!(s.connector_range_v, Some([0.4, 6.2]));
        assert_eq!(s.channels[0].clamped, vec!["amplitude".to_string()]);
        assert_eq!(s.channels[0].readback_v, None);
        assert!(s.inputs[1].is_load_cell() && !s.inputs[0].is_load_cell());
        assert_eq!(
            clamped_text(&s.channels[0].clamped).unwrap(),
            "Clamped: amplitude"
        );
        assert_eq!(clamped_text(&[]), None);
        // An empty body from an older station still parses to a safe "not allowed".
        let empty: DacStatus = serde_json::from_str("{}").unwrap();
        assert!(!empty.allowed && !empty.any_active);
    }

    #[test]
    fn waveform_shows_only_relevant_fields() {
        let dc = Waveform::Dc.fields();
        assert!(dc.offset && !dc.amplitude && !dc.frequency && !dc.steps && !dc.dwell);
        for w in [Waveform::Square, Waveform::Sine, Waveform::Triangle] {
            let f = w.fields();
            assert!(
                f.offset && f.amplitude && f.frequency && !f.steps && !f.dwell,
                "{w:?}"
            );
        }
        let st = Waveform::Staircase.fields();
        assert!(st.offset && st.amplitude && !st.frequency && st.steps && st.dwell);
        for w in Waveform::ALL {
            assert_eq!(Waveform::from_wire(w.wire()), Some(w));
        }
        assert_eq!(Waveform::from_wire("ramp"), None);
    }

    #[test]
    fn config_body_carries_every_key_with_neutral_unused_values() {
        let draft = ConfigDraft {
            waveform: Waveform::Dc,
            amplitude_v: "garbage".into(),
            offset_v: " 1.5 ".into(),
            frequency_hz: "".into(),
            steps: "x".into(),
            dwell_ms: "".into(),
            ..ConfigDraft::default()
        };
        assert_eq!(
            config_body(1, &draft).unwrap(),
            json!({"channel": 1, "waveform": "dc", "amplitude_v": 0.0, "offset_v": 1.5,
                   "frequency_hz": 0.0, "steps": 2, "dwell_ms": 1000})
        );
        let sine = ConfigDraft {
            waveform: Waveform::Sine,
            ..ConfigDraft::default()
        };
        assert_eq!(
            config_body(0, &sine).unwrap(),
            json!({"channel": 0, "waveform": "sine", "amplitude_v": 1.0, "offset_v": 2.0,
                   "frequency_hz": 0.5, "steps": 2, "dwell_ms": 1000, "phase_deg": 0.0})
        );
        let stairs = ConfigDraft {
            waveform: Waveform::Staircase,
            steps: "12".into(),
            dwell_ms: "250".into(),
            ..ConfigDraft::default()
        };
        let body = config_body(0, &stairs).unwrap();
        assert_eq!(body["steps"], 12);
        assert_eq!(body["dwell_ms"], 250);
        assert_eq!(body["frequency_hz"], 0.0);
    }

    #[test]
    fn config_body_rejects_unusable_text_but_leaves_range_clamping_to_the_gs() {
        let bad = |draft: ConfigDraft| config_body(0, &draft).unwrap_err();
        assert!(
            bad(ConfigDraft {
                offset_v: "".into(),
                ..Default::default()
            })
            .contains("Offset")
        );
        assert!(
            bad(ConfigDraft {
                waveform: Waveform::Sine,
                frequency_hz: "0".into(),
                ..Default::default()
            })
            .contains("Frequency")
        );
        assert!(
            bad(ConfigDraft {
                waveform: Waveform::Square,
                amplitude_v: "-1".into(),
                ..Default::default()
            })
            .contains("negative")
        );
        assert!(
            bad(ConfigDraft {
                waveform: Waveform::Staircase,
                steps: "400".into(),
                ..Default::default()
            })
            .contains("2..256")
        );
        assert!(
            bad(ConfigDraft {
                waveform: Waveform::Staircase,
                dwell_ms: "5".into(),
                ..Default::default()
            })
            .contains("Dwell")
        );
        assert!(
            bad(ConfigDraft {
                offset_v: "NaN".into(),
                ..Default::default()
            })
            .contains("Offset")
        );
        // Out of range but numeric: sent, the GS clamps and reports it.
        let wide = ConfigDraft {
            offset_v: "9".into(),
            ..Default::default()
        };
        assert_eq!(config_body(0, &wide).unwrap()["offset_v"], 9.0);
        assert!(
            range_hint(&wide, Some([0.4, 6.2]))
                .unwrap()
                .contains("clamp")
        );
        let sine = ConfigDraft {
            waveform: Waveform::Sine,
            ..Default::default()
        };
        assert_eq!(range_hint(&sine, Some([0.4, 6.2])), None);
        let wide_sine = ConfigDraft {
            waveform: Waveform::Sine,
            amplitude_v: "2".into(),
            ..Default::default()
        };
        assert!(range_hint(&wide_sine, Some([0.4, 6.2])).is_some());
        assert_eq!(range_hint(&wide_sine, None), None);
    }

    #[test]
    fn draft_starts_from_the_kept_alive_request() {
        let s = status();
        let draft = ConfigDraft::from_channel(&s.channels[0]);
        assert_eq!(draft.waveform, Waveform::Sine);
        assert_eq!(draft.amplitude_v, "1");
        assert_eq!(draft.offset_v, "2");
        assert_eq!(draft.frequency_hz, "0.5");
        assert_eq!(
            ConfigDraft::from_channel(&DacChannel::default()),
            ConfigDraft::default()
        );
    }

    #[test]
    fn disable_validation_and_sweep_bodies() {
        assert_eq!(disable_body(Some(1)), json!({"channel": 1}));
        assert_eq!(disable_body(None), json!({"channel": "all"}));
        assert_eq!(
            validation_body(Some("mcp_ch2"), 0, "1.0", Polarity::SPlus).unwrap(),
            json!({"target": "mcp_ch2", "channel": 0, "attenuation": 1.0})
        );
        assert_eq!(
            validation_body(Some("loadcell_kg50"), 1, "0.01", Polarity::SPlus).unwrap(),
            json!({"target": "loadcell_kg50", "channel": 1, "attenuation": 0.01,
                   "load_cell_polarity": "s_plus"})
        );
        assert_eq!(
            validation_body(None, 0, "bad", Polarity::SMinus).unwrap(),
            json!({"target": null})
        );
        assert!(validation_body(Some("sar1"), 0, "0", Polarity::SMinus).is_err());
        assert_eq!(
            sweep_body("sar1", 0, "1", Polarity::SMinus, "", " ", SweepPreset::Linearity).unwrap(),
            json!({"target": "sar1", "channel": 0, "attenuation": 1.0, "preset": "linearity"})
        );
        assert_eq!(
            sweep_body("sar1", 1, "1", Polarity::SMinus, "11", "3000", SweepPreset::FrequencyResponse).unwrap(),
            json!({"target": "sar1", "channel": 1, "attenuation": 1.0, "steps": 11, "dwell_ms": 3000, "preset": "frequency_response"})
        );
        assert!(sweep_body("", 0, "1", Polarity::SMinus, "", "", SweepPreset::Linearity).is_err());
        assert!(sweep_body("sar1", 0, "1", Polarity::SMinus, "1", "", SweepPreset::Linearity).is_err());
        assert_eq!(SweepPreset::from_wire("frequency_response"), SweepPreset::FrequencyResponse);
        assert_eq!(SweepPreset::from_wire("?"), SweepPreset::Linearity);
        assert_eq!(Polarity::from_wire("s_plus"), Polarity::SPlus);
        assert_eq!(Polarity::from_wire("anything"), Polarity::SMinus);
    }

    #[test]
    fn refusal_disables_enable_but_never_disable() {
        let mut s = status();
        let reason = enable_block_reason(Some(&s), true, false).unwrap();
        assert!(reason.contains("Fill"));
        assert_eq!(disable_block_reason(true, false), None);
        s.allowed = true;
        assert_eq!(enable_block_reason(Some(&s), true, false), None);
        s.status_fresh = false;
        assert!(
            enable_block_reason(Some(&s), true, false)
                .unwrap()
                .contains("stale")
        );
        s.status_fresh = true;
        s.allowed = false;
        s.refusal = None;
        assert!(
            enable_block_reason(Some(&s), true, false)
                .unwrap()
                .contains("not allowed")
        );
        assert!(
            enable_block_reason(None, true, false)
                .unwrap()
                .contains("not loaded")
        );
        assert!(
            enable_block_reason(Some(&s), false, false)
                .unwrap()
                .contains("DaqDacControl")
        );
        assert!(
            enable_block_reason(Some(&s), true, true)
                .unwrap()
                .contains("RRLS")
        );
        assert!(disable_block_reason(false, false).is_some());
        assert!(disable_block_reason(true, true).is_some());
    }

    #[test]
    fn status_age_and_labels() {
        let mut s = status();
        assert_eq!(status_age_text(&s), "DAQ status 0.4 s old");
        s.status_fresh = false;
        s.status_age_ms = Some(65_000);
        assert_eq!(status_age_text(&s), "DAQ status STALE (65 s old)");
        s.status_age_ms = None;
        assert_eq!(status_age_text(&s), "No DAQ DAC status received");
        assert_eq!(daq_state_text(Some(1)), "Idle (1)");
        assert_eq!(daq_state_text(None), "no status");
        assert_eq!(result_text("refused_state"), "refused (flight state)");
        assert_eq!(
            disable_reason_text("lease_expired"),
            "lease expired (GS link loss)"
        );
        assert_eq!(fmt_opt(Some(f64::NAN), 2), "—");
        assert_eq!(fmt_opt(Some(1.23456), 3), "1.235");
    }

    #[test]
    fn error_bodies_are_shown_to_the_operator() {
        assert_eq!(
            operator_error(
                r#"HTTP 409: {"error":"Flight state is Fill; DAC output is allowed only in Startup or Idle."}"#
            ),
            "Flight state is Fill; DAC output is allowed only in Startup or Idle."
        );
        assert_eq!(
            operator_error(r#"Export request failed (503): {"error": "DAQ status is stale"}"#),
            "DAQ status is stale"
        );
        assert_eq!(
            operator_error(r#"HTTP 400: {"error":"bad \"x\" value and more"#),
            "bad \"x\" value and more…"
        );
        assert_eq!(
            operator_error("Settings request timed out"),
            "Settings request timed out"
        );
        assert_eq!(operator_error("HTTP 500: <html>"), "HTTP 500: <html>");
        assert_eq!(
            accepted_reply(Err("EOF while parsing a value at line 1 column 0".into())),
            Ok(Value::Null)
        );
        assert_eq!(
            accepted_reply(Err(r#"HTTP 403: {"error":"RRLS is read-only"}"#.into())),
            Err("RRLS is read-only".into())
        );
    }

    #[test]
    fn sweep_progress_and_report_parse() {
        let sweep: SweepStatus = serde_json::from_str(
            r#"{"state":"running","progress":{"step":3,"steps":11},"message":null,"report":null}"#,
        )
        .unwrap();
        assert!(sweep.running());
        assert_eq!(sweep_progress_text(&sweep), "Sweep running · step 3 of 11");
        let done: SweepStatus = serde_json::from_str(
            r#"{"state":"done","progress":{"step":11,"steps":11},"report":{"target":"mcp_ch2","channel":0,
            "attenuation":1.0,"started_utc_ms":0,"finished_utc_ms":0,
            "points":[{"dac_pin_v":0.3,"expected":0.6,"measured_mean":0.601,"measured_std":0.001,"readback_v":0.299,"samples":10}],
            "offset_error":0.001,"gain_error_pct":0.2,"max_nonlinearity_pct_fs":0.05,"noise_rms":0.001,
            "tolerances":{"offset":0.01},"checks":[{"name":"offset","value":0.001,"limit":0.01,"pass":true}],
            "pass":true,"saved_path":"recordings/x/daq_dac_sweep_1.json"}}"#,
        )
        .unwrap();
        assert_eq!(sweep_progress_text(&done), "Sweep finished");
        let report = done.report.unwrap();
        assert!(report.pass && report.checks[0].pass);
        assert_eq!(report.points[0].samples, 10);
        assert_eq!(
            sweep_progress_text(&SweepStatus::default()),
            "No sweep running"
        );
    }

    #[test]
    fn plot_breaks_at_gaps_and_spans_both_series() {
        let series = ValidationSeries {
            t_ms: vec![1000.0, 1500.0, 2000.0, 3000.0],
            measured: vec![Some(0.0), None, Some(1.0), Some(2.0)],
            expected: vec![Some(0.0), Some(1.0), Some(2.0), Some(4.0)],
        };
        let g = plot_geometry(&series, 220.0, 120.0, 10.0).unwrap();
        assert_eq!(g.y_min, 0.0);
        assert_eq!(g.y_max, 4.0);
        assert_eq!(g.span_s, 2.0);
        assert_eq!(g.measured.matches('M').count(), 2);
        assert_eq!(g.expected.matches('M').count(), 1);
        assert!(g.expected.starts_with("M10.0,110.0"));
        assert!(g.expected.ends_with("L210.0,10.0"));
        assert!(plot_geometry(&ValidationSeries::default(), 220.0, 120.0, 10.0).is_none());
        let flat = ValidationSeries {
            t_ms: vec![0.0, 1.0],
            measured: vec![Some(1.0), Some(1.0)],
            expected: vec![None, None],
        };
        let g = plot_geometry(&flat, 220.0, 120.0, 10.0).unwrap();
        assert!(g.y_min < 1.0 && g.y_max > 1.0);
    }

    #[test]
    fn new_waveforms_send_their_parameters() {
        let square = ConfigDraft { waveform: Waveform::Square, duty_pct: "25".into(), phase_deg: "90".into(), ..ConfigDraft::default() };
        let body = config_body(0, &square).unwrap();
        assert_eq!((body["duty_pct"].as_f64(), body["phase_deg"].as_f64()), (Some(25.0), Some(90.0)));
        assert!(config_body(0, &ConfigDraft { duty_pct: "0".into(), ..square.clone() }).is_err());
        let saw = ConfigDraft { waveform: Waveform::Sawtooth, ramp_down: true, ..ConfigDraft::default() };
        assert_eq!(config_body(0, &saw).unwrap()["ramp_down"], true);
        let chirp = ConfigDraft { waveform: Waveform::Chirp, frequency_hz: "10".into(), frequency_stop_hz: "1000".into(), sweep_s: "5".into(), ..ConfigDraft::default() };
        let body = config_body(1, &chirp).unwrap();
        assert_eq!((body["frequency_stop_hz"].as_f64(), body["sweep_s"].as_f64(), body["log_sweep"].as_bool()), (Some(1000.0), Some(5.0), Some(true)));
        assert!(config_body(1, &ConfigDraft { sweep_s: "0".into(), ..chirp }).is_err());
        let noise = ConfigDraft { waveform: Waveform::Noise, frequency_hz: "50000".into(), ..ConfigDraft::default() };
        assert_eq!(config_body(0, &noise).unwrap()["frequency_hz"], 50000.0);
        assert_eq!(Waveform::Noise.frequency_max(), 200_000.0);
        assert_eq!(Waveform::Noise.frequency_label(), "Sample rate (S/s)");
        let stairs = ConfigDraft { waveform: Waveform::Staircase, steps: "256".into(), ..ConfigDraft::default() };
        assert_eq!(config_body(0, &stairs).unwrap()["steps"], 256);
        assert!(config_body(0, &ConfigDraft { steps: "257".into(), ..stairs }).is_err());
        for w in Waveform::ALL {
            assert_eq!(Waveform::from_wire(w.wire()), Some(w));
        }
    }

    #[test]
    fn custom_shapes_use_the_same_controls() {
        let draft = ConfigDraft {
            waveform: Waveform::Arbitrary,
            amplitude_v: "2".into(),
            offset_v: "3.2".into(),
            frequency_hz: "50".into(),
            phase_deg: "45".into(),
            ..ConfigDraft::default()
        };
        let body = config_body(0, &draft).unwrap();
        assert_eq!(body["waveform"], "arbitrary");
        assert_eq!((body["amplitude_v"].as_f64(), body["offset_v"].as_f64(), body["frequency_hz"].as_f64(), body["phase_deg"].as_f64()), (Some(2.0), Some(3.2), Some(50.0), Some(45.0)));
        assert_eq!(body["custom"]["kind"], "expression");
        assert!(body.get("steps").is_none() && body.get("duty_pct").is_none());
        let mut blank = draft.clone();
        blank.custom.expression = "  ".into();
        assert!(config_body(0, &blank).is_err());
        blank.custom.expression = "x".repeat(513);
        assert!(config_body(0, &blank).is_err());
        let mut sized = draft.clone();
        sized.custom.samples = "128".into();
        assert_eq!(config_body(0, &sized).unwrap()["custom"]["samples"], 128);
        sized.custom.samples = "1024".into();
        assert!(config_body(0, &sized).is_err());
        // WAV: blank frequency means "from the playback rate".
        let mut wav = draft.clone();
        wav.custom.kind = CustomKind::Wav;
        assert!(config_body(0, &wav).unwrap_err().contains("Open a WAV"));
        wav.custom.wav_base64 = "UklGRg==".into();
        wav.frequency_hz = "".into();
        wav.custom.wav_rate = "8000".into();
        let body = config_body(0, &wav).unwrap();
        assert!(body.get("frequency_hz").is_none());
        assert_eq!(body["custom"], json!({"kind": "wav", "data_base64": "UklGRg==", "normalize": true, "playback_rate_hz": 8000.0}));
        wav.custom.wav_rate = "300000".into();
        assert!(config_body(0, &wav).is_err());
        // A kept-alive custom request reloads into the draft (no WAV data is kept).
        let channel: DacChannel = serde_json::from_value(json!({
            "channel": 0, "active": true, "waveform": "arbitrary",
            "desired": {"channel": 0, "waveform": "arbitrary", "amplitude_v": 1.0, "offset_v": 3.0, "frequency_hz": 5.0,
                        "phase_deg": 30.0, "custom": {"kind": "expression", "expression": "sin(2*pi*x)"}},
            "samples": 512, "update_hz": 2560.0, "table_crc": "A7B93781", "table_crc_ok": true
        })).unwrap();
        let d = ConfigDraft::from_channel(&channel);
        assert_eq!((d.waveform, d.custom.expression.as_str(), d.phase_deg.as_str()), (Waveform::Arbitrary, "sin(2*pi*x)", "30"));
        assert_eq!((channel.samples, channel.table_crc_ok), (512, Some(true)));
    }
}
