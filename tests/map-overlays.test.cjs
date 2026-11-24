// MapOverlay plumbing in static/ground_map.js: normalisation, palette styles,
// the GeoJSON the 2D map draws, provider merging and the published event.
// Data: tests/fixtures/flight-predict-mock (hand-written mocks of the contract).
const {test}=require('node:test');
const assert=require('node:assert/strict');
const fs=require('node:fs');
const vm=require('node:vm');
const path=require('node:path');
const source=fs.readFileSync(path.join(__dirname,'../static/ground_map.js'),'utf8');
const mock=name=>JSON.parse(fs.readFileSync(path.join(__dirname,'fixtures/flight-predict-mock',name),'utf8'));
function runtime(){
 const events=[];
 const window={addEventListener(){},localStorage:{getItem(){return null;},setItem(){}},dispatchEvent(e){events.push(e);return true;}};
 class CustomEvent{constructor(type,init){this.type=type;this.detail=init&&init.detail;}}
 const context=vm.createContext({window,document:{getElementById(){return null;}},navigator:{},console,Uint8Array,ArrayBuffer,URL,Headers,Response,setTimeout,clearTimeout,setInterval,clearInterval,requestAnimationFrame:fn=>0,cancelAnimationFrame(){},performance,CustomEvent});
 vm.runInContext(source,context);
 context.events=events;
 return context;
}
const plan=mock('plan.json');
const los=mock('los-predicting.json');

test('setGroundMapOverlays is installed on window and consumes pending overlays',()=>{
 const window={addEventListener(){},localStorage:{getItem(){return null;}},dispatchEvent(){return true;},__gs26_pending_map_overlays:{los:[los.overlay]}};
 const c=vm.createContext({window,document:{getElementById(){return null;}},navigator:{},console,Uint8Array,ArrayBuffer,URL,Headers,Response,setTimeout,clearTimeout,setInterval,clearInterval,requestAnimationFrame:()=>0,performance,CustomEvent:class{constructor(t,i){this.type=t;this.detail=i&&i.detail;}}});
 vm.runInContext(source,c);
 assert.equal(typeof window.setGroundMapOverlays,'function');
 assert.equal(window.__gs26_map_overlays.length,1);
 assert.equal(window.__gs26_map_overlays[0].kind,'los_prediction');
});

test('normalisation keeps valid parts, drops invalid points and repeated ring closers',()=>{
 const c=runtime();
 c.raw={id:'x',kind:'flight_plan',label:'Predicted: test',predicted:true,
  tracks:[{id:'boost',phase:'boost',points:[[31.7,-102.0,890],[91,0,0],['a','b'],[31.71,-102.01,null]]},{id:'empty',points:[]}],
  markers:[{id:'m',kind:'landing',label:'Landing',lat_deg:31.72,lon_deg:-102.02,alt_msl_m:null},{id:'bad',lat_deg:'n/a',lon_deg:0}],
  areas:[{id:'a',kind:'landing_ellipse',polygon:[[31,-102],[31.1,-102],[31.1,-102.1],[31,-102]]},{id:'tiny',polygon:[[31,-102],[31.1,-102]]}]};
 const o=JSON.parse(JSON.stringify(vm.runInContext('normalizeMapOverlay(raw)',c)));
 assert.equal(o.tracks.length,1);
 assert.deepEqual(o.tracks[0].points,[[31.7,-102,890],[31.71,-102.01,null]]);
 assert.equal(o.tracks[0].style,'predicted','predicted overlays default to the predicted style');
 assert.equal(o.markers.length,1);
 assert.equal(o.markers[0].alt_msl_m,null);
 assert.equal(o.areas.length,1);
 assert.equal(o.areas[0].polygon.length,3,'closing point removed');
 assert.equal(vm.runInContext('normalizeMapOverlay({kind:"flight_plan"})',c),null,'an overlay needs an id');
 assert.equal(vm.runInContext('normalizeMapOverlays([{id:"a"},{id:"a"},null,{id:"b"}]).length',c),2);
});

test('flight-plan overlay: phase colours, dashed and lighter tracks, purple dashed landing ellipse',()=>{
 const c=runtime();c.plan=plan;
 const {collection,labels}=JSON.parse(JSON.stringify(vm.runInContext('mapOverlayFeatureCollection(normalizeMapOverlays([plan.overlay]))',c)));
 const tracks=collection.features.filter(f=>f.properties.role==='ov-track');
 assert.deepEqual(tracks.map(t=>t.properties.phase),['boost','coast','drogue','main']);
 assert.deepEqual(tracks.map(t=>t.properties.color),['#f97316','#facc15','#38bdf8','#22c55e']);
 for(const t of tracks){assert.equal(t.properties.dashed,true);assert.equal(t.properties.opacity,0.55);assert.equal(t.geometry.type,'LineString');}
 // GeoJSON is lon,lat.
 assert.equal(tracks[0].geometry.coordinates[0][0],plan.overlay.tracks[0].points[0][1]);
 const area=collection.features.find(f=>f.properties.role==='ov-area');
 assert.equal(area.properties.color,'#a78bfa');assert.equal(area.properties.fillOpacity,0.12);assert.equal(area.properties.dashed,true);
 const ring=area.geometry.coordinates[0];assert.deepEqual(ring[0],ring[ring.length-1],'GeoJSON polygons are closed');
 const apogee=collection.features.find(f=>f.properties.kind==='apogee');
 assert.equal(apogee.properties.fill,'#ffffff');assert.equal(apogee.properties.color,'#111827');
 const texts=labels.map(l=>l.text);
 assert.ok(texts.includes('Predicted: SWEET V6 8.5 kg'),'the overlay label is visible');
 assert.ok(texts.includes('Predicted landing'));
 assert.ok(texts.includes('95% landing area'));
});

