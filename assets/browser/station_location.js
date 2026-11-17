// One explicit request for THIS device's location, for the station location form.
// Nothing is saved and no watch is left running.
// The eval wrapper closes its channel when this script finishes, so the script
// awaits the answer; otherwise a slow permission prompt could lose the result.
//
// Order:
// 1. A recent fix the dashboard's own location watch already has (the map's
//    "you are here" marker). A stationary device may not produce a brand-new
//    fix quickly, so asking for one (maximumAge 0) used to time out even
//    though the map was showing this device's position.
// 2. getCurrentPosition, accepting a recent cached fix.
// 3. On timeout or a missing high-accuracy source, one coarse retry
//    (Wi-Fi/network location).
// Failures are reported as a code; the Rust side owns the wording.
await new Promise(done => {
  const MAX_AGE_MS = 120000;
  let sent = false;
  const send = value => { if (!sent) { sent = true; dioxus.send(value); done(); } };
  const fail = failure => send({failure});
  const coordinate = (c, at) => ({
    coordinate: {latitude: c.latitude, longitude: c.longitude,
      altitude_m: Number.isFinite(c.altitude) ? c.altitude : null,
      accuracy_m: Number.isFinite(c.accuracy) && c.accuracy >= 0 ? c.accuracy : null,
      source: 'device_location'},
    age_ms: Number.isFinite(at) ? Math.max(0, Date.now() - at) : null,
  });
  const valid = c => c && Number.isFinite(c.latitude) && Number.isFinite(c.longitude);
  try {
    const watched = window.__gs26_device_fix;
    if (valid(watched) && Number.isFinite(watched.at) && Date.now() - watched.at <= MAX_AGE_MS) {
      send(coordinate({latitude: watched.latitude, longitude: watched.longitude,
        altitude: watched.altitude_m, accuracy: watched.accuracy_m}, watched.at));
      return;
    }
  } catch (_) {}
  if (typeof window.isSecureContext === 'boolean' && !window.isSecureContext) {
    fail('insecure');
    return;
  }
  if (!navigator.geolocation || typeof navigator.geolocation.getCurrentPosition !== 'function') {
    fail('unsupported');
    return;
  }
  const ask = (options, retry) => {
    try {
      navigator.geolocation.getCurrentPosition(position => {
        const c = position && position.coords;
        if (!valid(c)) { fail('unavailable'); return; }
        send(coordinate(c, Number.isFinite(position.timestamp) ? position.timestamp : Date.now()));
      }, error => {
        const code = error && error.code;
        if (code === 1) fail('denied');
        else if (retry) retry();
        else fail(code === 3 ? 'timeout' : 'unavailable');
      }, options);
    } catch (_) {
      fail('unavailable');
    }
  };
  ask({enableHighAccuracy: true, maximumAge: MAX_AGE_MS, timeout: 15000},
    () => ask({enableHighAccuracy: false, maximumAge: 5 * 60000, timeout: 10000}, null));
});
