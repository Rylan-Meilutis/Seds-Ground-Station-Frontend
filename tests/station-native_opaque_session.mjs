// A native webview may have an opaque parent origin. Real media-page code,
// synthetic loopback APIs only: no camera, microphone, boards or live backend.
import fs from 'node:fs';
import path from 'node:path';
import http from 'node:http';
import assert from 'node:assert/strict';
const {chromium}=await import(process.env.PLAYWRIGHT_MODULE||'playwright');
const source=path.resolve(process.env.GS_MEDIA_SOURCE||'station_ui');
const requests=[];
const assets={'/media':'pages/media.html','/assets/media-login.js':'scripts/media-login.js','/assets/stay-awake.js':'scripts/stay-awake.js'};
const server=http.createServer(async(req,res)=>{
 const url=new URL(req.url,'http://localhost');requests.push({path:url.pathname,method:req.method,authorized:req.headers.authorization==='Bearer opaque-fixture-token'});
 if(assets[url.pathname]){res.setHeader('Content-Type',url.pathname.endsWith('.js')?'text/javascript':'text/html');res.end(fs.readFileSync(path.join(source,assets[url.pathname])));return;}
 if(url.pathname.startsWith('/api/')){
  assert.equal(req.method,'GET','Session handoff cannot issue control or media-publish requests');
  res.setHeader('Content-Type','application/json');
  const result=url.pathname==='/api/live_streams'?{can_manage_stream:true,can_publish_camera:false}:url.pathname==='/api/video/recordings'?{recordings:[],total:0,offset:0}:[];
  res.end(JSON.stringify(result));return;
 }
 res.statusCode=404;res.end();
});
await new Promise(r=>server.listen(0,'127.0.0.1',r));
const loopback=`http://127.0.0.1:${server.address().port}`;
// A synthetic public HTTPS origin avoids Chromium's data-URL-to-private-IP
// navigation block. Every request is fulfilled from our read-only mock server;
// normal origin checks remain enabled and no TLS/certificate bypass is used.
const origin='https://station.example.test';
const browser=await chromium.launch({headless:true,executablePath:process.env.CHROME_BINARY||(process.platform==='darwin'&&fs.existsSync('/Applications/Google Chrome.app/Contents/MacOS/Google Chrome')?'/Applications/Google Chrome.app/Contents/MacOS/Google Chrome':undefined),args:['--use-fake-device-for-media-stream']});
try{
 const page=await browser.newPage();
 await page.route(origin+'/**',async route=>{const url=new URL(route.request().url());const response=await route.fetch({url:loopback+url.pathname+url.search});await route.fulfill({response});});
 const errors=[];page.on('pageerror',e=>errors.push(e.message));
 const html=`<!doctype html><iframe id="media" src="${origin}/media?embedded=1"></iframe><script>window.captureMessages=[];addEventListener('message',event=>{if(event.source===document.getElementById('media').contentWindow&&event.data?.type==='gs26-capture-active')captureMessages.push(event.data);});setInterval(()=>{document.getElementById('media').contentWindow.postMessage({type:'gs26-session',token:'opaque-fixture-token',visible:true},${JSON.stringify(origin)});},100);<\/script>`;
 await page.goto('data:text/html;charset=utf-8,'+encodeURIComponent(html));
 assert.equal(await page.evaluate(()=>location.origin),'null','Fixture must retain actual opaque parent origin');
 await page.frameLocator('#media').getByText('Connected with your dashboard session.',{exact:true}).waitFor({timeout:5000}).catch(error=>{console.error(JSON.stringify({errors,requests,frames:page.frames().map(f=>f.url())}));throw error;});
 for(let i=0;i<100&&!requests.some(r=>r.path==='/api/video/streams'&&r.authorized);i++)await new Promise(r=>setTimeout(r,10));
 assert(requests.some(r=>r.path==='/api/video/streams'&&r.authorized),'Opaque parent session must reach the real media page API');
 assert.deepEqual(errors,[],'Opaque-origin capture-status reply must not throw before session setup');
 const messages=await page.evaluate(()=>window.captureMessages);
 assert(messages.some(m=>m.active===false),'Parent receives non-secret capture lifetime status');
 assert(messages.every(m=>Object.keys(m).sort().join(',')==='active,type'),'Capture reply never contains credentials');
 console.log(JSON.stringify({state:'PASS',parent_origin:'null',child_origin:origin,authenticated_read_only_requests:requests.filter(r=>r.authorized).length,capture_status_received:true,credentials_in_reply:false}));
 await page.close();
}finally{await browser.close();await new Promise(r=>server.close(r));}
