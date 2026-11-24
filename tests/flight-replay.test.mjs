// Flight replay engine without a browser: the timeline from rows, attitude from
// velocity, plume scale, chute sizes, time controls, camera modes, overlays,
// channel binding and the predicted label.
import assert from 'node:assert/strict';
import fs from 'node:fs';
import vm from 'node:vm';
const folder=new URL('../station_ui/rendering/three/',import.meta.url);
const load=name=>import('data:text/javascript;base64,'+fs.readFileSync(new URL(name,folder)).toString('base64'));
const T=await load('three.core.js');
const scene=await load('replay-scene.js');
vm.runInThisContext(fs.readFileSync(new URL('offline-replay.js',folder),'utf8'));
const R=globalThis.GSOfflineReplay;
const near=(a,b,tol=1e-6,msg)=>assert(Math.abs(a-b)<=tol,msg||`${a} != ${b}`);

// --- Phase vocabulary and palette (the flight-plan contract) ---------------
assert.deepEqual(scene.PHASE_COLORS,{pad:'#94a3b8',boost:'#f97316',coast:'#facc15',apogee:'#ffffff',drogue:'#38bdf8',main:'#22c55e',landed:'#a78bfa',unknown:'#64748b'});
assert.deepEqual(R.PHASE_COLORS,scene.PHASE_COLORS,'player and scene share the palette');
assert.deepEqual(R.FLIGHT_STATE_NAMES,scene.FLIGHT_STATE_NAMES);
const states=[...scene.FLIGHT_STATE_NAMES,'Fill','ParachuteDeploy','unknown-state',''];
for(const previous of ['pad','boost','coast','drogue','main','landed'])for(const s of [...states,...states.map((_,i)=>i)])assert.equal(R.flightPhaseOf(s,previous),scene.flightPhaseOf(s,previous),`parity ${s} after ${previous}`);
for(const [s,phase] of [['Startup','pad'],['Idle','pad'],['PreFill','pad'],['FillTest','pad'],['NitrogenFill','pad'],['NitrousFill','pad'],['Armed','pad'],['Launch','boost'],['Ascent','boost'],['Coast','coast'],['Apogee','coast'],['Descent','drogue'],['Reefing','main'],['Landed','landed'],['Recovery','landed']])assert.equal(scene.flightPhaseOf(s,'pad'),phase,s);
assert.equal(scene.flightPhaseOf('Aborted','coast'),'coast','Aborted keeps the previous phase');
assert.equal(scene.flightPhaseOf('InFlight','pad'),'boost','InFlight with no detail is boost');
assert.equal(scene.flightPhaseOf('InFlight','coast'),'coast','InFlight keeps a detailed phase once seen');
assert.equal(scene.flightPhaseOf(12,'drogue'),'main','numbers map like names');
console.log('PASS phase vocabulary, palette and FLIGHT_STATE mapping (player and scene agree)');

