// Local-file recovery only. File/Blob is streamed; no complete CSV buffering.
(()=>{
'use strict';
async function guarded(check,operation){await check();const result=await operation();await check();return result;}
async function runOwned(operation){
 let stopped=false,sequence=0;const pending=new Map(),retire=[];
 const fail=()=>{if(stopped)return;stopped=true;for(const cancel of retire)cancel();for(const item of pending.values())item.reject(Error('Original export owner retired'));pending.clear();};
 const emit=message=>{try{dioxus.send(message);}catch(_){fail();}};
 const onRetire=cancel=>{if(stopped)cancel();else retire.push(cancel);};
 const check=async()=>{
  if(stopped)throw Error('Original export owner retired');
  if(sequence>=Number.MAX_SAFE_INTEGER)throw Error('Export query identity exhausted');
  const id=++sequence;
  const response=new Promise((resolve,reject)=>pending.set(id,{resolve,reject}));
  emit({kind:'check',id});await response;
  if(stopped)throw Error('Original export owner retired');
 };
 const listener=(async()=>{try{while(!stopped){const reply=await dioxus.recv();if(reply?.abort){fail();break;}const item=pending.get(reply?.id);if(item){pending.delete(reply.id);if(reply.allowed===true)item.resolve();else{item.reject(Error('Original export owner retired'));fail();}}}}catch(_){fail();}})();
 try{await check();const value=await operation(check,onRetire);await check();emit({kind:'complete',value});}
 catch(error){emit({kind:'failed',message:String(error?.message||error)});}
 finally{fail();void listener;}
}
async function upload(input,url,metadata,token,check,onRetire){
 if(typeof check!=='function'||typeof onRetire!=='function')throw Error('Original export owner is required');
 const file=input?.files?.[0];
 if(!file)throw Error('Choose the SD CSV again.');
 if(metadata?.filename&&file.name!==metadata.filename)throw Error('Selected SD CSV changed; choose it again.');
 const controller=new AbortController();input.__sdAbort=controller;
 onRetire(()=>controller.abort());
 const assertCurrent=async()=>{await check();if(!input.isConnected&&input.isConnected!==undefined)throw Error('Selected file input was retired');};
 const headers=token?{Authorization:'Bearer '+token}:{};let pending=null;
 try{
  const prepared=await guarded(assertCurrent,()=>fetch(url,{method:'POST',headers:{...headers,'Content-Type':'application/json'},body:JSON.stringify(metadata),signal:controller.signal}));
  if(!prepared.ok)throw Error(await guarded(assertCurrent,()=>prepared.text()));
  pending=await guarded(assertCurrent,()=>prepared.json());
  if(typeof pending.upload_id!=='string'||!/^\d{1,40}$/.test(pending.upload_id)||!Number.isSafeInteger(pending.max_bytes)||pending.max_bytes<1)throw Error('Invalid import reservation');
  if(file.size>pending.max_bytes)throw Error('CSV exceeds available server file/disk budget of '+pending.max_bytes+' bytes.');
  const response=await guarded(assertCurrent,()=>fetch(url+'/'+pending.upload_id,{method:'PUT',headers:{...headers,'Content-Type':'text/csv'},body:file,signal:controller.signal}));
  if(!response.ok)throw Error(await guarded(assertCurrent,()=>response.text()));
  return JSON.stringify(await guarded(assertCurrent,()=>response.json()));
 }catch(error){
  if(pending&&typeof pending.upload_id==='string'&&/^\d{1,40}$/.test(pending.upload_id))await fetch(url+'/'+pending.upload_id,{method:'DELETE',headers}).catch(()=>{});
  throw error;
 }finally{if(input.__sdAbort===controller)input.__sdAbort=null;}
}
async function saveSource(url,token,name,check,onRetire){
 if(typeof check!=='function'||typeof onRetire!=='function')throw Error('Original export owner is required');
 if(typeof globalThis.showSaveFilePicker!=='function')throw Error('Streamed SD saving requires a browser with the File System Access API or the native station app. The source remains stored on the station.');
 const controller=new AbortController();onRetire(()=>controller.abort());
 const handle=await guarded(check,()=>globalThis.showSaveFilePicker({suggestedName:name,types:[{description:'Original SD CSV',accept:{'text/csv':['.csv']}}]}));
 const response=await guarded(check,()=>fetch(url,{headers:token?{Authorization:'Bearer '+token}:{},cache:'no-store',signal:controller.signal}));
 if(!response.ok||!response.body)throw Error('Original SD download failed: '+response.status);
 const target=await guarded(check,()=>handle.createWritable());
 const reader=response.body.getReader();let closed=false;
 try{
  for(;;){const item=await guarded(check,()=>reader.read());if(item.done)break;await guarded(check,()=>target.write(item.value));}
  await guarded(check,()=>target.close());closed=true;
 }catch(error){if(!closed)await target.abort().catch(()=>{});throw error;}
 finally{reader.releaseLock();}
 return 'Byte-exact original SD source saved.';
}
globalThis.GSSDImport={upload,saveSource,runOwned};
})();
