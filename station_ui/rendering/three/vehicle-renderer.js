import * as T from './three.module.js';
import {GLTFLoader} from './GLTFLoader.js';
import {SoftwareRenderer} from './software-renderer.js';
import {EARTH_RADIUS_M, validGps, gpsToLocal, recordedPath, visibleSegmentCount} from './replay-scene.js';
// Explicit named-node transforms; no dependence on model-viewer's private scene graph.
class VehicleViewer extends HTMLElement {
 static observedAttributes=['src','data-state','data-track'];
 connectedCallback(){
  if(this.renderer)return;
  this.tabIndex=0;this.setAttribute('role','group');this.setAttribute('aria-description','Drag to orbit; shift-drag or two fingers to pan; pinch or scroll to zoom. Arrow keys orbit, W A S D pan, Q E rise or descend, F follows, R resets.');this.style.display='block';this.style.touchAction='none';this.status=document.createElement('div');this.status.style.cssText='position:absolute;top:42px;left:12px;right:12px;pointer-events:none;color:#b9cad7;font:12px system-ui;z-index:2';this.append(this.status);
  this.software=false;this.dirty=true;
  try{this.renderer=new T.WebGLRenderer({alpha:true,antialias:false,logarithmicDepthBuffer:true});}catch{
   try{this.renderer=new SoftwareRenderer();this.software=true;}catch{this.status.textContent='3D unavailable: neither WebGL2 nor Canvas2D is available';return;}
  }
  this.renderer.setPixelRatio(Math.min(devicePixelRatio,1.25));this.renderer.toneMapping=T.ACESFilmicToneMapping;
  this.renderer.domElement.addEventListener('webglcontextrestored',()=>{this.dirty=true;});this.append(this.renderer.domElement);this.scene=new T.Scene();this.skyColor=new T.Color(0x779bb6);this.referenceColor=new T.Color(0x111d2b);this.scene.background=this.referenceColor;this.root=new T.Group();this.scene.add(this.root);
  this.scene.add(new T.HemisphereLight(0xffffff,0x34404b,2.8));const light=new T.DirectionalLight(0xffffff,3);light.position.set(6,10,8);this.scene.add(light);
  this.camera=new T.PerspectiveCamera(48,1,.05,20000000);this.theta=.6;this.phi=1.2;this.distance=14;this.center=new T.Vector3();this.target=new T.Vector3();this.follow=true;this.pointers=new Map();
  this.resize=new ResizeObserver(()=>{const w=this.clientWidth,h=this.clientHeight;if(!w||!h)return;this.renderer.setSize(w,h);this.dirty=true;this.camera.aspect=w/h;this.camera.updateProjectionMatrix();});this.resize.observe(this);
  this.oncontextmenu=e=>e.preventDefault();
  this.onpointerdown=e=>{if(e.target!==this.renderer.domElement)return;this.focus({preventScroll:true});this.setPointerCapture(e.pointerId);this.pointers.set(e.pointerId,[e.clientX,e.clientY]);this.dragPan=e.button===2||e.shiftKey;};
  this.onpointerup=this.onpointercancel=e=>{this.pointers.delete(e.pointerId);};
  this.onpointermove=e=>{
   const previous=this.pointers.get(e.pointerId);if(!previous)return;
   const dx=e.clientX-previous[0],dy=e.clientY-previous[1];
   if(this.pointers.size===2){const other=[...this.pointers.entries()].find(([id])=>id!==e.pointerId)[1];const before=Math.hypot(previous[0]-other[0],previous[1]-other[1]),after=Math.hypot(e.clientX-other[0],e.clientY-other[1]);if(before>1&&after>1)this.distance=T.MathUtils.clamp(this.distance*before/after,.3,10000000);this.pan(dx/2,dy/2);}
   else if(this.dragPan||e.shiftKey)this.pan(dx,dy);
   else{this.theta-=dx*.006;this.phi=T.MathUtils.clamp(this.phi+dy*.006,.03,Math.PI-.03);}
   this.pointers.set(e.pointerId,[e.clientX,e.clientY]);this.dirty=true;
  };
  this.onwheel=e=>{e.preventDefault();this.distance=T.MathUtils.clamp(this.distance*Math.exp(e.deltaY*.001),.3,10000000);this.dirty=true;};
  this.onkeydown=e=>{
   const actions={ArrowLeft:'left',ArrowRight:'right',ArrowUp:'up',ArrowDown:'down','+':'zoom-in','=':'zoom-in','-':'zoom-out',f:'follow',r:'reset',w:'pan-up',s:'pan-down',a:'pan-left',d:'pan-right',q:'rise',e:'descend'};
   const action=actions[e.key];if(action&&!e.ctrlKey&&!e.metaKey&&!e.altKey){e.preventDefault();this.cameraAction(action);}
  };
  this.clock=new T.Clock();this.load();this.renderer.setAnimationLoop(()=>this.frame());
 }
 attributeChangedCallback(name,old,value){if(old===value)return;if(name==='src'&&this.renderer)this.load();if(name==='data-track'){try{this.track=JSON.parse(value);}catch{this.track=null;}this.trackChanged=true;this.dirty=true;}if(name==='data-state'){this.dirty=true;try{this.state=JSON.parse(value);}catch{this.state={};}}}
 viewDistance(){return this.distance/Math.min(1,Math.max(.1,this.camera.aspect));}
 pan(dx,dy){
  const scale=2*this.viewDistance()*Math.tan(T.MathUtils.degToRad(this.camera.fov/2))/Math.max(this.clientHeight,1);
  const right=new T.Vector3(Math.cos(this.theta),0,-Math.sin(this.theta));
  const up=new T.Vector3(-Math.cos(this.phi)*Math.sin(this.theta),Math.sin(this.phi),-Math.cos(this.phi)*Math.cos(this.theta));
  this.target.addScaledVector(right,-dx*scale).addScaledVector(up,dy*scale);this.follow=false;this.dirty=true;
 }
 cameraAction(action){
  if(action==='follow'||action==='reset'){this.follow=true;this.target.copy(this.root.position);if(action==='reset'){this.theta=.6;this.phi=1.28;this.distance=this.modelDistance||14;}}
  if(action==='overview'&&this.positionAvailable&&this.trackPositions?.length){const box=new T.Box3();for(let i=0;i<this.trackPositions.length;i+=3)box.expandByPoint(new T.Vector3(...this.trackPositions.slice(i,i+3)));box.getCenter(this.target);this.distance=Math.max(box.getSize(new T.Vector3()).length()*1.6,this.modelDistance||14);this.phi=1.28;this.follow=false;}
  if(action==='zoom-in')this.distance=Math.max(.3,this.distance/1.4);
  if(action==='zoom-out')this.distance=Math.min(10000000,this.distance*1.4);
  if(action==='left')this.theta-=.15;if(action==='right')this.theta+=.15;
  if(action==='up')this.phi=Math.max(.03,this.phi-.15);if(action==='down')this.phi=Math.min(Math.PI-.03,this.phi+.15);
  if(action.startsWith('pan-'))this.pan(action==='pan-left'?40:action==='pan-right'?-40:0,action==='pan-up'?40:action==='pan-down'?-40:0);
  if(action==='rise'||action==='descend'){this.target.y+=(action==='rise'?1:-1)*this.distance*.12;this.follow=false;}
  this.dataset.cameraMode=this.follow?'follow':'free';this.dirty=true;
 }
 clearFlightScene(){for(const object of [this.ground,this.trajectory])if(object){this.scene.remove(object);object.geometry.dispose();object.material.dispose();}this.ground=this.trajectory=null;this.origin=null;this.trackPositions=[];this.trackTimes=[];}
 updateFlightScene(state){
  if(this.trackChanged){this.clearFlightScene();this.trackChanged=false;this.follow=true;this.theta=.6;this.phi=1.28;this.distance=this.modelDistance||14;const first=this.track?.points?.find(p=>validGps([p.latitude,p.longitude,p.altitude_m]));if(first){this.origin=[first.latitude,first.longitude,first.altitude_m];const path=recordedPath(this.track.points,this.origin);this.trackPositions=path.positions;this.trackTimes=path.times;const geometry=new T.BufferGeometry();geometry.setAttribute('position',new T.Float32BufferAttribute(this.trackPositions,3));this.trajectory=new T.LineSegments(geometry,new T.LineBasicMaterial({color:0x9de5ff,transparent:true,opacity:.85}));this.scene.add(this.trajectory);this.createGround();}}
  const time=Number.isFinite(state.replay_timestamp_ms)?state.replay_timestamp_ms:(this.track?.points?.[0]?.timestamp_ms||0)+(state.replay_time_seconds||0)*1000;
  if(this.trajectory){const count=visibleSegmentCount(this.trackTimes||[],time)*2;this.trajectory.geometry.setDrawRange(0,count);this.dataset.trailSegments=String(count/2);}
  const position=this.origin?gpsToLocal(state.gps,this.origin):null;
  const available=!!position;
  if(this.positionAvailable!==available){this.follow=true;this.distance=this.modelDistance||14;this.theta=.6;this.phi=1.28;this.positionAvailable=available;}
  this.root.visible=true;
  if(this.ground)this.ground.visible=available;if(this.trajectory)this.trajectory.visible=available;
  this.scene.background=available?this.skyColor:this.referenceColor;
  if(position){this.root.position.set(...position);if(this.follow)this.target.copy(this.root.position);}
  else{this.root.position.set(0,0,0);if(this.follow)this.target.set(0,0,0);}
  this.dataset.positionAvailable=String(!!position);this.dataset.cameraMode=this.follow?'follow':'free';
 }
 createGround(){
  // Curved sea-level reference, centred under the first saved GPS fix. No fabricated terrain.
  const positions=[],rings=30,slices=96,radius=EARTH_RADIUS_M;
  const point=(ring,i)=>{const r=ring===0?0:Math.exp(Math.log(10)+(Math.log(radius*.95)-Math.log(10))*(ring-1)/(rings-1));const a=i/slices*Math.PI*2;return [r*Math.cos(a),Math.sqrt(radius*radius-r*r)-radius-this.origin[2],r*Math.sin(a)];};
  for(let ring=0;ring<rings;ring++)for(let i=0;i<slices;i++){const a=point(ring,i),b=point(ring+1,i),c=point(ring+1,i+1),d=point(ring,i+1);positions.push(...a,...b,...c,...a,...c,...d);}
  const geometry=new T.BufferGeometry();geometry.setAttribute('position',new T.Float32BufferAttribute(positions,3));this.ground=new T.Mesh(geometry,new T.MeshBasicMaterial({color:0x415e55,side:T.DoubleSide}));this.ground.userData.referenceGround=true;this.scene.add(this.ground);
 }
 disposeModel(){if(this.model){this.root.remove(this.pivot||this.model);this.pivot=null;this.model.traverse(n=>{n.geometry?.dispose();for(const m of Array.isArray(n.material)?n.material:n.material?[n.material]:[]){for(const v of Object.values(m))if(v?.isTexture)v.dispose();m.dispose();}});this.model=null;}}
 async load(){const src=this.getAttribute('src');if(!src)return;const generation=this.generation=(this.generation||0)+1;this.status.textContent='Loading model…';try{
   const gltf=await new GLTFLoader().loadAsync(src);if(!this.isConnected||generation!==this.generation){gltf.scene.traverse(n=>{n.geometry?.dispose();});return;}
   this.disposeModel();this.model=gltf.scene;const box=new T.Box3().setFromObject(this.model);const modelCenter=box.getCenter(new T.Vector3());const size=box.getSize(new T.Vector3());this.pivot=new T.Group();this.pivot.position.sub(modelCenter);this.pivot.add(this.model);this.center.set(0,0,0);this.root.add(this.pivot);this.base=new Map();this.smooth=new Map();this.model.traverse(n=>this.base.set(n,{position:n.position.clone(),scale:n.scale.clone(),quaternion:n.quaternion.clone(),visible:n.visible}));
   this.mixer=new T.AnimationMixer(this.model);this.clips=gltf.animations;this.activeClip=null;
   this.modelDistance=Math.max(size.length()*1.6,2);this.distance=this.modelDistance;this.status.textContent='';this.dataset.loaded='true';this.dirty=true;
  }catch(e){this.status.textContent='Model could not load. Check the GLB and its assets.';this.dataset.loaded='false';}
 }
 frame(){
  if(Number.isFinite(this.state?.replay_time_seconds)&&!this.dirty)return;
  const now=performance.now();if(document.hidden||!this.isConnected||!this.clientWidth||!this.clientHeight||now-(this.lastFrame||0)<(this.software?1000/12:1000/30))return;this.lastFrame=now;
  if(!this.model)return;const dt=Math.min(this.clock.getDelta(),.1),state=this.state||{};
  if(state.orbit&&state.orbit!==this.orbit){const parts=state.orbit.trim().split(/\s+/);const theta=parseFloat(parts[0]),phi=parseFloat(parts[1]),radius=parseFloat(parts[2]);if(Number.isFinite(theta))this.theta=T.MathUtils.degToRad(theta);if(Number.isFinite(phi))this.phi=T.MathUtils.clamp(T.MathUtils.degToRad(phi),.1,3);if(Number.isFinite(radius)&&radius>0)this.distance=radius;this.orbit=state.orbit;}
  if(state.clip!==this.activeClip){this.mixer.stopAllAction();const clip=this.clips.find(c=>c.name===state.clip);if(clip)this.mixer.clipAction(clip).play();this.activeClip=state.clip;}
  for(const node of this.driven||[]){const b=this.base.get(node);if(b){node.position.copy(b.position);node.quaternion.copy(b.quaternion);node.scale.copy(b.scale);node.visible=b.visible;}}
  const replay=Number.isFinite(state.replay_time_seconds);
  if(replay)this.mixer.setTime(Math.max(0,state.replay_time_seconds));else this.mixer.update(dt);
  const modified=new Set();let missing=0,unknown=0;
  for(const motion of state.motions||[]){const node=this.model.getObjectByName(motion.node);if(!node){missing++;continue;}const base=this.base.get(node);if(!base)continue;
   if(!modified.has(node)){node.position.copy(base.position);node.quaternion.copy(base.quaternion);node.scale.copy(base.scale);node.visible=base.visible;modified.add(node);}
   if(motion.value==null||!Number.isFinite(motion.value)){unknown++;continue;}
   const target=motion.from+(motion.to-motion.from)*T.MathUtils.clamp(motion.value,0,1),key=motion.node+':'+motion.transform+':'+motion.axis.join(',');
   const prior=this.smooth.get(key)??target,v=replay?target:T.MathUtils.lerp(prior,target,1-Math.exp(-dt*10));this.smooth.set(key,v);
   const axis=new T.Vector3(...motion.axis);
   if(motion.transform==='rotate'&&axis.lengthSq()>0)node.quaternion.multiply(new T.Quaternion().setFromAxisAngle(axis.normalize(),T.MathUtils.degToRad(v)));
   if(motion.transform==='translate')node.position.addScaledVector(axis,v);
   if(motion.transform==='scale')node.scale.multiply(new T.Vector3(1+axis.x*(v-1),1+axis.y*(v-1),1+axis.z*(v-1)));
   if(motion.transform==='visible')node.visible=v>=.5;
  }
  this.driven=modified;
  this.status.textContent=missing?missing+' mapped model node(s) missing':unknown?unknown+' component(s): no telemetry':this.software?'Compatibility preview: reduced-detail model':'';
  this.updateFlightScene(state);
  const a=state.attitude||[null,null,null];this.root.rotation.set(...a.map(v=>T.MathUtils.degToRad(Number.isFinite(v)?v:0)));this.dataset.attitudeAvailable=String(a.every(Number.isFinite));if(!a.every(Number.isFinite))this.status.textContent=[this.status.textContent,'Attitude unavailable · reference orientation'].filter(Boolean).join(' · ');
  const note=!this.origin?'Reference attitude mode · no saved GPS track':!validGps(state.gps)?'Reference attitude mode · no fresh saved GPS':'Reference Earth surface · terrain unavailable'+(this.track?.sampled||this.track?.truncated?' · sampled path':'');this.status.textContent=[this.status.textContent,note].filter(Boolean).join(' · ');
  const distance=this.viewDistance(),far=Math.max(20000000,distance*2);if(this.camera.far!==far){this.camera.far=far;this.camera.updateProjectionMatrix();}this.camera.position.set(this.target.x+distance*Math.sin(this.phi)*Math.sin(this.theta),this.target.y+distance*Math.cos(this.phi),this.target.z+distance*Math.sin(this.phi)*Math.cos(this.theta));this.camera.lookAt(this.target);this.renderer.render(this.scene,this.camera);this.dirty=false;
 }
 disconnectedCallback(){this.generation++;this.resize?.disconnect();this.renderer?.setAnimationLoop(null);this.disposeModel();this.clearFlightScene();this.pointers?.clear();this.renderer?.dispose();this.renderer?.domElement.remove();this.status?.remove();this.renderer=null;}
}
if(!customElements.get('gs-vehicle-viewer'))customElements.define('gs-vehicle-viewer',VehicleViewer);
