// Native connection-page CSS geometry fixture; not a compiled iOS UI test.
import fs from 'node:fs';
import assert from 'node:assert/strict';
const {chromium}=await import(process.env.PLAYWRIGHT_MODULE||'playwright');
const source=fs.readFileSync('src/app_connect_page.rs','utf8');
const shell=fs.readFileSync('src/app_shell.rs','utf8');
const css=fs.readFileSync('src/connection_setup.css','utf8');
const global=shell.match(/const GLOBAL_CSS: &str = r#"([\s\S]*?)"#;/)[1];
const productionStyle=start=>source.split('\n').map(s=>s.trim()).find(s=>s.startsWith('"'+start)).slice(1).replace(/",$/,'').replaceAll('{}','#334155');
const pageStyle=productionStyle('min-height:var(--gs26-app-height)');
const addressStyle=productionStyle('position:relative; display:grid;');
const protocolStyle=productionStyle('position:relative; min-width:0; border-right:');
const inputStyle=productionStyle('width:100%; min-width:0; padding:12px; border:none;');
assert.match(source,/include_str!\("connection_setup.css"\)/);
assert.match(source,/use_effect\(move \|\| discover_stations\(\)\)/,'discovery begins without trusting or selecting hints');
const browser=await chromium.launch({headless:true,executablePath:process.env.CHROME_BINARY});
try {
 const page=await browser.newPage();
 for(const [width,height] of [[320,568],[390,844],[844,390]]) {
  await page.setViewportSize({width,height});
  await page.setContent(`<style>${global}${css}</style><main id="main"><div class="gs26-connect-screen" style="${pageStyle}"><div class="gs26-connect-card" style="width:min(760px,100%);padding:24px;border:1px solid #334155;border-radius:16px;display:flex;flex-direction:column;overflow:hidden;max-height:calc(var(--gs26-app-height) - 48px)"><div class="gs26-connect-header" style="display:flex;align-items:flex-start;justify-content:space-between;gap:12px;margin-bottom:12px"><h1 style="margin:0;font-size:20px">UBSEDS GS</h1><div style="display:flex;gap:10px;flex-wrap:wrap"><button>Settings</button><button>Version</button></div></div><div class="gs26-connect-body" style="flex:1 1 auto;min-height:0;overflow:auto;padding-right:4px"><p>Enter the station host and port.</p><button class="discovered-station">GroundStation · https://${'long-station-name-'.repeat(12)}.local</button><p role="status">Select a station, then Connect.</p><div class="gs26-station-address" style="${addressStyle}"><div style="${protocolStyle}"><button id="station-scheme" style="width:100%;height:100%;padding:0 8px">https ▾</button><div id="protocol-menu" hidden style="position:absolute;top:calc(100% + 6px);left:4px;right:4px;z-index:50"><button style="width:100%">https</button><button style="width:100%">http</button></div></div><input id="station-host" aria-label="Station host and port" style="${inputStyle}" value="192.168.3.7:443"></div></div><div class="gs26-connect-actions" style="display:flex;gap:12px;flex-wrap:wrap;margin-top:16px"><button>Explore offline</button><button>Test Connection</button><button>Connect</button></div></div></div></main>`);
  await page.addStyleTag({content:'button{min-height:44px;padding:10px 14px}'});
  const rect=await page.locator('.gs26-connect-card').boundingBox();
  assert(rect.x>=0&&rect.x+rect.width<=width+1,'card fits '+width);
  assert(rect.y>=0&&rect.y+rect.height<=height+1,'card fits height '+height);
  const row=await page.locator('.gs26-station-address').boundingBox();
  const input=await page.locator('#station-host').boundingBox();
  assert(input.width>=140,'host remains usable '+width);
  assert(input.x+input.width<=row.x+row.width+1,'host fits address row');
  assert(await page.evaluate(()=>document.documentElement.scrollWidth<=innerWidth),'no horizontal page scroll');
  await page.locator('#station-scheme').click();
  await page.locator('#protocol-menu').evaluate(e=>e.hidden=false);
  for(const name of ['http','https']) {
   const b=await page.locator('#protocol-menu').getByRole('button',{name,exact:true}).boundingBox();
   assert(b.height>=44&&b.x>=0&&b.x+b.width<=width,'protocol menu has accessible in-viewport targets');
  }
  console.log(JSON.stringify({width,height,card:rect,host:input}));
 }
 console.log('PASS native connection-screen geometry');
} finally {await browser.close();}
