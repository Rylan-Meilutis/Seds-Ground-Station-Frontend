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
  // The plume out of the stand nozzle, its length set by the recorded thrust or chamber pressure.
  const plumeLength=Math.max(1.2,span*.9),plume=new T.Group();plume.name='StandPlume';plume.position.y=-span/2-(nozzleRow.length_m||.1);motor.add(plume);
  const plumeGeometry=new T.ConeGeometry(nozzleRow.exit_radius_m*.95,1,16,1,true);plumeGeometry.rotateX(Math.PI);plumeGeometry.translate(0,-.5,0);
  const flameMesh=new T.Mesh(plumeGeometry,new T.MeshBasicMaterial({color:0xffb347,transparent:true,opacity:.9,depthWrite:false,blending:T.AdditiveBlending,side:T.DoubleSide}));flameMesh.name='StandPlumeCore';flameMesh.scale.y=plumeLength;plume.add(flameMesh);plume.visible=false;
  group.userData.plume={node:plume,core:flameMesh,length:plumeLength};
 }else{
  const rocket=buildVehicle(T,vehicle,{opacity:.4,internals:true,parachutes:true,segments:24});rocket.name='FlightVehicle';
  rocket.getObjectByName('nose').name='Nose';const flame=rocket.getObjectByName('motor-flame');flame.visible=false;moving.add(rocket);
  for(const role of ['drogue','main']){const chute=rocket.getObjectByName(role+'-parachute');if(chute)chute.visible=false;}
  group.userData.plume={node:flame,core:flame,length:1,flight:true,baseY:flame.position.y,flameLength:flame.geometry.parameters.height};
 }
 const plumeState=group.userData.plume,flightRocket=group.getObjectByName('FlightVehicle');
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
  // Flight state on the model: plume (0..1.25 of nominal), chutes deployed.
  const flight=state.flight&&typeof state.flight==='object'?state.flight:{};const plume=Number.isFinite(flight.plume)?Math.max(0,flight.plume):0;
  plumeState.node.visible=plume>0;
  if(plume>0){if(plumeState.flight){plumeState.core.scale.y=plume;plumeState.core.position.y=plumeState.baseY-(plume-1)*plumeState.flameLength/2;}else plumeState.core.scale.y=plumeState.length*plume;}
  for(const role of ['drogue','main']){const chute=flightRocket?.getObjectByName(role+'-parachute');if(chute)chute.visible=flight[role]===true;}
  group.userData={vehicleSource:group.userData.vehicleSource,plume:plumeState,flightState:{plume,drogue:flight.drogue===true,main:flight.main===true,phase:typeof flight.phase==='string'?flight.phase:null},mode:mode==='test_fire'?'test_fire':'flight',attitudeKnown:known,omittedValves:Math.max(0,(state.valves?.length||0)-REPLAY_VALVE_LIMIT),omittedTanks:Math.max(0,(Array.isArray(state.components)?state.components:[]).filter(c=>c&&c.kind==='tank').length-REPLAY_TANK_LIMIT)};
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

