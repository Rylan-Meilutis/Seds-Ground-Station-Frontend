//! Pure helpers for the DAC waveform preview: the GS preview reply, its plot,
//! the achieved-frequency readout, the in-browser audio preview (Web Audio)
//! and WAV file import. The GS evaluates equations and samples tables; this
//! module only shows and plays what the GS returns.
use base64::Engine;
use serde::Deserialize;

use super::daq_dac_model::Waveform;

/// `POST /api/daq/dac/preview` reply: exactly what the DAQ will play.
#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
pub(super) struct Preview {
    #[serde(default)]
    pub waveform: String,
    #[serde(default)]
    pub samples: u32,
    #[serde(default)]
    pub update_hz: f64,
    #[serde(default)]
    pub requested_frequency_hz: Option<f64>,
    #[serde(default)]
    pub frequency_hz: f64,
    #[serde(default)]
    pub frequency_error_ppm: Option<f64>,
    #[serde(default)]
    pub dwell_ms: u32,
    #[serde(default)]
    pub table_crc: String,
    #[serde(default)]
    pub shape_crc: Option<String>,
    #[serde(default)]
    pub upload_needed: Option<bool>,
    /// One period (or the staircase/noise table) in P10 connector volts.
    #[serde(default)]
    pub connector_v: Vec<f64>,
    #[serde(default)]
    pub shape: Option<Vec<f64>>,
    #[serde(default)]
    pub duty_achieved_pct: Option<f64>,
    #[serde(default)]
    pub clamped: Vec<String>,
    #[serde(default)]
    pub warnings: Vec<String>,
}

fn fmt_hz(hz: f64) -> String {
    if hz >= 1000.0 {
        format!("{:.6} kHz", hz / 1000.0)
    } else {
        format!("{hz:.6} Hz")
    }
    .replace(".000000 ", " ")
}

/// "Achieved 123.401184 Hz (+9.6 ppm) · 236 samples/period at 29.1227 kS/s".
pub(super) fn achieved_text(p: &Preview) -> String {
    let wave = Waveform::from_wire(&p.waveform);
    let rate = if p.update_hz > 0.0 {
        format!(" at {:.4} kS/s", p.update_hz / 1000.0)
    } else {
        String::new()
    };
    match wave {
        Some(Waveform::Dc) => "DC: one sample, no timer.".into(),
        Some(Waveform::Staircase) => format!(
            "{} steps × {} ms (achieved dwell){rate}",
            p.samples, p.dwell_ms
        ),
        Some(Waveform::Noise) => format!(
            "Noise at {} (256-sample repeating sequence)",
            fmt_hz(p.frequency_hz).replace("Hz", "S/s")
        ),
        _ => {
            let ppm = p
                .frequency_error_ppm
                .map(|e| format!(" ({e:+.1} ppm)"))
                .unwrap_or_default();
            format!(
                "Achieved {}{ppm} · {} samples/period{rate}",
                fmt_hz(p.frequency_hz),
                p.samples
            )
        }
    }
}

/// SVG geometry for two periods of the table as the DAC holds it, plus the
/// reachable range's lines.
#[derive(Clone, Debug, PartialEq)]
pub(super) struct PreviewPlot {
    pub path: String,
    pub lo_y: f64,
    pub hi_y: f64,
    pub y_min: f64,
    pub y_max: f64,
}

pub(super) fn preview_plot(
    table: &[f64],
    range: [f64; 2],
    width: f64,
    height: f64,
    pad: f64,
) -> Option<PreviewPlot> {
    if table.is_empty() || table.iter().any(|v| !v.is_finite()) {
        return None;
    }
    let y_min = range[0].min(table.iter().copied().fold(f64::INFINITY, f64::min)) - 0.1;
    let y_max = range[1].max(table.iter().copied().fold(f64::NEG_INFINITY, f64::max)) + 0.1;
    let y = |v: f64| height - pad - (v - y_min) / (y_max - y_min) * (height - 2.0 * pad);
    let n = table.len() * 2;
    let x = |i: usize| pad + i as f64 / n as f64 * (width - 2.0 * pad);
    let mut path = String::new();
    for i in 0..n {
        let v = y(table[i % table.len()]);
        if i == 0 {
            path.push_str(&format!("M{:.1},{v:.1} ", x(0)));
        } else {
            path.push_str(&format!("V{v:.1} "));
        }
        path.push_str(&format!("H{:.1} ", x(i + 1)));
    }
    Some(PreviewPlot {
        path: path.trim_end().to_string(),
        lo_y: y(range[0]),
        hi_y: y(range[1]),
        y_min,
        y_max,
    })
}

/// Audio rendering of a preview for the operator to hear.
#[derive(Clone, Debug, PartialEq)]
pub(super) struct AudioPreview {
    pub samples: Vec<f32>,
    pub sample_rate: u32,
    /// The frequency actually heard.
    pub heard_hz: f64,
    /// Octaves shifted to reach the audible band (0 when played as is).
    pub octaves: i32,
}

pub(super) const AUDIO_RATE: u32 = 22_050;

