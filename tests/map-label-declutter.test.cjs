// Map overlay label decluttering in static/ground_map.js: priorities, the
// collision layout (offset/stack, hide lower priority, pinned always shown),
// obstacles (stations, rocket, overlay points) and hover/tap reveal.
// Data: the backend contract fixtures (a stored plan, a simulated LOS cut).
const {test}=require('node:test');
const assert=require('node:assert/strict');
const fs=require('node:fs');
const vm=require('node:vm');
const path=require('node:path');
const source=fs.readFileSync(path.join(__dirname,'../static/ground_map.js'),'utf8');
const real=name=>JSON.parse(fs.readFileSync(path.join(__dirname,'fixtures/backend-contract',name),'utf8'));
function runtime(){
 const window={addEventListener(){},localStorage:{getItem(){return null;},setItem(){}},dispatchEvent(){return true;}};
 const context=vm.createContext({window,document:{getElementById(){return null;}},navigator:{},console,Uint8Array,ArrayBuffer,URL,Headers,Response,setTimeout,clearTimeout,setInterval,clearInterval,requestAnimationFrame:()=>0,cancelAnimationFrame(){},performance,CustomEvent:class{constructor(t,i){this.type=t;this.detail=i&&i.detail;}}});
 vm.runInContext(source,context);
 return context;
}
const box=(it,p)=>({x0:it.x+p.dx,y0:it.y+p.dy,x1:it.x+p.dx+it.w,y1:it.y+p.dy+it.h});
const overlaps=(a,b)=>a.x0<b.x1&&b.x0<a.x1&&a.y0<b.y1&&b.y0<a.y1;
const layout=(c,items,obstacles=[])=>JSON.parse(JSON.stringify(vm.runInContext('layoutMapOverlayLabels(items,obstacles)',Object.assign(c,{items,obstacles}))));

test('priorities: last fix, predicted now and landing are pinned; apogee and searchers before events and areas',()=>{
 const c=runtime();
 const p=l=>vm.runInContext('mapOverlayLabelPriority(l)',Object.assign(c,{l}));
 for(const role of ['last_fix','predicted_position','landing'])assert.equal(p({kind:'marker',role}),0,role);
 assert.equal(p({kind:'marker',role:'apogee'}),1);
 assert.equal(p({kind:'track',role:'searcher'}),1);
 assert.equal(p({kind:'marker',role:'event'}),2);
 assert.equal(p({kind:'area',role:'landing_ellipse'}),2);
 assert.equal(p({kind:'overlay',role:'flight_plan'}),2);
});

test('two labels on one point: the second moves to a free side instead of covering the first',()=>{
 const c=runtime();
 const items=[{x:200,y:200,w:150,h:18,priority:1,kind:'marker'},{x:201,y:200,w:110,h:18,priority:2,kind:'marker'}];
 const out=layout(c,items);
 assert.ok(out.every(p=>p.visible));
 assert.equal(out[0].dx,12,'the higher priority keeps the right side');
 assert.ok(!overlaps(box(items[0],out[0]),box(items[1],out[1])));
});

test('a crowded point: lower-priority labels are hidden, pinned labels always shown',()=>{
 const c=runtime();
 // Twenty labels on one point: there are fewer than twenty free spots around it.
 const items=Array.from({length:20},(_,i)=>({x:300,y:300,w:120,h:18,priority:i<2?0:2,kind:'marker'}));
 const out=layout(c,items);
 assert.ok(out[0].visible&&out[1].visible,'pinned labels are shown');
 assert.ok(out.some(p=>!p.visible),'some low-priority labels are hidden');
 const shown=items.map((it,i)=>[it,out[i]]).filter(([,p])=>p.visible&&!p.crowded);
 for(let i=0;i<shown.length;i++)for(let j=i+1;j<shown.length;j++)assert.ok(!overlaps(box(...shown[i]),box(...shown[j])),`labels ${i} and ${j} overlap`);
 // A hidden label keeps its default spot for the reveal.
 const hidden=out.find(p=>!p.visible);assert.equal(hidden.dx,12);
 // Pinned labels with nowhere free still show (crowded), never hidden.
 const pinned=Array.from({length:30},()=>({x:50,y:50,w:120,h:18,priority:0,kind:'marker'}));
 const pinnedOut=layout(c,pinned);
 assert.ok(pinnedOut.every(p=>p.visible));assert.ok(pinnedOut.some(p=>p.crowded));
});

test('labels keep clear of obstacles (station badges, the rocket, overlay points)',()=>{
 const c=runtime();
 const items=[{x:100,y:100,w:80,h:18,priority:2,kind:'marker'}];
 const station={x0:110,y0:88,x1:200,y1:112};
 const out=layout(c,items,[station]);
 assert.ok(out[0].visible);assert.ok(out[0].dx<0,'moved to the left of the station');
 assert.ok(!overlaps(box(items[0],out[0]),station));
 // Areas: centred above their top vertex, else below.
 const area=[{x:400,y:100,w:100,h:18,priority:2,kind:'area'}];
 assert.deepEqual(layout(c,area)[0],{dx:-50,dy:-22,visible:true,crowded:false});
 assert.equal(layout(c,area,[{x0:340,y0:70,x1:460,y1:99}])[0].dy,4);
});

