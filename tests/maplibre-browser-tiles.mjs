import fs from 'node:fs';
import assert from 'node:assert/strict';
const {chromium}=await import(process.env.PLAYWRIGHT_MODULE || 'playwright');
const ui=process.env.FRONTEND_ROOT || new URL('..',import.meta.url).pathname;
const browser=await chromium.launch({headless:true,executablePath:process.env.CHROME_BINARY || (process.platform==='darwin'?'/Applications/Google Chrome.app/Contents/MacOS/Google Chrome':undefined),args:['--enable-unsafe-swiftshader','--use-angle=swiftshader']});
try {
 for(const validated of [false,true]) {
  const page=await browser.newPage();let tiles=0;
  await page.route('http://map.test/**',async route=>{
   if(route.request().url().includes('/tiles/')) {tiles++;await route.fulfill({contentType:'text/html',body:'<!doctype html><title>Fallback page</title>'});}
   else await route.fulfill({contentType:'text/html',body:'<div id="map" style="width:800px;height:600px"></div>'});
  });
  await page.goto('http://map.test/');
  await page.addScriptTag({path:ui+'/static/vendor/maplibre-gl/maplibre-gl.js'});
  await page.addScriptTag({path:ui+'/static/ground_map.js'});
  await page.evaluate(validated=>{
   window.errors=[];currentTilesUrl='http://map.test/tiles/{z}/{x}/{y}.jpg';
   if(validated) {if(shouldUseNativeTileTemplate(currentTilesUrl))throw Error('browser bypasses validation');ensureMapProtocolOnce();}
   window.map=new maplibregl.Map({container:'map',center:[-80,43],zoom:4,style:{version:8,sources:{tiles:{type:'raster',tiles:[validated?tileProtocolTemplate():currentTilesUrl],tileSize:256}},layers:[{id:'tiles',type:'raster',source:'tiles'}]}});
   map.on('error',event=>errors.push(event.error.message));
  },validated);
  await page.waitForTimeout(2500);
  const errors=await page.evaluate(()=>window.errors);assert(tiles>0,'real raster requests');
  if(validated) assert.deepEqual(errors,[]);else assert(errors.some(e=>/could not (?:load image|be decoded)/i.test(e)),JSON.stringify(errors));
  console.log(JSON.stringify({validated,tiles,errors:errors.length}));await page.close();
 }
 // An incomplete offline download must not strand the viewport or cache a
 // transparent image forever after the server starts serving real tiles.
 {
  const page=await browser.newPage();let available=false, requests=0;
  await page.route('http://recovery.test/**',async route=>{
   if(route.request().url().includes('/tiles/')) {
    requests++;
    if(!available)await route.fulfill({status:204});
    else await route.fulfill({contentType:'image/png',body:Buffer.from(await page.evaluate(()=>{const c=document.createElement('canvas');c.width=c.height=256;const ctx=c.getContext('2d');ctx.fillStyle='rgb(23,118,73)';ctx.fillRect(0,0,256,256);return c.toDataURL().split(',')[1];}),'base64')});
   } else await route.fulfill({contentType:'text/html',body:'<div id="ground-map" style="width:800px;height:600px"></div>'});
  });
  await page.goto('http://recovery.test/');
  await page.addScriptTag({path:ui+'/static/vendor/maplibre-gl/maplibre-gl.js'});
  await page.addScriptTag({path:ui+'/static/ground_map.js'});
  await page.evaluate(()=>{
   currentTilesUrl='http://recovery.test/tiles/{z}/{x}/{y}.jpg';ensureMapProtocolOnce();
   groundMap=new maplibregl.Map({container:'ground-map',center:[-80,43],zoom:5,minZoom:5,preserveDrawingBuffer:true,style:makeMapStyle(currentTilesUrl,8)});
   window.errors=[];groundMap.on('error',e=>errors.push(e.error.message));
  });
  await page.waitForTimeout(1500);
  assert(requests>0);assert.equal(await page.evaluate(()=>groundMap.getMinZoom()),5);
  assert.equal(await page.evaluate(()=>groundMap.getZoom()),5);
  const before=requests;available=true;
  await page.evaluate(()=>refreshMissingMapTiles());
  await page.waitForTimeout(1500);
  assert(requests>before,'missing tiles are fetched again after the download');
  const pixel=await page.evaluate(()=>{const c=document.createElement('canvas');c.width=c.height=1;const ctx=c.getContext('2d');const gl=groundMap.getCanvas();ctx.drawImage(gl,gl.width/2,gl.height/2,1,1,0,0,1,1);return [...ctx.getImageData(0,0,1,1).data];});
  for(let i=0;i<4;i++)assert(Math.abs(pixel[i]-[23,118,73,255][i])<3,`visible tile pixel ${pixel}`);
  assert.deepEqual(await page.evaluate(()=>errors),[]);
  console.log('PASS: missing maps preserve zoom-out and recover visible pixels after download');
  await page.close();
 }
} finally {await browser.close();}