// --- Event/phase timeline from rows -----------------------------------------
const row=(id,t,type,values,sender='FC',phase)=>({id,timestamp_ms:t,source_timestamp_ms:null,sender_id:sender,data_type:type,values,phase});
const doc=samples=>({format:'seds-replay',version:1,mode:'flight',schema:{},layout:{},view_config:{stages:[],ground_systems:[]},valve_mappings:[],samples});
const flight=[row(1,0,'FLIGHT_STATE',[6],'FC','Armed'),row(2,10000,'FLIGHT_STATE',[7],'FC','Launch'),row(3,10200,'FLIGHT_STATE',[8],'FC','Ascent'),row(4,17000,'FLIGHT_STATE',[9]),row(5,36000,'FLIGHT_STATE',[10]),row(6,36500,'FLIGHT_STATE',[11]),row(7,150000,'FLIGHT_STATE',[12]),row(8,200000,'FLIGHT_STATE',[13]),row(9,220000,'BAROMETER_DATA',[0,0,0])];
const p=R.validate(doc(flight)),tl=R.flightTimeline(p);
assert.deepEqual(tl.segments.map(s=>[s.phase,s.from,s.to]),[['pad',0,10000],['boost',10000,17000],['coast',17000,36500],['drogue',36500,150000],['main',150000,200000],['landed',200000,220000]]);
assert.deepEqual(tl.events.map(e=>[e.kind,e.t]),[['ignition',10000],['burnout',17000],['apogee',36000],['drogue',36500],['main',150000],['landing',200000]]);
assert.equal(tl.t0,10000);assert.equal(R.phaseAt(tl,12000),'boost');assert.equal(R.phaseAt(tl,36200),'coast');assert.equal(R.phaseAt(tl,210000),'landed');
// Aborted and duplicate rows do not invent segments; InFlight alone is boost.
const network=R.validate(doc([row(1,0,'FLIGHT_STATE',[6]),row(2,5000,'FLIGHT_STATE',[16]),row(3,6000,'FLIGHT_STATE',[15]),row(4,7000,'FLIGHT_STATE',[16]),row(5,90000,'FLIGHT_STATE',[13])]));
assert.deepEqual(R.flightTimeline(network).segments.map(s=>s.phase),['pad','boost','landed']);
// No Apogee row: a flight review fills it, never overriding a recorded event.
const noApogee=R.validate(doc(flight.filter(r=>r.id!==5)));
const review={flight:{events:[{name:'Apogee (max altitude)',t_ms:35800},{name:'Drogue deploy',t_ms:99999},{name:'Liftoff',t_ms:10000}]}};
const filled=R.flightTimeline(noApogee,{flight_review:review});
assert.equal(filled.events.find(e=>e.kind==='apogee').t,35800);assert.equal(filled.events.find(e=>e.kind==='apogee').source,'flight review');
assert.equal(filled.events.find(e=>e.kind==='drogue').t,36500,'recorded rows win over the review');
// With neither, the highest recorded altitude between ignition and the drogue.
const byAltitude=R.flightTimeline(noApogee,{},{altitudeRows:[{t:20000,v:1000},{t:34000,v:3050},{t:40000,v:2900}]});
assert.equal(byAltitude.events.find(e=>e.kind==='apogee').t,34000);
// A plan's events (t_s from ignition) fill a plan replay.
const planEvents=R.flightTimeline(R.validate(doc([row(1,5000,'BAROMETER_DATA',[0,0,0])])),{flight_plan:{created_ms:5000,events:[{kind:'ignition',t_s:0},{kind:'apogee',t_s:26.4},{kind:'landing',t_s:204}]}});
assert.deepEqual(planEvents.events.map(e=>[e.kind,e.t]),[['ignition',5000],['apogee',31400],['landing',209000]]);
assert.equal(R.eventKindOf('Touchdown / Landed'),'landing');assert.equal(R.eventKindOf('T-0'),'ignition');assert.equal(R.eventKindOf('Main deploy'),'main');
console.log('PASS event/phase timeline from FLIGHT_STATE rows, review/plan/altitude fill-ins');

// --- T-clock -------------------------------------------------------------------
for(const [ms,text] of [[0,'T+ 00:00'],[3999,'T+ 00:03'],[65000,'T+ 01:05'],[3600000,'T+ 1:00:00'],[3723000,'T+ 1:02:03'],[90061000,'T+ 1:01:01:01'],[-1500,'T- 00:02'],[-600000,'T- 10:00'],[NaN,'T --:--'],[null,'T --:--']])assert.equal(R.formatTClock(ms),text,String(ms));
console.log('PASS T-clock formats');

// --- Attitude from velocity ----------------------------------------------------
assert.equal(scene.attitudeFromVelocity([0,1,0]),null,'below the minimum speed');
assert.equal(scene.attitudeFromVelocity([NaN,10,0]),null);
const turn=(q,v)=>new T.Vector3(...v).applyQuaternion(new T.Quaternion(...q));
for(const v of [[0,100,0],[30,100,0],[0,100,-40],[50,10,50],[0,-20,0],[5,-60,0]]){
 const a=scene.attitudeFromVelocity(v),axis=turn(a.quaternion,[0,1,0]),n=Math.hypot(...v);
 near(axis.x,v[0]/n,1e-9);near(axis.y,v[1]/n,1e-9);near(axis.z,v[2]/n,1e-9);
}
near(scene.attitudeFromVelocity([0,100,0]).pitch_deg,90);near(scene.attitudeFromVelocity([100,100,0]).pitch_deg,45,1e-9);
near(scene.attitudeFromVelocity([10,50,0]).heading_deg,90,1e-9,'east is 90°');near(scene.attitudeFromVelocity([0,50,-10]).heading_deg,0,1e-9,'north (-Z) is 0°');
// From a GPS track: interpolated position and a smoothed velocity, no bridge over a break.
const times=[0,1000,2000,3000,4000,10000],positions=[[0,0,0],[0,100,0],[10,200,0],[20,300,0],[30,400,0],[90,400,0]],sampler=scene.trackSampler(times,positions,[5],1000);
assert.deepEqual(sampler.position(1500),[5,150,0]);assert.equal(sampler.position(-1),null);
const v2=sampler.velocity(2000);near(v2[1],100,1e-9);near(v2[0],10,1e-9);
assert.deepEqual(sampler.position(7000),[30,400,0],'a break holds the last position');
console.log('PASS attitude from velocity (quaternion turns +Y onto the path), track interpolation and smoothing');

