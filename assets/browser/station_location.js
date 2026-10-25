(() => {
  const fail = message => dioxus.send({error: message});
  if (!navigator.geolocation) { fail('Location is unavailable on this device.'); return; }
  navigator.geolocation.getCurrentPosition(position => {
    const c = position.coords;
    if (!Number.isFinite(c.latitude) || !Number.isFinite(c.longitude)) {
      fail('The device did not provide valid coordinates.'); return;
    }
    dioxus.send({coordinate: {latitude: c.latitude, longitude: c.longitude,
      altitude_m: Number.isFinite(c.altitude) ? c.altitude : null,
      accuracy_m: Number.isFinite(c.accuracy) ? c.accuracy : null,
      source: 'device_location'}});
  }, () => fail('Location access failed. Check device permissions or enter coordinates.'),
  {enableHighAccuracy: true, timeout: 15000, maximumAge: 1000});
})();
