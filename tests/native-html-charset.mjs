// Exercise the actual native HTML prefix through a browser's HTML decoder.
// Build/run requires no backend, hardware, telemetry or existing browser session.
import assert from 'node:assert/strict';
import fs from 'node:fs';
import http from 'node:http';
import path from 'node:path';
import os from 'node:os';
const {chromium}=await import(process.env.PLAYWRIGHT_MODULE||'playwright');
const sourcePath=path.resolve(process.env.GS_NATIVE_HEAD_SOURCE||'src/main.rs');
const source=fs.readFileSync(sourcePath,'utf8');
const section=source.split('#[cfg(target_os = "linux")]\nfn startup_custom_head() -> String {')[1]?.split('#[cfg(any(target_os = "linux", test))]')[0];
assert(section,'Linux custom-head function exists');
const prefix=section.match(/let mut head = String::from\(\s*r##"([\s\S]*?)"##,/)?.[1];
assert(prefix,'native initial HTML prefix exists');
const control=prefix.replace(/<meta\s+charset=["']utf-8["']\s*\/?\s*>/i,'');
assert.notEqual(control,prefix,'native document must declare UTF-8 before inline scripts');
const output=path.resolve(process.env.GS_TEST_OUTPUT||path.join(os.tmpdir(),'gs-native-html-charset'));
fs.mkdirSync(output,{recursive:true});
const prefixes={control,fixed:prefix};
const server=http.createServer((req,res)=>{
 const page=new URL(req.url,'http://127.0.0.1').pathname.slice(1);
 if(!(page in prefixes)){res.writeHead(404);res.end();return;}
 const html=`<!doctype html><html><head>${prefixes[page]}</head><body style="margin:20px;font:32px system-ui"><div id="glyphs"></div><script>document.getElementById('glyphs').textContent='− 🚀';</script></body></html>`;
 const body=Buffer.from(html,'utf8');res.writeHead(200,{'Content-Type':'text/html','Content-Length':body.length});res.end(body);
});
await new Promise(resolve=>server.listen(0,'127.0.0.1',resolve));
const origin=`http://127.0.0.1:${server.address().port}`;let browser;
const result={source:sourcePath,fixture:'Fresh browser; UTF-8 bytes with no HTTP charset',pages:{}};
try{
 browser=await chromium.launch({headless:true,...(process.env.CHROME_BINARY?{executablePath:process.env.CHROME_BINARY}:{}),args:['--disable-background-networking','--disable-audio-output','--host-resolver-rules=MAP * ~NOTFOUND, EXCLUDE 127.0.0.1']});
 const context=await browser.newContext({viewport:{width:430,height:180},locale:'en-US'});
 await context.route('**/*',route=>new URL(route.request().url()).hostname==='127.0.0.1'?route.continue():route.abort());
 for(const which of ['control','fixed']){
  const page=await context.newPage();const response=await page.goto(`${origin}/${which}`);
  assert.equal(response.headers()['content-type'],'text/html');await page.locator('#glyphs').waitFor();
  result.pages[which]=await page.evaluate(()=>({charset:document.characterSet,text:document.getElementById('glyphs').textContent,codepoints:Array.from(document.getElementById('glyphs').textContent,c=>c.codePointAt(0))}));
  await page.screenshot({path:path.join(output,`${which}-head.png`)});await page.close();
 }
 assert.notEqual(result.pages.control.text,'− 🚀','negative control reproduces undecorated HTML misdecoding');
 assert.equal(result.pages.fixed.charset,'UTF-8');assert.equal(result.pages.fixed.text,'− 🚀');
 assert.equal(result.pages.fixed.codepoints[0],0x2212);assert.equal(result.pages.fixed.codepoints.at(-1),0x1f680);result.passed=true;
}catch(error){result.passed=false;result.error=String(error);throw error;}
finally{
 if(browser)await browser.close();await new Promise(resolve=>server.close(resolve));fs.writeFileSync(path.join(output,'encoding-proof.json'),JSON.stringify(result,null,2));console.log(JSON.stringify(result,null,2));
}