// --- Plume scale -------------------------------------------------------------------
assert.deepEqual(scene.plumeScale({phase:'coast',thrust:900,thrustMax:1000}),{scale:0,source:'none'});
assert.deepEqual(scene.plumeScale({phase:'boost'}),{scale:1,source:'phase'});
assert.deepEqual(scene.plumeScale({phase:'boost',thrust:500,thrustMax:1000}),{scale:.5,source:'thrust'});
assert.deepEqual(scene.plumeScale({phase:'boost',thrust:10,thrustMax:1000}),{scale:0,source:'thrust'},'tail-off below 3 % is out');
assert.deepEqual(scene.plumeScale({phase:'boost',thrust:50,thrustMax:1000}),{scale:.15,source:'thrust'},'a lit motor stays visible');
assert.deepEqual(scene.plumeScale({phase:'boost',chamber:300,chamberMax:600}),{scale:.5,source:'chamber'},'chamber pressure when thrust is not recorded');
assert.deepEqual(scene.plumeScale({phase:'boost',thrust:null,thrustMax:1000,chamber:600,chamberMax:600}),{scale:1,source:'chamber'});
assert.deepEqual(scene.plumeScale({phase:'pad',burning:true,thrust:2000,thrustMax:1000}),{scale:1.25,source:'thrust'},'test-fire burn, capped');
assert.deepEqual(scene.plumeScale({phase:'boost',burning:false}),{scale:0,source:'none'});
console.log('PASS plume scale from thrust, chamber pressure or phase');

// --- Chute sizes from geometry ------------------------------------------------------
const geometry=JSON.parse(fs.readFileSync(new URL('./fixtures/backend-contract/vehicle/geometry.json',import.meta.url)));
const sizes=scene.chuteSizes(geometry);
for(const role of ['drogue','main']){const r=geometry.recovery.find(d=>d.role===role);assert.equal(sizes[role].diameter_m,r.diameter_m);assert.equal(sizes[role].radius_m,r.diameter_m/2);assert.equal(sizes[role].line_length_m,r.line_length_m);assert.equal(sizes[role].separation_x_m,r.separation_x_m);}
assert.equal(scene.chuteSizes({...geometry,recovery:[]}).main,null);assert.equal(scene.chuteSizes(null).main.diameter_m,1.5,'generic rocket');
const fv=scene.createFlightVehicle(T,geometry);
for(const role of ['drogue','main'])near(fv.chutes[role].getObjectByName(role+'-canopy').geometry.parameters.radius*2,geometry.recovery.find(d=>d.role===role).diameter_m,1e-9,'canopy at the design size');
assert.equal(fv.fore.getObjectByName('nose')!=null,true);assert.equal(fv.aft.getObjectByName('fin')!=null,true,'the joint splits fore and aft');
fv.setState(scene.phaseVisual('boost',{plume:.8}));assert.ok(fv.plume.visible);assert.equal(fv.chutes.drogue.visible,false);
fv.setState(scene.phaseVisual('drogue'));assert.ok(fv.chutes.drogue.visible&&!fv.chutes.main.visible&&!fv.plume.visible&&fv.fore.position.y>0);
fv.setState(scene.phaseVisual('main'));assert.ok(fv.chutes.main.visible&&fv.chutes.drogue.visible);
fv.setState(scene.phaseVisual('landed'));assert.ok(!fv.chutes.main.visible,'canopies are not drawn on the ground');
assert.equal(scene.createFlightVehicle(T,geometry,{quality:'low'}).plume.getObjectByName('plume-glow'),undefined,'weak devices get no glow');
console.log('PASS chute sizes and the joint from geometry, flight state on the model');

