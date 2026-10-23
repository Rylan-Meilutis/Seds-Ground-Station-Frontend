import assert from 'node:assert/strict';
import fs from 'node:fs';
const source=process.env.GS_REPLAY_SCENE_SOURCE||new URL('../station_ui/rendering/three/replay-scene.js',import.meta.url);
const {gpsToLocal,recordedSegments,EARTH_RADIUS_M}=await import('data:text/javascript;base64,'+fs.readFileSync(source).toString('base64'));
const near=(a,b,tolerance=.001)=>assert(Math.abs(a-b)<tolerance,`${a} != ${b}`);
assert.deepEqual(gpsToLocal([0,0,0],[0,0,0]),[0,0,0]);
near(gpsToLocal([0,0,10000],[0,0,0])[1],10000);
assert(gpsToLocal([0,.01,0],[0,0,0])[0]>1110,'east is +X');
assert(gpsToLocal([.01,0,0],[0,0,0])[2]< -1110,'north is -Z');
assert(Math.abs(gpsToLocal([0,-179.999,0],[0,179.999,0])[0])<230,'dateline remains local');
assert.equal(gpsToLocal([91,0,0],[0,0,0]),null);
assert.equal(gpsToLocal([0,0,NaN],[0,0,0]),null);
assert.equal(gpsToLocal([0,0,10000001],[0,0,0]),null,'reject out-of-range saved altitude');
const p=(t,alt)=>({timestamp_ms:t,latitude:0,longitude:0,altitude_m:alt});
assert.equal(recordedSegments([p(0,0),p(1000,10),p(9000,90),p(10000,100)],[0,0,0]).length,12,'recording gaps are not joined');
assert.equal(recordedSegments([p(1000,1),p(1000,2),p(2000,3)],[0,0,0]).length,6,'equal timestamps do not make fake segments');
assert.equal(recordedSegments([p(0,0),{...p(1000,1),latitude:null},p(2000,2)],[0,0,0]).length,0);
assert.equal(recordedSegments(null,[0,0,0]).length,0);
assert.equal(recordedSegments(Array.from({length:9000},(_,i)=>p(i,0)),[0,0,0]).length,8191*6,'visual geometry stays bounded');
assert.equal(recordedSegments([{...p(0,0),break_before:true},{...p(20000,10),break_before:false}],[0,0,0]).length,6,'backend continuity survives downsampling');
assert.equal(recordedSegments([{...p(0,0),break_before:true},{...p(1000,10),break_before:true}],[0,0,0]).length,0,'explicit recording break is preserved');
near(EARTH_RADIUS_M,6371008.8);
for(const altitude of [1000,10000,100000])for(const phi of [1.2,1.28]){
 const horizonDepression=Math.acos(EARTH_RADIUS_M/(EARTH_RADIUS_M+altitude));
 const lookDepression=Math.PI/2-phi,halfFov=24*Math.PI/180;
 assert(horizonDepression>=lookDepression-halfFov&&horizonDepression<=lookDepression+halfFov,'default/reference camera contains horizon at apogee');
}
console.log('PASS saved GPS axes, altitude, dateline, invalid data and trajectory gap boundaries');