/// The table as the DAC holds it (zero-order hold, no DC, peak 0.25 full
/// scale) at its frequency, shifted by whole octaves into 40 Hz..8 kHz when
/// it is outside. Noise plays at its sample rate; a chirp sweeps its range
/// across the clip.
pub(super) fn audio_preview(
    p: &Preview,
    chirp_stop_hz: Option<f64>,
    seconds: f64,
) -> Option<AudioPreview> {
    let n = p.connector_v.len();
    if n == 0 || p.frequency_hz <= 0.0 || p.waveform == "dc" {
        return None;
    }
    let mean = p.connector_v.iter().sum::<f64>() / n as f64;
    let peak = p
        .connector_v
        .iter()
        .map(|v| (v - mean).abs())
        .fold(0.0, f64::max);
    // A period rate (Hz); noise reports its sample rate instead.
    let base = if p.waveform == "noise" {
        p.frequency_hz / n as f64
    } else {
        p.frequency_hz
    };
    let mut octaves = 0;
    let mut heard = base;
    while heard < 40.0 && octaves < 20 {
        heard *= 2.0;
        octaves += 1;
    }
    while heard > 8000.0 && octaves > -20 {
        heard /= 2.0;
        octaves -= 1;
    }
    let scale = 2f64.powi(octaves);
    let stop = chirp_stop_hz.map(|s| s * scale);
    let total = (seconds * f64::from(AUDIO_RATE)).round() as usize;
    let gain = if peak > 0.0 { 0.25 / peak } else { 0.0 };
    let mut phase = 0.0f64;
    let samples = (0..total)
        .map(|k| {
            let f = match stop {
                Some(stop) => heard + (stop - heard) * k as f64 / total as f64,
                None => heard,
            };
            let idx = ((phase * n as f64) as usize).min(n - 1);
            phase = (phase + f / f64::from(AUDIO_RATE)).fract();
            ((p.connector_v[idx] - mean) * gain) as f32
        })
        .collect();
    Some(AudioPreview {
        samples,
        sample_rate: AUDIO_RATE,
        heard_hz: heard,
        octaves,
    })
}

/// "Playing at 440 Hz (3 octaves up from 55 Hz)".
pub(super) fn audio_note(a: &AudioPreview) -> String {
    match a.octaves {
        0 => format!("Playing at {:.1} Hz, as the DAC will.", a.heard_hz),
        o => format!(
            "Playing at {:.1} Hz: {} octave{} {} from {:.3} Hz to be audible.",
            a.heard_hz,
            o.abs(),
            if o.abs() == 1 { "" } else { "s" },
            if o > 0 { "up" } else { "down" },
            a.heard_hz / 2f64.powi(o)
        ),
    }
}

/// JavaScript that plays the preview through Web Audio (replacing any
/// preview already playing) and stops on its own.
pub(super) fn play_script(a: &AudioPreview) -> String {
    let data: Vec<String> = a.samples.iter().map(|v| format!("{v:.4}")).collect();
    format!(
        "(function(){{const w=window;try{{w.__gs26DacAudio?.stop();}}catch(e){{}}\
         const C=w.AudioContext||w.webkitAudioContext;if(!C)return 'Web Audio is unavailable in this browser.';\
         const ctx=w.__gs26DacCtx||(w.__gs26DacCtx=new C());const d=[{}];\
         const b=ctx.createBuffer(1,d.length,{});b.getChannelData(0).set(d);\
         const s=ctx.createBufferSource();s.buffer=b;s.connect(ctx.destination);ctx.resume();s.start();\
         w.__gs26DacAudio=s;return 'ok';}})()",
        data.join(","),
        a.sample_rate
    )
}

pub(super) const STOP_AUDIO_SCRIPT: &str =
    "try{window.__gs26DacAudio?.stop();}catch(e){}delete window.__gs26DacAudio;";

/// Bytes of an opened WAV file, base64 for the GS (which parses and
/// resamples it). Checks the size limit and the RIFF/WAVE header.
pub(super) fn wav_to_base64(bytes: &[u8], max_bytes: usize) -> Result<String, String> {
    if bytes.len() > max_bytes {
        return Err(format!(
            "The file is {:.1} MiB; WAV import takes at most {:.1} MiB (a short clip: the DAC holds 512 samples).",
            bytes.len() as f64 / 1_048_576.0,
            max_bytes as f64 / 1_048_576.0
        ));
    }
    if bytes.len() < 12 || &bytes[0..4] != b"RIFF" || &bytes[8..12] != b"WAVE" {
        return Err("Not a WAV file (no RIFF/WAVE header). Convert other audio formats to WAV first.".into());
    }
    Ok(base64::engine::general_purpose::STANDARD.encode(bytes))
}

pub(super) const WAV_MAX_BYTES: usize = 1024 * 1024;

#[cfg(test)]
mod tests {
    use super::*;

    fn preview(json: &str) -> Preview {
        serde_json::from_str(json).unwrap()
    }

