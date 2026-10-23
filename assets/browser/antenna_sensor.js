(() => {
  window.__gs26aim?.stop?.();
  const s = {active:true, error:'', orientation:null, position:null};
  window.__gs26aim = s;
  let watch=null;
  const orientation=e=>{
    if (!s.active || ![e.beta,e.gamma].every(Number.isFinite)) return;
    let alpha=null;
    if (Number.isFinite(e.webkitCompassHeading) && (!Number.isFinite(e.webkitCompassAccuracy) || e.webkitCompassAccuracy>=0)) alpha=(360-e.webkitCompassHeading)%360;
    else if (e.absolute===true && Number.isFinite(e.alpha)) alpha=e.alpha;
    if(alpha!==null) s.orientation={alpha,beta:e.beta,gamma:e.gamma,at:performance.now()};
  };
  s.stop=()=>{s.active=false;if(watch!==null)navigator.geolocation?.clearWatch(watch);window.removeEventListener('deviceorientation',orientation);window.removeEventListener('deviceorientationabsolute',orientation);};
  // Both requests start from the operator's button gesture.
  const permission=typeof DeviceOrientationEvent!=='undefined' && typeof DeviceOrientationEvent.requestPermission==='function' ? DeviceOrientationEvent.requestPermission() : Promise.resolve('granted');
  permission.then(result=>{
    if(!s.active)return;
    if(result!=='granted'){s.error='Compass permission denied';return;}
    window.addEventListener('deviceorientation',orientation);
    window.addEventListener('deviceorientationabsolute',orientation);
  }).catch(()=>{if(s.active)s.error='Compass unavailable';});
  if(navigator.geolocation && window.isSecureContext){
    watch=navigator.geolocation.watchPosition(p=>{
      if(!s.active)return;
      s.position={latitude:p.coords.latitude,longitude:p.coords.longitude,altitude_m:p.coords.altitude,altitude_reference:'ellipsoid',age_ms:Math.floor(Math.max(0,Date.now()-p.timestamp)),stale:false,source:'This device',at:performance.now()};
    },()=>{if(s.active)s.error='Device GPS unavailable or permission denied';},{enableHighAccuracy:true,maximumAge:0,timeout:10000});
  }else{s.error='Device GPS requires HTTPS and location support';}
})();
