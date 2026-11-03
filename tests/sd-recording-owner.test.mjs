import assert from 'node:assert/strict';
import fs from 'node:fs';
import vm from 'node:vm';
const source=fs.readFileSync(new URL('../station_ui/sd-recording-upload.js',import.meta.url),'utf8');
function bridge(check){
 const queue=[],waiting=[],messages=[];
 const push=value=>waiting.length?waiting.shift()(value):queue.push(value);
 return {messages,push,dioxus:{send(message){messages.push(message);if(message.kind==='check')push({id:message.id,allowed:check(message.id)});},recv(){return queue.length?Promise.resolve(queue.shift()):new Promise(resolve=>waiting.push(resolve));}}};
}
const response=data=>({ok:true,json:async()=>data,text:async()=>String(data)});
for(const retireAt of [0,6]){
 const calls=[],file={name:'source.csv',size:4,arrayBuffer(){throw Error('Never materialize CSV');}},input={files:[file],isConnected:true};
 const owner=bridge(id=>!retireAt||id<retireAt);
 const context={AbortController,dioxus:owner.dioxus,fetch:async(url,request)=>{
  calls.push({url,...request});
  assert.equal(request.headers.Authorization,'Bearer fixture-original');
  if(request.method==='POST')return response({upload_id:'123',max_bytes:100});
  if(request.method==='DELETE')return response({});
  assert.equal(request.body,file);return response({recording:'groundstation_recording_1.db'});
 }};
 vm.runInNewContext(source,context);
 await context.GSSDImport.runOwned((check,retire)=>context.GSSDImport.upload(input,'http://original/imports',{filename:'source.csv'},'fixture-original',check,retire));
 if(retireAt){assert.equal(calls.some(c=>c.method==='PUT'),false);assert.equal(calls.at(-1).method,'DELETE');assert.equal(owner.messages.at(-1).kind,'failed');}
 else{assert.equal(owner.messages.at(-1).kind,'complete');assert.equal(JSON.parse(owner.messages.at(-1).value).recording,'groundstation_recording_1.db');}
 assert.equal(input.__sdAbort,null);
}
{
 const calls=[],owner=bridge(()=>true),input={files:[{name:'source.csv',size:4}],isConnected:true};
 const context={AbortController,dioxus:owner.dioxus,fetch:async(url,request)=>{
  calls.push({url,...request});
  if(request.method==='POST')return response({upload_id:'456',max_bytes:100});
  if(request.method==='DELETE')return response({});
  return await new Promise((_,reject)=>{
   request.signal.addEventListener('abort',()=>reject(Error('retired')), {once:true});
   queueMicrotask(()=>owner.push({abort:true}));
  });
 }};
 vm.runInNewContext(source,context);
 await context.GSSDImport.runOwned((check,retire)=>context.GSSDImport.upload(input,'http://original/imports',{filename:'source.csv'},'original',check,retire));
 assert.deepEqual(calls.map(c=>c.method),['POST','PUT','DELETE']);
 assert.equal(owner.messages.at(-1).kind,'failed');assert.equal(input.__sdAbort,null);
}
{
 const owner=bridge(()=>true),state={close:0,abort:0,release:0,writes:0};
 const context={AbortController,dioxus:owner.dioxus,showSaveFilePicker:async()=>({createWritable:async()=>({write:async()=>state.writes++,close:async()=>state.close++,abort:async()=>state.abort++})}),fetch:async(_,request)=>({ok:true,body:{getReader:()=>({read:()=>new Promise((_,reject)=>{request.signal.addEventListener('abort',()=>reject(Error('retired')),{once:true});queueMicrotask(()=>owner.push({abort:true}));}),releaseLock(){state.release++;}})}})};
 vm.runInNewContext(source,context);
 await context.GSSDImport.runOwned((check,retire)=>context.GSSDImport.saveSource('/source','original','source.csv',check,retire));
 assert.deepEqual(state,{close:0,abort:1,release:1,writes:0});assert.equal(owner.messages.at(-1).kind,'failed');
}
{
 const replacement=new AbortController(),input={files:[{size:4}]};
 const context={AbortController,fetch:async(_,request)=>request.method==='POST'?response({upload_id:'789',max_bytes:100}):(input.__sdAbort=replacement,response({recording:'done'}))};
 vm.runInNewContext(source,context);await context.GSSDImport.upload(input,'/imports',{},'original',async()=>{},()=>{});assert.equal(input.__sdAbort,replacement);assert.equal(replacement.signal.aborted,false);
}
console.log('5 owner bridge/cleanup controls passed; original URL/credential retained, stale owner cannot PUT/close or cancel newer controller');