    #[test]
    fn preview_reply_and_readout() {
        let p = preview(
            r#"{"waveform":"sine","samples":236,"update_hz":29122.68,"requested_frequency_hz":123.4,
               "frequency_hz":123.401184,"frequency_error_ppm":9.6,"dwell_ms":0,"table_crc":"AF1D0167",
               "connector_v":[3.2,5.2,3.2,1.2],"clamped":[],"warnings":[]}"#,
        );
        assert_eq!(
            achieved_text(&p),
            "Achieved 123.401184 Hz (+9.6 ppm) · 236 samples/period at 29.1227 kS/s"
        );
        let stair = Preview {
            waveform: "staircase".into(),
            samples: 11,
            dwell_ms: 500,
            update_hz: 2.0,
            ..Preview::default()
        };
        assert_eq!(achieved_text(&stair), "11 steps × 500 ms (achieved dwell) at 0.0020 kS/s");
        let noise = Preview { waveform: "noise".into(), frequency_hz: 1000.0, ..Preview::default() };
        assert!(achieved_text(&noise).contains("1 kS/s"));
        let custom = preview(r#"{"waveform":"arbitrary","shape_crc":"79119F9E","upload_needed":false,"shape":[0.0,1.0]}"#);
        assert_eq!((custom.upload_needed, custom.shape.unwrap().len()), (Some(false), 2));
    }

    #[test]
    fn plot_draws_two_held_periods_with_range_lines() {
        let plot = preview_plot(&[1.0, 2.0], [0.4, 6.2], 100.0, 50.0, 0.0).unwrap();
        assert_eq!(plot.path.matches('H').count(), 4);
        assert_eq!(plot.path.matches('V').count(), 3);
        assert!(plot.path.starts_with("M0.0,"));
        assert!(plot.lo_y > plot.hi_y);
        assert!((plot.y_min - 0.3).abs() < 1e-9 && (plot.y_max - 6.3).abs() < 1e-9);
        assert!(preview_plot(&[], [0.4, 6.2], 100.0, 50.0, 0.0).is_none());
        assert!(preview_plot(&[f64::NAN], [0.4, 6.2], 100.0, 50.0, 0.0).is_none());
    }

    #[test]
    fn audio_preview_holds_samples_and_shifts_octaves() {
        let p = Preview {
            waveform: "square".into(),
            frequency_hz: 441.0,
            connector_v: vec![5.0, 5.0, 1.0, 1.0],
            ..Preview::default()
        };
        let a = audio_preview(&p, None, 0.1).unwrap();
        assert_eq!((a.samples.len(), a.octaves, a.heard_hz), (2205, 0, 441.0));
        // DC removed, peak 0.25, 50 samples per period: 25 high then 25 low.
        assert!(a.samples[..25].iter().all(|v| (*v - 0.25).abs() < 1e-6));
        assert!(a.samples[26..50].iter().all(|v| (*v + 0.25).abs() < 1e-6));
        // 5 Hz is shifted up 3 octaves to 40 Hz; 10 kHz down one.
        let slow = audio_preview(&Preview { frequency_hz: 5.0, ..p.clone() }, None, 0.1).unwrap();
        assert_eq!((slow.octaves, slow.heard_hz), (3, 40.0));
        assert!(audio_note(&slow).contains("3 octaves up from 5.000 Hz"));
        let fast = audio_preview(&Preview { frequency_hz: 10_000.0, ..p.clone() }, None, 0.1).unwrap();
        assert_eq!((fast.octaves, fast.heard_hz), (-1, 5000.0));
        assert_eq!(audio_note(&a), "Playing at 441.0 Hz, as the DAC will.");
        // Noise reports a sample rate: the period is the 256-sample sequence.
        let noise = Preview { waveform: "noise".into(), frequency_hz: 256_000.0, connector_v: vec![1.0; 256], ..Preview::default() };
        assert_eq!(audio_preview(&noise, None, 0.1).unwrap().heard_hz, 1000.0);
        assert!(audio_preview(&Preview { waveform: "dc".into(), ..p.clone() }, None, 0.1).is_none());
        // Chirp sweeps the clip.
        let chirp = audio_preview(&Preview { waveform: "chirp".into(), frequency_hz: 100.0, ..p.clone() }, Some(1000.0), 0.5);
        assert_eq!(chirp.unwrap().samples.len(), 11025);
        let script = play_script(&a);
        assert!(script.contains("createBuffer(1,d.length,22050)") && script.contains("0.2500"));
        assert!(!script.contains("eval(") && !script.contains("fetch("));
    }

    #[test]
    fn wav_import_checks_header_and_size() {
        let mut wav = b"RIFF\x24\x00\x00\x00WAVEfmt ".to_vec();
        wav.extend([0u8; 20]);
        let b64 = wav_to_base64(&wav, WAV_MAX_BYTES).unwrap();
        assert_eq!(base64::engine::general_purpose::STANDARD.decode(b64).unwrap(), wav);
        assert!(wav_to_base64(b"ID3\x03 mp3 data here", WAV_MAX_BYTES).unwrap_err().contains("Not a WAV"));
        assert!(wav_to_base64(&vec![0u8; WAV_MAX_BYTES + 1], WAV_MAX_BYTES).unwrap_err().contains("at most 1.0 MiB"));
    }
}
