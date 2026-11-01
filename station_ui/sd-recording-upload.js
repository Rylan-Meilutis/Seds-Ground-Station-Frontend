// Explicit local-file upload. File/Blob passes directly to fetch; no FileReader
// or arrayBuffer conversion of the CSV. Manifest/config JSON is separately bounded.
(()=>{
'use strict';
async function upload(input,url,metadata,token){
 const file=input?.files?.[0];
 if(!file)throw Error('Choose the SD CSV again.');
 const controller=new AbortController();input.__sdAbort=controller;
 const headers=token?{Authorization:'Bearer '+token}:{};let pending=null;
 try{
  const prepared=await fetch(url,{method:'POST',headers:{...headers,'Content-Type':'application/json'},body:JSON.stringify(metadata),signal:controller.signal});
  if(!prepared.ok)throw Error(await prepared.text());pending=await prepared.json();
  if(file.size>pending.max_bytes)throw Error('CSV exceeds available server file/disk budget of '+pending.max_bytes+' bytes.');
  const response=await fetch(url+'/'+pending.upload_id,{method:'PUT',headers:{...headers,'Content-Type':'text/csv'},body:file,signal:controller.signal});
  if(!response.ok)throw Error(await response.text());return JSON.stringify(await response.json());
 }catch(error){
  if(pending)await fetch(url+'/'+pending.upload_id,{method:'DELETE',headers}).catch(()=>{});
  throw error;
 }finally{input.__sdAbort=null;}
}
async function saveSource(url,token,name){
 if(typeof globalThis.showSaveFilePicker!=='function')throw Error('Streamed SD saving requires a browser with the File System Access API or the native station app. The source remains stored on the station.');
 const handle=await globalThis.showSaveFilePicker({suggestedName:name,types:[{description:'Original SD CSV',accept:{'text/csv':['.csv']}}]});
 const response=await fetch(url,{headers:token?{Authorization:'Bearer '+token}:{},cache:'no-store'});
 if(!response.ok||!response.body)throw Error('Original SD download failed: '+response.status);
 const target=await handle.createWritable();
 try{await response.body.pipeTo(target);}catch(error){await target.abort().catch(()=>{});throw error;}
 return 'Byte-exact original SD source saved.';
}
globalThis.GSSDImport={upload,saveSource};
})();