test('the prediction scenario: no visible labels overlap and the pinned ones stay',()=>{
 const c=runtime();c.plan=real('flight-plans/plan.json');c.los=real('los-prediction/simulate.json');
 // A Web Mercator projection at zoom 14.6 around the predicted position (the browser scene).
 c.stations=[{role:'gs',lat:31.7535,lon:-102.0505,live:true},{role:'rrls',lat:31.7552,lon:-102.0418,live:false}];
 const result=JSON.parse(JSON.stringify(vm.runInContext(`(()=>{
  setGroundMapStations(stations);
  const {labels}=mapOverlayFeatureCollection(normalizeMapOverlays([los.overlay,plan.overlay]));
  const scale=256*Math.pow(2,14.6);
  const merc=(lat,lon)=>[(lon+180)/360*scale,(1-Math.log(Math.tan(Math.PI/4+lat*Math.PI/360))/Math.PI)/2*scale];
  const c0=merc(los.predicted_now.lat_deg,los.predicted_now.lon_deg);
  const project=(lat,lon)=>{const p=merc(lat,lon);return [p[0]-c0[0]+640,p[1]-c0[1]+430];};
  const items=labels.map(l=>{const [x,y]=project(l.latLng[0],l.latLng[1]);const [w,h]=estimateMapOverlayLabelSize(l.text);return {x,y,w,h,priority:mapOverlayLabelPriority(l),kind:l.kind,text:l.text};});
  const obstacles=mapOverlayLabelObstacles(project,labels);
  return {items,obstacles,out:layoutMapOverlayLabels(items,obstacles)};
 })()`,c)));
 const {items,out,obstacles}=result;
 assert.equal(obstacles.length>=2+7,true,'stations and overlay points are obstacles');
 const visible=items.map((it,i)=>[it,out[i]]).filter(([,p])=>p.visible);
 for(let i=0;i<visible.length;i++)for(let j=i+1;j<visible.length;j++){
  assert.ok(!overlaps(box(...visible[i]),box(...visible[j])),`${visible[i][0].text} overlaps ${visible[j][0].text}`);
 }
 for(const re of [/^Last fix/,/no signal: now$/,/no signal: landing$/,/^Predicted landing$/,/3768 m AGL/]){
  const i=items.findIndex(it=>re.test(it.text));assert.ok(i>=0&&out[i].visible,`${re} is shown`);
 }
 const drogue=items.findIndex(it=>it.text==='Predicted drogue');const apogee=items.findIndex(it=>/3768 m AGL/.test(it.text));
 assert.ok(!out[drogue].visible||!overlaps(box(items[drogue],out[drogue]),box(items[apogee],out[apogee])),'drogue never covers the apogee label');
});

test('hover and tap reveal hidden labels near the pointer and hide them again',()=>{
 const c=runtime();
 vm.runInContext(`mapOverlayLabelEntries=[
  {el:{style:{},dataset:{}},hidden:true,revealed:false,priority:2,point:[100,100]},
  {el:{style:{},dataset:{}},hidden:true,revealed:false,priority:2,point:[300,300]},
  {el:{style:{},dataset:{}},hidden:false,revealed:false,priority:0,point:[101,100]}];`,c);
 assert.equal(vm.runInContext('revealMapOverlayLabelsNear([105,104],18)',c),1);
 const state=()=>JSON.parse(JSON.stringify(vm.runInContext('mapOverlayLabelEntries.map(e=>[e.el.style.visibility||"",e.el.dataset.declutter||""])',c)));
 assert.deepEqual(state(),[['visible','revealed'],['',''],['','']]);
 vm.runInContext('revealMapOverlayLabelsNear(null,0)',c);
 assert.deepEqual(state()[0],['hidden','hidden']);
});

test('the raster fallback draws the same layout and leaves hidden labels out',()=>{
 const c=runtime();c.plan=real('flight-plans/plan.json');
 const texts=[];
 const el=()=>({setAttribute(){},appendChild(child){if(child.tag==='text')texts.push(child.textContent);},style:{}});
 c.document.createElementNS=(ns,tag)=>Object.assign(el(),{tag});
 vm.runInContext('mapOverlayProviders.set("flight_plan",normalizeMapOverlays([1,2,3,4,5].map(i=>({...plan.overlay,id:"p"+i}))))',c);
 // Thirty labels on the same pixel: the pinned ones (landing) are always drawn, others only where they fit.
 vm.runInContext('rasterFallbackOverlaySvg(()=>[100,100],800,600)',c);
 assert.equal(texts.filter(t=>t==='Predicted landing').length,5);
 assert.ok(texts.length<30,'crowded labels are left out');
});
