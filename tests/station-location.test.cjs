// "Use this device's location": reuse of the dashboard's own fix, timeouts with
// maximumAge, the coarse retry and failure codes. Synthetic geolocation only.
const {test}=require('node:test');
const assert=require('node:assert/strict');
const vm=require('node:vm');
const fs=require('node:fs');
const path=require('node:path');
const source=fs.readFileSync(path.resolve(__dirname,'../assets/browser/station_location.js'),'utf8');
const NOW=1_000_000;
// answers: one entry per getCurrentPosition call: {ok:coords} or {code}.
async function run({answers=[],deviceFix,secure=true,geolocation=true}={}){
  const calls=[];let sent;
  const window={isSecureContext:secure,__gs26_device_fix:deviceFix};
  const navigator={geolocation:geolocation?{getCurrentPosition:(ok,fail,options)=>{
    calls.push(options);const answer=answers.shift();if(!answer)return;
    setTimeout(()=>answer.ok?ok({coords:answer.ok,timestamp:NOW-(answer.age||0)}):fail({code:answer.code}),0);
  }}:undefined};
  const context=vm.createContext({window,navigator,Date:{now:()=>NOW},setTimeout,dioxus:{send:v=>sent=v}});
  await vm.runInContext(`(async()=>{${source}})()`,context);
  return {sent,calls};
}
test('a recent fix from the dashboard location watch is reused without a new request',async()=>{
  const {sent,calls}=await run({deviceFix:{latitude:43,longitude:-78.8,altitude_m:null,accuracy_m:9,at:NOW-30_000}});
  assert.equal(calls.length,0);
  assert.deepEqual({...sent.coordinate},{latitude:43,longitude:-78.8,altitude_m:null,accuracy_m:9,source:'device_location'});
  assert.equal(sent.age_ms,30_000);
});
test('an old watched fix is ignored and the request accepts a recent cached position',async()=>{
  const {sent,calls}=await run({deviceFix:{latitude:1,longitude:2,at:NOW-600_000},answers:[{ok:{latitude:42.5,longitude:-79.25,altitude:null,accuracy:7.6},age:5000}]});
  assert.equal(calls.length,1);
  assert.deepEqual({...calls[0]},{enableHighAccuracy:true,maximumAge:120000,timeout:15000});
  assert.equal(sent.coordinate.latitude,42.5);assert.equal(sent.age_ms,5000);
});
test('a timeout retries once with network accuracy before reporting a timeout',async()=>{
  const ok=await run({answers:[{code:3},{ok:{latitude:42,longitude:-79,accuracy:900}}]});
  assert.equal(ok.calls.length,2);
  assert.deepEqual({...ok.calls[1]},{enableHighAccuracy:false,maximumAge:300000,timeout:10000});
  assert.equal(ok.sent.coordinate.accuracy_m,900);
  const failed=await run({answers:[{code:3},{code:3}]});
  assert.equal(failed.sent.failure,'timeout');
  assert.equal(failed.sent.coordinate,undefined);
});
test('permission denied is reported at once and never retried',async()=>{
  const {sent,calls}=await run({answers:[{code:1}]});
  assert.equal(calls.length,1);assert.equal(sent.failure,'denied');
});
test('an unavailable position after the retry is reported as unavailable',async()=>{
  assert.equal((await run({answers:[{code:2},{code:2}]})).sent.failure,'unavailable');
});
test('insecure pages and missing geolocation fail without a request',async()=>{
  assert.equal((await run({secure:false})).sent.failure,'insecure');
  assert.equal((await run({geolocation:false})).sent.failure,'unsupported');
  // A recent watched fix still works: it was obtained by the page earlier.
  assert.equal((await run({secure:false,deviceFix:{latitude:1,longitude:2,at:NOW}})).sent.coordinate.latitude,1);
});
