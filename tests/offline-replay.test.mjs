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
console.log('PASS offline roundtrip, embedded config/schema/units/mappings, no future/live substitution, every valve update, null/stale unknown, malformed refusal');
