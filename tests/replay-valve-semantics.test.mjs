import fs from 'node:fs';import vm from 'node:vm';import assert from 'node:assert/strict';
const folder=new URL('../station_ui/rendering/three/',import.meta.url),load=async n=>import('data:text/javascript;base64,'+fs.readFileSync(new URL(n,folder)).toString('base64'));
const T=await load('three.core.js'),{createReplayAssembly,replayValveState}=await load('replay-scene.js');
class Element {
 constructor(tag){this.tagName=tag;this.children=[];this.dataset={};this.style={};this.attributes={};this.isConnected=true;this.textContent='';}
 append(...children){for(const child of children){child.parent=this;this.children.push(child);}}
 replaceChildren(...children){this.children=[];this.append(...children);}
 setAttribute(k,v){this.attributes[k]=String(v);}
 querySelectorAll(selector){assert.equal(selector,'[data-valve]');return descendants(this).filter(n=>Object.hasOwn(n.attributes,'data-valve'));}
 remove(){if(this.parent)this.parent.children=this.parent.children.filter(n=>n!==this);}
}
const descendants=e=>e.children.flatMap(c=>[c,...descendants(c)]);
const context=vm.createContext({TextEncoder,performance:{now:()=>0},setInterval:()=>1,clearInterval:()=>{},document:{createElement:tag=>new Element(tag),createElementNS:(_,tag)=>new Element(tag)}});vm.runInContext(fs.readFileSync(new URL('offline-replay.js',folder),'utf8'),context);const api=context.GSOfflineReplay;
const cases=[undefined,{},...['Open','Closed',' open ',' cLoSeD ','Unknown','On','Off','Opened','Shut','Ouvert','Fermé','',null,1].map(state=>({state})),{true_label:'Closed',false_label:'Open'},{true_label:'On',false_label:'Off'},{state:'Closed',true_label:'Open'},{state:null,true_label:'Open'},{state:undefined,true_label:'Closed',false_label:'Open'}];
for(const value of [0,1,null,undefined,NaN,Infinity,-1,2,'1',true])for(const row of cases){const actual=replayValveState(value,row);assert.equal(actual,api.replayValveState(value,row),'shared geometry/SVG classifiers agree');if(value!==0&&value!==1)assert.equal(actual,'unknown','saved text never overrides invalid raw value');}
assert.equal(replayValveState(1),'open');assert.equal(replayValveState(0),'closed');assert.equal(replayValveState(1,{true_label:'Closed'}),'closed');assert.equal(replayValveState(0,{false_label:'Open'}),'open');assert.equal(replayValveState(1,{state:'Closed',true_label:'Open'}),'closed');assert.equal(replayValveState(1,{state:undefined,true_label:'Closed'}),'closed');
const fixture=JSON.parse(fs.readFileSync(new URL('./fixtures/offline-valve-semantics.json',import.meta.url))),snapshot=JSON.stringify(fixture);
for(const mode of ['flight','test_fire']){
 const root=new Element('main'),player=api.mount(root,{...fixture,mode}),a=createReplayAssembly(T,mode);
 for(const [at,expected]of [[1000,['closed','open','unknown','unknown','closed','unknown','unknown','closed']],[2000,['open','closed','unknown','unknown','open','unknown','unknown','open']],[9000,Array(8).fill('unknown')]]){
  const frame=api.state(player.package,at),before=JSON.stringify(frame.valves);a.update(frame);player.seek(at);assert.equal(JSON.stringify(frame.valves),before,'raw values and exact custom saved labels remain untouched');
  assert.deepEqual(a.valves.map(v=>v.part.userData.state),expected);
  const glyphs=descendants(root).filter(n=>Object.hasOwn(n.attributes,'data-valve'));
  for(const [i,status]of expected.entries()){
   const color=status==='open'?'22c55e':status==='closed'?'f59e0b':'64748b';assert.equal(a.valves[i].handle.material.color.getHexString(),color);assert.equal(a.valves[i].handle.rotation.z,status==='open'?Math.PI/2:status==='closed'?0:Math.PI/4);assert.equal(glyphs[i].children[0].attributes.fill,'#'+color,'SVG fallback follows saved physical position semantics');
  }
  if(at===1000){assert.equal(frame.valves[2].state,'On');assert.equal(frame.valves[3].state,'Fermé');assert.equal(frame.valves[4].state,' cLoSeD ');assert.equal(frame.valves[6].value,2);}
 }
 a.dispose();player.dispose();
}
assert.equal(JSON.stringify(fixture),snapshot,'original recording data is preserved');console.log('PASS inverted/case/custom/localized/default saved valves: actual Three geometry and SVG parity, strict raw validity, stale unknown, exact saved labels preserved');
