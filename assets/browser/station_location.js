// One explicit device-location request for the station location form.
// A single getCurrentPosition call: no watch is left running and nothing is saved.
// The eval wrapper closes its channel when this script finishes, so the script
// awaits the answer; otherwise a slow permission prompt could lose the result.
await new Promise(done => {
  let sent = false;
  const send = value => { if (!sent) { sent = true; dioxus.send(value); done(); } };
  const fail = (code, message) => send({code, error: message});
  if (typeof window.isSecureContext === 'boolean' && !window.isSecureContext) {
    fail('insecure', 'Device location needs a secure page. Open this station over https:// or enter coordinates.');
    return;
  }
  if (!navigator.geolocation || typeof navigator.geolocation.getCurrentPosition !== 'function') {
    fail('unsupported', 'This browser cannot share its location. Enter coordinates manually.');
    return;
  }
  try {
    navigator.geolocation.getCurrentPosition(position => {
      const c = position && position.coords;
      if (!c || !Number.isFinite(c.latitude) || !Number.isFinite(c.longitude)) {
        fail('unavailable', 'The device did not provide valid coordinates.');
        return;
      }
      send({coordinate: {latitude: c.latitude, longitude: c.longitude,
        altitude_m: Number.isFinite(c.altitude) ? c.altitude : null,
        accuracy_m: Number.isFinite(c.accuracy) && c.accuracy >= 0 ? c.accuracy : null,
        source: 'device_location'}});
    }, error => {
      const code = error && error.code;
      if (code === 1) fail('denied', 'Location permission was denied. Allow location for this page or enter coordinates.');
      else if (code === 3) fail('timeout', 'The device did not find its location in time. Try again outdoors or enter coordinates.');
      else fail('unavailable', 'The device location is unavailable. Check location services or enter coordinates.');
    }, {enableHighAccuracy: true, timeout: 20000, maximumAge: 0});
  } catch (_) {
    fail('unavailable', 'The device location is unavailable. Check location services or enter coordinates.');
  }
});
