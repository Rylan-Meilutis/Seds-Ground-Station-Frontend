// Production workspace/notice CSS geometry, no backend or hardware.
import fs from 'node:fs';
import assert from 'node:assert/strict';
const {chromium}=await import(process.env.PLAYWRIGHT_MODULE||'playwright');
const shell=fs.readFileSync('src/app_shell.rs','utf8');
const global=shell.match(/const GLOBAL_CSS: &str = r#"([\s\S]*?)"#;/)[1];
const css=['navigation_rail.css','controls.css'].map(name=>fs.readFileSync('src/telemetry_dashboard/'+name,'utf8')).join('\n');
const source=fs.readFileSync('src/telemetry_dashboard/dashboard_component.rs','utf8');
assert(source.includes('class:"gs26-connection-notice gs26-layout-notice"'),'notice is explicitly placed in workspace');
assert(!source.includes('recovery_marker::RecoveryMarker {}'),'recovery controls must not be global');
assert(!source.includes('recovery_status::RecoveryStatus {}'),'nominal recovery status must not occupy global header');
assert(source.includes('recovery_marker::RecoveryRuntime {}'),'background capability synchronization remains mounted');
assert(source.includes('"Change station"'),'native connection recovery retains a change-station action');
const actions=fs.readFileSync('src/telemetry_dashboard/actions_tab.rs','utf8');
assert(actions.includes('super::recovery_marker::RecoveryMarker {}'),'recovery marker belongs in Actions');
const browser=await chromium.launch({headless:true,executablePath:process.env.CHROME_BINARY||'/Applications/Google Chrome.app/Contents/MacOS/Google Chrome'});
try{
 const page=await browser.newPage();
 for(const width of [320,390,844,1280])for(const collapsed of ['true','false']){
  await page.setViewportSize({width,height:844});
  await page.setContent(`<style>${global}${css}</style><main id="main"><div class="gs26-dashboard-shell" data-streamer="false" data-navigation="sidebar" data-rail-collapsed="${collapsed}" style="height:100dvh;color:white;background:#0b1220"><aside class="gs26-navigation-rail" data-collapsed="${collapsed}"><nav>Pages</nav></aside><aside role="alert" class="gs26-connection-notice gs26-layout-notice" style="border-color:#994949;background:#301818;color:#fee2e2"><div class="gs26-layout-notice-body"><strong>Station connection</strong><p>Using a saved dashboard. The station layout could not be refreshed.</p><details><summary>Connection details</summary><pre></pre></details></div><div class="gs26-layout-notice-actions"><button>Reconnect now</button><button>Change station</button><button>Dismiss</button></div></aside><header class="gs26-header-row">Normal header</header><section class="gs26-header-secondary">Pinned data and clocks</section><div class="gs26-page-content">Page content</div></div></main>`);
  const note=page.locator('.gs26-layout-notice');const b=await note.boundingBox();
  assert(b.width>=Math.min(width-80,250),`readable content width ${width}/${collapsed}: ${b.width}`);
  assert(b.x>=0&&b.x+b.width<=width+1,'notice fits viewport');
  assert(await note.evaluate(e=>e.scrollWidth<=e.clientWidth+1),'notice has no horizontal overflow');
  assert(await page.evaluate(()=>document.documentElement.scrollWidth<=innerWidth+1),'workspace has no overflow');
  for(const button of await note.getByRole('button').all()){
   const box=await button.boundingBox();assert(box.height>=44,'touch height');assert(box.x>=b.x&&box.x+box.width<=b.x+b.width+1,'button stays inside notice');
  }
  await note.locator('pre').evaluate(e=>e.textContent='Backend unreachable '+('verylongunbrokenconnectiondiagnostic'.repeat(200)));
  await note.getByText('Connection details',{exact:true}).click();
  assert(await note.locator('pre').evaluate(e=>e.scrollWidth<=e.clientWidth+1&&e.clientHeight<=128),'technical diagnostics wrap and stay bounded');
  if(width<=390&&collapsed==='true')await page.screenshot({path:'/tmp/dashboard-connection-banner-'+width+'.png'});
 }
 console.log('PASS production CSS: cached-layout notice fits320/390/844/1280, both rail states, touch actions and bounded diagnostics; recovery controls scoped to Actions');
}finally{await browser.close();}
