const assert=require('node:assert/strict');
const {presentation}=require('../station_ui/scripts/stream-sequence-visuals.js');
const observation=(value,evidence='board_reported')=>({value,evidence,age_ms:0,valid_for_ms:2000});
function frame(){return {version:1,clock:{kind:'t_minus',remaining_ms:5000,advancing:true},igniter:{planned_t_minus_ms:5000},valves:{pilot:{output:observation(true)}}};}
let value=presentation(frame());assert.equal(value.igniter.planned,true);assert.equal(value.igniter.output.value,null);assert.match(value.igniter.label,/planned/);assert.equal(value.valves[0].output.value,true);assert.equal(value.valves[0].output.level,'reported');assert.notEqual(value.valves[0].output.level,'electrical');assert.equal(value.valves[0].flow.value,null);
for(const ms of [5001,-1,NaN,Infinity]){const f=frame();f.clock.remaining_ms=ms;assert.equal(presentation(f).igniter.planned,false);}
for(const clock of [{kind:'idle',remaining_ms:5000,advancing:true},{kind:'t_plus',remaining_ms:5000,advancing:true},{kind:'t_minus',remaining_ms:5000,advancing:false}]){const f=frame();f.clock=clock;assert.equal(presentation(f).igniter.planned,false);}
for(const evidence of ['sdk_ack','accepted','measured']){const f=frame();f.igniter.output=observation(true,evidence);assert.equal(presentation(f).igniter.output.value,null);}
let f=frame();f.igniter.command=observation(true,'commanded');assert.equal(presentation(f).igniter.command.level,'commanded');assert.equal(presentation(f).igniter.output.value,null);
f.igniter.output=observation(true,'electrical_confirmed');assert.equal(presentation(f).igniter.output.value,null);f.igniter.output.source_kind='voltage_sensor';assert.equal(presentation(f).igniter.output.level,'electrical');
f.igniter.output=observation(false);assert.equal(presentation(f).igniter.output.value,false);assert.match(presentation(f).igniter.label,/off/);
f.valves.pilot.output.age_ms=2001;assert.equal(presentation(f).valves[0].output.value,null);
f=frame();f.valves.pilot.flow={...observation(2,'measured'),source_kind:'mass_derivative'};assert.equal(presentation(f).valves[0].flow.value,null);
f.valves.pilot.flow.source_kind='flow_sensor';assert.equal(presentation(f).valves[0].flow.level,'measured');
f.valves.pilot.flow={...observation(2,'inferred')};assert.equal(presentation(f).valves[0].flow.level,'expected');assert.match(presentation(f).valves[0].flow.label,/not measured/);
f.valves.pilot.flow={evidence:'planned',source_kind:'sequence_plan',active:true,age_ms:0,valid_for_ms:2000};assert.equal(presentation(f).valves[0].flow.value,null);assert.equal(presentation(f).valves[0].flow.level,'expected');assert.match(presentation(f).valves[0].flow.label,/unmeasured/);
for(const missing of [undefined,null,{}, {version:0}]){const p=presentation(missing);assert.equal(p.igniter.planned,false);assert(p.valves.every(v=>v.output.value===null&&v.flow.value===null));}
console.log('stream sequence visuals: T−5 plan / board-reported vs independent electrical / ACK / stale / measured-vs-inferred / missing-frame controls PASS');
