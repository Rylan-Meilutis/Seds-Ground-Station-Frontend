const {test}=require('node:test');
const assert=require('node:assert/strict');
const fs=require('node:fs');
const path=require('node:path');
const vm=require('node:vm');
const source=fs.readFileSync(path.join(__dirname,'../static/ground_map.js'),'utf8');
const rust=fs.readFileSync(path.join(__dirname,'../src/telemetry_dashboard/dashboard_component.rs'),'utf8');
const start=rust.indexOf('let (rocket_lat, rocket_lon) = coord_js(*rocket_gps.read());');
assert(start>=0,'actual Dashboard caller seam');
const raw=rust.slice(start).split('r#"')[1].split('"#')[0];
assert(raw.includes('window.setGroundMapPrefetchContext('));
const enabledStart=rust.indexOf('let map_prefetch_enabled = map_prefetch_enabled;');
assert(enabledStart>=0,'actual Dashboard enabled caller seam');
const enabledRaw=rust.slice(enabledStart).split('r#"')[1].split('"#')[0];
function runtime(){
 const requests=[],timers=[],cache=new Map();
 const context=vm.createContext({window:{location:{protocol:'http:'},innerWidth:1280,innerHeight:900,addEventListener(){},localStorage:{getItem(){return null},setItem(){}},requestIdleCallback:fn=>{timers.push(fn);return timers.length},cancelIdleCallback(){}},document:{getElementById(){return null}},navigator:{},console,URL,Headers,Response,Uint8Array,ArrayBuffer,performance,
  caches:{open:async()=>({match:async key=>{const hit=cache.get(key);return hit?new Response(hit.body,{headers:hit.headers}):undefined},put:async(key,value)=>cache.set(key,{body:await value.arrayBuffer(),headers:[...value.headers]}),keys:async()=>[]}),keys:async()=>[]},
  setTimeout:fn=>{timers.push(fn);return timers.length},clearTimeout(){},setInterval(){return 1},clearInterval(){},requestAnimationFrame:fn=>{timers.push(fn);return timers.length},
  fetch:async(url,options)=>{requests.push({url,method:options?.method||'GET'});return new Response(context.png,{headers:{'Content-Type':'image/png','Content-Length':String(context.png.byteLength)}})},
 });
 context.window.fetch=context.fetch;context.window.caches=context.caches;
 vm.runInContext(source,context);context.png=vm.runInContext('transparentTileFallbackArrayBuffer()',context);
 const read=code=>JSON.parse(vm.runInContext('JSON.stringify('+code+')',context));
 return{context,requests,timers,read};
}
function call(r,rocket,ground){
 const coord=values=>values===null?['null','null']:values.map(v=>JSON.stringify(v));
 const [rocket_lat,rocket_lon]=coord(rocket),[user_lat,user_lon]=coord(ground);
 const replacements={user_radius:'1609.344',rocket_radius:'1609.344',tiles_js:JSON.stringify('http://station.test/tiles/{z}/{x}/{y}.jpg'),rocket_lat,rocket_lon,user_lat,user_lon};
 let js=raw;for(const[key,value]of Object.entries(replacements))js=js.replaceAll('{'+key+'}',value);
 js=js.replaceAll('{{','{').replaceAll('}}','}');
 vm.runInContext(js,r.context);
}
const settle=async()=>{for(let i=0;i<20;i++)await new Promise(resolve=>setImmediate(resolve));};
test('literal Dashboard absent caller starts no prefetch or size-sample request',async()=>{
 const r=runtime();call(r,null,null);await settle();
 assert.deepEqual(r.read('[prefetchRocketLatLng,prefetchUserLatLng]'),[null,null]);
 assert.equal(r.read('buildHighResPrefetchPlan().coords.length'),0);
 assert.equal(r.read('tilePrefetchTimer'),null);
 assert.equal(r.requests.length,0,'no physical request even from size sampler');
});
test('empty and non-number fixes never become real locations',async()=>{
 for(const value of ['', ' ', '0',false,[],{}]){
  const r=runtime();call(r,[value,value],[value,value]);await settle();
  assert.deepEqual(r.read('[prefetchRocketLatLng,prefetchUserLatLng]'),[null,null]);
  assert.equal(r.read('buildHighResPrefetchPlan().coords.length'),0);
  assert.equal(r.requests.length,0);
 }
 const r=runtime();vm.runInContext("setGroundMapPrefetchContext('http://station.test/tiles/{z}/{x}/{y}.jpg',null,NaN,Infinity,-Infinity,NaN)",r.context);await settle();
 assert.deepEqual(r.read('[prefetchRocketLatLng,prefetchUserLatLng]'),[null,null]);assert.equal(r.requests.length,0);
 for(const pair of [[null,0],[0,null],[86,0],[0,181]]){
  const checked=runtime();call(checked,pair,pair);await settle();
  assert.deepEqual(checked.read('[prefetchRocketLatLng,prefetchUserLatLng]'),[null,null]);assert.equal(checked.requests.length,0);
 }
});
test('real numeric Null Island remains a valid rocket and ground location',()=>{
 const r=runtime();call(r,[0,0],[0,0]);
 assert.deepEqual(r.read('[prefetchRocketLatLng,prefetchUserLatLng]'),[[0,0],[0,0]]);
 const plan=r.read('buildHighResPrefetchPlan()');assert.equal(plan.coords.length,1152);
 const expected=[];for(let z=12;z>=5;z--){const middle=2**(z-1);for(let x=middle-6;x<=middle+5;x++)for(let y=middle-6;y<=middle+5;y++)expected.push({x,y,z});}
 assert.deepEqual(plan.coords,expected,'exact original1152tile order and5tilebuffer');
 assert.equal(r.read('HIGH_RES_PREFETCH_LOCATION_BUFFER_TILES'),5);
 assert.notEqual(r.read('tilePrefetchTimer'),null);
});
test('later real rocket-only or ground-only fix starts normal prefetch',async()=>{
 for(const which of ['rocket','ground']){
  const r=runtime();call(r,null,null);await settle();assert.equal(r.requests.length,0);
  call(r,which==='rocket'?[0,0]:null,which==='ground'?[0,0]:null);
  assert.equal(r.read('buildHighResPrefetchPlan().coords.length'),1152);
  assert.notEqual(r.read('tilePrefetchTimer'),null,'valid later source schedules original policy');
  assert.deepEqual(r.read('[prefetchRocketLatLng,prefetchUserLatLng]'),which==='rocket'?[[0,0],null]:[null,[0,0]]);
 }
});
test('invalid later input does not create a replacement coordinate or mutate telemetry/markers',()=>{
 const r=runtime();call(r,[45,-75],[46,-76]);
 const before=r.read('[lastRocketLatLng,lastUserLatLng]');
 call(r,null,null);
 assert.deepEqual(r.read('[prefetchRocketLatLng,prefetchUserLatLng]'),[null,null]);
 assert.deepEqual(r.read('[lastRocketLatLng,lastUserLatLng]'),before,'last-known marker facts not fabricated or erased');
});
test('common location validators and persisted absent ground values reject coercion',()=>{
 const r=runtime();
 for(const expression of ['null','undefined','""','"0"','false','NaN','Infinity']){
  assert.equal(r.read('isUsableLatLng('+expression+','+expression+')'),false);
  assert.equal(r.read('isUsableUserLatLng('+expression+','+expression+')'),false);
 }
 assert.equal(r.read('isUsableLatLng(0,0)'),true);assert.equal(r.read('isUsableUserLatLng(0,0)'),true);
 const storageKey=r.read('MAP_STATE_STORAGE_KEY');let reads=0,stored={userLat:null,userLon:null};
 r.context.window.localStorage.getItem=key=>{if(key!==storageKey)return null;reads++;return JSON.stringify(stored)};
 vm.runInContext('loadPersistedMapState()',r.context);
 assert.equal(reads,1,'actual current persisted-state key was read');
 assert.equal(r.read('lastUserLatLng'),null);
 stored={userLat:0,userLon:0};vm.runInContext('loadPersistedMapState()',r.context);
 assert.equal(reads,2);assert.deepEqual(r.read('lastUserLatLng'),[0,0],'real persisted numeric zero remains usable');
});
test('real rocket-only caller and original enabled policy service1152unique tiles',async()=>{
 const r=runtime();call(r,[0,0],null);
 vm.runInContext(enabledRaw.replaceAll('{enabled}','true').replaceAll('{{','{').replaceAll('}}','}'),r.context);
 const pending=new Set();let error;
 // The real serialized cache has yields before/after both match and put,
 // plus fetch/worker yields.20k bounds all1152rows without short-circuiting it.
 for(let step=0;step<20000;step++){
  for(const callback of r.timers.splice(0)){
   const returned=callback();if(returned?.then){const tracked=returned.catch(e=>{error=e}).finally(()=>pending.delete(tracked));pending.add(tracked);}
  }
  await new Promise(resolve=>setImmediate(resolve));
  if(error)throw error;
  if(r.read('tilePrefetchState.state')==='ready' && pending.size===0 && r.timers.length===0)break;
 }
 assert.equal(r.read('tilePrefetchState.state'),'ready',JSON.stringify({state:r.read('tilePrefetchState'),requests:r.requests.length,pending:pending.size,timers:r.timers.length}));
 assert.equal(pending.size,0,'all dispatched timer promises settled before stopping the driver');
 assert.equal(r.read('tilePrefetchState.completed'),1152);
 assert.equal(r.read('tilePrefetchState.failed'),0);
 const unique=new Set(r.requests.filter(x=>x.method==='GET').map(x=>x.url));
 assert.equal(unique.size,1152,'all original plan URLs actually reach the local fetch boundary');
 assert.equal(r.read('autoHighResPrefetchCompleted'),true);
});