// --- Time controls -----------------------------------------------------------------
const events=[{kind:'ignition',t:10000},{kind:'burnout',t:17000},{kind:'apogee',t:36000},{kind:'drogue',t:36500},{kind:'landing',t:200000}];
const c=R.createClock({start:0,end:220000,events,frames:[0,2000,4000,6000,10000,12000]});
assert.equal(c.stepFrame(1),2000);assert.equal(c.stepFrame(1),4000);assert.equal(c.stepFrame(-1),2000);assert.equal(c.stepFrame(-1),0);assert.equal(c.stepFrame(-1),0,'clamped at the start');
assert.equal(c.stepEvent(1).kind,'ignition');assert.equal(c.t,10000);assert.equal(c.stepEvent(1).kind,'burnout');assert.equal(c.stepEvent(-1).kind,'ignition');
assert.equal(c.jump('apogee').t,36000);assert.equal(c.t,36000);assert.equal(c.jump('main'),null);assert.equal(c.t,36000,'a missing event does not move');
assert.equal(c.setSpeed(50),20);assert.equal(c.setSpeed(.01),.1);c.setSpeed(2);
c.seek(0);c.play();c.tick(500);assert.equal(c.t,1000);
// Auto slow motion near an event.
c.autoSlow=true;c.seek(9500);assert.equal(c.effectiveSpeed(),.25);c.seek(50000);assert.equal(c.effectiveSpeed(),2);
c.seek(9500);c.tick(1000);assert.equal(c.t,9750);c.autoSlow=false;
// A/B loop: playing past B wraps to A.
c.seek(10000);c.setLoop('a');c.seek(12000);c.setLoop('b');c.seek(11800);c.play();c.tick(200);assert.ok(c.t>=10000&&c.t<=12000,'wrapped into the loop: '+c.t);assert.ok(c.playing);
c.setLoop('a',15000);assert.deepEqual(c.loop,{a:12000,b:15000},'A after B swaps');c.clearLoop();assert.equal(c.loopActive(),false);
c.seek(219900);c.play();c.tick(1000);assert.equal(c.t,220000);assert.equal(c.playing,false,'stops at the end');c.play();assert.equal(c.t,0,'play at the end restarts');
console.log('PASS time controls: step frame/event, jump, speed range, slow motion, A/B loop');

// --- Camera modes -------------------------------------------------------------------
assert.deepEqual(scene.CAMERA_MODES,['follow','chase','pad','station','orbit','top']);
const ctx={rocket:[100,500,-200],direction:[0,1,0],pad:[0,0,0],station:[300,2,400],theta:.6,phi:1.2,distance:20,target:[0,0,0]};
const dist=(a,b)=>Math.hypot(a[0]-b[0],a[1]-b[1],a[2]-b[2]);
const follow=scene.cameraPose('follow',ctx);near(dist(follow.position,ctx.rocket),20,1e-9);assert.deepEqual(follow.target,ctx.rocket);
const chase=scene.cameraPose('chase',{...ctx,direction:[0,.6,.8]});assert.ok(chase.position[1]<ctx.rocket[1]&&chase.position[2]<ctx.rocket[2],'behind along the path');
const pad=scene.cameraPose('pad',ctx);near(pad.position[1],1.7,1e-9);assert.deepEqual(pad.target,ctx.rocket);
const station=scene.cameraPose('station',ctx);assert.deepEqual(station.position,[300,4,400]);assert.deepEqual(station.target,ctx.rocket);
assert.equal(scene.cameraPose('station',{...ctx,station:null}).mode,'pad','no station position: pad view');
const top=scene.cameraPose('top',ctx);assert.deepEqual([top.position[0],top.position[2]],[100,-200]);assert.ok(top.position[1]>ctx.rocket[1]);assert.deepEqual(top.up,[0,0,-1],'north up');
const orbit=scene.cameraPose('orbit',ctx);assert.deepEqual(orbit.target,[0,0,0],'free orbit keeps its own target');
assert.equal(scene.cameraPose('nonsense',ctx).mode,'follow');
console.log('PASS camera modes: follow, chase, pad, station (pad fallback), free orbit, top-down');

