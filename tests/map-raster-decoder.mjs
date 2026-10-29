import fs from 'node:fs';
import assert from 'node:assert/strict';
const {chromium}=await import(process.env.PLAYWRIGHT_MODULE || 'playwright');
const browser=await chromium.launch({headless:true,executablePath:process.env.CHROME_BINARY || (process.platform==='darwin'?'/Applications/Google Chrome.app/Contents/MacOS/Google Chrome':undefined)});
try {
 const page=await browser.newPage();
 await page.addScriptTag({content:fs.readFileSync(new URL('../static/ground_map.js',import.meta.url),'utf8')});
 const image=await page.evaluate(async()=>{
  const bitmap=await createImageBitmap(new Blob([transparentTileFallbackArrayBuffer()],{type:'image/png'}));
  const canvas=document.createElement('canvas');canvas.width=canvas.height=1;const ctx=canvas.getContext('2d');ctx.drawImage(bitmap,0,0);
  return {width:bitmap.width,height:bitmap.height,alpha:ctx.getImageData(0,0,1,1).data[3]};
 });
 assert.deepEqual(image,{width:1,height:1,alpha:0});
 console.log('PASS: browser decoder accepts transparent map fallback PNG');
} finally {await browser.close();}
