const {test}=require('node:test');const assert=require('node:assert/strict');const vm=require('node:vm');const fs=require('node:fs');const path=require('node:path');
function element(){return {children:[],parentElement:null,dataset:{},attributes:{},ownerDocument:{createElement(){return element()}},setAttribute(name,value){this.attributes[name]=value},querySelector(selector){const id=selector.startsWith('#')?selector.slice(1):null;for(const child of this.children){if(child.id===id)return child;const nested=child.querySelector(selector);if(nested)return nested;}return null},append(child){this.appendChild(child)},appendChild(child){this.insertBefore(child,null)},insertBefore(child,before){const old=child.parentElement;if(old)old.children.splice(old.children.indexOf(child),1);const index=before?this.children.indexOf(before):this.children.length;this.children.splice(index,0,child);child.parentElement=this;},get firstChild(){return this.children[0]||null}}}
test('the same artwork grows without video and returns to the status bar when video resumes',()=>{
 const window={};vm.runInNewContext(fs.readFileSync(path.join(__dirname,'../station_ui/scripts/program-art.js'),'utf8'),{window});
 const holder=element(),footer=element(),inset=element(),banner=element();footer.appendChild(inset);footer.appendChild(banner);
 window.gsProgramArt.layout(holder,inset,footer,true);assert.equal(inset.parentElement,holder);assert.equal(footer.dataset.rocketMain,'true');
 for(let i=0;i<100;i++)window.gsProgramArt.layout(holder,inset,footer,true);
 assert.equal(holder.children.length,1);assert.equal(inset.children.length,1);const equipment=inset.querySelector('#gs26-sequence-equipment');assert.equal(equipment.hidden,true);assert.equal(equipment.attributes['aria-label'],'Programme GSE output evidence');window.gsProgramArt.layout(holder,inset,footer,false);assert.equal(footer.firstChild,inset);assert.equal(footer.children[1],banner);assert.equal(holder.children.length,0);assert.equal(holder.dataset.flat,'false');
});

test('programme artwork uses only explicit boolean mode and never infers firing from phase',()=>{
 const window={};vm.runInNewContext(fs.readFileSync(path.join(__dirname,'../station_ui/scripts/program-art.js'),'utf8'),{window});
 for(const phase of ['Idle','Armed','Ascent','Descent']) {
  const reported=Object.freeze({test_fire_mode:true,phase,sequence_visuals:{igniter:{output:{value:true,evidence:'board_reported'}}}});
  const art=window.gsProgramArt.artwork(reported);assert.equal(art.kind,'test_fire');assert.match(art.description,/Illustrative/);assert.match(art.description,/not ignition or flow evidence/);
  assert.equal(window.gsProgramArt.artwork(Object.freeze({test_fire_mode:false,phase})).kind,'flight');
 }
});
test('missing or malformed programme mode remains explicitly unavailable',()=>{
 const window={};vm.runInNewContext(fs.readFileSync(path.join(__dirname,'../station_ui/scripts/program-art.js'),'utf8'),{window});
 for(const mode of [undefined,null,'true','false',0,1,{},[]]) {
  const art=window.gsProgramArt.artwork({test_fire_mode:mode,phase:'Ascent'});assert.equal(art.kind,'unknown');assert.match(art.label,/unavailable/);
 }
 assert.equal(window.gsProgramArt.artwork(null).kind,'unknown');
});
