// Crew voice talk floor on the voice page, against a fake station socket
// (GroundStation docs/backend/crew-voice.md "Talk floor"). No station needed.
import assert from 'node:assert/strict';
import fs from 'node:fs';
import http from 'node:http';
import path from 'node:path';
const {chromium} = await import(process.env.PLAYWRIGHT_MODULE || 'playwright');
const source = process.env.GS_MEDIA_SOURCE || path.resolve('station_ui');
const server = http.createServer((req, res) => {
  const url = new URL(req.url, 'http://localhost');
  const files = {'/radio':'pages/voice.html','/assets/voice.js':'scripts/voice.js','/assets/media-login.js':'scripts/media-login.js','/assets/voice-worklet.js':'scripts/voice-worklet.js','/assets/stay-awake.js':'scripts/stay-awake.js'};
  if (files[url.pathname]) { res.setHeader('Content-Type',url.pathname.endsWith('.js')?'text/javascript':'text/html'); res.end(fs.readFileSync(path.join(source,files[url.pathname]))); return; }
  if (url.pathname === '/api/voice/status') { res.setHeader('Content-Type','application/json'); res.end(JSON.stringify({can_transmit:true,max_participants:32,max_talkers:4,radio:{configured:false,available:false}})); return; }
  res.statusCode=404;res.end('Not found');
});
await new Promise(resolve=>server.listen(0,'127.0.0.1',resolve));
const origin=`http://127.0.0.1:${server.address().port}`;
const browser=await chromium.launch({headless:true,executablePath:process.env.CHROME_BINARY,args:['--disable-audio-output','--use-fake-device-for-media-stream','--use-fake-ui-for-media-stream','--autoplay-policy=no-user-gesture-required']});
const roster=(me,others)=>({type:'roster',peers:[{id:1,name:'ada',transmitting:me==='on_air',waiting:me==='waiting',device:'web'},...others]});
const talkers=[2,3,4,5].map(id=>({id,name:`crew${id}`,transmitting:true,waiting:false,device:id===5?'watch':'web'}));
try {
  const context=await browser.newContext({permissions:['microphone']});
  const page=await context.newPage();const errors=[];page.on('pageerror',e=>errors.push(e.message));
  await page.route('**/assets/media-login.js',route=>route.fulfill({contentType:'text/javascript',body:'export async function signIn(){return {token:"t"}}'}));
  await page.addInitScript(()=>{
    window.__controls=[];window.__audio=0;
    window.WebSocket=class {
      static OPEN=1;readyState=1;bufferedAmount=0;
      constructor(){window.__socket=this;setTimeout(()=>this.onopen?.(),0);}
      send(data){
        if(typeof data!=='string'){window.__audio++;return;}
        const value=JSON.parse(data);window.__controls.push(value);
        if(value.type==='auth')setTimeout(()=>this.onmessage?.({data:JSON.stringify({type:'joined',id:1,max_participants:32,max_talkers:4})}),0);
      }
      close(){this.readyState=3;}
    };
    window.__station=value=>window.__socket.onmessage?.({data:JSON.stringify(value)});
  });
  await page.goto(`${origin}/radio`);
  await page.fill('#username','ada');await page.fill('#password','x');await page.click('#signin');
  await page.click('#join');await page.getByText('Connected to voice chat.',{exact:true}).waitFor();
  const station=value=>page.evaluate(v=>window.__station(v),value);
  const mic=()=>page.locator('#mic-state').textContent();

  // Five on the roster, four on air: the count shows the station's cap.
  await station(roster('idle',talkers));
  assert.equal(await page.locator('#count').textContent(),'5 / 32');
  assert.match(await page.locator('#peers').textContent(),/crew5 \(watch\) · On air/);

  // Open mic while the floor is full: the request waits and sends no audio.
  await page.selectOption('#mode','open');
  await page.waitForFunction(()=>window.__controls.some(c=>c.type==='transmitting'&&c.enabled));
  await station({type:'floor',state:'waiting',reason:null,talkers:4,max_talkers:4});
  await station(roster('waiting',talkers));
  assert.equal(await mic(),'Waiting for a free channel · 4 of 4 talking');
  assert.equal(await page.locator('#mic-state').getAttribute('data-floor'),'waiting');
  assert.match(await page.locator('#peers').textContent(),/ada \(you\) · Waiting to talk/);
  const waiting=await page.evaluate(()=>window.__audio);await page.waitForTimeout(300);
  assert.equal(await page.evaluate(()=>window.__audio),waiting,'no audio is sent while waiting');

  // A slot frees: on air, the label is just "Transmitting", audio flows.
  await station({type:'floor',state:'on_air',reason:null,talkers:4,max_talkers:4});
  assert.equal(await mic(),'Transmitting');
  await page.waitForFunction(n=>window.__audio>n+3,waiting);

  // An administrator override puts us back in line, with the reason.
  await station({type:'floor',state:'waiting',reason:'priority',talkers:4,max_talkers:4});
  assert.equal(await mic(),'Waiting for a free channel · 4 of 4 talking');
  await page.getByText(/An administrator took a talk slot/).waitFor();
  const bumped=await page.evaluate(()=>window.__audio);await page.waitForTimeout(300);
  assert.equal(await page.evaluate(()=>window.__audio),bumped);

  // The once-a-second roster repairs a lost floor message.
  await station(roster('on_air',talkers.slice(0,3)));
  assert.equal(await mic(),'Transmitting');

  // A station without a floor (no max_talkers in joined) puts requests on air at once.
  await page.click('#leave');
  await page.evaluate(()=>{const Fake=window.WebSocket;window.WebSocket=class extends Fake{send(data){if(typeof data==='string'&&JSON.parse(data).type==='auth'){setTimeout(()=>this.onmessage?.({data:JSON.stringify({type:'joined',id:1})}),0);return;}super.send(data);}};});
  await page.click('#join');await page.getByText('Connected to voice chat.',{exact:true}).waitFor();
  await station({type:'roster',peers:[{id:1,name:'ada',transmitting:false,device:'web'}]});
  assert.equal(await page.locator('#count').textContent(),'1');
  await page.selectOption('#mode','open');
  await page.waitForFunction(()=>document.querySelector('#mic-state').textContent==='Transmitting');
  assert.equal(errors.length,0,errors.join('\n'));
  console.log('PASS: talk floor waiting/on-air/override/roster repair, "Transmitting" label, station-reported participant cap');
} finally {await browser.close();server.close();}
