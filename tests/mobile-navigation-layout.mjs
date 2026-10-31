// Production workspace CSS geometry and mobile selection script, without a backend.
import fs from 'node:fs';
import assert from 'node:assert/strict';
const {chromium}=await import(process.env.PLAYWRIGHT_MODULE||'playwright');
const shell=fs.readFileSync('src/app_shell.rs','utf8');
const global=shell.match(/const GLOBAL_CSS: &str = r#"([\s\S]*?)"#;/)[1];
const css=fs.readFileSync('src/telemetry_dashboard/navigation_rail.css','utf8');
const source=fs.readFileSync('src/telemetry_dashboard/navigation_rail.rs','utf8');
const component=fs.readFileSync('src/telemetry_dashboard/dashboard_component.rs','utf8');
const selectionScript=source.match(/document::eval\("([^"]+)"\)/)[1];
assert(component.includes('navigation_rail::close_after_mobile_selection(rail_collapsed);'));
assert(component.includes('"aria-controls":"gs26-sidebar-navigation"'));
const browser=await chromium.launch({headless:true,executablePath:process.env.CHROME_BINARY||'/Applications/Google Chrome.app/Contents/MacOS/Google Chrome'});
try {
 const page=await browser.newPage();
 for(const [width,height] of [[320,568],[390,844],[844,390],[1280,800]]) {
  await page.setViewportSize({width,height});
  await page.setContent(`<style>${global}${css}</style><main id="main"><div class="gs26-dashboard-shell" data-navigation="sidebar" data-streamer="false" data-rail-collapsed="true" style="height:100dvh;box-sizing:border-box;overflow:hidden"><aside id="gs26-sidebar-navigation" class="gs26-navigation-rail" data-collapsed="true"><button class="gs26-rail-toggle" aria-label="Toggle desktop sidebar">‹</button><nav><div class="gs26-navigation-group"><p>Mission</p>${['Dashboard','Mission','Map','Checklists','Detailed info'].map(x=>`<a href="#"><svg viewBox="0 0 24 24"><path d="M3 3h18v18H3z"/></svg><span>${x}</span></a>`).join('')}</div></nav><footer><a href="#"><svg viewBox="0 0 24 24"><path d="M3 3h18v18H3z"/></svg><span>Settings</span></a></footer></aside><header class="gs26-header-row"><div class="gs26-header-title"><button class="gs26-mobile-sidebar-toggle" aria-controls="gs26-sidebar-navigation" aria-expanded="false">☰</button><span>Mission</span></div></header><section class="gs26-header-secondary">Network time</section><div class="gs26-page-content">Live content</div></div></main>`);
  await page.evaluate(script=>{
   const shell=document.querySelector('.gs26-dashboard-shell');const rail=document.querySelector('.gs26-navigation-rail');const toggle=document.querySelector('.gs26-mobile-sidebar-toggle');
   const set=hidden=>{shell.dataset.railCollapsed=String(hidden);rail.dataset.collapsed=String(hidden);toggle.setAttribute('aria-expanded',String(!hidden));};
   toggle.onclick=()=>set(shell.dataset.railCollapsed!=='true');
   document.querySelector('.gs26-rail-toggle').onclick=()=>set(shell.dataset.railCollapsed!=='true');
   for(const link of rail.querySelectorAll('a'))link.onclick=e=>{e.preventDefault();if(new Function(script)())set(true);};
  },selectionScript);
  const rail=page.locator('.gs26-navigation-rail'),content=page.locator('.gs26-page-content');
  const closedContent=await content.boundingBox();
  if(width<=900) {
   assert.equal(await rail.isVisible(),false,'mobile rail is fully hidden');
   await page.locator('.gs26-mobile-sidebar-toggle').click();
   assert(await rail.isVisible(),'mobile panel opens');
   const b=await rail.boundingBox(),c=await content.boundingBox();
   assert(b.width>=width-20&&c.width===closedContent.width,'open panel never squeezes content');
   assert(b.y+b.height<=c.y+1,'in-flow panel does not cover content');
   for(const label of ['Dashboard','Mission','Map','Checklists','Detailed info','Settings'])assert(await rail.locator('a').getByText(label,{exact:true}).isVisible(),'icons plus text: '+label);
   const labels=await rail.locator('a>span').evaluateAll(es=>es.map(e=>getComputedStyle(e).display));assert(labels.every(x=>x!=='none'));
   assert.equal(await rail.locator('svg').count(),6);
   assert(await rail.locator('a').first().evaluate(e=>e.getBoundingClientRect().height>=44));
   await rail.getByText('Map',{exact:true}).click();
   assert.equal(await rail.isVisible(),false,'selection closes mobile navigation');
   await page.locator('.gs26-mobile-sidebar-toggle').click();
   await page.locator('.gs26-mobile-sidebar-toggle').click();
   assert.equal(await rail.isVisible(),false,'toggle hides entire panel');
  } else {
   assert(await rail.isVisible(),'desktop compact rail remains available');
   assert.equal((await rail.boundingBox()).width,64);
   assert.equal(await rail.getByText('Dashboard',{exact:true}).isVisible(),false,'desktop compact labels hidden');
   await page.locator('.gs26-rail-toggle').click();
   assert.equal((await rail.boundingBox()).width,200);
   await rail.getByText('Map',{exact:true}).click();
   assert(await rail.getByText('Dashboard',{exact:true}).isVisible(),'desktop remains expanded after selection');
  }
  assert(await page.evaluate(()=>document.documentElement.scrollWidth<=innerWidth+1));
  console.log(JSON.stringify({width,height,contentWidth:closedContent.width}));
 }
 console.log('PASS mobile full-label/hidden navigation and desktop compact mode');
} finally {await browser.close();}
