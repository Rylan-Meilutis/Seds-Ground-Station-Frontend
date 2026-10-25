// Recorded GPS uses a mean-radius Earth reference; this is not a terrain survey.
export const EARTH_RADIUS_M = 6371008.8;
export function validGps(p) {
 return Array.isArray(p)&&p.length>=3&&p.slice(0,3).every(Number.isFinite)&&Math.abs(p[0])<=90&&Math.abs(p[1])<=180&&p[2]>=-20000&&p[2]<=10000000;
}
export function gpsToLocal(p,origin) {
 if(!validGps(p)||!validGps(origin))return null;
 const rad=Math.PI/180,lat=p[0]*rad,lon=p[1]*rad,a=origin[0]*rad,b=origin[1]*rad;
 const r=EARTH_RADIUS_M+p[2],r0=EARTH_RADIUS_M+origin[2];
 const dx=r*Math.cos(lat)*Math.cos(lon)-r0*Math.cos(a)*Math.cos(b);
 const dy=r*Math.cos(lat)*Math.sin(lon)-r0*Math.cos(a)*Math.sin(b);
 const dz=r*Math.sin(lat)-r0*Math.sin(a);
 // Three.js: east +X, up +Y, north -Z.
 return [-Math.sin(b)*dx+Math.cos(b)*dy,Math.cos(a)*Math.cos(b)*dx+Math.cos(a)*Math.sin(b)*dy+Math.sin(a)*dz,
  Math.sin(a)*Math.cos(b)*dx+Math.sin(a)*Math.sin(b)*dy-Math.cos(a)*dz];
}
export function recordedPath(points,origin,maxGapMs=5000) {
 const positions=[],times=[];let previous=null;
 for(const row of (Array.isArray(points)?points:[]).slice(0,8192)) {
  const gps=[row.latitude,row.longitude,row.altitude_m],position=gpsToLocal(gps,origin);
  if(!position||!Number.isFinite(row.timestamp_ms)){previous=null;continue;}
  if(previous&&row.timestamp_ms>previous.time&&row.break_before!==true&&(typeof row.break_before==='boolean'||row.timestamp_ms-previous.time<=maxGapMs)){
   positions.push(...previous.position,...position);times.push(row.timestamp_ms);
  }
  previous={position,time:row.timestamp_ms};
 }
 return {positions,times};
}
export function recordedSegments(points,origin,maxGapMs=5000) {return recordedPath(points,origin,maxGapMs).positions;}
export function visibleSegmentCount(times,at) {
 if(!Number.isFinite(at))return 0;
 let lo=0,hi=times.length;while(lo<hi){const mid=(lo+hi)>>>1;if(times[mid]<=at)lo=mid+1;else hi=mid;}return lo;
}
