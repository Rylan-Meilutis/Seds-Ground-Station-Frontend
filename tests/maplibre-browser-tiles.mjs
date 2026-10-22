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
} finally {await browser.close();}