// --- Overlays (MapOverlay) ----------------------------------------------------------
const overlay={id:'flight_plan:x',kind:'flight_plan',label:'Predicted: test',predicted:true,tracks:[{id:'boost',phase:'boost',style:'predicted',points:[[31.758,-102.046,890],[31.759,-102.046,2000]]},{id:'bad',points:[[1,2,3]]}],markers:[{id:'landing',kind:'landing',label:'Predicted landing',lat_deg:31.76,lon_deg:-102.03,alt_msl_m:890}],areas:[{id:'e',kind:'landing_ellipse',polygon:[[31.76,-102.03],[31.761,-102.031],[31.761,-102.029]]}]};
const og=scene.overlayGeometry([overlay],[31.758,-102.046,890],()=>5,0);
assert.equal(og.tracks.length,1);assert.equal(og.tracks[0].dashed,true);assert.equal(og.tracks[0].opacity,.55);assert.equal(og.tracks[0].color,'#f97316');
assert.equal(og.markers[0].position[1],5,'the landing marker sits on the ground');assert.ok(og.areas[0].ring.every(p=>p[1]===5),'areas drape on heightAt');assert.equal(og.areas[0].color,'#a78bfa');
assert.equal(scene.overlayGeometry([{...overlay,kind:'los_prediction'}],[31.758,-102.046,890]).tracks[0].color,'#f472b6','LOS prediction is pink');
const group=scene.buildOverlayGroup(T,[overlay],[31.758,-102.046,890],()=>0);
assert.ok(group.children.some(n=>n.name==='OverlayTrack'&&n.material.isLineDashedMaterial&&n.material.opacity===.55));assert.ok(group.children.some(n=>n.name==='OverlayAreaFill'&&n.userData.kind==='landing_ellipse'));
assert.equal(scene.overlayGeometry('nonsense',[0,0,0]).tracks.length,0);
console.log('PASS MapOverlay: dashed lighter predicted tracks, ground-draped ellipse and markers');

// --- Predicted label -----------------------------------------------------------------
assert.deepEqual(R.predictedInfo({}),{predicted:false,label:''});
assert.deepEqual(R.predictedInfo({predicted:true,label:'Predicted: SWEET V6 8.5 kg'}),{predicted:true,label:'Predicted: SWEET V6 8.5 kg'});
assert.deepEqual(R.predictedInfo({},{manifest:{predicted:true,label:'8.5 kg nominal'}}),{predicted:true,label:'Predicted: 8.5 kg nominal'});
assert.equal(R.predictedInfo({predicted:true}).label,'Predicted: Predicted flight'.replace('Predicted: Predicted','Predicted'));
console.log('PASS predicted label from replay.predicted or manifest.predicted');

// --- Channel binding via the layout -----------------------------------------------------
const layout={data_tab:{tabs:[{id:'BAROMETER_DATA',label:'Barometer Data',channels:['Pressure','Altitude','Temp']},{id:'LOADCELL',label:'Loadcell',channels:['x'],subtabs:[{id:'chamber',data_type:'COMBUSTION_CHAMBER_PRESSURE',sender_id:'DAQ',label:'Chamber pressure',channels:['Chamber pressure (psig)']}]},{id:'VEHICLE_SPEED',label:'Vehicle Speed',channels:['Vertical Speed'],channel_formatters:[{suffix:' m/s'}]}]}};
const bound=R.validate({...doc([row(1,0,'BAROMETER_DATA',[900,120,20]),row(2,0,'COMBUSTION_CHAMBER_PRESSURE',[350],'DAQ'),row(3,0,'VEHICLE_SPEED',[42]),row(4,0,'KG50',[1]),row(5,0,'KG1000',[300]),row(6,0,'GPS_STATUS',[3,9,0,0]),row(7,0,'BATTERY_VOLTAGE',[8],'PB'),row(8,0,'BATTERY_VOLTAGE',[12],'GB')]),layout});
const channels=Object.fromEntries(R.discoverChannels(bound).map(c=>[c.role,c]));
assert.deepEqual(channels.altitude.binding,{data_type:'BAROMETER_DATA',sender_id:'FC',index:1},'channel index from the layout name');
assert.equal(channels.chamber.unit,'psig');assert.equal(channels.chamber.binding.sender_id,'DAQ');
assert.equal(channels.velocity.unit,'m/s');assert.equal(channels.thrust.binding.data_type,'KG1000');assert.equal(channels.thrust.unit,'kg');
assert.equal(channels.battery.binding.sender_id,'PB','the vehicle battery first');assert.equal(R.formatChannel(channels.gps,3),'3D fix');
assert.equal(channels.radio.absent,true);assert.equal(R.formatChannel(channels.radio,null),'—','a missing value shows a dash');
const noLayout=Object.fromEntries(R.discoverChannels(R.validate(doc([row(1,0,'GPS_DATA',[31,-102,900]),row(2,1000,'GPS_DATA',[31,-102,1000])]))).map(c=>[c.role,c]));
assert.equal(noLayout.altitude.derived,'gps_agl');assert.equal(noLayout.velocity.derived,'gps_speed');
console.log('PASS channel binding through the layout, derived GPS fallbacks and dashes');
