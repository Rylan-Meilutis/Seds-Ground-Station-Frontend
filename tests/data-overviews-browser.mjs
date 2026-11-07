// Actual Wasm against isolated synthetic services; no station commands.
import fs from 'node:fs';
import path from 'node:path';
import {pathToFileURL} from 'node:url';
const source=fs.readFileSync('tests/navigation-checklists.mjs','utf8');
const boundary=source.indexOf('try {\n const context=');
if(boundary<0)throw Error('Navigation fixture boundary changed');
let fixture=source.slice(0,boundary).replace("process.env.PLAYWRIGHT_MODULE || 'playwright'",JSON.stringify(process.env.PLAYWRIGHT_MODULE?pathToFileURL(path.resolve(process.env.PLAYWRIGHT_MODULE)).href:import.meta.resolve('playwright')));
fixture+=`try {
 dashboardLayout.data_tab.tabs = [
  {id:'COMBUSTION_CHAMBER_PRESSURE',label:'Chamber',channels:['Chamber pressure'],chart:{enabled:true}},
  {id:'FUEL_TANK_PRESSURE',label:'Ox tank',channels:['Tank pressure'],chart:{enabled:true}},
  {id:'ASCENT_STATE',label:'Flight',channels:['Altitude','Speed'],chart:{enabled:true}},
  {id:'BATTERY_VOLTAGE',label:'Battery',channels:['Voltage'],chart:{enabled:true}},
  {id:'sensor-views',label:'Multiple views',channels:[],subtabs:[
   {id:'chamber',label:'Chamber view',data_type:'COMBUSTION_CHAMBER_PRESSURE',channels:['Selected chamber'],chart:{enabled:true}},
   {id:'tank',label:'Tank view',data_type:'FUEL_TANK_PRESSURE',channels:['Selected tank'],chart:{enabled:true}}
  ]}
 ];
 const context=await browser.newContext({viewport:{width:1440,height:1000}});
 await context.addInitScript(({session,origin})=>{if(!URL.parse)URL.parse=(a,b)=>{try{return new URL(a,b)}catch{return null}};localStorage.setItem('navigation-style','sidebar');localStorage.setItem('auth_session_v1',JSON.stringify({entries:[{host_scope:origin,updated_at_ms:Date.now(),session:{token:'ui-token',session,remember_me:true}}]}));},{session,origin});
 let socket;const commands=[],errors=[];
 await context.routeWebSocket(url=>url.pathname==='/_dioxus',()=>{});
 await context.routeWebSocket(url=>url.pathname==='/ws',s=>{socket=s;s.onMessage(data=>commands.push(data));});
 const page=await context.newPage();page.on('pageerror',e=>errors.push(e.message));await page.goto(origin+'/dashboard');
 await page.getByRole('link',{name:'Telemetry',exact:true}).click();
 const categories=page.getByRole('group',{name:'Data categories',exact:true});
 assert.deepEqual(await categories.getByRole('button').allTextContents(),['Fill overview','Avionics overview','Power overview','Chamber','Ox tank','Flight','Battery','Multiple views','Igniter current sense'],'Combined overviews precede individual sensor tabs');
 const send=()=>{const now=Date.now();socket.send(JSON.stringify({ty:'TelemetryBatch',data:[['COMBUSTION_CHAMBER_PRESSURE','DAQ',[123.25]],['FUEL_TANK_PRESSURE','VB',[456.75]],['ASCENT_STATE','FC',[789.5,12.5]],['BATTERY_VOLTAGE','GB',[14.25]]].map(([data_type,sender_id,values])=>({data_type,sender_id,values,timestamp_ms:now,received_timestamp_ms:now}))}));};
 await page.getByRole('button',{name:'Fill overview',exact:true}).click();send();
 await page.getByText('Chamber · Chamber pressure',{exact:true}).first().waitFor();await page.getByText('Ox tank · Tank pressure',{exact:true}).first().waitFor();
 await page.getByText('123.2500',{exact:true}).first().waitFor();await page.getByText('456.7500',{exact:true}).first().waitFor();
 await page.getByRole('button',{name:'Avionics overview',exact:true}).click();send();await page.getByText('Flight · Altitude',{exact:true}).first().waitFor();await page.getByText('Flight · Speed',{exact:true}).first().waitFor();
 await page.getByRole('button',{name:'Power overview',exact:true}).click();send();await page.getByText('Battery · Voltage',{exact:true}).first().waitFor();
 assert.equal(await page.locator('.gs26-data-subtab-shell').count(),0);
 assert.equal(await page.getByRole('group',{name:'Data views',exact:true}).count(),0);
 await page.getByRole('button',{name:'Multiple views',exact:true}).click();send();
 const views=page.getByRole('group',{name:'Data views',exact:true});
 assert.deepEqual(await views.getByRole('button').allTextContents(),['Chamber view','Tank view']);
 await page.getByText('Selected chamber',{exact:true}).first().waitFor();
 await views.getByRole('button',{name:'Tank view',exact:true}).click();send();
 await page.getByText('Selected tank',{exact:true}).first().waitFor();
 assert.equal(await views.getByRole('button',{name:'Tank view',exact:true}).getAttribute('aria-pressed'),'true');
 await page.getByRole('button',{name:'Power overview',exact:true}).click();send();
 await page.getByText('Battery · Voltage',{exact:true}).first().waitFor();
 assert.equal(await page.getByRole('group',{name:'Data views',exact:true}).count(),0);
 await page.getByRole('button',{name:'Multiple views',exact:true}).click();
 assert.equal(await page.getByRole('group',{name:'Data views',exact:true}).getByRole('button',{name:'Tank view',exact:true}).getAttribute('aria-pressed'),'true');
 await page.getByRole('button',{name:'Chamber',exact:true}).click();send();
 await page.getByText('Chamber pressure',{exact:true}).first().waitFor();
 assert.equal(await page.locator('.gs26-data-subtab-shell').count(),0,'Zero-subtab source remains directly usable');
 await page.getByRole('button',{name:'Multiple views',exact:true}).click();
 await page.setViewportSize({width:390,height:844});
 await page.locator('.gs26-data-subtab-toggle').click();
 await page.getByRole('group',{name:'Data views',exact:true}).getByRole('button',{name:'Tank view',exact:true}).click();send();
 await page.getByText('Selected tank',{exact:true}).first().waitFor();
 assert.equal(await page.locator('.gs26-data-subtab-nav button[aria-pressed="true"]').textContent(),'Tank view','Collapsed mobile menu retains the selected view');
 assert.equal(await page.locator('.gs26-data-subtab-nav').isVisible(),false,'Mobile selection closes its view menu');
 assert.deepEqual(commands,[]);assert.deepEqual(errors,[]);await context.close();console.log('PASS actual Wasm combined sensor views: fill/avionics/power, independent source values, no hardware commands');
}finally{await browser.close();await new Promise(r=>server.close(r));}`;
try{await import('data:text/javascript;base64,'+Buffer.from(fixture).toString('base64'));}catch(error){console.error(error.name+': '+error.message);process.exitCode=1;}