test('LOS overlay: pink dashed path, hollow predicted ring, 15% uncertainty areas, every element labelled Predicted',()=>{
 const c=runtime();c.los=los;
 const {collection,labels}=JSON.parse(JSON.stringify(vm.runInContext('mapOverlayFeatureCollection(normalizeMapOverlays([los.overlay]))',c)));
 const track=collection.features.find(f=>f.properties.role==='ov-track');
 assert.equal(track.properties.color,'#f472b6');assert.equal(track.properties.dashed,true);
 const ring=collection.features.find(f=>f.properties.kind==='predicted_position');
 assert.equal(ring.properties.fillOpacity,0,'hollow');assert.equal(ring.properties.color,'#f472b6');
 for(const area of collection.features.filter(f=>f.properties.role==='ov-area')){
  assert.equal(area.properties.color,'#f472b6');assert.equal(area.properties.fillOpacity,0.15);assert.equal(area.properties.dashed,true);
 }
 const markerLabels=labels.filter(l=>l.kind==='marker').map(l=>l.text);
 assert.ok(markerLabels.includes('Predicted: no signal · Predicted position'));
 assert.ok(markerLabels.includes('Predicted: no signal · Predicted landing'));
 assert.ok(markerLabels.includes('Last fix'),'the last real fix keeps its own label');
 for(const l of labels.filter(l=>l.kind!=='marker'))assert.match(l.text,/Predicted: no signal/);
});

test('search coverage: green swaths at 30%, red uncovered at 25%, per-searcher track colours',()=>{
 const c=runtime();
 c.overlay={id:'search:1',kind:'search_coverage',label:'Search',tracks:[{id:'s1',label:'Alex',style:'search',points:[[31.7,-102],[31.71,-102]]},{id:'s2',label:'Sam',style:'search',points:[[31.7,-102.01],[31.71,-102.01]]}],
  areas:[{id:'c',kind:'search_swath',polygon:[[31.7,-102],[31.71,-102],[31.71,-102.01]]},{id:'u',kind:'uncovered',polygon:[[31.72,-102],[31.73,-102],[31.73,-102.01]]}]};
 const {collection,labels}=JSON.parse(JSON.stringify(vm.runInContext('mapOverlayFeatureCollection(normalizeMapOverlays([overlay]))',c)));
 const tracks=collection.features.filter(f=>f.properties.role==='ov-track');
 assert.deepEqual(tracks.map(t=>t.properties.color),['#0ea5e9','#eab308']);
 assert.ok(tracks.every(t=>t.properties.dashed===false));
 const [covered,uncovered]=collection.features.filter(f=>f.properties.role==='ov-area');
 assert.equal(covered.properties.color,'#22c55e');assert.equal(covered.properties.fillOpacity,0.30);
 assert.equal(uncovered.properties.color,'#ef4444');assert.equal(uncovered.properties.fillOpacity,0.25);
 assert.deepEqual(labels.map(l=>l.text),['Alex','Sam']);
});

test('providers merge, an empty list clears one, and every change is published with an event',()=>{
 const c=runtime();c.plan=plan;c.los=los;c.events.length=0;
 vm.runInContext('setGroundMapOverlays("flight_plan",[plan.overlay]);setGroundMapOverlays("los",[los.overlay]);',c);
 assert.deepEqual(Array.from(c.window.__gs26_map_overlays,o=>o.kind),['flight_plan','los_prediction']);
 assert.equal(c.events.length,2);assert.equal(c.events[1].type,'gs26-map-overlays');
 assert.equal(c.events[1].detail.overlays.length,2);
 vm.runInContext('setGroundMapOverlays("flight_plan",[])',c);
 assert.deepEqual(Array.from(c.window.__gs26_map_overlays,o=>o.kind),['los_prediction']);
 vm.runInContext('setGroundMapOverlays("",[plan.overlay])',c);
 assert.equal(c.window.__gs26_map_overlays.length,1,'a provider id is required');
});

test('the 2D style carries the overlay source and layers below the real rocket and stations',()=>{
 const c=runtime();
 const style=JSON.parse(JSON.stringify(vm.runInContext('makeMapStyle("https://station.test/tiles/{z}/{x}/{y}.jpg", 12)',c)));
 assert.ok(style.sources['gs26-map-overlays-source']);
 const ids=style.layers.map(l=>l.id);
 for(const id of ['gs26-map-overlays-area-fill','gs26-map-overlays-track-dash','gs26-map-overlays-marker'])assert.ok(ids.includes(id),id);
 assert.ok(ids.indexOf('gs26-map-overlays-marker')<ids.indexOf('gs26-rocket-layer'),'the real rocket draws on top');
 assert.ok(ids.indexOf('gs26-map-overlays-area-fill')>ids.indexOf('gs26-raster-layer'));
});

test('the map source receives the merged overlays and labels become markers',()=>{
 const c=runtime();c.plan=plan;
 const data=[];const added=[];
 c.window.maplibregl={Map:function(){},Marker:class{constructor(o){this.el=o.element;}setLngLat(p){this.p=p;return this;}addTo(){added.push(this);return this;}remove(){}}};
 c.document.createElement=()=>({dataset:{},style:{}});
 vm.runInContext('groundMap={getSource:id=>id===MAP_OVERLAYS_SOURCE_ID?{setData:d=>data.push(d)}:null};mapReady=true;',c);
 c.data=data;
 vm.runInContext('setGroundMapOverlays("flight_plan",[plan.overlay])',c);
 assert.equal(data.length,1);
 assert.ok(data[0].features.length>=7);
 assert.ok(added.length>=3);
 vm.runInContext('setGroundMapOverlays("flight_plan",[plan.overlay])',c);
 assert.equal(data.length,1,'an unchanged list is not redrawn');
});
