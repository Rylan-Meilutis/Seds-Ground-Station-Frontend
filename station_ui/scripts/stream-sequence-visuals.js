/* Same-frame programme evidence only. This module performs no fetch or command. */
(function(root,factory){const api=factory();root.GS26SequenceVisuals=api;if(typeof module==='object'&&module.exports)module.exports=api;})(globalThis,function(){
'use strict';
const valves=[['pilot','Pilot'],['vent','Vent'],['dump','Dump'],['nitrogen','Nitrogen'],['nitrous','Nitrous']];
function fresh(value){return value&&Number.isFinite(value.age_ms)&&value.age_ms>=0&&Number.isFinite(value.valid_for_ms)&&value.valid_for_ms>0&&value.valid_for_ms<=2000&&value.age_ms<=value.valid_for_ms;}
function signal(value){
 if(!fresh(value)||typeof value.value!=='boolean')return {level:'unknown',value:null,label:'Unknown'};
 if(value.evidence==='board_reported')return {level:'reported',value:value.value,label:value.value?'Board reports on':'Board reports off'};
 if(value.evidence==='command_accepted')return {level:'commanded',value:value.value,label:value.value?'Command accepted · requested on':'Command accepted · requested off'};
 if(value.evidence==='electrical_confirmed'&&['current_sensor','voltage_sensor'].includes(value.source_kind))return {level:'electrical',value:value.value,label:value.value?'Electrical measurement on':'Electrical measurement off'};
 if(value.evidence==='commanded')return {level:'commanded',value:value.value,label:value.value?'Command accepted · requested on':'Command accepted · requested off'};
 if(value.evidence==='planned')return {level:'planned',value:value.value,label:value.value?'Planned on':'Planned off'};
 return {level:'unknown',value:null,label:'Unknown'}; // A transport ACK is not electrical readback.
}
function flow(value){
 if(fresh(value)&&value.evidence==='planned'&&value.source_kind==='sequence_plan'&&value.active===true)return {level:'expected',value:null,active:true,label:'Planned path · flow unmeasured'};
 if(!fresh(value)||!Number.isFinite(value.value)||value.value<0)return {level:'unknown',value:null,label:'Flow unmeasured'};
 if(value.evidence==='measured'&&value.source_kind==='flow_sensor')return {level:'measured',value:value.value,label:value.value>0?'Measured flow':'Measured no flow'};
 if(value.evidence==='inferred')return {level:'expected',value:value.value,label:'Expected flow · not measured'};
 return {level:'unknown',value:null,label:'Flow unmeasured'};
}
function presentation(frame){
 const trusted=frame&&frame.version===1;
 const clock=trusted?frame.clock:null;
 const plan=!!(clock&&clock.kind==='t_minus'&&clock.advancing===true&&Number.isFinite(clock.remaining_ms)&&clock.remaining_ms>=0&&clock.remaining_ms<=5000&&frame.igniter?.planned_t_minus_ms===5000);
 const igniter=signal(trusted?frame.igniter?.output:null);
 const igniterCommand=signal(trusted?frame.igniter?.command:null);
 return {version:1,igniter:{planned:plan,output:igniter,command:igniterCommand,label:['electrical','reported'].includes(igniter.level)?igniter.label:igniterCommand.level==='commanded'?igniterCommand.label:plan?'Igniter planned · T−5':'Igniter state unknown'},
  valves:valves.map(([id,label])=>({id,label,output:signal(trusted?frame.valves?.[id]?.output:null),command:signal(trusted?frame.valves?.[id]?.command:null),plan:signal(trusted?frame.valves?.[id]?.plan:null),flow:flow(trusted?frame.valves?.[id]?.flow:null)}))};
}
const mounted=new WeakMap();
const css=`.gs26-equipment{color:#e5eef5;font:12px system-ui;min-width:0;width:100%;text-align:left}.gs26-equipment-title{display:flex;justify-content:space-between;gap:8px;font-size:10px;letter-spacing:.1em;text-transform:uppercase;color:#9eb8cc;margin-bottom:7px}.gs26-igniter{border:1px solid #516b80;background:#152a3c;border-radius:9px;padding:8px 10px;margin-bottom:8px;font-size:11px;display:flex;align-items:center;gap:8px}.gs26-igniter[data-planned=true]{border-color:#dea65b;background:#60421a42}.gs26-igniter[data-level=electrical]{border-color:#8cd6be}.gs26-igniter-label{min-width:0;overflow-wrap:anywhere}.gs26-igniter-plan{font-size:9px;white-space:nowrap;border:1px solid #916735;border-radius:4px;padding:2px 4px;color:#ffdc9d}.gs26-igniter{flex-wrap:wrap}.gs26-igniter-dot{width:9px;height:9px;flex:none;border-radius:50%;background:#8397a8}.gs26-igniter[data-planned=true] .gs26-igniter-dot{background:#f2bd6e;box-shadow:0 0 12px #f2bd6e77}.gs26-valve-lane{display:grid;grid-template-columns:62px 72px minmax(0,1fr);gap:7px;align-items:center;padding:6px 0;border-top:1px solid #7892a52b}.gs26-valve-lane svg{width:72px!important;height:24px!important;min-height:0!important;max-height:none!important;filter:none!important}.gs26-valve-name{font-size:11px;font-weight:600}.gs26-valve-status{font-size:10px;line-height:1.4}.gs26-valve-status small{display:block;color:#9eb0bf;font-size:9px}.gs26-valve-symbol{stroke:#8799a7;fill:#122538;stroke-width:2}.gs26-valve-lane[data-level=electrical] .gs26-valve-symbol{stroke:#8cd6be}.gs26-valve-lane[data-level=reported] .gs26-valve-symbol{stroke:#9fc4e4}.gs26-flow{stroke:#8799a7;stroke-width:2;fill:none}.gs26-valve-lane[data-flow=measured] .gs26-flow{stroke:#6acaf2;stroke-dasharray:5 5;animation:gs26-flow-dash 1.8s linear infinite}.gs26-valve-lane[data-flow=expected] .gs26-flow{stroke:#e4b370;stroke-dasharray:2 7;animation:gs26-flow-dash 4s linear infinite}.gs26-equipment-note{font-size:9px;line-height:1.4;color:#9eb0bf;margin-top:6px}.gs26-equipment[data-compact=true] .gs26-valve-lane,.gs26-equipment[data-compact=true] .gs26-equipment-title,.gs26-equipment[data-compact=true] .gs26-equipment-note{display:none}.gs26-equipment[data-compact=true] .gs26-igniter{font-size:9px;padding:6px;margin:5px 0 0;justify-content:center}.gs26-equipment[data-compact=true] .gs26-igniter-dot{width:5px;height:5px}@keyframes gs26-flow-dash{to{stroke-dashoffset:-20}}@media(prefers-reduced-motion:reduce){.gs26-flow{animation:none!important}}`;
function element(doc,name,className,text){const node=doc.createElement(name);if(className)node.className=className;if(text!==undefined)node.textContent=text;return node;}
function mount(container,frame,options={}){
 if(!container?.ownerDocument)return presentation(frame);
 const previous=mounted.get(container);
 if(previous&&previous.frame===frame&&previous.compact===!!options.compact)return previous.model;
 const model=presentation(frame);mounted.set(container,{frame,compact:!!options.compact,model});
 const doc=container.ownerDocument;
 if(!doc.getElementById('gs26-sequence-visuals-style')){const style=element(doc,'style');style.id='gs26-sequence-visuals-style';style.textContent=css;doc.head.append(style);}
 container.replaceChildren();container.className='gs26-equipment';container.dataset.compact=String(!!options.compact);
 const title=element(doc,'div','gs26-equipment-title');title.append(element(doc,'span',null,'GSE output reports'),element(doc,'span',null,'Same programme frame'));container.append(title);
 const igniter=element(doc,'div','gs26-igniter');igniter.dataset.planned=String(model.igniter.planned);igniter.dataset.level=model.igniter.output.level;igniter.append(element(doc,'span','gs26-igniter-dot'),element(doc,'span','gs26-igniter-label',model.igniter.label));if(model.igniter.planned)igniter.append(element(doc,'small','gs26-igniter-plan','T−5 plan'));container.append(igniter);
 for(const valve of model.valves){
  const row=element(doc,'div','gs26-valve-lane');row.dataset.valve=valve.id;row.dataset.level=valve.output.level;row.dataset.flow=valve.flow.value>0||valve.flow.active?valve.flow.level:'unknown';
  const svg=doc.createElementNS('http://www.w3.org/2000/svg','svg');svg.setAttribute('viewBox','0 0 90 30');svg.setAttribute('role','img');svg.setAttribute('aria-label',valve.label+' output: '+valve.output.label);
  // Fixed artwork only; untrusted labels/data never enter markup.
  svg.innerHTML='<path class="gs26-flow" d="M0 15H30M60 15H90"/><path class="gs26-valve-symbol" d="M30 5L60 25V5L30 25Z"/><circle class="gs26-valve-symbol" cx="45" cy="15" r="4"/>';
  const status=element(doc,'div','gs26-valve-status',valve.output.label);status.append(element(doc,'small',null,valve.command.level==='commanded'?valve.command.label:valve.plan.level==='planned'?valve.plan.label:'No command confirmation'),element(doc,'small',null,valve.flow.label));
  row.append(element(doc,'span','gs26-valve-name',valve.label),svg,status);container.append(row);
 }
 container.append(element(doc,'div','gs26-equipment-note','A board output report does not confirm ignition or valve motion. Dotted paths are planned; flow is measured only with a flow sensor.'));
 return model;
}
return {presentation,mount};
});
