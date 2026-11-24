import fs from 'node:fs';
import vm from 'node:vm';
import assert from 'node:assert/strict';
const source=fs.readFileSync(new URL('../station_ui/rendering/three/offline-replay.js',import.meta.url),'utf8');
vm.runInThisContext(source);
const sample=(id,t,kind,values)=>({id,timestamp_ms:t,source_timestamp_ms:t-10,sender_id:'VB',data_type:kind,values});
export const fixture={format:'seds-replay',version:1,recording:'saved-flight.db',mode:'flight',schema:{types:[{name:'VALVE_STATE'}]},layout:{units:'psi'},view_config:{attitude:{roll:{data_type:'ROLL',index:0}},stages:[],ground_systems:[{id:'tank',label:'Fuel tank',kind:'tank',unit:'%',min:0,max:100,binding:{data_type:'FILL',index:0}}]},valve_mappings:[{label:'Pilot (recorded name)',data_type:'VALVE_STATE',index:0},{label:'Dump valve',data_type:'VALVE_STATE',index:1}],samples:[sample(1,1000,'VALVE_STATE',[1,null]),sample(2,2000,'VALVE_STATE',[0,1]),sample(3,1000,'ROLL',[30]),sample(4,1000,'FILL',[42])]};
const p=GSOfflineReplay.validate(JSON.stringify(fixture));
assert.equal(GSOfflineReplay.state(p,999).valves[0].state,'Unknown');
assert.equal(GSOfflineReplay.state(p,1500).valves[0].state,'Open');
assert.equal(GSOfflineReplay.state(p,1500).valves[1].state,'Unknown');
assert.equal(GSOfflineReplay.state(p,2000).valves[0].state,'Closed');
assert.equal(GSOfflineReplay.state(p,2000).valves[1].state,'Open');
assert.equal(GSOfflineReplay.state(p,1500).components[0].value,42);
assert.equal(GSOfflineReplay.state(p,9000).components[0].value,null);
const tank=GSOfflineReplay.state(p,1500).components[0];
assert.equal(GSOfflineReplay.tankFillFraction(tank),.42);
assert.equal(GSOfflineReplay.tankFillFraction({...tank,unit:'kg',min:1,max:11,value:6}),.5,'explicit mass range is usable without inventing capacity');
assert.equal(GSOfflineReplay.tankFillFraction({...tank,unit:'g',min:1000,max:11000,value:6000}),.5);
for(const unit of ['psi','bar','kPa','Pa','V','',undefined])assert.equal(GSOfflineReplay.tankFillFraction({...tank,unit}),null,`${unit} is not tank fill`);
for(const change of [c=>delete c.min,c=>delete c.max,c=>c.max=c.min,c=>c.min=NaN,c=>c.binding=null,c=>c.binding={data_type:''},c=>c.value=null,c=>c.value=Infinity]){
 const invalid=structuredClone(tank);change(invalid);assert.equal(GSOfflineReplay.tankFillFraction(invalid),null);
}
assert.equal(GSOfflineReplay.tankFillFraction(GSOfflineReplay.state(p,999).components[0]),null,'future sample cannot drive fill');
assert.equal(GSOfflineReplay.tankFillFraction(GSOfflineReplay.state(p,9000).components[0]),null,'stale sample remains unknown');
assert.deepEqual(GSOfflineReplay.validate(JSON.stringify(p)).layout,p.layout);
for(const change of [x=>x.version=2,x=>x.schema=null,x=>x.valve_mappings[0].index=-1,x=>x.samples[1].id=1,x=>x.samples[0].values=[Infinity],x=>x.view_config.ground_systems=[null],x=>x.view_config.stages=[null],x=>x.view_config.ground_systems[0].binding.scale='bad',x=>x.valve_mappings[0].true_label={bad:true}]){
 const bad=structuredClone(fixture);change(bad);assert.throws(()=>GSOfflineReplay.validate(bad));
}
{
 // A file saved before the ox tank PT rename: rows, bindings and labels follow the new name.
 const legacy=structuredClone(fixture);
 legacy.samples.push(sample(5,1000,'FUEL_TANK_PRESSURE',[512]));
 legacy.view_config.ground_systems.push({id:'ox',label:'Fuel Tank Pressure',kind:'gauge',unit:'psi',min:0,max:1000,binding:{data_type:'FUEL_TANK_PRESSURE',index:0}});
 legacy.layout.FUEL_TANK_PRESSURE={label:'Fuel tank pressure'};
 const q=GSOfflineReplay.validate(JSON.stringify(legacy));
 assert.equal(q.samples.find(r=>r.id===5).data_type,'OX_TANK_PRESSURE');
 assert.ok(!q.samples.some(r=>r.data_type==='FUEL_TANK_PRESSURE'));
 assert.equal(q.view_config.ground_systems[1].binding.data_type,'OX_TANK_PRESSURE');
 assert.equal(q.view_config.ground_systems[1].label,'Ox Tank Pressure');
 assert.deepEqual(q.layout.OX_TANK_PRESSURE,{label:'Ox tank pressure'});
 assert.equal(q.layout.FUEL_TANK_PRESSURE,undefined);
 assert.equal(GSOfflineReplay.state(q,1500).components[1].value,512);
 assert.equal(q.view_config.ground_systems[0].label,'Fuel tank','other labels are untouched');
}
console.log('PASS offline roundtrip, embedded config/schema/units/mappings, no future/live substitution, every valve update, null/stale unknown, malformed refusal');

