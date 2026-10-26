// Screen wake locks are released by browsers while hidden; reacquire on return.
(()=>{
 if(window.GS26StayAwake)return;
 const active=new Set();let lock=null,pending=false,closed=false;
 async function update(){
  const wanted=!closed&&active.size>0&&document.visibilityState==='visible';
  if(!wanted){const old=lock;lock=null;await old?.release().catch(()=>{});return;}
  if(lock||pending||!navigator.wakeLock?.request)return;
  pending=true;
  try{const next=await navigator.wakeLock.request('screen');if(closed||!active.size||document.visibilityState!=='visible'){await next.release();return;}lock=next;next.addEventListener('release',()=>{if(lock===next)lock=null;});}catch{}finally{pending=false;}
 }
 window.GS26StayAwake={setActive(key,value){if(value)active.add(key);else active.delete(key);update();}};
 document.addEventListener('visibilitychange',update);
 window.addEventListener('pageshow',()=>{closed=false;update();});
 window.addEventListener('pagehide',()=>{closed=true;update();});
 // Retry after user interaction when a browser initially requires a gesture.
 document.addEventListener('pointerdown',update,{passive:true});
 window.addEventListener('message',event=>{
  const frames=[...document.querySelectorAll('iframe')];
  const frame=frames.find(f=>f.contentWindow===event.source);
  if(!frame||event.origin!==new URL(frame.src,location.href).origin||event.data?.type!=='gs26-capture-active')return;
  window.GS26StayAwake.setActive(frame.id||frame.src,!!event.data.active);
 });
})();
