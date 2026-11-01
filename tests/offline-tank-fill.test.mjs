import fs from 'node:fs';
import vm from 'node:vm';
import assert from 'node:assert/strict';

// Exercise the actual standalone player's DOM path without a browser/network.
class Element {
 constructor(tag){this.tagName=tag;this.children=[];this.dataset={};this.style={};this.attributes={};this.isConnected=true;this.textContent='';}
 append(...children){for(const child of children){child.parent=this;this.children.push(child);}}
 replaceChildren(...children){this.children=[];this.append(...children);}
 setAttribute(name,value){this.attributes[name]=String(value);}
 querySelectorAll(selector){assert.equal(selector,'[data-valve]');return descendants(this).filter(n=>Object.hasOwn(n.attributes,'data-valve'));}
 remove(){if(this.parent)this.parent.children=this.parent.children.filter(n=>n!==this);}
}
function descendants(root){return root.children.flatMap(child=>[child,...descendants(child)]);}
const source=fs.readFileSync(process.env.GS_OFFLINE_PLAYER_SOURCE||new URL('../station_ui/rendering/three/offline-replay.js',import.meta.url),'utf8');
const context=vm.createContext({TextEncoder,performance:{now:()=>0},setInterval:()=>1,clearInterval:()=>{},document:{createElement:tag=>new Element(tag),createElementNS:(_,tag)=>new Element(tag)}});
vm.runInContext(source,context);
function player(unit,{min=0,max=100,value=42}={}){
 const root=new Element('main');
 const input={format:'seds-replay',version:1,mode:'flight',schema:{types:[]},layout:{},valve_mappings:[],view_config:{stages:[],ground_systems:[{id:'tank',label:'Oxidizer tank',kind:'tank',unit,min,max,binding:{data_type:'TANK',index:0}}]},samples:[{id:1,timestamp_ms:1000,sender_id:'VB',data_type:'TANK',values:[value]},{id:2,timestamp_ms:9000,sender_id:'VB',data_type:'OTHER',values:[0]}]};
 return {root,player:context.GSOfflineReplay.mount(root,input)};
}
for(const unit of ['psi','bar','kPa','V','']){
 const {root}=player(unit);
 assert.equal(descendants(root).filter(n=>n.tagName==='progress').length,0,`${unit} pressure/other quantity must not become tank fill`);
 assert.equal(descendants(root).filter(n=>n.dataset.tankFill==='unknown').length,1);
}
for(const [unit,min,max,value,fraction]of [['%',0,100,42,.42],['kg',1,11,6,.5]]){
 const {root,player:p}=player(unit,{min,max,value});
 assert.equal(descendants(root).find(n=>n.tagName==='progress')?.value,fraction);
 p.seek(9000);
 assert.equal(descendants(root).filter(n=>n.tagName==='progress').length,0,'stale measurements must not leave a false level');
 assert.equal(descendants(root).filter(n=>n.dataset.tankFill==='unknown').length,1);
 p.dispose();
}
console.log('PASS actual offline DOM: pressure never renders tank level; explicit percent/mass range renders; stale level becomes Unknown');
