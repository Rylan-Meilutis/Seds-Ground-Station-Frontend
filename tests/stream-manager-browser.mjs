// The visual stream manager in the real Wasm bundle against the shared fake
// backend (tests/dashboard-customization.mjs, reused without changing it):
// click a scene -> Preview, Cut / Fade with a duration -> Program, hotkeys,
// tally, drag a source into a slot (and tap-to-place), the layout editor
// (preset, pointer drag, typed geometry) and phone/tablet layouts. The
// monitors run the actual programme page. Never a live station.
// Build with `dx build --platform web`; SHOTS=dir saves screenshots.
import fs from 'node:fs';
import assert from 'node:assert/strict';
import {pathToFileURL} from 'node:url';
import path from 'node:path';
const sourcePath='tests/dashboard-customization.mjs';
const original=fs.readFileSync(sourcePath,'utf8');
let fixture=original.slice(0,original.indexOf('try{\n  const context='));
const studioHandlers=String.raw`
  const studioFile=f=>fs.readFileSync(f);
  if(pathname==='/api/media-assets/program'){res.setHeader('Content-Type','text/html');res.end(studioFile('station_ui/pages/program.html'));return;}
  const studioAssets={'/assets/program-studio.js':'station_ui/scripts/program-studio.js','/assets/program-diagram.js':'station_ui/scripts/program-diagram.js','/assets/program-art.js':'station_ui/scripts/program-art.js','/assets/stream-sequence-visuals.js':'station_ui/scripts/stream-sequence-visuals.js','/assets/hls.min.js':'station_ui/vendor/hls.min.js','/assets/stay-awake.js':'station_ui/scripts/stay-awake.js'};
  if(studioAssets[pathname]){res.setHeader('Content-Type','text/javascript');res.end(studioFile(studioAssets[pathname]));return;}
  if(pathname.startsWith('/assets/program-')||pathname.startsWith('/api/media-assets/program/')&&pathname!=='/api/media-assets/program/state'){res.setHeader('Content-Type','application/json');res.end(pathname.endsWith('.js')?'':'{}');return;}
  if(pathname==='/api/media-assets/program/state'){
   const q=new URL(req.url,'http://localhost').searchParams,id=q.get('scene')||studio.program_scene_id;studioStateRequests.push(q.get('scene')||'program');
   const scene=studio.scenes.find(s=>s.id===id),layout=scene&&studio.layouts.find(l=>l.id===scene.layout_id);
   const slots=layout?[...layout.slots].sort((a,b)=>(a.z||0)-(b.z||0)).map(s=>{const sid=scene.sources[s.id],src=studio.sources.find(x=>x.id===sid);return {...s,source:src?{id:src.id,kind:src.kind,view:src.view,label:src.label,transparent:src.transparent,stream_id:null}:null};}):[];
   res.setHeader('Content-Type','application/json');
   res.end(JSON.stringify({server_now_ms:Date.now(),broadcast:{...broadcastState,layout:studio.on_air?'studio':broadcastState.layout,delay_seconds:3,replay:null},streams:[],diagram_topology:slots.some(s=>s.source?.view==='diagram')?topology:null,
    studio:scene?{scene_id:scene.id,name:scene.name,program_bar:scene.program_bar,slots,mode:q.get('scene')?'preview':'program',take_seq:studio.take_seq,transition:studio.transition}:null,
    telemetry:{phase:'Fill',scene:'inherit',profile:'ground',cycle_seconds:5,stats_per_page:3,t_clock:'T- 00:09:'+String(59-Math.floor(Date.now()/1000)%60).padStart(2,'0')+'.00',stats:[{label:'Altitude',value:12.5,unit:'m',precision:1},{label:'Tank pressure',value:612.3,unit:'psi',precision:1}],diagram:{rows:[],boards:[],controls:[],status:{igt:false}}}}));return;
  }
  if(pathname==='/api/dashboard_status'){res.setHeader('Content-Type','application/json');res.end(JSON.stringify({t_clock:'T- 00:09:30.00',stats:[{label:'Altitude',value:12.5,unit:'m',precision:1}]}));return;}
  if(pathname.startsWith('/api/studio')){
   res.setHeader('Content-Type','application/json');
   if(req.method==='POST'){let raw='';for await(const part of req)raw+=part;const body=JSON.parse(raw);studioPosts.push({path:pathname,body});
    if(pathname==='/api/studio/take'){studio.program_scene_id=body.scene_id;studio.transition=body.transition;studio.take_seq++;studio.on_air=true;}
    else{if(body.revision!==studio.revision){res.statusCode=409;res.end('Studio changed; reload before saving');return;}if(pathname==='/api/studio/scenes')studio.scenes=body.scenes;else studio.layouts=body.layouts;}
    studio.revision++;
   }
   res.end(JSON.stringify(studio));return;
  }
`;
fixture=fixture.replace("  if(pathname.startsWith('/api/')){\n",studioHandlers+"  if(pathname.startsWith('/api/')){\n");
// The station's own studio view (contract fixture) with one live camera and a real diagram.
fixture=fixture.replace("let broadcastState=example('live-streams').broadcast;",String.raw`let broadcastState=example('live-streams').broadcast;
const studio=JSON.parse(fs.readFileSync('tests/fixtures/backend-contract/studio/view.json','utf8'));
studio.program_url='/api/media-assets/program?ticket=fixture';
for(const s of studio.sources){if(s.view==='diagram'){s.id='diagram:flight';s.label='Flight';}if(s.kind!=='camera'&&s.state==='no_signal')s.state='live';}
for(const sc of studio.scenes)for(const [k,v]of Object.entries(sc.sources))if(v.startsWith('diagram:'))sc.sources[k]='diagram:flight';
studio.sources.unshift({id:'camera:pad-wide',kind:'camera',view:'camera',label:'Pad wide',state:'live',transparent:false,stream_id:'pad-wide',preview_url:'/test-camera?ticket=fixture'});
const topologyDir='src/telemetry_dashboard/live_diagrams/topologies';
const topology=JSON.parse(fs.readFileSync(topologyDir+'/'+(fs.existsSync(topologyDir+'/flight.json')?'flight.json':'fill.json'),'utf8'));
const studioPosts=[],studioStateRequests=[];`);
fixture=fixture.replace("from './reset-confirmation-owned-shield.mjs'","from "+JSON.stringify(pathToFileURL(path.resolve('tests/reset-confirmation-owned-shield.mjs')).href));
fixture=fixture.replace("process.env.PLAYWRIGHT_MODULE || 'playwright'",JSON.stringify(process.env.PLAYWRIGHT_MODULE?pathToFileURL(path.resolve(process.env.PLAYWRIGHT_MODULE)).href:import.meta.resolve('playwright')));
fixture+=String.raw`try{
 const shots=process.env.SHOTS?path.resolve(process.env.SHOTS):null;if(shots)fs.mkdirSync(shots,{recursive:true});
 const context=await browser.newContext({viewport:{width:1440,height:1000}});
 await context.addInitScript(({session,origin})=>{localStorage.setItem('navigation-style','tabs');localStorage.setItem('auth_session_v1',JSON.stringify({entries:[{host_scope:origin,updated_at_ms:Date.now(),session:{token:'ui-token',session,remember_me:true}}]}));},{session,origin});
 const page=await context.newPage(),errors=[];page.on('pageerror',e=>errors.push(e.message));await page.goto(origin);
 await page.getByRole('button',{name:/Choose tab/}).click();await page.getByRole('button',{name:'Stream Manager',exact:true}).click();
 const manager=page.getByRole('region',{name:'Stream manager'});await manager.waitFor();
 const scenes=manager.getByRole('group',{name:'Scenes'});await scenes.getByRole('button',{name:/^Scene 1:/}).waitFor();
 assert.equal(await manager.locator('select').count(),0,'no drop-downs in the stream manager');
 const sceneButton=n=>scenes.getByRole('button',{name:new RegExp('^Scene '+n+':')});
 const preview=manager.getByRole('figure',{name:'Preview monitor'}),program=manager.getByRole('figure',{name:'Program monitor'});

 // Click a scene: it loads into Preview (the programme page drawing that scene).
 await sceneButton(2).click();
 assert.equal(await sceneButton(2).getAttribute('aria-pressed'),'true');assert.equal(await sceneButton(2).getAttribute('data-tally'),'preview');
 const second=studio.scenes[1].id;
 await page.waitForFunction(id=>[...document.querySelectorAll('iframe[title="Preview monitor"]')].some(f=>f.src.includes('scene='+id)),second);
 for(let i=0;i<100&&!studioStateRequests.includes(second);i++)await page.waitForTimeout(50);assert(studioStateRequests.includes(second),'the Preview monitor runs the programme page for that scene');
 assert.match(await program.locator('iframe').getAttribute('src'),/monitor=1/);

 // Cut: Preview goes to Program; its tally turns red.
 await manager.getByRole('button',{name:'Cut',exact:true}).click();
 await page.waitForFunction(()=>document.querySelector('.sw-scene[data-tally=program],.sw-scene[data-tally=both]'));
 let take=studioPosts.filter(p=>p.path==='/api/studio/take').at(-1);assert.deepEqual(take.body,{scene_id:second,transition:{kind:'cut',duration_ms:0}});
 assert.equal(await sceneButton(2).getAttribute('data-tally'),'both','on Program and still in Preview (nothing was on air to swap back)');assert.match(await program.innerText(),new RegExp(studio.scenes[1].name));

 // Fade with a duration.
 await sceneButton(1).click();await manager.getByLabel('Fade duration (ms)').fill('1200');
 await manager.getByRole('button',{name:'Fade',exact:true}).click();
 await page.waitForFunction(()=>document.querySelectorAll('.sw-scene[data-tally=program]').length===1&&document.querySelector('.sw-scene[data-tally=program] .sw-scene-name')?.textContent.includes('1'));
 take=studioPosts.filter(p=>p.path==='/api/studio/take').at(-1);assert.deepEqual(take.body,{scene_id:studio.scenes[0].id,transition:{kind:'fade',duration_ms:1200}});
 assert.equal(await sceneButton(2).getAttribute('data-tally'),'preview','the old Program scene is back in Preview');

 // Hotkeys: 3 previews scene 3; Enter cuts it; Space fades.
 await page.locator('body').click({position:{x:5,y:5}}).catch(()=>{});await page.evaluate(()=>document.activeElement?.blur());
 await page.keyboard.press('3');await page.waitForFunction(()=>document.querySelectorAll('.sw-scene')[2]?.getAttribute('aria-pressed')==='true');
 await page.keyboard.press('Enter');for(let i=0;i<60&&studioPosts.filter(p=>p.path==='/api/studio/take').at(-1).body.scene_id!==studio.scenes[2].id;i++)await page.waitForTimeout(50);
 take=studioPosts.filter(p=>p.path==='/api/studio/take').at(-1);assert.deepEqual(take.body,{scene_id:studio.scenes[2].id,transition:{kind:'cut',duration_ms:0}});
 await manager.getByText(studio.scenes[2].name+' on Program (cut)',{exact:true}).waitFor();
 await page.keyboard.press('1');await page.keyboard.press(' ');for(let i=0;i<60&&studioPosts.filter(p=>p.path==='/api/studio/take').at(-1).body.scene_id!==studio.scenes[0].id;i++)await page.waitForTimeout(50);
 assert.equal(studioPosts.filter(p=>p.path==='/api/studio/take').at(-1).body.transition.kind,'fade');

 // Tally on the source tiles: the Program scene's sources are red.
 const programScene=studio.scenes[0],programSource=Object.values(programScene.sources)[0];
 await page.waitForFunction(id=>document.querySelector('.sw-source[data-source="'+id+'"]')?.dataset.tally?.match(/program|both/),programSource);

 // Drag a source into a slot of the Preview scene, and tap-to-place another.
 await sceneButton(4).click();
 const target=studio.scenes[3],slots=studio.layouts.find(l=>l.id===target.layout_id).slots;
 const canvas=manager.getByLabel('Scene canvas: drop sources into slots');
 // HTML5 drag events with one DataTransfer, as a browser sends them.
 const transfer=await page.evaluateHandle(()=>new DataTransfer());
 await manager.locator('.sw-source[data-source="telemetry:tclock"]').dispatchEvent('dragstart',{dataTransfer:transfer});
 const dropSlot=canvas.locator('.sw-slot[data-slot="'+slots[0].id+'"]');
 await dropSlot.dispatchEvent('dragenter',{dataTransfer:transfer});await dropSlot.dispatchEvent('dragover',{dataTransfer:transfer});await dropSlot.dispatchEvent('drop',{dataTransfer:transfer});
 await manager.locator('.sw-source[data-source="telemetry:tclock"]').dispatchEvent('dragend',{dataTransfer:transfer});
 await canvas.locator('.sw-slot[data-slot="'+slots[0].id+'"]').getByText('T-clock').waitFor();
 await manager.locator('.sw-source[data-source="diagram:flight"]').click();await manager.getByText(/Tap a slot to place/).waitFor();
 await canvas.locator('.sw-slot[data-slot="'+slots[1].id+'"]').click();await canvas.locator('.sw-slot[data-slot="'+slots[1].id+'"]').getByText('Flight').waitFor();
 assert.match(await manager.getByLabel(/Browser source \(OBS\)/).inputValue(),/source=diagram%3Aflight/);
 await manager.getByRole('button',{name:'Save scenes',exact:true}).click();await manager.getByText('Scenes saved',{exact:true}).waitFor();
 const savedScenes=studioPosts.filter(p=>p.path==='/api/studio/scenes').at(-1).body;
 const saved=savedScenes.scenes.find(s=>s.id===target.id);assert.equal(saved.sources[slots[0].id],'telemetry:tclock');assert.equal(saved.sources[slots[1].id],'diagram:flight');
 assert.equal(Object.values(saved.sources).filter(v=>v.startsWith('diagram:')).length,1,'one diagram per scene');
 if(shots){await page.waitForTimeout(1500);await page.screenshot({path:path.join(shots,'stream-manager-desktop.png'),fullPage:true});}

 // Layout editor: a preset, a pointer drag (snapped), typed geometry, save.
 await manager.getByRole('tab',{name:'Layout editor'}).click();
 const layouts=manager.getByRole('group',{name:'Layouts'});await layouts.getByRole('button',{name:/^\+ ?New layout|New layout/}).click();
 await manager.getByRole('group',{name:'Aspect presets'}).getByRole('button',{name:'Preset Picture in picture'}).click();
 const editor=manager.getByLabel(/^Layout canvas/);const pip=editor.locator('.sw-slot[data-slot="pip"]');await pip.waitFor();
 await page.evaluate(()=>document.querySelector('.sw-grid').scrollIntoView({block:'center'}));await page.waitForTimeout(200);
 const box=await editor.boundingBox(),slotBox=await pip.boundingBox();
 await page.mouse.move(slotBox.x+slotBox.width/2,slotBox.y+slotBox.height/2);await page.mouse.down();
 await page.mouse.move(slotBox.x+slotBox.width/2-box.width*0.3,slotBox.y+slotBox.height/2+box.height*0.2,{steps:8});await page.mouse.up();await page.waitForTimeout(400);
 const moved=await pip.getAttribute('aria-label');const [, mx, my]=moved.match(/at ([\d.]+)%, ([\d.]+)%/);assert(Math.abs(+mx-38)<=3&&Math.abs(+my-24)<=3,'dragged slot moved: '+moved);assert.equal((+mx/2.5)%1,0,'snapped to the grid');
 await manager.getByLabel('Width').fill('40');await manager.getByLabel('Width').press('Tab');await page.waitForFunction(()=>/40 by/.test(document.querySelector('.sw-slot[data-slot=pip]').getAttribute('aria-label')));
 await manager.getByLabel('Layout name').fill('Big PiP');
 await manager.getByRole('button',{name:'Save layouts',exact:true}).click();await manager.getByText('Layouts saved',{exact:true}).waitFor();
 const savedLayout=studioPosts.filter(p=>p.path==='/api/studio/layouts').at(-1).body.layouts.find(l=>l.name==='Big PiP');
 assert.equal(savedLayout.slots.find(s=>s.id==='pip').width,40);assert.equal(savedLayout.slots.length,2);
 // A stale save is refused, not overwritten.
 studio.revision+=5;await manager.getByLabel('Layout name').fill('Bigger PiP');await manager.getByRole('button',{name:'Save layouts',exact:true}).click();await manager.getByText(/reload before saving/).waitFor();

 // Tablet and phone: compact, no horizontal page scroll.
 await manager.getByRole('tab',{name:'Scene & sources'}).click();
 for(const [width,height,name] of [[834,1112,'tablet'],[390,844,'phone']]){
  await page.setViewportSize({width,height});await page.waitForTimeout(400);
  assert(await page.evaluate(()=>document.querySelector('.gs26-switcher').scrollWidth<=document.querySelector('.gs26-switcher').clientWidth+1),'stream manager fits '+width);
  const cut=await manager.getByRole('button',{name:'Cut',exact:true}).boundingBox();assert(cut.width>=44&&cut.height>=44,'touch-sized Cut at '+width);
  if(shots)await page.screenshot({path:path.join(shots,'stream-manager-'+name+'.png'),fullPage:name==='tablet'});
 }
 // Tap-to-place works on the phone too.
 await sceneButton(5).click();await manager.locator('.sw-source[data-source="graphic:title"]').click();
 const phoneScene=studio.scenes[4],phoneSlot=studio.layouts.find(l=>l.id===phoneScene.layout_id).slots[0].id;
 await manager.getByLabel('Scene canvas: drop sources into slots').locator('.sw-slot[data-slot="'+phoneSlot+'"]').click();
 await manager.getByLabel('Scene canvas: drop sources into slots').locator('.sw-slot[data-slot="'+phoneSlot+'"]').getByText('Mission title card').waitFor();
 assert.deepEqual(errors,[]);
 console.log('PASS real Wasm stream manager: scene->Preview, Cut/Fade+duration->Program, hotkeys, tally, drag/tap source into slot, layout editor preset/drag/typed/save/conflict, tablet and phone');
 await context.close();
}finally{await browser.close();await new Promise(r=>server.close(r));}`;
assert.equal(fs.readFileSync(sourcePath,'utf8'),original,'shared fixture remains untouched');
try { await import('data:text/javascript;base64,'+Buffer.from(fixture).toString('base64')); } catch(error) { console.error(error.name+': '+error.message);process.exitCode=1; }
