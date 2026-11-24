//! Current-flight maxima as a station computes them from the rows it
//! receives (GroundStation26 `flight_maxima.rs`), for the local station's
//! `FlightMaxima` frames and `GET /api/flight_maxima`.
//!
//! The same rules as the station: tracked only while the T clock counts up,
//! one extreme per source (FC Kalman over barometer over GPS for altitude,
//! FC Kalman velocity over the GS vehicle speed for speed, raw accelerometer
//! magnitude for acceleration), each with its T+ time. A new T-0 starts a new
//! flight. The FC's own text reports ("EV apogee guess") arrive as messages,
//! which a local session does not carry, so `fc_*` stay empty.
use serde::Serialize;

/// GroundStation26 `telemetry_schema::baro::ALTITUDE_M`.
const BARO_ALTITUDE_M: usize = 0;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Quantity {
    Altitude,
    Speed,
    Acceleration,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub(crate) struct Extreme {
    pub value: f64,
    pub source: String,
    /// Lower is preferred when several sources report the same quantity.
    pub precedence: u8,
    pub t_plus_ms: Option<i64>,
    pub timestamp_ms: i64,
}

/// The station's `FlightMaxima` (field for field, the same JSON).
#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub(crate) struct FlightMaxima {
    pub flight_anchor_ms: Option<i64>,
    pub tracking: bool,
    pub altitude: Vec<Extreme>,
    pub speed: Vec<Extreme>,
    pub acceleration: Vec<Extreme>,
    pub fc_altitude: Option<Extreme>,
    pub fc_speed: Option<Extreme>,
    pub fc_acceleration: Option<Extreme>,
}

/// (quantity, value, source, precedence) samples one row carries
/// (GroundStation26 `flight_maxima::samples`).
pub(crate) fn samples(data_type: &str, values: &[Option<f32>]) -> Vec<(Quantity, f64, &'static str, u8)> {
    let v = |i: usize| values.get(i).copied().flatten().map(f64::from).filter(|v| v.is_finite());
    let mut out = Vec::new();
    match data_type {
        "ASCENT_STATE" => {
            if let Some(alt) = v(4) {
                out.push((Quantity::Altitude, alt, "FC Kalman altitude", 0));
            }
            if let Some(vel) = v(5) {
                out.push((Quantity::Speed, vel.abs(), "FC Kalman velocity", 0));
            }
        }
        "DESCENT_STATE" => {
            if let Some(alt) = v(2) {
                out.push((Quantity::Altitude, alt, "FC Kalman altitude", 0));
            }
            if let Some(vel) = v(3) {
                out.push((Quantity::Speed, vel.abs(), "FC Kalman velocity", 0));
            }
        }
        "BAROMETER_DATA" => {
            if let Some(alt) = v(BARO_ALTITUDE_M) {
                out.push((Quantity::Altitude, alt, "Barometer altitude", 1));
            }
        }
        "GPS_DATA" => {
            if let Some(alt) = v(2) {
                out.push((Quantity::Altitude, alt, "GPS altitude", 2));
            }
        }
        "VEHICLE_SPEED" => {
            if let Some(speed) = v(0) {
                out.push((Quantity::Speed, speed.abs(), "GS vehicle speed (baro/GPS)", 1));
            }
        }
        "ACCEL_DATA" => {
            if let (Some(x), Some(y), Some(z)) = (v(0), v(1), v(2)) {
                out.push((Quantity::Acceleration, (x * x + y * y + z * z).sqrt(), "Raw accelerometer", 0));
            }
        }
        _ => {}
    }
    out
}

impl FlightMaxima {
    /// A new flight anchored at `anchor_ms` (wall clock of T-0).
    pub(crate) fn flight(anchor_ms: i64) -> Self {
        Self { flight_anchor_ms: Some(anchor_ms), tracking: true, ..Self::default() }
    }

    /// Folds one row in (GroundStation26 `FlightMaxima::observe` per sample).
    pub(crate) fn observe_row(&mut self, data_type: &str, values: &[Option<f32>], t_plus_ms: i64, timestamp_ms: i64) {
        for (quantity, value, source, precedence) in samples(data_type, values) {
            let list = match quantity {
                Quantity::Altitude => &mut self.altitude,
                Quantity::Speed => &mut self.speed,
                Quantity::Acceleration => &mut self.acceleration,
            };
            match list.iter_mut().find(|e| e.source == source) {
                Some(existing) if existing.value >= value => {}
                Some(existing) => {
                    existing.value = value;
                    existing.t_plus_ms = Some(t_plus_ms);
                    existing.timestamp_ms = timestamp_ms;
                }
                None => {
                    list.push(Extreme { value, source: source.into(), precedence, t_plus_ms: Some(t_plus_ms), timestamp_ms });
                    list.sort_by_key(|e| e.precedence);
                }
            }
        }
    }
}
