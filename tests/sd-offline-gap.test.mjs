import fs from 'node:fs';import vm from 'node:vm';import assert from 'node:assert/strict';
const source=fs.readFileSync(new URL('../station_ui/rendering/three/offline-replay.js',import.meta.url),'utf8');
const context={};vm.runInNewContext(source,context);
const bundle={format:'seds-replay',version:1,schema:{},view_config:{stages:[],ground_systems:[]},layout:{},valve_mappings:[],samples:[{id:1,timestamp_ms:1000,valid_until_ms:1010,data_type:'KG50',sender_id:'DAQ',values:[5]}]};
const p=context.GSOfflineReplay.validate(bundle),binding={data_type:'KG50',sender_id:'DAQ',index:0};
assert.equal(context.GSOfflineReplay.value(p,binding,1005,true),5);
assert.equal(context.GSOfflineReplay.value(p,binding,1011,true),null);
assert.throws(()=>context.GSOfflineReplay.validate({...bundle,samples:[{...bundle.samples[0],valid_until_ms:999}]}));
console.log('SD offline coverage boundaries preserve gaps, including hold-mode bindings');
