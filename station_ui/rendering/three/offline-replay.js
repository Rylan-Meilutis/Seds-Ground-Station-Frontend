// Self-contained saved-file viewer. No network, live telemetry or command APIs.
(()=>{
'use strict';
const MAX_BYTES=64*1024*1024,MAX_ROWS=200000;
const finite=Number.isFinite;
// Files saved before the ox tank PT rename name it FUEL_TANK_PRESSURE.
// Whole strings only; the embedded schema keeps the names it was saved with.
const LEGACY_NAMES=new Map([['FUEL_TANK_PRESSURE','OX_TANK_PRESSURE'],['Fuel Tank Pressure','Ox Tank Pressure'],['Fuel tank pressure','Ox tank pressure'],['FUEL TANK PRESSURE','OX TANK PRESSURE']]);
function canonicalNames(v){
 if(typeof v==='string')return LEGACY_NAMES.get(v)??v;
 if(Array.isArray(v)){for(let i=0;i<v.length;i++)v[i]=canonicalNames(v[i]);return v;}
 if(v&&typeof v==='object')for(const k of Object.keys(v)){const x=canonicalNames(v[k]),n=LEGACY_NAMES.get(k);if(n&&!Object.hasOwn(v,n)){delete v[k];v[n]=x;}else v[k]=x;}
 return v;
}
function validate(input,options={}){
 if(typeof input==='string'&&new TextEncoder().encode(input).length>MAX_BYTES)throw Error('Replay exceeds 64 MiB.');
 const p=typeof input==='string'?JSON.parse(input):input;
 if(!p||p.format!=='seds-replay'||p.version!==1)throw Error('Choose a SEDS replay v1 file.');
 if(!Array.isArray(p.samples)||p.samples.length>MAX_ROWS||(!p.samples.length&&options.allowEmpty!==true))throw Error('Replay must contain 1–200,000 saved rows.');
 if(!p.schema||typeof p.schema!=='object'||!p.view_config||typeof p.view_config!=='object'||!p.layout||typeof p.layout!=='object'||!Array.isArray(p.valve_mappings))throw Error('Replay is missing its embedded schema, view configuration, layout or valve mappings.');
 for(const r of p.samples)if(r&&typeof r==='object'&&LEGACY_NAMES.has(r.data_type))r.data_type=LEGACY_NAMES.get(r.data_type);
 canonicalNames(p.view_config);canonicalNames(p.layout);canonicalNames(p.valve_mappings);
 const stages=p.view_config.stages||[],ground=p.view_config.ground_systems||[];
 if(!Array.isArray(stages)||!Array.isArray(ground)||stages.length>32||ground.length>128||stages.some(s=>!s||typeof s!=='object'||!Array.isArray(s.components)||s.components.length>128))throw Error('Invalid or too many visual components.');
 const validBinding=b=>b==null||(typeof b==='object'&&!Array.isArray(b)&&typeof b.data_type==='string'&&b.data_type.length>0&&b.data_type.length<=128&&(b.sender_id==null||(typeof b.sender_id==='string'&&b.sender_id.length<=256))&&(b.index===undefined||(Number.isInteger(b.index)&&b.index>=0&&b.index<1024))&&(b.scale==null||finite(b.scale))&&(b.offset==null||finite(b.offset)));
 if([...ground,...stages.flatMap(s=>s.components)].some(c=>!c||typeof c!=='object'||Array.isArray(c)||!validBinding(c.binding)||['id','label','kind','unit'].some(k=>c[k]!==undefined&&(typeof c[k]!=='string'||c[k].length>256))||['min','max'].some(k=>c[k]!==undefined&&c[k]!==null&&!finite(c[k]))))throw Error('Invalid saved visual component.');
 if(p.view_config.attitude!==undefined&&(!p.view_config.attitude||typeof p.view_config.attitude!=='object'||['roll','pitch','yaw'].some(k=>!validBinding(p.view_config.attitude[k]))))throw Error('Invalid saved attitude bindings.');
 const ids=new Set();
 for(const r of p.samples){
  if(!Number.isSafeInteger(r.id)||r.id<0||ids.has(r.id)||!Number.isSafeInteger(r.timestamp_ms)||r.timestamp_ms<0||r.timestamp_ms>8640000000000000||typeof r.data_type!=='string'||r.data_type.length>128||typeof r.sender_id!=='string'||r.sender_id.length>256||!Array.isArray(r.values)||r.values.length>1024||r.values.some(v=>v!==null&&!finite(v)))throw Error('Invalid saved sample; no replay data was loaded.');
  if(r.valid_until_ms!=null&&(!Number.isSafeInteger(r.valid_until_ms)||r.valid_until_ms<r.timestamp_ms))throw Error('Invalid saved coverage boundary.');
  ids.add(r.id);
 }
 if(p.valve_mappings.length>128||p.valve_mappings.some(v=>!v||typeof v.label!=='string'||v.label.length>128||!validBinding(v)||!Number.isInteger(v.index)||v.index<0||v.index>1023||['true_label','false_label','unknown_label'].some(k=>v[k]!==undefined&&(typeof v[k]!=='string'||v[k].length>128))))throw Error('Invalid valve mappings.');
 p.samples=p.samples.slice().sort((a,b)=>a.timestamp_ms-b.timestamp_ms||a.id-b.id);
 const series=new Map();
 for(const r of p.samples){let sources=series.get(r.data_type);if(!sources){sources=new Map();series.set(r.data_type,sources);}let rows=sources.get(r.sender_id);if(!rows){rows=[];sources.set(r.sender_id,rows);}rows.push(r);}
 Object.defineProperty(p,'__series',{value:series,configurable:true});
 p.start_ms=p.samples[0]?.timestamp_ms??0;p.end_ms=p.samples.at(-1)?.timestamp_ms??0;
 return p;
}
function latest(p,type,sender,at){
 const sources=p.__series.get(type);if(!sources)return null;
 function previous(rows){let lo=0,hi=rows.length;while(lo<hi){const mid=(lo+hi)>>>1;if(rows[mid].timestamp_ms<=at)lo=mid+1;else hi=mid;}return lo?rows[lo-1]:null;}
 if(sender)return sources.has(sender)?previous(sources.get(sender)):null;
 let found=null;for(const rows of sources.values()){const r=previous(rows);if(r&&(!found||r.timestamp_ms>found.timestamp_ms||(r.timestamp_ms===found.timestamp_ms&&r.id>found.id)))found=r;}return found;
}
function value(p,b,at,hold=false){if(!b)return null;const r=latest(p,b.data_type,b.sender_id,at);if(!r||(r.valid_until_ms!=null&&at>r.valid_until_ms)||(!hold&&at-r.timestamp_ms>5000))return null;const v=r.values[b.index??0];if(!finite(v))return null;const n=v*(b.scale??1)+(b.offset??0);return finite(n)?n:null;}
// A pressure range is not a liquid-level measurement. Only an explicitly
// configured percentage or loadcell mass range can drive this indicator.
function tankFillFraction(c){
 const unit=typeof c.unit==='string'?c.unit.trim().toLowerCase():'';
 if(c.kind!=='tank'||!['%','percent','kg','g'].includes(unit)||!c.binding||typeof c.binding.data_type!=='string'||!c.binding.data_type.trim()||!finite(c.value)||!finite(c.min)||!finite(c.max)||c.max<=c.min)return null;
 return Math.max(0,Math.min(1,(c.value-c.min)/(c.max-c.min)));
}
function replayValveState(value,row){
 if(value!==0&&value!==1)return 'unknown';
 const state=row?.state!==undefined?row.state:row?.[value===1?'true_label':'false_label'];
 if(state===undefined)return value===1?'open':'closed';
 const canonical=typeof state==='string'?state.trim().toLowerCase():'';
 return canonical==='open'||canonical==='closed'?canonical:'unknown';
}
function state(p,at){
 const components=[...(p.view_config.ground_systems||[]),...(p.view_config.stages||[]).flatMap(s=>s.components||[])];
 return {valves:p.valve_mappings.map(v=>({...v,value:value(p,v,at),state:(()=>{const n=value(p,v,at);return n===1?(v.true_label||'Open'):n===0?(v.false_label||'Closed'):(v.unknown_label||'Unknown');})()})),
 components:components.map(c=>({...c,value:value(p,c.binding,at,['parachute','link'].includes(c.kind))})),
 attitude:['roll','pitch','yaw'].map(k=>value(p,p.view_config.attitude?.[k],at)),
 phase:latest(p,'FLIGHT_STATE','FC',at)?.phase||'Unknown'};
}
// ---------------------------------------------------------------------------
// Flight replay: the flight-state timeline, time controls, sensor channels and
// the HUD. The phase vocabulary and palette are the shared flight-plan contract
// (replay-scene.js keeps the same table for the 3D model; a test checks both agree).
const PHASE_COLORS={pad:'#94a3b8',boost:'#f97316',coast:'#facc15',apogee:'#ffffff',drogue:'#38bdf8',main:'#22c55e',landed:'#a78bfa',unknown:'#64748b'};
const PHASE_LABELS={pad:'Pad',boost:'Powered ascent',coast:'Coast',drogue:'Drogue descent',main:'Main descent',landed:'Landed',unknown:'Unknown'};
const FLIGHT_STATE_NAMES=['Startup','Idle','PreFill','FillTest','NitrogenFill','NitrousFill','Armed','Launch','Ascent','Coast','Apogee','Descent','Reefing','Landed','Recovery','Aborted','InFlight'];
const STATE_PHASE={startup:'pad',idle:'pad',prefill:'pad',fill:'pad',filltest:'pad',nitrogenfill:'pad',nitrousfill:'pad',armed:'pad',launch:'boost',ascent:'boost',coast:'coast',apogee:'coast',descent:'drogue',parachutedeploy:'drogue',reefing:'main',landed:'landed',recovery:'landed'};
function flightPhaseOf(state,previous='pad'){
 const name=typeof state==='number'?FLIGHT_STATE_NAMES[state]:state;
 const key=String(name??'').replace(/[^a-z]/gi,'').toLowerCase();
 if(key==='aborted')return previous;
 if(key==='inflight')return ['boost','coast','drogue','main'].includes(previous)?previous:'boost';
 return STATE_PHASE[key]||previous;
}
const EVENT_KINDS=['ignition','liftoff','rail_exit','burnout','apogee','drogue','main','landing'];
const EVENT_LABELS={ignition:'Ignition',liftoff:'Liftoff',rail_exit:'Rail exit',burnout:'Burnout',apogee:'Apogee',drogue:'Drogue',main:'Main',landing:'Landing'};
const JUMP_EVENTS=['ignition','burnout','apogee','drogue','main','landing'];
const EVENT_PHASE={ignition:'boost',liftoff:'boost',rail_exit:'boost',burnout:'coast',apogee:'coast',drogue:'drogue',main:'main',landing:'landed'};
function stateName(row){if(typeof row?.phase==='string'&&row.phase)return row.phase;const v=row?.values?.[0];return finite(v)?FLIGHT_STATE_NAMES[Math.round(v)]||null:null;}
// Events named by a flight review or a plan, mapped to the contract's kinds.
function eventKindOf(name){
 const n=String(name||'').toLowerCase();
 if(EVENT_KINDS.includes(n))return n;
 if(/ignit|t-0|^t0/.test(n))return 'ignition';if(/lift.?off/.test(n))return 'liftoff';if(/rail/.test(n))return 'rail_exit';if(/burn.?out/.test(n))return 'burnout';
 if(/apogee/.test(n))return 'apogee';if(/drogue/.test(n))return 'drogue';if(/\bmain\b/.test(n))return 'main';if(/touch.?down|land/.test(n))return 'landing';return null;
}
// The phase timeline and events: the recorded FLIGHT_STATE rows first; a flight
// review or a plan (extras) only fills events the rows do not have.
function flightTimeline(p,extras={},options={}){
 const start=p.start_ms,end=Math.max(p.end_ms,start);
 const all=(p.__series?.get('FLIGHT_STATE')||new Map());const sender=all.has('FC')?'FC':[...all.keys()][0];
 const rows=sender?all.get(sender):[],segments=[],events=[];
 let phase=null,from=start;const seen=new Set();
 const addEvent=(kind,t,source)=>{if(!kind||seen.has(kind)||!finite(t))return;seen.add(kind);events.push({kind,t,label:EVENT_LABELS[kind],phase:EVENT_PHASE[kind],source});};
 for(const row of rows){
  const name=stateName(row);if(!name)continue;const before=phase??'pad',next=flightPhaseOf(name,before);
  if(/^apogee$/i.test(name))addEvent('apogee',row.timestamp_ms,'FLIGHT_STATE');
  if(phase===null){phase=next;from=Math.min(start,row.timestamp_ms);if(next!=='pad'&&next!=='unknown')addEvent(next==='boost'?'ignition':next==='landed'?'landing':next,row.timestamp_ms,'FLIGHT_STATE');continue;}
  if(next===phase)continue;
  segments.push({phase,from,to:row.timestamp_ms});
  if(next==='boost')addEvent('ignition',row.timestamp_ms,'FLIGHT_STATE');
  if(phase==='boost'&&next==='coast')addEvent('burnout',row.timestamp_ms,'FLIGHT_STATE');
  if(next==='drogue')addEvent('drogue',row.timestamp_ms,'FLIGHT_STATE');
  if(next==='main')addEvent('main',row.timestamp_ms,'FLIGHT_STATE');
  if(next==='landed')addEvent('landing',row.timestamp_ms,'FLIGHT_STATE');
  phase=next;from=row.timestamp_ms;
 }
 segments.push({phase:phase??'unknown',from,to:end});
 const inRange=t=>finite(t)&&t>=start-60000&&t<=end+60000;
 const review=extras.flight_review?.flight||extras.flight_review;
 for(const e of Array.isArray(review?.events)?review.events:[])if(inRange(e?.t_ms))addEvent(eventKindOf(e.name),e.t_ms,'flight review');
 if(review?.apogee&&inRange(review.apogee.t_ms))addEvent('apogee',review.apogee.t_ms,'flight review');
 const plan=extras.flight_plan;const planZero=finite(plan?.created_ms)?plan.created_ms:start;
 for(const e of Array.isArray(plan?.events)?plan.events:[])if(finite(e?.t_s))addEvent(eventKindOf(e.kind),planZero+e.t_s*1000,'flight plan');
 // No recorded apogee: the highest altitude between ignition and the drogue (or landing).
 if(!seen.has('apogee')&&Array.isArray(options.altitudeRows)&&options.altitudeRows.length){
  const a=events.find(e=>e.kind==='ignition')?.t,b=events.find(e=>e.kind==='drogue'||e.kind==='landing')?.t;
  if(finite(a)&&finite(b)&&b>a){let best=null;for(const r of options.altitudeRows||[]){if(r.t<a||r.t>b||!finite(r.v))continue;if(!best||r.v>best.v)best=r;}if(best)addEvent('apogee',best.t,'highest recorded altitude');}
 }
 events.sort((x,y)=>x.t-y.t||EVENT_KINDS.indexOf(x.kind)-EVENT_KINDS.indexOf(y.kind));
 const ignition=events.find(e=>e.kind==='ignition'||e.kind==='liftoff');
 const t0=ignition?.t??(finite(review?.t0_ms)?review.t0_ms:finite(plan?.created_ms)?plan.created_ms:null);
 return {segments:segments.filter(s=>s.to>=s.from),events,t0,start,end,source:rows.length?'FLIGHT_STATE':'none'};
}
function phaseAt(timeline,t){const s=timeline.segments;for(let i=s.length-1;i>=0;i--)if(t>=s[i].from)return s[i].phase;return s[0]?.phase||'unknown';}
// T-clock. TODO(integration): same output as the shared tclock.js formatTClock (fp-tclock);
// this classic script cannot import the ES module, so it keeps this copy (a test checks parity once both are on one branch).
function formatTClock(ms){
 if(!finite(ms))return 'T --:--';
 const negative=ms<0,total=negative?Math.ceil(-ms/1000):Math.floor(ms/1000),d=Math.floor(total/86400),h=Math.floor(total%86400/3600),m=Math.floor(total%3600/60),s=total%60,pad=v=>String(v).padStart(2,'0');
 const body=d?`${d}:${pad(h)}:${pad(m)}:${pad(s)}`:h?`${h}:${pad(m)}:${pad(s)}`:`${pad(m)}:${pad(s)}`;
 return (negative?'T- ':'T+ ')+body;
}
const SPEEDS=[.1,.25,.5,1,2,4,10,20],AUTO_SLOW_SPEED=.25,AUTO_SLOW_WINDOW_MS=1500;
// Replay time: play/pause, seek, step a frame or an event, jump to an event, speed,
// automatic slow motion around events and an A/B loop. Pure; the player ticks it.
function createClock({start,end,events=[],frames=[]}){
 const c={start,end,t:start,playing:false,speed:1,autoSlow:false,loop:{a:null,b:null},events:events.slice().sort((a,b)=>a.t-b.t),frames:[...new Set(frames.filter(finite))].sort((a,b)=>a-b)};
 const clampT=t=>Math.max(c.start,Math.min(c.end,t));
 c.seek=t=>{if(finite(t))c.t=clampT(t);return c.t;};
 c.play=()=>{if(c.t>=c.end)c.t=c.loopActive()?c.loop.a:c.start;c.playing=true;return c;};
 c.pause=()=>{c.playing=false;return c;};c.toggle=()=>c.playing?c.pause():c.play();
 c.setSpeed=s=>{if(finite(s))c.speed=Math.max(.1,Math.min(20,s));return c.speed;};
 c.loopActive=()=>finite(c.loop.a)&&finite(c.loop.b)&&c.loop.b>c.loop.a;
 c.setLoop=(which,t=c.t)=>{if(which==='a'||which==='b')c.loop[which]=clampT(t);if(finite(c.loop.a)&&finite(c.loop.b)&&c.loop.b<c.loop.a)[c.loop.a,c.loop.b]=[c.loop.b,c.loop.a];return c.loop;};
 c.clearLoop=()=>{c.loop={a:null,b:null};return c.loop;};
 c.stepFrame=(dir=1)=>{c.pause();const f=c.frames;if(f.length){if(dir>0){const n=f.find(t=>t>c.t+.5);return c.seek(n??c.end);}let p=null;for(const t of f){if(t<c.t-.5)p=t;else break;}return c.seek(p??c.start);}return c.seek(c.t+dir*100);};
 c.stepEvent=(dir=1)=>{const list=c.events;const e=dir>0?list.find(e=>e.t>c.t+.5):[...list].reverse().find(e=>e.t<c.t-.5);if(e)c.seek(e.t);return e||null;};
 c.jump=kind=>{const e=c.events.find(e=>e.kind===kind);if(e){c.seek(e.t);}return e||null;};
 c.nearEvent=t=>c.events.some(e=>JUMP_EVENTS.includes(e.kind)&&Math.abs(e.t-t)<=AUTO_SLOW_WINDOW_MS);
 c.effectiveSpeed=(t=c.t)=>c.autoSlow&&c.nearEvent(t)?Math.min(c.speed,AUTO_SLOW_SPEED):c.speed;
 c.tick=dtMs=>{
  if(!c.playing||!finite(dtMs)||dtMs<=0)return c.t;
  let next=c.t+Math.min(1000,dtMs)*c.effectiveSpeed();
  if(c.loopActive()&&c.t<=c.loop.b&&next>=c.loop.b)next=c.loop.a+(next-c.loop.b)%Math.max(1,c.loop.b-c.loop.a);
  else if(next>=c.end){next=c.end;c.playing=false;}
  c.t=clampT(next);return c.t;
 };
 return c;
}
// Channel roles for the HUD and charts, bound generically: the data types the
// recording holds, their channel names and units from the layout's data tabs.
const ROLES=[
 {role:'altitude',label:'Altitude',unit:'m',rules:[{type:/^BAROMETER_DATA$/,channel:/altitude/i,index:0},{type:/^(ASCENT|DESCENT)_STATE$/,channel:/altitude/i},{type:/^KALMAN/,channel:/altitude/i}],derive:'gps_agl'},
 {role:'velocity',label:'Velocity',unit:'m/s',rules:[{type:/^VEHICLE_SPEED$/,index:0},{type:/^(ASCENT|DESCENT)_STATE$/,channel:/velocity/i}],derive:'gps_speed'},
 {role:'acceleration',label:'Acceleration',unit:'',rules:[{type:/^ACCEL_DATA$/,vector:3},{type:/^IMU_DATA$/,vector:3}]},
 {role:'chamber',label:'Chamber pressure',unit:'psi',rules:[{type:/CHAMBER_PRESSURE$/,index:0},{type:/^DAQ_LOADCELL_CHAMBER$/,index:0}]},
 {role:'tank',label:'Tank pressure',unit:'psi',rules:[{type:/TANK_PRESSURE$/,index:0}]},
 {role:'thrust',label:'Thrust',unit:'',rules:[{type:/THRUST/,index:0},{type:/^KG\d+$/,largest:true,index:0,unit:'kg'}]},
 {role:'battery',label:'Battery',unit:'V',rules:[{type:/^BATTERY_VOLTAGE$/,index:0,prefer:['FC','PB','RF']}]},
 {role:'gps',label:'GPS fix',unit:'',rules:[{type:/^GPS_STATUS$/,index:0,format:'gps'},{type:/^GPS_SATELLITE_NUMBER$/,index:0,format:'sats'}]},
 {role:'radio',label:'Radio link',unit:'',rules:[{type:/RSSI|LINK_QUALITY|SNR/,index:0}]},
];
function layoutTabs(layout){
 const out=[];const walk=(tabs,parent)=>{for(const t of Array.isArray(tabs)?tabs:[]){if(!t||typeof t!=='object')continue;out.push({data_type:t.data_type||t.id||parent?.data_type,sender_id:t.sender_id??null,label:t.label,channels:Array.isArray(t.channels)?t.channels:parent?.channels||[],formatters:Array.isArray(t.channel_formatters)?t.channel_formatters:parent?.formatters||[]});walk(t.subtabs,out.at(-1));}};
 walk(layout?.data_tab?.tabs);return out;
}
function unitFrom(text,formatter){const suffix=typeof formatter?.suffix==='string'?formatter.suffix.trim():'';if(suffix)return suffix;const m=/\(([^)]{1,8})\)\s*$/.exec(String(text||''));return m?m[1]:'';}
function discoverChannels(p){
 const series=p.__series||new Map(),tabs=layoutTabs(p.layout),out=[];
 for(const spec of ROLES){
  let found=null;
  for(const rule of spec.rules){
   let types=[...series.keys()].filter(t=>rule.type.test(t));if(!types.length)continue;
   if(rule.largest)types.sort((a,b)=>Number(b.replace(/\D/g,''))-Number(a.replace(/\D/g,'')));
   const type=types[0],senders=[...series.get(type).keys()],sender=(rule.prefer||[]).find(s=>senders.includes(s))??(senders.length===1?senders[0]:senders.sort((a,b)=>series.get(type).get(b).length-series.get(type).get(a).length)[0]);
   const tab=tabs.find(t=>t.data_type===type&&(t.sender_id==null||t.sender_id===sender)&&t.channels.length)||tabs.find(t=>t.data_type===type);
   if(rule.channel&&!tab&&rule.index===undefined)continue;let index=rule.index??0;if(rule.channel&&tab){const i=tab.channels.findIndex(c=>rule.channel.test(String(c)));if(i<0)continue;index=i;}
   const channelName=tab?.channels?.[index];
   found={role:spec.role,label:spec.label,source:[tab?.label||type,rule.vector?null:channelName].filter(Boolean).filter((v,i,a)=>a.indexOf(v)===i).join(' · '),unit:unitFrom(channelName,tab?.formatters?.[index])||rule.unit||spec.unit,binding:{data_type:type,sender_id:sender,index},vector:rule.vector||0,format:rule.format||null};
   break;
  }
  if(!found&&spec.derive)found={role:spec.role,label:spec.label,source:'from GPS track',unit:spec.unit,binding:null,derived:spec.derive};
  out.push(found||{role:spec.role,label:spec.label,unit:spec.unit,binding:null,absent:true});
 }
 return out;
}
const R_EARTH=6371008.8;
function enu(origin,lat,lon,alt){const rad=Math.PI/180;return [(lon-origin[1])*rad*Math.cos(origin[0]*rad)*R_EARTH,alt-origin[2],-(lat-origin[0])*rad*R_EARTH];}
function validFix(v){return Array.isArray(v)&&v.length>=3&&v.slice(0,3).every(finite)&&Math.abs(v[0])<=90&&Math.abs(v[1])<=180&&v[2]>=-20000&&v[2]<=10000000&&!(v[0]===0&&v[1]===0);}
// One recorded GPS sender (the backend trajectory's, else FC, else RF), never stitched.
function gpsTrack(p,extras={}){
 const traj=extras.trajectory;
 if(Array.isArray(traj?.points)&&traj.points.length>1){
  const pts=dropRepeats(traj.points.slice(0,8192).filter(q=>finite(q?.timestamp_ms)&&validFix([q.latitude,q.longitude,q.altitude_m])).map(q=>({t:q.timestamp_ms,lat:q.latitude,lon:q.longitude,alt:q.altitude_m,break_before:q.break_before===true})));
  if(pts.length>1)return {points:pts,sender:traj.source_sender_id||null,sampled:!!traj.sampled};
 }
 for(const type of ['GPS_DATA','GPS','ROCKET_GPS']){
  const sources=p.__series?.get(type);if(!sources)continue;const sender=[traj?.source_sender_id,'FC','RF',...sources.keys()].find(s=>s&&sources.has(s));
  const rows=sources.get(sender)||[],pts=[];let last=null;
  for(const r of rows){const v=r.values;if(!validFix(v))continue;pts.push({t:r.timestamp_ms,lat:v[0],lon:v[1],alt:v[2],break_before:last!=null&&r.timestamp_ms-last>5000});last=r.timestamp_ms;}
  if(pts.length)return {points:dropRepeats(pts).slice(-20000),sender,sampled:false};
 }
 return {points:[],sender:null,sampled:false};
}
// A receiver that holds its last fix repeats it exactly; a repeat is not a new position.
function dropRepeats(pts){const out=[];for(const q of pts){const l=out.at(-1);if(l&&q.lat===l.lat&&q.lon===l.lon&&q.alt===l.alt&&!q.break_before)continue;out.push(q);}return out;}
// Height for the scene: the barometric altitude above the pad when recorded (GPS
// height is the weakest axis, worst in boost), else the GPS altitude.
function heightTrack(track,altitudeChannel,altitudeRows){
 const pts=track.points;if(!pts.length||!altitudeChannel?.binding||!altitudeRows?.length)return {points:pts,source:'gps'};
 const zero=altitudeRows.find(r=>r.t>=pts[0].t-5000)?.v??altitudeRows[0].v,pad=pts[0].alt;let used=0;
 const points=pts.map(q=>{const h=interpolate(altitudeRows,q.t,5000);if(!finite(h))return q;used++;return {...q,alt:pad+(h-zero)};});
 return used>=pts.length/2?{points,source:'barometer'}:{points:pts,source:'gps'};
}
function trackDerived(track,padIndex=0){
 const pts=track.points;if(!pts.length)return {origin:null,agl:[],speed:[]};
 const origin=[pts[padIndex].lat,pts[padIndex].lon,pts[padIndex].alt],local=pts.map(q=>enu(origin,q.lat,q.lon,q.alt)),agl=[],speed=[];
 for(let i=0;i<pts.length;i++){agl.push({t:pts[i].t,v:local[i][1]});if(i&&!pts[i].break_before){const dt=(pts[i].t-pts[i-1].t)/1000;if(dt>0)speed.push({t:(pts[i].t+pts[i-1].t)/2,v:Math.hypot(...[0,1,2].map(k=>local[i][k]-local[i-1][k]))/dt});}}
 return {origin,agl,speed};
}
function interpolate(rows,t,maxGap=5000){let lo=0,hi=rows.length;while(lo<hi){const m=(lo+hi)>>>1;if(rows[m].t<=t)lo=m+1;else hi=m;}const i=lo-1;if(i<0)return null;const a=rows[i],b=rows[i+1];if(!b)return t-a.t<=maxGap?a.v:null;if(b.t-a.t>maxGap)return t-a.t<=maxGap?a.v:null;return a.v+(b.v-a.v)*(t-a.t)/Math.max(1,b.t-a.t);}
function channelRows(p,ch,derived){
 if(ch.derived==='gps_agl')return derived.agl;if(ch.derived==='gps_speed')return derived.speed;if(!ch.binding)return [];
 const rows=p.__series?.get(ch.binding.data_type)?.get(ch.binding.sender_id)||[],out=[];
 for(const r of rows){let v;if(ch.vector){const xs=r.values.slice(0,ch.vector);v=xs.every(finite)?Math.hypot(...xs):null;}else v=r.values[ch.binding.index];if(finite(v))out.push({t:r.timestamp_ms,v});}
 return out;
}
function channelValue(rows,ch,at){
 if(!rows.length)return null;
 if(ch.derived)return interpolate(rows,at);
 let lo=0,hi=rows.length;while(lo<hi){const m=(lo+hi)>>>1;if(rows[m].t<=at)lo=m+1;else hi=m;}const r=rows[lo-1];return r&&at-r.t<=5000?r.v:null;
}
function formatChannel(ch,v){
 if(!finite(v))return '—';
 if(ch.format==='gps'){const n=Math.round(v);return n>=3?'3D fix':n===2?'No fix':n===1?'Silent':'No response';}
 if(ch.format==='sats')return Math.round(v)+' sats';
 const a=Math.abs(v),text=a>=1000?v.toFixed(0):a>=100?v.toFixed(1):v.toFixed(2);return ch.unit?text+' '+ch.unit:text;
}
function downsample(rows,max=360){
 if(rows.length<=max)return rows;const out=[],bucket=Math.ceil(rows.length/(max/2));
 for(let i=0;i<rows.length;i+=bucket){let lo=rows[i],hi=rows[i];for(let j=i;j<Math.min(rows.length,i+bucket);j++){if(rows[j].v<lo.v)lo=rows[j];if(rows[j].v>hi.v)hi=rows[j];}if(lo.t<=hi.t)out.push(lo,hi);else out.push(hi,lo);}
 return out;
}
// Predicted when the replay or its bundle says so (a flight plan played like a recording).
function predictedInfo(p,extras={}){
 const predicted=p?.predicted===true||extras?.manifest?.predicted===true||extras?.flight_plan?.predicted===true;
 const label=typeof p?.label==='string'&&p.label?p.label:typeof extras?.manifest?.label==='string'&&extras.manifest.label?extras.manifest.label:extras?.flight_plan?.label?'Predicted: '+extras.flight_plan.label:'Predicted flight';
 return {predicted:!!predicted,label:predicted?(/^predicted/i.test(label)?label:'Predicted: '+label):''};
}
function normalizeStations(value){
 const out=[];const push=(id,label,s)=>{if(!s||typeof s!=='object')return;const lat=s.lat??s.latitude??s.lat_deg,lon=s.lon??s.longitude??s.lon_deg,alt=s.alt??s.altitude_m??s.alt_msl_m;if(finite(lat)&&finite(lon)&&Math.abs(lat)<=90&&Math.abs(lon)<=180)out.push({id,label,lat,lon,alt:finite(alt)?alt:null});};
 if(Array.isArray(value))for(const [i,s] of value.entries())push(String(s?.id??'station-'+i),String(s?.label??s?.name??s?.id??'Station'),s);
 else if(value&&typeof value==='object'){push('ground_station','Ground station',value.ground_station);push('rrls','RRLS',value.rrls);if(Array.isArray(value.stations))out.push(...normalizeStations(value.stations));}
 return out.slice(0,8);
}
function planOverlays(extras={}){
 const list=[];if(extras.flight_plan?.overlay)list.push(extras.flight_plan.overlay);
 return list.concat(Array.isArray(extras.overlays)?extras.overlays:[]).filter(o=>o&&typeof o==='object');
}
// A flight model of a validated replay: everything the player derives once.
function flightModel(p,extras={}){
 const channels=discoverChannels(p),gps=gpsTrack(p,extras),altitude=channels.find(c=>c.role==='altitude');
 const measuredAltitude=altitude?.binding?channelRows(p,altitude,{}):[],height=heightTrack(gps,altitude,measuredAltitude);
 const track={...gps,points:height.points,heightSource:height.source},derived=trackDerived(track);
 const rows=new Map(channels.map(ch=>[ch.role,channelRows(p,ch,derived)]));
 const timeline=flightTimeline(p,extras,{altitudeRows:rows.get('altitude')});
 const frames=track.points.map(q=>q.t);if(frames.length<2)for(const r of p.samples.slice(0,20000))frames.push(r.timestamp_ms);
 const max=role=>{let m=-Infinity;for(const r of rows.get(role)||[])if(r.v>m)m=r.v;return finite(m)?m:null;};
 return {channels,track,derived,rows,timeline,frames,predicted:predictedInfo(p,extras),stations:normalizeStations(extras.stations),overlays:planOverlays(extras),max:{thrust:max('thrust'),chamber:max('chamber')}};
}
const svgNS='http://www.w3.org/2000/svg';
function node(tag,text){const e=document.createElement(tag);if(text!==undefined)e.textContent=text;return e;}
function svg(tag,attrs){const e=document.createElementNS(svgNS,tag);for(const [k,v]of Object.entries(attrs))e.setAttribute(k,String(v));return e;}
function button(text,label,handler){const b=node('button',text);b.type='button';if(label)b.setAttribute('aria-label',label);b.title=label||text;b.onclick=handler;b.className='gs-fr-button';return b;}
// Layout rules for the player; phones get a compact HUD and camera bar.
const PLAYER_CSS=`.gs-flight-replay .gs-fr-button,.gs-flight-replay .gs-fr-controls>button,.gs-flight-replay .gs-fr-controls>select{min-height:36px;min-width:36px;padding:4px 10px;border-radius:8px;border:1px solid #475569;background:#1e293b;color:#e2e8f0;font:inherit;cursor:pointer}
.gs-flight-replay .gs-fr-controls>button:disabled{opacity:.5;cursor:default}
.gs-flight-hud{position:absolute;left:8px;top:8px;z-index:3;max-width:min(270px,48%);padding:8px 10px;border-radius:10px;background:#0b1220d9;color:#e2e8f0;font:12px/1.35 system-ui;pointer-events:none;font-variant-numeric:tabular-nums}
.gs-fr-predicted{position:absolute;bottom:26px;left:50%;transform:translateX(-50%);z-index:4;padding:4px 12px;border-radius:999px;background:#7c3aedee;color:#fff;font:700 13px system-ui;letter-spacing:.06em;pointer-events:none;white-space:nowrap;max-width:80%;overflow:hidden;text-overflow:ellipsis}
.gs-fr-cameras{position:absolute;right:8px;top:8px;z-index:4;max-width:46%;display:flex;flex-wrap:wrap;gap:4px;justify-content:flex-end}
.gs-fr-cameras .gs-fr-button{min-height:32px;font-size:12px;padding:3px 8px}
@media (max-width:560px){.gs-flight-hud{font-size:11px;max-width:52%;padding:6px 8px}.gs-flight-hud .gs-fr-absent{display:none}.gs-fr-clock{font-size:17px!important}.gs-fr-cameras{max-width:44%}.gs-fr-cameras .gs-fr-button{min-height:28px;font-size:11px;padding:2px 6px}}`;
const CHART_ROLES=['altitude','velocity','acceleration','chamber','tank','thrust','battery','radio'];
function mount(root,input,options={}){
 const live=options.live===true;
 const p=validate(input,{allowEmpty:live});root.replaceChildren();root.style.overflowWrap='anywhere';root.className=((root.className||'').replace(/\bgs-flight-replay\b/g,'')+' gs-flight-replay').trim();
 const css=node('style',PLAYER_CSS);root.append(css);
 const extras={...(p.replay_extras&&typeof p.replay_extras==='object'?p.replay_extras:{}),...(options.extras||{})};
 let model=flightModel(p,extras);
 const flightMode=p.mode!=='test_fire';
 const title=node('h3',p.mode==='test_fire'?'Test-fire replay · motor on stand':live?'Flight replay · live':model.predicted.predicted?'Flight replay · predicted flight plan':'Flight replay · saved data');root.append(title);
 const badge=node('span');badge.style.cssText='display:none;margin-left:10px;padding:2px 10px;border-radius:999px;background:#7c3aed;color:#fff;font-size:13px;letter-spacing:.08em;vertical-align:middle';badge.dataset.predicted='false';title.append(badge);
 root.append(node('p',`${p.recording||(live?'Live telemetry':'Imported recording')} · ${live?'live':'offline'} · ${p.configuration_provenance||'configuration saved at export'}; no live values or commands`));
 const controls=node('div');controls.style.cssText='display:flex;gap:8px;flex-wrap:wrap;align-items:center';controls.className='gs-fr-controls';controls.setAttribute('role','toolbar');controls.setAttribute('aria-label','Replay time controls');root.append(controls);
 const play=node('button','Play'),rewind=node('button','Rewind'),resetView=node('button','Reset 3D view'),speed=node('select');resetView.disabled=true;speed.setAttribute('aria-label','Replay speed');
 for(const n of SPEEDS){const o=node('option',n+'×');o.value=n;o.selected=n===1;speed.append(o);}
 const stepBack=button('⏮','Step back one frame',()=>{clock.stepFrame(-1);following=false;render();}),stepFwd=button('⏭','Step forward one frame',()=>{clock.stepFrame(1);following=false;render();});
 const prevEvent=button('◀ Event','Previous event',()=>{clock.stepEvent(-1);following=false;render();}),nextEvent=button('Event ▶','Next event',()=>{clock.stepEvent(1);following=false;render();});
 const slowLabel=node('label'),slow=node('input');slow.type='checkbox';slow.setAttribute('aria-label','Automatic slow motion around events');slow.onchange=()=>{clock.autoSlow=!!slow.checked;};slowLabel.append(slow,node('span',' Slow-mo at events'));
 const loopA=button('A','Set loop start here',()=>{clock.setLoop('a');renderTimeline();}),loopB=button('B','Set loop end here',()=>{clock.setLoop('b');renderTimeline();}),loopClear=button('No loop','Clear the A/B loop',()=>{clock.clearLoop();renderTimeline();});
 controls.append(play,rewind,stepBack,stepFwd,prevEvent,nextEvent,speed,slowLabel,loopA,loopB,loopClear,resetView);
 const goLive=live?button('Go live','Follow the newest data',()=>{following=true;clock.seek(clock.end);render();}):null;if(goLive)controls.append(goLive);
 const slider=node('input');slider.type='range';slider.min=p.start_ms;slider.max=Math.max(p.end_ms,p.start_ms+1);slider.step=1;slider.value=p.start_ms;slider.setAttribute('aria-label','Offline replay position');slider.style.width='100%';root.append(slider);
 // The flight-state timeline: phase segments, event markers, the loop and the cursor.
 const timeline=node('div');timeline.className='gs-flight-timeline';timeline.setAttribute('role','group');timeline.setAttribute('aria-label','Flight-state timeline');timeline.style.cssText='position:relative;height:30px;margin:4px 0 2px;border-radius:6px;overflow:hidden;background:#0f172a;cursor:pointer';root.append(timeline);
 const jumps=node('div');jumps.setAttribute('role','group');jumps.setAttribute('aria-label','Jump to event');jumps.style.cssText='display:flex;gap:6px;flex-wrap:wrap;margin:4px 0';root.append(jumps);
 const stamp=node('output');stamp.setAttribute('aria-live','off');root.append(stamp);
 const host=node('div');host.setAttribute('role','img');host.setAttribute('aria-label',p.mode==='test_fire'?'Test-fire motor stand and named valve states':'Flight vehicle and named valve states');host.style.cssText='position:relative;width:100%;height:440px;max-height:68vh;min-height:300px;background:#0f172a;border-radius:12px;overflow:hidden';root.append(host);
 const scene=svg('svg',{viewBox:'0 0 800 410','aria-hidden':'true'});scene.style.cssText='width:100%;height:100%';host.append(scene);
 // HUD over the 3D view.
 const hud=node('div');hud.className='gs-flight-hud';hud.setAttribute('role','status');hud.setAttribute('aria-label','Replay readout');
 const hudClock=node('div','T --:--');hudClock.className='gs-fr-clock';hudClock.style.cssText='font:700 22px/1.1 ui-monospace,monospace;letter-spacing:.02em';const hudPhase=node('div','—');hudPhase.style.cssText='display:inline-block;margin:4px 0 6px;padding:1px 8px;border-radius:999px;font-weight:700;color:#0b1220;background:#64748b';
 const hudRows=node('div');hudRows.style.cssText='display:grid;grid-template-columns:auto auto;column-gap:10px;row-gap:1px';hud.append(hudClock,hudPhase,hudRows);
 const hudCells=new Map();
 const predictedTag=node('div');predictedTag.className='gs-fr-predicted';predictedTag.style.display='none';predictedTag.dataset.predictedLabel='true';
 const cameras=node('div');cameras.className='gs-fr-cameras';cameras.setAttribute('role','group');cameras.setAttribute('aria-label','Camera mode');cameras.style.display='none';
 const sceneNote=node('div');sceneNote.style.cssText='position:absolute;left:8px;right:8px;bottom:6px;z-index:3;color:#cbd5e1;font:11px system-ui;text-shadow:0 1px 2px #000;pointer-events:none';
 host.append(hud,predictedTag,cameras,sceneNote);
 const omittedNote=[p.valve_mappings.length>16?'first 16 valves shown in 3D':null,[...(p.view_config.ground_systems||[]),...(p.view_config.stages||[]).flatMap(s=>s.components||[])].filter(c=>c.kind==='tank').length>8?'first 8 tanks shown in 3D':null].filter(Boolean).join(' · ');
 const noteSuffix=' · tank bars show configured fill/mass range'+(omittedNote?' · '+omittedNote+'; all saved values listed below':'');
 const visualNote=node('p','Illustrative geometry · missing measurements stay unknown · loading 3D…'+noteSuffix);root.append(visualNote);
 const charts=node('div');charts.className='gs-flight-charts';charts.setAttribute('role','group');charts.setAttribute('aria-label','Time-synced sensor charts');charts.style.cssText='display:grid;grid-template-columns:repeat(auto-fit,minmax(min(100%,230px),1fr));gap:8px;margin:8px 0';root.append(charts);
 const shape=svg('g',{});scene.append(shape);
 if(p.mode==='test_fire'){
  shape.append(svg('path',{d:'M100 290 L310 290 M135 290 L150 200 L265 200 L280 290',stroke:'#94a3b8','stroke-width':10,fill:'none'}));
  shape.append(svg('rect',{x:130,y:140,width:150,height:55,rx:15,fill:'#cbd5e1'}));shape.append(svg('path',{d:'M280 145 L310 155 L310 180 L280 190 Z',fill:'#64748b'}));
 }else{
  shape.append(svg('path',{d:'M205 70 L235 125 L235 280 L260 310 L225 300 L215 325 L195 325 L185 300 L150 310 L175 280 L175 125 Z',fill:'#cbd5e1',stroke:'#38bdf8','stroke-width':3}));
 }
 const visualLabel=svg('text',{x:30,y:365,fill:'#cbd5e1','font-size':14});visualLabel.textContent='Illustrative geometry · missing measurements stay unknown';scene.append(visualLabel);
 const rows=node('div');rows.style.cssText='display:grid;grid-template-columns:repeat(auto-fit,minmax(220px,1fr));gap:8px';root.append(rows);
 const clock=createClock({start:p.start_ms,end:p.end_ms,events:model.timeline.events,frames:model.frames});
 let last=performance.now(),timer,visual=null,visuals=null,disposed=false,visualState=null,visualUnavailable=false,flightScene=false,following=live,chartCursors=[],lastTrackBuild=0,pendingTrack=false;
 // Readouts built once per model; values update each frame.
 function buildReadouts(){
  hudRows.replaceChildren();hudCells.clear();
  for(const ch of model.channels){const name=node('span',ch.label),value=node('span','—');if(ch.absent){name.className='gs-fr-absent';value.className='gs-fr-absent';}name.style.color=ch.absent?'#64748b':'#94a3b8';value.style.cssText='text-align:right;font-weight:600'+(ch.absent?';color:#64748b':'');name.title=ch.source||'not recorded';hudRows.append(name,value);hudCells.set(ch.role,value);}
  const pred=model.predicted;badge.textContent=pred.predicted?'PREDICTED':'';badge.style.display=pred.predicted?'inline-block':'none';badge.dataset.predicted=String(pred.predicted);
  predictedTag.textContent=pred.label;predictedTag.style.display=pred.predicted?'block':'none';root.dataset.predicted=String(pred.predicted);
  buildCharts();buildJumps();renderTimeline();
 }
 const pct=t=>((t-p.start_ms)/Math.max(1,p.end_ms-p.start_ms)*100).toFixed(3)+'%';
 function renderTimeline(){
  timeline.replaceChildren();
  for(const s of model.timeline.segments){const seg=node('div');seg.dataset.phase=s.phase;seg.title=PHASE_LABELS[s.phase]||s.phase;seg.style.cssText=`position:absolute;top:0;bottom:0;left:${pct(s.from)};width:calc(${pct(s.to)} - ${pct(s.from)});background:${PHASE_COLORS[s.phase]||PHASE_COLORS.unknown};opacity:.85`;timeline.append(seg);}
  if(clock.loopActive()){const l=node('div');l.dataset.loop='true';l.style.cssText=`position:absolute;top:0;bottom:0;left:${pct(clock.loop.a)};width:calc(${pct(clock.loop.b)} - ${pct(clock.loop.a)});background:#ffffff33;border-left:2px solid #fff;border-right:2px solid #fff`;timeline.append(l);}
  for(const e of model.timeline.events){const m=node('button');m.type='button';m.dataset.event=e.kind;m.setAttribute('aria-label',`Jump to ${e.label}`);m.title=`${e.label} · ${formatTClock(e.t-(model.timeline.t0??p.start_ms))}${e.source&&e.source!=='FLIGHT_STATE'?' · '+e.source:''}`;
   m.style.cssText=`position:absolute;top:2px;width:14px;height:26px;margin-left:-7px;left:${pct(e.t)};padding:0;border-radius:4px;border:2px solid ${e.kind==='apogee'?'#0b1220':'#0b1220'};background:${e.kind==='apogee'?'#ffffff':PHASE_COLORS[e.phase]||'#e2e8f0'};cursor:pointer`;
   m.onclick=ev=>{ev?.stopPropagation?.();clock.seek(e.t);following=false;render();};timeline.append(m);}
  cursor=node('div');cursor.style.cssText='position:absolute;top:0;bottom:0;width:2px;background:#fff;box-shadow:0 0 0 1px #0b1220;pointer-events:none';timeline.append(cursor);
  timeline.onclick=ev=>{const r=timeline.getBoundingClientRect?.();if(!r||!r.width)return;clock.seek(p.start_ms+(ev.clientX-r.left)/r.width*(p.end_ms-p.start_ms));following=false;render();};
  if(cursor)cursor.style.left=pct(clock.t);
 }
 let cursor=null;
 function buildJumps(){
  jumps.replaceChildren();
  for(const kind of JUMP_EVENTS){const e=model.timeline.events.find(x=>x.kind===kind);if(!e)continue;const b=button(`${e.label} ${formatTClock(e.t-(model.timeline.t0??p.start_ms))}`,`Jump to ${e.label}`,()=>{clock.jump(kind);following=false;render();});b.dataset.jump=kind;b.style.borderColor=kind==='apogee'?'#ffffff':PHASE_COLORS[e.phase];jumps.append(b);}
 }
 function buildCharts(){
  charts.replaceChildren();chartCursors=[];
  for(const role of CHART_ROLES){
   const ch=model.channels.find(c=>c.role===role),data=model.rows.get(role)||[];if(!ch||data.length<2)continue;
   const box=node('figure');box.style.cssText='margin:0;padding:6px 8px;border:1px solid #334155;border-radius:8px;background:#0b1220;color:#cbd5e1;font:11px system-ui';box.dataset.chart=role;
   const cap=node('figcaption',`${ch.label}${ch.unit?' ('+ch.unit+')':''}`);cap.style.cssText='display:flex;justify-content:space-between;gap:6px';const now=node('span','—');now.style.fontWeight='700';cap.append(now);box.append(cap);
   const pts=downsample(data);let lo=Infinity,hi=-Infinity;for(const r of pts){lo=Math.min(lo,r.v);hi=Math.max(hi,r.v);}if(hi-lo<1e-9){hi+=1;lo-=1;}
   const W=300,H=70,x=t=>((t-p.start_ms)/Math.max(1,p.end_ms-p.start_ms)*W).toFixed(1),y=v=>(H-2-(v-lo)/(hi-lo)*(H-4)).toFixed(1);
   const plot=svg('svg',{viewBox:`0 0 ${W} ${H}`,preserveAspectRatio:'none',role:'img','aria-label':`${ch.label} over the replay`});plot.style.cssText='display:block;width:100%;height:64px;cursor:pointer';
   for(const s of model.timeline.segments)plot.append(svg('rect',{x:x(s.from),y:0,width:Math.max(0,x(s.to)-x(s.from)),height:H,fill:PHASE_COLORS[s.phase]||PHASE_COLORS.unknown,opacity:.12}));
   plot.append(svg('polyline',{points:pts.map(r=>x(r.t)+','+y(r.v)).join(' '),fill:'none',stroke:'#e2e8f0','stroke-width':1.4,'vector-effect':'non-scaling-stroke'}));
   const line=svg('line',{x1:0,x2:0,y1:0,y2:H,stroke:'#f8fafc','stroke-width':1.5,'vector-effect':'non-scaling-stroke'});plot.append(line);
   plot.onclick=ev=>{const r=plot.getBoundingClientRect?.();if(!r||!r.width)return;clock.seek(p.start_ms+(ev.clientX-r.left)/r.width*(p.end_ms-p.start_ms));following=false;render();};
   box.append(plot);charts.append(box);chartCursors.push({role,ch,line,now,x});
  }
 }
 function setCameraButtons(){
  cameras.replaceChildren();if(!flightScene){cameras.style.display='none';return;}cameras.style.display='flex';
  for(const [i,mode] of visuals.CAMERA_MODES.entries()){if(mode==='station'&&!model.stations.length)continue;const b=button(visuals.CAMERA_LABELS[mode],`${visuals.CAMERA_LABELS[mode]} camera (key ${i+1})`,()=>{visual.setCameraMode(mode);markCamera();});b.dataset.camera=mode;cameras.append(b);}
  markCamera();
 }
 function markCamera(){for(const b of cameras.children){const on=b.dataset.camera===visual?.cameraMode;b.setAttribute('aria-pressed',String(on));b.style.background=on?'#0ea5e9':'#1e293bdd';}}
 function updateVisualNote(){
  if(!visual)return;
  if(flightScene){const terrainText=visual.terrainSource==='pad-altitude'?'ground at the pad altitude (no terrain data)':visual.terrainSource==='generic'?'reference ground':'terrain'+(visual.terrainAttribution?' · '+visual.terrainAttribution:'');
   visualNote.textContent=visualUnavailable?'3D context lost · saved text values remain available':`Vehicle from the station geometry at its ${model.predicted.predicted?'predicted':'recorded GPS'} position · ${terrainText} · attitude ${visualState?.attitudeSource||'reference'}${visual.software?' · reduced-detail 3D':''}${model.track.sampled?' · sampled path':''}`+noteSuffix;
   sceneNote.textContent=terrainText;return;}
  visualNote.textContent=visualUnavailable?'3D context lost · saved text values remain available':
   'Illustrative '+(p.mode==='test_fire'?'fixed motor stand':visualState?.attitude.every(finite)?'flight vehicle · recorded attitude':'flight vehicle · attitude unavailable; reference orientation')+' · drag/arrow keys to orbit'+(visual.software?' · reduced-detail 3D':'')+noteSuffix;
 }
 function trackForScene(){return model.track.points;}
 function mountVisual(){
  const useFlight=flightMode&&model.track.points.length>=2&&typeof visuals.mountFlightScene==='function';
  const onStatus=status=>{visualUnavailable=status==='lost';host.dataset.renderer=visualUnavailable?'unavailable':visual?.software?'canvas3d':'webgl';updateVisualNote();};
  if(useFlight){
   const geometry=p.vehicle_geometry||extras.vehicle_geometry;
   visual=visuals.mountFlightScene(visuals.T,visuals.SoftwareRenderer,host,{geometry,timeline:model.timeline,onStatus,onTerrain:()=>updateVisualNote(),onCamera:()=>markCamera(),railLength_m:extras.flight_plan?.inputs?.launch?.rail_length_m,loadTerrain:options.loadTerrain,fetchJson:options.fetchJson});
   flightScene=true;visual.setStations(model.stations);visual.setOverlays(model.overlays);visual.setTrack(trackForScene(),null,{timeline:model.timeline});
  }else{
   visual=visuals.mountReplayCanvas(visuals.T,visuals.SoftwareRenderer,host,p.mode==='test_fire'?'test_fire':'flight',onStatus,p.vehicle_geometry);flightScene=false;
  }
  host.dataset.renderer=visual.software?'canvas3d':'webgl';host.dataset.scene=flightScene?'flight':p.mode==='test_fire'?'test_fire':'assembly';
  // The canvas goes under the HUD and controls.
  if(host.children&&host.firstChild!==visual.canvas&&typeof host.insertBefore==='function')host.insertBefore(visual.canvas,host.firstChild);
  setCameraButtons();
 }
 resetView.onclick=()=>visual?.reset();
 if(typeof globalThis.GSLoadReplay3D==='function')globalThis.GSLoadReplay3D().then(loaded=>{
  if(disposed||!root.isConnected)return;visuals=loaded;mountVisual();scene.remove();resetView.disabled=false;updateVisualNote();render();
 }).catch(error=>{if(!disposed){host.dataset.renderer='schematic';host.dataset.error=String(error?.message||error).slice(0,200);visualNote.textContent='3D unavailable · schematic preview and saved values remain available';}});
 else{host.dataset.renderer='schematic';visualNote.textContent='3D package unavailable · schematic preview and saved values remain available';}
 function flightFrame(f,at,phase){
  const thrust=channelValue(model.rows.get('thrust')||[],model.channels.find(c=>c.role==='thrust'),at),chamber=channelValue(model.rows.get('chamber')||[],model.channels.find(c=>c.role==='chamber'),at);
  const standBurning=p.mode==='test_fire'?(finite(thrust)&&model.max.thrust>0?thrust>model.max.thrust*.05:finite(chamber)&&model.max.chamber>0?chamber>model.max.chamber*.05:false):undefined;
  const plume=visuals?.plumeScale?visuals.plumeScale({phase,burning:standBurning,thrust,thrustMax:model.max.thrust,chamber,chamberMax:model.max.chamber}):{scale:phase==='boost'?1:0,source:'phase'};
  const v=visuals?.phaseVisual?visuals.phaseVisual(phase,{plume:plume.scale}):{phase,plume:plume.scale,drogue:['drogue','main'].includes(phase),main:phase==='main'};
  if(p.mode==='test_fire')v.plume=plume.scale;
  return {visual:v,plume};
 }
 function render(){
  const at=clock.t,f=state(p,at),phase=model.timeline.source==='none'&&!model.timeline.events.length?'unknown':phaseAt(model.timeline,at);
  slider.value=at;if(cursor)cursor.style.left=pct(at);
  const t0=model.timeline.t0,tclock=formatTClock(finite(t0)?at-t0:NaN),phaseLabel=model.timeline.source==='none'&&!model.timeline.events.length?f.phase:PHASE_LABELS[phase]||phase;
  stamp.textContent=`${new Date(at).toISOString()} · ${((at-p.start_ms)/1000).toFixed(2)} s · ${tclock} · ${phaseLabel}${model.predicted.predicted?' · PREDICTED':''}${clock.playing&&clock.effectiveSpeed()<clock.speed?' · slow motion':''}`;
  hudClock.textContent=tclock;hudPhase.textContent=phaseLabel;hudPhase.style.background=PHASE_COLORS[phase]||PHASE_COLORS.unknown;hudPhase.dataset.phase=phase;
  for(const ch of model.channels){const cell=hudCells.get(ch.role);if(cell)cell.textContent=formatChannel(ch,channelValue(model.rows.get(ch.role)||[],ch,at));}
  for(const c of chartCursors){const x=c.x(at);c.line.setAttribute('x1',x);c.line.setAttribute('x2',x);c.now.textContent=formatChannel(c.ch,channelValue(model.rows.get(c.role)||[],c.ch,at));}
  const {visual:flightVisual,plume}=flightFrame(f,at,phase);
  visualState={...f,attitude:[f.attitude[1],f.attitude[2],f.attitude[0]],flight:flightVisual};
  if(visual){
   if(flightScene){const result=visual.update({t:at,phase,visual:flightVisual,recordedAttitude:visualState.attitude.every(finite)?visualState.attitude:null});visualState.attitudeSource=result?.attitudeSource;}
   else visual.update(visualState);
  }
  updateVisualNote();
  // Recorded roll only; neutral artwork is explicitly labelled when missing.
  shape.setAttribute('transform',p.mode==='test_fire'?'':`rotate(${f.attitude[0]??0} 205 200)`);
  for(const old of [...scene.querySelectorAll('[data-valve]')])old.remove();rows.replaceChildren();
  for(const [i,v]of f.valves.entries()){
   const g=svg('g',{'data-valve':i,transform:`translate(390 ${40+i*44})`});
   g.append(svg('circle',{cx:0,cy:0,r:12,fill:replayValveState(v.value,v)==='open'?'#22c55e':replayValveState(v.value,v)==='closed'?'#f59e0b':'#64748b'}));
   const label=svg('text',{x:20,y:4,fill:'white','font-size':13});label.textContent=v.label.length>38?v.label.slice(0,37)+'…':v.label;g.setAttribute('aria-label',v.label+': '+v.state);g.append(label);
   const status=svg('text',{x:20,y:24,fill:'#cbd5e1','font-size':13});status.textContent=v.state;g.append(status);scene.append(g);
   const card=node('div',`${v.label}: ${v.state}`);card.dataset.valve=v.label;card.style.cssText='border:1px solid #64748b;padding:8px;border-radius:6px';rows.append(card);
  }
  for(const c of f.components){const card=node('div');card.textContent=`${c.label||c.id}: ${c.value===null?'Unknown':c.value.toFixed(2)+' '+(c.unit||'')}`;
   if(c.kind==='tank'){
    const fraction=tankFillFraction(c);
    if(fraction===null){const unknown=node('span',' · Tank fill: Unknown');unknown.dataset.tankFill='unknown';card.append(unknown);}
    else{const fill=node('progress');fill.max=1;fill.value=fraction;fill.dataset.tankFill='known';fill.setAttribute('aria-label',`${c.label||c.id}: configured ${['kg','g'].includes(c.unit.trim().toLowerCase())?'mass':'fill'} range`);card.append(fill);}
   }
   rows.append(card);
  }
  play.textContent=clock.playing?'Pause':'Play';
  root.dataset.timestamp=at;root.dataset.loaded='true';root.dataset.phase=phase;root.dataset.plume=String(plume.scale);root.dataset.tclock=tclock;
 }
 play.onclick=()=>{clock.toggle();following=false;last=performance.now();render();};
 rewind.onclick=()=>{clock.pause();following=false;clock.seek(p.start_ms);render();};slider.oninput=()=>{clock.pause();following=false;clock.seek(Number(slider.value));render();};
 speed.onchange=()=>{clock.setSpeed(Number(speed.value));};
 timer=setInterval(()=>{if(!root.isConnected){disposed=true;clearInterval(timer);visual?.dispose();return;}const now=performance.now();if(clock.playing){clock.tick(Math.max(0,now-last));render();}else if(live&&pendingTrack&&now-lastTrackBuild>2000)render();last=now;},50);
 buildReadouts();render();
 // Live: new rows join the replay; following keeps the cursor on the newest data.
 function append(newRows){
  if(!Array.isArray(newRows)||!newRows.length)return 0;let added=0,nextId=(p.samples.at(-1)?.id??0)+1,gps=false,states=false;
  for(const r of newRows.slice(0,5000)){
   if(!r||!Number.isSafeInteger(r.timestamp_ms)||typeof r.data_type!=='string'||!Array.isArray(r.values))continue;if(p.samples.length&&r.timestamp_ms<p.samples.at(-1).timestamp_ms-60000)continue;
   const row={id:nextId++,timestamp_ms:r.timestamp_ms,data_type:r.data_type,sender_id:String(r.sender_id||''),values:r.values.map(v=>finite(v)?v:null),phase:typeof r.phase==='string'?r.phase:undefined};
   p.samples.push(row);let sources=p.__series.get(row.data_type);if(!sources){sources=new Map();p.__series.set(row.data_type,sources);}let list=sources.get(row.sender_id);if(!list){list=[];sources.set(row.sender_id,list);}list.push(row);added++;
   if(/GPS/.test(row.data_type))gps=true;if(row.data_type==='FLIGHT_STATE')states=true;
  }
  if(p.samples.length>MAX_ROWS){const drop=p.samples.length-MAX_ROWS;p.samples.splice(0,drop);validateSeries(p);}
  if(!added)return 0;p.start_ms=p.samples[0].timestamp_ms;p.end_ms=p.samples.at(-1).timestamp_ms;slider.min=p.start_ms;slider.max=Math.max(p.end_ms,p.start_ms+1);
  model=flightModel(p,extras);clock.start=p.start_ms;clock.end=p.end_ms;clock.events=model.timeline.events;clock.frames=model.frames;if(following)clock.seek(clock.end);
  const now=performance.now();
  if(states||gps){if(states)visual?.setTimeline?.(model.timeline);if(gps&&flightScene&&now-lastTrackBuild>2000){visual.setTrack(trackForScene(),null,{timeline:model.timeline});lastTrackBuild=now;pendingTrack=false;}else if(gps)pendingTrack=true;}
  if(!flightScene&&visuals&&model.track.points.length>=2&&flightMode){visual?.dispose();mountVisual();}
  buildReadouts();render();return added;
 }
 return {package:p,get visual(){return visual;},get model(){return model;},clock,seek:n=>{clock.seek(n);following=false;render();},
  play:()=>{clock.play();render();},pause:()=>{clock.pause();render();},setSpeed:s=>{clock.setSpeed(s);speed.value=String(SPEEDS.find(v=>v>=clock.speed)??20);},
  jump:kind=>{const e=clock.jump(kind);following=false;render();return e;},stepEvent:d=>{const e=clock.stepEvent(d);following=false;render();return e;},stepFrame:d=>{clock.stepFrame(d);following=false;render();},
  setLoop:(w,t)=>{clock.setLoop(w,t);renderTimeline();},clearLoop:()=>{clock.clearLoop();renderTimeline();},
  setCameraMode:m=>{const r=visual?.setCameraMode?.(m);markCamera();return r;},
  setOverlays:list=>{extras.overlays=Array.isArray(list)?list:[];model.overlays=planOverlays(extras);visual?.setOverlays?.(model.overlays);},
  setStations:list=>{extras.stations=list;model.stations=normalizeStations(list);visual?.setStations?.(model.stations);if(visuals)setCameraButtons();},
  append,dispose:()=>{disposed=true;clearInterval(timer);visual?.dispose();}};
}
function validateSeries(p){const series=new Map();for(const r of p.samples){let s=series.get(r.data_type);if(!s){s=new Map();series.set(r.data_type,s);}let l=s.get(r.sender_id);if(!l){l=[];s.set(r.sender_id,l);}l.push(r);}Object.defineProperty(p,'__series',{value:series,configurable:true});}
globalThis.GSOfflineReplay={MAX_BYTES,LEGACY_NAMES,validate,value,state,tankFillFraction,replayValveState,mount,
 PHASE_COLORS,PHASE_LABELS,FLIGHT_STATE_NAMES,flightPhaseOf,flightTimeline,phaseAt,formatTClock,createClock,SPEEDS,discoverChannels,gpsTrack,flightModel,predictedInfo,normalizeStations,channelRows,channelValue,formatChannel,eventKindOf};
})();
