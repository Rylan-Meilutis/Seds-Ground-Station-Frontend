// The offline replay player labels a predicted replay (a flight plan played
// as a recording: replay.json "predicted": true) and never labels a recording.
import fs from 'node:fs';
import vm from 'node:vm';
import assert from 'node:assert/strict';
const source=fs.readFileSync(new URL('../station_ui/rendering/three/offline-replay.js',import.meta.url),'utf8');
vm.runInThisContext(source);
const sample=(id,t,kind,values)=>({id,timestamp_ms:t,sender_id:'FC',data_type:kind,values});
const base={format:'seds-replay',version:1,mode:'flight',schema:{},layout:{},view_config:{stages:[],ground_systems:[]},valve_mappings:[],
 samples:[sample(1,1767225600000,'GPS_DATA',[31.758,-102.046,890]),sample(2,1767225600100,'BAROMETER_DATA',[0,90000,20]),sample(3,1767225600000,'FLIGHT_STATE',[7])]};
const recorded=GSOfflineReplay.validate(JSON.stringify(base));
assert.deepEqual(GSOfflineReplay.header(recorded),{predicted:false,badge:null,title:'Flight replay · saved data'});
const predicted=GSOfflineReplay.validate(JSON.stringify({...base,predicted:true,label:'Predicted: SWEET V6 8.5 kg'}));
const head=GSOfflineReplay.header(predicted);
assert.equal(head.predicted,true);
assert.equal(head.badge,'Predicted: SWEET V6 8.5 kg');
assert.match(head.title,/^Predicted flight/);
assert.equal(GSOfflineReplay.header({...predicted,label:'8.5 kg nominal'}).badge,'Predicted: 8.5 kg nominal');
assert.equal(GSOfflineReplay.header({...predicted,label:''}).badge,'Predicted');
assert.equal(GSOfflineReplay.header({...recorded,label:'Predicted: spoof',predicted:'true'}).predicted,false,'only a boolean true marks a prediction');
console.log('PASS predicted replay label');
