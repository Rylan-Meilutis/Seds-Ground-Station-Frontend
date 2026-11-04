// Recording-local viewer. No network, live state, commands, executable metadata,
// HTML insertion, popup or redirect. Wide counters remain decimal strings.
(()=>{'use strict';
const TYPES=new Set(['REC_NETWORK_STATE','REC_SIDE_COUNTERS','REC_SIDE_RATES','REC_BOARD_DIE_TEMPERATURE','REC_HOST_CPU_TEMPERATURE','REC_DIAGNOSTIC_GAP']);
function validate(p){
 if(!p||!Array.isArray(p.samples)||p.samples.length>200000)throw Error('Invalid saved diagnostic recording.');
 const sources=new Set();
 for(const r of p.samples){if(!TYPES.has(r.data_type)||r.diagnostic==null)continue;
  if(typeof r.sender_id!=='string'||r.sender_id.length>256||!Number.isSafeInteger(r.timestamp_ms)||r.timestamp_ms<0)throw Error('Invalid diagnostic identity or clock.');
  if(typeof r.diagnostic!=='object'||Array.isArray(r.diagnostic)||new TextEncoder().encode(JSON.stringify(r.diagnostic)).length>16384)throw Error('Oversized or invalid saved diagnostic payload.');
  sources.add(r.sender_id);if(sources.size>128)throw Error('Saved diagnostics exceed128 source streams; narrow the recording.');
 }
 return p;
}
function age(value){return typeof value==='number'&&Number.isFinite(value)&&value>=0?value:typeof value==='string'&&/^\d+$/.test(value)?Number(value):null;}
function resolvedNodes(d){
 if(d.topology_encoding!=='node-table-v1')return Array.isArray(d.nodes)&&d.nodes.every(n=>n&&typeof n==='object')?d.nodes:null;
 if(!Array.isArray(d.nodes)||!Array.isArray(d.node_table))return null;
 const resolve=refs=>Array.isArray(refs)&&refs.every(i=>Number.isSafeInteger(i)&&i>=0&&i<d.node_table.length&&d.node_table[i]&&typeof d.node_table[i]==='object')?refs.map(i=>d.node_table[i]):null;
 const nodes=resolve(d.nodes);if(nodes===null)return null;
 if(!Array.isArray(d.routes)||!d.routes.every(r=>Array.isArray(r.announcers)&&r.announcers.every(a=>resolve(a.router_refs)!==null)))return null;
 return nodes;
}
function resolvedContext(r,rows){
 const c=r.diagnostic?.context;if(!c||typeof c.snapshot_id!=='string')return null;
 const full=rows.filter(v=>['REC_NETWORK_STATE','REC_DIAGNOSTIC_GAP'].includes(v.data_type)&&v.diagnostic?.context?.snapshot_id===c.snapshot_id&&typeof v.diagnostic.context.recording_generation_decimal==='string').map(v=>v.diagnostic.context);
 if(!full.length)return null;const text=JSON.stringify(full[0]);return full.every(v=>JSON.stringify(v)===text)?full[0]:null;
}
function summary(r,at){
 const d=r.diagnostic;if(!d||((d.context?.version??d.version)!==1))return 'Unknown diagnostic version';
 if(r.data_type==='REC_DIAGNOSTIC_GAP')return `Capture gap: ${String(d.reason??'unavailable')}`;
 if(r.data_type==='REC_BOARD_DIE_TEMPERATURE'||r.data_type==='REC_HOST_CPU_TEMPERATURE'){
  const a=age(d.sample_age_ms);const valid=d.valid===true&&Number.isInteger(d.centi_c)&&a!==null&&a+Math.max(0,at-r.timestamp_ms)<=15000;
  return valid?`${(d.centi_c/100).toFixed(2)} °C · ${r.data_type==='REC_BOARD_DIE_TEMPERATURE'?'internal MCU die':'host CPU/package'} · sample age ${a}ms`:'Unknown / stale / unavailable temperature';
 }
 if(r.data_type==='REC_SIDE_COUNTERS'){
  const totals=d.totals_decimal;if(!totals||typeof totals!=='object')return 'Counters unavailable';
  return Object.entries(totals).map(([key,value])=>`${key}: ${typeof value==='string'&&/^\d+$/.test(value)?value:'Unknown'}`).join(' · ');
 }
 if(r.data_type==='REC_NETWORK_STATE'){
  if(d.topology_valid!==true||at-r.timestamp_ms>2000)return 'No fresh saved topology at this diagnostic cursor';
  if(!Array.isArray(d.links)||resolvedNodes(d)===null)return 'Saved topology references unavailable; no empty graph inferred';
  return d.links.length?d.links.map(l=>`${String(l.source??'Unknown')} → ${String(l.target??'Unknown')}`).join(' · '):'Observed snapshot contains no links';
 }
 return (r.values??[]).map(v=>v===null?'Unknown':String(v)).join(' · ');
}
function mount(root,input){
 const p=validate(input);root.replaceChildren();const make=(tag,text)=>{const e=document.createElement(tag);if(text!==undefined)e.textContent=text;return e;};
 root.append(make('h3','Recorded network, statistics and temperatures'));
 root.append(make('p','Saved observations only. Counter totals are exact; graph projections can round. Unknown readings are not zero.'));
 const rows=p.samples.filter(r=>TYPES.has(r.data_type)&&r.diagnostic!=null).slice().sort((a,b)=>a.timestamp_ms-b.timestamp_ms||a.id-b.id);
 if(!rows.length){root.append(make('p','No recorded diagnostics in this file. Live data is not substituted.'));return{seek(){},dispose(){root.replaceChildren();}};}
 const label=make('label','Diagnostic source '),select=make('select');select.setAttribute('aria-label','Recorded diagnostic source');select.append(new Option('All saved sources',''));
 for(const source of [...new Set(rows.map(r=>r.sender_id))].sort())select.append(new Option(source,source));label.append(select);root.append(label);
 const stamp=make('p');stamp.setAttribute('role','status');root.append(stamp);
 const slider=make('input');slider.type='range';slider.min=Number.isSafeInteger(p.start_ms)?Math.min(p.start_ms,rows[0].timestamp_ms):rows[0].timestamp_ms;slider.max=Number.isSafeInteger(p.end_ms)?Math.max(p.end_ms,rows.at(-1).timestamp_ms):rows.at(-1).timestamp_ms;slider.step=1;slider.value=slider.max;slider.style.width='100%';slider.setAttribute('aria-label','Recorded diagnostic position');root.append(slider);
 const table=make('table');table.style.cssText='width:100%;table-layout:fixed;overflow-wrap:anywhere';const head=make('thead'),header=make('tr');for(const title of ['Received UTC','Source','Stream / saved observation'])header.append(make('th',title));head.append(header);table.append(head);const body=make('tbody');table.append(body);root.append(table);
 let at=Number(slider.value);function render(){
  body.replaceChildren();stamp.textContent=new Date(at).toISOString()+' · recorded diagnostic cursor';
  const latest=new Map();for(const r of rows){if(r.timestamp_ms>at)break;if(!select.value||select.value===r.sender_id)latest.set(JSON.stringify([r.sender_id,r.data_type]),r);}
  if(latest.size>128)stamp.textContent+=' · showing first128 saved streams; select one source to narrow';
  for(const r of [...latest.values()].slice(0,128)){
   const tr=make('tr');tr.append(make('td',new Date(r.timestamp_ms).toISOString()),make('td',r.sender_id));
   const gap=rows.findLast(v=>v.timestamp_ms<=at&&v.data_type==='REC_DIAGNOSTIC_GAP'&&v.timestamp_ms>=r.timestamp_ms);
   const label=r.data_type==='REC_NETWORK_STATE'&&gap?'No fresh saved topology: later capture gap':summary(r,at);
   const cell=make('td');cell.append(make('strong',r.data_type),make('p',label));
   const details=make('details');details.append(make('summary','Saved clocks, quality and payload'));const pre=make('pre',JSON.stringify({source_timestamp_ms:r.source_timestamp_ms??null,resolved_capture_context:resolvedContext(r,rows),context_resolution:resolvedContext(r,rows)?'resolved':'Unknown / missing or ambiguous saved reference',resolved_nodes:r.data_type==='REC_NETWORK_STATE'?resolvedNodes(r.diagnostic):null,payload:r.diagnostic},null,2));pre.style.cssText='white-space:pre-wrap;overflow-wrap:anywhere';details.append(pre);cell.append(details);tr.append(cell);body.append(tr);
  }
 }
 slider.oninput=()=>{at=Number(slider.value);render();};select.onchange=render;render();
 return{seek:n=>{if(Number.isSafeInteger(n)){at=Math.max(Number(slider.min),Math.min(Number(slider.max),n));slider.value=at;render();}},dispose:()=>{root.replaceChildren();}};
}
globalThis.GSRecordedDiagnostics=Object.freeze({validate,summary,resolvedNodes,resolvedContext,mount});
})();
