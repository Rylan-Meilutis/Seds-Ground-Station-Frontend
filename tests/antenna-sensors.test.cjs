const {test}=require('node:test');
const assert=require('node:assert/strict');
const vm=require('node:vm');
const fs=require('node:fs');
const path=require('node:path');
const root=path.resolve(__dirname,'../assets/browser');
const source=fs.readFileSync(path.join(root,'antenna_sensor.js'),'utf8');
const snapshot=fs.readFileSync(path.join(root,'antenna_sensor_snapshot.js'),'utf8');
function device(permission=Promise.resolve('granted')) {
  const listeners=new Map();const cleared=[];let fix;let now=10.25;let sent;
  const window={isSecureContext:true,addEventListener:(k,v)=>listeners.set(k,v),removeEventListener:k=>listeners.delete(k)};
  const context=vm.createContext({window,navigator:{geolocation:{watchPosition:f=>{fix=f;return 7;},clearWatch:id=>cleared.push(id)}},DeviceOrientationEvent:{requestPermission:()=>permission},Date:{now:()=>1000},performance:{now:()=>now},dioxus:{send:v=>sent=v}});
  return {window,listeners,cleared,start:()=>vm.runInContext(source,context),fix:v=>fix(v),clock:v=>now=v,read:()=>{vm.runInContext(snapshot,context);return sent;}};
}
test('relative yaw never pretends to be north and absolute orientation expires',async()=>{
 const d=device();d.start();await Promise.resolve();const event=d.listeners.get('deviceorientation');
 event({alpha:10,beta:90,gamma:0,absolute:false});assert.equal(d.read().orientation,null);
 event({alpha:10,beta:90,gamma:0,absolute:true});assert.equal(d.read().orientation.alpha,10);
 d.clock(4011);assert.ok(d.read().orientation.age_ms>3000);
});
test('device fix keeps integer age and altitude datum across the JS/Rust bridge',async()=>{
 const d=device();d.start();await Promise.resolve();d.fix({timestamp:991.75,coords:{latitude:43,longitude:-78,altitude:200}});d.clock(12.75);
 const fix=d.read().position;assert.equal(fix.age_ms,10);assert.equal(fix.altitude_reference,'ellipsoid');assert.equal(fix.altitude_m,200);
});
test('exit removes watches and cannot resurrect listeners after a permission prompt',async()=>{
 let resolve;const d=device(new Promise(r=>resolve=r));d.start();d.window.__gs26aim.stop();resolve('granted');await Promise.resolve();
 assert.equal(d.listeners.size,0);assert.deepEqual(d.cleared,[7]);
});
test('restarting sensors replaces the earlier watch instead of leaking listeners',async()=>{
 const d=device();d.start();await Promise.resolve();d.start();await Promise.resolve();assert.deepEqual(d.cleared,[7]);assert.equal(d.listeners.size,2);d.window.__gs26aim.stop();assert.equal(d.listeners.size,0);
});
