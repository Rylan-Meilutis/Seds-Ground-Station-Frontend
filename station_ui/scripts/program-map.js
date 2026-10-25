// Audience map consumes only the canonical delayed station snapshot. It never
// requests device location or fabricates a GPS lock/manual rocket position.
let loading;
function library(){
 if(window.maplibregl)return Promise.resolve();
 if(loading)return loading;
 loading=new Promise((resolve,reject)=>{
  const css=document.createElement('link');css.rel='stylesheet';css.href='/assets/maplibre-gl.css';document.head.append(css);
  const script=document.createElement('script');script.src='/assets/maplibre-gl.js';script.onload=resolve;script.onerror=()=>{script.remove();css.remove();reject(Error('Map library unavailable'));};document.head.append(script);
 }).catch(error=>{loading=null;throw error;});return loading;
}
const valid=p=>p&&Number.isFinite(p.latitude)&&Math.abs(p.latitude)<=90&&Number.isFinite(p.longitude)&&Math.abs(p.longitude)<=180;
const names=[['rocket','Rocket','#67d5c8'],['ground_station','GroundStation','#8bc8ff'],['rrls','RRLS','#ebc87b']];
export async function mount(host){
 host.replaceChildren();Object.assign(host.style,{position:'absolute',inset:'0',display:'block',background:'#102032'});
 const canvas=document.createElement('div');canvas.style.cssText='position:absolute;inset:0';host.append(canvas);
 const cards=document.createElement('div');cards.setAttribute('aria-label','Programme coordinates');cards.style.cssText='position:absolute;left:12px;bottom:12px;z-index:2;max-width:calc(100% - 24px);display:flex;gap:8px;flex-wrap:wrap;pointer-events:none';host.append(cards);
 const status=document.createElement('div');status.setAttribute('role','status');status.style.cssText='position:absolute;left:12px;top:12px;padding:8px 12px;z-index:2;background:#101923e8;border-radius:8px;font:12px system-ui;color:#dbeafe';status.textContent='Waiting for position data';host.append(status);
 let map,disposed=false,latest={},latestFresh=true,signature='',markers=new Map(),mapError='';
 function update(snapshot={},fresh=true){
  if(disposed)return;latest=snapshot;latestFresh=fresh;cards.replaceChildren();const points=[];
  for(const [key,label,color]of names){
   const reported=snapshot[key],fix=reported&&!reported.manual&&!fresh?{...reported,stale:true}:reported;const card=document.createElement('article');card.style.cssText=`background:#101923ee;border:1px solid #344454;border-radius:9px;padding:9px 12px;min-width:150px;font:13px system-ui;color:#e4ebf0`;
   const title=document.createElement('strong');title.textContent=label;title.style.color=color;card.append(title);
   const value=document.createElement('div');value.style.cssText='margin-top:4px;font-variant-numeric:tabular-nums';value.textContent=valid(fix)?`${fix.latitude.toFixed(5)}°, ${fix.longitude.toFixed(5)}°`:'Position unavailable';card.append(value);
   const detail=document.createElement('div');detail.style.cssText='font-size:11px;color:#a9b7c4;margin-top:4px';
   if(valid(fix)){
    detail.textContent=`${fix.manual?'Saved position':fix.stale?'Stale position':'Live position'} · ${fix.source||'Unknown source'}${fix.stale?' · '+Math.floor((fix.age_ms||0)/1000)+'s old':''}${Number.isFinite(fix.altitude_m)?' · '+fix.altitude_m.toFixed(0)+'m':''}`;
    points.push({key,label,color,fix});
   }else detail.textContent='No valid coordinates received';card.append(detail);cards.append(card);
  }
  const active=new Set(points.map(p=>p.key));for(const [key,m]of markers)if(!active.has(key)){m.remove();markers.delete(key);}
  status.textContent=mapError||(!fresh?'Programme unavailable · last known positions':points.length?'Delayed recovery positions':'Waiting for valid coordinates');
  if(!map)return;
  const anchor=points[0]?.fix.longitude||0;const unwrap=lon=>anchor+((lon-anchor+540)%360-180);
  for(const {key,label,color,fix}of points){
   let marker=markers.get(key);
   if(!marker){const el=document.createElement('div');el.setAttribute('aria-label',label+' position');el.style.cssText='width:16px;height:16px';const dot=document.createElement('span');dot.style.cssText=`display:block;width:100%;height:100%;box-sizing:border-box;border-radius:50%;background:${color};border:2px solid #102032;box-shadow:0 0 0 2px ${color}`;el.append(dot);marker=new window.maplibregl.Marker({element:el}).setLngLat([unwrap(fix.longitude),fix.latitude]).addTo(map);marker.getElement().setAttribute('aria-label',label+' position');markers.set(key,marker);}
   marker.setLngLat([unwrap(fix.longitude),fix.latitude]);marker.getElement().firstElementChild.style.opacity=fix.stale?'.45':'1';
  }
  const key=points.map(p=>[p.key,p.fix.latitude,p.fix.longitude].join(':')).join('|');
  if(key!==signature&&points.length){signature=key;const bounds=new window.maplibregl.LngLatBounds();for(const p of points)bounds.extend([unwrap(p.fix.longitude),p.fix.latitude]);map.fitBounds(bounds,{padding:{top:70,bottom:150,left:45,right:45},maxZoom:15,duration:500});}
 }
 try{
  await library();if(disposed)return {update,destroy(){}};
  map=new window.maplibregl.Map({container:canvas,center:[0,0],zoom:1,interactive:false,attributionControl:true,renderWorldCopies:false,style:{version:8,sources:{offline:{type:'raster',tiles:[new URL('/tiles/{z}/{x}/{y}.jpg',location.origin).href.replaceAll('%7B','{').replaceAll('%7D','}')],tileSize:256,minzoom:0,maxzoom:17}},layers:[{id:'offline',type:'raster',source:'offline'}]}});
  map.on('error',()=>{mapError='Map tiles unavailable · coordinates retained';status.textContent=mapError;});map.on('load',()=>update(latest,latestFresh));
 }catch{mapError='Map unavailable · coordinates retained';status.textContent=mapError;}
 const resized=new ResizeObserver(()=>map?.resize());resized.observe(host);
 return {update,destroy(){disposed=true;resized.disconnect();for(const marker of markers.values())marker.remove();markers.clear();map?.remove();host.replaceChildren();}};
}