{
 // The flight replay on the actual offline DOM path: timeline, T-clock, HUD dashes, events.
 class Element {
  constructor(tag){this.tagName=tag;this.children=[];this.dataset={};this.style={};this.attributes={};this.isConnected=true;this.textContent='';}
  append(...c){for(const x of c){x.parent=this;this.children.push(x);}} replaceChildren(...c){this.children=[];this.append(...c);}
  setAttribute(k,v){this.attributes[k]=String(v);} querySelectorAll(){return [];} remove(){if(this.parent)this.parent.children=this.parent.children.filter(n=>n!==this);}
 }
 const all=e=>e.children.flatMap(c=>[c,...all(c)]);
 const context=vm.createContext({TextEncoder,performance:{now:()=>0},setInterval:()=>1,clearInterval:()=>{},document:{createElement:t=>new Element(t),createElementNS:(_,t)=>new Element(t)}});
 vm.runInContext(source,context);
 const fs_=(id,t,code,phase)=>({id,timestamp_ms:t,sender_id:'FC',data_type:'FLIGHT_STATE',values:[code],phase});
 const gps=(id,t,alt)=>({id,timestamp_ms:t,sender_id:'FC',data_type:'GPS_DATA',values:[31.758,-102.046+t*1e-8,alt]});
 const doc={...fixture,samples:[fs_(1,0,6,'Armed'),fs_(2,10000,7,'Launch'),fs_(3,17000,9,'Coast'),fs_(4,36000,11,'Descent'),fs_(5,200000,13,'Landed'),...[0,5000,10000,15000,20000,30000,40000,100000,200000].map((t,i)=>gps(10+i,t,890+Math.min(t,40000)/10))]};
 const root=new Element('main'),player=context.GSOfflineReplay.mount(root,doc);
 const timeline=all(root).find(n=>n.className==='gs-flight-timeline');
 assert.deepEqual(timeline.children.filter(n=>n.dataset.phase).map(n=>n.dataset.phase),['pad','boost','coast','drogue','landed']);
 assert.deepEqual(timeline.children.filter(n=>n.dataset.event).map(n=>n.dataset.event),['ignition','burnout','apogee','drogue','landing'],'no Apogee row: the highest recorded altitude marks it');
 player.seek(12500);assert.equal(root.dataset.phase,'boost');assert.equal(root.dataset.tclock,'T+ 00:02');
 player.jump('drogue');assert.equal(root.dataset.timestamp,36000);assert.equal(root.dataset.phase,'drogue');
 player.seek(5000);assert.equal(root.dataset.tclock,'T- 00:05');
 const hud=all(root).find(n=>n.className==='gs-flight-hud');assert.ok(all(hud).some(n=>n.textContent==='—'),'unrecorded channels show a dash');
 assert.equal(all(root).filter(n=>n.dataset.jump).map(n=>n.dataset.jump).join(),'ignition,burnout,apogee,drogue,landing');
 player.setLoop('a',10000);player.setLoop('b',17000);assert.equal(all(timeline).filter(n=>n.dataset.loop).length,1);
 assert.equal(root.dataset.predicted,'false');
 const predicted=context.GSOfflineReplay.mount(new Element('main'),{...doc,predicted:true,label:'Predicted: plan'});assert.equal(predicted.model.predicted.label,'Predicted: plan');
 console.log('PASS flight replay DOM: timeline phases/events, T-clock, jumps, loop, HUD dashes, predicted label');
}
