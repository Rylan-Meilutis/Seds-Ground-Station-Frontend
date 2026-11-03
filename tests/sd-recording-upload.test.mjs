import assert from 'node:assert/strict';
import fs from 'node:fs';
import vm from 'node:vm';
const script=fs.readFileSync(new URL('../station_ui/sd-recording-upload.js',import.meta.url),'utf8');
async function scenario(mode){
 const file={size:1024,arrayBuffer(){throw Error('CSV must never be materialized');}};
 const input={files:[file]},calls=[];
 const response=(data,ok=true)=>({ok,json:async()=>data,text:async()=>String(data)});
 const context={AbortController,fetch:async(url,options)=>{
  calls.push({url,...options});
  if(options.method==='POST')return response({upload_id:'123',max_bytes:mode==='limit'?512:4096});
  if(options.method==='DELETE')return response({});
  assert.equal(options.body,file);assert.equal(options.headers.Authorization,'Bearer token');
  if(mode==='cancel'){input.__sdAbort.abort();throw Error('aborted');}
  return response({recording:'groundstation_recording_123.db'});
 }};
 vm.runInNewContext(script,context);
 if(mode==='ok')assert.equal(JSON.parse(await context.GSSDImport.upload(input,'/imports',{manifest:{}},'token',async()=>{},()=>{})).recording,'groundstation_recording_123.db');
 else await assert.rejects(context.GSSDImport.upload(input,'/imports',{manifest:{}},'token',async()=>{},()=>{}));
 assert.equal(input.__sdAbort,null);
 assert.equal(calls.at(-1).method,mode==='ok'?'PUT':'DELETE');
 if(mode==='limit')assert.equal(calls.length,2);
}
await scenario('ok');await scenario('limit');await scenario('cancel');
console.log('3 SD browser upload protocol scenarios passed; raw File object preserved');
{
 const chunks=[new Uint8Array([1,2]),new Uint8Array([3])],state={saved:false,seen:[],released:false};let index=0;
 const target={write:async bytes=>state.seen.push(...bytes),close:async()=>{state.saved=true;},abort:async()=>{throw Error('Unexpected abort');}};
 const context={AbortController,showSaveFilePicker:async()=>({createWritable:async()=>target}),fetch:async()=>({ok:true,body:{getReader:()=>({read:async()=>index<chunks.length?{done:false,value:chunks[index++]}:{done:true},releaseLock(){state.released=true;}})},arrayBuffer(){throw Error('No whole-file read');},blob(){throw Error('No whole-file blob');}})};
 vm.runInNewContext(script,context);await context.GSSDImport.saveSource('/source','token','source.csv',async()=>{},()=>{});assert.equal(state.saved,true);assert.deepEqual(state.seen,[1,2,3]);assert.equal(state.released,true);
 console.log('Original SD save retains exact streaming bytes, closes and releases reader without whole-file buffering');
}
