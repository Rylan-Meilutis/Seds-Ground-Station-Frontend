//! The station's vehicle geometry (`GET /api/vehicle/geometry`, see
//! `docs/vehicle-geometry.md`). The station generates it from its OpenRocket
//! design; the dashboard draws whatever it is sent, so a different vehicle
//! needs no UI change.
//!
//! The last good reply is cached per station, for offline use and replays.
//! Without one (or with a version this build does not know) the dashboard
//! draws a neutral generic rocket.
use dioxus::prelude::*;
use serde::{Deserialize, Serialize};

pub(crate) const PATH: &str = "/api/vehicle/geometry";
pub(crate) const SCHEMA: &str = "gs26.vehicle_geometry";
pub(crate) const VERSION: u32 = 1;
const CACHE_KEY_PREFIX: &str = "gs_vehicle_geometry_v1_";
const MAX_BYTES: usize = 512 * 1024;

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub(crate) struct Source {
    pub kind: String,
    pub file: String,
    pub hash: String,
    pub rocket: String,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub(crate) struct Section {
    pub name: String,
    pub role: String,
    pub x0_m: f64,
    pub x1_m: f64,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub(crate) struct FinSet {
    pub count: u32,
    pub x_m: f64,
    pub root_radius_m: f64,
    pub span_m: f64,
    pub angle_offset_deg: f64,
    pub planform: Vec<[f64; 2]>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub(crate) struct Separation {
    pub x_m: f64,
}

/// The fields the 2D drawings use; the 3D renderer reads the full reply.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub(crate) struct Geometry {
    pub schema: String,
    pub version: u32,
    #[serde(default)]
    pub source: Source,
    pub length_m: f64,
    pub diameter_m: f64,
    #[serde(default)]
    pub span_m: f64,
    #[serde(default)]
    pub cg_m: Option<f64>,
    #[serde(default)]
    pub cp_m: Option<f64>,
    pub profile: Vec<[f64; 2]>,
    #[serde(default)]
    pub sections: Vec<Section>,
    #[serde(default)]
    pub fins: Vec<FinSet>,
    #[serde(default)]
    pub separations: Vec<Separation>,
}

fn finite(v: f64) -> bool {
    v.is_finite()
}

/// Parses and checks a reply; refuses unknown schemas/versions and nonsense.
pub(crate) fn parse(text: &str) -> Result<Geometry, String> {
    if text.len() > MAX_BYTES {
        return Err("The vehicle geometry is too large.".into());
    }
    let g: Geometry = serde_json::from_str(text).map_err(|e| format!("The vehicle geometry is not readable: {e}"))?;
    if g.schema != SCHEMA {
        return Err(format!("Unknown vehicle geometry schema `{}`.", g.schema));
    }
    if g.version != VERSION {
        return Err(format!("Vehicle geometry version {} is not supported (this dashboard reads version {VERSION}).", g.version));
    }
    if !(finite(g.length_m) && (0.1..=200.0).contains(&g.length_m) && finite(g.diameter_m) && g.diameter_m > 0.0 && g.diameter_m < g.length_m) {
        return Err("The vehicle geometry has an impossible length or diameter.".into());
    }
    if g.profile.len() < 2 || g.profile.len() > 4096 {
        return Err("The vehicle geometry has no usable profile.".into());
    }
    let mut last = f64::NEG_INFINITY;
    for [x, r] in &g.profile {
        if !(finite(*x) && finite(*r) && *r >= 0.0 && *x >= last - 1e-9 && *x <= g.length_m + 1e-3 && *r <= g.length_m) {
            return Err("The vehicle geometry profile is not a radius profile along the axis.".into());
        }
        last = *x;
    }
    for f in &g.fins {
        if f.count > 16 || f.planform.len() > 64 || !f.planform.iter().flatten().chain([&f.x_m, &f.root_radius_m]).all(|v| finite(*v)) {
            return Err("The vehicle geometry has an unusable fin set.".into());
        }
    }
    Ok(g)
}

/// A neutral generic rocket (3 m, 150 mm, ogive nose, four fins): drawn
/// until the station sends its own vehicle.
pub(crate) fn generic() -> Geometry {
    let (len, r, nose) = (3.0, 0.075, 0.6);
    let mut profile: Vec<[f64; 2]> = (0..=16)
        .map(|i| {
            let x = nose * i as f64 / 16.0;
            // Tangent ogive.
            let rho = (r * r + nose * nose) / (2.0 * r);
            [x, ((rho * rho - (nose - x).powi(2)).max(0.0).sqrt() + r - rho).max(0.0)]
        })
        .collect();
    profile.push([len, r]);
    Geometry {
        schema: SCHEMA.into(),
        version: VERSION,
        source: Source { kind: "generic".into(), ..Default::default() },
        length_m: len,
        diameter_m: 2.0 * r,
        span_m: 2.0 * (r + 0.15),
        cg_m: None,
        cp_m: None,
        profile,
        sections: vec![
            Section { name: "Nose".into(), role: "nose".into(), x0_m: 0.0, x1_m: nose },
            Section { name: "Body".into(), role: "airframe".into(), x0_m: nose, x1_m: len },
        ],
        fins: vec![FinSet {
            count: 4,
            x_m: len - 0.32,
            root_radius_m: r,
            span_m: 0.15,
            angle_offset_deg: 0.0,
            planform: vec![[0.0, 0.0], [0.15, 0.15], [0.27, 0.15], [0.3, 0.0]],
        }],
        separations: vec![],
    }
}

/// The station's geometry: the validated reply text and its parse.
static STATION: GlobalSignal<Option<(String, Geometry)>> = Signal::global(|| None);
static LOADED_FOR: GlobalSignal<Option<String>> = Signal::global(|| None);

fn cache_key(base: &str) -> String {
    let mut key = String::from(CACHE_KEY_PREFIX);
    let base = base.trim().trim_end_matches('/').to_ascii_lowercase();
    if base.is_empty() {
        key.push_str("default");
    }
    key.extend(base.chars().map(|c| if c.is_ascii_alphanumeric() { c } else { '_' }));
    key
}

/// The geometry to draw: the station's, else the generic rocket.
pub(crate) fn current() -> Geometry {
    STATION.read().as_ref().map(|(_, g)| g.clone()).unwrap_or_else(generic)
}

/// The station's reply as JSON for the 3D renderer (`None`: draw the generic rocket).
pub(crate) fn station_json() -> Option<String> {
    STATION.read().as_ref().map(|(text, _)| text.clone())
}

/// As `station_json`, without subscribing (async tasks).
pub(crate) fn station_json_now() -> Option<String> {
    STATION.peek().as_ref().map(|(text, _)| text.clone())
}

fn adopt(text: String) -> bool {
    match parse(&text) {
        Ok(g) => {
            if STATION.peek().as_ref().map(|(t, _)| t) != Some(&text) {
                *STATION.write() = Some((text, g));
            }
            true
        }
        Err(_) => false,
    }
}

/// Loads the cached geometry for the current station once, then asks the
/// station for its current one (keeping the cache if it cannot answer).
pub(crate) fn use_station_geometry() {
    use_future(move || async move {
        let base = super::UrlConfig::base_http();
        if LOADED_FOR.peek().as_deref() == Some(base.as_str()) {
            return;
        }
        *LOADED_FOR.write() = Some(base.clone());
        let key = cache_key(&base);
        match super::persist::get_string(&key) {
            Some(text) => {
                adopt(text);
            }
            None => *STATION.write() = None,
        }
        if let Ok(value) = super::http_get_json::<serde_json::Value>(PATH).await
            && super::UrlConfig::base_http() == base
        {
            let text = value.to_string();
            if adopt(text.clone()) {
                super::persist::set_string(&key, &text);
            }
        }
    });
}

/// SVG paths of the side profile laid out in a box (pixels), true to scale.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Silhouette {
    pub body: String,
    pub fins: String,
    /// `(role, path)` for each section with a role other than `airframe`.
    pub sections: Vec<(String, String, String)>,
    pub separations: Vec<[f64; 4]>,
    /// Pixels per metre, and the drawn length and width.
    pub scale: f64,
    pub length_px: f64,
    pub width_px: f64,
}

fn fmt(v: f64) -> String {
    format!("{:.1}", v)
}

/// The profile clipped to `[x0, x1]`, as `[x, r]` points.
fn clip(profile: &[[f64; 2]], x0: f64, x1: f64) -> Vec<[f64; 2]> {
    let at = |x: f64| -> f64 {
        for w in profile.windows(2) {
            let ([a, ra], [b, rb]) = (w[0], w[1]);
            if x >= a && x <= b {
                return if b - a > 1e-12 { ra + (rb - ra) * (x - a) / (b - a) } else { ra.max(rb) };
            }
        }
        profile.last().map_or(0.0, |p| p[1])
    };
    let mut out = vec![[x0, at(x0)]];
    out.extend(profile.iter().filter(|p| p[0] > x0 && p[0] < x1).copied());
    out.push([x1, at(x1)]);
    out
}

/// Lays the vehicle out vertically (nose up) in `(x, y, w, h)`, centred.
pub(crate) fn vertical(g: &Geometry, bx: f64, by: f64, bw: f64, bh: f64) -> Silhouette {
    let span = g.span_m.max(g.diameter_m);
    let s = (bh / g.length_m).min(bw / span).max(0.0);
    let cx = bx + bw / 2.0;
    let top = by + (bh - g.length_m * s) / 2.0;
    let pt = |x: f64, r: f64| format!("{} {}", fmt(cx + r * s), fmt(top + x * s));
    let outline = |pts: &[[f64; 2]]| -> String {
        let mut d = String::new();
        for (i, p) in pts.iter().enumerate() {
            d.push_str(if i == 0 { "M" } else { "L" });
            d.push_str(&pt(p[0], p[1]));
        }
        for p in pts.iter().rev() {
            d.push('L');
            d.push_str(&pt(p[0], -p[1]));
        }
        d.push('Z');
        d
    };
    let body = outline(&g.profile);
    let mut fins = String::new();
    for f in &g.fins {
        for k in 0..f.count {
            let a = f.angle_offset_deg.to_radians() + std::f64::consts::TAU * k as f64 / f.count.max(1) as f64;
            let side = a.cos();
            if side.abs() < 1e-3 {
                continue;
            }
            for (i, p) in f.planform.iter().enumerate() {
                fins.push(if i == 0 { 'M' } else { 'L' });
                fins.push_str(&pt(f.x_m + p[0], (f.root_radius_m + p[1]) * side));
            }
            fins.push('Z');
        }
    }
    let sections = g
        .sections
        .iter()
        .filter(|s| s.role != "airframe" && s.x1_m > s.x0_m)
        .map(|s| (s.role.clone(), s.name.clone(), outline(&clip(&g.profile, s.x0_m, s.x1_m))))
        .collect();
    let r_at = |x: f64| clip(&g.profile, x, x)[0][1];
    let separations = g
        .separations
        .iter()
        .map(|p| {
            let r = r_at(p.x_m) * s + 4.0;
            let y = top + p.x_m * s;
            [cx - r, y, cx + r, y]
        })
        .collect();
    Silhouette { body, fins, sections, separations, scale: s, length_px: g.length_m * s, width_px: span * s }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// The station's reply for its built-in design (SWEET_V6.ork), from the
    /// backend contract fixtures.
    pub(crate) fn station_example() -> String {
        crate::backend_contract::fixture("vehicle/geometry").to_string()
    }

    #[test]
    fn the_station_example_parses_true_to_the_design() {
        let g = parse(&station_example()).unwrap();
        assert_eq!(g.source.rocket, "SWEET_V6");
        assert!((g.length_m - 4.387).abs() < 0.01, "{}", g.length_m);
        assert!((g.diameter_m - 0.1565).abs() < 0.001);
        assert_eq!(g.fins[0].count, 3);
        assert!((g.fins[0].span_m - 0.1746).abs() < 0.001);
        assert!(g.sections.iter().any(|s| s.role == "avionics"));
        // About 28:1 (15 ft by 6 in): the drawing keeps it.
        let s = vertical(&g, 0.0, 0.0, 100.0, 560.0);
        assert!((s.length_px - 560.0).abs() < 0.5);
        let body_px = g.diameter_m * s.scale;
        assert!((s.length_px / body_px - g.length_m / g.diameter_m).abs() < 0.01);
        assert!(s.length_px / body_px > 27.0 && s.length_px / body_px < 29.0);
    }

    #[test]
    fn unknown_versions_and_nonsense_are_refused() {
        let mut v: serde_json::Value = serde_json::from_str(&station_example()).unwrap();
        v["version"] = 2.into();
        assert!(parse(&v.to_string()).unwrap_err().contains("version 2"));
        v["version"] = 1.into();
        v["schema"] = "other".into();
        assert!(parse(&v.to_string()).is_err());
        v["schema"] = SCHEMA.into();
        v["profile"] = serde_json::json!([[0.0, 0.0], [-1.0, 0.1]]);
        assert!(parse(&v.to_string()).is_err());
        assert!(parse("{}").is_err());
        assert!(parse("not json").is_err());
    }

    #[test]
    fn the_generic_rocket_is_valid_and_drawable() {
        let g = generic();
        let text = serde_json::to_string(&g).unwrap();
        assert_eq!(parse(&text).unwrap(), g);
        let s = vertical(&g, 10.0, 10.0, 60.0, 300.0);
        assert!(s.body.starts_with('M') && s.fins.matches('Z').count() == 2, "two of four fins face sideways");
    }

    #[test]
    fn cache_keys_are_per_station() {
        assert_ne!(cache_key("https://a.local"), cache_key("https://b.local"));
        assert_eq!(cache_key(""), "gs_vehicle_geometry_v1_default");
    }
}