// ---------------------------------------------------------------------------
// Flight replay engine: flight state on the model, the track by phase, terrain
// placement, overlays and camera modes. The vocabulary and the palette are the
// shared flight-plan contract (docs/recorded-vehicle-replay.md, "Flight state").
// The vehicle always comes from the station geometry; nothing here names one.
export const FLIGHT_PHASES=['pad','boost','coast','drogue','main','landed'];
export const PHASE_COLORS={pad:'#94a3b8',boost:'#f97316',coast:'#facc15',apogee:'#ffffff',drogue:'#38bdf8',main:'#22c55e',landed:'#a78bfa',unknown:'#64748b'};
// FLIGHT_STATE numbers (network values plus the FC's detailed values).
export const FLIGHT_STATE_NAMES=['Startup','Idle','PreFill','FillTest','NitrogenFill','NitrousFill','Armed','Launch','Ascent','Coast','Apogee','Descent','Reefing','Landed','Recovery','Aborted','InFlight'];
const STATE_PHASE={startup:'pad',idle:'pad',prefill:'pad',fill:'pad',filltest:'pad',nitrogenfill:'pad',nitrousfill:'pad',armed:'pad',launch:'boost',ascent:'boost',coast:'coast',apogee:'coast',descent:'drogue',parachutedeploy:'drogue',reefing:'main',landed:'landed',recovery:'landed'};
// The phase of a station flight state (a name or its number), given the phase before it.
export function flightPhaseOf(state,previous='pad'){
 const name=typeof state==='number'?FLIGHT_STATE_NAMES[state]:state;
 const key=String(name??'').replace(/[^a-z]/gi,'').toLowerCase();
 if(key==='aborted')return previous;
 if(key==='inflight')return ['boost','coast','drogue','main'].includes(previous)?previous:'boost';
 return STATE_PHASE[key]||previous;
}
const clamp=(v,a,b)=>Math.max(a,Math.min(b,v));
// Attitude from a velocity (local frame, m/s): the quaternion [x,y,z,w] that
// turns the vehicle axis (+Y) onto the direction of travel, or null when slower than minSpeed.
export function attitudeFromVelocity(v,minSpeed=3){
 if(!Array.isArray(v)||v.length<3||!v.slice(0,3).every(Number.isFinite))return null;
 const speed=Math.hypot(v[0],v[1],v[2]);if(!(speed>=minSpeed))return null;
 const [x,y,z]=[v[0]/speed,v[1]/speed,v[2]/speed];
 let q=y<-0.999999?[1,0,0,0]:[z,0,-x,1+y];const n=Math.hypot(...q);q=q.map(c=>c/n);
 return {quaternion:q,pitch_deg:Math.asin(clamp(y,-1,1))*180/Math.PI,heading_deg:(Math.atan2(x,-z)*180/Math.PI+360)%360,speed_mps:speed};
}
// A track sampled at times (ms) with local positions [x,y,z]: the interpolated
// position at t, and a velocity that blends the segment velocities around t.
export function trackSampler(times,positions,breaks=[],windowMs=2500){
 const n=Math.min(times.length,positions.length),cut=new Set(breaks);
 const index=t=>{let lo=0,hi=n;while(lo<hi){const m=(lo+hi)>>>1;if(times[m]<=t)lo=m+1;else hi=m;}return lo-1;};
 const segmentVelocity=i=>{if(i<0||i+1>=n||cut.has(i+1))return null;const dt=(times[i+1]-times[i])/1000;if(!(dt>0))return null;return [0,1,2].map(k=>(positions[i+1][k]-positions[i][k])/dt);};
 function position(t){
  if(!n||!Number.isFinite(t))return null;const i=index(t);if(i<0)return null;
  if(i+1>=n||cut.has(i+1))return positions[i].slice();
  const f=clamp((t-times[i])/Math.max(1,times[i+1]-times[i]),0,1);return [0,1,2].map(k=>positions[i][k]+(positions[i+1][k]-positions[i][k])*f);
 }
 function velocity(t){
  if(n<2||!Number.isFinite(t))return null;const i=index(t);if(i<0)return null;
  // A central difference over a window damps single bad fixes; a break or the
  // track's end falls back to the segment's own velocity.
  const ta=Math.max(times[0],t-windowMs),tb=Math.min(times[n-1],t+windowMs),ia=index(ta),ib=index(tb);
  let crossesBreak=false;for(let j=ia+1;j<=ib;j++)if(cut.has(j)){crossesBreak=true;break;}
  if(!crossesBreak&&tb-ta>=200){const a=position(ta),b=position(tb);if(a&&b)return [0,1,2].map(k=>(b[k]-a[k])/((tb-ta)/1000));}
  return segmentVelocity(i)||segmentVelocity(i-1);
 }
 return {position,velocity,count:n};
}
// Plume length as a fraction of its nominal length: from the recorded thrust,
// else the chamber pressure, each against its own recorded maximum; at the phase
// alone, nominal. Zero outside powered flight or the burn.
export function plumeScale({phase,burning,thrust,thrustMax,chamber,chamberMax}={}){
 const lit=burning===true||(burning!==false&&phase==='boost');if(!lit)return {scale:0,source:'none'};
 for(const [value,max,source] of [[thrust,thrustMax,'thrust'],[chamber,chamberMax,'chamber']]){
  if(Number.isFinite(value)&&Number.isFinite(max)&&max>0){const f=value/max;return {scale:f<.03?0:clamp(f,.15,1.25),source};}
 }
 return {scale:phase==='boost'||burning===true?1:0,source:'phase'};
}
// Parachutes at their real sizes, from the station geometry (`recovery`).
export function chuteSizes(value){
 const g=vehicleGeometry(value)||genericVehicleGeometry(),split=g.separations?.[0]?.x_m??g.length_m/2,out={drogue:null,main:null};
 for(const role of ['drogue','main']){
  const r=(g.recovery||[]).find(d=>d.role===role);if(!r||!(r.diameter_m>0))continue;
  out[role]={name:r.name||role,diameter_m:r.diameter_m,radius_m:r.diameter_m/2,line_length_m:r.line_length_m>0?r.line_length_m:r.diameter_m*1.25,line_count:r.line_count||8,separation_x_m:r.separation_x_m??split,cd:Number.isFinite(r.cd)?r.cd:null,deploy_altitude_m:r.deploy_altitude_m??null};
 }
 return out;
}
// What the model shows for a phase: plume, chutes, the airframe parted at its joint.
export function phaseVisual(phase,{plume=0}={}){
 return {phase,plume:phase==='boost'?plume:0,drogue:['drogue','main','landed'].includes(phase),main:['main','landed'].includes(phase),separated:['drogue','main','landed'].includes(phase),landed:phase==='landed',upright:['pad','drogue','main'].includes(phase)};
}
export const CAMERA_MODES=['follow','chase','pad','station','orbit','top'];
export const CAMERA_LABELS={follow:'Follow',chase:'Chase',pad:'Pad view',station:'Ground station',orbit:'Free orbit',top:'Top-down'};
const add=(a,b,s=1)=>[a[0]+b[0]*s,a[1]+b[1]*s,a[2]+b[2]*s];
// Camera position and target for a mode. ctx: {rocket, direction (unit, or null),
// pad, station (or null), theta, phi, distance, target (free orbit)}.
export function cameraPose(mode,ctx){
 const rocket=ctx.rocket||[0,0,0],d=Math.max(.5,ctx.distance||15),theta=ctx.theta??.6,phi=ctx.phi??1.25;
 const orbit=target=>add(target,[d*Math.sin(phi)*Math.sin(theta),d*Math.cos(phi),d*Math.sin(phi)*Math.cos(theta)]);
 let resolved=CAMERA_MODES.includes(mode)?mode:'follow';
 if(resolved==='station'&&!ctx.station)resolved='pad';
 if(resolved==='chase'){
  let dir=ctx.direction&&ctx.direction.every(Number.isFinite)&&Math.hypot(...ctx.direction)>.5?ctx.direction:[0,1,0];
  const n=Math.hypot(...dir);dir=dir.map(v=>v/n);
  // Behind and a little above; a vertical path is offset sideways so the vehicle stays in view.
  const side=Math.abs(dir[1])>.95?[Math.sin(theta),0,Math.cos(theta)]:[0,1,0];
  return {mode:resolved,position:add(add(rocket,dir,-d),side,d*.3),target:add(rocket,dir,d*.15),up:[0,1,0]};
 }
 if(resolved==='pad'){const pad=ctx.pad||[0,0,0],back=Math.max(30,d*2);return {mode:resolved,position:add(pad,[back*Math.sin(theta),1.7,back*Math.cos(theta)]),target:rocket,up:[0,1,0]};}
 if(resolved==='station')return {mode:resolved,position:add(ctx.station,[0,2,0]),target:rocket,up:[0,1,0]};
 if(resolved==='top')return {mode:resolved,position:add(rocket,[0,Math.max(40,d*3),0]),target:rocket,up:[0,0,-1]};
 if(resolved==='orbit'){const target=ctx.target||rocket;return {mode:resolved,position:orbit(target),target,up:[0,1,0]};}
 return {mode:'follow',position:orbit(rocket),target:rocket,up:[0,1,0]};
}
// MapOverlay (the flight-plan contract) to local geometry. Points with a null
// altitude, markers and areas sit on the ground (heightAt); offset lifts the
// GPS-altitude points by the terrain correction at the pad.
export function overlayGeometry(overlays,origin,heightAt=()=>0,offset=0){
 const out={tracks:[],markers:[],areas:[]};if(!Array.isArray(overlays)||!validGps(origin))return out;
 const local=(lat,lon,alt)=>{const p=gpsToLocal([lat,lon,Number.isFinite(alt)?alt:origin[2]],origin);if(!p)return null;const ground=heightAt(p[0],p[2]);p[1]=Number.isFinite(alt)?Math.max(ground,p[1]+offset):ground;return p;};
 for(const overlay of overlays.slice(0,16)){
  if(!overlay||typeof overlay!=='object')continue;const predicted=overlay.predicted!==false,los=overlay.kind==='los_prediction';
  for(const track of (Array.isArray(overlay.tracks)?overlay.tracks:[]).slice(0,32)){
   const points=(Array.isArray(track?.points)?track.points:[]).slice(0,4096).map(p=>Array.isArray(p)?local(p[0],p[1],p[2]):null).filter(Boolean);
   if(points.length<2)continue;
   out.tracks.push({id:String(track.id??''),phase:track.phase||'unknown',label:String(track.label||overlay.label||''),overlay:String(overlay.id??''),color:los?'#f472b6':PHASE_COLORS[track.phase]||PHASE_COLORS.unknown,dashed:predicted||track.style==='predicted',opacity:predicted?.55:1,points});
  }
  for(const m of (Array.isArray(overlay.markers)?overlay.markers:[]).slice(0,32)){const p=m&&local(m.lat_deg,m.lon_deg,m.kind==='apogee'?m.alt_msl_m:null);if(p)out.markers.push({id:String(m.id??''),kind:m.kind||'event',label:String(m.label||''),color:los?'#f472b6':m.kind==='apogee'?PHASE_COLORS.apogee:m.kind==='landing'?PHASE_COLORS.landed:'#e2e8f0',position:p});}
  for(const a of (Array.isArray(overlay.areas)?overlay.areas:[]).slice(0,16)){
   const ring=(Array.isArray(a?.polygon)?a.polygon:[]).slice(0,72).map(p=>Array.isArray(p)?local(p[0],p[1],null):null).filter(Boolean);
   if(ring.length<3)continue;out.areas.push({id:String(a.id??''),kind:a.kind||'area',label:String(a.label||''),color:los?'#f472b6':a.kind==='landing_ellipse'?PHASE_COLORS.landed:a.kind==='search_swath'?'#22c55e':a.kind==='uncovered'?'#ef4444':'#a78bfa',fill:los?.15:a.kind==='landing_ellipse'?.12:a.kind==='search_swath'?.3:.25,ring});
  }
 }
 return out;
}
// The renderer's budget: the software renderer and small or weak devices get fewer effects.
export function renderQuality({software=false,cores=globalThis.navigator?.hardwareConcurrency,memory=globalThis.navigator?.deviceMemory,width=globalThis.innerWidth,forced}={}){
 if(forced==='low'||forced==='high')return forced;
 return software||(Number.isFinite(cores)&&cores<=4)||(Number.isFinite(memory)&&memory<=2)||(Number.isFinite(width)&&width<=480)?'low':'high';
}
function hexColor(T,hex){return new T.Color(hex);}
function lineSegmentsFrom(T,points,{color,dashed=false,opacity=1,colors=null,width=1}){
 const pos=[],col=[];for(let i=1;i<points.length;i++){pos.push(...points[i-1],...points[i]);if(colors)col.push(...colors[i-1],...colors[i]);}
 const g=new T.BufferGeometry();g.setAttribute('position',new T.Float32BufferAttribute(pos,3));if(colors)g.setAttribute('color',new T.Float32BufferAttribute(col,3));
 const options={color:colors?0xffffff:color,vertexColors:!!colors,transparent:opacity<1,opacity,linewidth:width,depthWrite:opacity===1};
 const m=dashed?new T.LineDashedMaterial({...options,dashSize:25,gapSize:15}):new T.LineBasicMaterial(options);
 const line=new T.LineSegments(g,m);if(dashed)line.computeLineDistances();return line;
}
// A text label that always faces the camera; size in metres at 1 km scales with distance in the scene loop.
export function makeLabel(T,text,{color='#e4eff8',background='#142534e6',border=null}={}){
 if(typeof document==='undefined')return null;
 const canvas=document.createElement('canvas'),ctx=canvas.getContext('2d');if(!ctx)return null;
 canvas.width=512;canvas.height=96;ctx.fillStyle=background;ctx.fillRect(0,0,512,96);if(border){ctx.strokeStyle=border;ctx.lineWidth=6;ctx.strokeRect(3,3,506,90);}
 ctx.font='600 44px system-ui,sans-serif';ctx.fillStyle=color;ctx.textBaseline='middle';ctx.fillText(String(text).slice(0,24),18,50,476);
 const sprite=new T.Sprite(new T.SpriteMaterial({map:new T.CanvasTexture(canvas),transparent:true,depthTest:false}));sprite.renderOrder=20;sprite.userData.label=String(text);sprite.userData.aspect=512/96;return sprite;
}
export function buildOverlayGroup(T,overlays,origin,heightAt,offset=0){
 const g=overlayGeometry(overlays,origin,heightAt,offset),group=new T.Group();group.name='ReplayOverlays';group.userData={tracks:g.tracks.length,markers:g.markers.length,areas:g.areas.length};
 for(const t of g.tracks){const line=lineSegmentsFrom(T,t.points,{color:hexColor(T,t.color),dashed:t.dashed,opacity:t.opacity});line.name='OverlayTrack';line.userData={phase:t.phase,dashed:t.dashed,opacity:t.opacity,overlay:t.overlay};group.add(line);}
 for(const a of g.areas){
  const ring=a.ring.map(p=>[p[0],p[1]+.6,p[2]]),outline=lineSegmentsFrom(T,[...ring,ring[0]],{color:hexColor(T,a.color),dashed:true,opacity:.9});outline.name='OverlayAreaOutline';outline.userData={kind:a.kind};group.add(outline);
  const c=ring.reduce((s,p)=>add(s,p,1/ring.length),[0,0,0]),pos=[];for(let i=0;i<ring.length;i++){const b=ring[(i+1)%ring.length];pos.push(...c,...ring[i],...b);}
  const geom=new T.BufferGeometry();geom.setAttribute('position',new T.Float32BufferAttribute(pos,3));geom.computeVertexNormals();
  const fill=new T.Mesh(geom,new T.MeshBasicMaterial({color:hexColor(T,a.color),transparent:true,opacity:a.fill,side:T.DoubleSide,depthWrite:false}));fill.name='OverlayAreaFill';fill.userData={kind:a.kind,overlayArea:true};group.add(fill);
 }
 for(const m of g.markers){
  const marker=new T.Mesh(new T.SphereGeometry(1,12,8),new T.MeshBasicMaterial({color:hexColor(T,m.color),transparent:true,opacity:.75}));marker.name='OverlayMarker';marker.position.set(...m.position);marker.userData={kind:m.kind,scaleWithDistance:.006};group.add(marker);
  const label=m.label&&makeLabel(T,m.label,{border:m.color});if(label){label.position.set(...m.position);label.userData.scaleWithDistance=.03;label.userData.lift=1.6;group.add(label);}
 }
 return group;
}
// The vehicle with flight state: plume, chutes at their real sizes, the airframe
// parted at its real joint. Built from the station geometry (buildVehicle).
export function createFlightVehicle(T,geometry,{quality='high'}={}){
 const g=vehicleGeometry(geometry)||genericVehicleGeometry(),L=g.length_m,low=quality==='low';
 const group=buildVehicle(T,g,{segments:low?16:28,parachutes:true}),stage=group.getObjectByName('stage-1');
 const split=g.separations?.[0]?.x_m??L/2,ySplit=L/2-split;
 // Fore (nose to the joint) and aft groups, so the joint can open.
 const fore=new T.Group(),aft=new T.Group();fore.name='flight-fore';aft.name='flight-aft';
 for(const child of [...stage.children]){(['nose','airframe-upper'].includes(child.name)?fore:aft).add(child);}
 stage.add(fore,aft);
 const sizes=chuteSizes(g),chutes={};
 for(const role of ['drogue','main']){const c=group.getObjectByName(role+'-parachute');if(c){c.visible=false;chutes[role]=c;c.userData.size=sizes[role];}}
 // The plume: a bright core and, on capable devices, a soft outer glow; length set per frame.
 const nozzle=g.nozzle||{exit_radius_m:.05,length_m:.1},flame=group.getObjectByName('motor-flame'),gimbal=group.getObjectByName('gimbal-pivot');
 if(flame){flame.parent.remove(flame);flame.geometry.dispose();flame.material.dispose();}
 const r=Math.max(.01,nozzle.exit_radius_m||.05),nominal=Math.max(L*.9,1.5),plume=new T.Group();plume.name='motor-flame';plume.position.y=-(nozzle.length_m||.1);gimbal.add(plume);
 const cone=(radius,opacity,color)=>{const geom=new T.ConeGeometry(radius,1,low?10:20,1,true);geom.rotateX(Math.PI);geom.translate(0,-.5,0);const m=new T.Mesh(geom,new T.MeshBasicMaterial({color,transparent:true,opacity,depthWrite:false,blending:T.AdditiveBlending,side:T.DoubleSide}));return m;};
 const core=cone(r*1.2,.95,0xffd27a);core.name='plume-core';plume.add(core);
 let glow=null;if(!low){glow=cone(r*2.8,.35,0xff6a10);glow.name='plume-glow';plume.add(glow);}
 plume.visible=false;
 const state={phase:null,plume:0,separated:false};
 function setState(v,t=0){
  const scale=Math.max(0,v.plume||0);plume.visible=scale>0;
  if(scale>0){const flicker=low?1:1+.06*Math.sin(t*.031)+.04*Math.sin(t*.017);core.scale.set(1,nominal*scale*flicker,1);if(glow)glow.scale.set(1,nominal*scale*1.35*flicker,1);}
  const gap=v.separated?Math.min(2,Math.max(.4,L*.25)):0;fore.position.y=gap;
  for(const role of ['drogue','main']){const c=chutes[role];if(!c)continue;c.visible=!!v[role]&&!v.landed;c.position.y=ySplit+gap*.5;}
  Object.assign(state,{phase:v.phase,plume:scale,separated:!!v.separated});group.userData.flightState={...state,drogue:!!chutes.drogue?.visible,main:!!chutes.main?.visible};
 }
 setState(phaseVisual('pad'));
 group.userData.length_m=L;group.userData.radius_m=g.diameter_m/2;group.userData.chutes=sizes;
 return {group,setState,chutes,plume,fore,aft,length_m:L,radius_m:g.diameter_m/2,nominalPlume_m:nominal};
}
// Curved ground at the pad's altitude (no terrain): the fallback when terrain.js is absent.
function fallbackGround(T,extent){
 const positions=[],rings=24,slices=64,R=EARTH_RADIUS_M;
 const point=(ring,i)=>{const r=ring===0?0:Math.exp(Math.log(5)+(Math.log(Math.min(R*.9,extent*40))-Math.log(5))*(ring-1)/(rings-1));const a=i/slices*Math.PI*2;return [r*Math.cos(a),Math.sqrt(R*R-r*r)-R-.05,r*Math.sin(a)];};
 for(let ring=0;ring<rings;ring++)for(let i=0;i<slices;i++){const a=point(ring,i),b=point(ring+1,i),c=point(ring+1,i+1),d=point(ring,i+1);positions.push(...a,...b,...c,...a,...c,...d);}
 const geometry=new T.BufferGeometry();geometry.setAttribute('position',new T.Float32BufferAttribute(positions,3));
 const ground=new T.Mesh(geometry,new T.MeshBasicMaterial({color:0x4a6b4f,side:T.DoubleSide}));ground.userData.referenceGround=true;ground.name='PadAltitudeGround';
 const group=new T.Group();group.add(ground);
 const grid=new T.GridHelper(Math.min(20000,Math.max(2000,extent*2)),Math.min(200,Math.max(20,Math.round(extent/50))),0x7f9c84,0x5d7a62);grid.position.y=0;grid.material.transparent=true;grid.material.opacity=.35;grid.name='ScaleGrid';group.add(grid);
 return {group,heightAt:(x,z)=>{const r2=x*x+z*z;return r2<R*R?Math.sqrt(R*R-r2)-R:0;},source:'pad-altitude',attribution:'',dispose(){group.traverse(n=>{n.geometry?.dispose();n.material?.dispose?.();});}};
}
export async function loadTerrainModule(loader){try{const m=await (loader?loader():import('./terrain.js'));return typeof m?.loadTerrain==='function'?m:null;}catch{return null;}}
// The 3D flight replay: the vehicle at its recorded position and attitude over the
// ground (terrain when terrain.js is present), its track coloured by phase, the pad,
// the stations and the overlays, with camera modes. The player drives it with
// setTrack / update(frame); it redraws only when something changed.
export function mountFlightScene(T,SoftwareRenderer,container,options={}){
 let renderer,software=false;
 try{renderer=new T.WebGLRenderer({alpha:false,antialias:false,logarithmicDepthBuffer:true,powerPreference:'low-power'});}catch{renderer=new SoftwareRenderer();software=true;}
 const quality=renderQuality({software,forced:options.quality});
 renderer.setPixelRatio(Math.min(globalThis.devicePixelRatio||1,quality==='low'?1:1.5));
 const canvas=renderer.domElement;canvas.style.cssText='display:block;width:100%;height:100%;touch-action:none;';canvas.tabIndex=0;canvas.dataset.scene='flight-replay';canvas.dataset.quality=quality;canvas.setAttribute('aria-label','Flight replay scene. Drag to orbit, scroll or pinch to zoom; keys 1 to 6 change the camera.');container.append(canvas);
 const scene=new T.Scene();scene.background=new T.Color(0x7aa3c2);scene.add(new T.HemisphereLight(0xffffff,0x3b4a3f,2.6));
 const sun=new T.DirectionalLight(0xffffff,2.6);sun.position.set(400,900,300);scene.add(sun);
 const vehicle=createFlightVehicle(T,options.geometry,{quality});scene.add(vehicle.group);
 // A locator for far views (pad, station, top-down): the model keeps its true size.
 const locator=new T.Mesh(new T.SphereGeometry(1,12,8),new T.MeshBasicMaterial({color:0xffffff,transparent:true,opacity:.85,depthTest:false}));locator.name='RocketLocator';locator.renderOrder=30;locator.userData.scaleWithDistance=.005;scene.add(locator);
 const world=new T.Group();world.name='FlightWorld';scene.add(world);
 const camera=new T.PerspectiveCamera(50,1,.05,4e6);
 const view={mode:'follow',theta:.75,phi:1.25,distance:Math.max(12,vehicle.length_m*3.2),target:new T.Vector3(),stationIndex:0};
 let origin=null,times=[],positions=[],sampler=null,track=null,trackColors=null,terrain=null,offset=0,stations=[],overlays=[],overlayGroup=null,pad=null,eventMarkers=null;
 let frame={t:NaN},rocket=[0,0,0],direction=null,dirty=true,disposed=false,raf=0,width=0,height=0,contextLost=false,terrainToken=0;
 const listeners=[],listen=(target,name,fn,opts)=>{target.addEventListener(name,fn,opts);listeners.push(()=>target.removeEventListener(name,fn,opts));};
 const heightAt=(x,z)=>{const h=terrain?.heightAt?.(x,z);return Number.isFinite(h)?h:0;};
 function clearWorld(){for(const child of [...world.children]){world.remove(child);child.traverse(n=>{n.geometry?.dispose();for(const m of Array.isArray(n.material)?n.material:n.material?[n.material]:[]){m.map?.dispose();m.dispose();}});}track=null;overlayGroup=null;pad=null;eventMarkers=null;}
 function phaseColorAt(t){const seg=(options.timeline?.segments||[]).find(s=>t>=s.from&&t<s.to)||(options.timeline?.segments||[]).at(-1);return hexColor(T,PHASE_COLORS[seg?.phase]||PHASE_COLORS.unknown);}
 function buildWorld(){
  clearWorld();if(!origin)return;
  const extent=Math.max(1500,...positions.map(p=>Math.hypot(p[0],p[2])*1.3));
  if(!terrain){terrain=fallbackGround(T,extent);terrain.fallback=true;}
  world.add(terrain.group);offset=heightAt(0,0);
  // Pad and rail at the first fix, on the ground.
  pad=new T.Group();pad.name='LaunchPad';pad.position.set(0,offset,0);
  const slab=new T.Mesh(new T.BoxGeometry(3,.15,3),new T.MeshStandardMaterial({color:0x8b8f94,roughness:.9}));slab.position.y=.075;pad.add(slab);
  const railLength=Math.max(options.railLength_m||0,vehicle.length_m*1.2),rail=new T.Mesh(new T.BoxGeometry(.04,railLength,.04),new T.MeshStandardMaterial({color:0x5b6570,metalness:.6,roughness:.4}));rail.name='LaunchRail';rail.position.set(vehicle.radius_m+.04,railLength/2+.15,0);pad.add(rail);world.add(pad);
  // The track, coloured by phase.
  if(positions.length>1){
   const pts=positions.map(p=>[p[0],Math.max(p[1]+offset,heightAt(p[0],p[2])),p[2]]);trackColors=times.map(t=>{const c=phaseColorAt(t);return [c.r,c.g,c.b];});
   track=lineSegmentsFrom(T,pts,{colors:trackColors,opacity:1});track.name='FlightTrack';track.userData.segments=pts.length-1;world.add(track);
   track.userData.times=times.slice(1);
  }
  // Event markers on the track.
  eventMarkers=new T.Group();eventMarkers.name='EventMarkers';
  for(const e of options.timeline?.events||[]){const p=sampler?.position(e.t);if(!p)continue;const color=PHASE_COLORS[e.kind==='apogee'?'apogee':e.phase]||'#e2e8f0';
   const m=new T.Mesh(new T.SphereGeometry(1,10,8),new T.MeshBasicMaterial({color:hexColor(T,color)}));m.name='EventMarker';m.userData={kind:e.kind,scaleWithDistance:.005};m.position.set(p[0],Math.max(p[1]+offset,heightAt(p[0],p[2])),p[2]);eventMarkers.add(m);
   if(e.kind==='apogee'){const ring=new T.Mesh(new T.SphereGeometry(1.25,10,8),new T.MeshBasicMaterial({color:0x111827,side:T.BackSide}));ring.userData.scaleWithDistance=.005;ring.position.copy(m.position);eventMarkers.add(ring);}
  }
  world.add(eventMarkers);
  for(const s of stations){const p=gpsToLocal([s.lat,s.lon,Number.isFinite(s.alt)?s.alt:origin[2]],origin);if(!p)continue;const y=heightAt(p[0],p[2]);
   const mast=new T.Group();mast.name='GroundStation';mast.userData={id:s.id,label:s.label};mast.position.set(p[0],y,p[2]);
   const pole=new T.Mesh(new T.CylinderGeometry(.08,.08,3,8),new T.MeshStandardMaterial({color:0xe2e8f0}));pole.position.y=1.5;mast.add(pole);
   const dish=new T.Mesh(new T.ConeGeometry(.6,.3,16,1,true),new T.MeshStandardMaterial({color:0xf8fafc,side:T.DoubleSide}));dish.position.y=3.1;dish.rotation.x=-Math.PI/3;mast.add(dish);
   const label=makeLabel(T,s.label||s.id,{border:'#38bdf8'});if(label){label.position.y=5;label.userData.scaleWithDistance=.03;label.userData.lift=0;mast.add(label);}
   world.add(mast);s.local=[p[0],y,p[2]];
  }
  if(overlays.length){overlayGroup=buildOverlayGroup(T,overlays,origin,heightAt,offset);world.add(overlayGroup);}
  canvas.dataset.terrain=terrain.source||'generic';request();
 }
 async function startTerrain(){
  const token=++terrainToken,module=await loadTerrainModule(options.loadTerrain);if(disposed||token!==terrainToken||!module||!origin)return;
  try{
   const extent=Math.min(30000,Math.max(3000,...positions.map(p=>Math.hypot(p[0],p[2])*1.4)));
   const loaded=await module.loadTerrain(T,origin,{extent_m:extent,fetchJson:options.fetchJson||(path=>fetch(path,{credentials:'same-origin'}).then(r=>r.ok?r.json():Promise.reject(Error(r.status)))),fetchImage:options.fetchImage,bundle:options.terrainBundle,maxTriangles:quality==='low'?20000:120000,maxTextureSize:quality==='low'?1024:4096});
   if(disposed||token!==terrainToken||!loaded?.group)return;
   if(terrain?.fallback)terrain.dispose();terrain=loaded;options.onTerrain?.(loaded);buildWorld();
  }catch{}
 }
 function setTrack(points,trackOrigin,opts={}){
  const valid=(Array.isArray(points)?points:[]).filter(p=>Number.isFinite(p.t)&&validGps([p.lat,p.lon,p.alt])).slice(0,20000);
  origin=validGps(trackOrigin)?trackOrigin:valid[0]?[valid[0].lat,valid[0].lon,valid[0].alt]:null;
  times=[];positions=[];const breaks=[];
  for(const p of valid){const local=gpsToLocal([p.lat,p.lon,p.alt],origin);if(!local)continue;if(times.length&&p.t<=times.at(-1))continue;if(p.break_before&&times.length)breaks.push(times.length);times.push(p.t);positions.push(local);}
  sampler=trackSampler(times,positions,breaks);if(opts.timeline)options.timeline=opts.timeline;
  if(terrain&&!terrain.fallback){buildWorld();}else{terrain?.dispose?.();terrain=null;buildWorld();startTerrain();}
 }
 function setOverlays(list){overlays=Array.isArray(list)?list.filter(o=>o&&typeof o==='object').slice(0,16):[];if(origin)buildWorld();}
 function setStations(list){stations=(Array.isArray(list)?list:[]).filter(s=>s&&Number.isFinite(s.lat)&&Number.isFinite(s.lon)).slice(0,8).map(s=>({...s}));if(origin)buildWorld();}
 function setTimeline(timeline){options.timeline=timeline;if(origin)buildWorld();}
 // frame: {t, phase, visual (phaseVisual), attitude: {quaternion}|null, recordedAttitude: [roll,pitch,yaw] degrees | null}
 const q=new T.Quaternion(),upright=new T.Quaternion(),lying=new T.Quaternion().setFromAxisAngle(new T.Vector3(0,0,1),Math.PI/2);
 function update(next){
  frame={...next};const phase=frame.phase||'pad',visual=frame.visual||phaseVisual(phase);
  let p=phase==='pad'?[0,0,0]:sampler?.position(frame.t);const v=sampler?.velocity(frame.t)||null;
  if(!p)p=sampler?.count?(frame.t<times[0]?[0,0,0]:positions.at(-1).slice()):[0,0,0];
  const ground=heightAt(p[0],p[2]),L=vehicle.length_m;
  let y=p[1]+offset;
  if(visual.landed)y=ground+vehicle.radius_m*1.2;else y=Math.max(y,ground+L/2+(phase==='pad'?.15:0));
  rocket=[p[0],y,p[2]];vehicle.group.position.set(...rocket);locator.position.set(...rocket);locator.material.color.set(PHASE_COLORS[phase]||PHASE_COLORS.unknown);
  const fromVelocity=attitudeFromVelocity(v);direction=v&&Math.hypot(...v)>1?v.map(c=>c/Math.hypot(...v)):null;
  let source='reference';
  if(visual.landed){vehicle.group.quaternion.copy(lying);source='landed';}
  else if(visual.upright){vehicle.group.quaternion.copy(upright);source=phase==='pad'?'pad':'hanging';}
  else if(Array.isArray(frame.recordedAttitude)&&frame.recordedAttitude.every(Number.isFinite)){vehicle.group.rotation.set(...frame.recordedAttitude.map(d=>d*Math.PI/180));source='recorded';}
  else if(fromVelocity){q.set(...fromVelocity.quaternion);vehicle.group.quaternion.copy(q);source='velocity';}
  else vehicle.group.quaternion.copy(upright);
  vehicle.setState(visual,performance.now());
  if(track){const n=visibleSegmentCount(track.userData.times,frame.t);track.geometry.setDrawRange(0,n*2);canvas.dataset.trailSegments=String(n);}
  canvas.dataset.attitudeSource=source;canvas.dataset.phase=phase;
  request();return {position:rocket,attitudeSource:source,velocity:v};
 }
 const tmp=new T.Vector3();
 function placeCamera(){
  const station=stations.length?stations[view.stationIndex%stations.length]?.local:null;
  // Under canopy, frame the vehicle and its chute together.
  const chute=vehicle.chutes.main?.visible?vehicle.chutes.main.userData.size:vehicle.chutes.drogue?.visible?vehicle.chutes.drogue.userData.size:null;
  const lift=chute?chute.line_length_m*.55:0,focus=[rocket[0],rocket[1]+lift,rocket[2]],distance=chute?Math.max(view.distance,chute.line_length_m*2.6+chute.diameter_m):view.distance;
  const pose=cameraPose(view.mode,{rocket:focus,direction,pad:[0,offset,0],station,theta:view.theta,phi:view.phi,distance,target:view.mode==='orbit'?view.target.toArray():null});
  camera.position.set(...pose.position);camera.up.set(...pose.up);camera.lookAt(...pose.target);
  // A camera cannot sit under the ground.
  const floor=heightAt(camera.position.x,camera.position.z)+1;if(camera.position.y<floor){camera.position.y=floor;camera.lookAt(...pose.target);}
  canvas.dataset.cameraMode=pose.mode;return pose;
 }
 function scaleMarkers(){scene.traverse(n=>{const s=n.userData?.scaleWithDistance;if(!s)return;const d=Math.max(1,camera.position.distanceTo(n.getWorldPosition(tmp)));if(n.isSprite)n.scale.set(d*s*n.userData.aspect,d*s,1);else n.scale.setScalar(Math.max(.3,d*s));});}
 function redraw(){
  if(disposed||contextLost||!dirty||!container.isConnected||document.hidden)return;
  const w=Math.max(1,container.clientWidth),h=Math.max(1,container.clientHeight);if(w!==width||h!==height){width=w;height=h;renderer.setSize(w,h);camera.aspect=w/h;camera.updateProjectionMatrix();}
  placeCamera();scaleMarkers();locator.visible=camera.position.distanceTo(vehicle.group.position)>vehicle.length_m*25;const far=Math.max(4e6,camera.position.y*20);if(camera.far!==far){camera.far=far;camera.updateProjectionMatrix();}
  renderer.render(scene,camera);canvas.dataset.rendered='true';dirty=false;
 }
 function request(){dirty=true;canvas.dataset.rendered='false';if(!raf)raf=requestAnimationFrame(()=>{raf=0;redraw();});}
 function setCameraMode(mode){
  if(!CAMERA_MODES.includes(mode))return view.mode;
  if(mode==='orbit')view.target.set(...rocket);
  if(mode==='station'&&view.mode==='station'&&stations.length)view.stationIndex=(view.stationIndex+1)%stations.length;
  view.mode=mode;request();options.onCamera?.(mode);return mode;
 }
 let drag=null;const pointers=new Map();
 listen(canvas,'pointerdown',e=>{pointers.set(e.pointerId,[e.clientX,e.clientY]);drag={x:e.clientX,y:e.clientY};canvas.setPointerCapture?.(e.pointerId);canvas.focus({preventScroll:true});});
 listen(canvas,'pointermove',e=>{const prev=pointers.get(e.pointerId);if(!prev)return;const dx=e.clientX-prev[0],dy=e.clientY-prev[1];
  if(pointers.size===2){const other=[...pointers.entries()].find(([id])=>id!==e.pointerId)[1],before=Math.hypot(prev[0]-other[0],prev[1]-other[1]),after=Math.hypot(e.clientX-other[0],e.clientY-other[1]);if(before>1&&after>1)view.distance=clamp(view.distance*before/after,1,2e5);}
  else{view.theta-=dx*.007;view.phi=clamp(view.phi+dy*.007,.08,Math.PI-.08);if(view.mode==='pad'||view.mode==='station'||view.mode==='top'){}}
  pointers.set(e.pointerId,[e.clientX,e.clientY]);request();});
 const up=e=>{pointers.delete(e.pointerId);drag=null;};listen(canvas,'pointerup',up);listen(canvas,'pointercancel',up);
 listen(canvas,'wheel',e=>{e.preventDefault();view.distance=clamp(view.distance*Math.exp(e.deltaY*.001),1,2e5);request();},{passive:false});
 listen(canvas,'keydown',e=>{if(e.ctrlKey||e.metaKey||e.altKey)return;const n=Number(e.key);if(n>=1&&n<=6){e.preventDefault();setCameraMode(CAMERA_MODES[n-1]);return;}
  const k=e.key;if(['ArrowLeft','ArrowRight','ArrowUp','ArrowDown','+','=','-'].includes(k)){e.preventDefault();if(k==='ArrowLeft')view.theta-=.15;if(k==='ArrowRight')view.theta+=.15;if(k==='ArrowUp')view.phi=clamp(view.phi-.12,.08,Math.PI-.08);if(k==='ArrowDown')view.phi=clamp(view.phi+.12,.08,Math.PI-.08);if(k==='+'||k==='=')view.distance=Math.max(1,view.distance/1.3);if(k==='-')view.distance=Math.min(2e5,view.distance*1.3);request();}});
 listen(document,'visibilitychange',request);
 listen(canvas,'webglcontextlost',e=>{e.preventDefault();contextLost=true;canvas.dataset.rendered='false';options.onStatus?.('lost');});
 listen(canvas,'webglcontextrestored',()=>{contextLost=false;options.onStatus?.('ready');request();});
 const resize=typeof ResizeObserver==='function'?new ResizeObserver(request):null;resize?.observe(container);
 function reset(){Object.assign(view,{theta:.75,phi:1.25,distance:Math.max(12,vehicle.length_m*3.2)});request();}
 function zoom(f){view.distance=clamp(view.distance*f,1,2e5);request();}
 request();
 return {canvas,software,quality,vehicle,scene,camera,setTrack,setOverlays,setStations,setTimeline,update,setCameraMode,reset,zoom,
  get cameraMode(){return view.mode;},get terrainSource(){return terrain?.source||'none';},get terrainAttribution(){return terrain?.attribution||'';},get overlayGroup(){return overlayGroup;},get origin(){return origin;},heightAt,
  dispose(){disposed=true;terrainToken++;cancelAnimationFrame(raf);resize?.disconnect();for(const remove of listeners)remove();clearWorld();terrain?.dispose?.();vehicle.group.traverse(n=>{n.geometry?.dispose();n.material?.dispose?.();});renderer.dispose();renderer.forceContextLoss?.();canvas.remove();}};
}
