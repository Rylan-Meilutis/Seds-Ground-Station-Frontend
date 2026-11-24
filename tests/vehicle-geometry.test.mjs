// The station's vehicle geometry (GroundStation GET /api/vehicle/geometry,
// docs/vehicle-geometry.md) drives the 3D vehicle and the replay assembly.
// The fixture is the backend contract snapshot for the built-in SWEET_V6.ork.
import assert from 'node:assert/strict';
import fs from 'node:fs';
const folder=new URL('../station_ui/rendering/three/',import.meta.url);
const moduleFrom=async name=>import('data:text/javascript;base64,'+fs.readFileSync(new URL(name,folder)).toString('base64'));
const T=await moduleFrom('three.core.js');
const {vehicleGeometry,genericVehicleGeometry,buildVehicle,createReplayAssembly}=await moduleFrom('replay-scene.js');
const fixtureDir=process.env.GS_CONTRACT_FIXTURES_DIR?new URL('file://'+process.env.GS_CONTRACT_FIXTURES_DIR.replace(/\/?$/,'/')):new URL('fixtures/backend-contract/',import.meta.url);
const text=fs.readFileSync(new URL('vehicle/geometry.json',fixtureDir),'utf8');
const g=vehicleGeometry(text);

// Parsing: the station's reply is accepted; other versions and nonsense are refused.
assert.ok(g,'the station geometry parses');
assert.equal(vehicleGeometry({...g,version:2}),null,'an unknown version is refused');
assert.equal(vehicleGeometry({...g,schema:'other'}),null);
assert.equal(vehicleGeometry({...g,profile:[[0,0],[-1,.1]]}),null);
assert.equal(vehicleGeometry('not json'),null);assert.equal(vehicleGeometry(null),null);
assert.ok(vehicleGeometry(genericVehicleGeometry()),'the fallback is itself valid');

// Key numbers of the design (OpenRocket SWEET_V6: about 15 ft by 6 in, 3 fins).
assert.ok(Math.abs(g.length_m-4.3866)<1e-3,String(g.length_m));
assert.ok(Math.abs(g.diameter_m-.15646)<1e-4);
assert.equal(g.fins[0].count,3);assert.ok(Math.abs(g.fins[0].span_m-.174625)<1e-6);
assert.ok(Math.abs(g.nose.length_m-.5334)<1e-6);

// 3D: true to scale, with the node names motions and clips address.
const vehicle=buildVehicle(T,g),stage=vehicle.getObjectByName('stage-1');
for(const name of ['nose','airframe-upper','airframe-join','airframe-lower','gimbal-pivot','engine','motor-flame','fin-pivot-0','fin-pivot-2','drogue-parachute','main-parachute'])assert.ok(vehicle.getObjectByName(name),name);
assert.equal(vehicle.getObjectByName('fin-pivot-3'),undefined,'three fins');
let fins=0;vehicle.traverse(n=>{if(n.name==='fin')fins++;});assert.equal(fins,3);
vehicle.getObjectByName('motor-flame').visible=false;
const size=v=>{const box=new T.Box3();v.updateMatrixWorld(true);v.traverseVisible(n=>{if(n.isMesh)box.expandByObject(n);});return box.getSize(new T.Vector3());};
const body=size(stage);
assert.ok(Math.abs(body.y-g.length_m)<.02,'length '+body.y);
// Three fins 120 degrees apart: the first reaches the body radius plus the span along +X.
const reach=new T.Box3().setFromObject(vehicle.getObjectByName('fin-pivot-0')).max.x;
assert.ok(Math.abs(reach-g.span_m/2)<.01,'fin tip radius '+reach);
assert.ok(Math.abs(body.z-2*Math.sin(Math.PI/3)*g.span_m/2)<.02,'width across fins '+body.z);
const nose=size(vehicle.getObjectByName('nose'));assert.ok(Math.abs(nose.y-.5334)<1e-3,'nose length '+nose.y);
// The chutes leave through the recovery tube's aft joint.
const sep=g.separations[0].x_m,yOf=x=>g.length_m/2-x;
assert.ok(Math.abs(vehicle.getObjectByName('main-parachute').position.y-yOf(sep))<1e-9);
assert.ok(Math.abs(vehicle.getObjectByName('airframe-join').position.y-yOf(sep))<1e-9);
// The fins sit aft, at the design's station.
const fin=vehicle.getObjectByName('fin-pivot-0');assert.ok(Math.abs(fin.position.y-yOf(g.fins[0].x_m+g.fins[0].root_chord_m/2))<1e-9);

// Without station data: the generic rocket.
const generic=buildVehicle(T,null);assert.ok(generic.getObjectByName('fin-pivot-3'),'generic: four fins');

// Replays: the station vehicle in flight, its motor on the stand with the rocket apart.
const flight=createReplayAssembly(T,'flight',g),stand=createReplayAssembly(T,'test_fire',g);
assert.equal(flight.group.userData.vehicleSource,'default');
assert.ok(flight.group.getObjectByName('Nose'));assert.ok(flight.group.getObjectByName('InternalTank'));
const motor=stand.group.getObjectByName('HorizontalMotor');assert.ok(motor&&stand.group.getObjectByName('AirframeReference'));
const tank=g.tanks[0],mount=g.internals.find(i=>i.role==='motor_mount');
const motorLength=size(motor).x;assert.ok(motorLength>Math.max(tank.x1_m,mount.x1_m)-Math.min(tank.x0_m,mount.x0_m)-.01,'motor length '+motorLength);
for(const a of [flight,stand]){let triangles=0;a.group.traverse(n=>{if(n.geometry)triangles+=(n.geometry.index?.count||n.geometry.attributes.position.count)/3;});assert.ok(triangles<8000,'bounded '+triangles);a.dispose();}
assert.equal(createReplayAssembly(T,'flight',{...g,version:9}).group.userData.vehicleSource,'generic','a bad reply falls back');
console.log('PASS station vehicle geometry: parse/refuse, true-scale 3D with renderer node names, generic fallback, flight and stand replays');
