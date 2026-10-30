// Geometry regression for production CSS, without a backend or application build.
// This is a layout fixture, not a native/iOS or compiled-Wasm integration test.
import fs from 'node:fs';
import assert from 'node:assert/strict';
const {chromium}=await import(process.env.PLAYWRIGHT_MODULE||'playwright');
const app=fs.readFileSync('src/app.rs','utf8');
const shell=fs.readFileSync('src/app_shell.rs','utf8');
const css=fs.readFileSync('src/connection_failure.css','utf8');
const global=shell.match(/const GLOBAL_CSS: &str = r#"([\s\S]*?)"#;/)[1];
const helper=name=>shell.match(new RegExp(`fn ${name}[^]*?format!\\(\\s*"([^"]+)"`))[1];
const pageStyle=helper('shell_page_style').replaceAll('{}','#0b1220');
const cardStyle=helper('shell_card_style').replace('{width}','100%').replaceAll('{}','#334155');
const component=app.slice(app.indexOf('fn ConnectionFailedCard('),app.indexOf('\n#[component]',app.indexOf('fn ConnectionFailedCard(')));
assert.match(component,/details[\s\S]*pre \{ "\{message\}" \}/,'complete diagnostics remain expandable');
assert.match(component,/include_str!\("connection_failure.css"\)/);
for(const action of ['Back to Connect','Explore offline','Retry'])assert(component.includes(action));
const browser=await chromium.launch({headless:true,executablePath:process.env.CHROME_BINARY});
try {
 const page=await browser.newPage();
 for(const [width,height] of [[320,568],[390,844],[844,390],[1280,800]]) {
  await page.setViewportSize({width,height});
  await page.setContent(`<style>${global}${css}</style><main id="main"><div class="gs26-connection-failure" style="${pageStyle}"><div class="gs26-connection-failure-card" style="${cardStyle};padding:clamp(18px,4vw,24px)"><h1 style="margin:0 0 10px;font-size:22px">Failed to Connect</h1><p style="margin:0 0 12px">The Ground Station could not be reached. Check the backend address and your network connection, then retry.</p><details class="gs26-connection-failure-details"><summary>Connection details</summary><pre></pre></details><div style="display:flex;gap:12px;justify-content:flex-end;flex-wrap:wrap">${['Back to Connect','Explore offline','Retry'].map(s=>`<button style="min-height:44px;padding:10px 14px">${s}</button>`).join('')}</div></div></div></main>`);
  const diagnostics='TLS connection failed: '+('unbroken-diagnostic-'.repeat(600));
  await page.locator('pre').evaluate((e,t)=>e.textContent=t,diagnostics);
  const card=page.locator('.gs26-connection-failure-card'),bounds=await card.boundingBox();
  assert(bounds.width>=Math.min(288,width-32),'card keeps readable viewport width');
  assert(bounds.x>=0&&bounds.x+bounds.width<=width&&bounds.y>=0&&bounds.y+bounds.height<=height,'card fits '+width);
  assert(bounds.height<Math.min(height-16,430),'collapsed diagnostic stays compact '+width);
  assert(await page.evaluate(()=>document.documentElement.scrollWidth<=innerWidth));
  await page.getByText('Connection details',{exact:true}).click();
  const pre=page.locator('pre');assert.equal(await pre.innerText(),diagnostics);
  const dimensions=await pre.evaluate(e=>({height:e.clientHeight,scroll:e.scrollHeight,width:e.clientWidth,scrollWidth:e.scrollWidth}));
  assert(dimensions.height<=192&&dimensions.scroll>dimensions.height,'large diagnostic scrolls in bounded area');
  assert(dimensions.scrollWidth<=dimensions.width+1,'long diagnostic wraps without horizontal overflow');
  await page.getByText('Connection details',{exact:true}).click();
  for(const action of ['Back to Connect','Explore offline','Retry'])assert(await page.getByRole('button',{name:action,exact:true}).isVisible());
  console.log(JSON.stringify({width,height,card:bounds,diagnostic:dimensions}));
 }
 console.log('PASS production connection-error CSS: four viewport sizes, compact collapsed state, retained bounded diagnostics and actions');
} finally {await browser.close();}
