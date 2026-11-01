import assert from 'node:assert/strict';
import fs from 'node:fs';
import vm from 'node:vm';
const folder=new URL('../station_ui/rendering/three/',import.meta.url);
const moduleFrom=async name=>import('data:text/javascript;base64,'+fs.readFileSync(new URL(name,folder)).toString('base64'));
const T=await moduleFrom('three.core.js');
const {createReplayAssembly,configuredTankFraction,replayValveState,REPLAY_VALVE_LIMIT,REPLAY_TANK_LIMIT}=await moduleFrom('replay-scene.js');
vm.runInThisContext(fs.readFileSync(new URL('offline-replay.js',folder),'utf8'));
assert.equal(REPLAY_VALVE_LIMIT,16);assert.equal(REPLAY_TANK_LIMIT,8);
assert.equal(replayValveState(null),'unknown');assert.equal(replayValveState(2),'unknown');assert.equal(replayValveState(0),'closed');assert.equal(replayValveState(1),'open');
const tank={kind:'tank',unit:'kg',min:1,max:11,value:6,binding:{data_type:'FILL',index:0}};
for(const unit of ['kg','g','%','percent','psi','bar','kPa','V','',undefined])for(const value of [null,NaN,0,6,11]){
 const c={...tank,unit,value};assert.equal(configuredTankFraction(c),GSOfflineReplay.tankFillFraction(c),'shared3D and offline textual fill contracts must agree');
}
const flight=createReplayAssembly(T,'flight'),stand=createReplayAssembly(T,'test_fire');
assert.ok(flight.group.getObjectByName('Nose'));assert.equal(flight.group.getObjectByName('HorizontalMotor'),undefined);
assert.ok(stand.group.getObjectByName('MotorCradle'));assert.equal(stand.group.getObjectByName('Nose'),undefined);
assert.equal(stand.group.getObjectByName('HorizontalMotor').rotation.z,Math.PI/2);
for(const a of [flight,stand]){
 a.update({attitude:[10,20,30],valves:[{label:'Pilot',value:1},{label:'Vent',value:0},{label:'Dump',value:null}],components:[tank,{...tank,unit:'psi'}]});
 assert.deepEqual(a.valves.map(v=>v.part.userData.state),['open','closed','unknown']);
 assert.equal(a.valves[0].handle.rotation.z,Math.PI/2);assert.equal(a.valves[1].handle.rotation.z,0);
 assert.equal(a.tanks[0].fluid.scale.y,.5);assert.equal(a.tanks[0].fluid.visible,true);
 assert.equal(a.tanks[1].fluid.visible,false);assert.equal(a.tanks[1].unknown.visible,true,'pressure does not create a liquid surface');
 a.update({attitude:[null,20,30],valves:[{label:'Pilot',value:null}],components:[{...tank,value:null}]});
 assert.equal(a.group.userData.attitudeKnown,false);assert.equal(a.valves[0].part.userData.state,'unknown');assert.equal(a.valves[1].part.visible,false);
 assert.equal(a.tanks[0].fluid.visible,false);assert.equal(a.tanks[0].unknown.visible,true);
 const huge={valves:Array.from({length:128},(_,i)=>({label:'Valve'+i,value:i%2})),components:Array.from({length:100},(_,i)=>({...tank,id:'tank'+i}))};
 a.update(huge);assert.equal(a.valves.length,16);assert.equal(a.tanks.length,8);assert.equal(a.group.userData.omittedValves,112);assert.equal(a.group.userData.omittedTanks,92);
 let count=0;a.group.traverse(()=>count++);for(let i=0;i<30;i++)a.update(huge);let after=0;a.group.traverse(()=>after++);assert.equal(after,count,'seeks reuse bounded geometry');
 let triangles=0,geometries=0,materials=0,disposedGeometry=0,disposedMaterial=0;
 a.group.traverse(n=>{if(n.geometry){geometries++;triangles+=(n.geometry.index?.count||n.geometry.attributes.position.count)/3;n.geometry.addEventListener('dispose',()=>disposedGeometry++);}if(n.material){materials++;n.material.addEventListener('dispose',()=>disposedMaterial++);}});
 assert.ok(triangles<8000,'procedural model remains bounded');a.dispose();assert.equal(disposedGeometry,geometries);assert.equal(disposedMaterial,materials);
}
console.log('PASS actual Three geometry: distinct flight/motor stand, named valve states, pressure/unknown refusal, explicit mass range, bounded repeated seeks and disposal');
