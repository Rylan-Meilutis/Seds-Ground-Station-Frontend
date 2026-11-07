// Actual production Wasm against a synthetic station using the production
// default of actions disabled. No hardware or command execution.
import fs from 'node:fs';
const original=fs.readFileSync('tests/calibration-page-lifetime.mjs','utf8');
const boundary=original.indexOf('fixture+=`try {');
if(boundary<0)throw Error('Lifetime fixture prefix changed');
let script=original.slice(0,boundary);
script+=String.raw`fixture=fixture.replace("const dashboardLayout=example('layout.full');","const dashboardLayout=example('layout.full');dashboardLayout.actions_tab.disable_actions_by_default=true;");
fixture=fixture.replace('calibration.kg50_input=sent.input;','if(!refuseNextInput)calibration.kg50_input=sent.input;refuseNextInput=false;');
fixture=fixture.replace('let heldGetStarted=null;',"let heldGetStarted=null;let refuseNextInput=false;let board={selected:'amp2',board_input:'amp2',board_age_ms:10,state:'synced'};");
fixture=fixture.replace("  if(pathname==='/api/calibration'||pathname==='/api/calibration_config'||pathname==='/api/calibration/kg50_input'){","  if(pathname==='/api/calibration/kg50_input'&&req.method==='GET'){res.setHeader('Content-Type','application/json');res.end(JSON.stringify(board));return;}\n  if(pathname==='/api/calibration'||pathname==='/api/calibration_config'||pathname==='/api/calibration/kg50_input'){");
if(!fixture.includes('let board='))throw Error('Input fixture changed');
fixture+=\`try {
 holdInputPosts=false;session.station_role='primary';
 const context=await browser.newContext({viewport:{width:1280,height:900}});
 await context.addInitScript(({session,origin})=>{if(!URL.parse)URL.parse=(a,b)=>{try{return new URL(a,b)}catch{return null}};localStorage.setItem('navigation-style','sidebar');localStorage.setItem('auth_session_v1',JSON.stringify({entries:[{host_scope:origin,updated_at_ms:Date.now(),session:{token:'ui-token',session,remember_me:true}}]}));},{session,origin});
 await context.routeWebSocket(url=>url.pathname==='/_dioxus',()=>{});await context.routeWebSocket(url=>url.pathname==='/ws',()=>{});
 const page=await context.newPage();await page.goto(origin+'/dashboard');await page.getByRole('navigation',{name:'Mission navigation'}).waitFor();
 const station=page.locator('.gs26-ground-station-settings');const input=station.getByLabel('50 kg input',{exact:true});
 await page.getByRole('link',{name:'Settings',exact:true}).click();await page.getByRole('navigation',{name:'Settings scope'}).getByRole('button',{name:'Ground Station',exact:true}).click();await station.getByRole('navigation',{name:'Ground Station settings sections'}).getByRole('button',{name:'Loadcells',exact:true}).click();await input.waitFor();
 // Disabled for a stated reason, and the way out is reachable from Settings.
 await station.getByText('Actions are disabled, so the DAQ input cannot be changed.',{exact:true}).waitFor();
 await station.getByText('DAQ confirmed: using AMP2 / P8.',{exact:true}).waitFor();
 assert.equal(await input.isDisabled(),true);assert.deepEqual(inputPosts,[]);
 await station.getByRole('button',{name:'Enable actions',exact:true}).click();
 await page.waitForFunction(()=>!document.getElementById('kg50-input').disabled);
 assert.equal(await station.getByText('Actions are disabled, so the DAQ input cannot be changed.',{exact:true}).count(),0);
 assert.equal(await station.getByRole('button',{name:'Enable actions',exact:true}).count(),0);
 // Saved is not applied: the DAQ's own report decides what is shown as confirmed.
 board={selected:'p6_differential',board_input:'amp2',board_age_ms:10,state:'syncing'};
 await input.selectOption('p6_differential');assert.equal(calibration.kg50_input,'p6_differential');
 await station.getByText('DAQ is still on AMP2 / P8. Sending P6 differential…',{exact:true}).waitFor();
 assert.equal(await station.getByText('DAQ confirmed: using P6 differential.',{exact:true}).count(),0);
 board={selected:'p6_differential',board_input:'p6_differential',board_age_ms:10,state:'synced'};
 await station.getByText('DAQ confirmed: using P6 differential.',{exact:true}).waitFor();
 board={selected:'p6_differential',board_input:'p6_differential',board_age_ms:9000,state:'no_readback'};
 await station.getByText('DAQ is not reporting. Last reported P6 differential 9 s ago; its current input is unknown.',{exact:true}).waitFor();
 // A failed request leaves the selector on what the station reports, not on the choice.
 board={selected:'p6_differential',board_input:'p6_differential',board_age_ms:10,state:'synced'};
 failNextInput=true;refuseNextInput=true;await input.selectOption('amp2');
 await station.getByRole('button',{name:'Confirm selected input again',exact:true}).waitFor();
 assert.equal(calibration.kg50_input,'p6_differential');assert.equal(await input.inputValue(),'p6_differential');
 await context.close();console.log('PASS actual Wasm: production actions-disabled default explains itself and is recoverable from Settings; UI shows the DAQ report rather than the saved selection');
}catch(error){console.error('KG50_SELECTOR_FAILURE '+String(error));throw error;}finally{for(const release of postReleases.splice(0))release();await browser.close();for(const socket of serverSockets)socket.destroy();await new Promise(resolve=>server.close(resolve));}\`;
try{await import('data:text/javascript;base64,'+Buffer.from(fixture).toString('base64'));}catch(error){console.error(error.name+': '+error.message);process.exitCode=1;}
`;
script=script.replaceAll('\\`','`');
try{await import('data:text/javascript;base64,'+Buffer.from(script).toString('base64'));}catch(error){console.error(error.name+': '+error.message);process.exitCode=1;}
