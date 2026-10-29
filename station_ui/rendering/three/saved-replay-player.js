// Audience-only saved flight player. No live telemetry subscription or commands.
import './vehicle-renderer.js';
export function latestSaved(rows,kind,sender,at){
 let selected=null;
 for(const row of rows){if(row.data_type!==kind||(sender&&row.sender_id!==sender)||!Number.isFinite(row.timestamp_ms)||row.timestamp_ms>at)continue;
  if(!selected||row.timestamp_ms>selected.timestamp_ms||(row.timestamp_ms===selected.timestamp_ms&&row.id>selected.id))selected=row;
 }
 return selected;
}
export function savedValue(rows,binding,at,hold=false){
 if(!binding)return null;const row=latestSaved(rows,binding.data_type,binding.sender_id,at);
 if(!row||(!hold&&at-row.timestamp_ms>5000))return null;
 const value=row.values?.[binding.index||0];if(!Number.isFinite(value))return null;
 const result=value*(binding.scale??1)+(binding.offset||0);return Number.isFinite(result)?result:null;
}
export function replayPosition(plan,serverNow){
 const duration=Math.max(0,plan.end_ms-plan.start_ms),elapsed=plan.paused?0:Math.max(0,serverNow-plan.anchor_server_ms)*plan.speed;
 const value=plan.position_ms+elapsed;return plan.loop_playback&&duration>0?value%duration:Math.min(duration,Math.max(0,value));
}
function normalized(value){return String(value||'').replace(/[^a-z0-9]/gi,'').toLowerCase();}
export function savedFrame(rows,config,at,start,gpsSender){
 const phase=latestSaved(rows,'FLIGHT_STATE','FC',at)?.phase||'Unknown';
 const get=b=>savedValue(rows,b,at);
 const attitude=[get(config.attitude?.pitch),get(config.attitude?.yaw),get(config.attitude?.roll)];
 let gps=null;const candidates=['GPS_DATA','GPS','ROCKET_GPS'].map(type=>latestSaved(rows,type,gpsSender,at)).filter(Boolean).sort((a,b)=>b.timestamp_ms-a.timestamp_ms||b.id-a.id);
 const fix=candidates[0];if(fix&&at-fix.timestamp_ms<=5000){const v=fix.values?.slice(0,3);if(v?.length===3&&v.every(Number.isFinite)&&Math.abs(v[0])<=90&&Math.abs(v[1])<=180&&v[2]>=-20000&&v[2]<=10000000)gps=v;}
 const motions=(config.motions||[]).slice(0,128).map(m=>({...m,value:m.binding?get(m.binding):Object.entries(m.phase_values||{}).find(([name])=>normalized(name)===normalized(phase))?.[1]??m.phase_values?.['*']??null}));
 const clip=Object.entries(config.phase_animations||{}).find(([name])=>normalized(name)===normalized(phase))?.[1]||null;
 return {motions,attitude,gps,clip,orbit:config.camera_orbit||'',replay_time_seconds:Math.max(0,at-start)/1000,replay_timestamp_ms:at,phase};
}
export class SavedReplayPlayer {
 constructor(viewer,{ticket='',fetcher=fetch,status=()=>{},clock=()=>performance.now(),automatic=true}={}){
  this.viewer=viewer;this.ticket=ticket;this.fetcher=fetcher.bind(globalThis);this.status=status;this.clock=clock;this.stopped=false;this.rows=[];this.generation=0;this.abort=new AbortController();this.dataAbort=new AbortController();
  if(automatic){this.poll();this.pollTimer=setInterval(()=>this.poll(),1000);this.frameTimer=setInterval(()=>this.frame(),50);}
 }
 url(kind,params={}){const q=new URLSearchParams({...params,...(this.ticket?{ticket:this.ticket}:{})});return '/api/media-assets/recorded-vehicle/'+kind+'?'+q;}
 async json(kind,params={},signal=this.abort.signal){const r=await this.fetcher(this.url(kind,params),{signal,credentials:'same-origin',cache:'no-store'});if(!r.ok)throw Error('Saved replay unavailable');return r.json();}
 async poll(){if(this.stopped||this.polling)return;this.polling=true;
  try{const state=await this.json('state');if(this.stopped)return;const p=state.plan;
   if(!p||!Number.isSafeInteger(p.upper_id)||!Number.isSafeInteger(p.start_ms)||!Number.isSafeInteger(p.end_ms)||p.start_ms<0||p.end_ms>8640000000000000||p.end_ms<p.start_ms||!Number.isFinite(p.speed)||p.speed<.25||p.speed>4)throw Error('Invalid saved replay');
   const identity=JSON.stringify([p.recording_id,p.upper_id,p.start_ms,p.end_ms,state.config]);
   if(identity!==this.identity){
    // Snapshot growth replaces buffers in the background; a new selection clears the old flight.
    const old=this.state,prior=old?.plan;
    const growth=prior&&p.recording_id===prior.recording_id&&p.start_ms===prior.start_ms&&p.upper_id>=prior.upper_id&&p.end_ms>=prior.end_ms&&JSON.stringify(state.config)===JSON.stringify(old.config);
    this.identity=identity;this.generation++;this.dataAbort.abort();this.dataAbort=new AbortController();this.loading=false;this.trackLoading=false;this.chunk=null;this.nextTrackRetry=0;this.nextWindowRetry=0;this.trackNeedsRefresh=true;
    if(!growth){this.rows=[];this.buffer=null;this.track=null;this.viewer.setAttribute('data-track','null');this.viewer.setAttribute('data-state','{}');this.viewer.setAttribute('src',state.config?.model_url||'/assets/models/vehicle.glb');}
   }
   this.state=state;this.receivedAt=this.clock();this.failed=false;if(this.trackNeedsRefresh&&this.clock()>=(this.nextTrackRetry||0))this.loadTrack(this.generation);this.frame();
  }catch(e){if(!this.stopped){this.failed=true;this.status('Recorded replay unavailable');}}
  finally{this.polling=false;}
 }
 async loadTrack(generation){if(this.trackLoading)return;this.trackLoading=generation+1;try{const p=this.state?.plan;if(!p)return;const track=await this.json('trajectory',{recording_id:p.recording_id,upper_id:p.upper_id},this.dataAbort.signal);if(this.stopped||generation!==this.generation)return;if(track.recording_id!==p.recording_id||track.upper_id!==p.upper_id||!Array.isArray(track.points)||track.points.length>8192)throw Error('Invalid saved trajectory');this.track=track;this.trackNeedsRefresh=false;this.viewer.setAttribute('data-track',JSON.stringify(track));}catch(e){if(generation===this.generation&&!this.stopped){this.nextTrackRetry=this.clock()+2000;this.status('Saved trajectory unavailable');}}finally{if(generation===this.generation)this.trackLoading=false;}}
 async loadWindow(at){if(this.loading||!this.state||this.clock()<(this.nextWindowRetry||0))return;const p=this.state.plan,generation=this.generation,chunk=Math.floor((at-p.start_ms)/10000);this.loading=generation+1;
  const start=Math.max(p.start_ms,p.start_ms+chunk*10000-5000),end=Math.min(p.end_ms,start+25000);
  try{let after=0,rows=[];for(let i=0;i<32;i++){
    const page=await this.json('window',{recording_id:p.recording_id,upper_id:p.upper_id,after_id:after,start_ms:start,end_ms:end},this.dataAbort.signal);
    if(this.stopped||generation!==this.generation)return;
    if(page.recording_id!==p.recording_id||page.upper_id!==p.upper_id||!Array.isArray(page.rows)||!Array.isArray(page.seeds)||(page.has_more&&page.next_id<=after))throw Error('Invalid saved cursor');
    rows.push(...page.seeds,...page.rows);if(rows.length>16384)throw Error('Saved replay window too large');
    if(!page.has_more){this.rows=rows;this.buffer={start,end};this.chunk=chunk;return;}after=page.next_id;
   }throw Error('Saved replay window too large');
  }catch(e){if(!this.stopped&&generation===this.generation){this.nextWindowRetry=this.clock()+2000;this.status('Saved samples unavailable');}}
  finally{if(generation===this.generation)this.loading=false;}
 }
 frame(){if(this.stopped||this.failed||!this.state)return;const p=this.state.plan;
  const at=p.start_ms+replayPosition(p,this.state.server_now_ms+Math.max(0,this.clock()-this.receivedAt));
  const chunk=Math.floor((at-p.start_ms)/10000);if(this.chunk!==chunk&&!this.loading)this.loadWindow(at);
  if(!this.buffer||at<this.buffer.start||at>this.buffer.end){this.status('Buffering recorded samples…');return;}
  const frame=savedFrame(this.rows,this.state.config||{},at,p.start_ms,this.track?.source_sender_id||'FC');
  this.viewer.setAttribute('data-state',JSON.stringify(frame));this.status((p.paused?'Paused · ':'')+new Date(at).toISOString().replace('T',' ').replace('Z',' UTC')+' · '+frame.phase);
 }
 stop(){this.stopped=true;this.generation++;clearInterval(this.pollTimer);clearInterval(this.frameTimer);this.abort.abort();this.dataAbort.abort();this.viewer.setAttribute('data-track','null');this.viewer.setAttribute('data-state','{}');this.rows=[];this.buffer=null;}
}
