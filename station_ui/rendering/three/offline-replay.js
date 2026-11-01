// Self-contained saved-file viewer. No network, live telemetry or command APIs.
(()=>{
'use strict';
const MAX_BYTES=64*1024*1024,MAX_ROWS=200000;
const finite=Number.isFinite;
function validate(input){
 if(typeof input==='string'&&new TextEncoder().encode(input).length>MAX_BYTES)throw Error('Replay exceeds 64 MiB.');
 const p=typeof input==='string'?JSON.parse(input):input;
 if(!p||p.format!=='seds-replay'||p.version!==1)throw Error('Choose a SEDS replay v1 file.');
 if(!Array.isArray(p.samples)||p.samples.length>MAX_ROWS||!p.samples.length)throw Error('Replay must contain 1–200,000 saved rows.');
 if(!p.schema||typeof p.schema!=='object'||!p.view_config||typeof p.view_config!=='object'||!p.layout||typeof p.layout!=='object'||!Array.isArray(p.valve_mappings))throw Error('Replay is missing its embedded schema, view configuration, layout or valve mappings.');
 const stages=p.view_config.stages||[],ground=p.view_config.ground_systems||[];
 if(!Array.isArray(stages)||!Array.isArray(ground)||stages.length>32||ground.length>128||stages.some(s=>!s||typeof s!=='object'||!Array.isArray(s.components)||s.components.length>128))throw Error('Invalid or too many visual components.');
 const validBinding=b=>b==null||(typeof b==='object'&&!Array.isArray(b)&&typeof b.data_type==='string'&&b.data_type.length>0&&b.data_type.length<=128&&(b.sender_id==null||(typeof b.sender_id==='string'&&b.sender_id.length<=256))&&(b.index===undefined||(Number.isInteger(b.index)&&b.index>=0&&b.index<1024))&&(b.scale==null||finite(b.scale))&&(b.offset==null||finite(b.offset)));
 if([...ground,...stages.flatMap(s=>s.components)].some(c=>!c||typeof c!=='object'||Array.isArray(c)||!validBinding(c.binding)||['id','label','kind','unit'].some(k=>c[k]!==undefined&&(typeof c[k]!=='string'||c[k].length>256))||['min','max'].some(k=>c[k]!==undefined&&c[k]!==null&&!finite(c[k]))))throw Error('Invalid saved visual component.');
 if(p.view_config.attitude!==undefined&&(!p.view_config.attitude||typeof p.view_config.attitude!=='object'||['roll','pitch','yaw'].some(k=>!validBinding(p.view_config.attitude[k]))))throw Error('Invalid saved attitude bindings.');
 const ids=new Set();
 for(const r of p.samples){
  if(!Number.isSafeInteger(r.id)||r.id<0||ids.has(r.id)||!Number.isSafeInteger(r.timestamp_ms)||r.timestamp_ms<0||r.timestamp_ms>8640000000000000||typeof r.data_type!=='string'||r.data_type.length>128||typeof r.sender_id!=='string'||r.sender_id.length>256||!Array.isArray(r.values)||r.values.length>1024||r.values.some(v=>v!==null&&!finite(v)))throw Error('Invalid saved sample; no replay data was loaded.');
  ids.add(r.id);
 }
 if(p.valve_mappings.length>128||p.valve_mappings.some(v=>!v||typeof v.label!=='string'||v.label.length>128||!validBinding(v)||!Number.isInteger(v.index)||v.index<0||v.index>1023||['true_label','false_label','unknown_label'].some(k=>v[k]!==undefined&&(typeof v[k]!=='string'||v[k].length>128))))throw Error('Invalid valve mappings.');
 p.samples=p.samples.slice().sort((a,b)=>a.timestamp_ms-b.timestamp_ms||a.id-b.id);
 const series=new Map();
 for(const r of p.samples){let sources=series.get(r.data_type);if(!sources){sources=new Map();series.set(r.data_type,sources);}let rows=sources.get(r.sender_id);if(!rows){rows=[];sources.set(r.sender_id,rows);}rows.push(r);}
 Object.defineProperty(p,'__series',{value:series,configurable:true});
 p.start_ms=p.samples[0].timestamp_ms;p.end_ms=p.samples.at(-1).timestamp_ms;
 return p;
}
function latest(p,type,sender,at){
 const sources=p.__series.get(type);if(!sources)return null;
 function previous(rows){let lo=0,hi=rows.length;while(lo<hi){const mid=(lo+hi)>>>1;if(rows[mid].timestamp_ms<=at)lo=mid+1;else hi=mid;}return lo?rows[lo-1]:null;}
 if(sender)return sources.has(sender)?previous(sources.get(sender)):null;
 let found=null;for(const rows of sources.values()){const r=previous(rows);if(r&&(!found||r.timestamp_ms>found.timestamp_ms||(r.timestamp_ms===found.timestamp_ms&&r.id>found.id)))found=r;}return found;
}
function value(p,b,at,hold=false){if(!b)return null;const r=latest(p,b.data_type,b.sender_id,at);if(!r||(!hold&&at-r.timestamp_ms>5000))return null;const v=r.values[b.index??0];if(!finite(v))return null;const n=v*(b.scale??1)+(b.offset??0);return finite(n)?n:null;}
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
const svgNS='http://www.w3.org/2000/svg';
function node(tag,text){const e=document.createElement(tag);if(text!==undefined)e.textContent=text;return e;}
function svg(tag,attrs){const e=document.createElementNS(svgNS,tag);for(const [k,v]of Object.entries(attrs))e.setAttribute(k,String(v));return e;}
function mount(root,input){
 const p=validate(input);root.replaceChildren();const title=node('h3',p.mode==='test_fire'?'Test-fire replay · motor on stand':'Flight replay · saved data');root.append(title);
 root.append(node('p',`${p.recording||'Imported recording'} · offline · configuration saved at export; no live values or commands`));
 const controls=node('div');controls.style.cssText='display:flex;gap:8px;flex-wrap:wrap;align-items:center';root.append(controls);
 const play=node('button','Play'),rewind=node('button','Rewind'),resetView=node('button','Reset 3D view'),speed=node('select');resetView.disabled=true;for(const n of [.25,.5,1,2,4]){const o=node('option',n+'×');o.value=n;o.selected=n===1;speed.append(o);}controls.append(play,rewind,speed,resetView);
 const slider=node('input');slider.type='range';slider.min=p.start_ms;slider.max=p.end_ms;slider.step=1;slider.value=p.start_ms;slider.setAttribute('aria-label','Offline replay position');slider.style.width='100%';root.append(slider);
 const stamp=node('output');stamp.setAttribute('aria-live','off');root.append(stamp);
 const host=node('div');host.setAttribute('role','img');host.setAttribute('aria-label',p.mode==='test_fire'?'Test-fire motor stand and named valve states':'Flight vehicle and named valve states');host.style.cssText='position:relative;width:100%;height:410px;max-height:65vh;background:#0f172a;border-radius:12px;overflow:hidden';root.append(host);
 const scene=svg('svg',{viewBox:'0 0 800 410','aria-hidden':'true'});scene.style.cssText='width:100%;height:100%';host.append(scene);
 const omittedNote=[p.valve_mappings.length>16?'first 16 valves shown in 3D':null,[...(p.view_config.ground_systems||[]),...(p.view_config.stages||[]).flatMap(s=>s.components||[])].filter(c=>c.kind==='tank').length>8?'first 8 tanks shown in 3D':null].filter(Boolean).join(' · ');
 const noteSuffix=' · tank bars show configured fill/mass range'+(omittedNote?' · '+omittedNote+'; all saved values listed below':'');
 const visualNote=node('p','Illustrative geometry · missing measurements stay unknown · loading 3D…'+noteSuffix);root.append(visualNote);
 const shape=svg('g',{});scene.append(shape);
 if(p.mode==='test_fire'){
  shape.append(svg('path',{d:'M100 290 L310 290 M135 290 L150 200 L265 200 L280 290',stroke:'#94a3b8','stroke-width':10,fill:'none'}));
  shape.append(svg('rect',{x:130,y:140,width:150,height:55,rx:15,fill:'#cbd5e1'}));shape.append(svg('path',{d:'M280 145 L310 155 L310 180 L280 190 Z',fill:'#64748b'}));
 }else{
  shape.append(svg('path',{d:'M205 70 L235 125 L235 280 L260 310 L225 300 L215 325 L195 325 L185 300 L150 310 L175 280 L175 125 Z',fill:'#cbd5e1',stroke:'#38bdf8','stroke-width':3}));
 }
 const visualLabel=svg('text',{x:30,y:365,fill:'#cbd5e1','font-size':14});visualLabel.textContent='Illustrative geometry · missing measurements stay unknown';scene.append(visualLabel);
 const rows=node('div');rows.style.cssText='display:grid;grid-template-columns:repeat(auto-fit,minmax(220px,1fr));gap:8px';root.append(rows);
 let playing=false,at=p.start_ms,last=performance.now(),timer,visual=null,disposed=false,visualState=null,visualUnavailable=false;
 function updateVisualNote(){
  if(!visual)return;
  visualNote.textContent=visualUnavailable?'3D context lost · saved text values remain available':
   'Illustrative '+(p.mode==='test_fire'?'fixed motor stand':visualState?.attitude.every(finite)?'flight vehicle · recorded attitude':'flight vehicle · attitude unavailable; reference orientation')+' · drag/arrow keys to orbit'+(visual.software?' · reduced-detail 3D':'')+noteSuffix;
 }
 resetView.onclick=()=>visual?.reset();
 if(typeof globalThis.GSLoadReplay3D==='function')globalThis.GSLoadReplay3D().then(({T,SoftwareRenderer,mountReplayCanvas})=>{
  if(disposed||!root.isConnected)return;
  visual=mountReplayCanvas(T,SoftwareRenderer,host,p.mode==='test_fire'?'test_fire':'flight',status=>{visualUnavailable=status==='lost';host.dataset.renderer=visualUnavailable?'unavailable':visual?.software?'canvas3d':'webgl';updateVisualNote();});
  scene.remove();host.dataset.renderer=visual.software?'canvas3d':'webgl';resetView.disabled=false;
  updateVisualNote();
  if(visualState)visual.update(visualState);
 }).catch(()=>{if(!disposed){host.dataset.renderer='schematic';visualNote.textContent='3D unavailable · schematic preview and saved values remain available';}});
 else{host.dataset.renderer='schematic';visualNote.textContent='3D package unavailable · schematic preview and saved values remain available';}
 function render(){const f=state(p,at);slider.value=at;stamp.textContent=`${new Date(at).toISOString()} · ${((at-p.start_ms)/1000).toFixed(2)} s · ${f.phase}`;
  visualState={...f,attitude:[f.attitude[1],f.attitude[2],f.attitude[0]]};visual?.update(visualState);updateVisualNote();
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
  root.dataset.timestamp=at;root.dataset.loaded='true';
 }
 play.onclick=()=>{if(at>=p.end_ms)at=p.start_ms;playing=!playing;play.textContent=playing?'Pause':'Play';last=performance.now();render();};
 rewind.onclick=()=>{playing=false;play.textContent='Play';at=p.start_ms;render();};slider.oninput=()=>{playing=false;play.textContent='Play';at=Number(slider.value);render();};
 timer=setInterval(()=>{if(!root.isConnected){disposed=true;clearInterval(timer);visual?.dispose();return;}const now=performance.now();if(playing){at=Math.min(p.end_ms,at+Math.max(0,Math.min(1000,now-last))*Number(speed.value));if(at>=p.end_ms){playing=false;play.textContent='Play';}render();}last=now;},50);
 render();return {package:p,get visual(){return visual;},seek:n=>{at=Math.max(p.start_ms,Math.min(p.end_ms,n));render();},dispose:()=>{disposed=true;clearInterval(timer);visual?.dispose();}};
}
globalThis.GSOfflineReplay={MAX_BYTES,validate,value,state,tankFillFraction,replayValveState,mount};
})();
