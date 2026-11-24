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

// Shared saved-replay geometry. Dimensions are illustrative, never a survey or
// a claim about hardware plumbing. No asset fetches or live data are used here.
export const REPLAY_VALVE_LIMIT=16,REPLAY_TANK_LIMIT=8;
export function configuredTankFraction(c){
 if(!c||typeof c!=='object')return null;
 const unit=typeof c.unit==='string'?c.unit.trim().toLowerCase():'';
 if(c.kind!=='tank'||!['%','percent','kg','g'].includes(unit)||!c.binding||typeof c.binding.data_type!=='string'||!c.binding.data_type.trim()||!Number.isFinite(c.value)||!Number.isFinite(c.min)||!Number.isFinite(c.max)||c.max<=c.min)return null;
 return Math.max(0,Math.min(1,(c.value-c.min)/(c.max-c.min)));
}
// Raw validity is authoritative; only saved canonical position words orient a handle.
export function replayValveState(value,row){
 if(value!==0&&value!==1)return 'unknown';
 const state=row?.state!==undefined?row.state:row?.[value===1?'true_label':'false_label'];
 if(state===undefined)return value===1?'open':'closed';
 const canonical=typeof state==='string'?state.trim().toLowerCase():'';
 return canonical==='open'||canonical==='closed'?canonical:'unknown';
}
// Station vehicle geometry (GET /api/vehicle/geometry, docs/vehicle-geometry.md):
// metres along the axis from the nose tip; the station generates it from its
// OpenRocket design, so nothing here is specific to one vehicle. Without it a
// neutral generic rocket is drawn.
export const VEHICLE_GEOMETRY_SCHEMA='gs26.vehicle_geometry',VEHICLE_GEOMETRY_VERSION=1;
const finiteAll=a=>Array.isArray(a)&&a.every(Number.isFinite);
export function vehicleGeometry(value){
 let g=value;if(typeof g==='string'){try{g=JSON.parse(g);}catch{return null;}}
 if(!g||typeof g!=='object'||g.schema!==VEHICLE_GEOMETRY_SCHEMA||g.version!==VEHICLE_GEOMETRY_VERSION)return null;
 const L=g.length_m,D=g.diameter_m;if(!Number.isFinite(L)||L<.1||L>200||!Number.isFinite(D)||D<=0||D>=L)return null;
 if(!Array.isArray(g.profile)||g.profile.length<2||g.profile.length>4096)return null;
 let last=-Infinity;for(const p of g.profile){if(!finiteAll(p)||p.length<2||p[1]<0||p[0]<last-1e-9||p[0]>L+1e-3)return null;last=p[0];}
 for(const f of g.fins||[])if(!f||!(f.count>=1&&f.count<=16)||!Array.isArray(f.planform)||f.planform.length>64||!f.planform.every(finiteAll)||!Number.isFinite(f.x_m))return null;
 return g;
}
export function genericVehicleGeometry(){
 const len=3,r=.075,nose=.6,rho=(r*r+nose*nose)/(2*r),profile=[];
 for(let i=0;i<=16;i++){const x=nose*i/16;profile.push([x,Math.max(0,Math.sqrt(Math.max(0,rho*rho-(nose-x)**2))+r-rho)]);}profile.push([len,r]);
 return {schema:VEHICLE_GEOMETRY_SCHEMA,version:VEHICLE_GEOMETRY_VERSION,source:{kind:'generic'},length_m:len,diameter_m:2*r,span_m:2*(r+.15),profile,
  sections:[{name:'Nose',role:'nose',kind:'nosecone',x0_m:0,x1_m:nose},{name:'Body',role:'airframe',kind:'bodytube',x0_m:nose,x1_m:len}],
  fins:[{name:'Fins',count:4,x_m:len-.32,root_radius_m:r,root_chord_m:.3,span_m:.15,thickness_m:.006,cant_deg:0,angle_offset_deg:45,planform:[[0,0],[.15,.15],[.27,.15],[.3,0]]}],
  internals:[],tanks:[],recovery:[{name:'Parachute',role:'main',diameter_m:1.5,line_length_m:2,separation_x_m:1.4}],separations:[{x_m:1.4}],rail_guides:[],
  nozzle:{x_m:len,exit_radius_m:.05,throat_radius_m:.022,length_m:.11,estimated:true},
  parts:[{name:'airframe-join',x0_m:1.35,x1_m:1.45}]};
}
const MATERIAL_COLORS=[[/carbon/i,0x2a2e33],[/alumin|steel|titanium|metal/i,0xb9c1c9],[/glass/i,0xe9e5dc],[/cardboard|paper|wood|ply/i,0xc9a878]];
function sectionColor(s){if(typeof s?.color==='string'&&/^#[0-9a-f]{6}$/i.test(s.color))return parseInt(s.color.slice(1),16);for(const [re,c] of MATERIAL_COLORS)if(re.test(s?.material||''))return c;return 0xcbd5e1;}
function radiusAt(profile,x){for(let i=1;i<profile.length;i++){const [a,ra]=profile[i-1],[b,rb]=profile[i];if(x>=a&&x<=b)return b-a>1e-12?ra+(rb-ra)*(x-a)/(b-a):Math.max(ra,rb);}return profile[profile.length-1][1];}
function clipProfile(profile,x0,x1){const out=[[x0,radiusAt(profile,x0)]];for(const p of profile)if(p[0]>x0&&p[0]<x1)out.push(p);out.push([x1,radiusAt(profile,x1)]);return out;}
// Builds the vehicle nose up (+Y), its axis on Y, centred on its length.
// Node names follow the bundled vehicle.glb, which motions and clips address:
// stage-1, nose, airframe-upper/-join/-lower, gimbal-pivot, engine, motor-flame,
// fin-pivot-N > fin, drogue-parachute, main-parachute.
export function buildVehicle(T,value,options={}){
 const g=vehicleGeometry(value)||genericVehicleGeometry(),opacity=options.opacity??1,segments=options.segments||32;
 const L=g.length_m,y=x=>L/2-x,profile=g.profile;
 const material=(color,o=opacity,metal=.15,rough=.55)=>new T.MeshStandardMaterial({color,roughness:rough,metalness:metal,transparent:o<1,opacity:o,depthWrite:o===1,side:T.DoubleSide});
 const lathe=(points,mat,name)=>{const v=points.map(([x,r])=>new T.Vector2(Math.max(r,1e-4),y(x))).reverse();const m=new T.Mesh(new T.LatheGeometry(v,segments),mat);m.name=name;return m;};
 const root=new T.Group();root.name='stage-1';root.userData={geometry:g,length_m:L};
 const parts=new Map((g.parts||[]).map(p=>[p.name,p]));
 const sections=Array.isArray(g.sections)&&g.sections.length?g.sections:[{name:'Body',role:'airframe',x0_m:0,x1_m:L}];
 const nose=sections.find(s=>s.role==='nose'),noseEnd=nose?nose.x1_m:0;
 const split=g.separations?.[0]?.x_m??L/2;
 const upper=new T.Group(),lower=new T.Group();upper.name='airframe-upper';lower.name='airframe-lower';
 for(const s of sections){
  if(s===nose){const m=lathe(clipProfile(profile,s.x0_m,s.x1_m),material(sectionColor(s)),'nose');m.userData={section:s.name,role:s.role};root.add(m);continue;}
  for(const [owner,a,b] of [[upper,s.x0_m,Math.min(s.x1_m,split)],[lower,Math.max(s.x0_m,split),s.x1_m]]){
   if(b-a<1e-6)continue;const m=lathe(clipProfile(profile,a,b),material(sectionColor(s)),s.name);m.userData={section:s.name,role:s.role};owner.add(m);
  }
 }
 root.add(upper,lower);
 const joinPart=parts.get('airframe-join'),j0=Math.max(noseEnd,Math.min(split-.004,joinPart?.x0_m??split)),j1=Math.min(L,Math.max(split+.004,joinPart?.x1_m??split));
 // The joint: a thin seam band at the separation (the coupler itself is inside).
 const jr=radiusAt(profile,split)*1.006,join=new T.Mesh(new T.CylinderGeometry(jr,jr,.012,segments,1,true),material(0x11161b,opacity,.2,.4));join.name='airframe-join';join.position.y=y(split);join.userData={x0_m:j0,x1_m:j1};root.add(join);
 for(const f of (g.fins||[]).slice(0,4)){
  const shape=new T.Shape(f.planform.map(([u,v])=>new T.Vector2(v,-u)));const t=Math.max(f.thickness_m||.004,.001);
  const rounded=f.cross_section==='rounded'||f.cross_section==='airfoil';
  const geom=new T.ExtrudeGeometry(shape,{depth:rounded?t*.4:t,bevelEnabled:rounded,bevelThickness:t*.3,bevelSize:Math.min(t*.3,.003),bevelSegments:2});geom.translate(0,0,-(rounded?t*.2:t/2));
  const chord=f.root_chord_m||Math.max(...f.planform.map(p=>p[0])),mat=material(sectionColor(f));
  for(let k=0;k<f.count;k++){
   const a=(f.angle_offset_deg||0)*Math.PI/180+k*2*Math.PI/f.count,pivot=new T.Group();pivot.name='fin-pivot-'+(root.children.filter(n=>n.name.startsWith('fin-pivot-')).length);
   pivot.position.set((f.root_radius_m||radiusAt(profile,f.x_m))*Math.cos(a),y(f.x_m+chord/2),(f.root_radius_m||radiusAt(profile,f.x_m))*Math.sin(a));pivot.rotation.y=-a;
   const fin=new T.Mesh(k?geom.clone():geom,k?mat.clone():mat);fin.name='fin';fin.position.y=chord/2;fin.rotation.x=(f.cant_deg||0)*Math.PI/180;pivot.add(fin);root.add(pivot);
  }
 }
 for(const [i,rb] of (g.rail_guides||[]).slice(0,4).entries()){
  const a=Math.PI/((g.fins?.[0]?.count)||3),r=radiusAt(profile,rb.x_m),m=new T.Mesh(new T.BoxGeometry(.016,.022,.016),material(0x1f2933));m.name='rail-button';m.userData={index:i};m.position.set((r+.006)*Math.cos(a),y(rb.x_m),(r+.006)*Math.sin(a));root.add(m);
 }
 const n=g.nozzle||{x_m:L,exit_radius_m:radiusAt(profile,L)*.7,throat_radius_m:radiusAt(profile,L)*.3,length_m:radiusAt(profile,L)*1.5};
 const throatX=n.x_m-(n.length_m||.1),gimbal=new T.Group();gimbal.name='gimbal-pivot';gimbal.position.y=y(throatX);root.add(gimbal);
 const nozzle=new T.Mesh(new T.LatheGeometry([new T.Vector2(n.exit_radius_m,-(n.length_m||.1)),new T.Vector2(n.throat_radius_m||n.exit_radius_m*.45,0),new T.Vector2((n.throat_radius_m||n.exit_radius_m*.45)*1.3,(n.length_m||.1)*.35)],segments),material(0x3b434b,opacity,.7,.35));nozzle.name='engine';gimbal.add(nozzle);
 const flameLength=Math.max(.6,L*.25),flame=new T.Mesh(new T.ConeGeometry(n.exit_radius_m*.95,flameLength,16,1,true),new T.MeshStandardMaterial({color:0xff7a26,emissive:0xff6a10,emissiveIntensity:.8,roughness:.8,metalness:0,transparent:true,opacity:.85,depthWrite:false,side:T.DoubleSide}));
 flame.name='motor-flame';flame.rotation.x=Math.PI;flame.position.y=-(n.length_m||.1)-flameLength/2;gimbal.add(flame);
 if(options.internals){
  const inner=new T.Group();inner.name='internals';root.add(inner);
  for(const i of (g.internals||[]).filter(i=>['motor_mount','tank'].includes(i.role)).slice(0,8)){const m=new T.Mesh(new T.CylinderGeometry(i.outer_radius_m,i.outer_radius_m,i.x1_m-i.x0_m,20),material(i.role==='tank'?0x9fd8f5:0x8a96a3,1,.5,.4));m.name=i.role==='tank'?'InternalTank':'MotorMount';m.position.y=y((i.x0_m+i.x1_m)/2);inner.add(m);}
  for(const t of (g.tanks||[]).filter(t=>t.external).slice(0,4)){const m=new T.Mesh(new T.CylinderGeometry(t.radius_m*.96,t.radius_m*.96,t.x1_m-t.x0_m,20),material(0x9fd8f5,1,.3,.4));m.name='InternalTank';m.position.y=y((t.x0_m+t.x1_m)/2);inner.add(m);}
 }
 const group=new T.Group();group.name='vehicle';group.add(root);
 if(options.parachutes!==false){
  for(const role of ['drogue','main']){
   const r=(g.recovery||[]).find(d=>d.role===role);if(!r)continue;
   const chute=new T.Group();chute.name=role+'-parachute';chute.position.y=y(r.separation_x_m??split);
   const radius=Math.max(.2,(r.diameter_m||1)/2),lift=Math.max(.5,(r.line_length_m||radius*2.5)*.9);
   const canopy=new T.Mesh(new T.SphereGeometry(radius,20,6,0,Math.PI*2,0,Math.PI*.42),new T.MeshStandardMaterial({color:role==='main'?0xe8eef3:0xf59e0b,roughness:.8,metalness:0,side:T.DoubleSide}));
   canopy.name=role+'-canopy';canopy.scale.y=.55;canopy.position.y=lift;chute.add(canopy);
   const rim=radius*Math.sin(Math.PI*.42),rimY=lift+radius*Math.cos(Math.PI*.42)*.55;
   for(let k=0;k<8;k++){const a=k*Math.PI/4,end=new T.Vector3(rim*Math.cos(a),rimY,rim*Math.sin(a)),len=end.length(),line=new T.Mesh(new T.CylinderGeometry(.006,.006,len,4),new T.MeshStandardMaterial({color:0x8b98a5}));line.name=role+'-line';line.position.copy(end).multiplyScalar(.5);line.quaternion.setFromUnitVectors(new T.Vector3(0,1,0),end.clone().normalize());chute.add(line);}
   group.add(chute);
  }
 }
 return group;
}

export function createReplayAssembly(T,mode='flight',geometry=null){
 const group=new T.Group();group.name=mode==='test_fire'?'ReplayMotorStand':'ReplayFlightVehicle';
 const moving=new T.Group();moving.name='RecordedAttitude';group.add(moving);
 const material=(color,opacity=1)=>new T.MeshStandardMaterial({color,roughness:.7,metalness:.12,transparent:opacity<1,opacity,depthWrite:opacity===1});
 const solid=(owner,name,geometry,color,position,rotation,opacity=1)=>{
  const mesh=new T.Mesh(geometry,material(color,opacity));mesh.name=name;mesh.position.set(...position);if(rotation)mesh.rotation.set(...rotation);owner.add(mesh);return mesh;
 };
 const box=(owner,name,size,color,position)=>solid(owner,name,new T.BoxGeometry(...size),color,position);
 const cylinder=(owner,name,radius,height,color,position,rotation,opacity=1)=>solid(owner,name,new T.CylinderGeometry(radius,radius,height,16),color,position,rotation,opacity);
 const vehicle=vehicleGeometry(geometry)||genericVehicleGeometry();group.userData.vehicleSource=vehicle.source?.kind||'generic';
 if(mode==='test_fire'){
  // The motor alone on the stand (its tank and chamber from the vehicle geometry), the rocket apart.
  const tankRow=(vehicle.tanks||[])[0],mount=(vehicle.internals||[]).find(i=>i.role==='motor_mount');
  const pieces=[tankRow&&{name:'StandOxidizerTank',x0:tankRow.x0_m,x1:tankRow.x1_m,r:tankRow.radius_m,color:0xcbd5e1},mount&&{name:'StandChamber',x0:mount.x0_m,x1:mount.x1_m,r:mount.outer_radius_m,color:0x94a3b8}].filter(Boolean);
  if(!pieces.length)pieces.push({name:'StandChamber',x0:0,x1:1.6,r:.08,color:0xcbd5e1});
  const x0=Math.min(...pieces.map(p=>p.x0)),x1=Math.max(...pieces.map(p=>p.x1)),span=x1-x0,mid=(x0+x1)/2,height=Math.max(...pieces.map(p=>p.r));
  const motor=new T.Group();motor.name='HorizontalMotor';motor.rotation.z=Math.PI/2;motor.position.set(0,.1,0);group.add(motor);
  for(const p of pieces)cylinder(motor,p.name,p.r,p.x1-p.x0,p.color,[0,mid-(p.x0+p.x1)/2,0]);
  const nozzleRow=vehicle.nozzle||{exit_radius_m:height*.7,length_m:height*1.4};
  solid(motor,'MotorNozzle',new T.CylinderGeometry(nozzleRow.throat_radius_m||nozzleRow.exit_radius_m*.45,nozzleRow.exit_radius_m,nozzleRow.length_m||.1,16),0x475569,[0,-span/2-(nozzleRow.length_m||.1)/2,0]);
  const base=span+1;box(group,'StandBase',[base,.15,1.4],0x64748b,[0,-1.5,0]);
  const cradle=.1-height-.03,baseTop=-1.425,column=cradle-.03-baseTop;
  for(const x of [-span*.35,span*.35]){box(group,'StandColumn',[.15,column,.15],0x94a3b8,[x,baseTop+column/2,0]);box(group,'MotorCradle',[.22,.06,height*4],0x64748b,[x,cradle,0]);}
  box(group,'InstrumentRail',[span*.9,.07,.1],0x38bdf8,[0,-.5,.65]);
  // The airframe apart from the stand, lying beside it, translucent.
  const apart=buildVehicle(T,vehicle,{opacity:.3,parachutes:false,segments:20});apart.name='AirframeReference';apart.rotation.z=Math.PI/2;apart.position.set(0,-1.5+.075+vehicle.diameter_m/2+(vehicle.span_m-vehicle.diameter_m)/2,-1.6);
  apart.getObjectByName('motor-flame').visible=false;group.add(apart);
 }else{
  const rocket=buildVehicle(T,vehicle,{opacity:.4,internals:true,parachutes:false,segments:24});rocket.name='FlightVehicle';
  rocket.getObjectByName('nose').name='Nose';rocket.getObjectByName('motor-flame').visible=false;moving.add(rocket);
 }
 const valves=[],tanks=[];
 function setLabel(owner,text){
  if(owner.labelText===text||typeof document==='undefined')return;
  if(!owner.labelCanvas){
   const canvas=document.createElement('canvas');canvas.width=384;canvas.height=64;
   const context=canvas.getContext('2d');if(!context)return;
   const texture=new T.CanvasTexture(canvas),sprite=new T.Sprite(new T.SpriteMaterial({map:texture,transparent:true,depthTest:false}));
   sprite.scale.set(1.3,.217,1);sprite.position.set(0,.3,.25);sprite.renderOrder=10;owner.part.add(sprite);
   Object.assign(owner,{labelCanvas:canvas,labelContext:context,labelTexture:texture});
  }
  const ctx=owner.labelContext;ctx.clearRect(0,0,384,64);ctx.fillStyle='#142534ed';ctx.fillRect(0,0,384,64);ctx.font='600 24px system-ui';ctx.fillStyle='#e4eff8';ctx.fillText(text.slice(0,27),8,41,368);owner.labelTexture.needsUpdate=true;owner.labelText=text;
 }
 function setRows(state){
  const valveRows=Array.isArray(state.valves)?state.valves.slice(0,REPLAY_VALVE_LIMIT):[];
  const tankRows=(Array.isArray(state.components)?state.components:[]).filter(c=>c&&c.kind==='tank').slice(0,REPLAY_TANK_LIMIT);
  while(valves.length<valveRows.length){
   const part=new T.Group(),index=valves.length;part.name='ValveIndicator'+index;
   part.position.set(mode==='test_fire'?-1.4+(index%4)*.9:1.05+(index%2)*.8,mode==='test_fire'?-1.05-Math.floor(index/4)*.4:1.5-Math.floor(index/2)*.47,.8);
   cylinder(part,'ValveBody',.13,.2,0x94a3b8,[0,0,0],[Math.PI/2,0,0]);
   const handle=box(part,'ValveHandle',[.42,.07,.07],0x64748b,[0,.05,.15]);group.add(part);valves.push({part,handle});
  }
  while(tanks.length<tankRows.length){
   const part=new T.Group(),index=tanks.length;part.name='SavedTankIndicator'+index;
   part.position.set(mode==='test_fire'?-2.2-index*.65:-.95-index*.65,mode==='test_fire'?-.55:.1,0);
   cylinder(part,'TankShell',.24,1.45,0xcbd5e1,[0,0,0],null,.22);
   const fluid=cylinder(part,'ConfiguredFillRange',.22,1.4,0x38bdf8,[0,-.7,0]);
   const unknown=box(part,'UnknownTankLevel',[.3,.07,.3],0x64748b,[0,0,0]);group.add(part);tanks.push({part,fluid,unknown});
  }
  for(let i=0;i<valves.length;i++){
   const row=valveRows[i],v=valves[i];v.part.visible=!!row;if(!row)continue;
   const status=replayValveState(row.value,row);v.part.userData={label:String(row.label||row.id||'Valve').slice(0,128),state:status};
   setLabel(v,`${v.part.userData.label}: ${typeof row.state==='string'?row.state:status}`);
   v.handle.rotation.z=status==='open'?Math.PI/2:status==='closed'?0:Math.PI/4;
   v.handle.material.color.setHex(status==='open'?0x22c55e:status==='closed'?0xf59e0b:0x64748b);
  }
  for(let i=0;i<tanks.length;i++){
   const row=tankRows[i],t=tanks[i];t.part.visible=!!row;if(!row)continue;
   const fraction=configuredTankFraction(row);t.part.userData={label:String(row.label||row.id||'Tank').slice(0,128),fraction};
   setLabel(t,`${t.part.userData.label}: ${fraction===null?'fill unknown':['kg','g'].includes(String(row.unit).trim().toLowerCase())?'configured mass':'configured fill'}`);
   t.fluid.visible=fraction!==null&&fraction>0;t.unknown.visible=fraction===null;
   if(fraction!==null){t.fluid.scale.y=Math.max(.0001,fraction);t.fluid.position.y=-.7+.7*fraction;}
  }
  const attitude=state.attitude;
  const known=Array.isArray(attitude)&&attitude.length===3&&attitude.every(Number.isFinite);
  moving.rotation.set(...(known&&mode!=='test_fire'?attitude.map(v=>T.MathUtils.degToRad(v)):[0,0,0]));
  group.userData={vehicleSource:group.userData.vehicleSource,mode:mode==='test_fire'?'test_fire':'flight',attitudeKnown:known,omittedValves:Math.max(0,(state.valves?.length||0)-REPLAY_VALVE_LIMIT),omittedTanks:Math.max(0,(Array.isArray(state.components)?state.components:[]).filter(c=>c&&c.kind==='tank').length-REPLAY_TANK_LIMIT)};
 }
 function dispose(){group.traverse(n=>{n.geometry?.dispose();for(const m of Array.isArray(n.material)?n.material:n.material?[n.material]:[]){m.map?.dispose();m.dispose();}});}
 return {group,valves,tanks,update:setRows,dispose};
}

// Standalone replay and the configured viewer share the same assembly. This
// small camera owns its listeners/RAF and redraws only after a seek or gesture.
export function mountReplayCanvas(T,SoftwareRenderer,container,mode,onStatus=()=>{},geometry=null){
 let renderer,software=false;
 try{renderer=new T.WebGLRenderer({alpha:false,antialias:false});}catch{renderer=new SoftwareRenderer();software=true;}
 renderer.setPixelRatio(Math.min(globalThis.devicePixelRatio||1,1.5));
 const canvas=renderer.domElement;canvas.style.cssText='display:block;width:100%;height:100%;touch-action:none;';canvas.tabIndex=0;canvas.dataset.scene=mode;canvas.setAttribute('aria-label','Drag to orbit the illustrative saved '+(mode==='test_fire'?'motor stand':'flight vehicle'));container.append(canvas);
 const scene=new T.Scene();scene.background=new T.Color(0x0f172a);scene.add(new T.HemisphereLight(0xffffff,0x334155,2.8));
 const light=new T.DirectionalLight(0xffffff,3);light.position.set(4,7,6);scene.add(light);
 const assembly=createReplayAssembly(T,mode,geometry);scene.add(assembly.group);
 const camera=new T.PerspectiveCamera(45,1,.05,100),target=new T.Vector3(0,0,0);
 let theta=.65,phi=1.15,distance=mode==='test_fire'?11:10,dirty=true,disposed=false,raf=0,drag=null,width=0,height=0,contextLost=false;
 const listeners=[];const listen=(target,name,fn,options)=>{target.addEventListener(name,fn,options);listeners.push(()=>target.removeEventListener(name,fn,options));};
 function redraw(){if(disposed||contextLost||!dirty||!container.isConnected||document.hidden)return;const w=Math.max(1,container.clientWidth),h=Math.max(1,container.clientHeight);if(w!==width||h!==height){width=w;height=h;renderer.setSize(w,h);camera.aspect=w/h;camera.updateProjectionMatrix();}camera.position.set(target.x+distance*Math.sin(phi)*Math.sin(theta),target.y+distance*Math.cos(phi),target.z+distance*Math.sin(phi)*Math.cos(theta));camera.lookAt(target);renderer.render(scene,camera);canvas.dataset.rendered='true';dirty=false;}
 function request(){dirty=true;canvas.dataset.rendered='false';if(!raf)raf=requestAnimationFrame(()=>{raf=0;redraw();});}
 listen(canvas,'pointerdown',e=>{drag={id:e.pointerId,x:e.clientX,y:e.clientY};canvas.setPointerCapture?.(e.pointerId);});
 listen(canvas,'pointermove',e=>{if(drag?.id!==e.pointerId)return;theta-=(e.clientX-drag.x)*.007;phi=Math.max(.15,Math.min(Math.PI-.15,phi+(e.clientY-drag.y)*.007));drag={id:e.pointerId,x:e.clientX,y:e.clientY};request();});
 listen(canvas,'pointerup',()=>{drag=null;});listen(canvas,'pointercancel',()=>{drag=null;});
 listen(canvas,'wheel',e=>{e.preventDefault();distance=Math.max(3,Math.min(30,distance*Math.exp(e.deltaY*.001)));request();},{passive:false});
 listen(canvas,'keydown',e=>{if(['ArrowLeft','ArrowRight','ArrowUp','ArrowDown','+','-','r','R'].includes(e.key)){e.preventDefault();if(e.key==='ArrowLeft')theta-=.15;if(e.key==='ArrowRight')theta+=.15;if(e.key==='ArrowUp')phi=Math.max(.15,phi-.15);if(e.key==='ArrowDown')phi=Math.min(Math.PI-.15,phi+.15);if(e.key==='+')distance=Math.max(3,distance/1.2);if(e.key==='-')distance=Math.min(30,distance*1.2);if(e.key.toLowerCase()==='r')reset();request();}});
 listen(document,'visibilitychange',request);
 listen(canvas,'webglcontextlost',e=>{e.preventDefault();contextLost=true;canvas.dataset.rendered='false';canvas.style.opacity='.25';onStatus('lost');});
 listen(canvas,'webglcontextrestored',()=>{contextLost=false;canvas.style.opacity='1';onStatus('ready');request();});
 const resize=new ResizeObserver(request);resize.observe(container);
 function reset(){theta=.65;phi=1.15;distance=mode==='test_fire'?11:10;request();}
 request();
 return {assembly,canvas,software,update:state=>{assembly.update(state);request();},reset,dispose:()=>{disposed=true;cancelAnimationFrame(raf);resize.disconnect();for(const remove of listeners)remove();assembly.dispose();renderer.dispose();renderer.forceContextLoss?.();canvas.remove();}};
}
