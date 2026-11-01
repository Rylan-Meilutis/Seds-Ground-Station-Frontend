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
export function replayValveState(value){return value===1?'open':value===0?'closed':'unknown';}
export function createReplayAssembly(T,mode='flight'){
 const group=new T.Group();group.name=mode==='test_fire'?'ReplayMotorStand':'ReplayFlightVehicle';
 const moving=new T.Group();moving.name='RecordedAttitude';group.add(moving);
 const material=(color,opacity=1)=>new T.MeshStandardMaterial({color,roughness:.7,metalness:.12,transparent:opacity<1,opacity,depthWrite:opacity===1});
 const solid=(owner,name,geometry,color,position,rotation,opacity=1)=>{
  const mesh=new T.Mesh(geometry,material(color,opacity));mesh.name=name;mesh.position.set(...position);if(rotation)mesh.rotation.set(...rotation);owner.add(mesh);return mesh;
 };
 const box=(owner,name,size,color,position)=>solid(owner,name,new T.BoxGeometry(...size),color,position);
 const cylinder=(owner,name,radius,height,color,position,rotation,opacity=1)=>solid(owner,name,new T.CylinderGeometry(radius,radius,height,16),color,position,rotation,opacity);
 if(mode==='test_fire'){
  box(group,'StandBase',[4.2,.15,1.4],0x64748b,[0,-1.5,0]);
  for(const x of [-1.2,1.2]){box(group,'StandColumn',[.15,1.2,.15],0x94a3b8,[x,-.9,0]);box(group,'MotorCradle',[.22,.12,1.1],0x64748b,[x,-.3,0]);}
  cylinder(group,'HorizontalMotor',.35,2.8,0xcbd5e1,[0,.1,0],[0,0,Math.PI/2]);
  solid(group,'MotorNozzle',new T.CylinderGeometry(.22,.42,.5,16),0x475569,[1.65,.1,0],[0,0,Math.PI/2]);
  cylinder(group,'OxidizerReferenceTank',.3,1.7,0x94a3b8,[-2.2,-.55,0],null,.3);
  box(group,'InstrumentRail',[3.8,.07,.1],0x38bdf8,[0,-.5,.65]);
 }else{
  cylinder(moving,'Airframe',.35,3.6,0xcbd5e1,[0,0,0],null,.28);
  solid(moving,'Nose',new T.ConeGeometry(.35,.9,16),0xdbeafe,[0,2.25,0]);
  cylinder(moving,'FlightMotor',.27,.8,0x64748b,[0,-1.35,0]);
  solid(moving,'FlightNozzle',new T.CylinderGeometry(.14,.29,.35,16),0x475569,[0,-1.92,0]);
  for(let i=0;i<4;i++){const fin=box(moving,'Fin',[.58,.7,.06],0x60a5fa,[0,0,0]);fin.rotation.y=i*Math.PI/2;fin.position.set(Math.cos(i*Math.PI/2)*.46,-1.4,Math.sin(i*Math.PI/2)*.46);}
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
   const status=replayValveState(row.value);v.part.userData={label:String(row.label||row.id||'Valve').slice(0,128),state:status};
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
  group.userData={mode:mode==='test_fire'?'test_fire':'flight',attitudeKnown:known,omittedValves:Math.max(0,(state.valves?.length||0)-REPLAY_VALVE_LIMIT),omittedTanks:Math.max(0,(Array.isArray(state.components)?state.components:[]).filter(c=>c&&c.kind==='tank').length-REPLAY_TANK_LIMIT)};
 }
 function dispose(){group.traverse(n=>{n.geometry?.dispose();for(const m of Array.isArray(n.material)?n.material:n.material?[n.material]:[]){m.map?.dispose();m.dispose();}});}
 return {group,valves,tanks,update:setRows,dispose};
}

// Standalone replay and the configured viewer share the same assembly. This
// small camera owns its listeners/RAF and redraws only after a seek or gesture.
export function mountReplayCanvas(T,SoftwareRenderer,container,mode,onStatus=()=>{}){
 let renderer,software=false;
 try{renderer=new T.WebGLRenderer({alpha:false,antialias:false});}catch{renderer=new SoftwareRenderer();software=true;}
 renderer.setPixelRatio(Math.min(globalThis.devicePixelRatio||1,1.5));
 const canvas=renderer.domElement;canvas.style.cssText='display:block;width:100%;height:100%;touch-action:none;';canvas.tabIndex=0;canvas.dataset.scene=mode;canvas.setAttribute('aria-label','Drag to orbit the illustrative saved '+(mode==='test_fire'?'motor stand':'flight vehicle'));container.append(canvas);
 const scene=new T.Scene();scene.background=new T.Color(0x0f172a);scene.add(new T.HemisphereLight(0xffffff,0x334155,2.8));
 const light=new T.DirectionalLight(0xffffff,3);light.position.set(4,7,6);scene.add(light);
 const assembly=createReplayAssembly(T,mode);scene.add(assembly.group);
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
